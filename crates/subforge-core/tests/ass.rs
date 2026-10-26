use std::path::PathBuf;

use subforge_core::subtitle::{Cue, Format, SubtitleDoc, SubtitleError};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/subtitles")
        .join(name)
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).unwrap()
}

fn lines(bytes: &[u8]) -> Vec<String> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .split('\n')
        .map(str::to_owned)
        .collect()
}

#[test]
fn ass_roundtrip_byte_identical() {
    let original = fixture_bytes("arabic_full.ass");
    let doc = SubtitleDoc::load(&fixture("arabic_full.ass")).unwrap();
    assert_eq!(doc.format, Format::Ass);
    assert_eq!(doc.cues.len(), 6);
    assert_eq!(doc.to_bytes(), original);
}

#[test]
fn ssa_roundtrip_byte_identical() {
    // legacy.ssa has no trailing newline: that must survive too.
    let original = fixture_bytes("legacy.ssa");
    let doc = SubtitleDoc::load(&fixture("legacy.ssa")).unwrap();
    assert_eq!(doc.format, Format::Ssa);
    assert_eq!(doc.cues.len(), 3);
    assert_eq!(doc.cues[0].text, "Hello, world");
    assert_eq!(doc.to_bytes(), original);
}

#[test]
fn ass_mixed_eol_normalised_to_majority() {
    let original = fixture_bytes("mixed_eol.ass");
    let text = String::from_utf8(original.clone()).unwrap();
    let crlf = text.matches("\r\n").count();
    let bare_lf = text.matches('\n').count() - crlf;
    assert!(crlf > bare_lf && bare_lf > 0, "fixture must mix endings");

    let out = SubtitleDoc::parse(&original, Some(Format::Ass))
        .unwrap()
        .to_bytes();
    let expected = text.replace("\r\n", "\n").replace('\n', "\r\n");
    assert_eq!(String::from_utf8(out).unwrap(), expected);
}

#[test]
fn ass_comment_lines_flagged() {
    let doc = SubtitleDoc::load(&fixture("arabic_full.ass")).unwrap();
    let flags: Vec<bool> = doc.cues.iter().map(|c| c.comment).collect();
    assert_eq!(flags, [false, false, true, false, false, false]);
}

#[test]
fn ass_cues_expose_times_and_text() {
    let doc = SubtitleDoc::load(&fixture("arabic_full.ass")).unwrap();
    assert_eq!((doc.cues[0].start_ms, doc.cues[0].end_ms), (1000, 3500));
    assert_eq!(doc.cues[0].text, "مرحبا بالعالم");
    // Text keeps its commas and override tags; "\N" stays the hard break.
    assert_eq!(doc.cues[1].text, "كيف حالك اليوم؟\\Nهذه ترجمة تجريبية");
    assert!(doc.cues[3].text.starts_with("{\\an8\\pos(640,60)}"));
    assert!(doc.cues[3].text.contains("{\\i1}"));
    assert!(doc.cues[4].text.starts_with("{\\p1}"));
    // "Dialogue:0,..." without a space and a nonzero hour/minute still parse.
    assert_eq!(doc.cues[5].start_ms, 62_100);
}

#[test]
fn ass_edit_one_cue_only_changes_that_line() {
    let original = fixture_bytes("arabic_full.ass");
    let mut doc = SubtitleDoc::load(&fixture("arabic_full.ass")).unwrap();
    doc.cues[3].text = "{\\an8}نص جديد".to_owned();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edited.ass");
    doc.save(&path).unwrap();
    let edited = std::fs::read(&path).unwrap();

    let (a, b) = (lines(&original), lines(&edited));
    assert_eq!(a.len(), b.len());
    let changed: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
    assert_eq!(changed.len(), 1, "changed lines: {changed:?}");
    // Only the text was edited, so layer, style, margins and effect survive verbatim.
    assert_eq!(
        b[changed[0]],
        "Dialogue: 1,0:00:07.00,0:00:09.99,Top,,10,10,15,,{\\an8}نص جديد"
    );
}

#[test]
fn ass_time_format() {
    let mut doc = SubtitleDoc::load(&fixture("legacy.ssa")).unwrap();
    doc.cues[0].start_ms = 3_723_450;
    doc.cues[0].end_ms = 3_724_000;
    doc.cues[1].start_ms = 1005; // 100.5 cs rounds half up
    doc.cues[1].end_ms = 1004;
    let out = String::from_utf8(doc.to_bytes()).unwrap();
    assert!(
        out.contains("Dialogue: Marked=0,1:02:03.45,1:02:04.00,Default"),
        "{out}"
    );
    assert!(
        out.contains("Comment: Marked=0,0:00:01.01,0:00:01.00,Default"),
        "{out}"
    );
}

