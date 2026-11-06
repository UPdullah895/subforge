//! What a library file still needs, and the order to do it in.
//!
//! This crate is the *planning* half of the P2 pipeline: it reads a file's tracks, compares
//! them with a policy, and produces the steps a worker has to run (search for a human
//! subtitle, transcribe, translate, mux, verify, clean up). It performs no work itself, so
//! the whole decision can be tested without a media file, a GPU or a network — and the same
//! plan can be shown to a user as a dry run before anything is written.
//!
//! The executing half (a worker that runs the steps through `subforge-core`, plus the
//! watcher that decides *when* to plan) is deliberately not here yet: it needs the
//! scheduler's gate so a season is processed one heavy step at a time.

use serde::{Deserialize, Serialize};
use subforge_core::tools::{mkvtoolnix, ToolPaths};

pub mod exec;
pub mod language;
pub mod runner;

pub use language::normalize_language;

/// One job's worth of decisions for one file, from the policy file a library owner writes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Policy {
    /// Languages the library should end up with, e.g. `["ar"]`. Empty means "do nothing".
    #[serde(default)]
    pub target_languages: Vec<String>,
    /// Look for an existing human subtitle (Subsource/OpenSubtitles) before machine translation.
    #[serde(default = "default_true")]
    pub prefer_human_subtitle: bool,
    /// Translation engine name understood by `subforge-core::translate`; `None` lets the
    /// caller pick its configured default.
    #[serde(default)]
    pub translate_engine: Option<String>,
    /// Remove the external subtitle file once the mux is written **and verified**.
    #[serde(default)]
    pub delete_source_after_mux: bool,
    /// Plan only: report what would happen, change nothing. On by default for a new library.
    #[serde(default = "default_true")]
    pub dry_run: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            target_languages: Vec::new(),
            prefer_human_subtitle: true,
            translate_engine: None,
            delete_source_after_mux: false,
            dry_run: true,
        }
    }
}

impl Policy {
    /// Sane defaults for the common case: Arabic, search before translating, keep the source.
    pub fn arabic() -> Self {
        Self {
            target_languages: vec!["ar".to_string()],
            ..Self::default()
        }
    }
}

/// The languages a file already carries.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Inventory {
    /// Subtitle track languages, normalised (`ar`, `en`, …). Order preserved, duplicates kept.
    pub subtitle: Vec<String>,
    /// Audio track languages, normalised.
    pub audio: Vec<String>,
}

impl Inventory {
    /// Language codes missing from the subtitle tracks, in the policy's order.
    pub fn missing(&self, targets: &[String]) -> Vec<String> {
        targets
            .iter()
            .map(|target| normalize_language(target))
            .filter(|target| !target.is_empty())
            .filter(|target| !self.subtitle.iter().any(|have| have == target))
            .collect()
    }

    /// True when the file has no subtitle track in any language.
    pub fn has_no_subtitles(&self) -> bool {
        self.subtitle.is_empty()
    }

    /// The language the dialogue is most likely in: the first audio track that is not `und`.
    pub fn source_language(&self) -> Option<&str> {
        self.audio
            .iter()
            .map(String::as_str)
            .find(|code| !code.is_empty() && *code != "und")
    }
}

/// One thing the worker has to do. Every step is idempotent or safe to re-run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum Step {
    /// Ask the subtitle providers for an existing human subtitle in this language.
    SearchOnline { language: String },
    /// Transcribe the audio (needed when the file has no text subtitle at all).
    Transcribe { language: Option<String> },
    /// Machine-translate an existing subtitle into this language.
    ///
    /// `only_if_no_subtitle` is true when the step is the *fallback* to an online search:
    /// the executor skips it if the search already produced a human subtitle.
    Translate {
        from: Option<String>,
        to: String,
        #[serde(default)]
        only_if_no_subtitle: bool,
    },
    /// Mux the produced subtitle into the video with its language and flags.
    Mux { language: String },
    /// Re-read the file: the track must be there, complete and in sync.
    Verify { language: String },
    /// Delete the external subtitle the mux came from (only after a successful verify).
    CleanupSource { language: String },
}

impl Step {
    /// Steps that saturate the GPU/CPU and must not overlap with another heavy job.
    pub fn is_heavy(&self) -> bool {
        matches!(self, Step::Transcribe { .. } | Step::Translate { .. })
    }
}

/// What to do for one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub file: String,
    /// Languages the file already had (normalised).
    pub present: Vec<String>,
    /// Languages the policy asked for that are missing.
    pub missing: Vec<String>,
    pub steps: Vec<Step>,
}

impl Plan {
    /// Nothing to do: the file already satisfies the policy.
    pub fn is_noop(&self) -> bool {
        self.steps.is_empty()
    }

