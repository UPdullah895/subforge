//! Speech-to-text commands. The job logic lives in `subforge_core::whisper::jobs`; this file
//! only adapts it to the app's job queue, cancel registry and Tauri events.

use crate::jobs::{Job, JobControl, JobControlRegistry, JobStatus, JobType, SharedJobQueue};
use crate::tools::tool_paths;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use subforge_core::whisper::{
    self, ActiveDownloads, CatalogEntry, EngineInfo, JobKind, JobOutcome, JobSink,
    TranscribeOptions,
};
use subforge_core::CancellationToken;
use tauri::{AppHandle, Emitter, State};

/// Sends a job event to the frontend (`app.emit` in the app, a collector in tests).
pub(crate) type Emit = Arc<dyn Fn(&str, Value) + Send + Sync>;

/// Records whisper jobs in the app's queue and registry and emits the usual job events.
pub(crate) struct TauriSink {
    pub queue: SharedJobQueue,
    pub controls: JobControlRegistry,
    pub emit: Emit,
}

impl TauriSink {
    fn new(app: AppHandle, queue: SharedJobQueue, controls: JobControlRegistry) -> Arc<Self> {
        Arc::new(Self {
            queue,
            controls,
            emit: Arc::new(move |event, payload| {
                let _ = app.emit(event, payload);
            }),
        })
    }
}

