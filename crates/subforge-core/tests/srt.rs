use std::path::PathBuf;

use subforge_core::subtitle::encoding::{decode, DetectedEncoding};
use subforge_core::subtitle::{Format, SubtitleDoc, SubtitleError};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/subtitles")
        .join(name)
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).unwrap()
}

#[test]
fn decode_utf8_bom() {
    let (text, enc) = decode(b"\xEF\xBB\xBFhi");
    assert_eq!(text, "hi");
    assert_eq!(
        enc,
        DetectedEncoding {
            name: "UTF-8",
            had_bom: true
        }
    );
}

#[test]
fn decode_plain_utf8_has_no_bom() {
    let (text, enc) = decode("héllo".as_bytes());
    assert_eq!(text, "héllo");
    assert_eq!(
        enc,
        DetectedEncoding {
            name: "UTF-8",
            had_bom: false
        }
    );
}

#[test]
fn decode_utf16le_bom() {
    let doc = SubtitleDoc::load(&fixture("utf16le_bom.srt")).unwrap();
    assert_eq!(doc.encoding.name, "UTF-16LE");
    assert!(doc.encoding.had_bom);
    assert_eq!(doc.cues[0].text, "Hello");
}

#[test]
fn decode_utf16be_bom() {
    let mut bytes = vec![0xFE, 0xFF];
    for u in "1\n00:00:01,000 --> 00:00:02,000\nHi\n".encode_utf16() {
        bytes.extend_from_slice(&u.to_be_bytes());
    }
    let doc = SubtitleDoc::parse(&bytes, None).unwrap();
    assert_eq!(doc.encoding.name, "UTF-16BE");
    assert_eq!(doc.cues[0].text, "Hi");
}

#[test]
fn decode_cp1256_arabic() {
    let doc = SubtitleDoc::load(&fixture("arabic_cp1256.srt")).unwrap();
    assert_eq!(doc.encoding.name, "windows-1256");
    assert!(!doc.encoding.had_bom);
    assert!(doc.cues[0].text.contains("مرحبا"));
}

#[test]
fn cp1256_input_is_written_back_as_utf8() {
    let doc = SubtitleDoc::load(&fixture("arabic_cp1256.srt")).unwrap();
    let out = doc.to_bytes();
    let text = String::from_utf8(out).expect("output must be UTF-8");
    assert!(text.contains("مرحبا بكم في هذا الفيلم"));
}

#[test]
fn srt_parses_multiline_and_tags() {
    let doc = SubtitleDoc::load(&fixture("basic.srt")).unwrap();
    assert_eq!(doc.format, Format::Srt);
    assert_eq!(doc.cues.len(), 3);
    let cue = &doc.cues[1];
    assert_eq!(cue.text, "{\\i1}Line one{\\i0}\\NLine two");
    assert_eq!(cue.start_ms, 3_500);
    assert_eq!(cue.end_ms, 5_000);
    assert!(!cue.comment);
    assert_eq!(doc.cues[2].start_ms, 62_250);
}

#[test]
fn srt_tolerates_missing_index_and_extra_blank_lines() {
    let src = "\n\n00:00:01,000 --> 00:00:02,000\nFirst\n\n\n\n7\n00:00:03,000 --> 00:00:04,500\nSecond\nline\n\n\n";
    let doc = SubtitleDoc::parse(src.as_bytes(), None).unwrap();
    assert_eq!(doc.cues.len(), 2);
    assert_eq!(doc.cues[0].text, "First");
    assert_eq!(doc.cues[1].text, "Second\\Nline");
    assert_eq!(doc.cues[1].end_ms, 4_500);
}

#[test]
fn srt_accepts_dot_millisecond_separator() {
    let src = "1\n00:00:01.5 --> 00:00:02.25\nx\n";
    let doc = SubtitleDoc::parse(src.as_bytes(), None).unwrap();
    assert_eq!(doc.cues[0].start_ms, 1_500);
    assert_eq!(doc.cues[0].end_ms, 2_250);
}

#[test]
fn srt_bad_timing_reports_line() {
    let src = "1\n00:00:01,000 --> 00:00:02,000\nok\n\n2\nnot a timing\ntext\n";
    let err = SubtitleDoc::parse(src.as_bytes(), None).unwrap_err();
    match &err {
        SubtitleError::Parse { line, .. } => assert_eq!(*line, 5),
        other => panic!("unexpected error: {other:?}"),
    }
    assert!(err.to_string().contains("line 5"), "{err}");
}

#[test]
fn load_error_names_file_and_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("broken.srt");
    std::fs::write(&path, "garbage\n").unwrap();
    let err = SubtitleDoc::load(&path).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("broken.srt"), "{msg}");
    assert!(msg.contains("line 1:"), "{msg}");
}

