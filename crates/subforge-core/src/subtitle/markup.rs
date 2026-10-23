//! Conversion between SRT/HTML-style markup and the internal ASS-style cue text.
//!
//! Internal cue text always uses ASS conventions: `\N` is a hard line break and `{...}` blocks
//! carry override tags.

/// Convert SRT/HTML markup to ASS conventions.
pub(crate) fn html_to_ass(s: &str) -> String {
    let norm = s.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(norm.len());
    let mut rest = norm.as_str();
    while let Some(p) = rest.find('<') {
        out.push_str(&rest[..p]);
        let after = &rest[p..];
        match after.find('>') {
            Some(e) => {
                match convert_tag(&after[1..e]) {
                    Some(rep) => out.push_str(&rep),
                    None => out.push_str(&after[..=e]),
                }
                rest = &after[e + 1..];
            }
            None => {
                out.push_str(after);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.replace('\n', "\\N")
}

/// Map one HTML tag body (without `<` `>`) to ASS, or `None` if it should be kept literally.
fn convert_tag(inner: &str) -> Option<String> {
    let t = inner.trim().to_ascii_lowercase();
    match t.as_str() {
        "i" | "b" | "u" | "s" => return Some(format!("{{\\{t}1}}")),
        "/i" | "/b" | "/u" | "/s" => return Some(format!("{{\\{}0}}", &t[1..])),
        "/font" => return Some("{\\c}".to_string()),
        "br" | "br/" | "br /" => return Some("\\N".to_string()),
        _ => {}
    }
    if t == "font" || t.starts_with("font ") {
        return Some(font_color(&t).unwrap_or_default());
    }
    None
}

/// `color="#RRGGBB"` / `color=#RRGGBB` -> `{\c&HBBGGRR&}`.
fn font_color(tag: &str) -> Option<String> {
    let idx = tag.find("color")?;
    let after = tag[idx + 5..].trim_start().strip_prefix('=')?.trim_start();
    let after = after.trim_start_matches(['"', '\'']);
    let hex = after.strip_prefix('#')?;
    let hex = hex.get(..6)?;
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let bgr = format!("{}{}{}", &hex[4..6], &hex[2..4], &hex[0..2]).to_uppercase();
    Some(format!("{{\\c&H{bgr}&}}"))
}

/// Convert ASS-style cue text to HTML-style markup suitable for SRT.
///
/// Keeps italic/bold/underline/strike toggles, drops every other override block, converts
/// `\N`/`\n` to a newline and `\h` to a space. Vector drawings produce an empty string.
pub(crate) fn ass_to_html(s: &str) -> String {
    if has_drawing(s) {
        return String::new();
    }
    let mut out = String::with_capacity(s.len());
    // Open tags, in opening order, so they can be closed in reverse at the end.
    let mut open: Vec<char> = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        match rest.find('{') {
            Some(p) => {
                push_text(&mut out, &rest[..p]);
                let after = &rest[p + 1..];
                match after.find('}') {
                    Some(e) => {
                        apply_block(&after[..e], &mut out, &mut open);
                        rest = &after[e + 1..];
                    }
                    None => {
                        push_text(&mut out, &rest[p..]);
                        rest = "";
                    }
                }
            }
            None => {
                push_text(&mut out, rest);
                rest = "";
            }
        }
    }
    while let Some(c) = open.pop() {
        out.push_str(&format!("</{c}>"));
    }
    out
}

fn push_text(out: &mut String, text: &str) {
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('N') | Some('n') => {
                    chars.next();
                    out.push('\n');
                    continue;
                }
                Some('h') => {
                    chars.next();
                    out.push(' ');
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
    }
}

fn apply_block(block: &str, out: &mut String, open: &mut Vec<char>) {
    for tag in block.split('\\').skip(1) {
        let mut chars = tag.chars();
        let Some(name) = chars.next() else { continue };
        if !matches!(name, 'i' | 'b' | 'u' | 's') {
            continue;
        }
        let value = chars.as_str().trim();
        if !value.chars().all(|c| c.is_ascii_digit()) {
            continue; // e.g. \shad, \bord, \blur
        }
        let on = !value.is_empty() && value != "0";
        let is_open = open.contains(&name);
        if on && !is_open {
            open.push(name);
            out.push_str(&format!("<{name}>"));
        } else if !on && is_open {
            open.retain(|c| *c != name);
            out.push_str(&format!("</{name}>"));
        }
    }
}

pub(crate) fn has_drawing(s: &str) -> bool {
    let mut rest = s;
    while let Some(p) = rest.find('{') {
        let after = &rest[p + 1..];
        let Some(e) = after.find('}') else { break };
        for tag in after[..e].split('\\').skip(1) {
            if let Some(v) = tag.strip_prefix('p') {
                if v.chars().all(|c| c.is_ascii_digit()) && !v.trim_start_matches('0').is_empty() {
                    return true;
                }
            }
        }
        rest = &after[e + 1..];
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_basic_toggles_and_newline() {
        assert_eq!(
            html_to_ass("<i>Line one</i>\nLine two"),
            "{\\i1}Line one{\\i0}\\NLine two"
        );
        assert_eq!(
            html_to_ass("<B>x</B><u>y</u><s>z</s>"),
            "{\\b1}x{\\b0}{\\u1}y{\\u0}{\\s1}z{\\s0}"
        );
        assert_eq!(html_to_ass("a\r\nb"), "a\\Nb");
    }

    #[test]
    fn html_font_color_becomes_bgr() {
        assert_eq!(
            html_to_ass("<font color=\"#FF8000\">hot</font>"),
            "{\\c&H0080FF&}hot{\\c}"
        );
        assert_eq!(
            html_to_ass("<font color=#00ff10>x</font>"),
            "{\\c&H10FF00&}x{\\c}"
        );
    }

    #[test]
    fn html_font_without_color_is_stripped_and_unknown_tags_kept() {
        assert_eq!(html_to_ass("<font face=\"Arial\">x</font>"), "x{\\c}");
        assert_eq!(html_to_ass("a < b and <c>"), "a < b and <c>");
        assert_eq!(html_to_ass("dangling <i"), "dangling <i");
    }

    #[test]
    fn ass_toggles_and_breaks() {
        assert_eq!(
            ass_to_html("{\\i1}Line one{\\i0}\\NLine two"),
            "<i>Line one</i>\nLine two"
        );
        assert_eq!(ass_to_html("a\\nb\\hc"), "a\nb c");
    }

    #[test]
    fn ass_strips_other_overrides_and_closes_open_tags() {
        assert_eq!(
            ass_to_html("{\\an8\\pos(10,20)\\b1\\fs30\\shad2\\bord1}Top{\\c&H0000FF&}"),
            "<b>Top</b>"
        );
        assert_eq!(ass_to_html("{\\i1}never closed"), "<i>never closed</i>");
    }

    #[test]
    fn ass_drawing_is_empty() {
        assert_eq!(ass_to_html("{\\p1}m 0 0 l 10 10{\\p0}"), "");
        assert_eq!(ass_to_html("{\\p0}text"), "text");
    }

    #[test]
    fn html_ass_html_roundtrip() {
        let src = "<i>a</i>\n<b>b</b>";
        assert_eq!(ass_to_html(&html_to_ass(src)), src);
    }
}
