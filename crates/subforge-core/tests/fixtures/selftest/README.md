# Self-test goldens

Expected outputs for `subforge --self-test` (`crates/subforge-core/src/selftest.rs`). The
self-test copies `media/sample.mkv`, `media/speech_en_10s.opus`, `subtitles/basic.srt` and this
folder into `<out>/مجلد اختبار/` and compares its outputs with these files (line endings and a
leading BOM are normalised first).

| File | Produced from |
|---|---|
| `ara.srt` | track 2 of `sample.mkv` (ASS, extracted with mkvextract) converted to SRT |
| `ara.vtt` | `ara.srt` converted to WebVTT |
| `basic.shift.srt` | `subtitles/basic.srt` shifted by +1500 ms |
| `basic.stretch.srt` | `subtitles/basic.srt` stretched from 25 to 23.976 fps |
| `ara.translated.ass` | track 2 through `extract_lines`/`rebuild_lines` with the stub translator (uppercase + `" [AR]"`) and the RTL fix |
| `eng.translated.srt` | track 3 (SRT) through the same stub translation |

They were generated once by the self-test itself (`cargo test --test selftest -- --include-ignored`
with `SUBFORGE_SELFTEST_OUT` set) and checked by hand. Regenerate only for an intentional change
in the subtitle writers, and review the diff.
