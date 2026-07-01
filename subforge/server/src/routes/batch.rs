use crate::tools::mkvtoolnix::{ExternalSub, VideoFileInfo};
use crate::tools::mkvtoolnix;
use axum::{extract::Json, http::StatusCode};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;
use subforge_metadata::filenaming;
use walkdir::WalkDir;

#[derive(Deserialize)]
pub struct ScanRequest {
    path: String,
    recursive: bool,
    extensions: Vec<String>,
}

/// Finds sibling subtitle files matching the shared `{video_stem}.{lang}.{ext}`
/// convention (see `subforge_metadata::filenaming`) next to a video file —
/// e.g. `Show - S01E01.ar.srt` alongside `Show - S01E01.mkv`. This is the same
/// convention `extract_all_subtitles` writes to, so subtitles this app
/// extracts are picked back up as external subs on a later scan.
fn detect_external_subs(video_path: &Path) -> Vec<ExternalSub> {
    let stem = match video_path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s,
        None => return vec![],
    };
    let dir = match video_path.parent() {
        Some(d) => d,
        None => return vec![],
    };

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    let mut subs = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if let Some((lang, _ext)) = filenaming::parse_external_sub_filename(stem, name) {
            subs.push(ExternalSub {
                lang,
                status: "ok".to_string(),
                error_message: None,
                path: path.to_string_lossy().to_string(),
            });
        }
    }

    subs
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
                    external_subs: detect_external_subs(path),
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
    #[serde(default)]
    pub languages: Vec<String>,
}

pub async fn batch_extract_subtitles(
    Json(req): Json<BatchExtractRequest>,
) -> Result<Json<Vec<String>>, StatusCode> {
    let mut all_paths: Vec<String> = Vec::new();
    for file in &req.files {
        let result = if req.languages.is_empty() {
            mkvtoolnix::extract_all_subtitles(file, &req.output_dir)
        } else {
            mkvtoolnix::extract_subtitles_by_language(file, &req.output_dir, &req.languages)
        };
        match result {
            Ok(paths) => all_paths.extend(paths),
            Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
    Ok(Json(all_paths))
}

#[derive(Deserialize)]
pub struct SubToMerge {
    pub path: String,
    pub lang: String,
}

#[derive(Deserialize)]
pub struct MergeSubsRequest {
    pub video_path: String,
    pub subs: Vec<SubToMerge>,
    #[serde(default)]
    pub delete_originals: bool,
}

#[derive(serde::Serialize)]
pub struct MergeSubsResult {
    pub output_path: String,
}

/// Muxes existing external subtitle files (already detected by the batch
/// scan) into their video via mkvmerge, reusing the same
/// `mkvtoolnix::merge_subtitles` call the single-file Subtitles route uses.
/// Writes to a new `{video_stem}.merged.mkv` rather than overwriting the
/// source, matching the auto-translate pipeline's convention of not
/// clobbering the original file.
pub async fn merge_external_subs(
    Json(req): Json<MergeSubsRequest>,
) -> Result<Json<MergeSubsResult>, (StatusCode, String)> {
    let video_path = Path::new(&req.video_path);
    let stem = video_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video");
    let dir = video_path.parent().unwrap_or_else(|| Path::new("."));
    let output_path = dir
        .join(format!("{stem}.merged.mkv"))
        .to_string_lossy()
        .to_string();

    let subs: Vec<(String, Option<String>)> = req
        .subs
        .iter()
        .map(|s| (s.path.clone(), Some(s.lang.clone())))
        .collect();

    mkvtoolnix::merge_subtitles(&req.video_path, &subs, &output_path)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if req.delete_originals {
        for sub in &req.subs {
            let _ = std::fs::remove_file(&sub.path);
        }
    }

    Ok(Json(MergeSubsResult { output_path }))
}
