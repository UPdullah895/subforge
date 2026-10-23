//! SubRip (`.srt`) parsing and writing.

use std::sync::OnceLock;

use regex::Regex;

use super::markup::{ass_to_html, html_to_ass};
use super::{Cue, Format, FormatExtras, SubtitleError};

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

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// A cue number line such as `12`.
fn is_index(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && t.chars().all(|c| c.is_ascii_digit())
}

/// Parse SRT text (already decoded, any line ending) into cues.
///
/// Like pysubs2, timing lines delimit cues: a cue's text runs up to the next
/// timing line, minus the trailing cue number in front of it. So a blank line
/// inside cue text is kept (as `\N\N`) and stray lines between cues join the
/// previous cue. Lines before the first timing line are ignored.
pub(crate) fn parse(text: &str) -> Result<(Vec<Cue>, FormatExtras), SubtitleError> {
    let norm = text.replace("\r\n", "\n").replace('\r', "\n");
    let trailing_newlines = norm.len() - norm.trim_end_matches('\n').len();
    let lines: Vec<&str> = norm.split('\n').collect();

    let timings: Vec<(usize, (i64, i64))> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| parse_timing(l).map(|t| (i, t)))
        .collect();
    if timings.is_empty() {
        if let Some(first) = lines.iter().position(|l| !is_blank(l)) {
            return Err(SubtitleError::Parse {
                path: None,
                line: first + 1,
                msg: "expected a cue timing line such as 00:00:01,000 --> 00:00:02,000".into(),
            });
        }
    }

    let mut cues = Vec::with_capacity(timings.len());
    for (k, &(t, (start_ms, end_ms))) in timings.iter().enumerate() {
        let next = timings.get(k + 1).map_or(lines.len(), |&(n, _)| n);
        let first = t + 1;
        let trim_blank_tail = |mut end: usize| {
            while end > first && is_blank(lines[end - 1]) {
                end -= 1;
            }
            end
        };
        let mut end = trim_blank_tail(next);
        // Drop the following cue's number: it sits on its own line after the
        // text, or alone after blank lines (an empty cue).
        if end > first && is_index(lines[end - 1]) {
            let has_text = lines[first..end - 1].iter().any(|l| !is_blank(l));
            if has_text || end - 1 > first {
                end = trim_blank_tail(end - 1);
            }
        }
        let mut text_start = first;
        while text_start < end && is_blank(lines[text_start]) {
            text_start += 1;
        }
        let body = lines[text_start..end].join("\n");
        let raw_start = if t > 0 && is_index(lines[t - 1]) {
            t - 1
        } else {
            t
        };
        let mut cue = Cue::new(start_ms, end_ms, html_to_ass(body.trim()));
        cue.keep_original(lines[raw_start..end].join("\n"), Format::Srt);
        cues.push(cue);
    }
    Ok((cues, FormatExtras::Srt { trailing_newlines }))
}

/// Serialise cues as SRT text using `\n` line endings.
pub(crate) fn write(cues: &[Cue], trailing_newlines: usize) -> String {
    // ASS `Comment:` events are never rendered, so they have no place in an SRT file.
    let blocks: Vec<String> = cues
        .iter()
        .filter(|cue| !cue.comment)
        .enumerate()
        .map(|(n, cue)| match cue.unchanged_raw(Format::Srt) {
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
