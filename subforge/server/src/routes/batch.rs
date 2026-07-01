use crate::tools::mkvtoolnix::VideoFileInfo;
use crate::tools::mkvtoolnix;
use axum::{extract::Json, http::StatusCode};
use serde::Deserialize;
use std::collections::HashSet;
use walkdir::WalkDir;

#[derive(Deserialize)]
pub struct ScanRequest {
    path: String,
    recursive: bool,
    extensions: Vec<String>,
}

pub async fn scan_directory(Json(req): Json<ScanRequest>) -> Result<Json<Vec<VideoFileInfo>>, StatusCode> {
    let exts: HashSet<String> = req.extensions.into_iter().map(|e| e.to_lowercase().trim_start_matches('.').to_string()).collect();
    let mut files = Vec::new();

    let walker = if req.recursive {
        WalkDir::new(&req.path)
    } else {
        WalkDir::new(&req.path).max_depth(1)
    };

    for entry in walker {
        let entry = entry.map_err(|_| StatusCode::BAD_REQUEST)?;
        let path = entry.path();
        if !path.is_file() { continue; }

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if !exts.is_empty() && !exts.contains(&ext) { continue; }

        let path_str = path.to_string_lossy().to_string();
        match mkvtoolnix::enumerate_tracks(&path_str) {
            Ok(info) => {
                let subtitle_count = info.tracks.iter().filter(|t| t.track_type == "subtitles").count();
                let audio_count = info.tracks.iter().filter(|t| t.track_type == "audio").count();
                let video_count = info.tracks.iter().filter(|t| t.track_type == "video").count();
                files.push(VideoFileInfo {
                    path: path_str.clone(),
                    name: path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path_str.clone()),
                    tracks: info.tracks,
                    subtitle_count,
                    audio_count,
                    video_count,
                });
            }
            Err(_) => continue,
        }
    }

    Ok(Json(files))
}

#[derive(Deserialize)]
pub struct BatchExtractRequest {
    pub files: Vec<String>,
    pub output_dir: String,
}

pub async fn batch_extract_subtitles(
    Json(req): Json<BatchExtractRequest>,
) -> Result<Json<Vec<String>>, StatusCode> {
    let mut all_paths: Vec<String> = Vec::new();
    for file in &req.files {
        match mkvtoolnix::extract_all_subtitles(file, &req.output_dir) {
            Ok(paths) => all_paths.extend(paths),
            Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
    Ok(Json(all_paths))
}
