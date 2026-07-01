//! Shared convention for naming subtitle files that live alongside a video:
//! `{video_stem}.{lang}[.{flavor}][.{n}].{ext}`, where `flavor` is an optional
//! "forced"/"sdh" tag (so a video with a regular, forced, and SDH track of
//! the same language extracts to three distinguishable filenames instead of
//! a plain running number), and `n` is a numeric disambiguator used only as a
//! last resort when two tracks share both language and flavor.
//!
//! Both the subtitle-extraction output naming and the external-subtitle
//! directory scan must agree on this exact convention, so the app can pick
//! its own extracted/translated output files back up as "external subs" on
//! a later scan. This module is the single place that builds and parses it.

pub const SUBTITLE_EXTS: &[&str] = &["srt", "ass"];

pub const KNOWN_LANG_CODES: &[&str] = &[
    "en", "ar", "es", "fr", "de", "ja", "ko", "zh", "ru", "pt", "it", "tr", "hi", "fa", "ur", "eng",
    "ara", "spa", "fre", "ger", "jpn", "kor", "chi", "rus", "por", "ita", "tur", "hin", "per", "urd",
];

pub const KNOWN_FLAVORS: &[&str] = &["forced", "sdh"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedExternalSub {
    pub lang: String,
    pub ext: String,
    pub flavor: Option<String>,
}

/// Builds the sibling subtitle filename for a video's language, e.g.
/// `external_sub_filename("Show - S01E01", "eng", "srt", None)` ->
/// `"Show - S01E01.eng.srt"`, or with `Some(2)` -> `"Show - S01E01.eng.2.srt"`.
pub fn external_sub_filename(video_stem: &str, lang: &str, ext: &str, disambiguator: Option<u32>) -> String {
    external_sub_filename_flavored(video_stem, lang, None, ext, disambiguator)
}

/// Like `external_sub_filename`, but with an optional flavor tag ("forced"
/// or "sdh") inserted before the disambiguator/extension.
pub fn external_sub_filename_flavored(
    video_stem: &str,
    lang: &str,
    flavor: Option<&str>,
    ext: &str,
    disambiguator: Option<u32>,
) -> String {
    let mut name = format!("{video_stem}.{lang}");
    if let Some(f) = flavor {
        name.push('.');
        name.push_str(f);
    }
    if let Some(n) = disambiguator {
        name.push('.');
        name.push_str(&n.to_string());
    }
    name.push('.');
    name.push_str(ext);
    name
}

/// Parses a candidate sibling filename against a video's stem, returning the
/// detected language/extension/flavor if `candidate_name` matches the
/// `{video_stem}.{lang}[.{flavor}][.{n}].{ext}` convention.
pub fn parse_external_sub_filename(video_stem: &str, candidate_name: &str) -> Option<ParsedExternalSub> {
    let prefix = format!("{video_stem}.");
    if !candidate_name.starts_with(&prefix) {
        return None;
    }
    let rest = &candidate_name[prefix.len()..];

    let mut parts: Vec<&str> = rest.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let ext = parts.pop()?.to_lowercase();
    if !SUBTITLE_EXTS.contains(&ext.as_str()) {
        return None;
    }

    // Strip an optional trailing numeric disambiguator segment.
    if let Some(last) = parts.last() {
        if !last.is_empty() && last.chars().all(|c| c.is_ascii_digit()) {
            parts.pop();
        }
    }

    // Strip an optional trailing flavor segment (forced/sdh).
    let mut flavor = None;
    if let Some(last) = parts.last() {
        let lower = last.to_lowercase();
        if KNOWN_FLAVORS.contains(&lower.as_str()) {
            flavor = Some(lower);
            parts.pop();
        }
    }

    if parts.len() != 1 {
        return None;
    }
    let lang = parts[0].to_lowercase();

    if KNOWN_LANG_CODES.contains(&lang.as_str()) {
        Some(ParsedExternalSub { lang, ext, flavor })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_plain_filename_without_disambiguator() {
        assert_eq!(
            external_sub_filename("Show - S01E01", "eng", "srt", None),
            "Show - S01E01.eng.srt"
        );
    }

    #[test]
    fn builds_disambiguated_filename() {
        assert_eq!(
            external_sub_filename("Show - S01E01", "eng", "srt", Some(2)),
            "Show - S01E01.eng.2.srt"
        );
    }

    #[test]
    fn builds_flavored_filename() {
        assert_eq!(
            external_sub_filename_flavored("Show - S01E01", "ita", Some("forced"), "srt", None),
            "Show - S01E01.ita.forced.srt"
        );
    }

    #[test]
    fn parses_plain_filename() {
        let parsed = parse_external_sub_filename("Show - S01E01", "Show - S01E01.ar.srt").unwrap();
        assert_eq!(parsed.lang, "ar");
        assert_eq!(parsed.ext, "srt");
        assert_eq!(parsed.flavor, None);
    }

    #[test]
    fn parses_disambiguated_filename() {
        let parsed = parse_external_sub_filename("Show - S01E01", "Show - S01E01.eng.2.srt").unwrap();
        assert_eq!(parsed.lang, "eng");
        assert_eq!(parsed.flavor, None);
    }

    #[test]
    fn parses_flavored_filename() {
        let parsed = parse_external_sub_filename("Show - S01E01", "Show - S01E01.ita.forced.srt").unwrap();
        assert_eq!(parsed.lang, "ita");
        assert_eq!(parsed.flavor.as_deref(), Some("forced"));
    }

    #[test]
    fn parses_flavored_and_disambiguated_filename() {
        let parsed = parse_external_sub_filename("Show - S01E01", "Show - S01E01.eng.sdh.1.srt").unwrap();
        assert_eq!(parsed.lang, "eng");
        assert_eq!(parsed.flavor.as_deref(), Some("sdh"));
    }

    #[test]
    fn rejects_unknown_lang_and_mismatched_stem() {
        assert_eq!(
            parse_external_sub_filename("Show - S01E01", "Show - S01E01.xyz.srt"),
            None
        );
        assert_eq!(
            parse_external_sub_filename("Show - S01E01", "Other - S01E01.ar.srt"),
            None
        );
    }
}
