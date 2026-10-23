use subforge_deps::HideWindow;
use crate::jobs::{Job, JobStatus, JobType, SharedJobQueue};
use crate::tools::whisper::{self, TranscribeOptions, WhisperInfo};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[tauri::command]
pub async fn get_whisper_info() -> Result<WhisperInfo, String> {
    super::blocking(|| whisper::get_whisper_info().map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn list_whisper_models(cache_dir: Option<String>) -> Result<Vec<String>, String> {
    super::blocking(move || whisper::list_models(cache_dir).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn download_whisper_model(model: String, cache_dir: Option<String>) -> Result<(), String> {
    super::blocking(move || whisper::download_model(&model, cache_dir).map_err(|e| e.to_string())).await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscribeRequest {
    pub input: String,
    pub output: String,
    pub model: String,
    pub device: String,
    pub language: Option<String>,
    pub format: String,
    pub word_timestamps: bool,
    pub vad_filter: bool,
    pub cache_dir: Option<String>,
}

#[tauri::command]
pub async fn transcribe_audio(
    app: AppHandle,
    queue: State<'_, SharedJobQueue>,
    request: TranscribeRequest,
) -> Result<String, String> {
    let job_id = format!("transcribe-{}", uuid::Uuid::new_v4());

    let options = TranscribeOptions {
        input: request.input.clone(),
        output: request.output.clone(),
        model: request.model,
        device: request.device,
        language: request.language,
        format: request.format,
        word_timestamps: request.word_timestamps,
        vad_filter: request.vad_filter,
        cache_dir: request.cache_dir,
    };

    {
        let mut q = queue.lock().map_err(|e| e.to_string())?;
        q.add(Job {
            id: job_id.clone(),
            job_type: JobType::Transcribe,
            status: JobStatus::Pending,
            input_file: request.input,
            output_file: Some(request.output.clone()),
            progress: 0.0,
            message: Some("Queued".to_string()),
        });
    }

    let _ = app.emit("job-started", &job_id);

    let args = whisper::build_transcribe_args(&options);
    let python = crate::tools::find_python().ok_or("Python not found")?;

    let app_clone = app.clone();
    let queue_clone = queue.inner().clone();
    let job_id_clone = job_id.clone();

    tokio::spawn(async move {
        let mut child = match Command::new(&python).hide_window()
            .args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let mut q = queue_clone.lock().unwrap();
                q.update_status(&job_id_clone, JobStatus::Failed);
                q.update_progress(&job_id_clone, 0.0, Some(e.to_string()));
                let _ = app_clone.emit("job-completed", serde_json::json!({
                    "job_id": job_id_clone,
                    "status": "failed",
                    "message": e.to_string(),
                }));
                return;
            }
        };

        {
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, JobStatus::Running);
        }

        // Read stdout for progress lines
        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(progress) = serde_json::from_str::<serde_json::Value>(&line) {
                    if progress.get("type").and_then(|v| v.as_str()) == Some("progress") {
                        let pct = progress.get("percent").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let msg = progress.get("message").and_then(|v| v.as_str()).map(|s| s.to_string());
                        {
                            let mut q = queue_clone.lock().unwrap();
                            q.update_progress(&job_id_clone, pct, msg.clone());
                        }
                        let _ = app_clone.emit("job-progress", serde_json::json!({
                            "job_id": job_id_clone,
                            "progress": pct,
                            "message": msg,
                        }));
                    }
                }
            }
        }

        let status = match child.wait().await {
            Ok(s) if s.success() => JobStatus::Completed,
            _ => {
                // Try to read stderr for error message
                let mut stderr_msg = "Transcription failed".to_string();
                if let Some(stderr) = child.stderr {
                    let reader = BufReader::new(stderr);
                    let mut lines = reader.lines();
                    let mut errs = Vec::new();
                    while let Ok(Some(line)) = lines.next_line().await {
                        errs.push(line);
                    }
                    if !errs.is_empty() {
                        stderr_msg = errs.join("\n");
                    }
                }
                {
                    let mut q = queue_clone.lock().unwrap();
                    q.update_status(&job_id_clone, JobStatus::Failed);
                    q.update_progress(&job_id_clone, 0.0, Some(stderr_msg.clone()));
                }
                let _ = app_clone.emit("job-completed", serde_json::json!({
                    "job_id": job_id_clone,
                    "status": "failed",
                    "message": stderr_msg,
                }));
                return;
            }
        };

        {
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, status.clone());
            q.update_progress(&job_id_clone, 100.0, Some("Done".to_string()));
        }

        let _ = app_clone.emit("job-completed", serde_json::json!({
            "job_id": job_id_clone,
            "status": status,
            "message": "Done",
        }));
    });

    Ok(job_id)
}