    /// The steps that need the scheduler's heavy slot, in order.
    pub fn heavy_steps(&self) -> impl Iterator<Item = &Step> {
        self.steps.iter().filter(|step| step.is_heavy())
    }
}

/// Builds the plan for one file from its inventory and a policy.
///
/// The order inside a language is: get text (search, or transcribe, or translate), mux it,
/// verify it, and only then clean up. Languages are processed one after another, so a
/// two-language policy never doubles the peak work.
pub fn plan(file: &str, inventory: &Inventory, policy: &Policy) -> Plan {
    let missing = inventory.missing(&policy.target_languages);
    let mut steps = Vec::new();

    for language in &missing {
        let needs_text = !inventory.has_no_subtitles();

        if needs_text {
            if policy.prefer_human_subtitle {
                steps.push(Step::SearchOnline {
                    language: language.clone(),
                });
            }
            steps.push(Step::Translate {
                from: inventory.source_language().map(str::to_string),
                to: language.clone(),
                only_if_no_subtitle: policy.prefer_human_subtitle,
            });
        } else {
            // No subtitle track at all: the text has to be produced from the audio first, and
            // only translated when the dialogue is not already in the language we need.
            let source = inventory
                .source_language()
                .map(normalize_language)
                .unwrap_or_default();
            steps.push(Step::Transcribe {
                language: inventory.source_language().map(str::to_string),
            });
            if source != *language {
                steps.push(Step::Translate {
                    from: inventory.source_language().map(str::to_string),
                    to: language.clone(),
                    only_if_no_subtitle: false,
                });
            }
        }

        steps.push(Step::Mux {
            language: language.clone(),
        });
        steps.push(Step::Verify {
            language: language.clone(),
        });
        if policy.delete_source_after_mux {
            steps.push(Step::CleanupSource {
                language: language.clone(),
            });
        }
    }

    Plan {
        file: file.to_string(),
        present: inventory.subtitle.clone(),
        missing,
        steps,
    }
}

/// Reads a file's languages with mkvmerge. Cheap enough for a dry run over a whole season.
pub fn read_inventory(paths: &ToolPaths, file: &str) -> Result<Inventory, String> {
    let info = mkvtoolnix::enumerate_tracks(paths, file).map_err(|e| e.to_string())?;
    let mut inventory = Inventory::default();
    for track in info.tracks {
        let code = track
            .properties
            .as_ref()
            .and_then(|props| props.language.clone())
            .map(|language| normalize_language(&language))
            .unwrap_or_default();
        match track.track_type.as_str() {
            "subtitles" => inventory.subtitle.push(code),
            "audio" => inventory.audio.push(code),
            _ => {}
        }
    }
    Ok(inventory)
}

