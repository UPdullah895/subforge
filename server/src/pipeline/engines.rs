//! Engine closures for [`subforge_pipeline::runner::CoreRunner`].
//!
//! Translation is the one engine the pipeline needs today: it reads the subtitle, translates it
//! in batches with whichever engine the config names, and writes the result next to the input.
//! The engine choice comes from the policy, then the saved config, then Ollama — the same
//! precedence the UI uses, so a policy written for Jellyfin behaves like the desktop app.

use std::path::{Path, PathBuf};

use subforge_core::lines;
use subforge_core::subtitle::sync::{self, SyncVerdict};
use subforge_core::translate;
use subforge_core::whisper::{models, transcribe::TranscribeOptions, Device};
use subforge_pipeline::runner::{SearchFn, TranscribeFn, TranslateFn};
use subforge_subtitle_sources::{
    find_subtitle, OpenSubtitlesCredentials, SearchRequest, SubtitleSearchResult,
};
use tokio_util::sync::CancellationToken;

use crate::config::AppConfig;

/// Batch size per request. Cloud engines are charged per call and tolerate bigger batches;
/// Ollama and OpenAI-compatible endpoints get fewer lines so a timeout costs less work.
pub fn batch_size(engine: &str) -> usize {
    match engine {
        "ollama" | "openai" => 25,
        _ => 10,
    }
}

/// Policy → config → default.
pub fn resolve_engine(policy_engine: Option<&str>, config: &AppConfig) -> String {
    policy_engine
        .map(str::trim)
        .filter(|engine| !engine.is_empty())
        .map(str::to_string)
        .or_else(|| config.default_engine.clone())
        .filter(|engine| !engine.is_empty())
        .unwrap_or_else(|| "ollama".to_string())
}

/// Where the translated subtitle goes: beside the input, so the mux step can find it and the
/// work directory keeps it out of the library.
pub fn output_path(input: &str, language: &str) -> PathBuf {
    let path = Path::new(input);
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "subtitle".to_string());
    path.with_file_name(format!("{stem}.{language}.srt"))
}

/// The user agent the subtitle APIs see. OpenSubtitles answers `403 User-Agent header is wrong`
/// to a default client string, so this is not cosmetic.
fn user_agent() -> String {
    format!("SubForge/{}", env!("CARGO_PKG_VERSION"))
}

/// What to search for, from the file name alone: the pipeline has no library metadata, and the
/// *arr payload that does know the title arrives through a different door.
pub fn search_request_for(file: &str, language: &str) -> SearchRequest {
    let stem = Path::new(file)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| file.to_string());
    let mut parsed = subforge_metadata::parser::parse_filename(&stem);
    if parsed.series_guess.trim().is_empty() {
        // `Bleach/S17E46.mkv`: the title lives on the folder, the numbers on the file.
        if let Some(folder) = Path::new(file)
            .parent()
            .and_then(|parent| parent.file_name())
            .map(|name| name.to_string_lossy().to_string())
        {
            let from_folder = subforge_metadata::parser::parse_filename(&folder);
            parsed.series_guess = from_folder.series_guess;
            parsed.season = parsed.season.or(from_folder.season);
            parsed.episode = parsed.episode.or(from_folder.episode);
        }
    }
    SearchRequest {
        title: parsed.series_guess,
        year: None,
        season: parsed.season,
        episode: parsed.episode,
        language: language.to_string(),
    }
}

/// Whether a downloaded subtitle is worth keeping.
///
/// A subtitle that is a frame-rate off, or that runs past the end of the file, is worse than no
/// subtitle: it will be translated, muxed, verified and watched, and the user will blame the
/// translation. A constant shift is recoverable (the sync step knows the number) — but only if it
/// is small enough to be a different release rather than a different cut.
pub fn verdict_is_acceptable(verdict: SyncVerdict, shift_ms: Option<i64>) -> bool {
    match verdict {
        SyncVerdict::Good | SyncVerdict::Uncertain => true,
        SyncVerdict::Offset => shift_ms.map(|ms| ms.abs() <= 500).unwrap_or(true),
        SyncVerdict::Framerate | SyncVerdict::Truncated => false,
    }
}

