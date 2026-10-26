use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub mod ffmpeg;
pub mod mkvtoolnix;
pub mod pysubs2;
pub mod translate;
pub mod whisper;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub path: Option<String>,
    pub version: Option<String>,
    pub available: bool,
}

pub trait Tool {
    fn name(&self) -> &str;
    fn discover(&mut self) -> Option<PathBuf>;
    fn path(&self) -> Option<&str>;
    fn is_available(&self) -> bool;
}

pub fn which(cmd: &str) -> Option<PathBuf> {
    subforge_deps::find_executable(cmd, &subforge_deps::SearchEnv::from_process())
}

pub fn find_python() -> Option<PathBuf> {
    subforge_deps::Context::from_process().find_python().map(|info| info.path)
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

#[derive(Debug, Clone, Serialize)]
pub struct ToolRegistry {
    pub mkvmerge: ToolInfo,
    pub mkvextract: ToolInfo,
    pub ffmpeg: ToolInfo,
    pub ffprobe: ToolInfo,
}

impl ToolRegistry {
    pub fn discover() -> Self {
        Self {
            mkvmerge: mkvtoolnix::discover_mkvmerge(),
            mkvextract: mkvtoolnix::discover_mkvextract(),
            ffmpeg: ffmpeg::discover_ffmpeg(),
            ffprobe: ffmpeg::discover_ffprobe(),
        }
    }
}
