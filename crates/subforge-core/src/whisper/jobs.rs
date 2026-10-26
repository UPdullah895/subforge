//! Background jobs for model downloads and transcription, shared by the desktop app and the
//! server. Each front end only supplies a [`JobSink`] that records the job in its own queue,
//! registers cancellation and forwards events to its UI transport.
//!
//! The `start_*` functions return the job id at once and run the work on the current tokio
//! runtime (transcription goes through `spawn_blocking`), so they must be called from within
//! one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use super::models::{
    download_with, legacy_cache_dir, model_path, resolve_cache_dir, spec_for, ModelSpec,
    WhisperError, DEFAULT_BASE_URL, MODELS, VAD_MODEL,
};
use super::transcribe::{transcribe, TranscribeOptions, TranscribeOutcome};
use crate::progress::{CancellationToken, ProgressFn};
use crate::tools::{ToolError, ToolPaths};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    DownloadModel,
    Transcribe,
}

/// How a job ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobOutcome {
    Completed {
        message: String,
        output_path: Option<String>,
    },
    Cancelled {
        message: String,
    },
    Failed {
        message: String,
        /// Raw tool / network output, for a "details" disclosure.
        details: Option<String>,
    },
}

impl JobOutcome {
    pub fn from_error(e: WhisperError) -> Self {
        let details = match &e {
            WhisperError::Network { details, .. } => Some(details.clone()),
            WhisperError::Tool(ToolError::Failed { details, .. }) => Some(details.clone()),
            _ => None,
        };
        match e {
            WhisperError::Cancelled => JobOutcome::Cancelled {
                message: e.to_string(),
            },
            _ => JobOutcome::Failed {
                message: e.to_string(),
                details,
            },
        }
    }

    /// `"completed"`, `"cancelled"` or `"failed"` (the job-event status values).
    pub fn status(&self) -> &'static str {
        match self {
            JobOutcome::Completed { .. } => "completed",
            JobOutcome::Cancelled { .. } => "cancelled",
            JobOutcome::Failed { .. } => "failed",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            JobOutcome::Completed { message, .. }
            | JobOutcome::Cancelled { message }
            | JobOutcome::Failed { message, .. } => message,
        }
    }

    pub fn details(&self) -> Option<&str> {
        match self {
            JobOutcome::Failed { details, .. } => details.as_deref(),
            _ => None,
        }
    }

    pub fn output_path(&self) -> Option<&str> {
        match self {
            JobOutcome::Completed { output_path, .. } => output_path.as_deref(),
            _ => None,
        }
    }
}

/// Where a job's lifecycle is reported. Implemented by each front end.
pub trait JobSink: Send + Sync {
    /// Called synchronously, before the start function returns: record the job, register it
    /// for cancellation and announce it. The returned token cancels the job.
    fn started(
        &self,
        job_id: &str,
        kind: JobKind,
        input: &str,
        output: Option<&str>,
    ) -> CancellationToken;
    /// The work has begun.
    fn running(&self, _job_id: &str) {}
    fn progress(&self, job_id: &str, pct: f64, message: Option<&str>);
    /// Called exactly once when the job ends; unregister it here.
    fn finished(&self, job_id: &str, outcome: &JobOutcome);
}

/// Model downloads in flight (cache dir + model name -> job id), so a second request for the
/// same download joins the running job instead of racing it.
#[derive(Debug, Default, Clone)]
pub struct ActiveDownloads(Arc<Mutex<HashMap<String, String>>>);

