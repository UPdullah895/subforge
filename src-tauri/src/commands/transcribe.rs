//! Speech-to-text commands: whisper.cpp engine info, model catalog/download jobs and
//! transcription jobs. All heavy work runs off the main thread (async I/O or `spawn_blocking`).

use crate::jobs::{Job, JobControl, JobControlRegistry, JobStatus, JobType, SharedJobQueue};
use crate::tools::tool_paths;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use subforge_core::whisper::{
    self, default_cache_dir, legacy_cache_dir, list_downloaded, spec_for, EngineInfo, ModelSpec,
    TranscribeOptions, WhisperError, DEFAULT_BASE_URL, MODELS, VAD_MODEL,
};
use tauri::{AppHandle, Emitter, State};

/// Model downloads in flight, keyed by cache dir + model name, to the job id doing it.
pub type ActiveDownloads = Arc<Mutex<HashMap<String, String>>>;

/// Sends a job event to the frontend (`app.emit` in the app, a collector in tests).
pub(crate) type Emit = Arc<dyn Fn(&str, Value) + Send + Sync>;

fn app_emitter(app: AppHandle) -> Emit {
    Arc::new(move |event, payload| {
        let _ = app.emit(event, payload);
    })
}

#[derive(Clone)]
pub(crate) struct JobCtx {
    pub queue: SharedJobQueue,
    pub controls: JobControlRegistry,
    pub emit: Emit,
}

