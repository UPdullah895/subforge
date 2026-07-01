//! Shared convention for naming subtitle files that live alongside a video:
//! `{video_stem}.{lang}.{ext}`, with an optional numeric disambiguator when a
//! video has more than one subtitle track in the same language:
//! `{video_stem}.{lang}.{n}.{ext}`.
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

/// Builds the sibling subtitle filename for a video's language, e.g.
/// `external_sub_filename("Show - S01E01", "eng", "srt", None)` ->
/// `"Show - S01E01.eng.srt"`, or with `Some(2)` -> `"Show - S01E01.eng.2.srt"`.
pub fn external_sub_filename(video_stem: &str, lang: &str, ext: &str, disambiguator: Option<u32>) -> String {
    match disambiguator {
        Some(n) => format!("{video_stem}.{lang}.{n}.{ext}"),
        None => format!("{video_stem}.{lang}.{ext}"),
    }
}

/// Parses a candidate sibling filename against a video's stem, returning the
/// detected `(lang, ext)` if `candidate_name` matches the
/// `{video_stem}.{lang}[.{n}].{ext}` convention.
pub fn parse_external_sub_filename(video_stem: &str, candidate_name: &str) -> Option<(String, String)> {
    let prefix = format!("{video_stem}.");
    if !candidate_name.starts_with(&prefix) {
        return None;
    }
    let rest = &candidate_name[prefix.len()..];

    let mut parts = rest.rsplitn(2, '.');
    let ext = parts.next()?.to_lowercase();
    let mut lang_part = parts.next()?;

    // Strip an optional numeric disambiguator segment: "{lang}.{n}".
    if let Some(dot_idx) = lang_part.rfind('.') {
        let (maybe_lang, maybe_num) = lang_part.split_at(dot_idx);
        let maybe_num = &maybe_num[1..];
        if !maybe_num.is_empty() && maybe_num.chars().all(|c| c.is_ascii_digit()) {
            lang_part = maybe_lang;
        }
    }
    let lang = lang_part.to_lowercase();

    if SUBTITLE_EXTS.contains(&ext.as_str()) && KNOWN_LANG_CODES.contains(&lang.as_str()) {
        Some((lang, ext))
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
    fn parses_plain_filename() {
        assert_eq!(
            parse_external_sub_filename("Show - S01E01", "Show - S01E01.ar.srt"),
            Some(("ar".to_string(), "srt".to_string()))
        );
    }

    #[test]
    fn parses_disambiguated_filename() {
        assert_eq!(
            parse_external_sub_filename("Show - S01E01", "Show - S01E01.eng.2.srt"),
            Some(("eng".to_string(), "srt".to_string()))
        );
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
