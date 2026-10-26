use crate::tools::mkvtoolnix::{ExternalSub, VideoFileInfo};
use crate::tools::mkvtoolnix;
use crate::{AppState, jobs};
use axum::{extract::{Json, State}, http::StatusCode};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
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
        if let Some(parsed) = filenaming::parse_external_sub_filename(stem, name) {
            subs.push(ExternalSub {
                lang: parsed.lang,
                status: "ok".to_string(),
                error_message: None,
                path: path.to_string_lossy().to_string(),
                flavor: parsed.flavor,
            });
        }
    }

    subs
}

pub async fn scan_directory(Json(req): Json<ScanRequest>) -> Result<Json<Vec<VideoFileInfo>>, StatusCode> {
    crate::error::blocking(move || scan_directory_blocking(req))
        .await?
        .map(Json)
}

fn scan_directory_blocking(req: ScanRequest) -> Result<Vec<VideoFileInfo>, StatusCode> {
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

    Ok(files)
}

#[derive(Deserialize)]
pub struct BatchExtractRequest {
    pub files: Vec<String>,
    pub output_dir: String,
    #[serde(default)]
    pub languages: Vec<String>,
}

/// An empty `output_dir` means "next to each video" — where media servers
/// look for external subtitles, and the only sensible choice when the files
/// live in separate season folders.
fn effective_output_dir(video: &str, output_dir: &str) -> String {
    if !output_dir.trim().is_empty() {
        return output_dir.to_string();
    }
    Path::new(video)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| ".".to_string())
}

pub async fn batch_extract_subtitles(
    Json(req): Json<BatchExtractRequest>,
) -> Result<Json<Vec<String>>, StatusCode> {
    crate::error::blocking(move || {
        let mut all_paths: Vec<String> = Vec::new();
        for file in &req.files {
            let output_dir = effective_output_dir(file, &req.output_dir);
            let result = if req.languages.is_empty() {
                mkvtoolnix::extract_all_subtitles(file, &output_dir)
            } else {
                mkvtoolnix::extract_subtitles_by_language(file, &output_dir, &req.languages)
            };
            match result {
                Ok(paths) => all_paths.extend(paths),
                Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
            }
        }
        Ok(Json(all_paths))
    })
    .await?
}

#[derive(Deserialize, Clone)]
pub struct SubToMerge {
    pub path: String,
    pub lang: String,
    #[serde(default)]
    pub track_name: Option<String>,
    #[serde(default)]
    pub hearing_impaired: bool,
    #[serde(default)]
    pub make_default: bool,
}

#[derive(Deserialize)]
pub struct MergeSubsRequest {
    pub video_path: String,
    pub subs: Vec<SubToMerge>,
    #[serde(default)]
    pub delete_originals: bool,
    /// Swap the merged file in for the original video instead of leaving a
    /// `.merged.mkv` next to it.
    #[serde(default)]
    pub replace_original: bool,
}

