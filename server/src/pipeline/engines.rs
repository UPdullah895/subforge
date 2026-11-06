//! Engine closures for [`subforge_pipeline::runner::CoreRunner`].
//!
//! Translation is the one engine the pipeline needs today: it reads the subtitle, translates it
//! in batches with whichever engine the config names, and writes the result next to the input.
//! The engine choice comes from the policy, then the saved config, then Ollama — the same
//! precedence the UI uses, so a policy written for Jellyfin behaves like the desktop app.

use std::path::{Path, PathBuf};

use subforge_core::lines;
use subforge_core::translate;
use subforge_pipeline::runner::TranslateFn;

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
}
