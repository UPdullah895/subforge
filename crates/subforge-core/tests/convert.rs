use std::path::PathBuf;

use subforge_core::subtitle::{
    convert_file, shift_file, stretch_file, Cue, Format, SubtitleDoc, SubtitleError,
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/subtitles")
        .join(name)
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).unwrap()
}

fn is_drawing(text: &str) -> bool {
    text.contains("\\p1") || text.contains("\\p2")
}

fn visible(doc: &SubtitleDoc) -> Vec<&Cue> {
    doc.cues
        .iter()
        .filter(|c| !c.comment && !is_drawing(&c.text))
        .collect()
}

#[test]
fn vtt_roundtrip() {
    let bytes = fixture_bytes("basic.vtt");
    let doc = SubtitleDoc::parse(&bytes, Some(Format::Vtt)).unwrap();
    assert_eq!(doc.format, Format::Vtt);
    // Unedited cues are written back verbatim (NOTE blocks are not kept).
    let out = String::from_utf8(doc.to_bytes()).unwrap();
    assert!(
        out.starts_with("WEBVTT - basic sample\n\n1\n00:00:01.000 --> 00:00:03.000\nHello world\n")
    );
    assert!(out.contains("intro\n00:03.500 --> 00:05.000 align:start position:10%\n"));
    let again = SubtitleDoc::parse(out.as_bytes(), None).unwrap();
    assert_eq!(again.format, Format::Vtt);
    assert_eq!(again.cues.len(), 3);
    assert_eq!(again.to_bytes(), out.as_bytes());
}

#[test]
fn vtt_parses_cue_settings_and_ignores_note_blocks() {
    let doc = SubtitleDoc::parse(&fixture_bytes("basic.vtt"), None).unwrap();
    assert_eq!(doc.cues.len(), 3);
    assert_eq!((doc.cues[0].start_ms, doc.cues[0].end_ms), (1000, 3000));
    assert_eq!((doc.cues[1].start_ms, doc.cues[1].end_ms), (3500, 5000));
    assert_eq!(doc.cues[1].text, "{\\i1}Line one{\\i0}\\NLine & two");
    assert_eq!(doc.cues[2].start_ms, 62_250);
    assert_eq!(doc.cues[2].text, "Third cue");
    assert!(doc.cues.iter().all(|c| !c.text.contains("NOTE")));

    // Settings and identifier survive an edit of the times.
    let mut doc = doc;
    doc.shift(1000);
    let out = String::from_utf8(doc.to_bytes()).unwrap();
    assert!(out.contains("intro\n00:00:04.500 --> 00:00:06.000 align:start position:10%\n"));
    assert!(out.contains("<i>Line one</i>\nLine &amp; two"));
}

#[test]
fn vtt_writer_escapes_markup_and_skips_comments() {
    let mut doc = SubtitleDoc::parse(&fixture_bytes("basic.vtt"), None).unwrap();
    let mut note = Cue::new(3000, 4000, "note");
    note.comment = true;
    doc.cues = vec![
        Cue::new(0, 1000, "a < b & {\\b1}c{\\b0} --> d"),
        Cue::new(2000, 3000, "x\\N\\Ny"),
        note,
    ];
    let out = String::from_utf8(doc.to_bytes()).unwrap();
    assert_eq!(
        out,
        "WEBVTT - basic sample\n\n00:00:00.000 --> 00:00:01.000\na &lt; b &amp; <b>c</b> --&gt; d\n\n00:00:02.000 --> 00:00:03.000\nx\ny\n"
    );
}

// CHANGELOG "Intentional differences": WebVTT NOTE/STYLE/REGION blocks are dropped on write.
#[test]
fn vtt_style_and_region_blocks_are_dropped() {
    let src = "WEBVTT\n\nSTYLE\n::cue { color: yellow; }\n\nREGION\nid:fred\nwidth:40%\n\n\
               NOTE a comment\n\n00:00:01.000 --> 00:00:02.000\nHello\n\n\
               00:00:03.000 --> 00:00:04.000\nWorld\n";
    let doc = SubtitleDoc::parse(src.as_bytes(), None).unwrap();
    assert_eq!(doc.cues.len(), 2);
    let out = String::from_utf8(doc.to_bytes()).unwrap();
    for gone in ["STYLE", "::cue", "REGION", "id:fred", "NOTE"] {
        assert!(!out.contains(gone), "{gone} survived:\n{out}");
    }
    assert!(
        out.contains("00:00:01.000 --> 00:00:02.000\nHello"),
        "{out}"
    );
}

