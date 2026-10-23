//! Advanced SubStation Alpha (`.ass`) and SubStation Alpha (`.ssa`) parsing and writing.
//!
//! The file is kept as an ordered list of lines. Everything that is not an event line
//! (script info, styles, `[Fonts]`, `[Graphics]`, unknown sections, comments, blank lines)
//! is stored verbatim; each `Dialogue:`/`Comment:` line inside `[Events]` becomes a cue and a
//! placeholder in that list (raw lines are tagged `Format::Ass` for both ASS and SSA). An untouched cue writes back its original line; an edited one is
//! rebuilt from the `Format:` field order, keeping every field except Start/End/Text verbatim.

use super::{Cue, CueExtra, Format, FormatExtras, SubtitleError};

const ASS_FORMAT: [&str; 10] = [
    "layer", "start", "end", "style", "name", "marginl", "marginr", "marginv", "effect", "text",
];
const SSA_FORMAT: [&str; 10] = [
    "marked", "start", "end", "style", "name", "marginl", "marginr", "marginv", "effect", "text",
];

#[derive(Debug, Clone)]
enum Line {
    Raw(String),
    /// Position of the next cue.
    Event,
}

/// Everything about an ASS/SSA file that is not a cue.
#[derive(Debug, Clone)]
pub(crate) struct AssLayout {
    lines: Vec<Line>,
    /// Lower-case, trimmed field names from the `[Events]` `Format:` line.
    format: Vec<String>,
    /// Index in `lines` after which cues beyond the original slots are inserted.
    events_end: Option<usize>,
}

fn default_format(ssa: bool) -> Vec<String> {
    let names: &[&str] = if ssa { &SSA_FORMAT } else { &ASS_FORMAT };
    names.iter().map(|s| (*s).to_owned()).collect()
}

fn strip_prefix_ci<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    let head = line.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &line[prefix.len()..])
}

fn parse_err(line: usize, msg: impl Into<String>) -> SubtitleError {
    SubtitleError::Parse {
        path: None,
        line,
        msg: msg.into(),
    }
}

/// `H:MM:SS.cc` (the fraction is a decimal fraction of a second) to milliseconds.
fn parse_time(s: &str) -> Option<i64> {
    let s = s.trim();
    let mut parts = s.split(':');
    let (h, m, rest) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let (sec, frac) = rest.split_once('.').unwrap_or((rest, ""));
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
    if !(digits(h) && digits(m) && digits(sec)) || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let frac_ms: i64 = format!("{:0<3}", frac).get(..3)?.parse().ok()?;
    let total =
        (h.parse::<i64>().ok()? * 60 + m.parse::<i64>().ok()?) * 60 + sec.parse::<i64>().ok()?;
    Some(total * 1000 + frac_ms)
}

/// Milliseconds to `H:MM:SS.cc`, rounding half up to centiseconds.
fn format_time(ms: i64) -> String {
    let cs = (ms.max(0) + 5) / 10;
    format!(
        "{}:{:02}:{:02}.{:02}",
        cs / 360_000,
        cs / 6000 % 60,
        cs / 100 % 60,
        cs % 100
    )
}

/// Parse decoded ASS/SSA text (any line ending) into cues plus the layout needed to write it back.
pub(crate) fn parse(text: &str, ssa: bool) -> Result<(Vec<Cue>, FormatExtras), SubtitleError> {
    let norm = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<Line> = Vec::new();
    let mut cues = Vec::new();
    let mut format: Option<Vec<String>> = None;
    let mut events_end = None;
    let mut in_events = false;

    for (n, line) in norm.split('\n').enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_events = trimmed.eq_ignore_ascii_case("[events]");
        }
        if !in_events {
            lines.push(Line::Raw(line.to_owned()));
            continue;
        }
        if !trimmed.is_empty() {
            events_end = Some(lines.len() + 1);
        }
        if format.is_none() {
            if let Some(rest) = strip_prefix_ci(line, "format:") {
                let names: Vec<String> = rest
                    .split(',')
                    .map(|s| s.trim().to_ascii_lowercase())
                    .collect();
                for needed in ["start", "end", "text"] {
                    if !names.iter().any(|f| f == needed) {
                        return Err(parse_err(
                            n + 1,
                            format!("the [Events] Format line has no {needed} field"),
                        ));
                    }
                }
                if names.last().map(String::as_str) != Some("text") {
                    return Err(parse_err(
                        n + 1,
                        "the Text field must be last in the [Events] Format line",
                    ));
                }
                format = Some(names);
                lines.push(Line::Raw(line.to_owned()));
                continue;
            }
        }
        let event = strip_prefix_ci(line, "dialogue:")
            .map(|r| (false, r))
            .or_else(|| strip_prefix_ci(line, "comment:").map(|r| (true, r)));
        let Some((comment, rest)) = event else {
            lines.push(Line::Raw(line.to_owned()));
            continue;
        };
        let names = format.get_or_insert_with(|| default_format(ssa));
        let fields: Vec<String> = rest
            .trim_start()
            .splitn(names.len(), ',')
            .map(str::to_owned)
            .collect();
        if fields.len() < names.len() {
            return Err(parse_err(
                n + 1,
                format!(
                    "expected {} comma-separated fields, found {}",
                    names.len(),
                    fields.len()
                ),
            ));
        }
        let field = |name: &str| {
            let i = names.iter().position(|f| f == name).expect("validated");
            &fields[i]
        };
        let time = |name: &str| {
            parse_time(field(name)).ok_or_else(|| {
                parse_err(
                    n + 1,
                    format!(
                        "invalid {name} time \"{}\", expected H:MM:SS.cc",
                        field(name).trim()
                    ),
                )
            })
        };
        let mut cue = Cue::new(time("start")?, time("end")?, field("text").clone());
        cue.comment = comment;
        cue.keep_original(line.to_owned(), Format::Ass);
        cue.extra = CueExtra::Ass { fields };
        cues.push(cue);
        lines.push(Line::Event);
    }

    let layout = AssLayout {
        lines,
        format: format.unwrap_or_else(|| default_format(ssa)),
        events_end,
    };
    Ok((cues, FormatExtras::Ass(layout)))
}

