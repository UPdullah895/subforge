//! Speech-to-text routes. The job logic lives in `subforge_core::whisper::jobs`; this file
//! only adapts it to the server's job queue, cancel registry and WebSocket events.

use crate::error::{err400, err500, AppError};
use crate::jobs::{
    self, Job, JobControl, JobControlRegistry, JobEvent, JobStatus, JobType, SharedJobQueue,
};
use crate::AppState;
use axum::{extract::Json, extract::State, http::StatusCode};
use serde::Deserialize;
use std::path::Path;
use std::sync::Arc;
use subforge_core::whisper::{
    self, CatalogEntry, EngineInfo, JobKind, JobOutcome, JobSink, TranscribeOptions,
};
use subforge_core::CancellationToken;

/// Records whisper jobs in the server's queue and registry and broadcasts job events.
pub(crate) struct ServerSink {
    pub queue: SharedJobQueue,
    pub controls: JobControlRegistry,
    pub events: jobs::EventSender,
}

impl ServerSink {
    fn from_state(state: &AppState) -> Arc<Self> {
        Arc::new(Self {
            queue: state.job_queue.clone(),
            controls: state.job_controls.clone(),
            events: state.event_sender.clone(),
        })
    }

    fn emit(&self, event_type: &str, job_id: &str, fill: impl FnOnce(&mut JobEvent)) {
        let mut ev = JobEvent {
            event_type: event_type.to_string(),
            job_id: job_id.to_string(),
            status: None,
            progress: None,
            message: None,
            stage: None,
            output_path: None,
            details: None,
        };
        fill(&mut ev);
        jobs::emit_event(&self.events, ev);
    }
}

impl JobSink for ServerSink {
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
        self.emit("job-started", job_id, |_| {});
        control.token().clone()
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
        self.emit("job-progress", job_id, |ev| {
            ev.progress = Some(pct);
            ev.message = message;
        });
    }

    fn finished(&self, job_id: &str, outcome: &JobOutcome) {
        let (status, pct) = match outcome {
            JobOutcome::Completed { .. } => (JobStatus::Completed, 100.0),
            JobOutcome::Cancelled { .. } => (JobStatus::Cancelled, 0.0),
            JobOutcome::Failed { .. } => (JobStatus::Failed, 0.0),
        };
        {
            let mut q = self.queue.lock().unwrap();
            q.update_status(job_id, status.clone());
            q.update_progress(job_id, pct, Some(outcome.message().to_string()));
        }
        self.controls.lock().unwrap().remove(job_id);
        self.emit("job-completed", job_id, |ev| {
            ev.status = Some(status);
            ev.progress = Some(pct);
            ev.message = Some(outcome.message().to_string());
            ev.output_path = outcome.output_path().map(str::to_string);
            ev.details = outcome.details().map(str::to_string);
        });
    }
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
    let cache = whisper::resolve_cache_dir(req.cache_dir.as_deref().map(Path::new));
    let models = crate::error::blocking(move || whisper::list_downloaded(&cache)).await?;
    Ok(Json(models))
}

pub async fn list_whisper_catalog() -> Json<Vec<CatalogEntry>> {
    Json(whisper::catalog())
}

#[derive(Deserialize)]
pub struct DownloadRequest {
    model: String,
    cache_dir: Option<String>,
}

/// Starts (or joins) a model download job; answers with its id at once.
pub async fn download_whisper_model(
    State(state): State<AppState>,
    Json(req): Json<DownloadRequest>,
) -> Result<Json<String>, AppError> {
    whisper::start_model_download(
        ServerSink::from_state(&state),
        &state.whisper_downloads,
        &req.model,
        req.cache_dir.as_deref().map(Path::new),
    )
    .map(Json)
    .map_err(err400)
}

