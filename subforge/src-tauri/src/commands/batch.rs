use crate::jobs::{Job, JobStatus, JobType, SharedJobQueue};
use crate::tools::mkvtoolnix::{self, ExternalSub, VideoFileInfo};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use tauri::{AppHandle, Emitter, State};
use walkdir::WalkDir;

const KNOWN_LANG_CODES: &[&str] = &[
    "en", "ar", "es", "fr", "de", "ja", "ko", "zh", "ru", "pt", "it", "tr", "hi", "fa", "ur",
];

/// Finds sibling subtitle files matching `{video_stem}.{lang}.{srt,ass}` next
/// to a video file — e.g. `Show - S01E01.ar.srt` alongside `Show - S01E01.mkv`.
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

    let prefix = format!("{}.", stem);
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
        if !name.starts_with(&prefix) {
            continue;
        }
        let rest = &name[prefix.len()..];
        let parts: Vec<&str> = rest.rsplitn(2, '.').collect();
        if parts.len() != 2 {
            continue;
        }
        let ext = parts[0].to_lowercase();
        let lang = parts[1].to_lowercase();
        if (ext == "srt" || ext == "ass") && KNOWN_LANG_CODES.contains(&lang.as_str()) {
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
            let result = mkvtoolnix::extract_all_subtitles(&file, &output_dir);

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
