//! Language codes, normalised once so the rest of the pipeline can compare them.
//!
//! Matroska tracks carry free-form language tags: usually three-letter ISO 639-2 (`ara`,
//! `eng`, `jpn`), sometimes two-letter ISO 639-1 (`ar`), sometimes a region suffix
//! (`ar-SA`, `pt_BR`), and often just `und`. A policy is written in the short form, so every
//! code that reaches an [`Inventory`](crate::Inventory) goes through [`normalize_language`].

/// ISO 639-2/B, 639-2/T and legacy codes mapped to ISO 639-1, plus the codes that already
/// are short, mapped to themselves so callers get one canonical form.
const ALIASES: &[(&str, &str)] = &[
    ("ar", "ar"),
    ("ara", "ar"),
    ("en", "en"),
    ("eng", "en"),
    ("ja", "ja"),
    ("jpn", "ja"),
    ("fr", "fr"),
    ("fre", "fr"),
    ("fra", "fr"),
    ("de", "de"),
    ("ger", "de"),
    ("deu", "de"),
    ("es", "es"),
    ("spa", "es"),
    ("it", "it"),
    ("ita", "it"),
    ("pt", "pt"),
    ("por", "pt"),
    ("ru", "ru"),
    ("rus", "ru"),
    ("tr", "tr"),
    ("tur", "tr"),
    ("fa", "fa"),
    ("per", "fa"),
    ("fas", "fa"),
    ("ur", "ur"),
    ("urd", "ur"),
    ("hi", "hi"),
    ("hin", "hi"),
    ("he", "he"),
    ("heb", "he"),
    ("ko", "ko"),
    ("kor", "ko"),
    ("zh", "zh"),
    ("chi", "zh"),
    ("zho", "zh"),
    ("nl", "nl"),
    ("dut", "nl"),
    ("nld", "nl"),
    ("pl", "pl"),
    ("pol", "pl"),
    ("sv", "sv"),
    ("swe", "sv"),
    ("da", "da"),
    ("dan", "da"),
    ("no", "no"),
    ("nor", "no"),
    ("fi", "fi"),
    ("fin", "fi"),
    ("el", "el"),
    ("gre", "el"),
    ("ell", "el"),
    ("cs", "cs"),
    ("cze", "cs"),
    ("ces", "cs"),
    ("hu", "hu"),
    ("hun", "hu"),
    ("ro", "ro"),
    ("rum", "ro"),
    ("ron", "ro"),
    ("uk", "uk"),
    ("ukr", "uk"),
    ("vi", "vi"),
    ("vie", "vi"),
    ("th", "th"),
    ("tha", "th"),
    ("id", "id"),
    ("ind", "id"),
    ("ms", "ms"),
    ("may", "ms"),
    ("msa", "ms"),
    ("bn", "bn"),
    ("ben", "bn"),
    ("ta", "ta"),
    ("tam", "ta"),
    ("te", "te"),
    ("tel", "te"),
    ("ml", "ml"),
    ("mal", "ml"),
    ("tl", "tl"),
    ("fil", "tl"),
    ("tgl", "tl"),
];

/// Codes that must survive untouched: "undetermined" and "no linguistic content" are
/// meaningful answers from a muxer and must not be confused with a real language.
const KEEP: &[&str] = &["und", "zxx", "mul"];

/// Canonical short form of a language tag: lower case, region stripped, 639-2 mapped to
/// 639-1. Unknown codes are returned lower-cased and otherwise untouched.
pub fn normalize_language(code: &str) -> String {
    let trimmed = code.trim().trim_matches('.').to_ascii_lowercase();
    if trimmed.is_empty() {
        return String::new();
    }
    // `ar-SA`, `pt_BR`, `zh-Hans-CN` all reduce to their primary subtag.
    let base = trimmed
        .split(['-', '_'])
        .next()
        .unwrap_or(trimmed.as_str())
        .to_string();
    if base.is_empty() {
        return String::new();
    }
    if KEEP.contains(&base.as_str()) {
        return base;
    }
    ALIASES
        .iter()
        .find(|(alias, _)| *alias == base)
        .map(|(_, canonical)| (*canonical).to_string())
        .unwrap_or(base)
}

/// True when two tags mean the same language.
pub fn same_language(a: &str, b: &str) -> bool {
    normalize_language(a) == normalize_language(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_letter_codes_become_two_letter() {
        assert_eq!(normalize_language("ara"), "ar");
        assert_eq!(normalize_language("eng"), "en");
        assert_eq!(normalize_language("jpn"), "ja");
        assert_eq!(normalize_language("per"), "fa");
        assert_eq!(normalize_language("deu"), "de");
    }

    #[test]
    fn short_codes_stay_short_and_lower_case() {
        assert_eq!(normalize_language("AR"), "ar");
        assert_eq!(normalize_language(" en "), "en");
        assert_eq!(normalize_language("fa"), "fa");
    }

    #[test]
    fn a_region_suffix_is_stripped() {
        assert_eq!(normalize_language("ar-SA"), "ar");
        assert_eq!(normalize_language("pt_BR"), "pt");
        assert_eq!(normalize_language("zh-Hans-CN"), "zh");
    }

    #[test]
    fn undetermined_is_not_a_language_but_is_kept_as_is() {
        assert_eq!(normalize_language("und"), "und");
        assert_eq!(normalize_language("UND"), "und");
        assert_eq!(normalize_language("zxx"), "zxx");
        assert_ne!(normalize_language("und"), "ar");
    }

    #[test]
    fn unknown_codes_are_returned_lower_cased_without_inventing_a_mapping() {
        assert_eq!(normalize_language("qaa"), "qaa");
        assert_eq!(normalize_language("klingon"), "klingon");
    }

    #[test]
    fn empty_input_is_empty_output() {
        assert_eq!(normalize_language(""), "");
        assert_eq!(normalize_language("   "), "");
        assert_eq!(normalize_language("-"), "");
    }

    #[test]
    fn same_language_ignores_the_code_flavour() {
        assert!(same_language("ara", "ar"));
        assert!(same_language("ar-SA", "AR"));
        assert!(!same_language("ara", "eng"));
    }
}