/// Muxes existing external subtitle files (already detected by the batch
/// scan) into their video via mkvmerge, reusing the same
/// `mkvtoolnix::merge_subtitles_async` call the single-file Subtitles route uses.
/// Writes to a new `{video_stem}.merged.mkv` rather than overwriting the
/// source, matching the auto-translate pipeline's convention of not
/// clobbering the original file.
pub async fn merge_external_subs(
    State(state): State<AppState>,
    Json(req): Json<MergeSubsRequest>,
) -> Result<Json<String>, StatusCode> {
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

    let job_id = format!("batch-merge-{}", uuid::Uuid::new_v4());

    {
        let mut q = state.job_queue.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        q.add(jobs::Job {
            id: job_id.clone(),
            job_type: jobs::JobType::BatchOperation,
            status: jobs::JobStatus::Pending,
            input_file: req.video_path.clone(),
            output_file: Some(output_path.clone()),
            progress: 0.0,
            message: Some("Queued".to_string()),
        });
    }

    let sender = state.event_sender.clone();
    jobs::emit_event(
        &sender,
        jobs::JobEvent {
            event_type: "job-started".to_string(),
            job_id: job_id.clone(),
            status: None,
            progress: None,
            message: None,
            stage: None,
            output_path: None,
        },
    );

    let queue_clone = state.job_queue.clone();
    let controls_clone = state.job_controls.clone();
    let job_id_clone = job_id.clone();
    let sender_clone = state.event_sender.clone();
    let delete_originals = req.delete_originals;
    let replace_original = req.replace_original;

    let subs: Vec<mkvtoolnix::SubtitleMergeSpec> = req
        .subs
        .iter()
        .map(|s| mkvtoolnix::SubtitleMergeSpec {
            path: s.path.clone(),
            language: Some(s.lang.clone()),
            track_name: s.track_name.clone(),
            hearing_impaired: s.hearing_impaired,
            make_default: s.make_default,
        })
        .collect();

    let orig_subs = req.subs.clone();
    let video_path_str = req.video_path.clone();

    tokio::spawn(async move {
        let control = Arc::new(jobs::JobControl::new());
        {
            let mut registry = controls_clone.lock().unwrap();
            registry.insert(job_id_clone.clone(), control.clone());
        }

        {
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, jobs::JobStatus::Running);
        }

        let result = mkvtoolnix::merge_subtitles_async(&video_path_str, &subs, &output_path, Some(&control), |pct| {
            let mut q = queue_clone.lock().unwrap();
            q.update_progress(&job_id_clone, pct, None);
            jobs::emit_event(
                &sender_clone,
                jobs::JobEvent {
                    event_type: "job-progress".to_string(),
                    job_id: job_id_clone.clone(),
                    status: None,
                    progress: Some(pct),
                    message: None,
                    stage: None,
                    output_path: None,
                },
            );
        }).await;

        if let Err(e) = result {
            let cancelled = matches!(e, mkvtoolnix::MkvError::Cancelled);
            let msg = e.to_string();
            let status = if cancelled { jobs::JobStatus::Cancelled } else { jobs::JobStatus::Failed };
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, status.clone());
            q.update_progress(&job_id_clone, 0.0, Some(msg.clone()));
            jobs::emit_event(
                &sender_clone,
                jobs::JobEvent {
                    event_type: "job-completed".to_string(),
                    job_id: job_id_clone.clone(),
                    status: Some(status),
                    progress: Some(0.0),
                    message: Some(msg),
                    stage: None,
                    output_path: None,
                },
            );
            let mut registry = controls_clone.lock().unwrap();
            registry.remove(&job_id_clone);
            return;
        }

        if delete_originals {
            for sub in &orig_subs {
                let _ = std::fs::remove_file(&sub.path);
            }
        }

        // Optionally swap the merged file in for the original, so the library
        // doesn't end up with two copies of the same episode. Only after the
        // merge succeeded and produced a non-empty file; if the swap fails
        // (file in use, permissions) the merged copy is kept and we say so.
        let mut final_output = output_path.clone();
        let mut done_message = "Merge complete".to_string();
        if replace_original {
            let produced = std::fs::metadata(&output_path).map(|m| m.len() > 0).unwrap_or(false);
            if produced {
                match std::fs::rename(&output_path, &video_path_str) {
                    Ok(()) => final_output = video_path_str.clone(),
                    Err(e) => {
                        done_message =
                            format!("Merged, but could not replace the original (kept {output_path}): {e}");
                    }
                }
            }
        }

        {
            let mut q = queue_clone.lock().unwrap();
            if let Some(job) = q.get_mut(&job_id_clone) {
                job.output_file = Some(final_output.clone());
            }
            q.update_status(&job_id_clone, jobs::JobStatus::Completed);
            q.update_progress(&job_id_clone, 100.0, Some(done_message.clone()));
        }
        jobs::emit_event(
            &sender_clone,
            jobs::JobEvent {
                event_type: "job-completed".to_string(),
                job_id: job_id_clone.clone(),
                status: Some(jobs::JobStatus::Completed),
                progress: Some(100.0),
                message: Some(done_message),
                stage: None,
                output_path: Some(final_output),
            },
        );

        let mut registry = controls_clone.lock().unwrap();
        registry.remove(&job_id_clone);
    });

    // A bare job id string, matching the desktop backend and the other job
    // commands, so the frontend can wait on it uniformly.
    Ok(Json(job_id))
}
