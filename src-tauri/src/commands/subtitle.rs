use super::{blocking, CommandError};
use crate::config::AppConfig;
use crate::jobs::{Job, JobControlRegistry, JobStatus, JobType, SharedJobQueue};
use crate::tools::{tool_paths, ToolRegistry};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use subforge_core::subtitle::{self, Format};
use subforge_core::tools::mkvtoolnix::{self, Track};
use subforge_core::tools::{tool_status, ToolError, ToolStatus};
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

/// Legacy shape for the current Settings UI (built from the core's tool status).
#[tauri::command]
pub async fn get_tool_registry() -> Result<ToolRegistry, String> {
    blocking(|| Ok(ToolRegistry::discover())).await
}

#[tauri::command]
pub async fn get_tool_status() -> Result<Vec<ToolStatus>, String> {
    blocking(|| Ok(tool_status(&tool_paths()))).await
}

#[tauri::command]
pub fn load_config() -> AppConfig {
    AppConfig::load()
}

#[tauri::command]
pub fn save_config(mut config: AppConfig) -> Result<(), String> {
    // A client that doesn't know about `config_version` sends 0; never let that
    // re-arm the one-time path migration.
    if let Some(existing) = AppConfig::try_load() {
        config.config_version = config.config_version.max(existing.config_version);
    }
    config.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn enumerate_tracks(file_path: String) -> Result<Vec<TrackInfo>, CommandError> {
    blocking(move || {
        let info =
            mkvtoolnix::enumerate_tracks(&tool_paths(), &file_path).map_err(CommandError::from)?;
        Ok(info.tracks.iter().map(|t| t.into()).collect())
    })
    .await
}

#[tauri::command]
pub async fn extract_subtitle(
    file_path: String,
    track_id: u64,
    output_path: String,
) -> Result<String, CommandError> {
    blocking(move || {
        mkvtoolnix::extract_subtitle_track(&tool_paths(), &file_path, track_id, &output_path)
            .map_err(CommandError::from)?;
        Ok(output_path)
    })
    .await
}

#[tauri::command]
pub async fn extract_all_subtitles(
    file_path: String,
    output_dir: String,
) -> Result<Vec<String>, CommandError> {
    blocking(move || {
        mkvtoolnix::extract_all_subtitles(&tool_paths(), &file_path, &output_dir)
            .map_err(CommandError::from)
    })
    .await
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
    let paths = tool_paths();

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
        let result = mkvtoolnix::merge_subtitles_async(
            &paths,
            &video_path,
            &subs,
            &output_path,
            Some(control.token()),
            |pct| {
                let mut q = queue_clone.lock().unwrap();
                q.update_progress(&job_id_clone, pct, None);
                let _ = app_clone.emit(
                    "job-progress",
                    serde_json::json!({
                        "job_id": job_id_clone,
                        "progress": pct,
                    }),
                );
            },
        )
        .await;

        if let Err(e) = result {
            let cancelled = matches!(e, ToolError::Cancelled);
            let msg = e.to_string();
            {
                let mut q = queue_clone.lock().unwrap();
                q.update_status(
                    &job_id_clone,
                    if cancelled {
                        JobStatus::Cancelled
                    } else {
                        JobStatus::Failed
                    },
                );
                q.update_progress(&job_id_clone, 0.0, Some(msg.clone()));
            }
            let _ = app_clone.emit(
                "job-completed",
                serde_json::json!({
                    "job_id": job_id_clone,
                    "status": if cancelled { "cancelled" } else { "failed" },
                    "message": msg,
                    "details": e.details(),
                }),
            );
            let mut registry = controls_clone.lock().unwrap();
            registry.remove(&job_id_clone);
            return;
        }

        {
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, JobStatus::Completed);
            q.update_progress(&job_id_clone, 100.0, Some("Merge complete".to_string()));
        }
        let _ = app_clone.emit(
            "job-completed",
            serde_json::json!({
                "job_id": job_id_clone,
                "status": "completed",
                "message": "Merge complete",
                "output_path": output_path,
            }),
        );

        let mut registry = controls_clone.lock().unwrap();
        registry.remove(&job_id_clone);
    });

    Ok(job_id)
}

