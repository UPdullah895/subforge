//! Cleaning must not disturb anything in an ASS file except the removed events:
//! styles, `[Fonts]`, `[Graphics]`, comments and unknown sections stay.

use subforge_core::subtitle::clean::{clean, preview, CleanOptions};
use subforge_core::subtitle::{Cue, SubtitleDoc};

fn fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/subtitles")
        .join(name)
}

fn dialogue_lines(text: &str) -> usize {
    text.lines()
        .filter(|l| l.starts_with("Dialogue:") || l.starts_with("Comment:"))
        .count()
}

#[test]
fn removing_an_added_credit_restores_the_original_file() {
    let mut doc = SubtitleDoc::load(&fixture("arabic_full.ass")).unwrap();
    let original = doc.to_bytes();
    let last_end = doc.cues.iter().map(|c| c.end_ms).max().unwrap_or(0);
    doc.cues
        .push(Cue::new(last_end + 1000, last_end + 4000, "ترجمة: فريق المثال"));

    let report = preview(&doc, &CleanOptions::default());
    assert_eq!(report.removed.len(), 1);

    let removed = clean(&mut doc, &CleanOptions::default());
    assert_eq!(removed.removed.len(), 1);
    // The appended cue was inserted after the last event slot, so once removed
    // the serialised file is byte-for-byte the source again.
    assert_eq!(doc.to_bytes(), original);
}

#[test]
fn removing_an_existing_event_keeps_the_rest_of_the_file_intact() {
    let mut doc = SubtitleDoc::load(&fixture("arabic_full.ass")).unwrap();
    let original = doc.to_bytes();
    let original_text = String::from_utf8(original.clone()).unwrap();
    let before = dialogue_lines(&original_text);

    // Turn cue 0 into a credit card without touching anything else.
    doc.cues[0].text = "www.example.com".to_string();
    let report = clean(&mut doc, &CleanOptions::default());
    assert_eq!(report.removed.len(), 1);
    assert_eq!(report.removed[0].index, 0);

    let text = String::from_utf8(doc.to_bytes()).unwrap();
    assert_eq!(dialogue_lines(&text), before - 1);
    assert!(!text.contains("www.example.com"));
    // Not just the last line: the following cue must still be there.
    for section in ["[Script Info]", "[V4+ Styles]", "[Fonts]", "[Graphics]", "[Events]"] {
        assert!(text.contains(section), "missing {section}");
    }
    assert_eq!(
        text.matches("Style: ").count(),
        original_text.matches("Style: ").count()
    );
}

#[test]
fn a_comment_cue_is_kept_even_when_it_looks_like_an_ad() {
    let mut doc = SubtitleDoc::load(&fixture("arabic_full.ass")).unwrap();
    let comment_index = doc.cues.iter().position(|c| c.comment).unwrap();
    doc.cues[comment_index].text = "translated by nobody".to_string();
    let report = clean(&mut doc, &CleanOptions::default());
    assert!(!report.removed.iter().any(|c| c.index == comment_index));
}

#[test]
fn ass_drawing_cues_are_ignored() {
    let mut doc = SubtitleDoc::load(&fixture("arabic_full.ass")).unwrap();
    let index = doc.cues.len();
    doc.cues.push(Cue::new(0, 1000, "{\\p1}m 0 0 l 100 0 100 100{\\p0}"));
    let report = clean(&mut doc, &CleanOptions::default());
    assert!(!report.removed.iter().any(|c| c.index == index));
}
