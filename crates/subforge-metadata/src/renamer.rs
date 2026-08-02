use crate::Episode;

/// Sanitize a filename component against Windows-reserved characters,
/// even on Linux/macOS builds, since renamed files may later be moved
/// to or shared with a Windows machine.
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Apply a naming pattern with tokens: {title} {s} {e} {episodeTitle} {quality}.
/// {s}/{e} are zero-padded to 2 digits.
pub fn apply_pattern(pattern: &str, title: &str, episode: &Episode, quality: &str) -> String {
    pattern
        .replace("{title}", title)
        .replace("{s}", &format!("{:02}", episode.season))
        .replace("{e}", &format!("{:02}", episode.episode))
        .replace("{episodeTitle}", &episode.title)
        .replace("{quality}", quality)
}

/// Build a sanitized output filename (including extension) from a pattern.
pub fn build_output_name(
    pattern: &str,
    title: &str,
    episode: &Episode,
    tags: &[String],
    extension: &str,
) -> String {
    let quality = tags.first().map(String::as_str).unwrap_or("");
    let name = apply_pattern(pattern, title, episode, quality);
    format!("{}.{}", sanitize_filename(&name), extension.trim_start_matches('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_reserved_characters() {
        assert_eq!(sanitize_filename(r#"Show: The "Return"? <2>|3\4/5*"#), "Show_ The _Return__ _2__3_4_5_");
    }

    #[test]
    fn applies_pattern_tokens() {
        let ep = Episode {
            season: 1,
            episode: 2,
            title: "Pilot".to_string(),
        };
        let out = apply_pattern("{title} - S{s}E{e} - {episodeTitle}", "My Show", &ep, "1080p");
        assert_eq!(out, "My Show - S01E02 - Pilot");
    }

    #[test]
    fn builds_sanitized_output_name() {
        let ep = Episode {
            season: 1,
            episode: 2,
            title: "Colon: Test".to_string(),
        };
        let out = build_output_name(
            "{title} - S{s}E{e} - {episodeTitle}",
            "My Show",
            &ep,
            &["1080p".to_string()],
            "srt",
        );
        assert_eq!(out, "My Show - S01E02 - Colon_ Test.srt");
    }
}
