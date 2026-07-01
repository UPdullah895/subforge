use crate::jobs::{Job, JobStatus, JobType, SharedJobQueue};
use crate::tools::mkvtoolnix::{self, ExternalSub, VideoFileInfo};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use subforge_metadata::filenaming;
use tauri::{AppHandle, Emitter, State};
use walkdir::WalkDir;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanOptions {
    pub path: String,
    pub recursive: bool,
    pub extensions: Vec<String>,
}

#[tauri::command]
pub fn scan_directory(options: ScanOptions) -> Result<Vec<VideoFileInfo>, String> {
    let exts: HashSet<String> = options
        .extensions
        .into_iter()
        .map(|e| e.to_lowercase().trim_start_matches('.').to_string())
        .collect();

    let mut files = Vec::new();

    let walker = if options.recursive {
        WalkDir::new(&options.path)
    } else {
        WalkDir::new(&options.path).max_depth(1)
    };

    for entry in walker {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if !exts.is_empty() && !exts.contains(&ext) {
            continue;
        }

        let path_str = path.to_string_lossy().to_string();

        match mkvtoolnix::enumerate_tracks(&path_str) {
            Ok(info) => {
                let subtitle_count = info.tracks.iter().filter(|t| t.track_type == "subtitles").count();
                let audio_count = info.tracks.iter().filter(|t| t.track_type == "audio").count();
                let video_count = info.tracks.iter().filter(|t| t.track_type == "video").count();

                files.push(VideoFileInfo {
                    path: path_str.clone(),
                    name: path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path_str.clone()),
                    tracks: info.tracks,
                    subtitle_count,
                    audio_count,
                    video_count,
                    external_subs: detect_external_subs(path),
                });
            }
            Err(_) => {
                // Skip files that can't be parsed by mkvmerge
                continue;
            }
        }
    }

    Ok(files)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchExtractOptions {
    pub files: Vec<String>,
    pub output_dir: String,
    #[serde(default)]
    pub languages: Vec<String>,
}

#[tauri::command]
pub async fn batch_extract_subtitles(
    app: AppHandle,
    queue: State<'_, SharedJobQueue>,
    options: BatchExtractOptions,
) -> Result<Vec<String>, String> {
    let mut job_ids = Vec::new();

    for file in &options.files {
        let job_id = format!("batch-extract-{}", uuid::Uuid::new_v4());
        let output_dir = options.output_dir.clone();
        let languages = options.languages.clone();
        let file = file.clone();

        {
            let mut q = queue.lock().map_err(|e| e.to_string())?;
            q.add(Job {
                id: job_id.clone(),
                job_type: JobType::BatchOperation,
                status: JobStatus::Pending,
                input_file: file.clone(),
                output_file: Some(output_dir.clone()),
                progress: 0.0,
                message: Some("Queued".to_string()),
            });
        }

        job_ids.push(job_id.clone());

        let app_clone = app.clone();
        let queue_clone = queue.inner().clone();

        tokio::spawn(async move {
            let result = if languages.is_empty() {
                mkvtoolnix::extract_all_subtitles(&file, &output_dir)
            } else {
                mkvtoolnix::extract_subtitles_by_language(&file, &output_dir, &languages)
            };

            let mut q = queue_clone.lock().unwrap();
            if let Ok(extracted) = result {
                q.update_status(&job_id, JobStatus::Completed);
                q.update_progress(&job_id, 100.0, Some(format!("Extracted {} tracks", extracted.len())));
            } else {
                q.update_status(&job_id, JobStatus::Failed);
                q.update_progress(&job_id, 0.0, Some("Extraction failed".to_string()));
            }

            let _ = app_clone.emit("job-completed", serde_json::json!({
                "job_id": job_id,
                "status": q.get(&job_id).map(|j| &j.status),
                "message": q.get(&job_id).and_then(|j| j.message.clone()),
            }));
        });
    }

    Ok(job_ids)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubToMerge {
    pub path: String,
    pub lang: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeSubsRequest {
    pub video_path: String,
    pub subs: Vec<SubToMerge>,
    #[serde(default)]
    pub delete_originals: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeSubsResult {
    pub output_path: String,
}

/// Muxes existing external subtitle files (already detected by the batch
/// scan) into their video via mkvmerge, reusing the same
/// `mkvtoolnix::merge_subtitles` call the single-file Subtitles page uses.
/// Writes to a new `{video_stem}.merged.mkv` rather than overwriting the
/// source, matching the auto-translate pipeline's convention of not
/// clobbering the original file.
#[tauri::command]
pub fn merge_external_subs(request: MergeSubsRequest) -> Result<MergeSubsResult, String> {
    let video_path = Path::new(&request.video_path);
    let stem = video_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video");
    let dir = video_path.parent().unwrap_or_else(|| Path::new("."));
    let output_path = dir
        .join(format!("{stem}.merged.mkv"))
        .to_string_lossy()
        .to_string();

    let subs: Vec<(String, Option<String>)> = request
        .subs
        .iter()
        .map(|s| (s.path.clone(), Some(s.lang.clone())))
        .collect();

    mkvtoolnix::merge_subtitles(&request.video_path, &subs, &output_path).map_err(|e| e.to_string())?;

    if request.delete_originals {
        for sub in &request.subs {
            let _ = std::fs::remove_file(&sub.path);
        }
    }

    Ok(MergeSubsResult { output_path })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn detects_matching_lang_and_ext_siblings() {
        let dir = std::env::temp_dir().join(format!("subforge_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let video = dir.join("Show - S01E01.mkv");
        fs::write(&video, b"").unwrap();
        fs::write(dir.join("Show - S01E01.ar.srt"), b"").unwrap();
        fs::write(dir.join("Show - S01E01.en.ass"), b"").unwrap();
        // Not a recognized language code — should be ignored.
        fs::write(dir.join("Show - S01E01.xyz.srt"), b"").unwrap();
        // Doesn't share the video's stem — should be ignored.
        fs::write(dir.join("Other - S01E01.ar.srt"), b"").unwrap();

        let mut subs = detect_external_subs(&video);
        subs.sort_by(|a, b| a.lang.cmp(&b.lang));

        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0].lang, "ar");
        assert_eq!(subs[0].status, "ok");
        assert_eq!(subs[1].lang, "en");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn returns_empty_when_no_siblings_exist() {
        let dir = std::env::temp_dir().join(format!("subforge_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let video = dir.join("Lonely.mkv");
        fs::write(&video, b"").unwrap();

        assert!(detect_external_subs(&video).is_empty());

        fs::remove_dir_all(&dir).unwrap();
    }
}
