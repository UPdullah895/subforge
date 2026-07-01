use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
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

#[derive(Debug, Clone, Serialize)]
pub struct JobEvent {
    pub event_type: String,
    pub job_id: String,
    pub status: Option<JobStatus>,
    pub progress: Option<f64>,
    pub message: Option<String>,
}

pub type EventSender = broadcast::Sender<JobEvent>;
pub type EventReceiver = broadcast::Receiver<JobEvent>;

pub fn create_event_channel() -> (EventSender, EventReceiver) {
    broadcast::channel(128)
}

pub fn emit_event(sender: &EventSender, event: JobEvent) {
    let _ = sender.send(event);
}
