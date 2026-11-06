//! The executing half: walk a [`Plan`], one step at a time, and stop at the first failure.
//!
//! Everything the executor needs from the outside world sits behind [`StepRunner`], so the
//! order of operations can be tested with a fake runner (no ffmpeg, no model, no network),
//! a dry run can be produced by a runner that only records, and the server can plug in the
//! real engines without the state machine changing.
//!
//! Two rules matter more than the rest and are enforced here rather than left to callers:
//! a **verify that fails never allows the cleanup step**, and once a mux fails for a
//! language, the remaining steps of that language are skipped instead of compounding the
//! damage.

use crate::{Plan, Step};
use serde::{Deserialize, Serialize};

/// What a step produced, for the report and for the caller's log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// The step ran; the string is what it produced or touched (a subtitle path, a language).
    Done(String),
    /// Nothing to do, and that is fine (an online search found no human subtitle, say).
    Skipped(String),
    /// The step failed; everything after it in the same language is skipped.
    Failed(String),
}

/// One step and what came of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepResult {
    pub step: Step,
    pub outcome: Outcome,
}

/// Result of running a whole plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub file: String,
    pub results: Vec<StepResult>,
    /// True when every step either ran or was legitimately skipped.
    pub ok: bool,
}

impl Report {
    pub fn failures(&self) -> impl Iterator<Item = &StepResult> {
        self.results
            .iter()
            .filter(|result| matches!(result.outcome, Outcome::Failed(_)))
    }
}

/// Everything the executor needs from the outside world.
///
/// Paths are plain strings because that is what the core tool wrappers take; a runner may
/// write its intermediate files wherever it likes.
pub trait StepRunner {
    /// An existing human subtitle for this language, if the providers have one.
    fn search_online(&mut self, file: &str, language: &str) -> Result<Option<String>, String>;
    /// Transcribe the file's audio; `language` is a hint, not a requirement.
    fn transcribe(&mut self, file: &str, language: Option<&str>) -> Result<String, String>;
    /// Pull the subtitle already inside the file out to a file (needed before translating it).
    fn extract_existing(&mut self, file: &str, language: Option<&str>) -> Result<String, String>;
    /// Machine-translate a subtitle file.
    fn translate(&mut self, subtitle: &str, from: Option<&str>, to: &str)
        -> Result<String, String>;
    /// Mux a subtitle into the video; returns the path of the file now carrying it.
    fn mux(&mut self, file: &str, subtitle: &str, language: &str) -> Result<String, String>;
    /// Re-read the muxed file and confirm the track is there, complete and in sync.
    fn verify(&mut self, file: &str, language: &str) -> Result<(), String>;
    /// Delete the external subtitle a successful mux came from.
    fn cleanup(&mut self, subtitle: &str) -> Result<(), String>;
    /// Apply a readability rule-set to a subtitle in place. The default is a no-op so a runner
    /// that cannot lint reports "skipped" instead of lying about having fixed the file.
    fn lint(
        &mut self,
        _subtitle: &str,
        _profile: crate::RuleProfile,
    ) -> Result<Option<String>, String> {
        Ok(None)
    }
}

