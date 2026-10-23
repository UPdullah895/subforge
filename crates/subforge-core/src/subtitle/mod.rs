//! Subtitle model and formats: SRT, ASS/SSA and WebVTT, plus conversion, shift and stretch.

mod ass;
pub mod encoding;
pub(crate) mod markup;
mod srt;
mod vtt;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

pub use encoding::DetectedEncoding;

/// A supported subtitle format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Srt,
    Ass,
    Ssa,
    Vtt,
}

impl Format {
    /// Case-insensitive; a leading dot is accepted.
    pub fn from_extension(ext: &str) -> Option<Format> {
        match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
            "srt" => Some(Format::Srt),
            "ass" => Some(Format::Ass),
            "ssa" => Some(Format::Ssa),
            "vtt" => Some(Format::Vtt),
            _ => None,
        }
    }

    pub fn from_path(path: &Path) -> Option<Format> {
        Format::from_extension(path.extension()?.to_str()?)
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Srt => "srt",
            Format::Ass => "ass",
            Format::Ssa => "ssa",
            Format::Vtt => "vtt",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SubtitleError {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{}", fmt_parse(path.as_deref(), *line, msg))]
    Parse {
        path: Option<PathBuf>,
        line: usize,
        msg: String,
    },
    #[error("invalid frame rate: it must be a positive number")]
    InvalidFps,
    #[error("unsupported subtitle format: {0}")]
    UnsupportedFormat(String),
}

fn fmt_parse(path: Option<&Path>, line: usize, msg: &str) -> String {
    match path {
        Some(p) => format!("{}: line {line}: {msg}", p.display()),
        None => format!("line {line}: {msg}"),
    }
}

/// One subtitle cue. `text` always uses ASS conventions: `\N` is a hard line break and
/// `{...}` blocks hold override tags, whatever format the cue was read from.
#[derive(Debug, Clone)]
pub struct Cue {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    /// ASS `Comment:` events; never rendered, never translated.
    pub comment: bool,
    /// Original source block and the values it was parsed to. While the public fields still
    /// equal the originals the writer emits the block verbatim.
    original: Option<Original>,
    /// Format-specific data (filled in by the ASS and WebVTT parsers).
    extra: CueExtra,
}

#[derive(Debug, Clone)]
struct Original {
    /// The format `raw` is written in; only that format's writer may reuse it.
    format: Format,
    raw: String,
    start_ms: i64,
    end_ms: i64,
    text: String,
    comment: bool,
}

#[derive(Debug, Clone, Default)]
enum CueExtra {
    #[default]
    None,
    /// Every field of the event line in `Format:` order (Start/End/Text are rebuilt on write).
    Ass { fields: Vec<String> },
    /// WebVTT identifier line and the cue settings after the end time.
    Vtt {
        id: Option<String>,
        settings: String,
        /// Opening tag text of a leading voice span, such as `v Mary`.
        voice: Option<String>,
    },
}

impl Cue {
    pub fn new(start_ms: i64, end_ms: i64, text: impl Into<String>) -> Cue {
        Cue {
            start_ms,
            end_ms,
            text: text.into(),
            comment: false,
            original: None,
            extra: CueExtra::None,
        }
    }

    /// Remember the source block so an unedited cue is written back byte-for-byte.
    pub(crate) fn keep_original(&mut self, raw: String, format: Format) {
        self.original = Some(Original {
            format,
            raw,
            start_ms: self.start_ms,
            end_ms: self.end_ms,
            text: self.text.clone(),
            comment: self.comment,
        });
    }

    /// The source block, if the cue has not been edited since parsing and `format` is the
    /// format it was parsed from.
    pub(crate) fn unchanged_raw(&self, format: Format) -> Option<&str> {
        let o = self.original.as_ref()?;
        (o.format == format
            && o.start_ms == self.start_ms
            && o.end_ms == self.end_ms
            && o.text == self.text
            && o.comment == self.comment)
            .then_some(o.raw.as_str())
    }
}

/// Format-specific document data needed to write the file back losslessly.
#[derive(Debug, Clone)]
pub(crate) enum FormatExtras {
    Srt {
        trailing_newlines: usize,
    },
    Ass(ass::AssLayout),
    Vtt {
        /// The `WEBVTT` header block (usually just `WEBVTT`).
        header: String,
        trailing_newlines: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineEnding {
    Lf,
    CrLf,
}

impl LineEnding {
    /// Majority wins; ties go to LF.
    fn detect(text: &str) -> LineEnding {
        let crlf = text.matches("\r\n").count();
        let lf = text.matches('\n').count() - crlf;
        if crlf > lf {
            LineEnding::CrLf
        } else {
            LineEnding::Lf
        }
    }
}

/// A parsed subtitle file.
#[derive(Debug, Clone)]
pub struct SubtitleDoc {
    pub format: Format,
    pub encoding: DetectedEncoding,
    pub cues: Vec<Cue>,
    line_ending: LineEnding,
    extras: FormatExtras,
}

/// Decide the format from the decoded text. Content signatures win; `hint` only settles files
/// that carry no signature of their own.
fn detect_format(text: &str, hint: Option<Format>) -> Format {
    let head = text.trim_start();
    if let Some(rest) = head.strip_prefix("WEBVTT") {
        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            return Format::Vtt;
        }
    }
    if head.starts_with("[Script Info]") {
        let lower = text.to_ascii_lowercase();
        if lower.contains("scripttype: v4.00+") {
            return Format::Ass;
        }
        if lower.contains("scripttype: v4.00") {
            return Format::Ssa;
        }
        if lower.contains("[v4+ styles]") {
            return Format::Ass;
        }
        if lower.contains("[v4 styles]") {
            return Format::Ssa;
        }
        return match hint {
            Some(f @ (Format::Ass | Format::Ssa)) => f,
            _ => Format::Ass,
        };
    }
    match hint {
        // A signature-less file named .ass/.ssa/.vtt: trust the caller.
        Some(f @ (Format::Ass | Format::Ssa | Format::Vtt)) if looks_like_sections(head, f) => f,
        _ => Format::Srt,
    }
}

/// A signature-less body that fits the hinted format: ASS/SSA by its `[Events]` section, WebVTT
/// by a cue timing line with a dot fraction (comma fractions are SRT).
fn looks_like_sections(head: &str, f: Format) -> bool {
    static SRT_TIMING: OnceLock<regex::Regex> = OnceLock::new();
    match f {
        Format::Ass | Format::Ssa => head.contains("[Events]"),
        Format::Vtt => {
            let srt = SRT_TIMING
                .get_or_init(|| regex::Regex::new(r"\d,\d{1,3}\s*-->").expect("static regex"));
            head.contains("-->") && !srt.is_match(head)
        }
        Format::Srt => false,
    }
}

impl SubtitleDoc {
    /// A new document in `format` holding `cues`: UTF-8 without BOM, LF line endings and the
    /// format's default header.
    pub fn new(format: Format, cues: Vec<Cue>) -> SubtitleDoc {
        let extras = match format {
            Format::Vtt => FormatExtras::Vtt {
                header: "WEBVTT".to_owned(),
                trailing_newlines: 1,
            },
            // ASS/SSA: non-ASS extras make the writer build the default header.
            _ => FormatExtras::Srt {
                trailing_newlines: 1,
            },
        };
        SubtitleDoc {
            format,
            encoding: DetectedEncoding {
                name: encoding_rs::UTF_8.name(),
                had_bom: false,
            },
            cues,
            line_ending: LineEnding::Lf,
            extras,
        }
    }

    /// Decode and parse `bytes`. The format is detected from the content; `hint` (usually from
    /// the file extension) only settles ambiguous content.
    pub fn parse(bytes: &[u8], hint: Option<Format>) -> Result<SubtitleDoc, SubtitleError> {
        let (text, encoding) = encoding::decode(bytes);
        let format = detect_format(&text, hint);
        let line_ending = LineEnding::detect(&text);
        let (cues, extras) = match format {
            Format::Srt => srt::parse(&text)?,
            Format::Ass | Format::Ssa => ass::parse(&text, format == Format::Ssa)?,
            Format::Vtt => vtt::parse(&text)?,
        };
        Ok(SubtitleDoc {
            format,
            encoding,
            cues,
            line_ending,
            extras,
        })
    }

    pub fn load(path: &Path) -> Result<SubtitleDoc, SubtitleError> {
        let bytes = std::fs::read(path).map_err(|source| SubtitleError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        SubtitleDoc::parse(&bytes, Format::from_path(path)).map_err(|e| match e {
            SubtitleError::Parse {
                path: None,
                line,
                msg,
            } => SubtitleError::Parse {
                path: Some(path.to_path_buf()),
                line,
                msg,
            },
            other => other,
        })
    }

    /// Serialise as UTF-8 in `self.format` (with a BOM only if the source was UTF-8 with a BOM).
    ///
    /// The format-specific extras are used only when they belong to `self.format`; otherwise
    /// the document is written with defaults for that format.
    pub fn to_bytes(&self) -> Vec<u8> {
        let text = match self.format {
            Format::Srt => {
                let trailing = match &self.extras {
                    FormatExtras::Srt { trailing_newlines } => *trailing_newlines,
                    _ => 1,
                };
                srt::write(&self.cues, trailing)
            }
            Format::Ass | Format::Ssa => {
                let layout = match &self.extras {
                    FormatExtras::Ass(layout) => Some(layout),
                    _ => None,
                };
                ass::write(&self.cues, layout, self.format == Format::Ssa)
            }
            Format::Vtt => {
                let (header, trailing) = match &self.extras {
                    FormatExtras::Vtt {
                        header,
                        trailing_newlines,
                    } => (header.as_str(), *trailing_newlines),
                    _ => ("WEBVTT", 1),
                };
                vtt::write(&self.cues, header, trailing)
            }
        };
        let text = match self.line_ending {
            LineEnding::Lf => text,
            LineEnding::CrLf => text.replace('\n', "\r\n"),
        };
        let mut out = Vec::with_capacity(text.len() + 3);
        if self.encoding.had_bom && self.encoding.name == encoding_rs::UTF_8.name() {
            out.extend_from_slice(b"\xEF\xBB\xBF");
        }
        out.extend_from_slice(text.as_bytes());
        out
    }

    /// A copy of the document in `to`. Converting to SRT or WebVTT drops comment and drawing
    /// cues (the writers turn the text into HTML-style markup); converting from SRT/WebVTT to
    /// ASS/SSA produces a default header and a single `Default` style; ASS <-> SSA keeps the
    /// styles and translates the format lines.
    pub fn convert(&self, to: Format) -> SubtitleDoc {
        let mut doc = self.clone();
        if self.format == to {
            return doc;
        }
        let is_ass = |f: Format| matches!(f, Format::Ass | Format::Ssa);
        doc.format = to;
        if matches!(to, Format::Srt | Format::Vtt) {
            doc.cues
                .retain(|c| !c.comment && !markup::has_drawing(&c.text));
        }
        if !(is_ass(self.format) && is_ass(to)) {
            // Format-specific data does not carry over. For ASS/SSA targets any non-ASS
            // extras make the writer build the default header.
            doc.extras = match to {
                Format::Vtt => FormatExtras::Vtt {
                    header: "WEBVTT".to_owned(),
                    trailing_newlines: 1,
                },
                _ => FormatExtras::Srt {
                    trailing_newlines: 1,
                },
            };
        }
        doc
    }

    /// Move every cue by `ms` (negative moves earlier). Times are clamped at 0 and a cue never
    /// ends before it starts.
    pub fn shift(&mut self, ms: i64) {
        for cue in &mut self.cues {
            cue.start_ms = cue.start_ms.saturating_add(ms).max(0);
            cue.end_ms = cue.end_ms.saturating_add(ms).max(cue.start_ms);
        }
    }

    /// Multiply every time by `from_fps / to_fps` (rounded to the millisecond), e.g. to retime
    /// subtitles made for 25 fps video onto a 23.976 fps cut.
    pub fn stretch(&mut self, from_fps: f64, to_fps: f64) -> Result<(), SubtitleError> {
        let valid = |f: f64| f.is_finite() && f > 0.0;
        if !valid(from_fps) || !valid(to_fps) {
            return Err(SubtitleError::InvalidFps);
        }
        let scale = |t: i64| (t as f64 * from_fps / to_fps).round() as i64;
        for cue in &mut self.cues {
            cue.start_ms = scale(cue.start_ms).max(0);
            cue.end_ms = scale(cue.end_ms).max(cue.start_ms);
        }
        Ok(())
    }

    pub fn save(&self, path: &Path) -> Result<(), SubtitleError> {
        std::fs::write(path, self.to_bytes()).map_err(|source| SubtitleError::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}

/// Free-function form of [`SubtitleDoc::parse`].
pub fn parse(bytes: &[u8], hint: Option<Format>) -> Result<SubtitleDoc, SubtitleError> {
    SubtitleDoc::parse(bytes, hint)
}

/// Free-function form of [`SubtitleDoc::load`].
pub fn load(path: &Path) -> Result<SubtitleDoc, SubtitleError> {
    SubtitleDoc::load(path)
}

/// Load `input`, convert it to `to` and save it as UTF-8 at `output`.
pub fn convert_file(input: &Path, output: &Path, to: Format) -> Result<(), SubtitleError> {
    SubtitleDoc::load(input)?.convert(to).save(output)
}

/// Load `input`, shift every cue by `ms` and save it (same format, UTF-8) at `output`.
pub fn shift_file(input: &Path, output: &Path, ms: i64) -> Result<(), SubtitleError> {
    let mut doc = SubtitleDoc::load(input)?;
    doc.shift(ms);
    doc.save(output)
}

/// Load `input`, stretch its times from `from_fps` to `to_fps` and save it at `output`.
pub fn stretch_file(
    input: &Path,
    output: &Path,
    from_fps: f64,
    to_fps: f64,
) -> Result<(), SubtitleError> {
    let mut doc = SubtitleDoc::load(input)?;
    doc.stretch(from_fps, to_fps)?;
    doc.save(output)
}
