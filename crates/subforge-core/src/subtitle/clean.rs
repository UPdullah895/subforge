//! Remove translator credits, watermarks and advertisement lines.
//!
//! The rules are deliberately conservative and pattern-based: a cue is removed
//! only when its visible text contains a link, an email address, a social-media
//! handle, or an unambiguous credit/advertisement phrase. Dialogue that merely
//! mentions a site is left alone unless the whole line is short enough to be a
//! credit card.
//!
//! Removal is a decision the caller makes after showing a preview: [`preview`]
//! lists exactly what [`clean`] would remove, and `keep` lets the user veto
//! individual cues.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use super::{SubtitleDoc, SubtitleError};

/// What the caller wants removed. `keep` holds cue indexes the user chose to
/// keep in the preview.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CleanOptions {
    #[serde(default)]
    pub keep: Vec<usize>,
}

/// One cue the cleaner matched.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemovedCue {
    /// Index in the original document (stable across preview and apply).
    pub index: usize,
    pub start_ms: i64,
    pub end_ms: i64,
    /// Visible text, with ASS override tags and line breaks removed.
    pub text: String,
    /// Why it matched: `link`, `email`, `handle` or `credit`.
    pub rule: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CleanReport {
    pub removed: Vec<RemovedCue>,
    pub kept: usize,
}

impl CleanReport {
    pub fn is_empty(&self) -> bool {
        self.removed.is_empty()
    }
}

fn link_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(concat!(
            r"(?i)(?:https?://|www\.)\S+",
            r"|\b[a-z0-9][a-z0-9-]{0,62}\.(?:com|net|org|me|tv|link|xyz|info|co|io|app|site|online|top|club)\b"
        ))
        .expect("static regex")
    })
}

fn email_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\b[\w.+-]+@[\w-]+\.[\w.-]{2,}\b").expect("static regex")
    })
}

/// `@name` at the start of the text or after a space/parenthesis. Emails are
/// tested first, so `a@b.com` is never reported as a handle.
fn handle_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)(?:^|[\s(])@[a-z0-9_]{2,}").expect("static regex")
    })
}

/// Unambiguous credit/advertisement phrases. Matched case-insensitively against
/// the visible text, and only on short lines (see [`MAX_CREDIT_CHARS`]).
const CREDIT_PHRASES: &[&str] = &[
    // English
    "translated by",
    "translation by",
    "translations by",
    "subtitles by",
    "subtitle by",
    "subtitled by",
    "synced by",
    "synchronized by",
    "sync by",
    "timed by",
    "timing by",
    "captioned by",
    "encoded by",
    "ripped by",
    "retimed by",
    "corrected by",
    "subtitles downloaded",
    "downloaded from",
    "join us on",
    "follow us on",
    "find us on",
    "subscribe to our",
    // Arabic: explicit labels (colon or a word that is not ordinary dialogue)
    "ترجمة:",
    "ترجمه:",
    "إعداد:",
    "اعداد:",
    "توقيت:",
    "مزامنة:",
    "فريق الترجمة",
    "الترجمة من",
    "ترجمة وتوقيت",
    "ترجمة وإعداد",
    "إعداد وتوقيت",
    "حقوق النشر",
    "جميع الحقوق",
    "قناتنا على",
    "قناتنا في",
    "قناتي على",
    "اشترك في",
    "اشترك بقناة",
    "تابعنا على",
    "تابعونا على",
    "لا تنسى الاشتراك",
    "لمتابعة باقي",
    "لمشاهدة الحلقات",
    "شاهد الحلقة",
];

/// Lines longer than this are treated as dialogue even if a phrase matches.
const MAX_CREDIT_CHARS: usize = 120;

/// A cue whose visible text is exactly one of these is a credit card.
const CREDIT_WORDS: &[&str] = &["ترجمة", "ترجمه", "الترجمة", "اعداد", "إعداد", "توقيت"];

