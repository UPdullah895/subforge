//! Does this subtitle belong to this video, and is it in sync?
//!
//! Two levels of checking:
//!
//! * [`check`] is instant and offline: it compares the subtitle's span with the
//!   real video duration and looks for the classic 25↔23.976 (PAL/NTSC) drift.
//! * [`deep_check`] transcribes three short windows of the actual audio with
//!   whisper and matches them against nearby cues, which catches a constant
//!   delay and a wrong episode. It needs a downloaded speech model.
//!
//! The matching/decision part ([`align`]) is pure, so it is tested without a
//! model or an audio file.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::SubtitleDoc;
use crate::progress::{CancellationToken, ProgressFn};
use crate::tools::ffmpeg::{self, MediaInfo};
use crate::tools::{ToolError, ToolPaths};

/// Frame-rate pairs that explain a subtitle drifting away from its video.
/// `stretch(from, to)` multiplies times by `from / to`.
const FPS_PAIRS: &[(f64, f64)] = &[
    (25.0, 23.976),
    (24.0, 23.976),
    (25.0, 24.0),
    (30.0, 29.97),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncVerdict {
    /// Timings look consistent with the video.
    Good,
    /// Every line is shifted by (roughly) the same amount.
    Offset,
    /// The mismatch grows over time: a frame-rate difference.
    Framerate,
    /// The subtitle runs past the end of the video.
    Truncated,
    /// Not enough evidence either way.
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncIssue {
    /// Machine-readable kind, e.g. `bad_timing`, `overlaps`, `ends_early`.
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FpsFix {
    pub from: f64,
    pub to: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncCheck {
    pub verdict: SyncVerdict,
    pub cue_count: usize,
    pub subtitle_start_ms: i64,
    pub subtitle_end_ms: i64,
    pub video_duration_ms: Option<i64>,
    pub video_fps: Option<f64>,
    /// Present when a frame-rate mismatch is the likely cause.
    pub suggested_fix: Option<FpsFix>,
    /// Present when a constant shift is the likely cause (deep check only).
    pub suggested_shift_ms: Option<i64>,
    pub issues: Vec<SyncIssue>,
}

impl SyncCheck {
    fn issue(&mut self, kind: &str, message: impl Into<String>) {
        self.issues.push(SyncIssue {
            kind: kind.to_string(),
            message: message.into(),
        });
    }
}

fn spoken_cues(doc: &SubtitleDoc) -> impl Iterator<Item = &super::Cue> {
    doc.cues.iter().filter(|c| !c.comment)
}

/// Closest known frame-rate pair for a measured time ratio. When the video's
/// real frame rate is known, only pairs that end at that rate are considered,
/// so 1.04 s/s is reported as 25 -> 23.976 for a 23.976 fps file.
fn fps_fix_for(ratio: f64, video_fps: Option<f64>) -> Option<FpsFix> {
    let mut best: Option<(f64, FpsFix)> = None;
    for &(from, to) in FPS_PAIRS {
        for (a, b) in [(from, to), (to, from)] {
            if let Some(fps) = video_fps {
                if (b - fps).abs() / fps > 0.0005 {
                    continue;
                }
            }
            let expected = a / b;
            let error = (ratio - expected).abs() / expected;
            if error < 0.004 && best.as_ref().is_none_or(|(e, _)| error < *e) {
                best = Some((error, FpsFix { from: a, to: b }));
            }
        }
    }
    best.map(|(_, fix)| fix)
}

/// Instant consistency check against the container's duration and frame rate.
pub fn check(doc: &SubtitleDoc, media: &MediaInfo) -> SyncCheck {
    let cues: Vec<&super::Cue> = spoken_cues(doc).collect();
    let subtitle_start_ms = cues.iter().map(|c| c.start_ms).min().unwrap_or(0);
    let subtitle_end_ms = cues.iter().map(|c| c.end_ms).max().unwrap_or(0);

    let mut out = SyncCheck {
        verdict: SyncVerdict::Good,
        cue_count: cues.len(),
        subtitle_start_ms,
        subtitle_end_ms,
        video_duration_ms: media.duration_ms,
        video_fps: media.fps,
        suggested_fix: None,
        suggested_shift_ms: None,
        issues: Vec::new(),
    };

    if cues.is_empty() {
        out.verdict = SyncVerdict::Uncertain;
        out.issue("empty", "The subtitle has no dialogue cues.");
        return out;
    }

    let bad = cues.iter().filter(|c| c.end_ms <= c.start_ms).count();
    if bad > 0 {
        out.issue(
            "bad_timing",
            format!("{bad} cue(s) end at or before they start."),
        );
    }

    let mut overlaps = 0;
    for pair in cues.windows(2) {
        if pair[1].start_ms < pair[0].end_ms - 200 {
            overlaps += 1;
        }
    }
    if overlaps * 10 > cues.len() {
        out.issue(
            "overlaps",
            format!("{overlaps} cues overlap the previous one, which is unusual."),
        );
    }

    if let Some(duration) = media.duration_ms {
        let over = subtitle_end_ms - duration;
        if over > 2_000 {
            out.verdict = SyncVerdict::Truncated;
            out.issue(
                "beyond_video",
                format!(
                    "The subtitle ends {:.1} s after the video does.",
                    over as f64 / 1000.0
                ),
            );
        } else if duration - subtitle_end_ms > 90_000 {
            out.issue(
                "ends_early",
                format!(
                    "The subtitle stops {:.0} s before the video ends (normal for credits, wrong for a short cut).",
                    (duration - subtitle_end_ms) as f64 / 1000.0
                ),
            );
        }

        // A frame-rate mismatch shows up as a stable ratio between the spans.
        if duration > 60_000 && subtitle_end_ms > 0 {
            let ratio = subtitle_end_ms as f64 / duration as f64;
            if (ratio - 1.0).abs() * duration as f64 > 2_000.0 {
                if let Some(fix) = fps_fix_for(ratio, media.fps) {
                    out.verdict = SyncVerdict::Framerate;
                    out.suggested_fix = Some(fix);
                    out.issue(
                        "framerate",
                        format!(
                            "The subtitle spans {:.1}% of the video: it was made for {:.3} fps and played at {:.3} fps.",
                            (ratio - 1.0) * 100.0,
                            fix.from,
                            fix.to
                        ),
                    );
                }
            }
        }

        if subtitle_start_ms > 300_000 {
            out.issue(
                "starts_late",
                format!(
                    "The first cue appears after {} min.",
                    subtitle_start_ms / 60_000
                ),
            );
        }
    }

    out
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AsrCue {
    /// Milliseconds from the start of the window.
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignWindow {
    /// Where this window starts in the video.
    pub at_ms: i64,
    pub cues: Vec<AsrCue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncMatch {
    pub window_start_ms: i64,
    pub asr_start_ms: i64,
    pub subtitle_start_ms: i64,
    /// How late the subtitle is at this point (positive = subtitles behind).
    pub offset_ms: i64,
    pub score: f64,
    pub subtitle_text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeepCheck {
    pub verdict: SyncVerdict,
    pub matches: Vec<SyncMatch>,
    /// Median offset across matches (positive = subtitles are late).
    pub offset_ms: Option<i64>,
    pub suggested_shift_ms: Option<i64>,
    pub suggested_fix: Option<FpsFix>,
    /// Machine-readable summary: `good`, `offset`, `framerate`, `no_match`, `too_few_matches`.
    pub reason: String,
}

fn tokens(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 2)
        .map(str::to_string)
        .collect()
}

/// Fraction of the shorter word set found in the other one. ASR drops words,
/// so containment works better than exact equality.
fn similarity(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let shared = a.intersection(b).count();
    shared as f64 / a.len().min(b.len()) as f64
}

fn median(mut values: Vec<i64>) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(values[values.len() / 2])
}

/// Match sampled speech against the subtitle and decide what is wrong.
pub fn align(doc: &SubtitleDoc, windows: &[AlignWindow]) -> DeepCheck {
    let cues: Vec<&super::Cue> = spoken_cues(doc).collect();
    let mut matches: Vec<SyncMatch> = Vec::new();
    let mut used: HashSet<usize> = HashSet::new();

    for window in windows {
        for asr in &window.cues {
            let asr_text = tokens(&asr.text);
            if asr_text.len() < 2 {
                continue;
            }
            let asr_abs = window.at_ms + asr.start_ms;
            let mut best: Option<(f64, usize)> = None;
            for (i, cue) in cues.iter().enumerate() {
                // Only compare with cues that could plausibly be the same line.
                if (cue.start_ms - asr_abs).abs() > 15_000 {
                    continue;
                }
                let score = similarity(&asr_text, &tokens(&crate::lines::plain_text(&cue.text)));
                if score >= 0.5 && best.is_none_or(|(s, _)| score > s) {
                    best = Some((score, i));
                }
            }
            if let Some((score, index)) = best {
                if !used.insert(index) {
                    continue;
                }
                matches.push(SyncMatch {
                    window_start_ms: window.at_ms,
                    asr_start_ms: asr.start_ms,
                    subtitle_start_ms: cues[index].start_ms,
                    offset_ms: cues[index].start_ms - asr_abs,
                    score,
                    subtitle_text: crate::lines::plain_text(&cues[index].text),
                });
            }
        }
    }

    let offsets: Vec<i64> = matches.iter().map(|m| m.offset_ms).collect();
    let median_offset = median(offsets.clone());
    let spread = match (offsets.iter().min(), offsets.iter().max()) {
        (Some(a), Some(b)) => b - a,
        _ => 0,
    };

    if matches.len() < 2 {
        return DeepCheck {
            verdict: SyncVerdict::Uncertain,
            reason: "too_few_matches".into(),
            matches,
            offset_ms: median_offset,
            suggested_shift_ms: None,
            suggested_fix: None,
        };
    }

    if spread > 800 {
        // The offset grows over time: frame-rate drift. Fit the ratio from the
        // first and last match and snap it to a known pair.
        let first = matches.first().expect("non-empty");
        let last = matches.last().expect("non-empty");
        let subtitle_span = (last.subtitle_start_ms - first.subtitle_start_ms) as f64;
        let asr_span =
            ((last.window_start_ms + last.asr_start_ms) - (first.window_start_ms + first.asr_start_ms))
                as f64;
        let ratio = if asr_span.abs() > 1.0 {
            subtitle_span / asr_span
        } else {
            1.0
        };
        return DeepCheck {
            verdict: SyncVerdict::Framerate,
            reason: "framerate".into(),
            matches,
            offset_ms: median_offset,
            suggested_shift_ms: median_offset.map(|m| -m),
            suggested_fix: fps_fix_for(ratio, None),
        };
    }

    match median_offset {
        Some(offset) if offset.abs() >= 400 => DeepCheck {
            verdict: SyncVerdict::Offset,
            reason: "offset".into(),
            matches,
            offset_ms: Some(offset),
            suggested_shift_ms: Some(-offset),
            suggested_fix: None,
        },
        _ => DeepCheck {
            verdict: SyncVerdict::Good,
            reason: "good".into(),
            matches,
            offset_ms: median_offset,
            suggested_shift_ms: None,
            suggested_fix: None,
        },
    }
}

/// Timing corrections to apply, in this order: frame rate, then offset.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SyncFix {
    pub from_fps: Option<f64>,
    pub to_fps: Option<f64>,
    pub shift_ms: Option<i64>,
}

impl SyncFix {
    pub fn is_empty(&self) -> bool {
        self.from_fps.is_none() && self.to_fps.is_none() && self.shift_ms.unwrap_or(0) == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncFixOutcome {
    pub output: PathBuf,
    pub backup: Option<PathBuf>,
    pub cues: usize,
}

/// Apply [`SyncFix`] to `doc` (stretch first, then shift).
pub fn apply_fix(doc: &mut SubtitleDoc, fix: &SyncFix) -> Result<(), super::SubtitleError> {
    if let (Some(from), Some(to)) = (fix.from_fps, fix.to_fps) {
        doc.stretch(from, to)?;
    }
    if let Some(shift) = fix.shift_ms.filter(|s| *s != 0) {
        doc.shift(shift);
    }
    Ok(())
}

/// Load `input`, apply the fix and write `output` (default `{stem}.synced.{ext}`).
/// Overwriting the input keeps a `.bak` copy first.
pub fn fix_file(
    input: &Path,
    output: Option<&Path>,
    overwrite: bool,
    fix: &SyncFix,
) -> Result<SyncFixOutcome, SyncError> {
    let mut doc = SubtitleDoc::load(input)?;
    apply_fix(&mut doc, fix)?;

    let output = match output.filter(|p| !p.as_os_str().to_string_lossy().trim().is_empty()) {
        Some(p) => p.to_path_buf(),
        None if overwrite => input.to_path_buf(),
        None => {
            let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("srt");
            let stem = input
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("subtitle");
            input.with_file_name(format!("{stem}.synced.{ext}"))
        }
    };

    let backup = if output == input {
        let name = input
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("subtitle");
        let backup = input.with_file_name(format!("{name}.bak"));
        std::fs::copy(input, &backup).map_err(|source| super::SubtitleError::Io {
            path: backup.clone(),
            source,
        })?;
        Some(backup)
    } else {
        None
    };

    doc.save(&output)?;
    Ok(SyncFixOutcome {
        output,
        backup,
        cues: doc.cues.len(),
    })
}

/// Convenience for the "found a subtitle online" flow: load both files and run
/// the instant check. `Err` means the check itself could not run.
pub fn quick_check(
    paths: &ToolPaths,
    video: &Path,
    subtitle: &Path,
) -> Result<SyncCheck, SyncError> {
    let doc = SubtitleDoc::load(subtitle)?;
    let media = ffmpeg::probe_media(paths, &video.to_string_lossy())?;
    Ok(check(&doc, &media))
}

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Tool(#[from] ToolError),
    #[error(transparent)]
    Whisper(#[from] crate::whisper::WhisperError),
    #[error(transparent)]
    Subtitle(#[from] super::SubtitleError),
    #[error("The video duration could not be read, so its timings cannot be checked.")]
    NoDuration,
    #[error("The operation was cancelled.")]
    Cancelled,
    #[error("{0}")]
    Io(String),
}

/// Window length used for the speech check.
const WINDOW_MS: i64 = 22_000;

/// Transcribe three windows of the real audio and align them with the
/// subtitle. `model` must already be downloaded.
pub fn deep_check(
    paths: &ToolPaths,
    video: &Path,
    doc: &SubtitleDoc,
    model: &str,
    cache_dir: Option<&Path>,
    progress: ProgressFn<'_>,
    cancel: CancellationToken,
) -> Result<DeepCheck, SyncError> {
    let media = ffmpeg::probe_media(paths, &video.to_string_lossy())?;
    let duration = media.duration_ms.ok_or(SyncError::NoDuration)?;
    if duration < WINDOW_MS * 2 {
        return Err(SyncError::Io(
            "The video is too short for a speech check.".into(),
        ));
    }

    // Start at 10/45/80 % so credits and the cold open are skipped.
    let starts = [
        duration / 10,
        duration * 45 / 100,
        duration * 80 / 100,
    ];
    let dir = std::env::temp_dir().join(format!("subforge-sync-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).map_err(|e| SyncError::Io(e.to_string()))?;

    let mut windows = Vec::new();
    for (i, start) in starts.iter().enumerate() {
        if cancel.is_cancelled() {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(SyncError::Cancelled);
        }
        let base = 5.0 + (i as f64) * 30.0;
        progress(base, Some("Extracting audio"));
        let wav: PathBuf = dir.join(format!("w{i}.wav"));
        let wav_s = wav.to_string_lossy().to_string();
        ffmpeg::extract_audio_snippet(
            paths,
            &video.to_string_lossy(),
            &wav_s,
            *start,
            WINDOW_MS.min(duration - start),
        )?;

        progress(base + 5.0, Some("Listening"));
        let srt = dir.join(format!("w{i}.srt"));
        let opts = crate::whisper::TranscribeOptions {
            input: wav,
            output: srt.clone(),
            model: model.to_string(),
            device: Default::default(),
            language: None,
            format: crate::whisper::OutFormat::Srt,
            word_timestamps: false,
            vad_filter: false,
            cache_dir: cache_dir.map(Path::to_path_buf),
        };
        let mut listen = |pct: f64, msg: Option<&str>| {
            progress(base + 5.0 + pct * 0.25, msg);
        };
        crate::whisper::transcribe(paths, &opts, &mut listen, cancel.clone())?;
        let asr_doc = SubtitleDoc::load(&srt)?;
        windows.push(AlignWindow {
            at_ms: *start,
            cues: asr_doc
                .cues
                .iter()
                .filter(|c| !c.comment)
                .map(|c| AsrCue {
                    start_ms: c.start_ms,
                    end_ms: c.end_ms,
                    text: crate::lines::plain_text(&c.text),
                })
                .collect(),
        });
    }

    let _ = std::fs::remove_dir_all(&dir);
    progress(95.0, Some("Comparing"));
    let mut result = align(doc, &windows);
    result.matches.sort_by_key(|m| m.subtitle_start_ms);
    progress(100.0, None);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subtitle::{Cue, Format, SubtitleDoc};

    fn doc(lines: &[(&str, i64, i64)]) -> SubtitleDoc {
        SubtitleDoc::new(
            Format::Srt,
            lines
                .iter()
                .map(|(text, start, end)| Cue::new(*start, *end, *text))
                .collect(),
        )
    }

    fn media(duration_ms: i64) -> MediaInfo {
        MediaInfo {
            duration_ms: Some(duration_ms),
            fps: Some(23.976),
        }
    }

    #[test]
    fn a_matching_span_is_good() {
        let d = doc(&[("Hello", 1000, 3000), ("Bye", 50_000, 52_000)]);
        let check = check(&d, &media(52_000));
        assert_eq!(check.verdict, SyncVerdict::Good);
        assert!(check.suggested_fix.is_none());
    }

    #[test]
    fn pal_speedup_is_detected() {
        // 25 fps material played at 23.976 runs 4.27% long.
        let d = doc(&[("Hello", 1000, 3_000), ("Bye", 103_000, 104_000)]);
        let check = check(&d, &media(100_000));
        assert_eq!(check.verdict, SyncVerdict::Framerate);
        let fix = check.suggested_fix.unwrap();
        assert_eq!(fix.from, 25.0);
        assert!((fix.to - 23.976).abs() < 0.001);
    }

    #[test]
    fn a_subtitle_past_the_video_end_is_truncated() {
        let d = doc(&[("Hello", 1000, 3_000), ("Bye", 104_000, 110_000)]);
        let check = check(&d, &media(100_000));
        assert_eq!(check.verdict, SyncVerdict::Truncated);
        assert!(check.issues.iter().any(|i| i.kind == "beyond_video"));
    }

    #[test]
    fn a_short_ending_is_only_a_note() {
        let d = doc(&[("Hello", 1000, 3_000), ("Bye", 10_000, 12_000)]);
        let check = check(&d, &media(400_000));
        assert_eq!(check.verdict, SyncVerdict::Good);
        assert!(check.issues.iter().any(|i| i.kind == "ends_early"));
    }

    fn window(at: i64, lines: &[(&str, i64)]) -> AlignWindow {
        AlignWindow {
            at_ms: at,
            cues: lines
                .iter()
                .map(|(text, start)| AsrCue {
                    start_ms: *start,
                    end_ms: *start + 2000,
                    text: (*text).to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn a_constant_delay_is_reported_with_the_shift_that_fixes_it() {
        // Subtitle lines are 2 s later than the speech in two windows.
        let d = doc(&[
            ("hello there my friend", 22_000, 24_000),
            ("how are you doing today", 74_000, 76_000),
        ]);
        let windows = [
            window(20_000, &[("hello there my friend", 0)]),
            window(72_000, &[("how are you doing today", 0)]),
        ];
        let result = align(&d, &windows);
        assert_eq!(result.verdict, SyncVerdict::Offset);
        assert_eq!(result.offset_ms, Some(2000));
        assert_eq!(result.suggested_shift_ms, Some(-2000));
    }

    #[test]
    fn in_sync_speech_is_good() {
        let d = doc(&[
            ("hello there my friend", 20_300, 22_000),
            ("how are you doing today", 72_200, 74_000),
        ]);
        let windows = [
            window(20_000, &[("hello there my friend", 0)]),
            window(72_000, &[("how are you doing today", 0)]),
        ];
        let result = align(&d, &windows);
        assert_eq!(result.verdict, SyncVerdict::Good);
        assert_eq!(result.suggested_shift_ms, None);
    }

    #[test]
    fn growing_offsets_point_at_a_frame_rate_problem() {
        let d = doc(&[
            ("hello there my friend", 21_000, 23_000),
            ("how are you doing today", 76_000, 78_000),
        ]);
        let windows = [
            window(20_000, &[("hello there my friend", 0)]),
            window(72_000, &[("how are you doing today", 0)]),
        ];
        let result = align(&d, &windows);
        assert_eq!(result.verdict, SyncVerdict::Framerate);
    }

    #[test]
    fn unrelated_speech_does_not_produce_a_fix() {
        let d = doc(&[("completely different words here", 20_000, 22_000)]);
        let windows = [window(20_000, &[("nothing alike at all", 0)])];
        let result = align(&d, &windows);
        assert_eq!(result.verdict, SyncVerdict::Uncertain);
        assert_eq!(result.reason, "too_few_matches");
        assert!(result.suggested_shift_ms.is_none());
    }
    #[test]
    fn fix_file_applies_stretch_then_shift() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("episode.srt");
        std::fs::write(
            &input,
            "1\n00:00:10,000 --> 00:00:12,000\nHello\n\n2\n00:01:40,000 --> 00:01:42,000\nBye\n",
        )
        .unwrap();

        let outcome = fix_file(
            &input,
            None,
            false,
            &SyncFix {
                from_fps: Some(25.0),
                to_fps: Some(23.976),
                shift_ms: Some(-1000),
            },
        )
        .unwrap();

        assert!(outcome.output.ends_with("episode.synced.srt"));
        let text = std::fs::read_to_string(&outcome.output).unwrap();
        // 10.000 s * 25/23.976 = 10.427 s, minus 1 s = 9.427 s.
        assert!(text.contains("00:00:09,427"), "{text}");
        // 100 s * 1.0427 = 104.271 - 1 = 103.271 s.
        assert!(text.contains("00:01:43,271"), "{text}");
        // The source is untouched and there is no backup for a new file.
        assert!(std::fs::read_to_string(&input).unwrap().contains("00:00:10,000"));
        assert!(outcome.backup.is_none());
    }

    #[test]
    fn fix_file_overwriting_keeps_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("episode.srt");
        let original = "1\n00:00:10,000 --> 00:00:12,000\nHello\n";
        std::fs::write(&input, original).unwrap();

        let outcome = fix_file(
            &input,
            None,
            true,
            &SyncFix {
                shift_ms: Some(500),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(outcome.output, input);
        let backup = outcome.backup.expect("backup");
        assert_eq!(std::fs::read_to_string(backup).unwrap(), original);
        assert!(std::fs::read_to_string(&input).unwrap().contains("00:00:10,500"));
    }

    #[test]
    fn an_empty_fix_is_recognised() {
        assert!(SyncFix::default().is_empty());
        assert!(!SyncFix {
            shift_ms: Some(1),
            ..Default::default()
        }
        .is_empty());
    }

}