impl JobCtx {
    fn begin(
        &self,
        job_id: &str,
        job_type: JobType,
        input: String,
        output: Option<String>,
    ) -> Arc<JobControl> {
        let control = Arc::new(JobControl::new());
        self.queue.lock().unwrap().add(Job {
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
        (self.emit)("job-started", json!(job_id));
        control
    }

    fn running(&self, job_id: &str) {
        self.queue
            .lock()
            .unwrap()
            .update_status(job_id, JobStatus::Running);
    }

    fn progress(&self, job_id: &str, pct: f64, message: Option<&str>) {
        self.queue
            .lock()
            .unwrap()
            .update_progress(job_id, pct, message.map(str::to_string));
        (self.emit)(
            "job-progress",
            json!({ "job_id": job_id, "progress": pct, "message": message }),
        );
    }

    fn finish(&self, job_id: &str, result: Result<(String, Option<String>), WhisperError>) {
        let (status, payload) = match result {
            Ok((message, output_path)) => (
                JobStatus::Completed,
                json!({
                    "job_id": job_id,
                    "status": "completed",
                    "message": message,
                    "output_path": output_path,
                }),
            ),
            Err(e) => {
                let cancelled = matches!(e, WhisperError::Cancelled);
                let details = match &e {
                    WhisperError::Network { details, .. } => Some(details.clone()),
                    WhisperError::Tool(subforge_core::tools::ToolError::Failed {
                        details, ..
                    }) => Some(details.clone()),
                    _ => None,
                };
                (
                    if cancelled {
                        JobStatus::Cancelled
                    } else {
                        JobStatus::Failed
                    },
                    json!({
                        "job_id": job_id,
                        "status": if cancelled { "cancelled" } else { "failed" },
                        "message": e.to_string(),
                        "details": details,
                    }),
                )
            }
        };
        {
            let mut q = self.queue.lock().unwrap();
            let pct = if status == JobStatus::Completed {
                100.0
            } else {
                0.0
            };
            q.update_status(job_id, status);
            q.update_progress(job_id, pct, payload["message"].as_str().map(str::to_string));
        }
        self.controls.lock().unwrap().remove(job_id);
        (self.emit)("job-completed", payload);
    }
}

#[tauri::command]
pub async fn get_whisper_info() -> Result<EngineInfo, String> {
    Ok(whisper::engine_info())
}

fn cache_dir_or_default(cache_dir: Option<String>) -> PathBuf {
    cache_dir
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_cache_dir)
}

#[tauri::command]
pub async fn list_whisper_models(cache_dir: Option<String>) -> Result<Vec<String>, String> {
    let cache = cache_dir_or_default(cache_dir);
    super::blocking(move || Ok(list_downloaded(&cache))).await
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CatalogEntry {
    pub name: String,
    pub size_mb: u32,
}

pub fn catalog() -> Vec<CatalogEntry> {
    MODELS
        .iter()
        .map(|m| CatalogEntry {
            name: m.name.to_string(),
            size_mb: m.size_mb,
        })
        .collect()
}

#[tauri::command]
pub async fn list_whisper_catalog() -> Result<Vec<CatalogEntry>, String> {
    Ok(catalog())
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

#[tauri::command]
pub async fn download_whisper_model(
    app: AppHandle,
    queue: State<'_, SharedJobQueue>,
    controls: State<'_, JobControlRegistry>,
    downloads: State<'_, ActiveDownloads>,
    model: String,
    cache_dir: Option<String>,
) -> Result<String, String> {
    let spec = *spec_for(&model).ok_or_else(|| WhisperError::UnknownModel(model).to_string())?;
    let ctx = JobCtx {
        queue: queue.inner().clone(),
        controls: controls.inner().clone(),
        emit: app_emitter(app),
    };
    Ok(start_download(
        ctx,
        downloads.inner().clone(),
        spec,
        Some(VAD_MODEL),
        cache_dir_or_default(cache_dir),
        DEFAULT_BASE_URL.to_string(),
    ))
}

/// Path of the old faster-whisper model folder, if it still exists.
#[tauri::command]
pub async fn whisper_legacy_cache() -> Result<Option<String>, String> {
    super::blocking(|| Ok(existing_dir(&legacy_cache_dir()))).await
}

fn existing_dir(dir: &Path) -> Option<String> {
    dir.is_dir().then(|| dir.to_string_lossy().into_owned())
}

/// Deletes the old faster-whisper model folder (and nothing else).
#[tauri::command]
pub async fn delete_whisper_legacy_cache() -> Result<(), String> {
    super::blocking(|| delete_dir(&legacy_cache_dir())).await
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
    paths: Option<subforge_core::tools::ToolPaths>,
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
            let paths = paths.unwrap_or_else(tool_paths);
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

#[tauri::command]
pub async fn transcribe_audio(
    app: AppHandle,
    queue: State<'_, SharedJobQueue>,
    controls: State<'_, JobControlRegistry>,
    request: TranscribeOptions,
) -> Result<String, String> {
    let ctx = JobCtx {
        queue: queue.inner().clone(),
        controls: controls.inner().clone(),
        emit: app_emitter(app),
    };
    Ok(start_transcription(ctx, None, request))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::JobQueue;
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

    type Events = Arc<Mutex<Vec<(String, Value)>>>;

    fn ctx() -> (JobCtx, Events) {
        let events: Events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let ctx = JobCtx {
            queue: Arc::new(Mutex::new(JobQueue::new())),
            controls: Arc::new(Mutex::new(HashMap::new())),
            emit: Arc::new(move |e, p| sink.lock().unwrap().push((e.to_string(), p))),
        };
        (ctx, events)
    }

    /// Minimal HTTP server: sends the body in 16 KiB chunks 20 ms apart. With `stall` it stops
    /// after half the body and keeps the connection open.
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

    async fn wait_completed(events: &Events, job_id: &str) -> Value {
        for _ in 0..500 {
            let found = events
                .lock()
                .unwrap()
                .iter()
                .find(|(e, p)| e == "job-completed" && p["job_id"] == job_id)
                .map(|(_, p)| p.clone());
            if let Some(p) = found {
                return p;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("job {job_id} never completed: {:?}", events.lock().unwrap());
    }

    fn progress_of(events: &Events, job_id: &str) -> Vec<f64> {
        events
            .lock()
            .unwrap()
            .iter()
            .filter(|(e, p)| e == "job-progress" && p["job_id"] == job_id)
            .map(|(_, p)| p["progress"].as_f64().unwrap())
            .collect()
    }

    #[tokio::test]
    async fn download_job_reports_progress_and_completes() {
        let base = stub_server(false).await;
        let dir = tempfile::tempdir().unwrap();
        let (ctx, events) = ctx();
        let downloads = ActiveDownloads::default();
        let id = start_download(
            ctx.clone(),
            downloads.clone(),
            TEST_SPEC,
            None,
            dir.path().to_path_buf(),
            base,
        );
        assert!(id.starts_with("download-"));
        assert_eq!(
            events.lock().unwrap()[0],
            ("job-started".to_string(), json!(id))
        );

        let done = wait_completed(&events, &id).await;
        assert_eq!(done["status"], "completed", "{done}");
        let progress = progress_of(&events, &id);
        assert!(progress.len() >= 2, "{progress:?}");
        assert!(progress.windows(2).all(|w| w[0] <= w[1]), "{progress:?}");
        assert_eq!(*progress.last().unwrap(), 100.0);
        assert!(dir.path().join("ggml-stub.bin").is_file());
        assert!(downloads.lock().unwrap().is_empty());
        assert!(ctx.controls.lock().unwrap().is_empty());
        let q = ctx.queue.lock().unwrap();
        assert_eq!(q.get(&id).unwrap().status, JobStatus::Completed);
    }

    #[tokio::test]
    async fn download_job_cancel_leaves_no_model() {
        let base = stub_server(true).await;
        let dir = tempfile::tempdir().unwrap();
        let (ctx, events) = ctx();
        let downloads = ActiveDownloads::default();
        let id = start_download(
            ctx.clone(),
            downloads.clone(),
            TEST_SPEC,
            None,
            dir.path().to_path_buf(),
            base,
        );
        // Let some bytes arrive, then cancel through the job registry like `cancel_job` does.
        for _ in 0..200 {
            if !progress_of(&events, &id).is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
        ctx.controls.lock().unwrap().get(&id).unwrap().cancel();

        let done = wait_completed(&events, &id).await;
        assert_eq!(done["status"], "cancelled", "{done}");
        assert!(!dir.path().join("ggml-stub.bin").exists());
        assert!(downloads.lock().unwrap().is_empty());
        assert_eq!(
            ctx.queue.lock().unwrap().get(&id).unwrap().status,
            JobStatus::Cancelled
        );
    }

    #[tokio::test]
    async fn second_download_of_same_model_joins_the_running_job() {
        let base = stub_server(true).await;
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _events) = ctx();
        let downloads = ActiveDownloads::default();
        let start = || {
            start_download(
                ctx.clone(),
                downloads.clone(),
                TEST_SPEC,
                None,
                dir.path().to_path_buf(),
                base.clone(),
            )
        };
        let first = start();
        let second = start();
        assert_eq!(first, second);
        assert_eq!(ctx.queue.lock().unwrap().jobs.len(), 1);
        ctx.controls.lock().unwrap().get(&first).unwrap().cancel();
    }

    #[tokio::test]
    async fn transcription_job_without_model_fails_with_clear_message() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, events) = ctx();
        let options: TranscribeOptions = serde_json::from_value(json!({
            "input": dir.path().join("in.wav"),
            "output": dir.path().join("in.srt"),
            "model": "tiny",
            "device": "cuda",
            "language": null,
            "format": "srt",
            "word_timestamps": false,
            "vad_filter": true,
            "cache_dir": dir.path().join("cache"),
        }))
        .unwrap();
        let id = start_transcription(ctx.clone(), Some(Default::default()), options);
        let done = wait_completed(&events, &id).await;
        assert_eq!(done["status"], "failed", "{done}");
        assert!(
            done["message"].as_str().unwrap().contains("not downloaded"),
            "{done}"
        );
        assert!(ctx.controls.lock().unwrap().is_empty());
    }

    #[test]
    fn catalog_lists_every_model_with_size() {
        let c = catalog();
        assert_eq!(c.len(), MODELS.len());
        assert!(c.contains(&CatalogEntry {
            name: "base".into(),
            size_mb: 141
        }));
    }

    #[test]
    fn legacy_cache_helpers() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("whisper-models");
        assert_eq!(existing_dir(&legacy), None);
        std::fs::create_dir_all(legacy.join("models--x")).unwrap();
        std::fs::write(legacy.join("models--x/blob"), b"x").unwrap();
        assert_eq!(
            existing_dir(&legacy),
            Some(legacy.to_string_lossy().into_owned())
        );
        delete_dir(&legacy).unwrap();
        assert!(!legacy.exists());
        // Deleting again is not an error.
        delete_dir(&legacy).unwrap();
    }
}