fn rule_for(text: &str) -> Option<&'static str> {
    let visible = crate::lines::plain_text(text);
    let trimmed = visible.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Emails first: the domain part would otherwise look like a plain link.
    if email_re().is_match(trimmed) {
        return Some("email");
    }
    if link_re().is_match(trimmed) {
        return Some("link");
    }
    if handle_re().is_match(trimmed) {
        return Some("handle");
    }

    let chars = trimmed.chars().count();
    if chars > MAX_CREDIT_CHARS {
        return None;
    }
    let lower = trimmed.to_lowercase();
    let bare = lower.trim_matches(['.', '!', '،', ',']).trim();
    if CREDIT_WORDS.contains(&bare) {
        return Some("credit");
    }
    CREDIT_PHRASES
        .iter()
        .any(|phrase| lower.contains(phrase))
        .then_some("credit")
}

/// Cues the cleaner would remove, without changing `doc`.
pub fn preview(doc: &SubtitleDoc, opts: &CleanOptions) -> CleanReport {
    let keep = |i: usize| opts.keep.contains(&i);
    let removed: Vec<RemovedCue> = doc
        .cues
        .iter()
        .enumerate()
        .filter(|(i, cue)| !cue.comment && !keep(*i))
        .filter_map(|(index, cue)| {
            let rule = rule_for(&cue.text)?;
            Some(RemovedCue {
                index,
                start_ms: cue.start_ms,
                end_ms: cue.end_ms,
                text: crate::lines::plain_text(&cue.text),
                rule: rule.to_string(),
            })
        })
        .collect();
    let kept = doc.cues.len() - removed.len();
    CleanReport { removed, kept }
}

/// Remove the matched cues from `doc` and report what went. Comment cues are
/// never removed (they are not rendered anyway).
pub fn clean(doc: &mut SubtitleDoc, opts: &CleanOptions) -> CleanReport {
    let report = preview(doc, opts);
    let mut drop = vec![false; doc.cues.len()];
    for cue in &report.removed {
        if let Some(slot) = drop.get_mut(cue.index) {
            *slot = true;
        }
    }
    doc.remove_cues(&drop);
    report
}

/// Load `input`, clean it and save it to `output` (same format, UTF-8). When
/// `output` differs from `input`, the source file is left untouched.
pub fn clean_file(
    input: &Path,
    output: &Path,
    opts: &CleanOptions,
) -> Result<CleanReport, SubtitleError> {
    let mut doc = SubtitleDoc::load(input)?;
    let report = clean(&mut doc, opts);
    doc.save(output)?;
    Ok(report)
}

#[derive(Debug, Clone)]
pub struct CleanOutcome {
    pub report: CleanReport,
    /// Where the cleaned file was written; `None` for a preview.
    pub output: Option<PathBuf>,
    /// Copy of the original, made when the destination is the input itself.
    pub backup: Option<PathBuf>,
}

/// `{dir}/{stem}.clean.{ext}` next to `input`.
pub fn default_output(input: &Path) -> PathBuf {
    let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("srt");
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("subtitle");
    input.with_file_name(format!("{stem}.clean.{ext}"))
}

/// `{name}.{ext}.bak` next to `input`.
pub fn backup_path(input: &Path) -> PathBuf {
    let name = input
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("subtitle");
    input.with_file_name(format!("{name}.bak"))
}

/// The whole "preview, then apply safely" flow used by both front ends.
///
/// With `apply == false` nothing is written. With `apply == true` the cleaned
/// file goes to `output` (or [`default_output`]); if that is the input itself,
/// the original is first copied to [`backup_path`] so a clean can always be
/// undone.
pub fn clean_file_to(
    input: &Path,
    output: Option<&Path>,
    opts: &CleanOptions,
    apply: bool,
) -> Result<CleanOutcome, SubtitleError> {
    let mut doc = SubtitleDoc::load(input)?;
    let report = clean(&mut doc, opts);
    if !apply {
        return Ok(CleanOutcome {
            report,
            output: None,
            backup: None,
        });
    }

    let output = output
        .filter(|p| !p.as_os_str().to_string_lossy().trim().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| default_output(input));

    let backup = if same_file(input, &output) {
        let backup = backup_path(input);
        std::fs::copy(input, &backup).map_err(|source| SubtitleError::Io {
            path: backup.clone(),
            source,
        })?;
        Some(backup)
    } else {
        None
    };

    doc.save(&output)?;
    Ok(CleanOutcome {
        report,
        output: Some(output),
        backup,
    })
}