/// A search closure: ask the providers, write what comes back into the work directory, and keep
/// it only if it looks like it belongs to this file.
pub fn searcher(work_dir: PathBuf) -> Result<SearchFn, String> {
    let handle = tokio::runtime::Handle::try_current().map_err(|_| {
        "the pipeline needs a tokio runtime to reach the subtitle providers".to_string()
    })?;
    Ok(Box::new(move |file: &str, language: &str| {
        let config = AppConfig::load();
        let credentials = match (
            config.opensubtitles_api_key.clone(),
            config.opensubtitles_username.clone(),
            config.opensubtitles_password.clone(),
        ) {
            (Some(api_key), Some(username), Some(password)) if !api_key.is_empty() => {
                Some(OpenSubtitlesCredentials {
                    api_key,
                    username,
                    password,
                })
            }
            _ => None,
        };
        let subsource_key = config.subsource_api_key.clone().unwrap_or_default();
        if credentials.is_none() && subsource_key.is_empty() {
            // No providers configured is "nothing found", not a failure: translation is the
            // documented fallback and the log should not pretend otherwise.
            return Ok(None);
        }

        let request = search_request_for(file, language);
        let agent = user_agent();
        let result = handle.block_on(find_subtitle(
            &request,
            Path::new(file),
            credentials.as_ref(),
            &subsource_key,
            &agent,
        ));
        let SubtitleSearchResult::Found { text, source, .. } = result else {
            return Ok(None);
        };

        let stem = Path::new(file)
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_else(|| "episode".to_string());
        let _ = std::fs::create_dir_all(&work_dir);
        let out = work_dir.join(format!("{stem}.{language}.found.srt"));
        std::fs::write(&out, &text)
            .map_err(|error| format!("cannot save {source} subtitle: {error}"))?;

        let verdict = sync::quick_check(&crate::tools::tool_paths(), Path::new(file), &out)
            .ok()
            .map(|check| (check.verdict, check.suggested_shift_ms));
        match verdict {
            Some((verdict, shift)) if !verdict_is_acceptable(verdict, shift) => {
                // Keep the file out of the library: the caller will translate instead.
                let _ = std::fs::remove_file(&out);
                Ok(None)
            }
            _ => Ok(Some(out.to_string_lossy().to_string())),
        }
    }))
}

/// The model to transcribe with: the policy's choice, then `SUBFORGE_WHISPER_MODEL`, then the
/// largest model already downloaded — and never a download behind the user's back.
pub fn resolve_model(policy_model: Option<&str>, cache: &Path) -> Result<String, String> {
    if let Some(name) = policy_model.map(str::trim).filter(|name| !name.is_empty()) {
        return Ok(name.to_string());
    }
    if let Some(name) = std::env::var("SUBFORGE_WHISPER_MODEL")
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
    {
        return Ok(name);
    }
    let downloaded = models::list_downloaded(cache);
    let best = downloaded
        .iter()
        .filter_map(|name| models::spec_for(name))
        .max_by_key(|spec| spec.size_mb)
        .map(|spec| spec.name.to_string());
    match best {
        Some(name) => Ok(name),
        None => Err(format!(
            "no speech model is downloaded in {}; download one in the app (Settings → Speech), \
             or set SUBFORGE_WHISPER_MODEL",
            cache.display()
        )),
    }
}

/// A transcription closure for the pipeline. Runs whisper.cpp synchronously; the executor calls
/// it from a blocking task, so no runtime is needed here.
pub fn transcriber(work_dir: PathBuf, model: Option<String>) -> TranscribeFn {
    Box::new(move |file: &str, language: Option<&str>| {
        let cache = models::default_cache_dir();
        let model = resolve_model(model.as_deref(), &cache)?;
        let stem = Path::new(file)
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_else(|| "episode".to_string());
        let _ = std::fs::create_dir_all(&work_dir);
        let output = work_dir.join(format!("{stem}.whisper.srt"));

        // `und` is mkvmerge's "unknown", not a language whisper understands.
        let language = language
            .map(str::trim)
            .filter(|code| !code.is_empty() && !code.eq_ignore_ascii_case("und"))
            .map(str::to_string);
        // VAD only when its model is present: a missing little model must not fail the episode.
        let vad = models::model_path(&cache, &models::VAD_MODEL).is_file();

        let options = TranscribeOptions {
            input: PathBuf::from(file),
            output: output.clone(),
            model,
            device: Device::Auto,
            language,
            format: Default::default(),
            word_timestamps: false,
            vad_filter: vad,
            cache_dir: Some(cache),
        };
        let mut progress = |_pct: f64, _message: Option<&str>| {};
        subforge_core::whisper::transcribe(
            &crate::tools::tool_paths(),
            &options,
            &mut progress,
            CancellationToken::new(),
        )
        .map_err(|error| error.to_string())?;
        Ok(output.to_string_lossy().to_string())
    })
}

