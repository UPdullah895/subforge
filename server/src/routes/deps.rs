use crate::error::{blocking, err400, err403, err500, AppError};
use crate::jobs::JobStatus;
use crate::{jobs, AppState};
use axum::{extract::State, Json};
use serde::Deserialize;
use subforge_deps::{check_all, plan_for, run_plan, Context, DepId, DepReport};

pub async fn get_deps() -> Result<Json<DepReport>, AppError> {
    let report = blocking(|| check_all(&Context::from_process()))
        .await
        .map_err(|_| err500("dependency check failed"))?;
    Ok(Json(report))
}

#[derive(Deserialize)]
pub struct InstallRequest {
    pub id: String,
}

fn emit(sender: &jobs::EventSender, event_type: &str, id: &str, status: Option<JobStatus>, message: String) {
    jobs::emit_event(
        sender,
        jobs::JobEvent {
            event_type: event_type.to_string(),
            job_id: id.to_string(),
            status,
            progress: None,
            message: Some(message),
            stage: None,
            output_path: None,
            details: None,
        },
    );
}

/// Installs software on the machine running the server, so it must be switched
/// on explicitly with `SUBFORGE_ALLOW_INSTALL=1`.
pub async fn install_dep(
    State(state): State<AppState>,
    Json(req): Json<InstallRequest>,
) -> Result<Json<DepReport>, AppError> {
    let dep = DepId::parse(&req.id).ok_or_else(|| err400(format!("Unknown dependency '{}'", req.id)))?;

    let ctx = Context::from_process();
    let plan = plan_for(dep, &ctx).map_err(|e| err400(e.to_string()))?;

    if std::env::var("SUBFORGE_ALLOW_INSTALL").as_deref() != Ok("1") {
        return Err(err403(format!(
            "Installing software from the web UI is disabled on this server. Run this on the server, or start it with SUBFORGE_ALLOW_INSTALL=1: {}",
            plan.display_command
        )));
    }
    if !plan.auto_runnable {
        return Err(err400(format!(
            "This needs administrator rights and no graphical password prompt (pkexec) was found. Run this in a terminal: {}",
            plan.display_command
        )));
    }

    let sender = state.event_sender.clone();
    let id = req.id.clone();
    let result = blocking(move || {
        let result = run_plan(&plan, |line| emit(&sender, "dep-install-progress", &id, None, line.to_string()));
        let (status, message) = match &result {
            Ok(()) => (JobStatus::Completed, "Installed".to_string()),
            Err(e) => (JobStatus::Failed, e.to_string()),
        };
        emit(&sender, "dep-install-done", &id, Some(status), message);
        result.map(|_| check_all(&Context::from_process()))
    })
    .await
    .map_err(|_| err500("install task crashed"))?;

    result.map(Json).map_err(|e| err500(e.to_string()))
}