#[test]
fn windows_1256_ass_with_fonts_roundtrips_to_utf8() {
    let bytes = fixture_bytes("arabic_fonts_cp1256.ass");
    assert!(
        std::str::from_utf8(&bytes).is_err(),
        "fixture must not be UTF-8"
    );
    let doc = SubtitleDoc::parse(&bytes, Some(Format::Ass)).unwrap();
    assert_eq!(doc.encoding.name, "windows-1256");

    // Edit one cue so the file is genuinely rebuilt, not just echoed.
    let mut doc = doc;
    doc.cues[0].text.push_str(" !");
    let out = doc.to_bytes();
    let out = String::from_utf8(out).expect("output must be valid UTF-8");

    // The [Fonts] block is ASCII, so it must be byte-for-byte what the input had.
    let block = |s: &str| {
        let start = s.find("[Fonts]").expect("[Fonts] present");
        let rest = &s[start..];
        let end = rest[1..].find("\n[").map_or(rest.len(), |i| i + 1);
        rest[..end].to_owned()
    };
    let (decoded_in, _) = subforge_core::subtitle::encoding::decode(&bytes);
    assert_eq!(block(&out), block(&decoded_in));
    assert!(block(&out).contains("fontname: Tiny_0.ttf"));

    // The Arabic text is the same text as in the UTF-8 fixture.
    let reference = SubtitleDoc::load(&fixture("arabic_full.ass")).unwrap();
    let reparsed = SubtitleDoc::parse(out.as_bytes(), Some(Format::Ass)).unwrap();
    assert_eq!(reparsed.cues.len(), reference.cues.len());
    assert_eq!(
        reparsed.cues[0].text,
        format!("{} !", reference.cues[0].text)
    );
    for (a, b) in reparsed.cues.iter().zip(&reference.cues).skip(1) {
        assert_eq!(a.text, b.text);
    }
}

#[test]
fn windows_1256_ass_without_fonts_decodes_arabic() {
    let doc = SubtitleDoc::load(&fixture("arabic_full_cp1256.ass")).unwrap();
    assert_eq!(doc.encoding.name, "windows-1256");
    assert_eq!(doc.cues[0].text, "مرحبا بالعالم");
    let out = doc.to_bytes();
    assert!(std::str::from_utf8(&out).is_ok());
}

#[test]
fn utf8_fonts_section_is_preserved_when_a_cue_is_edited() {
    let original = fixture_bytes("arabic_full.ass");
    let mut doc = SubtitleDoc::parse(&original, None).unwrap();
    doc.cues[0].text = "x".into();
    let out = doc.to_bytes();
    let (a, b) = (lines(&original), lines(&out));
    assert_eq!(a.iter().zip(&b).filter(|(x, y)| x != y).count(), 1);
}

#[test]
fn added_and_removed_cues_are_written_inside_events() {
    let mut doc = SubtitleDoc::load(&fixture("arabic_full.ass")).unwrap();
    doc.cues.remove(0);
    doc.cues.push(Cue::new(20_000, 21_000, "new"));
    let out = String::from_utf8(doc.to_bytes()).unwrap();
    let events = out.find("[Events]").unwrap();
    let fonts = out.find("[Fonts]").unwrap();
    let added = out
        .find("Dialogue: 0,0:00:20.00,0:00:21.00,Default,,0,0,0,,new")
        .unwrap();
    assert!(
        events < added && added < fonts,
        "new cue must land in [Events]"
    );
    assert!(!out.contains("0:00:01.00,0:00:03.50"));
}

#[test]
fn bad_time_names_the_line() {
    let src = "[Script Info]\nScriptType: v4.00+\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,oops,0:00:02.00,Default,,0,0,0,,hi\n";
    let err = SubtitleDoc::parse(src.as_bytes(), None).unwrap_err();
    assert!(matches!(err, SubtitleError::Parse { line: 6, .. }), "{err}");
    assert!(err.to_string().contains("oops"), "{err}");
}

#[test]
fn to_bytes_follows_the_public_format_field() {
    // ASS document switched to SRT is written as SRT, not silently as ASS.
    let mut doc = SubtitleDoc::load(&fixture("legacy.ssa")).unwrap();
    doc.format = Format::Srt;
    let srt = String::from_utf8(doc.to_bytes()).unwrap();
    assert!(
        srt.starts_with("1\n00:00:01,000 --> 00:00:03,000\nHello, world"),
        "{srt}"
    );

    // SRT document switched to ASS gets a generated ASS header.
    let mut doc = SubtitleDoc::load(&fixture("basic.srt")).unwrap();
    doc.format = Format::Ass;
    let ass = String::from_utf8(doc.to_bytes()).unwrap();
    assert!(
        ass.starts_with("[Script Info]\nScriptType: v4.00+"),
        "{ass}"
    );
    assert!(ass.contains("[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:03.00,Default,,0,0,0,,Hello world"), "{ass}");
    let back = SubtitleDoc::parse(ass.as_bytes(), None).unwrap();
    assert_eq!(back.format, Format::Ass);
    assert_eq!(back.cues.len(), doc.cues.len());
}

