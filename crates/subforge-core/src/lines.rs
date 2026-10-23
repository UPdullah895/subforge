//! Translation line pipeline: pull plain lines out of a subtitle file for machine
//! translation and rebuild the file from the translated lines (with Arabic RTL and
//! punctuation fixes). Port of the former `scripts/subtitle_lines.py`.

use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::subtitle::markup::has_drawing;
use crate::subtitle::{load, SubtitleError};

/// Longest line `reflow` aims for before it adds another line.
const MAX_LINE: usize = 42;
const MAX_PARTS: usize = 3;
const RLE: char = '\u{202b}';
const PDF: char = '\u{202c}';

/// One cue as sent to a translator. Serialized as JSON for the translation pipeline.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SubtitleLine {
    pub id: usize,
    pub start: i64,
    pub end: i64,
    pub text: String,
}

/// ASS vector drawings (`\p1` ...) are shapes, not dialogue.
pub fn is_drawing(text: &str) -> bool {
    has_drawing(text)
}

/// Remove every `{...}` block (an unterminated `{` is kept, as the regex did).
fn strip_blocks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        match rest[open..].find('}') {
            Some(close) => {
                out.push_str(&rest[..open]);
                rest = &rest[open + close + 1..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

/// Split on `\N`, `\n` and real newlines.
fn split_breaks(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut iter = text.char_indices().peekable();
    while let Some((i, c)) = iter.next() {
        if c == '\n' {
            parts.push(&text[start..i]);
            start = i + 1;
        } else if c == '\\' {
            if let Some(&(_, n)) = iter.peek() {
                if n == 'N' || n == 'n' {
                    parts.push(&text[start..i]);
                    iter.next();
                    start = i + 2;
                }
            }
        }
    }
    parts.push(&text[start..]);
    parts
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Visible text of a cue as one line: override tags removed, breaks joined with spaces.
/// Drawings give an empty string.
pub fn plain_text(text: &str) -> String {
    if is_drawing(text) {
        return String::new();
    }
    let text = strip_blocks(text).replace("\\h", " ");
    collapse_whitespace(&split_breaks(&text).join(" "))
}

/// One entry per cue in file order. Comment cues and drawings get empty text.
pub fn extract_lines(path: &Path) -> Result<Vec<SubtitleLine>, SubtitleError> {
    let doc = load(path)?;
    Ok(doc
        .cues
        .iter()
        .enumerate()
        .map(|(id, cue)| SubtitleLine {
            id,
            start: cue.start_ms,
            end: cue.end_ms,
            text: if cue.comment {
                String::new()
            } else {
                plain_text(&cue.text)
            },
        })
        .collect())
}

/// Arabic, Hebrew, Syriac, Thaana, N'Ko, Persian/Urdu extensions + presentation forms.
fn is_rtl_char(c: char) -> bool {
    matches!(c, '\u{0590}'..='\u{08FF}' | '\u{FB1D}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}')
}

/// Length in bytes of the leading run of `{...}` blocks.
fn leading_blocks_len(text: &str) -> usize {
    let mut end = 0;
    while text[end..].starts_with('{') {
        match text[end..].find('}') {
            Some(close) => end += close + 1,
            None => break,
        }
    }
    end
}

/// Force right-to-left layout for one physical line by wrapping it in RLE ... PDF,
/// keeping leading ASS tags such as `{\an8}` in front.
fn fix_rtl_line(line: &str) -> String {
    if !line.chars().any(is_rtl_char) || line.contains(RLE) {
        return line.to_string();
    }
    let (prefix, rest) = line.split_at(leading_blocks_len(line));
    format!("{prefix}{RLE}{rest}{PDF}")
}

pub fn fix_rtl_text(text: &str) -> String {
    text.split('\n')
        .map(fix_rtl_line)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Unicode decimal digit (category Nd), the same set as Python's `\d`.
fn is_decimal_digit(c: char) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d$").unwrap())
        .is_match(c.encode_utf8(&mut [0; 4]))
}

/// Use Arabic `؟ ، ؛` instead of the Latin marks models often leave in Arabic lines.
/// Commas between two digits (1,000) are left alone.
pub fn arabic_punctuation(text: &str) -> String {
    if !text.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c)) {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '?' => out.push('\u{061f}'),
            ';' => out.push('\u{061b}'),
            ',' => {
                let after_digit = i > 0 && is_decimal_digit(chars[i - 1]);
                let before_digit = chars.get(i + 1).is_some_and(|n| is_decimal_digit(*n));
                // (?<!\d),|,(?!\d): replaced unless digits sit on both sides.
                if after_digit && before_digit {
                    out.push(',');
                } else {
                    out.push('\u{060c}');
                }
            }
            _ => out.push(c),
        }
    }
    out
}

fn italic_tag() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\\i[01]").unwrap())
}

/// True when every visible character of the cue is inside `{\i1}` ... `{\i0}`.
fn is_all_italic(text: &str) -> bool {
    let (mut italic, mut seen) = (false, false);
    let mut rest = text;
    while !rest.is_empty() {
        if rest.starts_with('{') {
            if let Some(close) = rest.find('}') {
                for m in italic_tag().find_iter(&rest[1..close]) {
                    italic = m.as_str() == "\\i1";
                }
                rest = &rest[close + 1..];
            } else {
                rest = &rest[1..];
            }
            continue;
        }
        let end = rest.find('{').unwrap_or(rest.len());
        let run = rest[..end].replace("\\N", "").replace("\\n", "");
        if !run.trim().is_empty() {
            seen = true;
            if !italic {
                return false;
            }
        }
        rest = &rest[end..];
    }
    seen
}

