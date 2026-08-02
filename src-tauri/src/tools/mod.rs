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
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let full = dir.join(cmd);
        if full.is_file() {
            return Some(full);
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{}.exe", cmd));
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

pub fn find_python() -> Option<PathBuf> {
    #[cfg(windows)]
    let candidates: &[&str] = &["python", "python3", "py"];
    #[cfg(not(windows))]
    let candidates: &[&str] = &["python3", "python"];
    candidates.iter().find_map(|cmd| which(cmd))
}

pub fn script_path(name: &str) -> String {
    // 1. Next to the binary — Linux/Windows packaged builds
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("scripts").join(name);
            if p.exists() {
                return p.to_string_lossy().to_string();
            }
            // 2. macOS .app bundle: exe is in Contents/MacOS, resources in Contents/Resources
            if let Some(resources) = dir.parent().map(|d| d.join("Resources")) {
                let p2 = resources.join("scripts").join(name);
                if p2.exists() {
                    return p2.to_string_lossy().to_string();
                }
            }
        }
    }
    // 3. CWD fallback for dev mode (cargo tauri dev from project root)
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
