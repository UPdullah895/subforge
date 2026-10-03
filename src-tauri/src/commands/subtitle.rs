use crate::tools::mkvtoolnix::{self, Track};
use crate::tools::pysubs2;
use crate::tools::ToolRegistry;
use crate::config::AppConfig;
use crate::jobs::{Job, JobStatus, JobType, SharedJobQueue, JobControlRegistry};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackInfo {
    pub id: u64,
    pub track_type: String,
    pub codec: String,
    pub language: String,
    pub track_name: Option<String>,
    pub default_track: bool,
    pub forced_track: bool,
    pub hearing_impaired: bool,
}

impl From<&Track> for TrackInfo {
    fn from(t: &Track) -> Self {
        let props = t.properties.as_ref();
        Self {
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
    }
}

#[tauri::command]
pub fn get_tool_registry() -> ToolRegistry {
    ToolRegistry::discover()
}

#[tauri::command]
pub fn load_config() -> AppConfig {
    AppConfig::load()
}

#[tauri::command]
pub fn save_config(config: AppConfig) -> Result<(), String> {
    config.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn enumerate_tracks(file_path: String) -> Result<Vec<TrackInfo>, String> {
    let info = mkvtoolnix::enumerate_tracks(&file_path).map_err(|e| e.to_string())?;
    let tracks: Vec<TrackInfo> = info.tracks.iter().map(|t| t.into()).collect();
    Ok(tracks)
}

#[tauri::command]
pub fn extract_subtitle(
    file_path: String,
    track_id: u64,
    output_path: String,
) -> Result<String, String> {
    mkvtoolnix::extract_subtitle_track(&file_path, track_id, &output_path)
        .map_err(|e| e.to_string())?;
    Ok(output_path)
}

#[tauri::command]
pub fn extract_all_subtitles(
    file_path: String,
    output_dir: String,
) -> Result<Vec<String>, String> {
    mkvtoolnix::extract_all_subtitles(&file_path, &output_dir)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn merge_subtitle_into_video(
    app: AppHandle,
    queue: State<'_, SharedJobQueue>,
    controls: State<'_, JobControlRegistry>,
    video_path: String,
    subtitle_path: String,
    output_path: String,
    language: Option<String>,
) -> Result<String, String> {
    let job_id = format!("merge-{}", uuid::Uuid::new_v4());

    {
        let mut q = queue.lock().map_err(|e| e.to_string())?;
        q.add(Job {
            id: job_id.clone(),
            job_type: JobType::MergeSubtitle,
            status: JobStatus::Pending,
            input_file: video_path.clone(),
            output_file: Some(output_path.clone()),
            progress: 0.0,
            message: Some("Queued".to_string()),
        });
    }

    let _ = app.emit("job-started", &job_id);

    let app_clone = app.clone();
    let queue_clone = queue.inner().clone();
    let controls_clone = controls.inner().clone();
    let job_id_clone = job_id.clone();

    tokio::spawn(async move {
        let control = Arc::new(crate::jobs::JobControl::new());
        {
            let mut registry = controls_clone.lock().unwrap();
            registry.insert(job_id_clone.clone(), control.clone());
        }

        {
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, JobStatus::Running);
        }

        let subs = vec![mkvtoolnix::SubtitleMergeSpec::new(subtitle_path, language)];
        let result = mkvtoolnix::merge_subtitles_async(&video_path, &subs, &output_path, Some(&control), |pct| {
            let mut q = queue_clone.lock().unwrap();
            q.update_progress(&job_id_clone, pct, None);
            let _ = app_clone.emit("job-progress", serde_json::json!({
                "job_id": job_id_clone,
                "progress": pct,
            }));
        }).await;

        if let Err(e) = result {
            let cancelled = matches!(e, mkvtoolnix::MkvError::Cancelled);
            let msg = e.to_string();
            {
                let mut q = queue_clone.lock().unwrap();
                q.update_status(&job_id_clone, if cancelled { JobStatus::Cancelled } else { JobStatus::Failed });
                q.update_progress(&job_id_clone, 0.0, Some(msg.clone()));
            }
            let _ = app_clone.emit("job-completed", serde_json::json!({
                "job_id": job_id_clone,
                "status": if cancelled { "cancelled" } else { "failed" },
                "message": msg,
            }));
            let mut registry = controls_clone.lock().unwrap();
            registry.remove(&job_id_clone);
            return;
        }

        {
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, JobStatus::Completed);
            q.update_progress(&job_id_clone, 100.0, Some("Merge complete".to_string()));
        }
        let _ = app_clone.emit("job-completed", serde_json::json!({
            "job_id": job_id_clone,
            "status": "completed",
            "message": "Merge complete",
            "output_path": output_path,
        }));

        let mut registry = controls_clone.lock().unwrap();
        registry.remove(&job_id_clone);
    });

    Ok(job_id)
}

#[tauri::command]
pub fn remove_subtitle_tracks(
    file_path: String,
    output_path: String,
) -> Result<(), String> {
    mkvtoolnix::remove_all_subtitles(&file_path, &output_path)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn convert_subtitle_format(
    input: String,
    output: String,
    format: String,
) -> Result<(), String> {
    pysubs2::convert_format(&input, &output, &format).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn shift_subtitle_timing(
    input: String,
    output: String,
    milliseconds: i64,
) -> Result<(), String> {
    pysubs2::shift_subtitles(&input, &output, milliseconds).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn stretch_subtitle_timing(
    input: String,
    output: String,
    from_fps: f64,
    to_fps: f64,
) -> Result<(), String> {
    pysubs2::stretch_framerate(&input, &output, from_fps, to_fps).map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
pub struct SetTrackMetaRequest {
    pub file_path: String,
    pub track_id: u64,
    pub track_name: Option<String>,
    pub default_flag: Option<bool>,
    pub forced_flag: Option<bool>,
}

#[tauri::command]
pub async fn set_track_metadata(request: SetTrackMetaRequest) -> Result<(), String> {
    let mkvpropedit = crate::tools::which("mkvpropedit")
        .ok_or_else(|| "mkvpropedit not found in PATH".to_string())?;

    let mut args: Vec<String> = vec![request.file_path.clone()];
    args.push("--edit".to_string());
    // mkvpropedit uses 1-indexed @N notation for track selection
    args.push(format!("track:@{}", request.track_id + 1));

    if let Some(name) = &request.track_name {
        args.push("--set".to_string());
        args.push(format!("name={}", name));
    }
    if let Some(default) = request.default_flag {
        args.push("--set".to_string());
        args.push(format!("flag-default={}", if default { 1 } else { 0 }));
    }
    if let Some(forced) = request.forced_flag {
        args.push("--set".to_string());
        args.push(format!("flag-forced={}", if forced { 1 } else { 0 }));
    }

    let output = std::process::Command::new(&mkvpropedit)
        .args(&args)
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    Ok(())
}
