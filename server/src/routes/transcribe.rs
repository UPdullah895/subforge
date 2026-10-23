//! Speech-to-text routes: whisper.cpp engine info, model catalog/download jobs and
//! transcription jobs. Mirrors `src-tauri/src/commands/transcribe.rs`.

use crate::error::{err400, err500, AppError};
use crate::jobs::{
    self, JobControl, JobControlRegistry, JobEvent, JobStatus, JobType, SharedJobQueue,
};
use crate::AppState;
use axum::{extract::Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use subforge_core::tools::ToolPaths;
use subforge_core::whisper::{
    self, default_cache_dir, legacy_cache_dir, list_downloaded, spec_for, EngineInfo, ModelSpec,
    TranscribeOptions, WhisperError, DEFAULT_BASE_URL, MODELS, VAD_MODEL,
};

/// Model downloads in flight, keyed by cache dir + model name, to the job id doing it.
pub type ActiveDownloads = Arc<Mutex<HashMap<String, String>>>;

#[derive(Clone)]
pub(crate) struct JobCtx {
    pub queue: SharedJobQueue,
    pub controls: JobControlRegistry,
    pub events: jobs::EventSender,
}

impl JobCtx {
    fn from_state(state: &AppState) -> Self {
        Self {
            queue: state.job_queue.clone(),
            controls: state.job_controls.clone(),
            events: state.event_sender.clone(),
        }
    }

    fn emit(
        &self,
        event_type: &str,
        job_id: &str,
        status: Option<JobStatus>,
        progress: Option<f64>,
        message: Option<String>,
        output_path: Option<String>,
    ) {
        jobs::emit_event(
            &self.events,
            JobEvent {
                event_type: event_type.to_string(),
                job_id: job_id.to_string(),
                status,
                progress,
                message,
                stage: None,
                output_path,
            },
        );
    }

    fn begin(
        &self,
        job_id: &str,
        job_type: JobType,
        input: String,
        output: Option<String>,
    ) -> Arc<JobControl> {
        let control = Arc::new(JobControl::new());
        self.queue.lock().unwrap().add(jobs::Job {
            id: job_id.to_string(),
            job_type,
            status: JobStatus::Pending,
            input_file: input,
            output_file: output,
            progress: 0.0,
            message: Some("Queued".to_string()),
        });
        // Registered before the task starts so a cancel right after the reply is never lost.
        self.controls
            .lock()
            .unwrap()
            .insert(job_id.to_string(), control.clone());
        self.emit("job-started", job_id, None, None, None, None);
        control
    }

    fn running(&self, job_id: &str) {
        self.queue
            .lock()
            .unwrap()
            .update_status(job_id, JobStatus::Running);
    }

    fn progress(&self, job_id: &str, pct: f64, message: Option<&str>) {
        let message = message.map(str::to_string);
        self.queue
            .lock()
            .unwrap()
            .update_progress(job_id, pct, message.clone());
        self.emit("job-progress", job_id, None, Some(pct), message, None);
    }

    fn finish(&self, job_id: &str, result: Result<(String, Option<String>), WhisperError>) {
        let (status, message, output_path) = match result {
            Ok((message, output)) => (JobStatus::Completed, message, output),
            Err(e) => {
                let status = if matches!(e, WhisperError::Cancelled) {
                    JobStatus::Cancelled
                } else {
                    JobStatus::Failed
                };
                (status, e.to_string(), None)
            }
        };
        let pct = if status == JobStatus::Completed {
            100.0
        } else {
            0.0
        };
        {
            let mut q = self.queue.lock().unwrap();
            q.update_status(job_id, status.clone());
            q.update_progress(job_id, pct, Some(message.clone()));
        }
        self.controls.lock().unwrap().remove(job_id);
        self.emit(
            "job-completed",
            job_id,
            Some(status),
            Some(pct),
            Some(message),
            output_path,
        );
    }
}

fn cache_dir_or_default(cache_dir: Option<String>) -> PathBuf {
    cache_dir
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_cache_dir)
}

pub async fn get_whisper_info() -> Json<EngineInfo> {
    Json(whisper::engine_info())
}

#[derive(Deserialize)]
pub struct ListModelsRequest {
    pub cache_dir: Option<String>,
}

