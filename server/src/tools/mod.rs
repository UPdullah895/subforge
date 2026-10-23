//! Adapter between the server's config and `subforge-core`'s tool
//! resolution. The tool wrappers themselves live in `subforge_core::tools`.

use crate::config::AppConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use subforge_core::tools::{tool_status, ToolId, ToolPaths, ToolStatus};

pub mod whisper;

/// Directory holding tools shipped with the server: `SUBFORGE_BUNDLED_DIR` if
/// set and non-empty, else `<exe_dir>/bin` if it exists.
pub fn bundled_dir() -> Option<PathBuf> {
    bundled_dir_from(
        std::env::var("SUBFORGE_BUNDLED_DIR").ok().as_deref(),
        std::env::current_exe().ok().as_deref(),
    )
}

fn bundled_dir_from(env: Option<&str>, exe: Option<&std::path::Path>) -> Option<PathBuf> {
    if let Some(dir) = env.map(str::trim).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let bin = exe?.parent()?.join("bin");
    bin.is_dir().then_some(bin)
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

pub fn find_python() -> Option<PathBuf> {
    subforge_deps::Context::from_process()
        .find_python()
        .map(|info| info.path)
}

/// Resolves a bundled Python script by name.
/// 1. Next to the server binary (packaged deployment)
/// 2. `../../scripts/` relative to binary (dev — binary in `target/debug/`, scripts at project root)
/// 3. `./scripts/` relative to current working directory (fallback)
pub fn script_path(name: &str) -> String {
    if let Some(p) = subforge_deps::locate_script(name) {
        return p.to_string_lossy().to_string();
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Packaged: scripts shipped next to binary
            let p1 = dir.join("scripts").join(name);
            if p1.exists() {
                return p1.to_string_lossy().to_string();
            }
            // Dev mode: binary is in target/debug/, scripts are two levels up at project root
            if let Some(root) = dir.parent().and_then(|p| p.parent()) {
                let p2 = root.join("scripts").join(name);
                if p2.exists() {
                    return p2.to_string_lossy().to_string();
                }
            }
        }
    }
    // Fallback: relative to CWD (running server from project root)
    std::env::current_dir()
        .map(|d| d.join("scripts").join(name).to_string_lossy().to_string())
        .unwrap_or_else(|_| format!("scripts/{}", name))
}

/// Shape of `GET /api/tools/registry` that the current web client reads.
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

pub fn status() -> Vec<ToolStatus> {
    tool_status(&tool_paths())
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
            ffprobe_path: Some("/opt/ff/ffprobe".into()),
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
        assert_eq!(
            paths.custom.get(&ToolId::Ffprobe),
            Some(&PathBuf::from("/opt/ff/ffprobe"))
        );
        assert!(!paths.custom.contains_key(&ToolId::Ffmpeg));
        assert!(!paths.custom.contains_key(&ToolId::Mkvextract));
        assert_eq!(paths.bundled_dir, Some(PathBuf::from("/app/bin")));
    }

    #[test]
    fn bundled_dir_prefers_env_then_existing_exe_bin() {
        let tmp = std::env::temp_dir().join(format!("subforge_bd_{}", std::process::id()));
        std::fs::create_dir_all(tmp.join("bin")).unwrap();
        let exe = tmp.join("subforge-server");

        assert_eq!(
            bundled_dir_from(Some("/custom"), Some(&exe)),
            Some(PathBuf::from("/custom"))
        );
        assert_eq!(
            bundled_dir_from(Some("  "), Some(&exe)),
            Some(tmp.join("bin"))
        );
        assert_eq!(bundled_dir_from(None, Some(&exe)), Some(tmp.join("bin")));
        let missing = tmp.join("nobin").join("subforge-server");
        assert_eq!(bundled_dir_from(None, Some(&missing)), None);
        std::fs::remove_dir_all(tmp).unwrap();
    }
}