/// Runs every step of `plan` in order, stopping a language at its first failure.
pub fn execute<R: StepRunner>(plan: &Plan, runner: &mut R) -> Report {
    let mut results: Vec<StepResult> = Vec::new();
    let mut video = plan.file.clone();
    // The subtitle produced for the language being processed (search, transcribe, translate).
    let mut pending: Option<String> = None;
    let mut language_failed = false;
    let mut current_language: Option<String> = None;

    for step in &plan.steps {
        let language = step_language(step);
        if language != current_language {
            // A new language block starts clean.
            current_language = language.clone();
            language_failed = false;
            pending = None;
        }

        if language_failed {
            results.push(StepResult {
                step: step.clone(),
                outcome: Outcome::Skipped("an earlier step for this language failed".to_string()),
            });
            continue;
        }

        let outcome = match step {
            Step::SearchOnline { language } => match runner.search_online(&video, language) {
                Ok(Some(path)) => {
                    pending = Some(path.clone());
                    Outcome::Done(path)
                }
                Ok(None) => {
                    Outcome::Skipped("no human subtitle found; translating instead".to_string())
                }
                Err(error) => {
                    // A provider being down must not stop the pipeline: translation is the fallback.
                    Outcome::Skipped(format!("search failed ({error}); translating instead"))
                }
            },

            Step::Transcribe { language } => match runner.transcribe(&video, language.as_deref()) {
                Ok(path) => {
                    pending = Some(path.clone());
                    Outcome::Done(path)
                }
                Err(error) => {
                    language_failed = true;
                    Outcome::Failed(error)
                }
            },

            Step::Translate {
                from,
                to,
                only_if_no_subtitle,
            } => {
                if *only_if_no_subtitle && pending.is_some() {
                    results.push(StepResult {
                        step: step.clone(),
                        outcome: Outcome::Skipped(
                            "a human subtitle was found; nothing to translate".to_string(),
                        ),
                    });
                    continue;
                }
                let source = match pending.clone() {
                    Some(path) => path,
                    None => match runner.extract_existing(&video, from.as_deref()) {
                        Ok(path) => path,
                        Err(error) => {
                            language_failed = true;
                            results.push(StepResult {
                                step: step.clone(),
                                outcome: Outcome::Failed(error),
                            });
                            continue;
                        }
                    },
                };
                match runner.translate(&source, from.as_deref(), to) {
                    Ok(path) => {
                        pending = Some(path.clone());
                        Outcome::Done(path)
                    }
                    Err(error) => {
                        language_failed = true;
                        Outcome::Failed(error)
                    }
                }
            }

            Step::Lint { profile, .. } => match pending.clone() {
                None => Outcome::Skipped("no subtitle to lint".to_string()),
                Some(subtitle) => match runner.lint(&subtitle, *profile) {
                    // `Some(report)` is the summary the user sees; `None` means the runner has no
                    // linter, which is a skip, not a success.
                    Ok(Some(report)) => Outcome::Done(report),
                    Ok(None) => Outcome::Skipped("this runner cannot lint subtitles".to_string()),
                    Err(error) => Outcome::Failed(error),
                },
            },

            Step::Mux { language } => match pending.clone() {
                None => {
                    language_failed = true;
                    Outcome::Failed("no subtitle was produced to mux".to_string())
                }
                Some(subtitle) => match runner.mux(&video, &subtitle, language) {
                    Ok(path) => {
                        video = path.clone();
                        Outcome::Done(path)
                    }
                    Err(error) => {
                        language_failed = true;
                        Outcome::Failed(error)
                    }
                },
            },

            Step::Verify { language } => match runner.verify(&video, language) {
                Ok(()) => Outcome::Done(format!("{language} verified")),
                Err(error) => {
                    // A failed verify blocks the cleanup step, which is the whole point of it.
                    language_failed = true;
                    Outcome::Failed(error)
                }
            },

            Step::CleanupSource { .. } => match pending.take() {
                Some(subtitle) => match runner.cleanup(&subtitle) {
                    Ok(()) => Outcome::Done(format!("removed {subtitle}")),
                    Err(error) => Outcome::Failed(error),
                },
                None => Outcome::Skipped("nothing to clean up".to_string()),
            },
        };

        results.push(StepResult {
            step: step.clone(),
            outcome,
        });
    }

    let ok = !results
        .iter()
        .any(|result| matches!(result.outcome, Outcome::Failed(_)));
    Report {
        file: plan.file.clone(),
        results,
        ok,
    }
}

