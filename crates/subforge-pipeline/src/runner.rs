//! The real [`StepRunner`]: mux, verify, extract and clean up through `subforge-core`.
//!
//! The three steps that need an engine (search, transcribe, translate) are injected as
//! closures, because they depend on configuration the pipeline crate must not own: API keys,
//! a whisper model, a translation engine. The rest is deterministic and lives here.
//!
//! Publishing is atomic: a mux writes a temporary file next to the episode, the original is
//! moved aside, the temporary takes its place, and the verify step either deletes the backup
//! (success) or restores it (failure). An episode is never left half-rewritten.

use std::path::{Path, PathBuf};

use subforge_core::tools::mkvtoolnix::{self, SubtitleMergeSpec};
use subforge_core::tools::ToolPaths;

use crate::exec::StepRunner;
use crate::language::normalize_language;

pub type SearchFn = Box<dyn Fn(&str, &str) -> Result<Option<String>, String> + Send>;
pub type TranscribeFn = Box<dyn Fn(&str, Option<&str>) -> Result<String, String> + Send>;
pub type TranslateFn = Box<dyn Fn(&str, Option<&str>, &str) -> Result<String, String> + Send>;

/// Runs the plan's steps for real. Intermediates go to `work_dir`; the episodes themselves
/// are only touched by the atomic publish inside [`StepRunner::mux`].
pub struct CoreRunner {
    paths: ToolPaths,
    work_dir: PathBuf,
    search: Option<SearchFn>,
    transcribe: Option<TranscribeFn>,
    translate: Option<TranslateFn>,
    /// Set while a mux is published but not yet verified.
    backup: Option<PathBuf>,
}

impl CoreRunner {
    pub fn new(paths: ToolPaths, work_dir: impl Into<PathBuf>) -> Self {
        Self {
            paths,
            work_dir: work_dir.into(),
            search: None,
            transcribe: None,
            translate: None,
            backup: None,
        }
    }

    pub fn with_search(mut self, search: SearchFn) -> Self {
        self.search = Some(search);
        self
    }

    pub fn with_transcribe(mut self, transcribe: TranscribeFn) -> Self {
        self.transcribe = Some(transcribe);
        self
    }

    pub fn with_translate(mut self, translate: TranslateFn) -> Self {
        self.translate = Some(translate);
        self
    }

    fn scratch(&self, stem: &str, suffix: &str) -> PathBuf {
        let _ = std::fs::create_dir_all(&self.work_dir);
        self.work_dir.join(format!("{stem}.{suffix}"))
    }

    fn subtitle_track(
        &self,
        file: &str,
        language: Option<&str>,
        want: bool,
    ) -> Result<(u64, String), String> {
        let info = mkvtoolnix::enumerate_tracks(&self.paths, file).map_err(|e| e.to_string())?;
        let wanted = language.map(normalize_language).unwrap_or_default();
        let tracks: Vec<_> = info
            .tracks
            .iter()
            .filter(|track| track.track_type == "subtitles")
            .collect();
        if tracks.is_empty() {
            return Err(format!("{file} has no subtitle track"));
        }
        let picked = if want && !wanted.is_empty() {
            tracks
                .iter()
                .find(|track| {
                    track
                        .properties
                        .as_ref()
                        .and_then(|props| props.language.clone())
                        .map(|language| normalize_language(&language) == wanted)
                        .unwrap_or(false)
                })
                .copied()
        } else {
            None
        }
        .or_else(|| tracks.first().copied())
        .ok_or_else(|| format!("{file} has no subtitle track"))?;

        let codec = picked
            .properties
            .as_ref()
            .and_then(|props| props.codec_id.clone())
            .unwrap_or_default()
            .to_ascii_uppercase();
        let extension = if codec.contains("ASS") || codec.contains("SSA") {
            "ass"
        } else if codec.contains("VTT") {
            "vtt"
        } else {
            "srt"
        };
        Ok((picked.id, extension.to_string()))
    }

