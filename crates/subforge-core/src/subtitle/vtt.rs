//! WebVTT (`.vtt`) parsing and writing.
//!
//! `NOTE`, `STYLE` and `REGION` blocks are not kept. A cue keeps its identifier line and its
//! cue settings (`align:start ...`), so editing the times or text does not lose them; an
//! untouched cue is written back verbatim.
//!
//! A leading voice tag (`<v Speaker>` / `<v.class Speaker>`) is stored with the cue and
//! re-emitted around the text when an edited cue is regenerated. The other inline tags (`<c>`,
//! `<lang>`, ruby, inline timestamps) are dropped from the text at parse time, so they are lost
//! once the cue is edited (or converted); `<i>`, `<b>` and `<u>` are kept.

use std::sync::OnceLock;

use regex::Regex;

use super::markup::{ass_to_html, html_to_ass};
use super::{Cue, CueExtra, Format, FormatExtras, SubtitleError};

fn timing_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        let t = r"(?:(\d+):)?(\d+):(\d{1,2})[.,](\d{1,3})";
        Regex::new(&format!(r"^\s*{t}\s*-->\s*{t}(.*)$")).expect("static regex")
    })
}

/// Parse a timing line into (start, end, trailing cue settings).
fn parse_timing(line: &str) -> Option<(i64, i64, String)> {
    let c = timing_re().captures(line)?;
    let num = |i: usize| c.get(i).map_or(Some(0), |m| m.as_str().parse::<i64>().ok());
    let t = |h, m, s, f: usize| -> Option<i64> {
        let frac: i64 = format!("{:0<3}", &c[f]).parse().ok()?;
        Some(((num(h)? * 60 + num(m)?) * 60 + num(s)?) * 1000 + frac)
    };
    Some((t(1, 2, 3, 4)?, t(5, 6, 7, 8)?, c[9].trim().to_owned()))
}

fn is_special_block(first: &str) -> bool {
    ["NOTE", "STYLE", "REGION"].iter().any(|k| {
        first
            .strip_prefix(k)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
    })
}

/// Parse decoded WebVTT text (any line ending). Text without a `WEBVTT` header is accepted too.
pub(crate) fn parse(text: &str) -> Result<(Vec<Cue>, FormatExtras), SubtitleError> {
    let norm = text.replace("\r\n", "\n").replace('\r', "\n");
    let norm = norm.strip_prefix('\u{feff}').unwrap_or(&norm);
    let trailing_newlines = norm.len() - norm.trim_end_matches('\n').len();
    let lines: Vec<&str> = norm.split('\n').collect();

    let mut header = "WEBVTT".to_owned();
    let mut i = 0;
    if lines.first().is_some_and(|l| l.starts_with("WEBVTT")) {
        while i < lines.len() && !lines[i].trim().is_empty() {
            i += 1;
        }
        header = lines[..i].join("\n");
    }

    let mut cues = Vec::new();
    while i < lines.len() {
        if lines[i].trim().is_empty() {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i;
        while end < lines.len() && !lines[end].trim().is_empty() {
            end += 1;
        }
        i = end;
        if is_special_block(lines[start]) {
            continue;
        }
        let found =
            (start..end.min(start + 2)).find_map(|n| parse_timing(lines[n]).map(|t| (n, t)));
        let Some((timing_idx, (start_ms, end_ms, settings))) = found else {
            return Err(SubtitleError::Parse {
                path: None,
                line: start + 1,
                msg: "expected a cue timing line such as 00:00:01.000 --> 00:00:02.000".into(),
            });
        };
        let id = (timing_idx > start).then(|| lines[start].to_owned());
        let body = lines[timing_idx + 1..end].join("\n");
        let mut cue = Cue::new(start_ms, end_ms, vtt_to_ass(&body));
        cue.keep_original(lines[start..end].join("\n"), Format::Vtt);
        cue.extra = CueExtra::Vtt {
            id,
            settings,
            voice: leading_voice(&body),
        };
        cues.push(cue);
    }
    Ok((
        cues,
        FormatExtras::Vtt {
            header,
            trailing_newlines,
        },
    ))
}

/// The opening tag text (without `<` `>`) of a voice span that starts the cue, e.g. `v Mary`.
fn leading_voice(body: &str) -> Option<String> {
    let rest = body.trim_start().strip_prefix('<')?;
    let inner = &rest[..rest.find('>')?];
    let after_v = inner.strip_prefix('v')?;
    after_v
        .starts_with([' ', '\t', '.'])
        .then(|| inner.to_owned())
}

fn is_timestamp(tag: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d+:\d+(:\d+)?\.\d+$").expect("static regex"))
        .is_match(tag.trim())
}

/// WebVTT cue text to internal (ASS-convention) text.
fn vtt_to_ass(body: &str) -> String {
    unescape(&html_to_ass(&strip_vtt_tags(body)))
}

/// Drop the WebVTT-only tags (`<c.x>`, `<v Name>`, `<lang>`, ruby, timestamps) but keep their text.
fn strip_vtt_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('<') {
        out.push_str(&rest[..p]);
        let after = &rest[p..];
        let Some(e) = after.find('>') else {
            rest = after;
            break;
        };
        if after[1..e].contains('<') {
            // A stray `<` (as in "I <3 you"): literal text, the real tag comes later.
            out.push('<');
            rest = &after[1..];
            continue;
        }
        let inner = after[1..e].trim_start_matches('/');
        let name: String = inner
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '.')
            .collect();
        let vtt_only = matches!(name.as_str(), "c" | "v" | "lang" | "ruby" | "rt" | "rp")
            || is_timestamp(inner);
        if vtt_only {
            // dropped, text kept
        } else if matches!(name.as_str(), "i" | "b" | "u") && inner.len() > name.len() {
            // `<i.foo>` is italic with a class: keep it as plain `<i>`.
            let slash = if after[1..].starts_with('/') { "/" } else { "" };
            out.push_str(&format!("<{slash}{name}>"));
        } else {
            out.push_str(&after[..=e]);
        }
        rest = &after[e + 1..];
    }
    out.push_str(rest);
    out
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&lrm;", "\u{200e}")
        .replace("&rlm;", "\u{200f}")
        .replace("&nbsp;", "\\h")
        .replace("&amp;", "&")
}

