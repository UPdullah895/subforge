//! whisper.cpp transcription. Tests that need the real `tiny` model are ignored unless
//! `SUBFORGE_WHISPER_TEST=1` (see tests/fixtures/media/README.md).

use std::path::{Path, PathBuf};

use subforge_core::subtitle::{Format, SubtitleDoc};
use subforge_core::tools::ToolPaths;
use subforge_core::whisper::{
    default_cache_dir, engine_info, transcribe, Device, OutFormat, TranscribeOptions, WhisperError,
    VAD_MODEL,
};
use subforge_core::CancellationToken;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/media/speech_en_10s.opus")
}

fn model_cache() -> PathBuf {
    std::env::var_os("SUBFORGE_WHISPER_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(default_cache_dir)
}

/// Fails loudly (like `tools_handle_unicode_space_paths`) when run without the model setup.
fn require_model() {
    assert_eq!(
        std::env::var("SUBFORGE_WHISPER_TEST").as_deref(),
        Ok("1"),
        "this test needs the tiny whisper model: set SUBFORGE_WHISPER_TEST=1 and SUBFORGE_WHISPER_CACHE"
    );
}

fn options(output: PathBuf, cache: PathBuf) -> TranscribeOptions {
    TranscribeOptions {
        input: fixture(),
        output,
        model: "tiny".into(),
        device: Device::Cpu,
        language: Some("en".into()),
        format: OutFormat::Srt,
        word_timestamps: false,
        vad_filter: false,
        cache_dir: Some(cache),
    }
}

fn assert_sane_timing(doc: &SubtitleDoc) {
    assert!(!doc.cues.is_empty(), "no cues");
    let mut prev_start = 0;
    for c in &doc.cues {
        assert!(c.start_ms <= c.end_ms, "start after end: {c:?}");
        assert!(c.end_ms <= 11_000, "cue past the audio: {c:?}");
        assert!(c.start_ms >= prev_start, "not monotonic: {c:?}");
        prev_start = c.start_ms;
    }
}

fn all_text(doc: &SubtitleDoc) -> String {
    doc.cues
        .iter()
        .map(|c| c.text.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
#[ignore = "needs the tiny whisper model; set SUBFORGE_WHISPER_TEST=1"]
fn transcribe_tiny_cpu_produces_srt() {
    require_model();
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out put ملف.srt");
    let mut last = -1.0;
    let mut reports = 0;
    let mut cb = |p: f64, _: Option<&str>| {
        assert!(p >= last, "progress went backwards: {last} -> {p}");
        last = p;
        reports += 1;
    };
    let outcome = transcribe(
        &ToolPaths::default(),
        &options(out.clone(), model_cache()),
        &mut cb,
        CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(outcome.device_used, "cpu");
    assert!(outcome.segments >= 1);
    assert_eq!(last, 100.0);
    assert!(reports >= 3);

    let doc = SubtitleDoc::load(&out).unwrap();
    assert_eq!(doc.format, Format::Srt);
    assert_eq!(doc.cues.len(), outcome.segments);
    assert_sane_timing(&doc);
    let text = all_text(&doc);
    assert!(
        text.contains("fox") || text.contains("subforge"),
        "unexpected text: {text}"
    );
}

#[test]
#[ignore = "needs the tiny whisper model; set SUBFORGE_WHISPER_TEST=1"]
fn transcribe_word_timestamps_vtt_with_vad() {
    require_model();
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("words.vtt");
    let mut opts = options(out.clone(), model_cache());
    opts.format = OutFormat::Vtt;
    opts.word_timestamps = true;
    opts.vad_filter = true;
    opts.language = None;
    let outcome = transcribe(
        &ToolPaths::default(),
        &opts,
        &mut |_, _| {},
        CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(outcome.detected_language.as_deref(), Some("en"));

    let raw = std::fs::read_to_string(&out).unwrap();
    assert!(raw.starts_with("WEBVTT"), "{raw}");
    let doc = SubtitleDoc::load(&out).unwrap();
    assert_eq!(doc.format, Format::Vtt);
    assert_sane_timing(&doc);
    // One cue per word: several cues, none holding more than one word.
    assert!(doc.cues.len() >= 5, "{raw}");
    assert!(
        doc.cues.iter().all(|c| !c.text.trim().contains(' ')),
        "{raw}"
    );
}

#[test]
#[ignore = "needs the tiny whisper model; set SUBFORGE_WHISPER_TEST=1"]
fn transcribe_cancel_mid_run_returns_cancelled() {
    require_model();
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("cancelled.srt");
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    // 10 % is reported right before whisper.cpp starts, so this exercises the abort callback.
    let mut cb = |p: f64, _: Option<&str>| {
        if p >= 10.0 {
            trigger.cancel();
        }
    };
    let err = transcribe(
        &ToolPaths::default(),
        &options(out.clone(), model_cache()),
        &mut cb,
        cancel,
    )
    .unwrap_err();
    assert!(matches!(err, WhisperError::Cancelled), "{err:?}");
    assert!(!out.exists(), "a cancelled run must not write output");
}

#[test]
#[ignore = "needs the tiny whisper model; set SUBFORGE_WHISPER_TEST=1"]
fn vad_without_vad_model_is_a_clear_error() {
    require_model();
    let real = model_cache();
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::copy(real.join("ggml-tiny.bin"), cache.join("ggml-tiny.bin")).unwrap();
    let mut opts = options(dir.path().join("x.srt"), cache);
    opts.vad_filter = true;
    let err = transcribe(
        &ToolPaths::default(),
        &opts,
        &mut |_, _| {},
        CancellationToken::new(),
    )
    .unwrap_err();
    assert!(
        matches!(err, WhisperError::VadModelMissing { .. }),
        "{err:?}"
    );
    assert!(err.to_string().contains(VAD_MODEL.file), "{err}");
}

#[test]
fn transcribe_cancel_returns_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.srt");
    let cancel = CancellationToken::new();
    cancel.cancel();
    let err = transcribe(
        &ToolPaths::default(),
        &options(out.clone(), dir.path().to_path_buf()),
        &mut |_, _| {},
        cancel,
    )
    .unwrap_err();
    assert!(matches!(err, WhisperError::Cancelled), "{err:?}");
    assert!(!out.exists());
}

#[test]
fn missing_model_is_a_clear_error() {
    let dir = tempfile::tempdir().unwrap();
    let err = transcribe(
        &ToolPaths::default(),
        &options(dir.path().join("out.srt"), dir.path().join("empty cache")),
        &mut |_, _| {},
        CancellationToken::new(),
    )
    .unwrap_err();
    assert!(
        matches!(err, WhisperError::ModelNotDownloaded { ref model } if model == "tiny"),
        "{err:?}"
    );
    assert!(err.to_string().contains("Download"), "{err}");
}

#[test]
fn unknown_model_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path().join("out.srt"), dir.path().to_path_buf());
    opts.model = "large-v9".into();
    let err = transcribe(
        &ToolPaths::default(),
        &opts,
        &mut |_, _| {},
        CancellationToken::new(),
    )
    .unwrap_err();
    assert!(matches!(err, WhisperError::UnknownModel(_)), "{err:?}");
}

#[test]
fn device_cuda_string_deserialises_to_auto() {
    let d: Device = serde_json::from_str("\"cuda\"").unwrap();
    assert_eq!(d, Device::Auto);
    for (s, want) in [
        ("auto", Device::Auto),
        ("cpu", Device::Cpu),
        ("gpu", Device::Gpu),
    ] {
        let d: Device = serde_json::from_str(&format!("\"{s}\"")).unwrap();
        assert_eq!(d, want);
        assert_eq!(serde_json::to_string(&want).unwrap(), format!("\"{s}\""));
    }
}

#[test]
fn transcribe_options_deserialise_from_request_json() {
    let opts: TranscribeOptions = serde_json::from_str(
        r#"{"input":"/a b/in.mkv","output":"/a b/in.vtt","model":"base","device":"cuda",
            "language":null,"format":"vtt","word_timestamps":true,"vad_filter":false,
            "cache_dir":null}"#,
    )
    .unwrap();
    assert_eq!(opts.device, Device::Auto);
    assert_eq!(opts.format, OutFormat::Vtt);
    assert_eq!(opts.input, PathBuf::from("/a b/in.mkv"));
    assert!(opts.word_timestamps && !opts.vad_filter);
}

#[test]
fn engine_info_is_cpu_only_for_now() {
    let info = engine_info();
    assert!(info.engine.starts_with("whisper.cpp "), "{}", info.engine);
    assert_eq!(info.gpu, None);
    assert_eq!(info.devices, vec!["auto".to_string(), "cpu".to_string()]);
}
