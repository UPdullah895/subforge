//! Subtitle readability rules, and the fixes that are safe to apply automatically.
//!
//! The numbers come from the published style guides, not from taste. Netflix: 42 characters per
//! line, at most two lines, 20 characters per second for adult content and 17 for children,
//! 5/6 s minimum and 7 s maximum duration, gaps of two frames. BBC: 160–180 words per minute,
//! ~37 characters per line, gaps of at least one second.
//! (<https://partnerhelp.netflixstudios.com/hc/en-us/articles/217350977>,
//!  <https://www.bbc.co.uk/accessibility/forproducts/guides/subtitles/>)
//!
//! Only SRT is rewritten: it is what the pipeline produces between steps. An ASS or VTT file is
//! reported as skipped rather than parsed badly, because a linter that mangles styling is worse
//! than no linter.

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Which rule-set to apply. `Off` is the default: a linter that rewrites text must be asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuleProfile {
    #[default]
    Off,
    NetflixAdult,
    NetflixKids,
    Bbc,
}

/// The resolved numbers for a profile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rules {
    pub name: &'static str,
    pub max_chars_per_line: usize,
    pub max_lines: usize,
    /// Characters per second (Netflix) — the BBC profile expresses the same idea as words per
    /// minute, converted here on the usual five-characters-per-word basis.
    pub max_cps: f32,
    pub min_duration_ms: u64,
    pub max_duration_ms: u64,
    pub min_gap_ms: u64,
}

impl Rules {
    pub fn for_profile(profile: RuleProfile) -> Option<Self> {
        match profile {
            RuleProfile::Off => None,
            RuleProfile::NetflixAdult => Some(Self {
                name: "netflix-adult",
                max_chars_per_line: 42,
                max_lines: 2,
                max_cps: 20.0,
                // 5/6 s at 24 fps (20 frames), 7 s maximum.
                min_duration_ms: 833,
                max_duration_ms: 7_000,
                // "gaps are either 2 frames or half a second or more"; 2 frames at 24 fps.
                min_gap_ms: 83,
            }),
            RuleProfile::NetflixKids => Some(Self {
                name: "netflix-kids",
                max_chars_per_line: 42,
                max_lines: 2,
                max_cps: 17.0,
                min_duration_ms: 833,
                max_duration_ms: 7_000,
                min_gap_ms: 83,
            }),
            RuleProfile::Bbc => Some(Self {
                name: "bbc",
                max_chars_per_line: 37,
                max_lines: 2,
                // 160–180 wpm at five characters per word ≈ 15 cps at the conservative end.
                max_cps: 15.0,
                min_duration_ms: 1_200,
                max_duration_ms: 7_000,
                min_gap_ms: 1_000,
            }),
        }
    }
}

/// One subtitle cue, enough to lint and to write back unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cue {
    pub index: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub lines: Vec<String>,
}

impl Cue {
    pub fn duration_ms(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }

    /// Characters the reader has to take in: newlines are not read, everything else counts.
    pub fn characters(&self) -> usize {
        self.lines.iter().map(|line| line.chars().count()).sum()
    }

    pub fn cps(&self) -> f32 {
        let duration = self.duration_ms() as f32 / 1000.0;
        if duration <= 0.0 {
            f32::INFINITY
        } else {
            self.characters() as f32 / duration
        }
    }
}

/// Something a reader would notice. `Fix` says whether [`fix`] knows how to repair it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    LineTooLong { cue: u32, chars: usize, max: usize },
    TooManyLines { cue: u32, lines: usize, max: usize },
    TooFast { cue: u32, cps: u32, max: u32 },
    TooShort { cue: u32, ms: u64, min: u64 },
    TooLong { cue: u32, ms: u64, max: u64 },
    GapTooSmall { after: u32, gap_ms: u64, min: u64 },
    Overlap { after: u32, ms: u64 },
    EmptyCue { cue: u32 },
}