// CHANGELOG: on edited cues, inline <c>, <lang>, ruby and timestamp tags are dropped
// (their text stays); speaker <v> labels are kept.
#[test]
fn vtt_lang_ruby_and_timestamps_are_dropped_on_edited_cues() {
    let src = "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\n\
               <v Anna><lang en>Hello</lang> <ruby>漢<rt>kan</rt></ruby> <00:00:01.500><c.loud>now</c></v>\n";
    let mut doc = SubtitleDoc::parse(src.as_bytes(), None).unwrap();
    // Untouched: written back verbatim.
    assert!(String::from_utf8(doc.to_bytes())
        .unwrap()
        .contains("<lang en>"));
    doc.shift(1000);
    let out = String::from_utf8(doc.to_bytes()).unwrap();
    for gone in [
        "<lang",
        "</lang>",
        "<ruby>",
        "<rt>",
        "<00:00:01.500>",
        "<c.loud>",
    ] {
        assert!(!out.contains(gone), "{gone} survived:\n{out}");
    }
    assert!(out.contains("<v Anna>"), "{out}");
    assert!(out.contains("Hello"), "{out}");
    assert!(out.contains("漢kan"), "{out}");
    assert!(out.contains("now"), "{out}");
}

// CHANGELOG: `{\p1 }`/`{\p1abc}` are not drawings and `{\p01}` is (pysubs2's regex differed).
#[test]
fn drawing_detection_edge_cases() {
    use subforge_core::lines::is_drawing;
    assert!(!is_drawing(r"{\p1 }m 0 0 l 1 1"));
    assert!(!is_drawing(r"{\p1abc}m 0 0 l 1 1"));
    assert!(is_drawing(r"{\p01}m 0 0 l 1 1"));
    assert!(!is_drawing(r"{\p0}text"));
}

#[test]
fn signature_less_vtt_needs_the_hint() {
    let body = b"00:00:00.000 --> 00:00:01.000\nx\n";
    assert_eq!(SubtitleDoc::parse(body, None).unwrap().format, Format::Srt);
    let doc = SubtitleDoc::parse(body, Some(Format::Vtt)).unwrap();
    assert_eq!(doc.format, Format::Vtt);
    assert_eq!(doc.cues.len(), 1);
}

#[test]
fn convert_matrix() {
    let sources = ["basic.srt", "basic.vtt", "arabic_full.ass", "legacy.ssa"];
    let targets = [Format::Srt, Format::Ass, Format::Ssa, Format::Vtt];
    for src in sources {
        let doc =
            SubtitleDoc::parse(&fixture_bytes(src), Format::from_path(&fixture(src))).unwrap();
        for to in targets {
            let converted = doc.convert(to);
            assert_eq!(converted.format, to);
            let bytes = converted.to_bytes();
            let again = SubtitleDoc::parse(&bytes, Some(to))
                .unwrap_or_else(|e| panic!("{src} -> {to:?} does not re-parse: {e}"));
            assert_eq!(again.format, to, "{src} -> {to:?}");
            let (a, b) = (visible(&doc), visible(&again));
            assert_eq!(a.len(), b.len(), "{src} -> {to:?} cue count");
            for (x, y) in a.iter().zip(&b) {
                assert!(
                    (x.start_ms - y.start_ms).abs() <= 10,
                    "{src} -> {to:?} start"
                );
                assert!((x.end_ms - y.end_ms).abs() <= 10, "{src} -> {to:?} end");
            }
        }
    }
}

#[test]
fn srt_to_ass_uses_the_pysubs2_default_header() {
    let doc = SubtitleDoc::parse(&fixture_bytes("basic.srt"), None).unwrap();
    let ass = String::from_utf8(doc.convert(Format::Ass).to_bytes()).unwrap();
    assert!(ass.contains("ScriptType: v4.00+\nPlayResX: 384\nPlayResY: 288\n"));
    assert!(ass.contains(
        "Style: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1"
    ));
    assert!(ass.contains("Dialogue: 0,0:00:01.00,0:00:03.00,Default,,0,0,0,,Hello world"));
    let ssa = String::from_utf8(doc.convert(Format::Ssa).to_bytes()).unwrap();
    assert!(ssa.contains("ScriptType: v4.00\nPlayResX: 384\nPlayResY: 288\n"));
    assert!(ssa.contains("Style: Default,Arial,20,16777215,255,0,0,0,0,1,2,2,2,10,10,10,0,1"));
    assert!(ssa.contains("Dialogue: Marked=0,0:00:01.00,"));
}

#[test]
fn ass_to_ssa_keeps_styles() {
    let doc = SubtitleDoc::parse(&fixture_bytes("arabic_full.ass"), None).unwrap();
    let ssa = String::from_utf8(doc.convert(Format::Ssa).to_bytes()).unwrap();
    assert!(ssa.contains("[V4 Styles]"));
    assert!(ssa.contains("ScriptType: v4.00\n"));
    assert!(ssa.contains("Format: Marked, Start,"));
}

#[test]
fn convert_drops_comments_and_drawings_for_text_formats() {
    let mut doc = SubtitleDoc::parse(&fixture_bytes("basic.srt"), None)
        .unwrap()
        .convert(Format::Ass);
    let mut note = Cue::new(0, 10, "note");
    note.comment = true;
    doc.cues.push(note);
    doc.cues.push(Cue::new(0, 10, "{\\p1}m 0 0 l 1 1{\\p0}"));
    let n = doc.cues.len();
    for to in [Format::Srt, Format::Vtt] {
        assert_eq!(doc.convert(to).cues.len(), n - 2);
    }
    assert_eq!(doc.convert(Format::Ssa).cues.len(), n);
}