#[tauri::command]
pub async fn remove_subtitle_tracks(
    file_path: String,
    output_path: String,
) -> Result<(), CommandError> {
    blocking(move || {
        mkvtoolnix::remove_all_subtitles(&tool_paths(), &file_path, &output_path)
            .map_err(CommandError::from)
    })
    .await
}

fn parse_format(format: &str) -> Result<Format, String> {
    Format::from_extension(format).ok_or_else(|| {
        format!("Unsupported subtitle format '{format}'. Use one of: srt, ass, ssa, vtt.")
    })
}

#[tauri::command]
pub async fn convert_subtitle_format(
    input: String,
    output: String,
    format: String,
) -> Result<(), String> {
    let to = parse_format(&format)?;
    blocking(move || {
        subtitle::convert_file(Path::new(&input), Path::new(&output), to).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn shift_subtitle_timing(
    input: String,
    output: String,
    milliseconds: i64,
) -> Result<(), String> {
    blocking(move || {
        subtitle::shift_file(Path::new(&input), Path::new(&output), milliseconds)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn stretch_subtitle_timing(
    input: String,
    output: String,
    from_fps: f64,
    to_fps: f64,
) -> Result<(), String> {
    blocking(move || {
        subtitle::stretch_file(Path::new(&input), Path::new(&output), from_fps, to_fps)
            .map_err(|e| e.to_string())
    })
    .await
}

#[derive(serde::Deserialize)]
pub struct SetTrackMetaRequest {
    pub file_path: String,
    pub track_id: u64,
    pub track_name: Option<String>,
    pub default_flag: Option<bool>,
    pub forced_flag: Option<bool>,
}

/// mkvpropedit `--set` pairs for the fields the request changes.
fn track_flag_props(request: &SetTrackMetaRequest) -> Vec<(String, String)> {
    let mut props = Vec::new();
    if let Some(name) = &request.track_name {
        props.push(("name".to_string(), name.clone()));
    }
    if let Some(default) = request.default_flag {
        props.push(("flag-default".to_string(), u8::from(default).to_string()));
    }
    if let Some(forced) = request.forced_flag {
        props.push(("flag-forced".to_string(), u8::from(forced).to_string()));
    }
    props
}

#[tauri::command]
pub async fn set_track_metadata(request: SetTrackMetaRequest) -> Result<(), CommandError> {
    blocking(move || {
        let props = track_flag_props(&request);
        // mkvpropedit uses 1-indexed track:@N selection.
        let number = u32::try_from(request.track_id + 1).map_err(|e| e.to_string())?;
        mkvtoolnix::set_track_flags(
            &tool_paths(),
            &PathBuf::from(&request.file_path),
            number,
            &props,
        )
        .map_err(CommandError::from)
    })
    .await
}


#[derive(Debug, Clone, Deserialize)]
pub struct CleanSubtitleRequest {
    pub input: String,
    /// Destination when `apply` is true. Defaults to `{name}.clean.{ext}` next
    /// to the input; using the input path itself keeps a `.bak` copy.
    pub output: Option<String>,
    /// Cue indexes the user chose to keep in the preview.
    #[serde(default)]
    pub keep: Vec<usize>,
    /// `false` (default) only reports what would be removed.
    #[serde(default)]
    pub apply: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanSubtitleResponse {
    pub removed: Vec<subforge_core::subtitle::clean::RemovedCue>,
    pub kept: usize,
    /// Set when a file was written.
    pub output_path: Option<String>,
    /// Set when the original was copied before being overwritten.
    pub backup_path: Option<String>,
}

#[tauri::command]
pub async fn clean_subtitle(
    request: CleanSubtitleRequest,
) -> Result<CleanSubtitleResponse, String> {
    blocking(move || clean_subtitle_at(&request)).await
}

fn clean_subtitle_at(
    request: &CleanSubtitleRequest,
) -> Result<CleanSubtitleResponse, String> {
    use subforge_core::subtitle::clean::{clean_file_to, CleanOptions};

    let outcome = clean_file_to(
        Path::new(&request.input),
        request.output.as_deref().map(Path::new),
        &CleanOptions {
            keep: request.keep.clone(),
        },
        request.apply,
    )
    .map_err(|e| e.to_string())?;

    let to_string = |p: Option<std::path::PathBuf>| {
        p.map(|p| p.to_string_lossy().into_owned())
    };
    Ok(CleanSubtitleResponse {
        removed: outcome.report.removed,
        kept: outcome.report.kept,
        output_path: to_string(outcome.output),
        backup_path: to_string(outcome.backup),
    })
}



#[derive(Debug, Clone, Deserialize)]
pub struct SyncCheckRequest {
    pub subtitle: String,
    pub video: String,
    /// Also transcribe short audio windows (needs a downloaded speech model).
    #[serde(default)]
    pub deep: bool,
    /// Whisper model for the deep check; defaults to the smallest downloaded.
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncCheckResponse {
    pub check: subforge_core::subtitle::sync::SyncCheck,
    pub deep: Option<subforge_core::subtitle::sync::DeepCheck>,
    pub model: Option<String>,
}

#[tauri::command]
pub async fn check_subtitle_sync(
    request: SyncCheckRequest,
) -> Result<SyncCheckResponse, String> {
    blocking(move || check_subtitle_sync_at(request)).await
}

fn check_subtitle_sync_at(request: SyncCheckRequest) -> Result<SyncCheckResponse, String> {
    use subforge_core::subtitle::sync;

    let paths = tool_paths();
    let doc = subtitle::SubtitleDoc::load(Path::new(&request.subtitle))
        .map_err(|e| e.to_string())?;
    let media = subforge_core::tools::ffmpeg::probe_media(&paths, &request.video)
        .map_err(|e| e.to_string())?;
    let check = sync::check(&doc, &media);

    if !request.deep {
        return Ok(SyncCheckResponse {
            check,
            deep: None,
            model: None,
        });
    }

    let cache = subforge_core::whisper::resolve_cache_dir(None);
    let downloaded = subforge_core::whisper::list_downloaded(&cache);
    let model = match request.model.filter(|m| !m.trim().is_empty()) {
        Some(model) => {
            if !downloaded.iter().any(|m| m == &model) {
                return Err(format!(
                    "The speech model '{model}' is not downloaded. Download it on the Speech to Text page first."
                ));
            }
            model
        }
        None => downloaded.first().cloned().ok_or_else(|| {
            "No speech model is downloaded. Download one on the Speech to Text page, then run the deep check again."
                .to_string()
        })?,
    };

    let mut progress = |_pct: f64, _msg: Option<&str>| {};
    let deep = sync::deep_check(
        &paths,
        Path::new(&request.video),
        &doc,
        &model,
        None,
        &mut progress,
        subforge_core::CancellationToken::new(),
    )
    .map_err(|e| e.to_string())?;

    Ok(SyncCheckResponse {
        check,
        deep: Some(deep),
        model: Some(model),
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct SyncFixRequest {
    pub input: String,
    pub output: Option<String>,
    /// Frame-rate correction, applied before the shift.
    pub from_fps: Option<f64>,
    pub to_fps: Option<f64>,
    /// Constant offset to apply after the frame-rate correction.
    pub shift_ms: Option<i64>,
    /// Write over the input instead of `{name}.synced.{ext}` (keeps a `.bak`).
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncFixResponse {
    pub output_path: String,
    pub backup_path: Option<String>,
    pub cues: usize,
}

#[tauri::command]
pub async fn fix_subtitle_sync(request: SyncFixRequest) -> Result<SyncFixResponse, String> {
    blocking(move || fix_subtitle_sync_at(&request)).await
}

fn fix_subtitle_sync_at(request: &SyncFixRequest) -> Result<SyncFixResponse, String> {
    use subforge_core::subtitle::sync::{fix_file, SyncFix};

    let outcome = fix_file(
        Path::new(&request.input),
        request.output.as_deref().map(Path::new),
        request.overwrite,
        &SyncFix {
            from_fps: request.from_fps,
            to_fps: request.to_fps,
            shift_ms: request.shift_ms,
        },
    )
    .map_err(|e| e.to_string())?;

    Ok(SyncFixResponse {
        output_path: outcome.output.to_string_lossy().into_owned(),
        backup_path: outcome
            .backup
            .map(|p| p.to_string_lossy().into_owned()),
        cues: outcome.cues,
    })
}


#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("subforge_sub_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const ASS: &str = "[Script Info]\nScriptType: v4.00+\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:02.50,Default,,0,0,0,,Hello world\nDialogue: 0,0:00:03.00,0:00:04.00,Default,,0,0,0,,Second line\n";

    #[tokio::test]
    async fn convert_command_writes_srt_from_ass() {
        let dir = temp_dir();
        let input = dir.join("in.ass");
        let output = dir.join("out.srt");
        std::fs::write(&input, ASS).unwrap();

        convert_subtitle_format(
            input.to_string_lossy().into(),
            output.to_string_lossy().into(),
            "srt".into(),
        )
        .await
        .unwrap();

        let srt = std::fs::read_to_string(&output).unwrap();
        assert!(srt.contains("00:00:01,000 --> 00:00:02,500"), "{srt}");
        assert!(srt.contains("Hello world"), "{srt}");
        assert!(srt.contains("Second line"), "{srt}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn convert_command_rejects_unknown_format() {
        let err = convert_subtitle_format("a.ass".into(), "b.x".into(), "docx".into())
            .await
            .unwrap_err();
        assert!(err.contains("docx"), "{err}");
    }

    #[tokio::test]
    async fn shift_command_moves_cues() {
        let dir = temp_dir();
        let input = dir.join("in.ass");
        let output = dir.join("out.ass");
        std::fs::write(&input, ASS).unwrap();

        shift_subtitle_timing(
            input.to_string_lossy().into(),
            output.to_string_lossy().into(),
            1000,
        )
        .await
        .unwrap();

        let out = std::fs::read_to_string(&output).unwrap();
        assert!(out.contains("0:00:02.00,0:00:03.50"), "{out}");
        std::fs::remove_dir_all(dir).unwrap();
    }

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
    const CLEAN_SRT: &str = "1\n00:00:01,000 --> 00:00:03,000\nترجمة: فريق المثال\n\n2\n00:00:04,000 --> 00:00:06,000\nمرحبا\n\n3\n00:00:07,000 --> 00:00:09,000\nwww.example.com\n";

    #[test]
    fn clean_preview_reports_but_writes_nothing() {
        let dir = temp_dir();
        let input = dir.join("in.srt");
        std::fs::write(&input, CLEAN_SRT).unwrap();

        let response = clean_subtitle_at(&CleanSubtitleRequest {
            input: input.to_string_lossy().into(),
            output: None,
            keep: vec![],
            apply: false,
        })
        .unwrap();

        assert_eq!(response.removed.len(), 2);
        assert_eq!(response.kept, 1);
        assert_eq!(response.output_path, None);
        // Only the source file exists.
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn clean_apply_writes_a_copy_by_default() {
        let dir = temp_dir();
        let input = dir.join("episode.srt");
        std::fs::write(&input, CLEAN_SRT).unwrap();

        let response = clean_subtitle_at(&CleanSubtitleRequest {
            input: input.to_string_lossy().into(),
            output: None,
            keep: vec![],
            apply: true,
        })
        .unwrap();

        let output = response.output_path.unwrap();
        assert!(output.ends_with("episode.clean.srt"), "{output}");
        let text = std::fs::read_to_string(&output).unwrap();
        assert!(!text.contains("ترجمة"));
        assert!(!text.contains("example.com"));
        assert!(text.contains("مرحبا"));
        // The original is untouched.
        assert_eq!(std::fs::read_to_string(&input).unwrap(), CLEAN_SRT);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn overwriting_the_original_keeps_a_backup() {
        let dir = temp_dir();
        let input = dir.join("episode.srt");
        std::fs::write(&input, CLEAN_SRT).unwrap();

        let response = clean_subtitle_at(&CleanSubtitleRequest {
            input: input.to_string_lossy().into(),
            output: Some(input.to_string_lossy().into()),
            keep: vec![0],
            apply: true,
        })
        .unwrap();

        let backup = response.backup_path.expect("backup path");
        assert_eq!(std::fs::read_to_string(backup).unwrap(), CLEAN_SRT);
        // Cue 0 was kept, cue 2 removed.
        let text = std::fs::read_to_string(&input).unwrap();
        assert!(text.contains("ترجمة: فريق المثال"));
        assert!(!text.contains("example.com"));
        std::fs::remove_dir_all(dir).unwrap();
    }

}
