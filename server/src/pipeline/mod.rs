//! Library automation: the policy file, the folder watcher and the dry run.
//!
//! This is the server-side shell around `subforge-pipeline`: it decides *which* files to look
//! at, loads the policy its owner wrote, and turns a file into a plan. Execution is gated and
//! arrives with the engine wiring; until then a policy that asks for `dry_run: false` is
//! refused rather than quietly half-applied.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use subforge_pipeline::Policy;

pub mod arr;
pub mod engines;
pub mod hooks;
pub mod run;
pub mod watch;

/// `pipeline.json` in the config directory (or `SUBFORGE_PIPELINE_CONFIG`). Missing means the
/// feature is off, which is the safe default for a server someone else may be watching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    /// Folders whose episodes should be processed. Empty disables the watcher.
    #[serde(default)]
    pub watch_dirs: Vec<String>,
    /// Descend into subfolders (`Series/Season 01/`).
    #[serde(default = "default_true")]
    pub recursive: bool,
    /// The policy the watcher applies.
    #[serde(default)]
    pub policy: Policy,
    /// Sonarr/Radarr instances to follow. Empty means "watch folders only".
    #[serde(default)]
    pub arr: Vec<arr::ArrEndpoint>,
}

fn default_true() -> bool {
    true
}

impl Default for PipelineConfig {
    fn default() -> Self {
        // `derive(Default)` would set `recursive` to false; the documented default is true, so
        // a config file that omits the field and a config that is absent behave the same.
        Self {
            watch_dirs: Vec::new(),
            recursive: true,
            policy: Policy::default(),
            arr: Vec::new(),
        }
    }
}

impl PipelineConfig {
    pub fn path() -> Option<PathBuf> {
        if let Some(custom) = std::env::var_os("SUBFORGE_PIPELINE_CONFIG").filter(|v| !v.is_empty())
        {
            return Some(PathBuf::from(custom));
        }
        crate::config::AppConfig::config_dir()
            .ok()
            .map(|dir| dir.join("pipeline.json"))
    }

    /// Reads the file; a missing or unreadable file is "feature off", not an error, so a
    /// container without a policy still starts.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|error| {
                eprintln!(
                    "[pipeline] {} is not valid JSON ({error}); automation stays off",
                    path.display()
                );
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn enabled(&self) -> bool {
        !self.watch_dirs.is_empty() && !self.policy.target_languages.is_empty()
    }
}

/// Video extensions the watcher reacts to. Sidecars (`.srt`, `.nfo`, images) are ignored, and
/// so are partial downloads.
pub fn is_episode_candidate(path: &std::path::Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    // A download client writes `episode.mkv.part`, `episode.mkv.tmp`, `.!qB` sidecars, ...
    let lowered = name.to_ascii_lowercase();
    if lowered.ends_with(".part")
        || lowered.ends_with(".tmp")
        || lowered.ends_with(".crdownload")
        || lowered.ends_with(".!qb")
        || lowered.starts_with('.')
    {
        return false;
    }
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("mkv" | "mp4" | "m4v" | "avi" | "mov" | "ts" | "webm")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_config_means_off() {
        let config = PipelineConfig::default();
        assert!(!config.enabled());
        assert!(config.recursive);
    }

    #[test]
    fn a_policy_without_languages_does_not_enable_the_watcher() {
        let config = PipelineConfig {
            watch_dirs: vec!["/media/tv".to_string()],
            policy: Policy::default(),
            ..PipelineConfig::default()
        };
        assert!(!config.enabled());
    }

    #[test]
    fn a_policy_with_languages_and_a_folder_enables_it() {
        let config = PipelineConfig {
            watch_dirs: vec!["/media/tv".to_string()],
            policy: Policy::arabic(),
            ..PipelineConfig::default()
        };
        assert!(config.enabled());
    }

    #[test]
    fn only_finished_video_files_are_candidates() {
        let candidate = |name: &str| is_episode_candidate(std::path::Path::new(name));
        assert!(candidate("Show.S01E01.mkv"));
        assert!(candidate("/media/tv/Show/S01E01.MP4"));
        assert!(candidate("episode.webm"));

        assert!(!candidate("Show.S01E01.mkv.part"));
        assert!(!candidate("Show.S01E01.mkv.tmp"));
        assert!(!candidate("Show.S01E01.mkv.crdownload"));
        assert!(!candidate("Show.S01E01.mkv.!qb"));
        assert!(!candidate("Show.S01E01.ar.srt"));
        assert!(!candidate("Show.S01E01.nfo"));
        assert!(!candidate("folder"));
        assert!(!candidate(".hidden.mkv"));
    }

    #[test]
    fn the_config_round_trips_and_defaults_are_safe() {
        let text = r#"{
            "watch_dirs": ["/media/tv"],
            "policy": { "target_languages": ["ar"], "delete_source_after_mux": true }
        }"#;
        let config: PipelineConfig = serde_json::from_str(text).unwrap();
        assert!(config.recursive);
        assert!(config.enabled());
        // Dry run stays on unless the file says otherwise.
        assert!(config.policy.dry_run);
        assert!(config.policy.prefer_human_subtitle);
        let again: PipelineConfig =
            serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        assert_eq!(again.policy, config.policy);
    }
}