#[test]
fn ass_to_srt_keeps_italics_drops_positioning() {
    let mut doc = SubtitleDoc::parse(&fixture_bytes("basic.srt"), None)
        .unwrap()
        .convert(Format::Ass);
    doc.cues = vec![Cue::new(0, 1000, "{\\an8}{\\i1}Hi{\\i0}")];
    for to in [Format::Srt, Format::Vtt] {
        let out = String::from_utf8(doc.convert(to).to_bytes()).unwrap();
        assert!(out.contains("<i>Hi</i>"), "{out}");
        assert!(!out.contains("an8"));
    }
}

#[test]
fn shift_negative_clamps_at_zero() {
    let mut doc = SubtitleDoc::parse(&fixture_bytes("basic.srt"), None).unwrap();
    doc.cues = vec![Cue::new(500, 1500, "x"), Cue::new(5000, 6000, "y")];
    doc.shift(-1000);
    assert_eq!((doc.cues[0].start_ms, doc.cues[0].end_ms), (0, 500));
    assert_eq!((doc.cues[1].start_ms, doc.cues[1].end_ms), (4000, 5000));
    doc.shift(-10_000);
    assert_eq!((doc.cues[0].start_ms, doc.cues[0].end_ms), (0, 0));
}

#[test]
fn stretch_25_to_23976() {
    let mut doc = SubtitleDoc::parse(&fixture_bytes("basic.srt"), None).unwrap();
    doc.cues = vec![Cue::new(60_000, 120_000, "x")];
    doc.stretch(25.0, 23.976).unwrap();
    assert_eq!(
        doc.cues[0].start_ms,
        (60000.0f64 * 25.0 / 23.976).round() as i64
    );
    assert_eq!(doc.cues[0].start_ms, 62_563);
}

#[test]
fn stretch_rejects_zero_fps() {
    let mut doc = SubtitleDoc::parse(&fixture_bytes("basic.srt"), None).unwrap();
    for (a, b) in [
        (0.0, 25.0),
        (25.0, 0.0),
        (-1.0, 25.0),
        (f64::NAN, 25.0),
        (25.0, f64::INFINITY),
    ] {
        assert!(
            matches!(doc.stretch(a, b), Err(SubtitleError::InvalidFps)),
            "{a} {b}"
        );
    }
    assert_eq!(doc.cues[0].start_ms, 1000);
}

#[test]
fn file_helpers_convert_shift_stretch() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out dir مرحبا.vtt");
    convert_file(&fixture("basic.srt"), &out, Format::Vtt).unwrap();
    assert!(std::fs::read_to_string(&out)
        .unwrap()
        .starts_with("WEBVTT\n"));

    let shifted = dir.path().join("shifted.srt");
    shift_file(&fixture("basic.srt"), &shifted, 500).unwrap();
    assert_eq!(SubtitleDoc::load(&shifted).unwrap().cues[0].start_ms, 1500);

    let stretched = dir.path().join("stretched.srt");
    stretch_file(&fixture("basic.srt"), &stretched, 25.0, 50.0).unwrap();
    assert_eq!(SubtitleDoc::load(&stretched).unwrap().cues[0].end_ms, 1500);
    assert!(matches!(
        stretch_file(&fixture("basic.srt"), &stretched, 0.0, 25.0),
        Err(SubtitleError::InvalidFps)
    ));
}

#[test]
fn vtt_voice_tag_survives_edit() {
    let src = b"WEBVTT\n\n00:00:01.000 --> 00:00:02.000\n<v Mary>old &amp; text</v>\n\n00:00:03.000 --> 00:00:04.000\n<v.loud Bob>hey</v>\n";
    let mut doc = SubtitleDoc::parse(src, None).unwrap();
    assert_eq!(doc.cues[0].text, "old & text");
    doc.cues[0].text = "new text".into();
    doc.cues[1].text = "yo\\Nthere".into();
    let out = String::from_utf8(doc.to_bytes()).unwrap();
    assert!(out.contains("<v Mary>new text</v>"), "{out}");
    assert!(out.contains("<v.loud Bob>yo\nthere</v>"), "{out}");
    let ass = String::from_utf8(doc.convert(Format::Ass).to_bytes()).unwrap();
    assert!(!ass.contains("<v"));
}

#[test]
fn vtt_lt_three_is_text() {
    let src = b"WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nI <3 you <00:00:01.500>so\n";
    let doc = SubtitleDoc::parse(src, None).unwrap();
    assert_eq!(doc.cues[0].text, "I <3 you so");
}

#[test]
fn vtt_class_suffix_on_italic() {
    let src = b"WEBVTT\n\n00:00:01.000 --> 00:00:02.000\n<i.foo>a</i> <b.bar>b</b> <u.baz>c</u>\n";
    let doc = SubtitleDoc::parse(src, None).unwrap();
    assert_eq!(
        doc.cues[0].text,
        "{\\i1}a{\\i0} {\\b1}b{\\b0} {\\u1}c{\\u0}"
    );
}