/// [`plan`] for a real file: reads the tracks first.
pub fn plan_file(paths: &ToolPaths, file: &str, policy: &Policy) -> Result<Plan, String> {
    let inventory = read_inventory(paths, file)?;
    Ok(plan(file, &inventory, policy))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inventory(subtitle: &[&str], audio: &[&str]) -> Inventory {
        Inventory {
            subtitle: subtitle.iter().map(|s| s.to_string()).collect(),
            audio: audio.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn a_file_that_already_has_the_language_needs_nothing() {
        let files = inventory(&["ar", "en"], &["ja"]);
        let plan = plan("/lib/S01E01.mkv", &files, &Policy::arabic());
        assert!(plan.is_noop());
        assert!(plan.missing.is_empty());
        assert!(plan.present.contains(&"ar".to_string()));
    }

    #[test]
    fn missing_arabic_with_english_present_translates_then_muxes_then_verifies() {
        let files = inventory(&["en"], &["ja"]);
        let policy = Policy {
            delete_source_after_mux: true,
            dry_run: true,
            ..Policy::arabic()
        };
        let plan = plan("/lib/S01E01.mkv", &files, &policy);

        assert_eq!(plan.missing, vec!["ar".to_string()]);
        assert_eq!(
            plan.steps,
            vec![
                Step::SearchOnline {
                    language: "ar".into()
                },
                Step::Translate {
                    from: Some("ja".into()),
                    to: "ar".into(),
                    only_if_no_subtitle: true,
                },
                Step::Mux {
                    language: "ar".into()
                },
                Step::Verify {
                    language: "ar".into()
                },
                Step::CleanupSource {
                    language: "ar".into()
                },
            ]
        );
    }

    #[test]
    fn a_file_without_any_subtitle_is_transcribed_first() {
        let files = inventory(&[], &["ja"]);
        let plan = plan("/lib/S01E01.mkv", &files, &Policy::arabic());
        assert_eq!(
            plan.steps.first(),
            Some(&Step::Transcribe {
                language: Some("ja".into())
            })
        );
        assert!(plan.steps.contains(&Step::Translate {
            from: Some("ja".into()),
            to: "ar".into(),
            only_if_no_subtitle: false,
        }));
    }

    #[test]
    fn source_language_is_already_arabic_so_no_translation_step() {
        let files = inventory(&[], &["ar"]);
        let plan = plan("/lib/S01E01.mkv", &files, &Policy::arabic());
        assert_eq!(
            plan.steps.first(),
            Some(&Step::Transcribe {
                language: Some("ar".into())
            })
        );
        assert!(!plan
            .steps
            .iter()
            .any(|step| matches!(step, Step::Translate { .. })));
        assert!(plan.steps.contains(&Step::Mux {
            language: "ar".into()
        }));
    }

    #[test]
    fn without_human_search_the_plan_goes_straight_to_translation() {
        let files = inventory(&["en"], &["ja"]);
        let policy = Policy {
            prefer_human_subtitle: false,
            ..Policy::arabic()
        };
        let plan = plan("/lib/S01E01.mkv", &files, &policy);
        assert!(!plan
            .steps
            .iter()
            .any(|step| matches!(step, Step::SearchOnline { .. })));
        assert_eq!(
            plan.steps.first(),
            Some(&Step::Translate {
                from: Some("ja".into()),
                to: "ar".into(),
                // No human search in this policy, so the translation is unconditional.
                only_if_no_subtitle: false,
            })
        );
    }

    #[test]
    fn two_languages_are_planned_one_after_the_other() {
        let files = inventory(&["en"], &["ja"]);
        let policy = Policy {
            target_languages: vec!["ar".into(), "fr".into()],
            prefer_human_subtitle: false,
            ..Policy::default()
        };
        let plan = plan("/lib/S01E01.mkv", &files, &policy);
        assert_eq!(plan.missing, vec!["ar".to_string(), "fr".to_string()]);
        let muxes: Vec<&Step> = plan
            .steps
            .iter()
            .filter(|step| matches!(step, Step::Mux { .. }))
            .collect();
        assert_eq!(muxes.len(), 2);
        // Arabic's whole block comes before French's.
        let first_fr = plan
            .steps
            .iter()
            .position(|step| matches!(step, Step::Translate { to, .. } if to == "fr"))
            .unwrap();
        let first_ar_verify = plan
            .steps
            .iter()
            .position(|step| matches!(step, Step::Verify { language } if language == "ar"))
            .unwrap();
        assert!(first_ar_verify < first_fr);
    }

    #[test]
    fn cleanup_only_appears_when_the_policy_asks_for_it() {
        let files = inventory(&["en"], &["ja"]);
        let plan = plan("/lib/S01E01.mkv", &files, &Policy::arabic());
        assert!(!plan
            .steps
            .iter()
            .any(|step| matches!(step, Step::CleanupSource { .. })));
    }

    #[test]
    fn heavy_steps_are_the_gpu_ones() {
        let files = inventory(&["en"], &["ja"]);
        let policy = Policy {
            delete_source_after_mux: true,
            ..Policy::arabic()
        };
        let plan = plan("/lib/S01E01.mkv", &files, &policy);
        let heavy: Vec<&Step> = plan.heavy_steps().collect();
        assert_eq!(heavy.len(), 1);
        assert!(matches!(heavy[0], Step::Translate { .. }));
    }

    #[test]
    fn an_empty_policy_does_nothing() {
        let files = inventory(&["en"], &["ja"]);
        assert!(plan("/lib/S01E01.mkv", &files, &Policy::default()).is_noop());
    }

    #[test]
    fn a_policy_file_round_trips_through_json() {
        let policy = Policy {
            target_languages: vec!["ar".into(), "en".into()],
            delete_source_after_mux: true,
            dry_run: false,
            ..Policy::default()
        };
        let text = serde_json::to_string(&policy).unwrap();
        let parsed: Policy = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed, policy);
        // Absent fields fall back to the safe defaults.
        let minimal: Policy = serde_json::from_str(r#"{"target_languages":["ar"]}"#).unwrap();
        assert!(minimal.dry_run);
        assert!(minimal.prefer_human_subtitle);
        assert!(!minimal.delete_source_after_mux);
    }

    #[test]
    fn the_plan_is_serialisable_for_a_dry_run_screen() {
        let files = inventory(&["en"], &["ja"]);
        let text =
            serde_json::to_string(&plan("/lib/S01E01.mkv", &files, &Policy::arabic())).unwrap();
        assert!(text.contains("\"step\":\"mux\""));
    }
}
