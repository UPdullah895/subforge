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
    if let Ok(paths) = std::env::var("PATH") {
        for dir in paths.split(':') {
            let full = PathBuf::from(dir).join(cmd);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
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
