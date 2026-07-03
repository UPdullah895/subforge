use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::broadcast;
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
}

impl JobControl {
    pub fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            notify: Notify::new(),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
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

#[derive(Debug, Clone, Serialize)]
pub struct JobEvent {
    pub event_type: String,
    pub job_id: String,
    pub status: Option<JobStatus>,
    pub progress: Option<f64>,
    pub message: Option<String>,
    pub stage: Option<String>,
    pub output_path: Option<String>,
}

pub type EventSender = broadcast::Sender<JobEvent>;
pub type EventReceiver = broadcast::Receiver<JobEvent>;

pub fn create_event_channel() -> (EventSender, EventReceiver) {
    broadcast::channel(128)
}

pub fn emit_event(sender: &EventSender, event: JobEvent) {
    let _ = sender.send(event);
}