#[test]
fn ass_switched_to_ssa_is_rewritten_in_the_ssa_dialect() {
    let mut doc = SubtitleDoc::load(&fixture("arabic_full.ass")).unwrap();
    doc.format = Format::Ssa;
    let bytes = doc.to_bytes();
    let out = String::from_utf8(bytes.clone()).unwrap();
    let l = lines(&bytes);
    assert!(l.contains(&"ScriptType: v4.00".to_owned()), "{out}");
    assert!(!out.contains("v4.00+") && !out.contains("[V4+ Styles]"));
    assert!(l.contains(&"[V4 Styles]".to_owned()));
    assert!(l.contains(&"Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, TertiaryColour, BackColour, Bold, Italic, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, AlphaLevel, Encoding".to_owned()));
    // Colours become decimal BGR; the top-aligned style (ASS 8) becomes SSA 6.
    assert!(
        l.contains(
            &"Style: Default,Noto Naskh Arabic,48,16777215,255,0,0,0,0,1,2,1,2,30,30,25,0,178"
                .to_owned()
        ),
        "{out}"
    );
    assert!(
        l.contains(
            &"Style: Top,Noto Naskh Arabic,40,65535,255,0,0,-1,0,1,2,0,6,30,30,25,0,178".to_owned()
        ),
        "{out}"
    );
    assert!(l.contains(
        &"Format: Marked, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text"
            .to_owned()
    ));
    assert!(
        l.contains(
            &"Dialogue: Marked=0,0:00:01.00,0:00:03.50,Default,,0,0,0,,مرحبا بالعالم".to_owned()
        ),
        "{out}"
    );
    // Fonts, graphics and unknown sections are untouched.
    for keep in [
        "[Fonts]",
        "[Graphics]",
        "[Aegisub Project Garbage]",
        "fontname: NotoNaskhArabic_0.ttf",
    ] {
        assert!(l.contains(&keep.to_owned()), "{keep}");
    }
    let back = SubtitleDoc::parse(&bytes, None).unwrap();
    assert_eq!(back.format, Format::Ssa);
    assert_eq!(back.cues.len(), 6);
    assert_eq!(back.cues[3].text, doc.cues[3].text);
}

#[test]
fn ssa_switched_to_ass_is_rewritten_in_the_ass_dialect() {
    let mut doc = SubtitleDoc::load(&fixture("legacy.ssa")).unwrap();
    doc.format = Format::Ass;
    let bytes = doc.to_bytes();
    let out = String::from_utf8(bytes.clone()).unwrap();
    let l = lines(&bytes);
    assert!(l.contains(&"ScriptType: v4.00+".to_owned()), "{out}");
    assert!(l.contains(&"[V4+ Styles]".to_owned()));
    assert!(!out.contains("[V4 Styles]"));
    assert!(l.contains(&"Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding".to_owned()));
    assert!(l.contains(&"Style: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H0000FFFF,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,30,30,30,0".to_owned()), "{out}");
    assert!(l.contains(
        &"Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text"
            .to_owned()
    ));
    assert!(
        l.contains(
            &"Dialogue: 0,0:00:01.00,0:00:03.00,Default,,0000,0000,0000,,Hello, world".to_owned()
        ),
        "{out}"
    );
    assert!(
        l.contains(&"Comment: 0,0:00:03.00,0:00:04.00,Default,,0000,0000,0000,,A note".to_owned()),
        "{out}"
    );
    assert_eq!(
        SubtitleDoc::parse(&bytes, None).unwrap().format,
        Format::Ass
    );
}

#[test]
fn switching_dialect_and_back_restores_the_original_cue_lines() {
    let original = fixture_bytes("legacy.ssa");
    let mut doc = SubtitleDoc::parse(&original, None).unwrap();
    doc.format = Format::Ass;
    doc.format = Format::Ssa;
    assert_eq!(doc.to_bytes(), original);
}

#[test]
fn srt_output_skips_comment_cues() {
    let mut doc = SubtitleDoc::load(&fixture("arabic_full.ass")).unwrap();
    doc.format = Format::Srt;
    let srt = String::from_utf8(doc.to_bytes()).unwrap();
    assert!(!srt.contains("ملاحظة المترجم"));
    assert!(srt.starts_with("1\n00:00:01,000"));
    assert!(srt.contains("\n\n5\n00:01:02,100"), "{srt}");
}