fn step_language(step: &Step) -> Option<String> {
    match step {
        Step::SearchOnline { language }
        | Step::Mux { language }
        | Step::Verify { language }
        | Step::CleanupSource { language } => Some(language.clone()),
        Step::Lint { language, .. } => Some(language.clone()),
        Step::Translate { to, .. } => Some(to.clone()),
        Step::Transcribe { language } => language.clone(),
    }
}

/// A runner that changes nothing and records what it was asked to do — the dry run a library
/// owner reviews before a policy is allowed to touch files.
#[derive(Debug, Default)]
pub struct DryRun {
    pub calls: Vec<String>,
    produced: usize,
}

impl DryRun {
    fn fake_subtitle(&mut self, language: &str, extension: &str) -> String {
        self.produced += 1;
        format!("/dry-run/{}-{}.{}", language, self.produced, extension)
    }
}

impl StepRunner for DryRun {
    fn search_online(&mut self, file: &str, language: &str) -> Result<Option<String>, String> {
        self.calls.push(format!("search {language} in {file}"));
        Ok(None)
    }

    fn transcribe(&mut self, file: &str, language: Option<&str>) -> Result<String, String> {
        self.calls.push(format!(
            "transcribe {file} ({})",
            language.unwrap_or("auto")
        ));
        Ok(self.fake_subtitle(language.unwrap_or("auto"), "srt"))
    }

    fn extract_existing(&mut self, file: &str, language: Option<&str>) -> Result<String, String> {
        self.calls.push(format!(
            "extract {} from {file}",
            language.unwrap_or("first subtitle")
        ));
        Ok(self.fake_subtitle(language.unwrap_or("src"), "srt"))
    }

    fn translate(
        &mut self,
        subtitle: &str,
        from: Option<&str>,
        to: &str,
    ) -> Result<String, String> {
        self.calls.push(format!(
            "translate {subtitle} {}->{to}",
            from.unwrap_or("auto")
        ));
        Ok(self.fake_subtitle(to, "srt"))
    }

    fn mux(&mut self, file: &str, subtitle: &str, language: &str) -> Result<String, String> {
        self.calls
            .push(format!("mux {subtitle} as {language} into {file}"));
        Ok(file.to_string())
    }

    fn verify(&mut self, file: &str, language: &str) -> Result<(), String> {
        self.calls.push(format!("verify {language} in {file}"));
        Ok(())
    }