pub async fn list_whisper_models(
    Json(req): Json<ListModelsRequest>,
) -> Result<Json<Vec<String>>, StatusCode> {
    let cache = cache_dir_or_default(req.cache_dir);
    let models = crate::error::blocking(move || list_downloaded(&cache)).await?;
    Ok(Json(models))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CatalogEntry {
    pub name: String,
    pub size_mb: u32,
}

pub async fn list_whisper_catalog() -> Json<Vec<CatalogEntry>> {
    Json(
        MODELS
            .iter()
            .map(|m| CatalogEntry {
                name: m.name.to_string(),
                size_mb: m.size_mb,
            })
            .collect(),
    )
}

/// Starts (or joins) a download job for `spec`. Returns the job id immediately.
pub(crate) fn start_download(
    ctx: JobCtx,
    downloads: ActiveDownloads,
    spec: ModelSpec,
    vad: Option<ModelSpec>,
    cache: PathBuf,
    base_url: String,
) -> String {
    let key = format!("{}|{}", cache.display(), spec.name);
    let job_id = {
        let mut active = downloads.lock().unwrap();
        if let Some(existing) = active.get(&key) {
            return existing.clone();
        }
        let id = format!("download-{}", uuid::Uuid::new_v4());
        active.insert(key.clone(), id.clone());
        id
    };
    let control = ctx.begin(
        &job_id,
        JobType::DownloadModel,
        spec.name.to_string(),
        Some(
            whisper::model_path(&cache, &spec)
                .to_string_lossy()
                .into_owned(),
        ),
    );

    let id = job_id.clone();
    tokio::spawn(async move {
        ctx.running(&id);
        let mut on_progress = |pct: f64, msg: Option<&str>| ctx.progress(&id, pct, msg);
        let result = whisper::download_with(
            &spec,
            vad.as_ref(),
            &cache,
            &base_url,
            &mut on_progress,
            control.token().clone(),
        )
        .await
        .map(|path| {
            (
                format!("Model '{}' downloaded", spec.name),
                Some(path.to_string_lossy().into_owned()),
            )
        });
        downloads.lock().unwrap().remove(&key);
        ctx.finish(&id, result);
    });
    job_id
}

#[derive(Deserialize)]
pub struct DownloadRequest {
    model: String,
    cache_dir: Option<String>,
}

pub async fn download_whisper_model(
    State(state): State<AppState>,
    Json(req): Json<DownloadRequest>,
) -> Result<Json<String>, AppError> {
    let spec = *spec_for(&req.model)
        .ok_or_else(|| err400(WhisperError::UnknownModel(req.model.clone())))?;
    Ok(Json(start_download(
        JobCtx::from_state(&state),
        state.whisper_downloads.clone(),
        spec,
        Some(VAD_MODEL),
        cache_dir_or_default(req.cache_dir),
        DEFAULT_BASE_URL.to_string(),
    )))
}

/// Path of the old faster-whisper model folder, if it still exists.
pub async fn whisper_legacy_cache() -> Result<Json<Option<String>>, StatusCode> {
    let dir = crate::error::blocking(|| existing_dir(&legacy_cache_dir())).await?;
    Ok(Json(dir))
}

fn existing_dir(dir: &Path) -> Option<String> {
    dir.is_dir().then(|| dir.to_string_lossy().into_owned())
}

/// Deletes the old faster-whisper model folder (and nothing else).
pub async fn delete_whisper_legacy_cache() -> Result<StatusCode, AppError> {
    crate::error::blocking(|| delete_dir(&legacy_cache_dir()))
        .await
        .map_err(|_| err500("The delete task stopped unexpectedly."))?
        .map_err(err500)?;
    Ok(StatusCode::OK)
}

fn delete_dir(dir: &Path) -> Result<(), String> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!(
            "Could not delete the old model folder {}: {e}. Close other programs using it and try again.",
            dir.display()
        )),
    }
}

pub(crate) fn start_transcription(
    ctx: JobCtx,
    paths: Option<ToolPaths>,
    options: TranscribeOptions,
) -> String {
    let job_id = format!("transcribe-{}", uuid::Uuid::new_v4());
    let control = ctx.begin(
        &job_id,
        JobType::Transcribe,
        options.input.to_string_lossy().into_owned(),
        Some(options.output.to_string_lossy().into_owned()),
    );
    let id = job_id.clone();
    tokio::spawn(async move {
        ctx.running(&id);
        let token = control.token().clone();
        let worker_ctx = ctx.clone();
        let worker_id = id.clone();
        let joined = tokio::task::spawn_blocking(move || {
            let paths = paths.unwrap_or_else(crate::tools::tool_paths);
            let mut on_progress =
                |pct: f64, msg: Option<&str>| worker_ctx.progress(&worker_id, pct, msg);
            whisper::transcribe(&paths, &options, &mut on_progress, token).map(|o| {
                let lang = o.detected_language.as_deref().unwrap_or("?");
                (
                    format!(
                        "Done: {} subtitles, language {lang}, on {}",
                        o.segments, o.device_used
                    ),
                    Some(options.output.to_string_lossy().into_owned()),
                )
            })
        })
        .await;
        let result = joined.unwrap_or_else(|e| {
            Err(WhisperError::Engine(format!(
                "the transcription task stopped unexpectedly ({e})"
            )))
        });
        ctx.finish(&id, result);
    });
    job_id
}