impl JobSink for TauriSink {
    fn started(
        &self,
        job_id: &str,
        kind: JobKind,
        input: &str,
        output: Option<&str>,
    ) -> CancellationToken {
        let control = Arc::new(JobControl::new());
        self.queue.lock().unwrap().add(Job {
            id: job_id.to_string(),
            job_type: match kind {
                JobKind::DownloadModel => JobType::DownloadModel,
                JobKind::Transcribe => JobType::Transcribe,
            },
            status: JobStatus::Pending,
            input_file: input.to_string(),
            output_file: output.map(str::to_string),
            progress: 0.0,
            message: Some("Queued".to_string()),
        });
        self.controls
            .lock()
            .unwrap()
            .insert(job_id.to_string(), control.clone());
        (self.emit)("job-started", json!(job_id));
        control.token().clone()
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

    fn finished(&self, job_id: &str, outcome: &JobOutcome) {
        let (status, pct) = match outcome {
            JobOutcome::Completed { .. } => (JobStatus::Completed, 100.0),
            JobOutcome::Cancelled { .. } => (JobStatus::Cancelled, 0.0),
            JobOutcome::Failed { .. } => (JobStatus::Failed, 0.0),
        };
        {
            let mut q = self.queue.lock().unwrap();
            q.update_status(job_id, status);
            q.update_progress(job_id, pct, Some(outcome.message().to_string()));
        }
        self.controls.lock().unwrap().remove(job_id);
        (self.emit)(
            "job-completed",
            json!({
                "job_id": job_id,
                "status": outcome.status(),
                "message": outcome.message(),
                "details": outcome.details(),
                "output_path": outcome.output_path(),
            }),
        );
    }
}

fn cache_dir(dir: &Option<String>) -> Option<&Path> {
    dir.as_deref().map(Path::new)
}

#[tauri::command]
pub async fn get_whisper_info() -> Result<EngineInfo, String> {
    Ok(whisper::engine_info())
}

#[tauri::command]
pub async fn list_whisper_models(cache_dir: Option<String>) -> Result<Vec<String>, String> {
    let cache = whisper::resolve_cache_dir(self::cache_dir(&cache_dir));
    super::blocking(move || Ok(whisper::list_downloaded(&cache))).await
}

#[tauri::command]
pub async fn list_whisper_catalog() -> Result<Vec<CatalogEntry>, String> {
    Ok(whisper::catalog())
}

/// Starts (or joins) a model download job; returns its id at once.
#[tauri::command]
pub async fn download_whisper_model(
    app: AppHandle,
    queue: State<'_, SharedJobQueue>,
    controls: State<'_, JobControlRegistry>,
    downloads: State<'_, ActiveDownloads>,
    model: String,
    cache_dir: Option<String>,
) -> Result<String, String> {
    let sink = TauriSink::new(app, queue.inner().clone(), controls.inner().clone());
    whisper::start_model_download(sink, &downloads, &model, self::cache_dir(&cache_dir))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn transcribe_audio(
    app: AppHandle,
    queue: State<'_, SharedJobQueue>,
    controls: State<'_, JobControlRegistry>,
    request: TranscribeOptions,
) -> Result<String, String> {
    let sink = TauriSink::new(app, queue.inner().clone(), controls.inner().clone());
    Ok(whisper::start_transcription(sink, tool_paths, request))
}

/// Path of the old faster-whisper model folder, if it still exists.
#[tauri::command]
pub async fn whisper_legacy_cache() -> Result<Option<String>, String> {
    super::blocking(|| Ok(whisper::legacy_cache().map(|p| p.to_string_lossy().into_owned()))).await
}

/// Deletes the old faster-whisper model folder (and nothing else).
#[tauri::command]
pub async fn delete_whisper_legacy_cache() -> Result<(), String> {
    super::blocking(|| whisper::delete_legacy_cache().map_err(|e| e.to_string())).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::JobQueue;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::Duration;

    type Events = Arc<Mutex<Vec<(String, Value)>>>;

    fn sink() -> (Arc<TauriSink>, Events) {
        let events: Events = Arc::new(Mutex::new(Vec::new()));
        let rec = events.clone();
        let sink = Arc::new(TauriSink {
            queue: Arc::new(Mutex::new(JobQueue::new())),
            controls: Arc::new(Mutex::new(HashMap::new())),
            emit: Arc::new(move |e, p| rec.lock().unwrap().push((e.to_string(), p))),
        });
        (sink, events)
    }

    #[test]
    fn sink_records_job_registers_cancel_and_emits_events() {
        let (sink, events) = sink();
        let token = sink.started(
            "download-1",
            JobKind::DownloadModel,
            "base",
            Some("/c/ggml-base.bin"),
        );
        {
            let q = sink.queue.lock().unwrap();
            let job = q.get("download-1").unwrap();
            assert_eq!(job.job_type, JobType::DownloadModel);
            assert_eq!(job.output_file.as_deref(), Some("/c/ggml-base.bin"));
        }
        // `cancel_job` goes through the registry and must reach core's token.
        sink.controls.lock().unwrap()["download-1"].cancel();
        assert!(token.is_cancelled());

        sink.running("download-1");
        sink.progress("download-1", 42.0, Some("10/141 MB"));
        sink.finished(
            "download-1",
            &JobOutcome::Failed {
                message: "Download failed".into(),
                details: Some("GET x -> 404".into()),
            },
        );
        assert!(sink.controls.lock().unwrap().is_empty());
        assert_eq!(
            sink.queue.lock().unwrap().get("download-1").unwrap().status,
            JobStatus::Failed
        );
        let ev = events.lock().unwrap();
        assert_eq!(ev[0], ("job-started".to_string(), json!("download-1")));
        assert_eq!(ev[1].1["progress"], 42.0);
        assert_eq!(ev[2].0, "job-completed");
        assert_eq!(ev[2].1["status"], "failed");
        assert_eq!(ev[2].1["details"], "GET x -> 404");
    }

    #[tokio::test]
    async fn transcription_job_through_sink_fails_cleanly_without_model() {
        let (sink, events) = sink();
        let dir = std::env::temp_dir().join(format!("subforge_tx_{}", uuid::Uuid::new_v4()));
        let options: TranscribeOptions = serde_json::from_value(json!({
            "input": dir.join("in.wav"),
            "output": dir.join("in.srt"),
            "model": "tiny",
            "device": "auto",
            "format": "srt",
            "cache_dir": dir.join("cache"),
        }))
        .unwrap();
        let id = whisper::start_transcription(sink.clone(), Default::default, options);
        let done = loop {
            let found = events
                .lock()
                .unwrap()
                .iter()
                .find(|(e, _)| e == "job-completed")
                .map(|(_, p)| p.clone());
            if let Some(p) = found {
                break p;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        assert_eq!(done["job_id"], id.as_str());
        assert_eq!(done["status"], "failed");
        assert!(
            done["message"].as_str().unwrap().contains("not downloaded"),
            "{done}"
        );
        assert!(sink.controls.lock().unwrap().is_empty());
        assert_eq!(
            sink.queue.lock().unwrap().get(&id).unwrap().job_type,
            JobType::Transcribe
        );
    }
}
