use crate::tools::tool_paths;
use subforge_core::tools::dependency_report;
use subforge_deps::{plan_for, run_plan, Context, DepId, DepReport};
use tauri::{AppHandle, Emitter};

#[tauri::command]
pub async fn check_dependencies() -> Result<DepReport, String> {
    tokio::task::spawn_blocking(|| dependency_report(&tool_paths()))
        .await
        .map_err(|e| e.to_string())
}

/// Installs one dependency, streaming installer output as `dep-install-progress`
/// events, and returns a fresh report so the UI updates immediately.
#[tauri::command]
pub async fn install_dependency(app: AppHandle, id: String) -> Result<DepReport, String> {
    let dep = DepId::parse(&id).ok_or_else(|| format!("Unknown dependency '{id}'"))?;

    tokio::task::spawn_blocking(move || {
        let ctx = Context::from_process();
        let plan = plan_for(dep, &ctx).map_err(|e| e.to_string())?;
        if !plan.auto_runnable {
            return Err(format!(
                "This needs administrator rights and no graphical password prompt (pkexec) was found. Run this in a terminal: {}",
                plan.display_command
            ));
        }

        let result = run_plan(&plan, |line| {
            let _ = app.emit(
                "dep-install-progress",
                serde_json::json!({ "job_id": id, "message": line }),
            );
        });

        let ok = result.is_ok();
        let message = match &result {
            Ok(()) => "Installed".to_string(),
            Err(e) => e.to_string(),
        };
        let _ = app.emit(
            "dep-install-done",
            serde_json::json!({
                "job_id": id,
                "status": if ok { "completed" } else { "failed" },
                "message": message,
            }),
        );

        result.map_err(|e| e.to_string())?;
        // Fresh context: PATH-independent lookups now see the new install.
        Ok(dependency_report(&tool_paths()))
    })
    .await
    .map_err(|e| e.to_string())?
}