impl ActiveDownloads {
    pub fn is_empty(&self) -> bool {
        self.map().is_empty()
    }

    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Starts a background download of catalog model `name` (plus the VAD model) into `cache_dir`
/// (blank means the default cache). Returns the job id, or the running job's id when the same
/// download is already in progress.
pub fn start_model_download(
    sink: Arc<dyn JobSink>,
    downloads: &ActiveDownloads,
    name: &str,
    cache_dir: Option<&Path>,
) -> Result<String, WhisperError> {
    let spec = *spec_for(name).ok_or_else(|| WhisperError::UnknownModel(name.to_string()))?;
    Ok(start_download_with(
        sink,
        downloads,
        spec,
        Some(VAD_MODEL),
        resolve_cache_dir(cache_dir),
        DEFAULT_BASE_URL.to_string(),
    ))
}

/// [`start_model_download`] for an explicit spec, VAD spec and base URL (tests use a stub).
pub fn start_download_with(
    sink: Arc<dyn JobSink>,
    downloads: &ActiveDownloads,
    spec: ModelSpec,
    vad: Option<ModelSpec>,
    cache: PathBuf,
    base_url: String,
) -> String {
    let key = format!("{}|{}", cache.display(), spec.name);
    let job_id = {
        let mut active = downloads.map();
        if let Some(existing) = active.get(&key) {
            return existing.clone();
        }
        let id = format!("download-{}", uuid_v4());
        active.insert(key.clone(), id.clone());
        id
    };
    let target = model_path(&cache, &spec).to_string_lossy().into_owned();
    let token = sink.started(&job_id, JobKind::DownloadModel, spec.name, Some(&target));

    let downloads = downloads.clone();
    let id = job_id.clone();
    tokio::spawn(async move {
        sink.running(&id);
        let mut on_progress = |pct: f64, msg: Option<&str>| sink.progress(&id, pct, msg);
        let result = download_with(
            &spec,
            vad.as_ref(),
            &cache,
            &base_url,
            &mut on_progress,
            token,
        )
        .await;
        let outcome = match result {
            Ok(path) => JobOutcome::Completed {
                message: format!("Model '{}' downloaded", spec.name),
                output_path: Some(path.to_string_lossy().into_owned()),
            },
            Err(e) => JobOutcome::from_error(e),
        };
        downloads.map().remove(&key);
        sink.finished(&id, &outcome);
    });
    job_id
}

/// Process-wide limit of one running transcription (see [`start_transcription`]).
static TRANSCRIBE_SLOT: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

/// Progress message of a transcription waiting for another one to finish.
pub const QUEUED_MESSAGE: &str = "Queued: waiting for another transcription to finish";

/// Starts a background transcription. `paths` is called on the worker thread (it may read
/// config from disk). Only one transcription runs at a time; later ones report
/// [`QUEUED_MESSAGE`] until the running one ends, and can be cancelled while they wait.
pub fn start_transcription(
    sink: Arc<dyn JobSink>,
    paths: impl FnOnce() -> ToolPaths + Send + 'static,
    options: TranscribeOptions,
) -> String {
    start_transcription_with(sink, paths, options, transcribe)
}

/// [`start_transcription`] with the transcription body supplied by the caller (tests use a
/// stub so they need no model).
#[doc(hidden)]
pub fn start_transcription_with<W>(
    sink: Arc<dyn JobSink>,
    paths: impl FnOnce() -> ToolPaths + Send + 'static,
    options: TranscribeOptions,
    work: W,
) -> String
where
    W: FnOnce(
            &ToolPaths,
            &TranscribeOptions,
            ProgressFn<'_>,
            CancellationToken,
        ) -> Result<TranscribeOutcome, WhisperError>
        + Send
        + 'static,
{
    let job_id = format!("transcribe-{}", uuid_v4());
    let input = options.input.to_string_lossy().into_owned();
    let output = options.output.to_string_lossy().into_owned();
    let token = sink.started(&job_id, JobKind::Transcribe, &input, Some(&output));

    let id = job_id.clone();
    tokio::spawn(async move {
        // One transcription at a time per process: each loads a whole model into memory, so
        // running several together can exhaust RAM / VRAM and crash the app.
        let permit = match TRANSCRIBE_SLOT.try_acquire() {
            Ok(p) => p,
            Err(_) => {
                sink.progress(&id, 0.0, Some(QUEUED_MESSAGE));
                tokio::select! {
                    biased;
                    _ = token.cancelled() => {
                        sink.finished(&id, &JobOutcome::from_error(WhisperError::Cancelled));
                        return;
                    }
                    p = TRANSCRIBE_SLOT.acquire() => p.expect("the transcription semaphore is never closed"),
                }
            }
        };
        if token.is_cancelled() {
            drop(permit);
            sink.finished(&id, &JobOutcome::from_error(WhisperError::Cancelled));
            return;
        }
        sink.running(&id);
        let worker_sink = sink.clone();
        let worker_id = id.clone();
        let joined = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let paths = paths();
            let mut on_progress =
                |pct: f64, msg: Option<&str>| worker_sink.progress(&worker_id, pct, msg);
            work(&paths, &options, &mut on_progress, token)
        })
        .await;
        let outcome = match joined {
            Ok(Ok(o)) => JobOutcome::Completed {
                message: format!(
                    "Done: {} subtitles, language {}, on {}",
                    o.segments,
                    o.detected_language.as_deref().unwrap_or("?"),
                    o.device_used
                ),
                output_path: Some(output),
            },
            Ok(Err(e)) => JobOutcome::from_error(e),
            Err(e) => JobOutcome::from_error(WhisperError::Engine(format!(
                "the transcription task stopped unexpectedly ({e})"
            ))),
        };
        sink.finished(&id, &outcome);
    });
    job_id
}

/// One downloadable model as the UI lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// The old faster-whisper model folder, if it still exists.
pub fn legacy_cache() -> Option<PathBuf> {
    existing_dir(&legacy_cache_dir())
}

/// Deletes the old faster-whisper model folder (and nothing else). Missing is not an error.
pub fn delete_legacy_cache() -> Result<(), WhisperError> {
    delete_dir(&legacy_cache_dir())
}

fn existing_dir(dir: &Path) -> Option<PathBuf> {
    dir.is_dir().then(|| dir.to_path_buf())
}

fn delete_dir(dir: &Path) -> Result<(), WhisperError> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(WhisperError::Delete {
            path: dir.to_path_buf(),
            source,
        }),
    }
}

fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn legacy_dir_helpers() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("whisper-models");
        assert_eq!(existing_dir(&legacy), None);
        std::fs::create_dir_all(legacy.join("models--x")).unwrap();
        std::fs::write(legacy.join("models--x/blob"), b"x").unwrap();
        assert_eq!(existing_dir(&legacy), Some(legacy.clone()));
        delete_dir(&legacy).unwrap();
        assert!(!legacy.exists());
        delete_dir(&legacy).unwrap();
    }

    #[test]
    fn outcome_from_error_keeps_details_and_cancel() {
        let o = JobOutcome::from_error(WhisperError::Network {
            summary: "Download failed".into(),
            details: "GET x -> 404".into(),
        });
        assert_eq!(o.status(), "failed");
        assert_eq!(o.details(), Some("GET x -> 404"));
        let c = JobOutcome::from_error(WhisperError::Cancelled);
        assert_eq!(c.status(), "cancelled");
        assert_eq!(c.details(), None);
    }
}