impl Violation {
    pub fn cue(&self) -> u32 {
        match self {
            Violation::LineTooLong { cue, .. }
            | Violation::TooManyLines { cue, .. }
            | Violation::TooFast { cue, .. }
            | Violation::TooShort { cue, .. }
            | Violation::TooLong { cue, .. }
            | Violation::EmptyCue { cue } => *cue,
            Violation::GapTooSmall { after, .. } | Violation::Overlap { after, .. } => *after,
        }
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Violation::LineTooLong { cue, chars, max } => {
                write!(f, "cue {cue}: line of {chars} characters (max {max})")
            }
            Violation::TooManyLines { cue, lines, max } => {
                write!(f, "cue {cue}: {lines} lines (max {max})")
            }
            Violation::TooFast { cue, cps, max } => {
                write!(f, "cue {cue}: {cps} characters/second (max {max})")
            }
            Violation::TooShort { cue, ms, min } => {
                write!(f, "cue {cue}: {ms} ms on screen (min {min})")
            }
            Violation::TooLong { cue, ms, max } => {
                write!(f, "cue {cue}: {ms} ms on screen (max {max})")
            }
            Violation::GapTooSmall { after, gap_ms, min } => {
                write!(f, "cue {after}: {gap_ms} ms gap (min {min})")
            }
            Violation::Overlap { after, ms } => write!(f, "cue {after}: overlaps by {ms} ms"),
            Violation::EmptyCue { cue } => write!(f, "cue {cue}: no text"),
        }
    }
}

/// What [`fix`] changed, for the report the user sees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LintReport {
    pub profile: String,
    /// Violations found before any fixing.
    pub found: Vec<String>,
    /// Cues whose lines were re-split to fit the width.
    pub rewrapped: usize,
    /// Cues split into two because the text could not fit in two lines.
    pub split: usize,
    /// Cues extended, shortened or nudged to satisfy duration and gap rules.
    pub retimed: usize,
    /// True when the file was rewritten.
    pub written: bool,
}

impl LintReport {
    pub fn clean(&self) -> bool {
        self.found.is_empty()
    }
}

// ── parsing ──────────────────────────────────────────────────────────────────────────────

fn parse_timestamp(text: &str) -> Option<u64> {
    // 00:01:02,500 or 00:01:02.500
    let text = text.trim();
    let (clock, millis) = text.split_once([',', '.'])?;
    let mut parts = clock.split(':');
    let hours: u64 = parts.next()?.trim().parse().ok()?;
    let minutes: u64 = parts.next()?.trim().parse().ok()?;
    let seconds: u64 = parts.next()?.trim().parse().ok()?;
    let millis: u64 = millis.trim().parse().ok()?;
    Some(((hours * 60 + minutes) * 60 + seconds) * 1000 + millis)
}

fn format_timestamp(ms: u64) -> String {
    let hours = ms / 3_600_000;
    let minutes = (ms % 3_600_000) / 60_000;
    let seconds = (ms % 60_000) / 1000;
    let millis = ms % 1000;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

/// Parses SRT text. Tolerant on purpose: BOM, CRLF, blank lines inside a cue, a missing blank
/// line between cues, and non-sequential indices all appear in real downloads.
pub fn parse_srt(text: &str) -> Vec<Cue> {
    let text = text
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut cues = Vec::new();
    let mut lines = text.split('\n').peekable();

    while let Some(line) = lines.next() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // An index, then a timing line. If the timing line is not where it should be, skip this
        // block instead of inventing timings.
        let Some(timing) = lines.next() else { break };
        let timing = timing.trim();
        let Some((start, end)) = timing.split_once("-->") else {
            continue;
        };
        let (Some(start_ms), Some(end_ms)) = (parse_timestamp(start), parse_timestamp(end)) else {
            continue;
        };

        let mut body = Vec::new();
        while let Some(next) = lines.peek() {
            if next.trim().is_empty() {
                break;
            }
            body.push(lines.next().unwrap().trim_end().to_string());
        }

        let index = line.parse::<u32>().unwrap_or((cues.len() + 1) as u32);
        cues.push(Cue {
            index,
            start_ms,
            end_ms: end_ms.max(start_ms),
            lines: body,
        });
    }
    cues
}

pub fn render_srt(cues: &[Cue]) -> String {
    let mut out = String::new();
    for (position, cue) in cues.iter().enumerate() {
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            position + 1,
            format_timestamp(cue.start_ms),
            format_timestamp(cue.end_ms),
            cue.lines.join("\n")
        ));
    }
    out
}