    fn restore_backup(&mut self, file: &str) {
        if let Some(backup) = self.backup.take() {
            let _ = std::fs::rename(&backup, file);
        }
    }
}

fn stem_of(file: &str) -> String {
    Path::new(file)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "episode".to_string())
}

impl StepRunner for CoreRunner {
    fn search_online(&mut self, file: &str, language: &str) -> Result<Option<String>, String> {
        match &self.search {
            Some(search) => search(file, language),
            None => Ok(None),
        }
    }

    fn transcribe(&mut self, file: &str, language: Option<&str>) -> Result<String, String> {
        match &self.transcribe {
            Some(transcribe) => transcribe(file, language),
            None => Err("transcription engine is not configured".to_string()),
        }
    }

    fn extract_existing(&mut self, file: &str, language: Option<&str>) -> Result<String, String> {
        let (track_id, extension) = self.subtitle_track(file, language, true)?;
        let out = self.scratch(&stem_of(file), &format!("source.{extension}"));
        let out_str = out.to_string_lossy().to_string();
        mkvtoolnix::extract_subtitle_track(&self.paths, file, track_id, &out_str)
            .map_err(|e| e.to_string())?;
        Ok(out_str)
    }

    fn translate(
        &mut self,
        subtitle: &str,
        from: Option<&str>,
        to: &str,
    ) -> Result<String, String> {
        match &self.translate {
            Some(translate) => translate(subtitle, from, to),
            None => Err("translation engine is not configured".to_string()),
        }
    }

    fn mux(&mut self, file: &str, subtitle: &str, language: &str) -> Result<String, String> {
        let temporary = self.scratch(&format!("{}.muxed", stem_of(file)), "mkv");
        let _ = std::fs::remove_file(&temporary);
        let spec = SubtitleMergeSpec {
            path: subtitle.to_string(),
            language: Some(normalize_language(language)),
            track_name: None,
            hearing_impaired: false,
            make_default: false,
        };
        mkvtoolnix::merge_subtitles(&self.paths, file, &[spec], &temporary.to_string_lossy())
            .map_err(|e| e.to_string())?;

        // Publish: original aside, mux in place. Verify decides whether the backup survives.
        let backup = PathBuf::from(format!("{file}.subforge-bak"));
        let _ = std::fs::remove_file(&backup);
        std::fs::rename(file, &backup)
            .map_err(|e| format!("cannot move the original aside: {e}"))?;
        if let Err(error) = std::fs::rename(&temporary, file) {
            // Put the original back before reporting anything.
            let _ = std::fs::rename(&backup, file);
            return Err(format!("cannot publish the muxed file: {error}"));
        }
        self.backup = Some(backup);
        Ok(file.to_string())
    }

    fn verify(&mut self, file: &str, language: &str) -> Result<(), String> {
        let wanted = normalize_language(language);
        let info = mkvtoolnix::enumerate_tracks(&self.paths, file).map_err(|e| e.to_string());
        let info = match info {
            Ok(info) => info,
            Err(error) => {
                self.restore_backup(file);
                return Err(format!("the muxed file cannot be read back: {error}"));
            }
        };
        let present = info.tracks.iter().any(|track| {
            track.track_type == "subtitles"
                && track
                    .properties
                    .as_ref()
                    .and_then(|props| props.language.clone())
                    .map(|have| normalize_language(&have) == wanted)
                    .unwrap_or(false)
        });
        if !present {
            self.restore_backup(file);
            return Err(format!("{wanted} is not in the muxed file"));
        }
        // The track has to carry something, not just a header.
        let size = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
        if size < 1024 {
            self.restore_backup(file);
            return Err("the muxed file is suspiciously small".to_string());
        }
        if let Some(backup) = self.backup.take() {
            let _ = std::fs::remove_file(backup);
        }
        Ok(())
    }

