//! End-to-end self-test of an installed build (`subforge --self-test`).
//!
//! Runs the core operations through the same code paths the app uses, against the bundled
//! FFmpeg/MKVToolNix, on copies of the test fixtures placed in a folder whose name has Arabic
//! characters and a space. The result is written to `<out>/selftest.json`. CI runs it against
//! the installed Windows build and the AppImage.

use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::lines::{extract_lines, rebuild_lines};
use crate::progress::CancellationToken;
use crate::subtitle::{self, Format, SubtitleDoc};
use crate::tools::mkvtoolnix::{self, SubtitleMergeSpec};
use crate::tools::{ffmpeg, tool_status, ToolPaths, ToolSource};
use crate::whisper::{self, Device, OutFormat, TranscribeOptions};

/// Every step, in the order it runs.
pub const STEP_NAMES: [&str; 11] = [
    "tools_bundled",
    "prepare_fixtures",
    "list_tracks",
    "extract_subtitles",
    "extract_audio",
    "convert_ass_srt_vtt",
    "shift_stretch",
    "translate_stub",
    "whisper_tiny",
    "mux",
    "verify_mux",
];

/// Folder the fixtures are copied into (inside the output directory).
pub const WORK_DIR_NAME: &str = "مجلد اختبار";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepResult {
    pub name: String,
    pub ok: bool,
    pub millis: u64,
    /// What was checked, or why the step failed.
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfTestReport {
    pub steps: Vec<StepResult>,
    pub ok: bool,
    /// Device the transcription ran on (`"cpu"` or `"gpu"`), when it ran.
    pub device_used: Option<String>,
}

type Outcome = Result<String, String>;

/// Adds context to any displayable error.
trait Context<T> {
    fn ctx(self, what: &str) -> Result<T, String>;
}

impl<T, E: Display> Context<T> for Result<T, E> {
    fn ctx(self, what: &str) -> Result<T, String> {
        self.map_err(|e| format!("{what}: {e}"))
    }
}

/// Paths of the copied fixtures and of everything the steps produce.
struct Work {
    dir: PathBuf,
    golden: PathBuf,
    mkv: PathBuf,
    speech: PathBuf,
    basic_srt: PathBuf,
    ara_ass: PathBuf,
    eng_srt: PathBuf,
    audio: PathBuf,
    ara_srt: PathBuf,
    ara_vtt: PathBuf,
    shifted: PathBuf,
    stretched: PathBuf,
    ara_translated: PathBuf,
    eng_translated: PathBuf,
    transcript: PathBuf,
    muxed: PathBuf,
}

impl Work {
    fn new(dir: PathBuf) -> Self {
        let p = |name: &str| dir.join(name);
        Work {
            golden: p("golden"),
            mkv: p("sample clip.mkv"),
            speech: p("speech en.opus"),
            basic_srt: p("basic.srt"),
            ara_ass: p("ara.ass"),
            eng_srt: p("eng.srt"),
            audio: p("audio.mka"),
            ara_srt: p("ara.srt"),
            ara_vtt: p("ara.vtt"),
            shifted: p("basic.shift.srt"),
            stretched: p("basic.stretch.srt"),
            ara_translated: p("ara.translated.ass"),
            eng_translated: p("eng.translated.srt"),
            transcript: p("transcript.srt"),
            muxed: p("muxed out.mkv"),
            dir,
        }
    }
}

struct Runner {
    steps: Vec<StepResult>,
}

impl Runner {
    fn step(&mut self, name: &str, f: impl FnOnce() -> Outcome) -> bool {
        let started = Instant::now();
        let (ok, detail) = match f() {
            Ok(d) => (true, d),
            Err(e) => (false, e),
        };
        self.steps.push(StepResult {
            name: name.to_string(),
            ok,
            millis: started.elapsed().as_millis() as u64,
            detail,
        });
        ok
    }
}

/// Runs the self-test and writes `<out>/selftest.json`. Never panics on a failing step; the
/// report says what failed. If a tool does not resolve to the bundled copy nothing else runs,
/// since the remaining steps would test the wrong binaries.
pub fn run(paths: &ToolPaths, fixtures: &Path, out: &Path, whisper_cache: &Path) -> SelfTestReport {
    let mut r = Runner { steps: Vec::new() };
    let mut device_used = None;

    if r.step("tools_bundled", || tools_bundled(paths)) {
        let w = Work::new(out.join(WORK_DIR_NAME));
        r.step("prepare_fixtures", || prepare_fixtures(fixtures, &w));
        r.step("list_tracks", || list_tracks(paths, &w));
        r.step("extract_subtitles", || extract_subtitles(paths, &w));
        r.step("extract_audio", || extract_audio(paths, &w));
        r.step("convert_ass_srt_vtt", || convert(&w));
        r.step("shift_stretch", || shift_stretch(&w));
        r.step("translate_stub", || translate_stub(&w));
        r.step("whisper_tiny", || {
            let (detail, device) = whisper_tiny(paths, &w, whisper_cache)?;
            device_used = Some(device);
            Ok(detail)
        });
        r.step("mux", || mux(paths, &w));
        r.step("verify_mux", || verify_mux(paths, &w));
    }

    let ok = !r.steps.is_empty() && r.steps.iter().all(|s| s.ok);
    let report = SelfTestReport {
        steps: r.steps,
        ok,
        device_used,
    };
    if let Err(e) = write_report(&report, out) {
        eprintln!(
            "self-test: could not write {}: {e}",
            out.join("selftest.json").display()
        );
    }
    report
}

fn write_report(report: &SelfTestReport, out: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(out)?;
    let json = serde_json::to_string_pretty(report).map_err(std::io::Error::other)?;
    std::fs::write(out.join("selftest.json"), json + "\n")
}

fn tools_bundled(paths: &ToolPaths) -> Outcome {
    let mut lines = Vec::new();
    let mut ok = true;
    for s in tool_status(paths) {
        let line = match (&s.source, &s.path) {
            (Some(ToolSource::Bundled), Some(p)) => format!(
                "{}: bundled, {} ({})",
                s.id,
                short_version(s.version.as_deref()),
                p.display()
            ),
            (Some(src), Some(p)) => {
                ok = false;
                format!(
                    "{}: {src:?} ({}), expected the bundled copy",
                    s.id,
                    p.display()
                )
            }
            _ => {
                ok = false;
                format!(
                    "{}: {}",
                    s.id,
                    s.error
                        .as_deref()
                        .unwrap_or("not found, expected the bundled copy")
                )
            }
        };
        lines.push(line);
    }
    let detail = lines.join("; ");
    if ok {
        Ok(detail)
    } else {
        Err(detail)
    }
}

/// `ffmpeg version n9.0.2 Copyright (c) ...` -> `ffmpeg version n9.0.2`.
fn short_version(v: Option<&str>) -> &str {
    match v {
        Some(v) => v.split(" Copyright").next().unwrap_or(v).trim(),
        None => "unknown version",
    }
}

fn prepare_fixtures(fixtures: &Path, w: &Work) -> Outcome {
    if w.dir.exists() {
        std::fs::remove_dir_all(&w.dir).ctx("clearing the previous work folder")?;
    }
    std::fs::create_dir_all(&w.golden).ctx("creating the work folder")?;
    let copies = [
        (fixtures.join("media/sample.mkv"), w.mkv.clone()),
        (fixtures.join("media/speech_en_10s.opus"), w.speech.clone()),
        (fixtures.join("subtitles/basic.srt"), w.basic_srt.clone()),
    ];
    for (from, to) in &copies {
        std::fs::copy(from, to).ctx(&format!("copying {}", from.display()))?;
    }
    let goldens = fixtures.join("selftest");
    let entries = std::fs::read_dir(&goldens).ctx(&format!("reading {}", goldens.display()))?;
    let mut n = 0;
    for entry in entries {
        let entry = entry.ctx("listing the golden files")?;
        if entry.path().is_file() {
            std::fs::copy(entry.path(), w.golden.join(entry.file_name()))
                .ctx("copying a golden file")?;
            n += 1;
        }
    }
    Ok(format!(
        "{} fixtures and {n} golden files in {}",
        copies.len(),
        w.dir.display()
    ))
}

fn path_str(p: &Path) -> Result<&str, String> {
    p.to_str()
        .ok_or_else(|| format!("path is not valid UTF-8: {}", p.display()))
}

fn track_lang(t: &mkvtoolnix::Track) -> &str {
    t.properties
        .as_ref()
        .and_then(|p| p.language.as_deref())
        .unwrap_or("und")
}

fn list_tracks(paths: &ToolPaths, w: &Work) -> Outcome {
    let info = mkvtoolnix::enumerate_tracks(paths, path_str(&w.mkv)?).ctx("mkvmerge -J")?;
    let summary: Vec<String> = info
        .tracks
        .iter()
        .map(|t| format!("{}:{}:{}", t.id, t.track_type, track_lang(t)))
        .collect();
    let expected = [
        "0:video:und",
        "1:audio:und",
        "2:subtitles:ara",
        "3:subtitles:eng",
    ];
    if summary != expected {
        return Err(format!("tracks {summary:?}, expected {expected:?}"));
    }
    let audio = ffmpeg::probe_audio_tracks(paths, path_str(&w.mkv)?).ctx("ffprobe")?;
    match audio.as_slice() {
        [a] if a.index == 1 && a.codec_name.as_deref() == Some("aac") => Ok(format!(
            "mkvmerge: {}; ffprobe: stream 1 aac",
            summary.join(", ")
        )),
        _ => Err(format!(
            "ffprobe audio streams {:?}, expected one aac stream at index 1",
            audio
                .iter()
                .map(|a| (a.index, a.codec_name.clone()))
                .collect::<Vec<_>>()
        )),
    }
}

fn extract_subtitles(paths: &ToolPaths, w: &Work) -> Outcome {
    let mkv = path_str(&w.mkv)?;
    mkvtoolnix::extract_subtitle_track(paths, mkv, 2, path_str(&w.ara_ass)?)
        .ctx("mkvextract track 2")?;
    mkvtoolnix::extract_subtitle_track(paths, mkv, 3, path_str(&w.eng_srt)?)
        .ctx("mkvextract track 3")?;
    let ara = subtitle::load(&w.ara_ass).ctx("reading the extracted ASS")?;
    let eng = subtitle::load(&w.eng_srt).ctx("reading the extracted SRT")?;
    let texts = |d: &SubtitleDoc| d.cues.iter().map(|c| c.text.clone()).collect::<Vec<_>>();
    if ara.format != Format::Ass || texts(&ara) != ["مرحبا بالعالم"] {
        return Err(format!(
            "ara track: {:?} {:?}, expected one ASS line",
            ara.format,
            texts(&ara)
        ));
    }
    if eng.format != Format::Srt || texts(&eng) != ["Hello world"] {
        return Err(format!("eng track: {:?} {:?}", eng.format, texts(&eng)));
    }
    Ok("ara.ass (1 line) and eng.srt (1 line)".into())
}

fn extract_audio(paths: &ToolPaths, w: &Work) -> Outcome {
    ffmpeg::extract_audio(paths, path_str(&w.mkv)?, 1, path_str(&w.audio)?, None)
        .ctx("ffmpeg audio extraction")?;
    let streams = ffmpeg::probe_audio_tracks(paths, path_str(&w.audio)?).ctx("ffprobe")?;
    match streams.as_slice() {
        [s] if s.codec_name.as_deref() == Some("aac") => {
            let size = std::fs::metadata(&w.audio).map(|m| m.len()).unwrap_or(0);
            Ok(format!("audio.mka: one aac stream, {size} bytes"))
        }
        _ => Err(format!("audio.mka has {} audio streams", streams.len())),
    }
}

/// Text with line endings and a leading BOM normalised, so goldens hold on every platform.
fn normalised(p: &Path) -> Result<String, String> {
    let bytes = std::fs::read(p).ctx(&format!("reading {}", p.display()))?;
    let text = String::from_utf8(bytes).ctx(&format!("{} is not UTF-8", p.display()))?;
    Ok(text.trim_start_matches('\u{feff}').replace("\r\n", "\n"))
}

fn compare_golden(w: &Work, actual: &Path, golden_name: &str) -> Result<(), String> {
    let got = normalised(actual)?;
    let want = normalised(&w.golden.join(golden_name))?;
    if got == want {
        return Ok(());
    }
    let (n, (g, e)) = got
        .lines()
        .chain(std::iter::repeat("<end of file>"))
        .zip(want.lines().chain(std::iter::repeat("<end of file>")))
        .enumerate()
        .find(|(_, (g, e))| g != e)
        .unwrap_or((0, ("?", "?")));
    Err(format!(
        "{} differs from golden {golden_name} at line {}: got {g:?}, expected {e:?}",
        actual.display(),
        n + 1
    ))
}

fn convert(w: &Work) -> Outcome {
    subtitle::convert_file(&w.ara_ass, &w.ara_srt, Format::Srt).ctx("ASS to SRT")?;
    subtitle::convert_file(&w.ara_srt, &w.ara_vtt, Format::Vtt).ctx("SRT to VTT")?;
    compare_golden(w, &w.ara_srt, "ara.srt")?;
    compare_golden(w, &w.ara_vtt, "ara.vtt")?;
    Ok("ara.ass -> ara.srt -> ara.vtt match the goldens".into())
}

fn shift_stretch(w: &Work) -> Outcome {
    subtitle::shift_file(&w.basic_srt, &w.shifted, 1500).ctx("shift +1500 ms")?;
    subtitle::stretch_file(&w.basic_srt, &w.stretched, 25.0, 23.976).ctx("stretch 25 -> 23.976")?;
    compare_golden(w, &w.shifted, "basic.shift.srt")?;
    compare_golden(w, &w.stretched, "basic.stretch.srt")?;
    Ok("shift +1500 ms and stretch 25 -> 23.976 fps match the goldens".into())
}

/// In-process stand-in for a translation provider.
fn stub_translate(text: &str) -> String {
    if text.trim().is_empty() {
        String::new()
    } else {
        format!("{} [AR]", text.to_uppercase())
    }
}

fn translate_stub(w: &Work) -> Outcome {
    let mut lines = 0;
    for (src, dst, golden) in [
        (&w.ara_ass, &w.ara_translated, "ara.translated.ass"),
        (&w.eng_srt, &w.eng_translated, "eng.translated.srt"),
    ] {
        let extracted =
            extract_lines(src).ctx(&format!("extracting lines of {}", src.display()))?;
        let translated: Vec<String> = extracted.iter().map(|l| stub_translate(&l.text)).collect();
        rebuild_lines(src, &translated, dst, true).ctx(&format!("rebuilding {}", dst.display()))?;
        compare_golden(w, dst, golden)?;
        lines += extracted.len();
    }
    let ara = normalised(&w.ara_translated)?;
    if !ara.contains('\u{202B}') {
        return Err("RTL fix missing: no RLE mark in the Arabic output".into());
    }
    Ok(format!(
        "{lines} lines translated by the stub with RTL fix; outputs match the goldens"
    ))
}

fn whisper_tiny(paths: &ToolPaths, w: &Work, cache: &Path) -> Result<(String, String), String> {
    let started = Instant::now();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ctx("starting the download runtime")?;
    let mut progress = |_: f64, _: Option<&str>| {};
    rt.block_on(whisper::download(
        "tiny",
        cache,
        whisper::DEFAULT_BASE_URL,
        &mut progress,
        CancellationToken::new(),
    ))
    .ctx("downloading the tiny model")?;
    let download_ms = started.elapsed().as_millis();

    let opts = TranscribeOptions {
        input: w.speech.clone(),
        output: w.transcript.clone(),
        model: "tiny".into(),
        device: Device::Auto,
        language: Some("en".into()),
        format: OutFormat::Srt,
        word_timestamps: false,
        vad_filter: false,
        cache_dir: Some(cache.to_path_buf()),
    };
    let mut progress = |_: f64, _: Option<&str>| {};
    let outcome = whisper::transcribe(paths, &opts, &mut progress, CancellationToken::new())
        .ctx("transcribing")?;

    let doc = subtitle::load(&w.transcript).ctx("reading the transcript")?;
    if doc.cues.is_empty() {
        return Err("the transcript has no cues".into());
    }
    let mut prev_start = 0;
    for c in &doc.cues {
        if c.start_ms < prev_start || c.end_ms <= c.start_ms || c.end_ms > 10_500 {
            return Err(format!(
                "bad timestamps {}..{} ms in a 10 s clip",
                c.start_ms, c.end_ms
            ));
        }
        prev_start = c.start_ms;
    }
    let text: Vec<&str> = doc.cues.iter().map(|c| c.text.trim()).collect();
    let text = text.join(" ");
    if text.trim().is_empty() {
        return Err("the transcript is empty".into());
    }
    Ok((
        format!(
            "device {}; model ready in {download_ms} ms; {} cues: {text:?}",
            outcome.device_used,
            doc.cues.len()
        ),
        outcome.device_used,
    ))
}

fn mux(paths: &ToolPaths, w: &Work) -> Outcome {
    let subs = [
        SubtitleMergeSpec::new(path_str(&w.ara_translated)?.to_string(), Some("ara".into())),
        SubtitleMergeSpec::new(path_str(&w.transcript)?.to_string(), Some("eng".into())),
    ];
    mkvtoolnix::merge_subtitles(paths, path_str(&w.mkv)?, &subs, path_str(&w.muxed)?)
        .ctx("mkvmerge")?;
    Ok(format!(
        "{} with the translated ASS and the transcript",
        w.muxed.display()
    ))
}

fn verify_mux(paths: &ToolPaths, w: &Work) -> Outcome {
    let info = mkvtoolnix::enumerate_tracks(paths, path_str(&w.muxed)?).ctx("mkvmerge -J")?;
    let subs: Vec<String> = info
        .tracks
        .iter()
        .filter(|t| t.track_type == "subtitles")
        .map(|t| format!("{}:{}", t.codec, track_lang(t)))
        .collect();
    let langs: Vec<&str> = info
        .tracks
        .iter()
        .filter(|t| t.track_type == "subtitles")
        .map(track_lang)
        .collect();
    if info.tracks.len() != 6 || langs != ["ara", "eng", "ara", "eng"] {
        return Err(format!(
            "{} tracks, subtitles {subs:?}; expected 6 tracks with subtitles ara, eng, ara, eng",
            info.tracks.len()
        ));
    }
    Ok(format!("6 tracks; subtitles {}", subs.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_compare_ignores_bom_and_crlf_but_reports_the_first_difference() {
        let dir = tempfile::tempdir().unwrap();
        let w = Work::new(dir.path().to_path_buf());
        std::fs::create_dir_all(&w.golden).unwrap();
        std::fs::write(w.golden.join("g.srt"), "1\nA\n\n2\nB\n").unwrap();

        let same = dir.path().join("same.srt");
        std::fs::write(&same, "\u{feff}1\r\nA\r\n\r\n2\r\nB\r\n").unwrap();
        assert_eq!(compare_golden(&w, &same, "g.srt"), Ok(()));

        let other = dir.path().join("other.srt");
        std::fs::write(&other, "1\nA\n\n2\nC\n").unwrap();
        let err = compare_golden(&w, &other, "g.srt").unwrap_err();
        assert!(err.contains("line 5"), "{err}");
        assert!(err.contains("\"C\"") && err.contains("\"B\""), "{err}");
    }

    #[test]
    fn stub_translates_non_empty_lines_only() {
        assert_eq!(stub_translate("Hello world"), "HELLO WORLD [AR]");
        assert_eq!(stub_translate("  "), "");
    }
}