// ── linting and fixing ──────────────────────────────────────────────────────────────────

pub fn violations(cues: &[Cue], rules: &Rules) -> Vec<Violation> {
    let mut found = Vec::new();
    for (position, cue) in cues.iter().enumerate() {
        if cue.lines.iter().all(|line| line.trim().is_empty()) {
            found.push(Violation::EmptyCue { cue: cue.index });
            continue;
        }
        for line in &cue.lines {
            let chars = line.chars().count();
            if chars > rules.max_chars_per_line {
                found.push(Violation::LineTooLong {
                    cue: cue.index,
                    chars,
                    max: rules.max_chars_per_line,
                });
            }
        }
        if cue.lines.len() > rules.max_lines {
            found.push(Violation::TooManyLines {
                cue: cue.index,
                lines: cue.lines.len(),
                max: rules.max_lines,
            });
        }
        if cue.duration_ms() < rules.min_duration_ms {
            found.push(Violation::TooShort {
                cue: cue.index,
                ms: cue.duration_ms(),
                min: rules.min_duration_ms,
            });
        }
        if cue.duration_ms() > rules.max_duration_ms {
            found.push(Violation::TooLong {
                cue: cue.index,
                ms: cue.duration_ms(),
                max: rules.max_duration_ms,
            });
        }
        let cps = cue.cps();
        if cps > rules.max_cps {
            found.push(Violation::TooFast {
                cue: cue.index,
                cps: cps.round() as u32,
                max: rules.max_cps as u32,
            });
        }
        if let Some(next) = cues.get(position + 1) {
            if next.start_ms < cue.end_ms {
                found.push(Violation::Overlap {
                    after: cue.index,
                    ms: cue.end_ms - next.start_ms,
                });
            } else if next.start_ms - cue.end_ms < rules.min_gap_ms {
                found.push(Violation::GapTooSmall {
                    after: cue.index,
                    gap_ms: next.start_ms - cue.end_ms,
                    min: rules.min_gap_ms,
                });
            }
        }
    }
    found
}

/// Splits a line into at most `max` characters per line without breaking words. Long words are
/// left alone: breaking a word is worse than a wide line.
fn wrap(text: &str, max: usize) -> Vec<String> {
    if text.chars().count() <= max {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let prospective = if current.is_empty() {
            word.chars().count()
        } else {
            current.chars().count() + 1 + word.chars().count()
        };
        if prospective > max && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(text.to_string());
    }
    lines
}

