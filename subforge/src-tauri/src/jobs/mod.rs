use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::process::Command;

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
        .args(args)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    let (status, message) = if output.status.success() {
        (
            JobStatus::Completed,
            Some(if stdout.is_empty() { "Done".to_string() } else { stdout.trim().to_string() }),
        )
    } else {
        (
            JobStatus::Failed,
            Some(if stderr.is_empty() { "Unknown error".to_string() } else { stderr.trim().to_string() }),
        )
    };

    {
        let mut q = queue.lock().map_err(|e| e.to_string())?;
        q.update_status(&job_id, status.clone());
        q.update_progress(&job_id, 100.0, message.clone());
    }

    let _ = app.emit("job-completed", serde_json::json!({
        "job_id": job_id,
        "status": status,
        "message": message,
    }));

    Ok(())
}
