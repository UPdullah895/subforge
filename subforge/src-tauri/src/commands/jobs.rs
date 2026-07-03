use crate::jobs::JobControlRegistry;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobControlRequest {
    pub job_id: String,
}

#[tauri::command]
pub fn cancel_job(
    request: JobControlRequest,
    controls: State<'_, JobControlRegistry>,
) -> Result<(), String> {
    let registry = controls.lock().map_err(|e| e.to_string())?;
    match registry.get(&request.job_id) {
        Some(control) => {
            control.cancel();
            Ok(())
        }
        None => Err(format!("No active job '{}'", request.job_id)),
    }
}

#[tauri::command]
pub fn pause_job(
    request: JobControlRequest,
    controls: State<'_, JobControlRegistry>,
) -> Result<(), String> {
    let registry = controls.lock().map_err(|e| e.to_string())?;
    match registry.get(&request.job_id) {
        Some(control) => {
            control.pause();
            Ok(())
        }
        None => Err(format!("No active job '{}'", request.job_id)),
    }
}

#[tauri::command]
pub fn resume_job(
    request: JobControlRequest,
    controls: State<'_, JobControlRegistry>,
) -> Result<(), String> {
    let registry = controls.lock().map_err(|e| e.to_string())?;
    match registry.get(&request.job_id) {
        Some(control) => {
            control.resume();
            Ok(())
        }
        None => Err(format!("No active job '{}'", request.job_id)),
    }
}
