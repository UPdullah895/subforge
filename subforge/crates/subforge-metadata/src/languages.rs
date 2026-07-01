/// (ISO 639-1 code, English name, native name) for languages offered across
/// SubForge's translate/rename UIs. Used to auto-generate track names like
/// "Arabic - عربي" when merging subtitles/audio into a video, so users don't
/// have to hand-type them for every language.
const LANGUAGES: &[(&str, &str, &str)] = &[
    ("en", "English", "English"),
    ("ar", "Arabic", "عربي"),
    ("es", "Spanish", "Español"),
    ("fr", "French", "Français"),
    ("de", "German", "Deutsch"),
    ("ja", "Japanese", "日本語"),
    ("ko", "Korean", "한국어"),
    ("zh", "Chinese", "中文"),
    ("ru", "Russian", "Русский"),
    ("pt", "Portuguese", "Português"),
    ("it", "Italian", "Italiano"),
    ("tr", "Turkish", "Türkçe"),
    ("hi", "Hindi", "हिन्दी"),
    ("fa", "Persian", "فارسی"),
    ("ur", "Urdu", "اردو"),
    ("nl", "Dutch", "Nederlands"),
    ("pl", "Polish", "Polski"),
    ("sv", "Swedish", "Svenska"),
    ("th", "Thai", "ไทย"),
    ("vi", "Vietnamese", "Tiếng Việt"),
    ("id", "Indonesian", "Bahasa Indonesia"),
    ("und", "Undetermined", "Undetermined"),
];

pub fn english_name(code: &str) -> Option<&'static str> {
    let code = code.to_lowercase();
    LANGUAGES
        .iter()
        .find(|(c, _, _)| *c == code)
        .map(|(_, en, _)| *en)
}

pub fn native_name(code: &str) -> Option<&'static str> {
    let code = code.to_lowercase();
    LANGUAGES
        .iter()
        .find(|(c, _, _)| *c == code)
        .map(|(_, _, native)| *native)
}

/// Builds an auto-generated track name like "Arabic - عربي" (or just
/// "Arabic" when the native/English names are identical, e.g. English
/// itself), appending "(SDH)" when `hearing_impaired` is set — unless the
/// name already mentions it, so we don't double-tag a file that already
/// carries its own hearing-impaired label.
pub fn track_display_name(code: &str, hearing_impaired: bool) -> String {
    let english = english_name(code).unwrap_or(code);
    let native = native_name(code).unwrap_or(code);

    let mut name = if english.eq_ignore_ascii_case(native) {
        english.to_string()
    } else {
        format!("{english} - {native}")
    };

    if hearing_impaired && !name.to_lowercase().contains("sdh") {
        name.push_str(" (SDH)");
    }

    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_display_name_with_native_script() {
        assert_eq!(track_display_name("ar", false), "Arabic - عربي");
    }

    #[test]
    fn appends_sdh_suffix_once() {
        assert_eq!(track_display_name("ar", true), "Arabic - عربي (SDH)");
    }

    #[test]
    fn falls_back_to_code_for_unknown_language() {
        assert_eq!(track_display_name("xx", false), "xx");
    }

    #[test]
    fn skips_duplicate_name_for_english() {
        assert_eq!(track_display_name("en", false), "English");
    }
}
