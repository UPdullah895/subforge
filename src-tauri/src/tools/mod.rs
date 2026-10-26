//! Adapter between the desktop app's config and `subforge-core`'s tool
//! resolution. The tool wrappers themselves live in `subforge_core::tools`.

use crate::config::AppConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use subforge_core::tools::{tool_status, ToolId, ToolPaths, ToolStatus};

static BUNDLED_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Registers the directory holding the tools shipped with the app. Only the
/// first call has an effect.
pub fn set_bundled_dir(dir: PathBuf) {
    let _ = BUNDLED_DIR.set(dir);
}

pub fn bundled_dir() -> Option<PathBuf> {
    BUNDLED_DIR.get().cloned()
}

/// Where tools may come from, built from the saved config (custom paths) and
/// the bundled directory.
pub fn tool_paths() -> ToolPaths {
    tool_paths_from(&AppConfig::load(), bundled_dir())
}

pub fn tool_paths_from(cfg: &AppConfig, bundled_dir: Option<PathBuf>) -> ToolPaths {
    let configured = [
        (ToolId::Mkvmerge, &cfg.mkvmerge_path),
        (ToolId::Mkvextract, &cfg.mkvextract_path),
        (ToolId::Mkvpropedit, &cfg.mkvpropedit_path),
        (ToolId::Ffmpeg, &cfg.ffmpeg_path),
        (ToolId::Ffprobe, &cfg.ffprobe_path),
    ];
    let custom: HashMap<ToolId, PathBuf> = configured
        .into_iter()
        .filter_map(|(id, p)| {
            let p = p.as_deref()?.trim();
            (!p.is_empty()).then(|| (id, PathBuf::from(p)))
        })
        .collect();
    ToolPaths {
        custom,
        bundled_dir,
    }
}

/// Shape of the `get_tool_registry` reply that the current frontend reads.
/// Task 12 moves the UI to `get_tool_status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub path: Option<String>,
    pub version: Option<String>,
    pub available: bool,
}

impl ToolInfo {
    fn from_status(s: &ToolStatus) -> Self {
        Self {
            name: s.id.name().to_string(),
            path: s.path.as_ref().map(|p| p.to_string_lossy().to_string()),
            version: s.version.clone(),
            available: s.path.is_some(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolRegistry {
    pub mkvmerge: ToolInfo,
    pub mkvextract: ToolInfo,
    pub mkvpropedit: ToolInfo,
    pub ffmpeg: ToolInfo,
    pub ffprobe: ToolInfo,
}

impl ToolRegistry {
    pub fn from_statuses(statuses: &[ToolStatus]) -> Self {
        let pick = |id: ToolId| {
            statuses
                .iter()
                .find(|s| s.id == id)
                .map(ToolInfo::from_status)
                .unwrap_or_else(|| ToolInfo {
                    name: id.name().to_string(),
                    path: None,
                    version: None,
                    available: false,
                })
        };
        Self {
            mkvmerge: pick(ToolId::Mkvmerge),
            mkvextract: pick(ToolId::Mkvextract),
            mkvpropedit: pick(ToolId::Mkvpropedit),
            ffmpeg: pick(ToolId::Ffmpeg),
            ffprobe: pick(ToolId::Ffprobe),
        }
    }

    pub fn discover() -> Self {
        Self::from_statuses(&tool_status(&tool_paths()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_paths_use_configured_custom_paths_and_skip_blanks() {
        let cfg = AppConfig {
            mkvmerge_path: Some("/opt/mkv/mkvmerge".into()),
            mkvpropedit_path: Some("/opt/mkv/mkvpropedit".into()),
            ffmpeg_path: Some("   ".into()),
            ffprobe_path: Some(String::new()),
            ..AppConfig::default()
        };
        let paths = tool_paths_from(&cfg, Some(PathBuf::from("/app/bin")));
        assert_eq!(
            paths.custom.get(&ToolId::Mkvmerge),
            Some(&PathBuf::from("/opt/mkv/mkvmerge"))
        );
        assert_eq!(
            paths.custom.get(&ToolId::Mkvpropedit),
            Some(&PathBuf::from("/opt/mkv/mkvpropedit"))
        );
        assert!(!paths.custom.contains_key(&ToolId::Ffmpeg));
        assert!(!paths.custom.contains_key(&ToolId::Ffprobe));
        assert!(!paths.custom.contains_key(&ToolId::Mkvextract));
        assert_eq!(paths.bundled_dir, Some(PathBuf::from("/app/bin")));
    }
}