/// Leading override blocks such as `{\an8}` or `{\pos(..)}`, minus italic toggles.
fn leading_tags(text: &str) -> String {
    let prefix = &text[..leading_blocks_len(text)];
    let mut out = String::new();
    let mut rest = prefix;
    while let Some(close) = rest.find('}') {
        let block = italic_tag().replace_all(&rest[..=close], "");
        if block != "{}" {
            out.push_str(&block);
        }
        rest = &rest[close + 1..];
    }
    out
}

fn split_balanced(text: &str, parts: usize) -> Vec<String> {
    let words: Vec<&str> = text.split(' ').collect();
    if parts < 2 || words.len() < parts {
        return vec![text.to_string()];
    }
    let total = text.chars().count() as f64;
    let (mut out, mut cur): (Vec<String>, Vec<&str>) = (Vec::new(), Vec::new());
    for w in words {
        cur.push(w);
        let joined = cur.join(" ");
        if out.len() < parts - 1 && joined.chars().count() as f64 >= total / parts as f64 {
            out.push(joined);
            cur.clear();
        }
    }
    if !cur.is_empty() {
        out.push(cur.join(" "));
    }
    out
}

fn is_dash(c: char) -> bool {
    matches!(c, '-' | '\u{2013}' | '\u{2014}')
}

/// `^\s*[-–—]\s*` on an already trimmed line.
fn starts_with_dash(line: &str) -> bool {
    line.trim_start().chars().next().is_some_and(is_dash)
}

/// Split before each dash that starts a new speaker turn: the regex
/// `\s+(?=[-–—]\s?\S)` as a manual scan. Pieces are trimmed, empties dropped.
fn split_dash_turns(text: &str) -> Vec<String> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut pieces = Vec::new();
    let mut piece_start = 0;
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].1.is_whitespace() {
            i += 1;
            continue;
        }
        let run_start = i;
        while i < chars.len() && chars[i].1.is_whitespace() {
            i += 1;
        }
        // Whitespace run is [run_start, i); a turn starts when a dash follows it.
        let Some(&(_, dash)) = chars.get(i) else {
            break;
        };
        if !is_dash(dash) {
            continue;
        }
        let next = chars.get(i + 1).map(|&(_, c)| c);
        let after = match next {
            Some(c) if c.is_whitespace() => chars.get(i + 2).map(|&(_, c)| c),
            other => other,
        };
        if after.is_some_and(|c| !c.is_whitespace()) {
            pieces.push(&text[piece_start..chars[run_start].0]);
            piece_start = chars[i].0;
        }
    }
    pieces.push(&text[piece_start..]);
    pieces
        .into_iter()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

/// Put a translated cue back into the original cue's shape: keeps leading positioning
/// tags, whole-cue italics, dash-led dialogue and the number of lines, and wraps overly
/// long translations.
pub fn reflow(original: &str, translated: &str) -> String {
    let translated = collapse_whitespace(&translated.replace("\\N", " ").replace('\n', " "));
    let visible: Vec<String> = split_breaks(original)
        .iter()
        .map(|p| strip_blocks(p).trim().to_string())
        .filter(|v| !v.is_empty())
        .collect();
    let count = visible.len().max(1);

    let mut parts: Option<Vec<String>> = None;
    if count > 1 && visible.iter().all(|v| starts_with_dash(v)) {
        let pieces = split_dash_turns(&translated);
        if pieces.len() == count {
            parts = Some(pieces);
        }
    }
    let mut parts = parts.unwrap_or_else(|| {
        let len = translated.chars().count();
        let wanted = if len > MAX_LINE {
            count.max(len.div_ceil(MAX_LINE))
        } else {
            count.max(1)
        };
        split_balanced(&translated, wanted.min(MAX_PARTS))
    });

    if is_all_italic(original) {
        parts = parts
            .into_iter()
            .map(|p| format!("{{\\i1}}{p}{{\\i0}}"))
            .collect();
    }
    format!("{}{}", leading_tags(original), parts.join("\n"))
}

/// Rebuild `original` with `translations` (by cue index) into `output`, in the original's
/// format. Cues without a translation, with a blank one, drawings and comment cues keep
/// their original text; everything else (styles, fonts, timings) is preserved.
pub fn rebuild_lines(
    original: &Path,
    translations: &[String],
    output: &Path,
    rtl_fix: bool,
) -> Result<(), SubtitleError> {
    let mut doc = load(original)?;
    for (cue, translated) in doc.cues.iter_mut().zip(translations) {
        if cue.comment || is_drawing(&cue.text) || translated.trim().is_empty() {
            continue;
        }
        let mut text = reflow(&cue.text, translated);
        if rtl_fix {
            text = fix_rtl_text(&arabic_punctuation(&text));
        }
        cue.text = text.replace('\n', "\\N");
    }
    doc.save(output)
}