#[test]
fn load_missing_file_is_io_error_naming_path() {
    let err = SubtitleDoc::load(&fixture("nope.srt")).unwrap_err();
    assert!(matches!(err, SubtitleError::Io { .. }));
    assert!(err.to_string().contains("nope.srt"));
}

#[test]
fn srt_write_roundtrip_crlf() {
    let bytes = fixture_bytes("crlf.srt");
    let doc = SubtitleDoc::parse(&bytes, None).unwrap();
    assert_eq!(doc.to_bytes(), bytes);
}

#[test]
fn srt_write_roundtrip_lf_and_odd_numbering() {
    let bytes = fixture_bytes("basic.srt");
    let doc = SubtitleDoc::parse(&bytes, None).unwrap();
    assert_eq!(doc.to_bytes(), bytes);

    // Missing index and a non-sequential index survive untouched.
    let src = b"00:00:01,000 --> 00:00:02,000\nA\n\n9\n00:00:03,000 --> 00:00:04,000\nB\n";
    let doc = SubtitleDoc::parse(src, None).unwrap();
    assert_eq!(doc.to_bytes(), src.to_vec());
}

#[test]
fn srt_roundtrip_keeps_utf8_bom() {
    let mut bytes = b"\xEF\xBB\xBF".to_vec();
    bytes.extend_from_slice(&fixture_bytes("basic.srt"));
    let doc = SubtitleDoc::parse(&bytes, None).unwrap();
    assert!(doc.encoding.had_bom);
    assert_eq!(doc.to_bytes(), bytes);
}

#[test]
fn edited_cue_is_regenerated_and_others_untouched() {
    let bytes = fixture_bytes("crlf.srt");
    let mut doc = SubtitleDoc::parse(&bytes, None).unwrap();
    doc.cues[1].text = "{\\b1}Edited{\\b0}\\NSecond".to_string();
    doc.cues[1].end_ms = 3_723_004;
    let out = String::from_utf8(doc.to_bytes()).unwrap();
    assert!(
        out.contains("2\r\n00:00:03,500 --> 01:02:03,004\r\n<b>Edited</b>\r\nSecond\r\n"),
        "{out:?}"
    );
    // untouched neighbours keep their original bytes
    assert!(out.starts_with("1\r\n00:00:01,000 --> 00:00:03,000\r\nHello world\r\n\r\n"));
    assert!(out.ends_with("3\r\n00:01:02,250 --> 00:01:04,000\r\nThird cue\r\n"));
}

#[test]
fn utf16_input_is_written_as_utf8_without_bom() {
    let doc = SubtitleDoc::load(&fixture("utf16le_bom.srt")).unwrap();
    let out = doc.to_bytes();
    assert_eq!(out, b"1\n00:00:01,000 --> 00:00:02,000\nHello\n".to_vec());
}

#[test]
fn save_writes_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.srt");
    let doc = SubtitleDoc::load(&fixture("basic.srt")).unwrap();
    doc.save(&path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), fixture_bytes("basic.srt"));
}

#[test]
fn format_extension_helpers() {
    assert_eq!(Format::from_extension("SRT"), Some(Format::Srt));
    assert_eq!(Format::from_extension(".ass"), Some(Format::Ass));
    assert_eq!(Format::from_extension("ssa"), Some(Format::Ssa));
    assert_eq!(Format::from_extension("vtt"), Some(Format::Vtt));
    assert_eq!(Format::from_extension("txt"), None);
    assert_eq!(
        Format::from_path(std::path::Path::new("/a b/ملف.Srt")),
        Some(Format::Srt)
    );
    assert_eq!(Format::Vtt.extension(), "vtt");
}

#[test]
fn content_detection_beats_hint_and_hint_resolves_ambiguity() {
    // content says SRT even though hint says VTT -> hint only used when ambiguous, but
    // plain SRT text is the fallback so Srt wins unless hint is given for a signature-less file.
    let srt = fixture_bytes("basic.srt");
    assert_eq!(
        SubtitleDoc::parse(&srt, Some(Format::Srt)).unwrap().format,
        Format::Srt
    );
    // Signatures route to their own parsers, whatever the hint says.
    let ass = b"[Script Info]\nScriptType: v4.00+\n";
    assert_eq!(
        SubtitleDoc::parse(ass, Some(Format::Srt)).unwrap().format,
        Format::Ass
    );
    // WebVTT is not implemented yet.
    let vtt = b"WEBVTT\n\n00:00.000 --> 00:01.000\nx\n";
    assert!(matches!(
        SubtitleDoc::parse(vtt, None),
        Err(SubtitleError::UnsupportedFormat(_))
    ));
}
