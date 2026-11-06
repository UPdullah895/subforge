//! The folder watcher: turn new episodes into plans, and plans into files.
//!
//! Conservative on purpose. It only runs when a policy file enables it, it ignores partial
//! downloads and sidecar files, and it waits for a file to stop changing before looking at it.
//! While `dry_run` is on it logs the plan and touches nothing. When it is off, every file goes
//! through the scheduler's heavy slot, so a season unpacked in one go becomes one transcription
//! at a time instead of ten.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};
use subforge_pipeline::Policy;
use tokio::runtime::Handle;

use super::arr::ArrEndpoint;
use super::{is_episode_candidate, PipelineConfig};
use crate::jobs::scheduler::{JobSource, Scheduler};
use crate::jobs::JobType;

/// How long a file has to stay untouched before it counts as finished.
const SETTLE: Duration = Duration::from_secs(3);

/// Starts the triggers: one task per configured *arr, plus the folder watcher when folders are
/// configured. Never fails: a server without a policy, or with an unreadable folder, still has to
/// start and serve the rest of its API.
pub fn spawn(scheduler: Scheduler, queue: crate::jobs::SharedJobQueue) {
    let config = PipelineConfig::load();
    let handle = Handle::current();

    // The *arr feed is a separate trigger: an instance with no watch folders is a legitimate
    // setup ("let Sonarr tell me"), so it is started before the folder check.
    for endpoint in config.arr.clone() {
        spawn_arr(
            endpoint,
            config.policy.clone(),
            scheduler.clone(),
            queue.clone(),
            handle.clone(),
        );
    }

    if config.policy.target_languages.is_empty() {
        eprintln!("[pipeline] the policy has no target languages; nothing would be planned");
        return;
    }
    if config.watch_dirs.is_empty() {
        eprintln!("[pipeline] no watch_dirs configured; only the *arr feed triggers work");
        return;
    }

    let (tx, rx) = mpsc::channel::<PathBuf>();
    let mut watcher =
        match notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event {
                if event.kind.is_create() || event.kind.is_modify() {
                    for path in event.paths {
                        let _ = tx.send(path);
                    }
                }
            }
        }) {
            Ok(watcher) => watcher,
            Err(error) => {
                eprintln!("[pipeline] cannot create a watcher: {error}");
                return;
            }
        };

    let mode = if config.recursive {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    };
    let mut watched = 0;
    for dir in &config.watch_dirs {
        match watcher.watch(Path::new(dir), mode) {
            Ok(()) => watched += 1,
            Err(error) => eprintln!("[pipeline] cannot watch {dir}: {error}"),
        }
    }
    if watched == 0 {
        eprintln!("[pipeline] no folder could be watched; watcher is off");
        return;
    }

    eprintln!(
        "[pipeline] watching {watched} folder(s) for {} ({})",
        config.policy.target_languages.join(", "),
        if config.policy.dry_run {
            "dry run"
        } else {
            "live"
        }
    );
    let policy = config.policy.clone();
    // The watcher must outlive this function; it is moved into the thread with the receiver.
    std::thread::spawn(move || run(rx, policy, watcher, scheduler, queue, handle));
}

/// Follows one *arr instance and processes every file it reports as imported.
fn spawn_arr(
    endpoint: ArrEndpoint,
    policy: Policy,
    scheduler: Scheduler,
    queue: crate::jobs::SharedJobQueue,
    handle: Handle,
) {
    eprintln!("[arr] following {}", endpoint.url);
    let on_import: crate::pipeline::arr::OnImport = Box::new(move |import| {
        let path = PathBuf::from(&import.path);
        handle_file(&path, &policy, &scheduler, &queue, &handle);
    });
    tokio::spawn(super::arr::follow(endpoint, on_import));
}