/// Path equality that does not require the destination to exist yet.
fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subtitle::{Cue, Format};

    fn doc(texts: &[&str]) -> SubtitleDoc {
        let cues = texts
            .iter()
            .enumerate()
            .map(|(i, t)| Cue::new(i as i64 * 1000, i as i64 * 1000 + 900, *t))
            .collect();
        SubtitleDoc::new(Format::Srt, cues)
    }

    fn rules(doc: &SubtitleDoc) -> Vec<&'static str> {
        preview(doc, &CleanOptions::default())
            .removed
            .iter()
            .map(|c| match c.rule.as_str() {
                "link" => "link",
                "email" => "email",
                "handle" => "handle",
                _ => "credit",
            })
            .collect()
    }

    #[test]
    fn removes_links_domains_and_handles() {
        let d = doc(&[
            "Hello there.",
            "Visit https://example.com for more",
            "www.example.net",
            "Follow @some_group",
            "yes",
        ]);
        assert_eq!(rules(&d), vec!["link", "link", "handle"]);
    }

    #[test]
    fn removes_email_but_not_as_a_handle() {
        let d = doc(&["contact me: someone@example.org"]);
        assert_eq!(rules(&d), vec!["email"]);
    }

    #[test]
    fn removes_english_and_arabic_credit_lines() {
        let d = doc(&[
            "Translated by the Example Team",
            "ترجمة: أحمد",
            "فريق الترجمة",
            "جميع الحقوق محفوظة",
            "لا تنسى الاشتراك في القناة",
            "مرحبا، كيف حالك؟",
        ]);
        assert_eq!(
            rules(&d),
            vec!["credit", "credit", "credit", "credit", "credit"]
        );
    }

    #[test]
    fn keeps_ordinary_dialogue_that_mentions_a_word() {
        let d = doc(&[
            "هل شاهدت ذلك الفيديو على يوتيوب؟",
            "I saw it on telegram yesterday, it was funny",
            "هذه الترجمة سيئة للغاية",
        ]);
        assert!(rules(&d).is_empty(), "{:?}", preview(&d, &CleanOptions::default()));
    }

    #[test]
    fn strips_ass_tags_before_matching() {
        let d = doc(&["{\\i1}Subtitles by{\\i0} Somebody"]);
        assert_eq!(rules(&d), vec!["credit"]);
    }

    #[test]
    fn keep_prevents_removal_of_a_specific_cue() {
        let d = doc(&["Translated by X", "Hello"]);
        let opts = CleanOptions { keep: vec![0] };
        assert!(preview(&d, &opts).removed.is_empty());
    }

    #[test]
    fn clean_removes_only_the_matches() {
        let mut d = doc(&["Translated by X", "Hello", "www.example.com"]);
        let report = clean(&mut d, &CleanOptions::default());
        assert_eq!(report.removed.len(), 2);
        assert_eq!(report.kept, 1);
        assert_eq!(d.cues.len(), 1);
        assert_eq!(d.cues[0].text, "Hello");
    }

    #[test]
    fn comments_are_never_removed() {
        let mut d = doc(&["www.example.com"]);
        d.cues[0].comment = true;
        assert!(preview(&d, &CleanOptions::default()).removed.is_empty());
        let report = clean(&mut d, &CleanOptions::default());
        assert_eq!(report.removed.len(), 0);
        assert_eq!(d.cues.len(), 1);
    }

    #[test]
    fn clean_is_idempotent() {
        let mut d = doc(&["Translated by X", "Hello"]);
        clean(&mut d, &CleanOptions::default());
        let again = clean(&mut d, &CleanOptions::default());
        assert!(again.removed.is_empty());
    }
}
