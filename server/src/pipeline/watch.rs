//! The folder watcher: turn new episodes into plans.
//!
//! Deliberately conservative. It only runs when a policy file enables it, it ignores partial
//! downloads and sidecar files, it waits for a file to stop changing before looking at it, and
//! while `dry_run` is on it logs the plan instead of touching anything. A policy that asks for
//! `dry_run: false` is refused: execution needs the engines wired and the scheduler's heavy
//! slot, and pretending otherwise would be worse than doing nothing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};
use subforge_pipeline::Policy;

use super::is_episode_candidate;
use super::PipelineConfig;

/// How long a file has to stay untouched before it counts as finished.
const SETTLE: Duration = Duration::from_secs(3);

/// Starts the watcher if the policy enables it. Never fails: a server without a policy, or
/// with an unreadable folder, still has to start and serve the rest of its API.
pub fn spawn() {
    let config = PipelineConfig::load();
    if !config.enabled() {
        eprintln!("[pipeline] no watch_dirs/target_languages configured; watcher is off");
        return;
    }
    if !config.policy.dry_run {
        eprintln!(
            "[pipeline] execution is not wired yet, so dry_run=false is refused; \
             keep dry_run on to see the plans in the log"
        );
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
        "[pipeline] watching {watched} folder(s) for {} (dry run)",
        config.policy.target_languages.join(", ")
    );
    let policy = config.policy.clone();
    // The watcher must outlive this function; it is moved into the thread with the receiver.
    std::thread::spawn(move || run(rx, policy, watcher));
}

/// Collects paths, waits until each stops changing, then plans it.
fn run(rx: mpsc::Receiver<PathBuf>, policy: Policy, _watcher: impl Watcher + Send) {
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
            match plan_one(&path, &policy) {
                Ok(Some(plan)) if plan.is_noop() => {
                    eprintln!("[pipeline] {} already satisfies the policy", path.display());
                }
                Ok(Some(plan)) => {
                    eprintln!(
                        "[pipeline] {} needs {} language(s); plan: {}",
                        path.display(),
                        plan.missing.len(),
                        describe(&plan.steps)
                    );
                }
                Ok(None) => {}
                Err(error) => eprintln!("[pipeline] cannot plan {}: {error}", path.display()),
            }
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
}