/// Escape `&` and `<` in HTML produced by `ass_to_html`, keeping the tags WebVTT knows
/// (`i`, `b`, `u`) and dropping strike-through, which it has no tag for.
fn escape(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(c) = rest.chars().next() {
        if let Some(tag) = ["<i>", "</i>", "<b>", "</b>", "<u>", "</u>"]
            .iter()
            .find(|t| rest.starts_with(**t))
        {
            out.push_str(tag);
            rest = &rest[tag.len()..];
        } else if rest.starts_with("<s>") || rest.starts_with("</s>") {
            rest = &rest[rest.find('>').expect("matched above") + 1..];
        } else if rest.starts_with("-->") {
            out.push_str("--&gt;");
            rest = &rest[3..];
        } else {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                _ => out.push(c),
            }
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// Cue body as WebVTT markup; a blank line would end the cue, so empty lines are removed.
fn body_text(text: &str) -> String {
    escape(&ass_to_html(text))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_time(ms: i64) -> String {
    let ms = ms.max(0);
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

/// Serialise cues as WebVTT text using `\n` line endings. Comment cues are skipped (`convert` also drops drawings).
pub(crate) fn write(cues: &[Cue], header: &str, trailing_newlines: usize) -> String {
    let blocks: Vec<String> = cues
        .iter()
        .filter_map(|cue| {
            if let Some(raw) = cue.unchanged_raw(Format::Vtt) {
                return Some(raw.to_owned());
            }
            if cue.comment {
                return None;
            }
            let (id, settings, voice) = match &cue.extra {
                CueExtra::Vtt {
                    id,
                    settings,
                    voice,
                } => (id.as_deref(), settings.as_str(), voice.as_deref()),
                _ => (None, "", None),
            };
            let mut block = String::new();
            if let Some(id) = id {
                block.push_str(id);
                block.push('\n');
            }
            block.push_str(&format!(
                "{} --> {}",
                format_time(cue.start_ms),
                format_time(cue.end_ms)
            ));
            if !settings.is_empty() {
                block.push(' ');
                block.push_str(settings);
            }
            let body = body_text(&cue.text);
            if !body.is_empty() {
                block.push('\n');
                match voice {
                    Some(v) => block.push_str(&format!("<{v}>{body}</v>")),
                    None => block.push_str(&body),
                }
            }
            Some(block)
        })
        .collect();
    let mut out = header.to_owned();
    if blocks.is_empty() {
        out.push('\n');
    } else {
        out.push_str("\n\n");
        out.push_str(&blocks.join("\n\n"));
        out.push_str(&"\n".repeat(trailing_newlines));
    }
    out
}