/// Applies the fixes that cannot change meaning: re-wrapping text, splitting an over-long cue in
/// two, extending or trimming durations, and closing gaps that are too small.
pub fn fix(cues: &mut Vec<Cue>, rules: &Rules) -> (usize, usize, usize) {
    let mut rewrapped = 0;
    let mut split = 0;
    let mut retimed = 0;

    // 1. Re-wrap: join the lines, wrap to the width, and split the cue if it still needs more
    //    lines than the profile allows.
    let mut stage: Vec<Cue> = Vec::with_capacity(cues.len());
    for cue in cues.iter() {
        let joined = cue.lines.join(" ");
        let wrapped = wrap(&joined, rules.max_chars_per_line);
        if wrapped != cue.lines {
            rewrapped += 1;
        }
        if wrapped.len() <= rules.max_lines {
            let mut cue = cue.clone();
            cue.lines = wrapped;
            stage.push(cue);
            continue;
        }
        // Two cues, each within the profile, with the duration shared by character count so the
        // reading speed stays even.
        let total: usize = wrapped.iter().map(|line| line.chars().count()).sum();
        let first_lines: Vec<String> = wrapped[..rules.max_lines].to_vec();
        let rest_lines: Vec<String> = wrapped[rules.max_lines..].to_vec();
        let first_chars: usize = first_lines.iter().map(|line| line.chars().count()).sum();
        let ratio = if total == 0 {
            0.5
        } else {
            first_chars as f64 / total as f64
        };
        let boundary = cue.start_ms + (cue.duration_ms() as f64 * ratio).round() as u64;

        let mut first = cue.clone();
        first.lines = first_lines;
        first.end_ms = boundary.min(cue.end_ms);
        let mut second = cue.clone();
        second.index = cue.index;
        second.lines = rest_lines;
        second.start_ms = boundary.min(cue.end_ms);
        split += 1;
        stage.push(first);
        stage.push(second);
    }

    // 2. Timing: extend short cues, trim long ones, and close small gaps — never into the next
    //    cue, and never past the profile's maximum duration.
    let count = stage.len();
    for position in 0..count {
        let next_start = stage.get(position + 1).map(|cue| cue.start_ms);
        let cue = &mut stage[position];
        let mut changed = false;

        if cue.duration_ms() < rules.min_duration_ms {
            let target = cue.start_ms + rules.min_duration_ms;
            let ceiling = next_start
                .map(|start| start.saturating_sub(rules.min_gap_ms))
                .unwrap_or(u64::MAX);
            let end = target.min(ceiling);
            if end > cue.end_ms {
                cue.end_ms = end;
                changed = true;
            }
        }
        if cue.duration_ms() > rules.max_duration_ms {
            cue.end_ms = cue.start_ms + rules.max_duration_ms;
            changed = true;
        }
        if let Some(start) = next_start {
            let gap = start.saturating_sub(cue.end_ms);
            // A gap shorter than the profile's minimum is a flash, not a pause: hold the line
            // until the next one starts rather than shaving time off it.
            if start > cue.end_ms
                && gap < rules.min_gap_ms
                && start - cue.start_ms <= rules.max_duration_ms
            {
                cue.end_ms = start;
                changed = true;
            }
        }
        if changed {
            retimed += 1;
        }
    }

    *cues = stage;
    (rewrapped, split, retimed)
}

fn parse_and_report(path: &Path, rules: &Rules) -> Result<(Vec<Cue>, LintReport), String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut cues = parse_srt(&text);
    if cues.is_empty() {
        return Err(format!(
            "{}: no SRT cues found (only SRT can be linted)",
            path.display()
        ));
    }
    let found = violations(&cues, rules);
    let (rewrapped, split, retimed) = fix(&mut cues, rules);
    Ok((
        cues,
        LintReport {
            profile: rules.name.to_string(),
            found: found.iter().map(Violation::to_string).collect(),
            rewrapped,
            split,
            retimed,
            written: false,
        },
    ))
}