    fn cleanup(&mut self, subtitle: &str) -> Result<(), String> {
        match std::fs::remove_file(subtitle) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("cannot remove {subtitle}: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::{execute, Outcome};
    use crate::{plan, Inventory, Policy};

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../subforge-core/tests/fixtures")
            .join(name)
    }

    /// `None` when no mkvmerge is reachable, so CI without MKVToolNix skips instead of failing.
    fn tool_paths() -> Option<ToolPaths> {
        let bundled = std::env::var("SUBFORGE_BUNDLED_DIR")
            .ok()
            .map(PathBuf::from)
            .filter(|dir| dir.is_dir());
        let paths = ToolPaths {
            custom: Default::default(),
            bundled_dir: bundled,
        };
        mkvtoolnix::enumerate_tracks(&paths, &fixture("media/sample.mkv").to_string_lossy())
            .ok()
            .map(|_| paths)
    }

    struct Workspace {
        dir: PathBuf,
    }

    impl Workspace {
        fn new() -> Self {
            let dir =
                std::env::temp_dir().join(format!("subforge_runner_{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::copy(fixture("media/sample.mkv"), dir.join("episode.mkv")).unwrap();
            std::fs::copy(fixture("subtitles/basic.srt"), dir.join("human.srt")).unwrap();
            Self { dir }
        }
        fn episode(&self) -> String {
            self.dir.join("episode.mkv").to_string_lossy().to_string()
        }
    }

    impl Drop for Workspace {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn policy() -> Policy {
        Policy {
            target_languages: vec!["ar".to_string()],
            prefer_human_subtitle: true,
            delete_source_after_mux: true,
            dry_run: false,
            translate_engine: None,
        }
    }

    #[test]
    fn muxes_verifies_and_publishes_atomically() {
        let Some(paths) = tool_paths() else {
            eprintln!("skipping: no mkvmerge available");
            return;
        };
        let workspace = Workspace::new();
        let episode = workspace.episode();
        let srt = workspace
            .dir
            .join("human.srt")
            .to_string_lossy()
            .to_string();

        let inventory = Inventory {
            subtitle: vec!["en".to_string()],
            audio: vec!["und".to_string()],
        };
        let plan = plan(&episode, &inventory, &policy());
        assert!(plan.missing.contains(&"ar".to_string()));

        let srt_for_search = srt.clone();
        let mut runner = CoreRunner::new(paths, workspace.dir.join("work")).with_search(Box::new(
            move |_file, _language| Ok(Some(srt_for_search.clone())),
        ));

        let report = execute(&plan, &mut runner);
        assert!(report.ok, "steps failed: {:?}", report.results);

        // The episode now carries Arabic…
        let info = mkvtoolnix::enumerate_tracks(&runner.paths, &episode).unwrap();
        let arabic = info.tracks.iter().any(|track| {
            track.track_type == "subtitles"
                && track
                    .properties
                    .as_ref()
                    .and_then(|props| props.language.clone())
                    .map(|lang| normalize_language(&lang) == "ar")
                    .unwrap_or(false)
        });
        assert!(arabic, "the muxed file has no Arabic subtitle");

        // …the source subtitle is gone (policy asked for it)…
        assert!(
            !Path::new(&srt).exists(),
            "the source subtitle was not removed"
        );
        // …and no backup or temporary file was left behind.
        assert!(!Path::new(&format!("{episode}.subforge-bak")).exists());
        let leftovers: Vec<_> = std::fs::read_dir(&workspace.dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.contains("subforge-bak") || name.contains("muxed"))
            .collect();
        assert!(leftovers.is_empty(), "leftovers: {leftovers:?}");
    }

    #[test]
    fn a_wrong_language_track_is_not_mistaken_for_success() {
        let Some(paths) = tool_paths() else {
            eprintln!("skipping: no mkvmerge available");
            return;
        };
        let workspace = Workspace::new();
        let episode = workspace.episode();
        let mut runner = CoreRunner::new(paths, workspace.dir.join("work"));

        // Verify on the original file: it carries ara + eng but no French, so this must fail
        // and must not leave a backup behind.
        let error = runner.verify(&episode, "fr").unwrap_err();
        assert!(error.contains("fr"));
        assert!(!Path::new(&format!("{episode}.subforge-bak")).exists());

        let info = mkvtoolnix::enumerate_tracks(&runner.paths, &episode).unwrap();
        assert_eq!(
            info.tracks.len(),
            4,
            "the episode was modified by a failed verify"
        );
    }

    #[test]
    fn a_failed_verify_restores_the_original() {
        let Some(paths) = tool_paths() else {
            eprintln!("skipping: no mkvmerge available");
            return;
        };
        let workspace = Workspace::new();
        let episode = workspace.episode();
        let srt = workspace
            .dir
            .join("human.srt")
            .to_string_lossy()
            .to_string();
        let before = std::fs::read(&episode).unwrap();

        let mut runner = CoreRunner::new(paths, workspace.dir.join("work"));
        // Publish the mux…
        runner.mux(&episode, &srt, "ar").unwrap();
        assert!(std::fs::read(&episode).unwrap() != before);
        // …then force a verify failure by asking for a language that mux did not add.
        let error = runner.verify(&episode, "fr").unwrap_err();
        assert!(error.contains("fr"));
        // The original is back, byte for byte, and no backup was left behind.
        assert_eq!(std::fs::read(&episode).unwrap(), before);
        assert!(!Path::new(&format!("{episode}.subforge-bak")).exists());
    }

    #[test]
    fn extracting_a_missing_subtitle_is_an_error_not_a_panic() {
        let Some(paths) = tool_paths() else {
            eprintln!("skipping: no mkvmerge available");
            return;
        };
        let workspace = Workspace::new();
        let mut runner = CoreRunner::new(paths, workspace.dir.join("work"));
        // sample.mkv has subtitles, so asking for a language that is not there falls back to
        // the first track; a file with no subtitle track at all is the error case.
        let audio_only = workspace.dir.join("audio-only.mkv");
        std::fs::write(&audio_only, b"not really a matroska file").unwrap();
        let error = runner
            .extract_existing(&audio_only.to_string_lossy(), Some("ar"))
            .unwrap_err();
        assert!(!error.is_empty());
    }

    #[test]
    fn engines_are_optional_and_say_so() {
        let runner_paths = ToolPaths::default();
        let mut runner = CoreRunner::new(runner_paths, std::env::temp_dir());
        let error = runner.transcribe("/x.mkv", Some("ja")).unwrap_err();
        assert!(error.contains("not configured"));
        let error = runner.translate("/x.srt", None, "ar").unwrap_err();
        assert!(error.contains("not configured"));
        // Search is optional in a different way: no providers configured means "nothing found".
        assert_eq!(runner.search_online("/x.mkv", "ar").unwrap(), None);
    }

    #[test]
    fn outcomes_report_what_happened() {
        let Some(paths) = tool_paths() else {
            eprintln!("skipping: no mkvmerge available");
            return;
        };
        let workspace = Workspace::new();
        let episode = workspace.episode();
        let srt = workspace
            .dir
            .join("human.srt")
            .to_string_lossy()
            .to_string();
        let srt_for_search = srt.clone();
        let mut runner = CoreRunner::new(paths, workspace.dir.join("work"))
            .with_search(Box::new(move |_, _| Ok(Some(srt_for_search.clone()))));

        let inventory = Inventory {
            subtitle: vec!["en".to_string()],
            audio: vec!["und".to_string()],
        };
        let plan = plan(&episode, &inventory, &policy());
        let report = execute(&plan, &mut runner);

        // Search found a human subtitle, so the fallback translation is reported as skipped.
        let translate = report
            .results
            .iter()
            .find(|result| matches!(result.step, crate::Step::Translate { .. }))
            .expect("the plan has a translate step");
        assert!(matches!(translate.outcome, Outcome::Skipped(_)));
        assert!(report.ok);
    }
}
