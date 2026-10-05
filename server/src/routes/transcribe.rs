use subforge_deps::HideWindow;
use crate::tools::whisper;
use crate::{jobs, AppState};
use axum::{extract::Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Serialize)]
pub struct WhisperInfoResp {
    faster_whisper_version: String,
    cuda_available: bool,
    devices: Vec<String>,
}

pub async fn get_whisper_info() -> Result<Json<WhisperInfoResp>, StatusCode> {
    let info = crate::error::blocking(whisper::get_whisper_info)
        .await?
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(Json(WhisperInfoResp {
        faster_whisper_version: info.faster_whisper_version,
        cuda_available: info.cuda_available,
        devices: info.devices,
    }))
}

#[derive(Deserialize)]
pub struct ListModelsRequest {
    pub cache_dir: Option<String>,
}

pub async fn list_whisper_models(
    Json(req): Json<ListModelsRequest>,
) -> Result<Json<Vec<String>>, StatusCode> {
    let models = crate::error::blocking(move || whisper::list_models(req.cache_dir))
        .await?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(models))
}

#[derive(Deserialize)]
pub struct DownloadRequest {
    model: String,
    cache_dir: Option<String>,
}

pub async fn download_whisper_model(
    Json(req): Json<DownloadRequest>,
) -> Result<StatusCode, StatusCode> {
    crate::error::blocking(move || whisper::download_model(&req.model, req.cache_dir))
        .await?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct TranscribeRequest {
    input: String,
    output: String,
    model: String,
    device: String,
    language: Option<String>,
    format: String,
    word_timestamps: bool,
    vad_filter: bool,
    cache_dir: Option<String>,
}

pub async fn transcribe_audio(
    State(state): State<AppState>,
    Json(req): Json<TranscribeRequest>,
) -> Result<Json<String>, StatusCode> {
    let job_id = format!("transcribe-{}", uuid::Uuid::new_v4());

    let options = whisper::TranscribeOptions {
        input: req.input.clone(),
        output: req.output.clone(),
        model: req.model,
        device: req.device,
        language: req.language,
        format: req.format,
        word_timestamps: req.word_timestamps,
        vad_filter: req.vad_filter,
        cache_dir: req.cache_dir,
    };

    {
        let mut q = state.job_queue.lock().unwrap();
        q.add(jobs::Job {
            id: job_id.clone(),
            job_type: jobs::JobType::Transcribe,
            status: jobs::JobStatus::Pending,
            input_file: req.input,
            output_file: Some(req.output),
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
    let job_id_clone = job_id.clone();

    tokio::spawn(async move {
        // Gracefully handle missing Python instead of panicking
        let python = match crate::tools::find_python() {
            Some(p) => p,
            None => {
                let mut q = queue_clone.lock().unwrap();
                q.update_status(&job_id_clone, jobs::JobStatus::Failed);
                jobs::emit_event(
                    &sender,
                    jobs::JobEvent {
                        event_type: "job-completed".to_string(),
                        job_id: job_id_clone,
                        status: Some(jobs::JobStatus::Failed),
                        progress: Some(0.0),
                        message: Some("Python not found in PATH".to_string()),
                        stage: None,
                        output_path: None,
                    },
                );
                return;
            }
        };

        let args = whisper::build_transcribe_args(&options);

        let mut child = match Command::new(&python).hide_window()
            .args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let mut q = queue_clone.lock().unwrap();
                q.update_status(&job_id_clone, jobs::JobStatus::Failed);
                jobs::emit_event(
                    &sender,
                    jobs::JobEvent {
                        event_type: "job-completed".to_string(),
                        job_id: job_id_clone,
                        status: Some(jobs::JobStatus::Failed),
                        progress: Some(0.0),
                        message: Some(e.to_string()),
                        stage: None,
                        output_path: None,
                    },
                );
                return;
            }
        };

        {
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, jobs::JobStatus::Running);
        }

        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(progress) = serde_json::from_str::<serde_json::Value>(&line) {
                    if progress.get("type").and_then(|v| v.as_str()) == Some("progress") {
                        let pct =
                            progress.get("percent").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let msg = progress
                            .get("message")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        {
                            let mut q = queue_clone.lock().unwrap();
                            q.update_progress(&job_id_clone, pct, msg.clone());
                        }
                        jobs::emit_event(
                            &sender,
                            jobs::JobEvent {
                                event_type: "job-progress".to_string(),
                                job_id: job_id_clone.clone(),
                                status: None,
                                progress: Some(pct),
                                message: msg,
                                stage: None,
                                output_path: None,
                            },
                        );
                    }
                }
            }
        }

        let status = match child.wait().await {
            Ok(s) if s.success() => jobs::JobStatus::Completed,
            _ => {
                let mut q = queue_clone.lock().unwrap();
                q.update_status(&job_id_clone, jobs::JobStatus::Failed);
                jobs::emit_event(
                    &sender,
                    jobs::JobEvent {
                        event_type: "job-completed".to_string(),
                        job_id: job_id_clone.clone(),
                        status: Some(jobs::JobStatus::Failed),
                        progress: Some(0.0),
                        message: Some("Transcription failed".to_string()),
                        stage: None,
                        output_path: None,
                    },
                );
                return;
            }
        };

        {
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, status.clone());
            q.update_progress(&job_id_clone, 100.0, Some("Done".to_string()));
        }
        jobs::emit_event(
            &sender,
            jobs::JobEvent {
                event_type: "job-completed".to_string(),
                job_id: job_id_clone,
                status: Some(status),
                progress: Some(100.0),
                message: Some("Done".to_string()),
                stage: None,
                output_path: None,
            },
        );
    });

    Ok(Json(job_id))
}
