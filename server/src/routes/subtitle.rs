use crate::error::{blocking, AppError};
use crate::tools::tool_paths;
use crate::{jobs, AppState};
use axum::{
    extract::{Json, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use subforge_core::subtitle::{self, Format};
use subforge_core::tools::mkvtoolnix;
use subforge_core::tools::ToolError;

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
) -> Result<Json<Vec<TrackInfo>>, AppError> {
    let info = blocking(move || mkvtoolnix::enumerate_tracks(&tool_paths(), &req.file_path))
        .await?
        .map_err(AppError::from)?;
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

pub async fn extract_subtitle(Json(req): Json<ExtractRequest>) -> Result<String, AppError> {
    let output_path = req.output_path.clone();
    blocking(move || {
        mkvtoolnix::extract_subtitle_track(
            &tool_paths(),
            &req.file_path,
            req.track_id,
            &req.output_path,
        )
    })
    .await?
    .map_err(AppError::from)?;
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
        let mut q = state
            .job_queue
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
            details: None,
        },
    );

    let paths = tool_paths();
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
        let result = mkvtoolnix::merge_subtitles_async(
            &paths,
            &video_path,
            &subs,
            &output_path,
            Some(control.token()),
            |pct| {
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
                        details: None,
                    },
                );
            },
        )
        .await;

        if let Err(e) = result {
            let cancelled = matches!(e, ToolError::Cancelled);
            let msg = e.to_string();
            let details = e.details().map(str::to_string);
            let status = if cancelled {
                jobs::JobStatus::Cancelled
            } else {
                jobs::JobStatus::Failed
            };
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
                    details,
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
                details: None,
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

pub async fn remove_subtitles(Json(req): Json<RemoveRequest>) -> Result<StatusCode, AppError> {
    blocking(move || {
        mkvtoolnix::remove_all_subtitles(&tool_paths(), &req.file_path, &req.output_path)
    })
    .await?
    .map_err(AppError::from)?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct ConvertRequest {
    input: String,
    output: String,
    format: String,
}

pub async fn convert_format(Json(req): Json<ConvertRequest>) -> Result<StatusCode, AppError> {
    let to = Format::from_extension(&req.format).ok_or_else(|| {
        crate::error::err400(format!(
            "Unsupported subtitle format '{}'. Use one of: srt, ass, ssa, vtt.",
            req.format
        ))
    })?;
    blocking(move || subtitle::convert_file(Path::new(&req.input), Path::new(&req.output), to))
        .await
        .map_err(AppError::from)?
        .map_err(crate::error::err500)?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct ShiftRequest {
    input: String,
    output: String,
    milliseconds: i64,
}

pub async fn shift_timing(Json(req): Json<ShiftRequest>) -> Result<StatusCode, AppError> {
    blocking(move || {
        subtitle::shift_file(
            Path::new(&req.input),
            Path::new(&req.output),
            req.milliseconds,
        )
    })
    .await
    .map_err(AppError::from)?
    .map_err(crate::error::err500)?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct ExtractAllRequest {
    file_path: String,
    output_dir: String,
}

pub async fn extract_all_subtitles(
    Json(req): Json<ExtractAllRequest>,
) -> Result<Json<Vec<String>>, AppError> {
    let paths = blocking(move || {
        mkvtoolnix::extract_all_subtitles(&tool_paths(), &req.file_path, &req.output_dir)
    })
    .await?
    .map_err(AppError::from)?;
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
pub async fn stretch_timing(Json(req): Json<StretchRequest>) -> Result<StatusCode, AppError> {
    if !(req.from_fps > 0.0 && req.to_fps > 0.0) {
        return Err(crate::error::err400(
            "Frame rates must be positive numbers.",
        ));
    }
    blocking(move || {
        subtitle::stretch_file(
            Path::new(&req.input),
            Path::new(&req.output),
            req.from_fps,
            req.to_fps,
        )
    })
    .await
    .map_err(AppError::from)?
    .map_err(crate::error::err500)?;
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

/// mkvpropedit `--set` pairs for the fields the request changes.
fn track_flag_props(req: &SetTrackMetaRequest) -> Vec<(String, String)> {
    let mut props = Vec::new();
    if let Some(name) = &req.track_name {
        props.push(("name".to_string(), name.clone()));
    }
    if let Some(default) = req.default_flag {
        props.push(("flag-default".to_string(), u8::from(default).to_string()));
    }
    if let Some(forced) = req.forced_flag {
        props.push(("flag-forced".to_string(), u8::from(forced).to_string()));
    }
    props
}

pub async fn set_track_metadata(
    Json(req): Json<SetTrackMetaRequest>,
) -> Result<StatusCode, StatusCode> {
    // mkvpropedit uses 1-indexed track:@N selection.
    let number = u32::try_from(req.track_id + 1).map_err(|_| StatusCode::BAD_REQUEST)?;
    blocking(move || {
        let props = track_flag_props(&req);
        mkvtoolnix::set_track_flags(
            &tool_paths(),
            &PathBuf::from(&req.file_path),
            number,
            &props,
        )
    })
    .await?
    .map_err(|e| match e {
        ToolError::NotFound(_) | ToolError::CustomPathInvalid { .. } => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    })?;
    Ok(StatusCode::OK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_flags_cover_only_requested_fields() {
        let req = SetTrackMetaRequest {
            file_path: "x.mkv".into(),
            track_id: 2,
            track_name: Some("English".into()),
            default_flag: Some(true),
            forced_flag: Some(false),
        };
        assert_eq!(
            track_flag_props(&req),
            vec![
                ("name".to_string(), "English".to_string()),
                ("flag-default".to_string(), "1".to_string()),
                ("flag-forced".to_string(), "0".to_string()),
            ]
        );
    }
}
