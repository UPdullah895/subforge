use super::{which, ToolInfo};
use crate::config::AppConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MkvError {
    #[error("mkvmerge not found in PATH")]
    MkvmergeNotFound,
    #[error("mkvextract not found in PATH")]
    MkvextractNotFound,
    #[error("Failed to run command: {0}")]
    CommandFailed(String),
    #[error("Failed to parse JSON: {0}")]
    JsonParseError(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackProperties {
    #[serde(default)]
    pub language: Option<String>,
    #[serde(rename = "track_name", default)]
    pub track_name: Option<String>,
    #[serde(default)]
    pub default_track: Option<bool>,
    #[serde(default)]
    pub forced_track: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: u64,
    #[serde(rename = "type")]
    pub track_type: String,
    pub codec: String,
    #[serde(default)]
    pub properties: Option<TrackProperties>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerProperties {
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MkvInfo {
    #[serde(default)]
    pub container: Option<ContainerProperties>,
    #[serde(default)]
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoFileInfo {
    pub path: String,
    pub name: String,
    pub tracks: Vec<Track>,
    pub subtitle_count: usize,
    pub audio_count: usize,
    pub video_count: usize,
}

pub fn discover_mkvmerge() -> ToolInfo {
    let path = which("mkvmerge");
    let available = path.is_some();
    let version = path.as_ref().and_then(|p| {
        Command::new(p)
            .args(["--version"])
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8(o.stdout)
                    .ok()
                    .and_then(|s| s.lines().next().map(|l| l.trim().to_string()))
            })
    });

    ToolInfo {
        name: "mkvmerge".to_string(),
        path: path.map(|p| p.to_string_lossy().to_string()),
        version,
        available,
    }
}

pub fn discover_mkvextract() -> ToolInfo {
    let path = which("mkvextract");
    let available = path.is_some();
    let version = path.as_ref().and_then(|p| {
        Command::new(p)
            .args(["--version"])
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8(o.stdout)
                    .ok()
                    .and_then(|s| s.lines().next().map(|l| l.trim().to_string()))
            })
    });

    ToolInfo {
        name: "mkvextract".to_string(),
        path: path.map(|p| p.to_string_lossy().to_string()),
        version,
        available,
    }
}

pub fn get_mkvmerge_path() -> Result<PathBuf, MkvError> {
    let config = AppConfig::load();
    if let Some(p) = config.mkvmerge_path {
        let path = PathBuf::from(&p);
        if path.is_file() {
            return Ok(path);
        }
    }
    which("mkvmerge").ok_or(MkvError::MkvmergeNotFound)
}

pub fn get_mkvextract_path() -> Result<PathBuf, MkvError> {
    let config = AppConfig::load();
    if let Some(p) = config.mkvextract_path {
        let path = PathBuf::from(&p);
        if path.is_file() {
            return Ok(path);
        }
    }
    which("mkvextract").ok_or(MkvError::MkvextractNotFound)
}

pub fn enumerate_tracks(file_path: &str) -> Result<MkvInfo, MkvError> {
    let mkvmerge = get_mkvmerge_path()?;

    let output = Command::new(&mkvmerge)
        .args(["-J", file_path])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(MkvError::CommandFailed(stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let info: MkvInfo = serde_json::from_str(&stdout)
        .map_err(|e| MkvError::JsonParseError(e.to_string()))?;

    Ok(info)
}

pub fn extract_subtitle_track(
    file_path: &str,
    track_id: u64,
    output_path: &str,
) -> Result<String, MkvError> {
    let mkvextract = get_mkvextract_path()?;

    let output = Command::new(&mkvextract)
        .args(["tracks", file_path, &format!("{}:{}", track_id, output_path)])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(MkvError::CommandFailed(stderr));
    }

    Ok(output_path.to_string())
}

pub fn extract_all_subtitles(
    file_path: &str,
    output_dir: &str,
) -> Result<Vec<String>, MkvError> {
    let info = enumerate_tracks(file_path)?;
    let mut extracted = Vec::new();

    for track in &info.tracks {
        if track.track_type == "subtitles" {
            let lang = track
                .properties
                .as_ref()
                .and_then(|p| p.language.clone())
                .unwrap_or_else(|| "und".to_string());
            let out = std::path::PathBuf::from(output_dir)
                .join(format!("{}_{}.srt", track.id, lang))
                .to_string_lossy()
                .to_string();
            extract_subtitle_track(file_path, track.id, &out)?;
            extracted.push(out);
        }
    }

    Ok(extracted)
}

pub fn merge_subtitles(
    video_path: &str,
    subtitle_paths: &[(String, Option<String>)], // (path, optional_language)
    output_path: &str,
) -> Result<(), MkvError> {
    let mkvmerge = get_mkvmerge_path()?;
    let mut args = vec!["-o".to_string(), output_path.to_string(), video_path.to_string()];

    for (sub_path, lang) in subtitle_paths {
        args.push("--language".to_string());
        args.push(format!("0:{}", lang.as_deref().unwrap_or("und")));
        args.push(sub_path.clone());
    }

    let output = Command::new(&mkvmerge).args(&args).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(MkvError::CommandFailed(stderr));
    }

    Ok(())
}

pub fn remove_tracks(
    video_path: &str,
    track_ids: &[u64],
    output_path: &str,
) -> Result<(), MkvError> {
    let mkvmerge = get_mkvmerge_path()?;
    let info = enumerate_tracks(video_path)?;
    let remove_set: std::collections::HashSet<u64> = track_ids.iter().copied().collect();
    let mut args = vec!["-o".to_string(), output_path.to_string()];

    let has_sub_removal = info.tracks.iter()
        .any(|t| t.track_type == "subtitles" && remove_set.contains(&t.id));
    if has_sub_removal {
        let keep: Vec<String> = info.tracks.iter()
            .filter(|t| t.track_type == "subtitles" && !remove_set.contains(&t.id))
            .map(|t| t.id.to_string())
            .collect();
        if keep.is_empty() {
            args.push("--no-subtitles".to_string());
        } else {
            args.push("--subtitle-tracks".to_string());
            args.push(keep.join(","));
        }
    }

    let has_audio_removal = info.tracks.iter()
        .any(|t| t.track_type == "audio" && remove_set.contains(&t.id));
    if has_audio_removal {
        let keep: Vec<String> = info.tracks.iter()
            .filter(|t| t.track_type == "audio" && !remove_set.contains(&t.id))
            .map(|t| t.id.to_string())
            .collect();
        if keep.is_empty() {
            args.push("--no-audio".to_string());
        } else {
            args.push("--audio-tracks".to_string());
            args.push(keep.join(","));
        }
    }

    args.push(video_path.to_string());
    let output = Command::new(&mkvmerge).args(&args).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(MkvError::CommandFailed(stderr));
    }

    Ok(())
}

pub fn remove_all_subtitles(video_path: &str, output_path: &str) -> Result<(), MkvError> {
    let mkvmerge = get_mkvmerge_path()?;
    let output = Command::new(&mkvmerge)
        .args(["-o", output_path, "--no-subtitles", video_path])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(MkvError::CommandFailed(stderr));
    }

    Ok(())
}

pub fn remove_all_audio(video_path: &str, output_path: &str) -> Result<(), MkvError> {
    let mkvmerge = get_mkvmerge_path()?;
    let output = Command::new(&mkvmerge)
        .args(["-o", output_path, "--no-audio", video_path])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(MkvError::CommandFailed(stderr));
    }

    Ok(())
}
