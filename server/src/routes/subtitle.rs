use subforge_deps::HideWindow;
use crate::error::blocking;
use crate::tools::mkvtoolnix;
use crate::{AppState, jobs};
use axum::{extract::{Json, State}, http::StatusCode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Deserialize)]
pub struct EnumerateRequest {
    file_path: String,
}

#[derive(Serialize)]
pub struct TrackInfo {
    id: u64,
    track_type: String,
    codec: String,
    language: String,
    track_name: Option<String>,
    default_track: bool,
    forced_track: bool,
    hearing_impaired: bool,
}

pub async fn enumerate_tracks(
    Json(req): Json<EnumerateRequest>,
) -> Result<Json<Vec<TrackInfo>>, StatusCode> {
    let info = blocking(move || mkvtoolnix::enumerate_tracks(&req.file_path))
        .await?
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let tracks: Vec<TrackInfo> = info
        .tracks
        .iter()
        .map(|t| {
            let props = t.properties.as_ref();
            TrackInfo {
                id: t.id,
                track_type: t.track_type.clone(),
                codec: t.codec.clone(),
                language: props
                    .and_then(|p| p.language.clone())
                    .unwrap_or_else(|| "und".to_string()),
                track_name: props.and_then(|p| p.track_name.clone()),
                default_track: props.and_then(|p| p.default_track).unwrap_or(false),
                forced_track: props.and_then(|p| p.forced_track).unwrap_or(false),
                hearing_impaired: props.and_then(|p| p.hearing_impaired).unwrap_or(false),
            }
        })
        .collect();
    Ok(Json(tracks))
}

#[derive(Deserialize)]
pub struct ExtractRequest {
    file_path: String,
    track_id: u64,
    output_path: String,
}

pub async fn extract_subtitle(Json(req): Json<ExtractRequest>) -> Result<String, StatusCode> {
    let output_path = req.output_path.clone();
    blocking(move || {
        mkvtoolnix::extract_subtitle_track(&req.file_path, req.track_id, &req.output_path)
    })
    .await?
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(output_path)
}

#[derive(Deserialize)]
pub struct MergeRequest {
    video_path: String,
    subtitle_path: String,
    output_path: String,
    language: Option<String>,
}

pub async fn merge_subtitle(
    State(state): State<AppState>,
    Json(req): Json<MergeRequest>,
) -> Result<Json<String>, StatusCode> {
    let job_id = format!("merge-{}", uuid::Uuid::new_v4());

    {
        let mut q = state.job_queue.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        q.add(jobs::Job {
            id: job_id.clone(),
            job_type: jobs::JobType::MergeSubtitle,
            status: jobs::JobStatus::Pending,
            input_file: req.video_path.clone(),
            output_file: Some(req.output_path.clone()),
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
    let video_path = req.video_path.clone();
    let subtitle_path = req.subtitle_path.clone();
    let output_path = req.output_path.clone();
    let language = req.language.clone();

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

        let subs = vec![mkvtoolnix::SubtitleMergeSpec::new(subtitle_path, language)];
        let result = mkvtoolnix::merge_subtitles_async(&video_path, &subs, &output_path, Some(&control), |pct| {
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

        {
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, jobs::JobStatus::Completed);
            q.update_progress(&job_id_clone, 100.0, Some("Merge complete".to_string()));
        }
        jobs::emit_event(
            &sender_clone,
            jobs::JobEvent {
                event_type: "job-completed".to_string(),
                job_id: job_id_clone.clone(),
                status: Some(jobs::JobStatus::Completed),
                progress: Some(100.0),
                message: Some("Merge complete".to_string()),
                stage: None,
                output_path: Some(output_path),
            },
        );

        let mut registry = controls_clone.lock().unwrap();
        registry.remove(&job_id_clone);
    });

    // A bare job id string, matching the desktop command.
    Ok(Json(job_id))
}

#[derive(Deserialize)]
pub struct RemoveRequest {
    file_path: String,
    output_path: String,
}

pub async fn remove_subtitles(Json(req): Json<RemoveRequest>) -> Result<StatusCode, StatusCode> {
    blocking(move || mkvtoolnix::remove_all_subtitles(&req.file_path, &req.output_path))
        .await?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct ConvertRequest {
    input: String,
    output: String,
    format: String,
}

pub async fn convert_format(Json(req): Json<ConvertRequest>) -> Result<StatusCode, StatusCode> {
    blocking(move || crate::tools::pysubs2::convert_format(&req.input, &req.output, &req.format))
        .await?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct ShiftRequest {
    input: String,
    output: String,
    milliseconds: i64,
}

pub async fn shift_timing(Json(req): Json<ShiftRequest>) -> Result<StatusCode, StatusCode> {
    blocking(move || {
        crate::tools::pysubs2::shift_subtitles(&req.input, &req.output, req.milliseconds)
    })
    .await?
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct ExtractAllRequest {
    file_path: String,
    output_dir: String,
}

pub async fn extract_all_subtitles(
    Json(req): Json<ExtractAllRequest>,
) -> Result<Json<Vec<String>>, StatusCode> {
    let paths = blocking(move || mkvtoolnix::extract_all_subtitles(&req.file_path, &req.output_dir))
        .await?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(paths))
}

#[derive(Deserialize)]
pub struct StretchRequest {
    input: String,
    output: String,
    from_fps: f64,
    to_fps: f64,
}

/// Fixes progressive drift caused by a frame-rate mismatch between the
/// subtitle's origin and the video (a constant shift can't).
pub async fn stretch_timing(Json(req): Json<StretchRequest>) -> Result<StatusCode, (StatusCode, String)> {
    if !(req.from_fps > 0.0 && req.to_fps > 0.0) {
        return Err((StatusCode::BAD_REQUEST, "Frame rates must be positive numbers.".to_string()));
    }
    blocking(move || {
        crate::tools::pysubs2::stretch_framerate(&req.input, &req.output, req.from_fps, req.to_fps)
    })
    .await
    .map_err(|c| (c, "task failed".to_string()))?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct SetTrackMetaRequest {
    file_path: String,
    track_id: u64,
    track_name: Option<String>,
    default_flag: Option<bool>,
    forced_flag: Option<bool>,
}

pub async fn set_track_metadata(
    Json(req): Json<SetTrackMetaRequest>,
) -> Result<StatusCode, StatusCode> {
    let mkvpropedit = crate::tools::which("mkvpropedit")
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let mut args: Vec<String> = vec![req.file_path.clone()];
    args.push("--edit".to_string());
    // mkvpropedit uses 1-indexed @N notation for tracks
    args.push(format!("track:@{}", req.track_id + 1));

    if let Some(name) = &req.track_name {
        args.push("--set".to_string());
        args.push(format!("name={}", name));
    }
    if let Some(default) = req.default_flag {
        args.push("--set".to_string());
        args.push(format!("flag-default={}", if default { 1 } else { 0 }));
    }
    if let Some(forced) = req.forced_flag {
        args.push("--set".to_string());
        args.push(format!("flag-forced={}", if forced { 1 } else { 0 }));
    }

    let output = blocking(move || std::process::Command::new(&mkvpropedit).hide_window().args(&args).output())
        .await?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Exit code 1 means "finished with warnings" for the mkvtoolnix tools.
    if !matches!(output.status.code(), Some(0) | Some(1)) {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    Ok(StatusCode::OK)
}