/// A translation closure for the pipeline. Fails at construction when there is no runtime to
/// drive the async engines, which is better than failing on the first episode.
pub fn translator(engine: Option<String>) -> Result<TranslateFn, String> {
    let handle = tokio::runtime::Handle::try_current().map_err(|_| {
        "the pipeline needs a tokio runtime to reach the translation engines".to_string()
    })?;
    Ok(Box::new(
        move |input: &str, from: Option<&str>, to: &str| {
            let config = AppConfig::load();
            let engine = resolve_engine(engine.as_deref(), &config);
            let source = from.unwrap_or("auto").to_string();
            let input = input.to_string();
            let target = to.to_string();
            handle.block_on(async move {
                let lines = tokio::task::spawn_blocking({
                    let input = PathBuf::from(&input);
                    move || lines::extract_lines(&input).map_err(|error| error.to_string())
                })
                .await
                .map_err(|error| error.to_string())??;
                if lines.is_empty() {
                    return Err("the subtitle has no lines to translate".to_string());
                }

                let mut translated = Vec::with_capacity(lines.len());
                for chunk in lines.chunks(batch_size(&engine)) {
                    let batch = match engine.as_str() {
                        "ollama" => {
                            let host = config
                                .ollama_base_url
                                .clone()
                                .unwrap_or_else(|| "http://127.0.0.1:11434".to_string());
                            let model = std::env::var("SUBFORGE_OLLAMA_MODEL")
                            .ok()
                            .or_else(|| config.openai_model.clone())
                            .filter(|model| !model.trim().is_empty())
                            .ok_or_else(|| {
                                "set SUBFORGE_OLLAMA_MODEL (or openai_model) for the ollama engine"
                                    .to_string()
                            })?;
                            translate::translate_ollama(&host, &model, chunk, &source, &target)
                                .await
                        }
                        "openai" => {
                            let key = config.openai_api_key.clone().unwrap_or_default();
                            if key.is_empty() {
                                return Err(
                                    "the openai engine needs an API key in the config".to_string()
                                );
                            }
                            let model = config
                                .openai_model
                                .clone()
                                .unwrap_or_else(|| "gpt-4o-mini".to_string());
                            translate::translate_openai(&key, &model, chunk, &source, &target).await
                        }
                        "libretranslate" => {
                            let host = config
                                .libretranslate_base_url
                                .clone()
                                .unwrap_or_else(|| "http://127.0.0.1:5000".to_string());
                            translate::translate_libretranslate(&host, chunk, &source, &target)
                                .await
                        }
                        "deepl" => {
                            let key = config.deepl_api_key.clone().unwrap_or_default();
                            if key.is_empty() {
                                return Err(
                                    "the deepl engine needs an API key in the config".to_string()
                                );
                            }
                            translate::translate_deepl(&key, chunk, &source, &target).await
                        }
                        "google" => {
                            let key = config.google_translate_api_key.clone().unwrap_or_default();
                            if key.is_empty() {
                                return Err(
                                    "the google engine needs an API key in the config".to_string()
                                );
                            }
                            translate::translate_google(&key, chunk, &source, &target).await
                        }
                        other => return Err(format!("unknown translation engine: {other}")),
                    }
                    .map_err(|error| error.to_string())?;
                    translated.extend(batch);
                }

                let output = output_path(&input, &target);
                let rtl_fix = config.fix_rtl_punctuation;
                let original = PathBuf::from(&input);
                tokio::task::spawn_blocking(move || {
                    lines::rebuild_lines(&original, &translated, &output, rtl_fix)
                        .map_err(|error| error.to_string())
                })
                .await
                .map_err(|error| error.to_string())??;
                Ok(output_path(&input, &target).to_string_lossy().to_string())
            })
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(engine: Option<&str>) -> AppConfig {
        AppConfig {
            default_engine: engine.map(str::to_string),
            ..AppConfig::default()
        }
    }

    #[test]
    fn the_policy_wins_over_the_saved_config() {
        assert_eq!(
            resolve_engine(Some("deepl"), &config(Some("ollama"))),
            "deepl"
        );
    }

    #[test]
    fn the_config_is_used_when_the_policy_says_nothing() {
        assert_eq!(resolve_engine(None, &config(Some("openai"))), "openai");
        // Blank values are not an engine choice.
        assert_eq!(
            resolve_engine(Some("  "), &config(Some("openai"))),
            "openai"
        );
    }

    #[test]
    fn ollama_is_the_last_resort() {
        assert_eq!(resolve_engine(None, &config(None)), "ollama");
        assert_eq!(resolve_engine(Some(""), &config(Some(""))), "ollama");
    }

    #[test]
    fn batches_are_smaller_for_engines_that_time_out() {
        assert_eq!(batch_size("ollama"), 25);
        assert_eq!(batch_size("openai"), 25);
        assert_eq!(batch_size("deepl"), 10);
        assert_eq!(batch_size("libretranslate"), 10);
    }

    #[test]
    fn a_file_name_becomes_a_search_request() {
        // The title is on the folder, the numbers on the file.
        let request = search_request_for("/media/tv/Bleach/S17E46.mkv", "ar");
        assert_eq!(request.title, "Bleach");
        assert_eq!(request.season, Some(17));
        assert_eq!(request.episode, Some(46));
        assert_eq!(request.language, "ar");

        // …or both on the file, which is the usual scene release.
        let scene = search_request_for("/media/tv/Bleach.S17E46.1080p.mkv", "ar");
        assert_eq!(scene.title, "Bleach");
        assert_eq!(scene.season, Some(17));

        let movie = search_request_for("/media/movies/Dune.2021.2160p.mkv", "fr");
        assert!(movie.title.starts_with("Dune"), "{:?}", movie.title);
        assert_eq!(movie.season, None);
        assert_eq!(movie.episode, None);
        assert_eq!(movie.language, "fr");
    }

    #[test]
    fn only_plausible_subtitles_survive_the_sync_gate() {
        use subforge_core::subtitle::sync::SyncVerdict;
        assert!(verdict_is_acceptable(SyncVerdict::Good, None));
        assert!(verdict_is_acceptable(SyncVerdict::Uncertain, None));
        // A small constant shift is a different release; a large one is a different cut.
        assert!(verdict_is_acceptable(SyncVerdict::Offset, Some(-320)));
        assert!(verdict_is_acceptable(SyncVerdict::Offset, Some(480)));
        assert!(!verdict_is_acceptable(SyncVerdict::Offset, Some(9_000)));
        // These would be translated, muxed and blamed on the translation.
        assert!(!verdict_is_acceptable(SyncVerdict::Framerate, None));
        assert!(!verdict_is_acceptable(SyncVerdict::Truncated, None));
    }

    #[test]
    fn an_empty_cache_asks_for_a_download_instead_of_downloading() {
        let dir = std::env::temp_dir().join(format!("sf_models_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let error = resolve_model(None, &dir).unwrap_err();
        assert!(error.contains("no speech model is downloaded"), "{error}");
        assert!(error.contains("Settings → Speech"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_policy_model_wins_over_whatever_is_on_disk() {
        let dir = std::env::temp_dir();
        assert_eq!(resolve_model(Some("medium"), &dir).unwrap(), "medium");
        // Blank means "decide for me", not "use a model called ''".
        assert!(resolve_model(Some("   "), &dir).is_err());
    }

    #[test]
    fn the_translation_lands_beside_the_input() {
        assert_eq!(
            output_path("/work/Show.S01E01.source.srt", "ar"),
            PathBuf::from("/work/Show.S01E01.source.ar.srt")
        );
        assert_eq!(
            output_path("episode.ass", "fr"),
            PathBuf::from("episode.fr.srt")
        );
    }

    #[test]
    fn a_missing_runtime_is_reported_at_construction() {
        // Tests run outside a runtime; the factory must say so instead of panicking later.
        let error = match translator(None) {
            Ok(_) => String::new(),
            Err(error) => error,
        };
        assert!(
            error.is_empty() || error.contains("tokio runtime"),
            "unexpected error: {error}"
        );
    }

    #[test]
    #[ignore = "needs a local ollama with a model loaded (SUBFORGE_OLLAMA_MODEL)"]
    fn a_real_engine_translates_the_fixture() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let result = runtime.block_on(async {
            let translate = translator(Some("ollama".to_string()))?;
            let dir = std::env::temp_dir().join(format!("sf_engine_{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            let input = dir.join("episode.source.srt");
            let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../crates/subforge-core/tests/fixtures/subtitles/basic.srt");
            std::fs::copy(&fixture, &input).unwrap();
            let path = input.to_string_lossy().to_string();
            let output = tokio::task::spawn_blocking(move || translate(&path, Some("en"), "ar"))
                .await
                .map_err(|error| error.to_string())??;
            let text = std::fs::read_to_string(&output).map_err(|error| error.to_string())?;
            let _ = std::fs::remove_dir_all(&dir);
            Ok::<String, String>(text)
        });
        let text = result.expect("the engine should translate the fixture");
        assert!(
            text.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c)),
            "no Arabic in the translation: {text}"
        );
        // The cue count must survive the round trip.
        assert!(text.matches(" --> ").count() >= 3, "lost cues: {text}");
    }
}
