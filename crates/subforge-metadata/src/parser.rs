use crate::ParsedFilename;
use regex::Regex;
use std::sync::LazyLock;

// Tried in order; the first pattern that matches wins.
static SEASON_EP_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"(?i)[Ss](\d{1,2})[Ee](\d{1,3})").unwrap(),
        Regex::new(r"(?i)(\d{1,2})x(\d{1,3})").unwrap(),
        Regex::new(r"(?i)[Ee]pisode[.\s_-]?(\d{1,3})").unwrap(),
    ]
});

static TAG_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"(?i)\b(2160p|1080p|720p|480p)\b").unwrap(),
        Regex::new(r"(?i)\b(x264|x265|h264|h265|hevc|avc)\b").unwrap(),
        Regex::new(r"\[([^\]]+)\]").unwrap(),
    ]
});

/// Parse a filename stem (no extension) into a series guess, season/episode,
/// and any quality/codec/release-group tags found.
pub fn parse_filename(stem: &str) -> ParsedFilename {
    let mut season = None;
    let mut episode = None;
    let mut match_start = stem.len();

    for (idx, re) in SEASON_EP_PATTERNS.iter().enumerate() {
        if let Some(caps) = re.captures(stem) {
            let m = caps.get(0).unwrap();
            match_start = match_start.min(m.start());
            if idx == 2 {
                // "Episode N" pattern has no season group; default to season 1.
                season = Some(1);
                episode = caps.get(1).and_then(|g| g.as_str().parse().ok());
            } else {
                season = caps.get(1).and_then(|g| g.as_str().parse().ok());
                episode = caps.get(2).and_then(|g| g.as_str().parse().ok());
            }
            break;
        }
    }

    let mut tags = Vec::new();
    for re in TAG_PATTERNS.iter() {
        for caps in re.captures_iter(stem) {
            if let Some(g) = caps.get(1) {
                tags.push(g.as_str().to_string());
                match_start = match_start.min(caps.get(0).unwrap().start());
            }
        }
    }

    let series_guess = clean_series_guess(&stem[..match_start.min(stem.len())]);

    ParsedFilename {
        series_guess,
        season,
        episode,
        tags,
    }
}

fn clean_series_guess(raw: &str) -> String {
    raw.replace(['.', '_'], " ")
        .trim_matches(|c: char| c == '-' || c.is_whitespace())
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_s_e_pattern() {
        let p = parse_filename("Show.Name.S01E02.1080p.x264-GROUP");
        assert_eq!(p.season, Some(1));
        assert_eq!(p.episode, Some(2));
        assert_eq!(p.series_guess, "Show Name");
        assert!(p.tags.contains(&"1080p".to_string()));
        assert!(p.tags.contains(&"x264".to_string()));
    }

    #[test]
    fn parses_nxn_pattern() {
        let p = parse_filename("Another Show 3x07 720p");
        assert_eq!(p.season, Some(3));
        assert_eq!(p.episode, Some(7));
        assert_eq!(p.series_guess, "Another Show");
    }

    #[test]
    fn parses_episode_word_pattern() {
        let p = parse_filename("Podcast Episode 42");
        assert_eq!(p.season, Some(1));
        assert_eq!(p.episode, Some(42));
    }

    #[test]
    fn no_match_returns_none() {
        let p = parse_filename("random_video_file");
        assert_eq!(p.season, None);
        assert_eq!(p.episode, None);
    }
}