/// Lints a subtitle file and writes it back when anything changed. Returns the report; the file is
/// only touched when a fix was applied, so a clean file keeps its exact bytes.
pub fn lint_file(path: &Path, profile: RuleProfile) -> Result<LintReport, String> {
    let Some(rules) = Rules::for_profile(profile) else {
        return Ok(LintReport {
            profile: "off".to_string(),
            found: Vec::new(),
            rewrapped: 0,
            split: 0,
            retimed: 0,
            written: false,
        });
    };
    let (cues, mut report) = parse_and_report(path, &rules)?;
    if report.rewrapped + report.split + report.retimed > 0 {
        std::fs::write(path, render_srt(&cues))
            .map_err(|error| format!("{}: {error}", path.display()))?;
        report.written = true;
        // Re-lint the result so the report says what a reader would still notice.
        let remaining = violations(&cues, &rules);
        report.found = remaining.iter().map(Violation::to_string).collect();
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(profile: RuleProfile) -> Rules {
        Rules::for_profile(profile).unwrap()
    }

    fn cue(index: u32, start: u64, end: u64, lines: &[&str]) -> Cue {
        Cue {
            index,
            start_ms: start,
            end_ms: end,
            lines: lines.iter().map(|line| line.to_string()).collect(),
        }
    }

    #[test]
    fn off_means_off() {
        assert!(Rules::for_profile(RuleProfile::Off).is_none());
    }

    #[test]
    fn the_profiles_carry_the_published_numbers() {
        let netflix = rules(RuleProfile::NetflixAdult);
        assert_eq!(netflix.max_chars_per_line, 42);
        assert_eq!(netflix.max_cps, 20.0);
        assert_eq!(netflix.min_duration_ms, 833);
        assert_eq!(rules(RuleProfile::NetflixKids).max_cps, 17.0);
        let bbc = rules(RuleProfile::Bbc);
        assert_eq!(bbc.max_chars_per_line, 37);
        assert_eq!(bbc.min_gap_ms, 1_000);
    }

    #[test]
    fn srt_parses_and_round_trips() {
        let text = "\u{feff}1\r\n00:00:01,000 --> 00:00:03,500\r\nHello there\r\n\r\n2\r\n00:00:04,000 --> 00:00:06,000\r\nSecond line\r\nwrapped\r\n\r\n";
        let cues = parse_srt(text);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start_ms, 1_000);
        assert_eq!(cues[0].end_ms, 3_500);
        assert_eq!(cues[0].lines, vec!["Hello there"]);
        assert_eq!(cues[1].lines, vec!["Second line", "wrapped"]);

        let rendered = render_srt(&cues);
        let again = parse_srt(&rendered);
        assert_eq!(again, cues);
    }

    #[test]
    fn a_broken_block_is_skipped_not_invented() {
        let text = "1\nnot a timestamp\nignored\n\n2\n00:00:01,000 --> 00:00:02,000\nreal\n";
        let cues = parse_srt(text);
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].lines, vec!["real"]);
    }

    #[test]
    fn cps_counts_every_character_but_the_newline() {
        let cue = cue(1, 0, 2_000, &["0123456789", "0123456789"]);
        assert_eq!(cue.characters(), 20);
        assert!((cue.cps() - 10.0).abs() < 0.01);
    }

    #[test]
    fn each_rule_fires_on_the_case_it_is_meant_for() {
        let rules = rules(RuleProfile::NetflixAdult);
        let found = violations(
            &[
                cue(1, 0, 1_000, &["x".repeat(50).as_str()]), // too long + too fast
                cue(2, 1_000, 6_000, &["a", "b", "c"]),       // three lines
                cue(3, 6_050, 6_100, &["short"]),             // 50 ms + 50 ms gap
                cue(4, 6_200, 14_000, &["long"]),             // 7.8 s
                cue(5, 14_000, 15_000, &["   "]),             // empty
            ],
            &rules,
        );
        let text: Vec<String> = found.iter().map(Violation::to_string).collect();
        assert!(
            text.iter()
                .any(|line| line.contains("line of 50 characters")),
            "{text:?}"
        );
        assert!(text.iter().any(|line| line.contains("3 lines")), "{text:?}");
        assert!(
            text.iter().any(|line| line.contains("ms on screen (min")),
            "{text:?}"
        );
        assert!(
            text.iter().any(|line| line.contains("ms on screen (max")),
            "{text:?}"
        );
        assert!(text.iter().any(|line| line.contains("no text")), "{text:?}");
        assert!(text.iter().any(|line| line.contains("gap")), "{text:?}");
    }

    #[test]
    fn wrapping_never_breaks_a_word() {
        let lines = wrap("the quick brown fox jumps over the lazy dog", 20);
        assert!(
            lines.iter().all(|line| line.chars().count() <= 20),
            "{lines:?}"
        );
        assert_eq!(
            lines.join(" "),
            "the quick brown fox jumps over the lazy dog"
        );
        // A single long word is left intact rather than chopped.
        assert_eq!(
            wrap("supercalifragilisticexpialidocious", 10),
            vec!["supercalifragilisticexpialidocious"]
        );
    }

    #[test]
    fn a_long_line_is_rewrapped_and_a_longer_one_is_split() {
        let rules = rules(RuleProfile::NetflixAdult);
        let mut cues = vec![cue(
            1,
            0,
            4_000,
            &["This is a fairly long line of dialogue that fits on two rows nicely."],
        )];
        let (rewrapped, split, _) = fix(&mut cues, &rules);
        assert_eq!(rewrapped, 1);
        assert_eq!(split, 0);
        assert!(cues[0].lines.len() <= 2, "{:?}", cues[0].lines);

        // 120 characters cannot become two 42-character lines: it must become two cues.
        let long = "x".repeat(40) + " " + &"y".repeat(40) + " " + &"z".repeat(38);
        let mut cues = vec![cue(1, 0, 6_000, &[long.as_str()])];
        let (_, split, _) = fix(&mut cues, &rules);
        assert_eq!(split, 1);
        assert_eq!(cues.len(), 2);
        // The two halves are contiguous and inside the original window.
        assert_eq!(cues[0].start_ms, 0);
        assert_eq!(cues[0].end_ms, cues[1].start_ms);
        assert_eq!(cues[1].end_ms, 6_000);
    }

    #[test]
    fn a_short_cue_is_extended_but_never_into_the_next_one() {
        let rules = rules(RuleProfile::NetflixAdult);
        let mut cues = vec![cue(1, 0, 300, &["Hi"]), cue(2, 900, 3_000, &["There"])];
        let (_, _, retimed) = fix(&mut cues, &rules);
        assert_eq!(retimed, 1);
        assert_eq!(cues[0].end_ms, 900 - rules.min_gap_ms);
    }

    #[test]
    fn a_small_gap_is_closed_and_a_big_one_is_left_alone() {
        let rules = rules(RuleProfile::NetflixAdult);
        let mut cues = vec![cue(1, 0, 2_000, &["One"]), cue(2, 2_050, 4_000, &["Two"])];
        fix(&mut cues, &rules);
        // A 50 ms gap is a flash: the line is held until the next one starts (gap 0), which is
        // what the style guides ask for and never costs reading time.
        assert_eq!(cues[0].end_ms, 2_050);

        let mut cues = vec![cue(1, 0, 2_000, &["One"]), cue(2, 4_000, 6_000, &["Two"])];
        let before = cues.clone();
        fix(&mut cues, &rules);
        assert_eq!(cues, before);
    }

    #[test]
    fn a_long_cue_is_trimmed_to_the_maximum() {
        let rules = rules(RuleProfile::NetflixAdult);
        let mut cues = vec![cue(1, 0, 9_000, &["This sits for nine seconds."])];
        fix(&mut cues, &rules);
        assert_eq!(cues[0].duration_ms(), rules.max_duration_ms);
    }

    #[test]
    fn linting_a_clean_file_leaves_its_bytes_untouched() {
        let dir = std::env::temp_dir().join(format!("sf_lint_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("clean.srt");
        let text = "1\n00:00:01,000 --> 00:00:03,000\nA short line\n\n";
        std::fs::write(&path, text).unwrap();

        let report = lint_file(&path, RuleProfile::NetflixAdult).unwrap();
        assert!(report.clean(), "{:?}", report.found);
        assert!(!report.written);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn linting_a_messy_file_rewrites_it_and_reports_what_is_left() {
        let dir = std::env::temp_dir().join(format!("sf_lint2_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("messy.srt");
        let text = format!(
            "1\n00:00:01,000 --> 00:00:01,200\n{}\n\n",
            "This line is far too long for any subtitle profile and must be re-wrapped."
        );
        std::fs::write(&path, &text).unwrap();

        let report = lint_file(&path, RuleProfile::NetflixAdult).unwrap();
        assert!(report.written, "the file should have been rewritten");
        assert!(report.rewrapped >= 1 || report.split >= 1);

        let after = std::fs::read_to_string(&path).unwrap();
        let cues = parse_srt(&after);
        assert!(!cues.is_empty());
        assert!(cues.iter().all(|cue| cue.lines.len() <= 2), "{after}");
        assert!(
            cues.iter()
                .all(|cue| cue.lines.iter().all(|line| line.chars().count() <= 42)),
            "{after}"
        );
        // A 200 ms cue at 74 characters is still too fast — the report must say so rather than
        // pretend the fix was complete.
        assert!(
            report
                .found
                .iter()
                .any(|line| line.contains("characters/second")),
            "{:?}",
            report.found
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_non_srt_file_is_an_error_not_a_rewrite() {
        let dir = std::env::temp_dir().join(format!("sf_lint3_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("style.ass");
        let text = "[Script Info]\nTitle: x\n";
        std::fs::write(&path, text).unwrap();
        let error = lint_file(&path, RuleProfile::NetflixAdult).unwrap_err();
        assert!(error.contains("SRT"), "{error}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            text,
            "the file must be untouched"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