pub async fn transcribe_audio(
    State(state): State<AppState>,
    Json(req): Json<TranscribeOptions>,
) -> Json<String> {
    Json(start_transcription(JobCtx::from_state(&state), None, req))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::{create_event_channel, JobQueue};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const BODY_LEN: usize = 256 * 1024;

    const TEST_SPEC: ModelSpec = ModelSpec {
        name: "stub",
        file: "ggml-stub.bin",
        size_mb: 1,
        size_bytes: Some(BODY_LEN as u64),
        sha256: None,
    };

    fn ctx() -> (JobCtx, jobs::EventReceiver) {
        let (events, rx) = create_event_channel();
        (
            JobCtx {
                queue: Arc::new(Mutex::new(JobQueue::new())),
                controls: Arc::new(Mutex::new(HashMap::new())),
                events,
            },
            rx,
        )
    }

    /// Minimal HTTP server sending the body in 16 KiB chunks 20 ms apart; with `stall` it
    /// stops after half the body and keeps the connection open.
    async fn stub_server(stall: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let mut req = Vec::new();
                    let mut buf = [0u8; 1024];
                    while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                        match sock.read(&mut buf).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => req.extend_from_slice(&buf[..n]),
                        }
                    }
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {BODY_LEN}\r\nConnection: close\r\n\r\n"
                    );
                    if sock.write_all(head.as_bytes()).await.is_err() {
                        return;
                    }
                    let body = vec![7u8; BODY_LEN];
                    for (i, chunk) in body.chunks(16 * 1024).enumerate() {
                        if stall && i * 16 * 1024 >= BODY_LEN / 2 {
                            tokio::time::sleep(Duration::from_secs(30)).await;
                            return;
                        }
                        if sock.write_all(chunk).await.is_err() {
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                });
            }
        });
        format!("http://{addr}")
    }

    /// Collects this job's events until it completes.
    async fn collect(
        rx: &mut jobs::EventReceiver,
        job_id: &str,
        on_progress: impl Fn(),
    ) -> Vec<JobEvent> {
        let mut seen = Vec::new();
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(10), rx.recv())
                .await
                .expect("job never completed")
                .unwrap();
            if ev.job_id != job_id {
                continue;
            }
            if ev.event_type == "job-progress" {
                on_progress();
            }
            let done = ev.event_type == "job-completed";
            seen.push(ev);
            if done {
                return seen;
            }
        }
    }

    #[tokio::test]
    async fn download_job_reports_progress_and_completes() {
        let base = stub_server(false).await;
        let dir = tempfile_dir();
        let (ctx, mut rx) = ctx();
        let downloads = ActiveDownloads::default();
        let id = start_download(
            ctx.clone(),
            downloads.clone(),
            TEST_SPEC,
            None,
            dir.clone(),
            base,
        );
        let events = collect(&mut rx, &id, || {}).await;
        assert_eq!(events[0].event_type, "job-started");
        let progress: Vec<f64> = events
            .iter()
            .filter(|e| e.event_type == "job-progress")
            .map(|e| e.progress.unwrap())
            .collect();
        assert!(progress.len() >= 2, "{progress:?}");
        assert_eq!(*progress.last().unwrap(), 100.0);
        let done = events.last().unwrap();
        assert_eq!(done.status, Some(JobStatus::Completed));
        assert!(dir.join("ggml-stub.bin").is_file());
        assert!(downloads.lock().unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn download_job_cancel_leaves_no_model() {
        let base = stub_server(true).await;
        let dir = tempfile_dir();
        let (ctx, mut rx) = ctx();
        let downloads = ActiveDownloads::default();
        let id = start_download(
            ctx.clone(),
            downloads.clone(),
            TEST_SPEC,
            None,
            dir.clone(),
            base,
        );
        let controls = ctx.controls.clone();
        let cancel_id = id.clone();
        let events = collect(&mut rx, &id, move || {
            if let Some(c) = controls.lock().unwrap().get(&cancel_id) {
                c.cancel();
            }
        })
        .await;
        assert_eq!(events.last().unwrap().status, Some(JobStatus::Cancelled));
        assert!(!dir.join("ggml-stub.bin").exists());
        assert!(downloads.lock().unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn second_download_of_same_model_joins_the_running_job() {
        let base = stub_server(true).await;
        let dir = tempfile_dir();
        let (ctx, _rx) = ctx();
        let downloads = ActiveDownloads::default();
        let first = start_download(
            ctx.clone(),
            downloads.clone(),
            TEST_SPEC,
            None,
            dir.clone(),
            base.clone(),
        );
        let second = start_download(
            ctx.clone(),
            downloads.clone(),
            TEST_SPEC,
            None,
            dir.clone(),
            base,
        );
        assert_eq!(first, second);
        assert_eq!(ctx.queue.lock().unwrap().jobs.len(), 1);
        ctx.controls.lock().unwrap().get(&first).unwrap().cancel();
    }

    #[test]
    fn legacy_cache_helpers() {
        let dir = tempfile_dir();
        let legacy = dir.join("whisper-models");
        assert_eq!(existing_dir(&legacy), None);
        std::fs::create_dir_all(legacy.join("models--x")).unwrap();
        assert!(existing_dir(&legacy).is_some());
        delete_dir(&legacy).unwrap();
        assert!(!legacy.exists());
        delete_dir(&legacy).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn tempfile_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("subforge_whisper_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
}
