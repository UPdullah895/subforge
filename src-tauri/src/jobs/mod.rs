use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use subforge_core::CancellationToken;
use subforge_deps::HideWindow;
use tauri::{AppHandle, Emitter};
use tokio::process::Command;
use tokio::sync::Notify;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Paused,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JobType {
    ExtractSubtitle,
    ExtractAudio,
    MergeSubtitle,
    MergeAudio,
    RemoveTracks,
    ConvertSubtitle,
    ShiftSubtitle,
    BatchOperation,
    Transcribe,
    Translate,
    DownloadModel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub job_type: JobType,
    pub status: JobStatus,
    pub input_file: String,
    pub output_file: Option<String>,
    pub progress: f64,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobQueue {
    pub jobs: Vec<Job>,
}

impl Default for JobQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl JobQueue {
    pub fn new() -> Self {
        Self { jobs: Vec::new() }
    }

    pub fn add(&mut self, job: Job) -> String {
        let id = job.id.clone();
        self.jobs.push(job);
        id
    }

    pub fn get(&self, id: &str) -> Option<&Job> {
        self.jobs.iter().find(|j| j.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|j| j.id == id)
    }

    pub fn update_status(&mut self, id: &str, status: JobStatus) {
        if let Some(job) = self.get_mut(id) {
            job.status = status;
        }
    }

    pub fn update_progress(&mut self, id: &str, progress: f64, message: Option<String>) {
        if let Some(job) = self.get_mut(id) {
            job.progress = progress;
            if let Some(msg) = message {
                job.message = Some(msg);
            }
        }
    }
}

pub type SharedJobQueue = Arc<Mutex<JobQueue>>;

#[derive(Debug)]
pub struct JobControl {
    cancelled: AtomicBool,
    paused: AtomicBool,
    notify: Notify,
    /// Cancelled together with the job; handed to `subforge-core` operations.
    token: CancellationToken,
}

impl Default for JobControl {
    fn default() -> Self {
        Self::new()
    }
}

impl JobControl {
    pub fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            notify: Notify::new(),
            token: CancellationToken::new(),
        }
    }

    pub fn token(&self) -> &CancellationToken {
        &self.token
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.token.cancel();
        self.notify.notify_waiters();
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    /// Waits until resumed or cancelled. Uses the documented Notify idiom
    /// (create the `notified()` future, re-check the condition, THEN await
    /// it) to avoid the missed-wakeup race between checking `is_paused()`
    /// and a concurrent `resume()`/`cancel()` call.
    pub async fn wait_if_paused(&self) {
        loop {
            if self.is_cancelled() || !self.is_paused() {
                return;
            }
            let notified = self.notify.notified();
            if self.is_cancelled() || !self.is_paused() {
                return;
            }
            notified.await;
        }
    }
}

pub type JobControlRegistry = Arc<Mutex<std::collections::HashMap<String, Arc<JobControl>>>>;

pub async fn run_command_job(
    app: AppHandle,
    queue: SharedJobQueue,
    job_id: String,
    program: &str,
    args: &[String],
) -> Result<(), String> {
    {
        let mut q = queue.lock().map_err(|e| e.to_string())?;
        q.update_status(&job_id, JobStatus::Running);
    }

    let _ = app.emit("job-started", &job_id);

    let output = Command::new(program)
        .hide_window()
        .args(args)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    let (status, message) = if output.status.success() {
        (
            JobStatus::Completed,
            Some(if stdout.is_empty() {
                "Done".to_string()
            } else {
                stdout.trim().to_string()
            }),
        )
    } else {
        (
            JobStatus::Failed,
            Some(if stderr.is_empty() {
                "Unknown error".to_string()
            } else {
                stderr.trim().to_string()
            }),
        )
    };

    {
        let mut q = queue.lock().map_err(|e| e.to_string())?;
        q.update_status(&job_id, status.clone());
        q.update_progress(&job_id, 100.0, message.clone());
    }

    let _ = app.emit(
        "job-completed",
        serde_json::json!({
            "job_id": job_id,
            "status": status,
            "message": message,
        }),
    );

    Ok(())
}
