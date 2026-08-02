use super::{which, ToolInfo};
use crate::config::AppConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FfmpegError {
    #[error("ffmpeg not found in PATH")]
    FfmpegNotFound,
    #[error("ffprobe not found in PATH")]
    FfprobeNotFound,
    #[error("Failed to run command: {0}")]
    CommandFailed(String),
    #[error("Failed to parse JSON: {0}")]
    JsonParseError(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FfprobeStream {
    pub index: u64,
    #[serde(rename = "codec_type")]
    pub codec_type: String,
    pub codec_name: Option<String>,
    pub codec_long_name: Option<String>,
    #[serde(default)]
    pub tags: Option<FfprobeTags>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FfprobeTags {
    pub language: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FfprobeResult {
    pub streams: Vec<FfprobeStream>,
}

pub struct FfmpegTool {
    path: Option<PathBuf>,
}

impl FfmpegTool {
    pub fn new() -> Self {
        Self { path: None }
    }

    pub fn with_path(path: &str) -> Self {
        Self {
            path: Some(PathBuf::from(path)),
        }
    }
}

impl super::Tool for FfmpegTool {
    fn name(&self) -> &str {
        "ffmpeg"
    }

    fn discover(&mut self) -> Option<PathBuf> {
        self.path = which("ffmpeg");
        self.path.clone()
    }

    fn path(&self) -> Option<&str> {
        self.path.as_ref().map(|p| p.to_str()).flatten()
    }

    fn is_available(&self) -> bool {
        self.path.is_some()
    }
}

pub fn discover_ffmpeg() -> ToolInfo {
    let path = which("ffmpeg");
    let available = path.is_some();
    let version = path.as_ref().and_then(|p| {
        Command::new(p)
            .args(["-version"])
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8(o.stdout)
                    .ok()
                    .and_then(|s| s.lines().next().map(|l| l.to_string()))
            })
    });

    ToolInfo {
        name: "ffmpeg".to_string(),
        path: path.map(|p| p.to_string_lossy().to_string()),
        version,
        available,
    }
}

pub fn discover_ffprobe() -> ToolInfo {
    let path = which("ffprobe");
    let available = path.is_some();
    let version = path.as_ref().and_then(|p| {
        Command::new(p)
            .args(["-version"])
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8(o.stdout)
                    .ok()
                    .and_then(|s| s.lines().next().map(|l| l.to_string()))
            })
    });

    ToolInfo {
        name: "ffprobe".to_string(),
        path: path.map(|p| p.to_string_lossy().to_string()),
        version,
        available,
    }
}

pub fn get_ffmpeg_path() -> Result<PathBuf, FfmpegError> {
    let config = AppConfig::load();
    if let Some(p) = config.ffmpeg_path {
        let path = PathBuf::from(&p);
        if path.is_file() {
            return Ok(path);
        }
    }
    which("ffmpeg").ok_or(FfmpegError::FfmpegNotFound)
}

pub fn get_ffprobe_path() -> Result<PathBuf, FfmpegError> {
    let config = AppConfig::load();
    if let Some(p) = config.ffprobe_path {
        let path = PathBuf::from(&p);
        if path.is_file() {
            return Ok(path);
        }
    }
    which("ffprobe").ok_or(FfmpegError::FfprobeNotFound)
}

pub fn probe_audio_tracks(video_path: &str) -> Result<Vec<FfprobeStream>, FfmpegError> {
    let ffprobe = get_ffprobe_path()?;

    let output = Command::new(&ffprobe)
        .args([
            "-v", "error",
            "-select_streams", "a",
            "-show_entries", "stream=index,codec_type,codec_name,codec_long_name:stream_tags=language,title",
            "-of", "json",
            video_path,
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(FfmpegError::CommandFailed(stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: FfprobeResult = serde_json::from_str(&stdout)
        .map_err(|e| FfmpegError::JsonParseError(e.to_string()))?;

    Ok(result.streams)
}

pub fn extract_audio(
    video_path: &str,
    track_index: u64,
    output_path: &str,
    format: Option<&str>,
) -> Result<(), FfmpegError> {
    let ffmpeg = get_ffmpeg_path()?;

    let mut args = vec![
        "-i".to_string(),
        video_path.to_string(),
        "-map".to_string(),
        format!("0:a:{}", track_index),
        "-vn".to_string(),
    ];

    if let Some(fmt) = format {
        match fmt.to_lowercase().as_str() {
            "aac" => {
                args.push("-c:a".to_string());
                args.push("copy".to_string());
            }
            "mp3" => {
                args.push("-c:a".to_string());
                args.push("libmp3lame".to_string());
                args.push("-q:a".to_string());
                args.push("2".to_string());
            }
            "opus" => {
                args.push("-c:a".to_string());
                args.push("libopus".to_string());
                args.push("-b:a".to_string());
                args.push("128k".to_string());
            }
            "flac" => {
                args.push("-c:a".to_string());
                args.push("flac".to_string());
            }
            _ => {
                args.push("-c:a".to_string());
                args.push("copy".to_string());
            }
        }
    }

    args.push(output_path.to_string());

    let output = Command::new(&ffmpeg).args(&args).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(FfmpegError::CommandFailed(stderr));
    }

    Ok(())
}

pub fn merge_audio(
    video_path: &str,
    audio_path: &str,
    output_path: &str,
) -> Result<(), FfmpegError> {
    let ffmpeg = get_ffmpeg_path()?;

    let output = Command::new(&ffmpeg)
        .args([
            "-i", video_path,
            "-i", audio_path,
            "-c", "copy",
            "-map", "0:v:0",
            "-map", "0:a?",
            "-map", "1:a:0",
            output_path,
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(FfmpegError::CommandFailed(stderr));
    }

    Ok(())
}