fn field_default(name: &str) -> &'static str {
    match name {
        "layer" => "0",
        "marked" => "Marked=0",
        "style" => "Default",
        "marginl" | "marginr" | "marginv" => "0",
        _ => "",
    }
}

fn cue_line(cue: &Cue, format: &[String]) -> String {
    if let Some(raw) = cue.unchanged_raw(Format::Ass) {
        return raw.to_owned();
    }
    let previous = match &cue.extra {
        CueExtra::Ass { fields } if fields.len() == format.len() => Some(fields),
        _ => None,
    };
    let values: Vec<String> = format
        .iter()
        .enumerate()
        .map(|(i, name)| match name.as_str() {
            "start" => format_time(cue.start_ms),
            "end" => format_time(cue.end_ms),
            "text" => cue.text.replace("\r\n", "\\N").replace(['\n', '\r'], "\\N"),
            other => previous
                .map(|f| f[i].clone())
                .unwrap_or_else(|| field_default(other).to_owned()),
        })
        .collect();
    let kind = if cue.comment { "Comment" } else { "Dialogue" };
    format!("{kind}: {}", values.join(","))
}

fn format_line(format: &[String]) -> String {
    let names: Vec<String> = format
        .iter()
        .map(|n| match n.as_str() {
            "marginl" => "MarginL".to_owned(),
            "marginr" => "MarginR".to_owned(),
            "marginv" => "MarginV".to_owned(),
            other => {
                let mut c = other.chars();
                c.next()
                    .map(|f| f.to_uppercase().chain(c).collect())
                    .unwrap_or_default()
            }
        })
        .collect();
    format!("Format: {}", names.join(", "))
}

/// Header and `[Events]` Format line for a document that was not read from an ASS/SSA file.
fn default_prelude(ssa: bool) -> Vec<String> {
    let (script_type, styles_header, styles_format, style) = if ssa {
        (
            "v4.00",
            "[V4 Styles]",
            "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, TertiaryColour, BackColour, Bold, Italic, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, AlphaLevel, Encoding",
            "Style: Default,Arial,20,16777215,65535,65535,0,0,0,1,2,2,2,30,30,30,0,0",
        )
    } else {
        (
            "v4.00+",
            "[V4+ Styles]",
            "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding",
            "Style: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1",
        )
    };
    vec![
        "[Script Info]".into(),
        format!("ScriptType: {script_type}"),
        String::new(),
        styles_header.into(),
        styles_format.into(),
        style.into(),
        String::new(),
    ]
}

/// Serialise cues as ASS/SSA text using `\n` line endings. `layout` is `None` when the document
/// did not come from an ASS/SSA file.
pub(crate) fn write(cues: &[Cue], layout: Option<&AssLayout>, ssa: bool) -> String {
    let default_layout;
    let layout = match layout {
        Some(l) => l,
        None => {
            let mut lines: Vec<Line> = default_prelude(ssa).into_iter().map(Line::Raw).collect();
            lines.push(Line::Raw("[Events]".into()));
            let format = default_format(ssa);
            lines.push(Line::Raw(format_line(&format)));
            let events_end = Some(lines.len());
            lines.push(Line::Raw(String::new()));
            default_layout = AssLayout {
                lines,
                format,
                events_end,
            };
            &default_layout
        }
    };

    let mut out: Vec<String> = Vec::with_capacity(layout.lines.len() + cues.len());
    let mut remaining = cues.iter();
    for (i, line) in layout.lines.iter().enumerate() {
        match line {
            Line::Raw(s) => out.push(s.clone()),
            Line::Event => {
                if let Some(cue) = remaining.next() {
                    out.push(cue_line(cue, &layout.format));
                }
            }
        }
        if layout.events_end == Some(i + 1) {
            out.extend(remaining.by_ref().map(|c| cue_line(c, &layout.format)));
        }
    }
    if layout.events_end.is_none() && remaining.len() > 0 {
        // The file had no [Events] section: add one before the final newline.
        let had_final_newline = out.last().is_some_and(String::is_empty);
        if had_final_newline {
            out.pop();
        }
        if out.last().is_some_and(|l| !l.is_empty()) {
            out.push(String::new());
        }
        out.push("[Events]".into());
        out.push(format_line(&layout.format));
        out.extend(remaining.map(|c| cue_line(c, &layout.format)));
        if had_final_newline {
            out.push(String::new());
        }
    }
    out.join("\n")
}