    fn cleanup(&mut self, subtitle: &str) -> Result<(), String> {
        self.calls.push(format!("delete {subtitle}"));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{plan, Inventory, Policy};

    /// Records calls and can be told to fail at a given step name.
    #[derive(Default)]
    struct Fake {
        calls: Vec<String>,
        fail_at: Option<&'static str>,
        search_hit: bool,
        counter: usize,
    }

    impl Fake {
        fn fail(mut self, step: &'static str) -> Self {
            self.fail_at = Some(step);
            self
        }
        fn with_search_hit(mut self) -> Self {
            self.search_hit = true;
            self
        }
        fn next(&mut self, language: &str) -> String {
            self.counter += 1;
            format!("/tmp/{language}-{}.srt", self.counter)
        }
        fn record(&mut self, call: String, step: &'static str) -> Result<(), String> {
            self.calls.push(call);
            if self.fail_at == Some(step) {
                return Err(format!("{step} failed on purpose"));
            }
            Ok(())
        }
    }

    impl StepRunner for Fake {
        fn search_online(&mut self, file: &str, language: &str) -> Result<Option<String>, String> {
            self.record(format!("search {language} {file}"), "search")?;
            Ok(self.search_hit.then(|| self.next(language)))
        }
        fn transcribe(&mut self, file: &str, _language: Option<&str>) -> Result<String, String> {
            self.record(format!("transcribe {file}"), "transcribe")?;
            Ok(self.next("src"))
        }
        fn extract_existing(
            &mut self,
            file: &str,
            _language: Option<&str>,
        ) -> Result<String, String> {
            self.record(format!("extract {file}"), "extract")?;
            Ok(self.next("src"))
        }
        fn translate(
            &mut self,
            subtitle: &str,
            _from: Option<&str>,
            to: &str,
        ) -> Result<String, String> {
            self.record(format!("translate {subtitle} to {to}"), "translate")?;
            Ok(self.next(to))
        }
        fn mux(&mut self, file: &str, subtitle: &str, language: &str) -> Result<String, String> {
            self.record(format!("mux {subtitle} {language} {file}"), "mux")?;
            Ok(file.to_string())
        }
        fn verify(&mut self, file: &str, _language: &str) -> Result<(), String> {
            self.record(format!("verify {file}"), "verify")
        }
        fn cleanup(&mut self, subtitle: &str) -> Result<(), String> {
            self.record(format!("delete {subtitle}"), "cleanup")
        }
    }

    fn arabic_plan(delete_source: bool) -> Plan {
        let inventory = Inventory {
            subtitle: vec!["en".to_string()],
            audio: vec!["ja".to_string()],
        };
        let policy = Policy {
            delete_source_after_mux: delete_source,
            ..Policy::arabic()
        };
        plan("/lib/S01E01.mkv", &inventory, &policy)
    }

    #[test]
    fn a_full_plan_runs_in_order_and_cleans_up_last() {
        let plan = arabic_plan(true);
        let mut runner = Fake::default();
        let report = execute(&plan, &mut runner);

        assert!(report.ok);
        assert_eq!(
            runner.calls,
            vec![
                "search ar /lib/S01E01.mkv".to_string(),
                "extract /lib/S01E01.mkv".to_string(),
                "translate /tmp/src-1.srt to ar".to_string(),
                "mux /tmp/ar-2.srt ar /lib/S01E01.mkv".to_string(),
                "verify /lib/S01E01.mkv".to_string(),
                "delete /tmp/ar-2.srt".to_string(),
            ]
        );
    }

    #[test]
    fn a_human_subtitle_skips_translation_entirely() {
        let plan = arabic_plan(false);
        let mut runner = Fake::default().with_search_hit();
        let report = execute(&plan, &mut runner);

        assert!(report.ok);
        assert!(runner
            .calls
            .iter()
            .any(|call| call.starts_with("search ar")));
        assert!(
            !runner
                .calls
                .iter()
                .any(|call| call.starts_with("translate")),
            "a found subtitle must not be translated again: {:?}",
            runner.calls
        );
        assert!(runner
            .calls
            .iter()
            .any(|call| call.starts_with("mux /tmp/ar-1.srt")));
    }

    #[test]
    fn a_failed_verify_blocks_the_cleanup() {
        let plan = arabic_plan(true);
        let mut runner = Fake::default().fail("verify");
        let report = execute(&plan, &mut runner);

        assert!(!report.ok);
        assert_eq!(report.failures().count(), 1);
        assert!(
            !runner.calls.iter().any(|call| call.starts_with("delete")),
            "the source was deleted even though verify failed: {:?}",
            runner.calls
        );
        // The cleanup step is reported as skipped, so the log explains the missing deletion.
        let cleanup = report
            .results
            .iter()
            .find(|result| matches!(result.step, Step::CleanupSource { .. }))
            .unwrap();
        assert!(matches!(cleanup.outcome, Outcome::Skipped(_)));
    }

    #[test]
    fn a_failed_translation_skips_the_rest_of_that_language() {
        let plan = arabic_plan(true);
        let mut runner = Fake::default().fail("translate");
        let report = execute(&plan, &mut runner);

        assert!(!report.ok);
        assert!(!runner.calls.iter().any(|call| call.starts_with("mux")));
        assert!(!runner.calls.iter().any(|call| call.starts_with("delete")));
        assert!(
            report
                .results
                .iter()
                .filter(|r| matches!(r.outcome, Outcome::Skipped(_)))
                .count()
                >= 3
        );
    }

    #[test]
    fn a_search_outage_falls_back_to_translation() {
        let plan = arabic_plan(false);
        let mut runner = Fake::default().fail("search");
        let report = execute(&plan, &mut runner);

        assert!(report.ok, "a provider outage must not fail the job");
        assert!(runner
            .calls
            .iter()
            .any(|call| call.starts_with("translate")));
    }

    #[test]
    fn a_transcription_plan_starts_from_the_audio() {
        let inventory = Inventory {
            subtitle: vec![],
            audio: vec!["ja".to_string()],
        };
        let plan = plan("/lib/E02.mkv", &inventory, &Policy::arabic());
        let mut runner = Fake::default();
        let report = execute(&plan, &mut runner);
        assert!(report.ok);
        assert_eq!(runner.calls.first().unwrap(), "transcribe /lib/E02.mkv");
    }

    #[test]
    fn a_runner_without_a_linter_reports_a_skip_not_a_success() {
        let plan = Plan {
            file: "/tv/E01.mkv".to_string(),
            present: vec!["en".to_string()],
            missing: vec!["ar".to_string()],
            steps: vec![
                Step::SearchOnline {
                    language: "ar".to_string(),
                },
                Step::Lint {
                    language: "ar".to_string(),
                    profile: crate::RuleProfile::NetflixAdult,
                },
                Step::Mux {
                    language: "ar".to_string(),
                },
            ],
        };
        // `Fake` does not override `lint`, so the default trait method applies.
        let mut runner = Fake::default().with_search_hit();
        let report = execute(&plan, &mut runner);
        let lint = report
            .results
            .iter()
            .find(|result| matches!(result.step, Step::Lint { .. }))
            .expect("the plan has a lint step");
        assert!(
            matches!(&lint.outcome, Outcome::Skipped(reason) if reason.contains("cannot lint")),
            "{:?}",
            lint.outcome
        );
    }

    #[test]
    fn a_dry_run_records_everything_and_touches_nothing() {
        let plan = arabic_plan(true);
        let mut runner = DryRun::default();
        let report = execute(&plan, &mut runner);

        assert!(report.ok);
        // One result per step, even when one step needs two runner calls (extract, translate).
        assert_eq!(report.results.len(), plan.steps.len());
        assert!(runner.calls.len() >= plan.steps.len());
        assert!(runner.calls.iter().any(|call| call.starts_with("extract")));
        assert!(runner
            .calls
            .iter()
            .any(|call| call.starts_with("translate")));
        // Nothing is deleted or written inside the library during a dry run; the file path
        // only ever appears as the input of search/extract/mux/verify.
        assert!(!runner
            .calls
            .iter()
            .any(|call| call.starts_with("delete") && call.contains("/lib/")));
        assert!(runner.calls.iter().all(|call| [
            "search ",
            "extract ",
            "transcribe ",
            "translate ",
            "mux ",
            "verify ",
            "delete "
        ]
        .iter()
        .any(|prefix| call.starts_with(prefix))));
    }

    #[test]
    fn two_languages_keep_their_subtitles_apart() {
        let inventory = Inventory {
            subtitle: vec!["en".to_string()],
            audio: vec!["ja".to_string()],
        };
        let policy = Policy {
            target_languages: vec!["ar".into(), "fr".into()],
            prefer_human_subtitle: false,
            delete_source_after_mux: true,
            ..Policy::default()
        };
        let plan = plan("/lib/E03.mkv", &inventory, &policy);
        let mut runner = Fake::default();
        let report = execute(&plan, &mut runner);

        assert!(report.ok);
        let deletes: Vec<&String> = runner
            .calls
            .iter()
            .filter(|call| call.starts_with("delete"))
            .collect();
        assert_eq!(deletes.len(), 2);
        assert_ne!(
            deletes[0], deletes[1],
            "the second language deleted the first one's file"
        );
    }
}
