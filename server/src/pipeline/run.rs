//! Turning a plan into files: one function, used by the folder watcher and the *arr feed.
//!
//! Everything that can rewrite a library goes through here, and the caller is responsible for
//! holding the scheduler's slot around it — this module does the work, not the queueing.

use subforge_pipeline::exec::{execute, Outcome};
use subforge_pipeline::runner::CoreRunner;
use subforge_pipeline::{plan_file, Policy, Step};

/// Where intermediate subtitles live: inside the config directory, never in the library.
pub fn work_dir() -> std::path::PathBuf {
    crate::config::AppConfig::config_dir()
        .map(|dir| dir.join("pipeline-work"))
        .unwrap_or_else(|_| std::env::temp_dir().join("subforge-pipeline"))
}

fn describe(steps: &[Step]) -> String {
    steps
        .iter()
        .map(|step| match step {
            Step::SearchOnline { language } => format!("search:{language}"),
            Step::Transcribe { .. } => "transcribe".to_string(),
            Step::Translate { to, .. } => format!("translate:{to}"),
            Step::Lint { language, .. } => format!("lint:{language}"),
            Step::Mux { language } => format!("mux:{language}"),
            Step::Verify { language } => format!("verify:{language}"),
            Step::CleanupSource { language } => format!("cleanup:{language}"),
        })
        .collect::<Vec<_>>()
        .join(" -> ")
}

/// Plans one file and, unless the policy is a dry run, executes the plan.
///
/// Returns the summary a human (or the job log) reads. An error means the episode is **not** in
/// the state the policy asked for; the pipeline's own atomic publish means the file is either
/// the original or the finished version, never something in between.
pub async fn process(
    file: String,
    policy: Policy,
    arr: Option<crate::pipeline::arr::ArrRef>,
) -> Result<String, String> {
    let paths = crate::tools::tool_paths();
    let plan = {
        let file = file.clone();
        let paths = paths.clone();
        let policy = policy.clone();
        tokio::task::spawn_blocking(move || plan_file(&paths, &file, &policy))
            .await
            .map_err(|error| format!("planning panicked: {error}"))??
    };

    if plan.is_noop() {
        return Ok(format!("{file}: already satisfies the policy"));
    }
    if policy.dry_run {
        return Ok(format!(
            "{file}: dry run, {} step(s): {}",
            plan.steps.len(),
            describe(&plan.steps)
        ));
    }

    // The runner drives async engines from a blocking task, so it needs a handle to this runtime.
    let translate = crate::pipeline::engines::translator(policy.translate_engine.clone())?;
    let work_dir = work_dir();
    // Search is optional: with no provider key configured it answers "nothing found" and the
    // translation path runs, which is the documented fallback rather than an error.
    let search = crate::pipeline::engines::searcher(work_dir.clone()).ok();
    let (ok, summary) = tokio::task::spawn_blocking(move || {
        let mut runner = CoreRunner::new(paths, work_dir.clone())
            .with_translate(translate)
            .with_transcribe(crate::pipeline::engines::transcriber(
                work_dir.clone(),
                None,
            ));
        if let Some(search) = search {
            runner = runner.with_search(search);
        }
        let report = execute(&plan, &mut runner);
        let mut parts: Vec<String> = Vec::new();
        for result in &report.results {
            // Name the step: a summary that says only "failed: ..." leaves the user guessing
            // which part of the pipeline is talking.
            let step = match &result.step {
                Step::SearchOnline { .. } => "search",
                Step::Transcribe { .. } => "transcribe",
                Step::Translate { .. } => "translate",
                Step::Lint { .. } => "lint",
                Step::Mux { .. } => "mux",
                Step::Verify { .. } => "verify",
                Step::CleanupSource { .. } => "cleanup",
            };
            match &result.outcome {
                Outcome::Failed(error) => parts.push(format!("{step} failed: {error}")),
                Outcome::Skipped(reason) => parts.push(format!("{step} skipped: {reason}")),
                Outcome::Done(detail) => parts.push(format!("{step}: {detail}")),
            }
        }
        (report.ok, format!("{file}: {}", parts.join("; ")))
    })
    .await
    .map_err(|error| format!("the pipeline panicked: {error}"))?;

    if !ok {
        return Err(summary);
    }

    // The episode is correct on disk; telling the *arr is a courtesy, so a failure is logged and
    // swallowed rather than turning a finished job into a failed one.
    if let Some(reference) = arr {
        let client = reqwest::Client::new();
        if let Err(error) = crate::pipeline::arr::request_rescan(&client, &reference).await {
            eprintln!(
                "[arr] {} was not told to rescan: {error}",
                reference.endpoint.url
            );
        }
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_step_description_reads_like_a_plan() {
        let steps = vec![
            Step::SearchOnline {
                language: "ar".into(),
            },
            Step::Translate {
                from: Some("ja".into()),
                to: "ar".into(),
                only_if_no_subtitle: true,
            },
            Step::Lint {
                language: "ar".into(),
                profile: subforge_pipeline::RuleProfile::NetflixAdult,
            },
            Step::Mux {
                language: "ar".into(),
            },
        ];
        assert_eq!(
            describe(&steps),
            "search:ar -> translate:ar -> lint:ar -> mux:ar"
        );
    }

    #[test]
    fn the_work_directory_is_under_the_config_directory() {
        let dir = work_dir();
        assert!(dir.ends_with("pipeline-work"), "{}", dir.display());
    }
}