/// Path of the old faster-whisper model folder, if it still exists.
pub async fn whisper_legacy_cache() -> Result<Json<Option<String>>, StatusCode> {
    let dir = crate::error::blocking(|| {
        whisper::legacy_cache().map(|p| p.to_string_lossy().into_owned())
    })
    .await?;
    Ok(Json(dir))
}

/// Deletes the old faster-whisper model folder (and nothing else).
pub async fn delete_whisper_legacy_cache() -> Result<StatusCode, AppError> {
    crate::error::blocking(whisper::delete_legacy_cache)
        .await
        .map_err(|_| err500("The delete task stopped unexpectedly."))?
        .map_err(err500)?;
    Ok(StatusCode::OK)
}

pub async fn transcribe_audio(
    State(state): State<AppState>,
    Json(req): Json<TranscribeOptions>,
) -> Json<String> {
    Json(whisper::start_transcription(
        ServerSink::from_state(&state),
        crate::tools::tool_paths,
        req,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::{create_event_channel, JobQueue};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::Duration;

    fn sink() -> (Arc<ServerSink>, jobs::EventReceiver) {
        let (events, rx) = create_event_channel();
        (
            Arc::new(ServerSink {
                queue: Arc::new(Mutex::new(JobQueue::new())),
                controls: Arc::new(Mutex::new(HashMap::new())),
                events,
            }),
            rx,
        )
    }

    #[tokio::test]
    async fn sink_records_job_registers_cancel_and_emits_details() {
        let (sink, mut rx) = sink();
        let token = sink.started(
            "download-1",
            JobKind::DownloadModel,
            "base",
            Some("/c/ggml-base.bin"),
        );
        assert_eq!(
            sink.queue
                .lock()
                .unwrap()
                .get("download-1")
                .unwrap()
                .job_type,
            JobType::DownloadModel
        );
        sink.controls.lock().unwrap()["download-1"].cancel();
        assert!(token.is_cancelled());

        sink.progress("download-1", 42.0, Some("10/141 MB"));
        sink.finished(
            "download-1",
            &JobOutcome::Failed {
                message: "Download failed".into(),
                details: Some("GET x -> 404".into()),
            },
        );
        assert!(sink.controls.lock().unwrap().is_empty());

        let started = rx.recv().await.unwrap();
        assert_eq!(started.event_type, "job-started");
        let progress = rx.recv().await.unwrap();
        assert_eq!(progress.progress, Some(42.0));
        let done = rx.recv().await.unwrap();
        assert_eq!(done.event_type, "job-completed");
        assert_eq!(done.status, Some(JobStatus::Failed));
        assert_eq!(done.details.as_deref(), Some("GET x -> 404"));
        // Serialised events omit `details` when absent (older clients see the same shape).
        let json = serde_json::to_value(&progress).unwrap();
        assert!(json.get("details").is_none(), "{json}");
        assert_eq!(
            serde_json::to_value(&done).unwrap()["details"],
            "GET x -> 404"
        );
    }

    #[tokio::test]
    async fn transcription_job_through_sink_fails_cleanly_without_model() {
        let (sink, mut rx) = sink();
        let dir = std::env::temp_dir().join(format!("subforge_tx_{}", uuid::Uuid::new_v4()));
        let options: TranscribeOptions = serde_json::from_value(serde_json::json!({
            "input": dir.join("in.wav"),
            "output": dir.join("in.srt"),
            "model": "tiny",
            "format": "srt",
            "cache_dir": dir.join("cache"),
        }))
        .unwrap();
        let id = whisper::start_transcription(sink.clone(), Default::default, options);
        let done = loop {
            let ev = tokio::time::timeout(Duration::from_secs(10), rx.recv())
                .await
                .unwrap()
                .unwrap();
            if ev.event_type == "job-completed" {
                break ev;
            }
        };
        assert_eq!(done.job_id, id);
        assert_eq!(done.status, Some(JobStatus::Failed));
        assert!(done.message.unwrap().contains("not downloaded"));
        assert!(sink.controls.lock().unwrap().is_empty());
    }
}
