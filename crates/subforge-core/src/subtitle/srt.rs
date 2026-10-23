//! SubRip (`.srt`) parsing and writing.

use std::sync::OnceLock;

use regex::Regex;

use super::markup::{ass_to_html, html_to_ass};
use super::{Cue, FormatExtras, SubtitleError};

fn timing_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^\s*(\d+):(\d{1,2}):(\d{1,2})[,.](\d{1,3})\s*-->\s*(\d+):(\d{1,2}):(\d{1,2})[,.](\d{1,3})",
        )
        .expect("static regex")
    })
}

fn parse_timing(line: &str) -> Option<(i64, i64)> {
    let c = timing_re().captures(line)?;
    let num = |i: usize| c[i].parse::<i64>().ok();
    let frac = |i: usize| -> Option<i64> {
        // "5" means 500 ms, "25" means 250 ms: the digits are a decimal fraction.
        format!("{:0<3}", &c[i]).parse().ok()
    };
    let t = |h, m, s, f| Some(((num(h)? * 60 + num(m)?) * 60 + num(s)?) * 1000 + frac(f)?);
    Some((t(1, 2, 3, 4)?, t(5, 6, 7, 8)?))
}

/// Parse SRT text (already decoded, any line ending) into cues.
pub(crate) fn parse(text: &str) -> Result<(Vec<Cue>, FormatExtras), SubtitleError> {
    let norm = text.replace("\r\n", "\n").replace('\r', "\n");
    let trailing_newlines = norm.len() - norm.trim_end_matches('\n').len();
    let lines: Vec<&str> = norm.split('\n').collect();

    let mut cues = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if line.is_empty() {
            i += 1;
            continue;
        }
        let start = i;
        let timing_idx = if parse_timing(line).is_some() {
            i
        } else if line.chars().all(|c| c.is_ascii_digit())
            && lines.get(i + 1).is_some_and(|l| parse_timing(l).is_some())
        {
            i + 1
        } else {
            return Err(SubtitleError::Parse {
                path: None,
                line: i + 1,
                msg: "expected a cue timing line such as 00:00:01,000 --> 00:00:02,000".into(),
            });
        };
        let (start_ms, end_ms) = parse_timing(lines[timing_idx]).expect("checked above");
        let mut end = timing_idx + 1;
        while end < lines.len() && !lines[end].trim().is_empty() {
            end += 1;
        }
        let body = lines[timing_idx + 1..end].join("\n");
        let mut cue = Cue::new(start_ms, end_ms, html_to_ass(&body));
        cue.keep_original(lines[start..end].join("\n"));
        cues.push(cue);
        i = end;
    }
    Ok((cues, FormatExtras::Srt { trailing_newlines }))
}

/// Serialise cues as SRT text using `\n` line endings.
pub(crate) fn write(cues: &[Cue], trailing_newlines: usize) -> String {
    let blocks: Vec<String> = cues
        .iter()
        .enumerate()
        .map(|(n, cue)| match cue.unchanged_raw() {
            Some(raw) => raw.to_owned(),
            None => format!(
                "{}\n{} --> {}\n{}",
                n + 1,
                format_time(cue.start_ms),
                format_time(cue.end_ms),
                ass_to_html(&cue.text)
            ),
        })
        .collect();
    let mut out = blocks.join("\n\n");
    if !blocks.is_empty() {
        out.push_str(&"\n".repeat(trailing_newlines));
    }
    out
}

fn format_time(ms: i64) -> String {
    let ms = ms.max(0);
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}