/// What happens to one finished file: a logged plan, or a job on the heavy slot.
pub fn handle_file(
    path: &Path,
    policy: &Policy,
    scheduler: &Scheduler,
    queue: &crate::jobs::SharedJobQueue,
    handle: &Handle,
) {
    if policy.dry_run {
        match plan_one(path, policy) {
            Ok(Some(plan)) if plan.is_noop() => {
                eprintln!("[pipeline] {} already satisfies the policy", path.display())
            }
            Ok(Some(plan)) => eprintln!(
                "[pipeline] {} needs {} language(s); plan: {}",
                path.display(),
                plan.missing.len(),
                describe(&plan.steps)
            ),
            Ok(None) => {}
            Err(error) => eprintln!("[pipeline] cannot plan {}: {error}", path.display()),
        }
        return;
    }

    if !path.is_file() {
        return;
    }
    let file = path.to_string_lossy().to_string();
    let job_id = format!("pipeline-{}", uuid::Uuid::new_v4());
    let policy = policy.clone();
    let label = file.clone();
    let queue = queue.clone();

    // The job list is the audit trail: without these two updates a failed episode would show up
    // as "completed" because the queue only knows what the caller tells it.
    {
        let mut jobs = queue.lock().unwrap_or_else(|error| error.into_inner());
        jobs.add(crate::jobs::Job {
            id: job_id.clone(),
            job_type: JobType::BatchOperation,
            status: crate::jobs::JobStatus::Pending,
            input_file: file.clone(),
            output_file: None,
            progress: 0.0,
            message: Some("queued by the pipeline".to_string()),
        });
    }

    // `gated` spawns the job, which needs a runtime context even from the watcher's thread.
    let _entered = handle.enter();
    scheduler.gated(
        job_id.clone(),
        JobType::BatchOperation,
        JobSource::Automation,
        file.clone(),
        async move {
            {
                let mut jobs = queue.lock().unwrap_or_else(|error| error.into_inner());
                jobs.update_status(&job_id, crate::jobs::JobStatus::Running);
            }
            let outcome = super::run::process(file, policy).await;
            let mut jobs = queue.lock().unwrap_or_else(|error| error.into_inner());
            match outcome {
                Ok(summary) => {
                    eprintln!("[pipeline] {summary}");
                    jobs.update_progress(&job_id, 100.0, Some(summary));
                    jobs.update_status(&job_id, crate::jobs::JobStatus::Completed);
                }
                Err(error) => {
                    eprintln!("[pipeline] {label}: {error}");
                    jobs.update_progress(&job_id, 0.0, Some(error));
                    jobs.update_status(&job_id, crate::jobs::JobStatus::Failed);
                }
            }
        },
    );
}

/// Collects paths, waits until each stops changing, then hands it over.
fn run(
    rx: mpsc::Receiver<PathBuf>,
    policy: Policy,
    _watcher: impl Watcher + Send,
    scheduler: Scheduler,
    queue: crate::jobs::SharedJobQueue,
    handle: Handle,
) {
    let mut pending: HashMap<PathBuf, Instant> = HashMap::new();
    loop {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(path) => {
                if is_episode_candidate(&path) && path.is_file() {
                    // Re-arm the timer on every event: a copy still in progress keeps moving.
                    pending.insert(path, Instant::now());
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }

        let ready: Vec<PathBuf> = pending
            .iter()
            .filter(|(_, seen)| seen.elapsed() >= SETTLE)
            .map(|(path, _)| path.clone())
            .collect();

        for path in ready {
            pending.remove(&path);
            handle_file(&path, &policy, &scheduler, &queue, &handle);
        }
    }
}

/// Plans one file, or `None` when there is nothing to inspect.
pub fn plan_one(path: &Path, policy: &Policy) -> Result<Option<subforge_pipeline::Plan>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let paths = crate::tools::tool_paths();
    let file = path.to_string_lossy().to_string();
    subforge_pipeline::plan_file(&paths, &file, policy).map(Some)
}

fn describe(steps: &[subforge_pipeline::Step]) -> String {
    steps
        .iter()
        .map(|step| match step {
            subforge_pipeline::Step::SearchOnline { language } => format!("search:{language}"),
            subforge_pipeline::Step::Transcribe { .. } => "transcribe".to_string(),
            subforge_pipeline::Step::Translate { to, .. } => format!("translate:{to}"),
            subforge_pipeline::Step::Lint { language, .. } => format!("lint:{language}"),
            subforge_pipeline::Step::Mux { language } => format!("mux:{language}"),
            subforge_pipeline::Step::Verify { language } => format!("verify:{language}"),
            subforge_pipeline::Step::CleanupSource { language } => format!("cleanup:{language}"),
        })
        .collect::<Vec<_>>()
        .join(" -> ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn a_non_file_is_not_planned() {
        let policy = Policy::arabic();
        assert!(plan_one(Path::new("/definitely/not/here.mkv"), &policy)
            .unwrap()
            .is_none());
    }

    #[test]
    fn the_step_description_is_readable() {
        let steps = vec![
            subforge_pipeline::Step::SearchOnline {
                language: "ar".into(),
            },
            subforge_pipeline::Step::Mux {
                language: "ar".into(),
            },
        ];
        assert_eq!(describe(&steps), "search:ar -> mux:ar");
    }

    #[test]
    fn a_dry_run_hands_nothing_to_the_scheduler() {
        let dir = std::env::temp_dir().join(format!("sf_watch_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("E01.mkv");
        std::fs::write(&file, b"x").unwrap();

        let queue: crate::jobs::SharedJobQueue =
            std::sync::Arc::new(std::sync::Mutex::new(crate::jobs::JobQueue::new()));
        let scheduler = Scheduler::with_journal(
            queue.clone(),
            crate::jobs::create_event_channel().0,
            Some(dir.join("journal")),
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();

        // A dry run must not enqueue anything, even for a file it cannot plan.
        handle_file(&file, &Policy::arabic(), &scheduler, &queue, &handle);
        assert!(scheduler.snapshot().is_empty());
        assert!(
            queue.lock().unwrap().jobs.is_empty(),
            "a dry run must not create job entries"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
