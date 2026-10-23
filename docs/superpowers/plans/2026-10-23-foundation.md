# Foundation (Self-Contained SubForge) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** SubForge works out of the box on Windows and Linux. It has no Python, it bundles FFmpeg and MKVToolNix, shares one Rust core between the desktop app and the server, never freezes the UI, and every change is checked by an installed-build end-to-end test on Windows CI.

**Architecture:** A new crate `crates/subforge-core` owns the subtitle engine (SRT/ASS/SSA/VTT, lossless ASS), the translation line pipeline, tool resolution plus the FFmpeg/MKVToolNix wrappers, the online translation providers, and whisper.cpp transcription with model downloads. `src-tauri` and `server` become thin adapters. Each adapter builds a `ToolPaths` from its own config and bundled-tools directory, then maps core progress callbacks to its job events. Release builds bundle pinned FFmpeg and MKVToolNix binaries into `<resources>/bin/`. A hidden `--self-test` mode in the app binary drives the e2e CI jobs.

**Tech Stack:** Rust 2021, Tauri 2, axum 0.8, tokio, reqwest 0.12, `encoding_rs` + `chardetng` (encoding detection), `whisper-rs` (whisper.cpp bindings), `tokio-util` `CancellationToken`, Vue 3 + Pinia + vue-i18n (en, ar), vitest, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-10-23-foundation-design.md`

## Global Constraints

- License is GPL-3.0-only. Bundled FFmpeg must be a GPL or LGPL build. Bundled tools ship their license texts.
- Platforms: Windows x64 and Linux x64 are first-class. macOS must still `cargo check`.
- No Python anywhere. No interpreter, venv, pip or `scripts/*.py` may ship or be invoked.
- Existing Tauri command names and REST routes stay backward compatible unless a task says otherwise. The frontend calls go through `apiInvoke`.
- Existing `config.json` must keep loading. `src-tauri/src/config.rs` and `server/src/config.rs` share one file on disk, so every new field must be added to **both** with `#[serde(default)]`.
- Tool resolution order: **custom path (error if invalid, never silent fallback) → bundled → system PATH / well-known locations**.
- Subtitle formats: SRT, ASS, SSA, WebVTT. Output is always UTF-8.
- An ASS/SSA load→save with no edits is byte-identical, except that mixed line endings are normalised to the file's majority ending.
- Rust subtitle and translation output is "at least as good as Python". Each intentional difference is listed in CHANGELOG with a test.
- Test fixtures are synthetic and redistributable. Embedded fonts are OFL (Noto Naskh Arabic). User media is never committed.
- Whisper models: GGML files from `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-<name>.bin`. VAD model: Silero GGML from the whisper.cpp VAD repository on Hugging Face. Verify the exact URL in Task 9 and record it in code as a single constant.
- Device values: `auto` (default) / `cpu` / `gpu`. A request with `cuda` is treated as `auto`.
- No blocking work on the Tauri main thread. Process spawns, network I/O and large-file work are `async` or go through `spawn_blocking`.
- User-facing errors name the tool or file, what failed, and what to do. Raw stderr goes in a `details` field.
- New UI strings go in both `src/i18n/en.json` and `src/i18n/ar.json`.

## Review Focus

1. **Arabic subtitle in a legacy encoding (Windows-1256) with an ASS `[Fonts]` section** must load, translate-rebuild and save as UTF-8 with the fonts block intact. Test: `windows_1256_ass_with_fonts_roundtrips_to_utf8` (Task 2).
2. **A path containing spaces and Arabic characters** for the input file, the output file, the install directory and the model cache must work for every external tool call. Test: `tools_handle_unicode_space_paths` (Task 5) and the e2e fixture folder (Task 14).
3. **A machine with no Vulkan loader or GPU (the CI runner itself)** must launch the app and transcribe on CPU. Test: the e2e job asserts `device_used == "cpu"` on the runner (Task 14), and `select_device_falls_back_to_cpu` (Task 13).
4. **An interrupted or cancelled model download** must leave no file that `list_models` reports as downloaded. A retry must resume or restart cleanly. Test: `cancelled_download_leaves_no_model` and `resume_appends_from_partial` (Task 9).
5. **A custom tool path that no longer exists** (for example the user uninstalled MKVToolNix) must produce a `CustomPathInvalid` error naming the setting. It must not silently use the bundled copy. Test: `custom_path_missing_is_error_not_fallback` (Task 5).

---

## File Structure

```
crates/subforge-core/
  Cargo.toml
  src/lib.rs                    re-exports; progress + cancel types
  src/progress.rs               Progress callback type, CancellationToken re-export
  src/subtitle/mod.rs           Format, SubtitleDoc, Cue, load/parse/save, convert/shift/stretch, *_file helpers
  src/subtitle/encoding.rs      decode(bytes) -> (String, DetectedEncoding)
  src/subtitle/srt.rs           SRT parse/write
  src/subtitle/ass.rs           ASS/SSA lossless parse/write
  src/subtitle/vtt.rs           WebVTT parse/write
  src/subtitle/markup.rs        ASS-tag <-> HTML-tag conversion between formats
  src/lines.rs                  translation line pipeline (port of subtitle_lines.py)
  src/tools/mod.rs              ToolId, ToolSource, ToolPaths, ResolvedTool, resolve(), ToolError
  src/tools/ffmpeg.rs           merged ffmpeg/ffprobe wrapper
  src/tools/mkvtoolnix.rs       merged mkvmerge/mkvextract/mkvpropedit wrapper
  src/translate.rs              merged provider code (ollama/openai/libre/deepl/google)
  src/whisper/models.rs         model catalog, cache dir, list, download (+ VAD model)
  src/whisper/transcribe.rs     decode audio via ffmpeg, run whisper-rs, write SRT/VTT
  src/whisper/device.rs         GPU detection + device selection
  src/selftest.rs               e2e scenario used by `--self-test`
  tests/fixtures/...            synthetic subtitles + media
  tests/*.rs                    golden/integration tests
packaging/bundled-tools.json    pinned tool versions, URLs, sha256, per platform
scripts/fetch-bundled-tools.mjs downloads + verifies + lays out src-tauri/binaries/bin/
.github/workflows/e2e.yml       Windows + Linux installed-build e2e
```

Deleted by the end: `src-tauri/src/tools/{pysubs2,translate,whisper,ffmpeg,mkvtoolnix}.rs` (replaced by thin adapters in `src-tauri/src/tools/mod.rs`), the same files under `server/src/tools/`, and `scripts/*.py`.

---

### Task 1: `subforge-core` crate, encoding detection, SRT

**Files:**
- Create: `crates/subforge-core/Cargo.toml`, `src/lib.rs`, `src/progress.rs`, `src/subtitle/{mod,encoding,srt,markup}.rs`
- Create: `crates/subforge-core/tests/srt.rs`, `tests/fixtures/subtitles/{basic.srt,arabic_cp1256.srt,utf16le_bom.srt,crlf.srt}`
- Modify: `.github/workflows/ci.yml` (add `crates/subforge-core` to `shared-crates` cache and run `cargo test` there)

**Interfaces:**
- Produces:
  - `pub type ProgressFn<'a> = &'a mut (dyn FnMut(f64, Option<&str>) + Send);` where percent is 0.0–100.0. `pub use tokio_util::sync::CancellationToken;`
  - `pub enum Format { Srt, Ass, Ssa, Vtt }` with `from_extension(&str) -> Option<Format>`, `from_path(&Path) -> Option<Format>` and `extension(self) -> &'static str`
  - `pub struct DetectedEncoding { pub name: &'static str, pub had_bom: bool }`
  - `pub fn encoding::decode(bytes: &[u8]) -> (String, DetectedEncoding)`
  - `pub struct Cue { pub start_ms: i64, pub end_ms: i64, pub text: String, pub comment: bool, /* crate-private: raw line + original values for lossless write, ass fields */ }`. `text` always uses ASS conventions: `\N` hard break, `{...}` override blocks.
  - `pub struct SubtitleDoc { pub format: Format, pub encoding: DetectedEncoding, pub cues: Vec<Cue>, /* crate-private format extras */ }`
  - `pub fn parse(bytes: &[u8], hint: Option<Format>) -> Result<SubtitleDoc, SubtitleError>`. Detection from content: `[Script Info]` means ASS/SSA (`ScriptType: v4.00+` gives ASS, `v4.00` gives SSA), a leading `WEBVTT` means VTT, anything else is SRT. The hint is used only when the content is ambiguous.
  - `pub fn load(path: &Path) -> Result<SubtitleDoc, SubtitleError>`, `SubtitleDoc::to_bytes(&self) -> Vec<u8>`, `SubtitleDoc::save(&self, &Path) -> Result<(), SubtitleError>`
  - `pub enum SubtitleError { Io{path: PathBuf, source: io::Error}, Parse{path: Option<PathBuf>, line: usize, msg: String}, InvalidFps, UnsupportedFormat(String) }`, with a `Display` that names the file and line

- [ ] **Step 1: Write failing tests** in `tests/srt.rs`:
  - `decode_utf8_bom`: `decode(b"\xEF\xBB\xBFhi")` gives `("hi", DetectedEncoding{name:"UTF-8", had_bom:true})`
  - `decode_utf16le_bom`: decodes `utf16le_bom.srt`, and the first cue text equals `"Hello"`
  - `decode_cp1256_arabic`: loading `arabic_cp1256.srt` gives `encoding.name == "windows-1256"`, and the cue text contains `"مرحبا"`
  - `srt_parses_multiline_and_tags`: in `basic.srt`, cue 2 text `== "<i>Line one</i>\\NLine two"` is converted to ASS form by markup rules (`"{\\i1}Line one{\\i0}\\NLine two"`), and `start_ms == 3_500`, `end_ms == 5_000`
  - `srt_tolerates_missing_index_and_extra_blank_lines`
  - `srt_write_roundtrip_crlf`: `crlf.srt` load→`to_bytes` gives identical bytes
- [ ] **Step 2:** Run `cargo test` in `crates/subforge-core`. Expected: compile failure or FAIL.
- [ ] **Step 3: Implement.**
  - `decode`: check for a BOM (UTF-8, UTF-16LE, UTF-16BE). If there is none, accept the bytes as UTF-8 when they are valid UTF-8. Otherwise use `chardetng::EncodingDetector::guess(None, true)` and decode with `encoding_rs`.
  - SRT writer: emit the original raw block for unchanged cues. For changed cues, emit `HH:MM:SS,mmm`.
  - `markup.rs`: `fn html_to_ass(&str) -> String` handles `<i> <b> <u> <s>` and `<font color="#RRGGBB">` (becomes `{\c&HBBGGRR&}`), and turns newline into `\N`. `fn ass_to_html(&str) -> String` handles `\i \b \u \s` toggles, strips every other `{...}` block, turns `\N`/`\n` into a newline and `\h` into a space, and returns an empty string for drawings.
  - Keep the line ending (`\r\n` or `\n`, majority wins) and the UTF-8 BOM flag, so that unchanged documents round-trip.
- [ ] **Step 4:** Run `cargo test`. Expected: all PASS.
- [ ] **Step 5: Commit** with `feat(core): add subforge-core crate with encoding detection and SRT`.

### Task 2: Lossless ASS/SSA

**Files:**
- Create: `crates/subforge-core/src/subtitle/ass.rs`, `tests/ass.rs`
- Create fixtures: `tests/fixtures/subtitles/arabic_full.ass` (Script Info, V4+ Styles, Events with Dialogue + Comment, `\an8`, `\pos`, italics, `\p1` drawing, `[Fonts]` with a uuencoded Noto Naskh Arabic subset, `[Graphics]`, an unknown `[Aegisub Project Garbage]` section), `arabic_full_cp1256.ass` (the same content saved as Windows-1256 without the Fonts section, plus a second file that keeps a short `[Fonts]` block), `legacy.ssa` (v4.00, `Marked=0`), `mixed_eol.ass`, and `fonts/NotoNaskhArabic-OFL.txt`

**Interfaces:**
- Consumes: Task 1 types.
- Produces: the ASS/SSA branch of `parse`, `to_bytes`. The `Events` `Format:` line drives field mapping. Every field except Start/End/Text is kept verbatim in the cue. Unknown sections and any lines that are not events are kept in order.

- [ ] **Step 1: Write failing tests:**
  - `ass_roundtrip_byte_identical`: `to_bytes(load(arabic_full.ass)) == fs::read(arabic_full.ass)`
  - `ssa_roundtrip_byte_identical` for `legacy.ssa`
  - `ass_mixed_eol_normalised_to_majority`
  - `ass_comment_lines_flagged`: the Comment event has `comment == true`
  - `ass_edit_one_cue_only_changes_that_line`: set `cues[3].text`, save, then diff against the original and confirm exactly one line differs
  - `ass_time_format`: a cue at 3 723 450 ms writes as `1:02:03.45`
  - `windows_1256_ass_with_fonts_roundtrips_to_utf8` (Review Focus 1): load the cp1256 file that has `[Fonts]`, then save. The output is valid UTF-8, contains `[Fonts]` with an unchanged block body, and the Arabic text matches the UTF-8 fixture.
- [ ] **Step 2:** Run `cargo test --test ass`. Expected: FAIL.
- [ ] **Step 3: Implement `ass.rs`.** Parse into an ordered list of sections, each of which is either raw lines or the parsed `[Events]`. When writing, emit the raw line for every cue whose start/end/text/comment equal the parsed originals. Otherwise rebuild the line from the `Format:` field order. Times are `H:MM:SS.cc`, rounded to centiseconds.
- [ ] **Step 4:** Run `cargo test`. Expected: all PASS.
- [ ] **Step 5: Commit** with `feat(core): lossless ASS/SSA parse and write`.

### Task 3: WebVTT, cross-format convert, shift, stretch

**Files:**
- Create: `crates/subforge-core/src/subtitle/vtt.rs`, `tests/convert.rs`, `tests/fixtures/subtitles/basic.vtt`
- Modify: `crates/subforge-core/src/subtitle/mod.rs`

**Interfaces:**
- Produces:
  - `SubtitleDoc::convert(&self, to: Format) -> SubtitleDoc`. Converting to SRT or VTT drops comment cues and drawing cues and turns text into HTML-style markup. Converting to ASS or SSA from SRT or VTT creates a default `[Script Info]` (`PlayResX: 384`, `PlayResY: 288`, `ScriptType` set to match) and a single `Default` style equal to pysubs2's default (Arial 20, white primary, black outline 2, shadow 2, alignment 2, margins 10/10/10). ASS to SSA keeps styles, with the format lines translated.
  - `SubtitleDoc::shift(&mut self, ms: i64)`. Times are clamped at 0, and `end_ms >= start_ms` is kept.
  - `SubtitleDoc::stretch(&mut self, from_fps: f64, to_fps: f64) -> Result<(), SubtitleError>`. Every time is multiplied by `from_fps / to_fps` and rounded to ms. A value of zero or less gives `InvalidFps`.
  - `pub fn convert_file(input: &Path, output: &Path, to: Format) -> Result<(), SubtitleError>`, `pub fn shift_file(input: &Path, output: &Path, ms: i64) -> Result<(), SubtitleError>`, and `pub fn stretch_file(input: &Path, output: &Path, from_fps: f64, to_fps: f64) -> Result<(), SubtitleError>`

- [ ] **Step 1: Write failing tests:**
  - `vtt_roundtrip`, `vtt_parses_cue_settings_and_ignores_note_blocks`
  - `convert_matrix`: for each source in {basic.srt, basic.vtt, arabic_full.ass, legacy.ssa} and each target in {Srt, Ass, Ssa, Vtt}, the converted document re-parses, has the same number of non-comment, non-drawing cues, and the same times to within 10 ms
  - `ass_to_srt_keeps_italics_drops_positioning`: `{\an8}{\i1}Hi{\i0}` becomes `<i>Hi</i>`
  - `shift_negative_clamps_at_zero`: shifting a cue at 500..1500 ms by -1000 gives 0..500
  - `stretch_25_to_23976`: 60 000 ms becomes `(60000.0*25.0/23.976).round()` = 62 563
  - `stretch_rejects_zero_fps`
- [ ] **Step 2:** Run the tests. Expected: FAIL.
- [ ] **Step 3: Implement** the VTT parser and writer and the methods above.
- [ ] **Step 4:** Run `cargo test`. Expected: PASS.
- [ ] **Step 5: Commit** with `feat(core): WebVTT, format conversion, shift and stretch`.

### Task 4: Translation line pipeline (port of `subtitle_lines.py`)

**Files:**
- Create: `crates/subforge-core/src/lines.rs`, `tests/lines.rs`
- Create: `tests/fixtures/lines/` with frozen parity outputs (see Step 1)

**Interfaces:**
- Consumes: `load`, `SubtitleDoc`, and `Cue` from Tasks 1–3.
- Produces:
  - `#[derive(Serialize, Deserialize, Clone, Debug)] pub struct SubtitleLine { pub id: usize, pub start: i64, pub end: i64, pub text: String }` (same JSON shape as today)
  - `pub fn is_drawing(text: &str) -> bool`, `pub fn plain_text(text: &str) -> String`
  - `pub fn extract_lines(path: &Path) -> Result<Vec<SubtitleLine>, SubtitleError>`. One entry per cue in file order. Comment cues and drawings get `text: ""`. This is an intentional difference: Python sent Comment lines for translation.
  - `pub fn reflow(original: &str, translated: &str) -> String` (`MAX_LINE = 42`, at most 3 parts)
  - `pub fn arabic_punctuation(text: &str) -> String`, `pub fn fix_rtl_text(text: &str) -> String` (RLE `U+202B` / PDF `U+202C`, with leading ASS tags kept outside)
  - `pub fn rebuild_lines(original: &Path, translations: &[String], output: &Path, rtl_fix: bool) -> Result<(), SubtitleError>`. It skips index ≥ len, drawings, comments and blank translations. It writes in the original's format and joins with `\N`.

- [ ] **Step 1: Write failing tests.** First port every assertion from `scripts/test_subtitle_lines.py` verbatim as Rust tests (`plain_text_strips_tags_and_joins_breaks`, `reflow_keeps_italics_and_position`, `reflow_dash_dialogue`, `reflow_wraps_long_single_line`, `reflow_plain_not_italic`, `arabic_punctuation`). Then generate parity fixtures **once**, locally, before deleting Python: run `scripts/subtitle_lines.py extract` and `rebuild --rtl-fix` on `arabic_full.ass` and `basic.srt` with a fixed translations JSON. Commit those outputs as `tests/fixtures/lines/*.expected`. Add the tests `rebuild_matches_python_parity_srt` and `rebuild_matches_python_parity_ass`. Their comparison ignores only the differences listed in the CHANGELOG under "Intentional differences" (comment cues untranslated, plus ASS fields pysubs2 rewrote, such as normalised style floats). Each ignored difference has its own named test that shows the new behaviour.
- [ ] **Step 2:** Run the tests. Expected: FAIL.
- [ ] **Step 3: Implement** `lines.rs` by porting the Python logic one function at a time, with regexes from the `regex` crate. The dash-split uses the lookahead `\s+(?=[-–—]\s?\S)`. `regex` has no lookahead, so implement it as a manual scan.
- [ ] **Step 4:** Run `cargo test`. Expected: PASS.
- [ ] **Step 5: Commit** with `feat(core): native translation line extract/rebuild with RTL fixes`.

### Task 5: Tool resolution and merged FFmpeg/MKVToolNix wrappers

**Files:**
- Create: `crates/subforge-core/src/tools/{mod,ffmpeg,mkvtoolnix}.rs`, `tests/tools.rs`, `tests/fixtures/media/sample.mkv` (≤ 300 KB: 5 s, 160×90 video, stereo audio, one ASS subtitle track `ara` + one SRT `eng`, made with ffmpeg/mkvmerge), `tests/fixtures/media/README.md` (the exact commands used to create it)
- Source to merge: `src-tauri/src/tools/{ffmpeg,mkvtoolnix}.rs` and `server/src/tools/{ffmpeg,mkvtoolnix}.rs`

**Interfaces:**
- Consumes: `subforge_deps::{find_executable, SearchEnv, HideWindow, tool_version}`.
- Produces:
  - `#[derive(Copy, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)] pub enum ToolId { Ffmpeg, Ffprobe, Mkvmerge, Mkvextract, Mkvpropedit }` with `exe_name(self) -> &'static str`, which appends `.exe` on Windows
  - `#[serde(rename_all="lowercase")] pub enum ToolSource { Custom, Bundled, System }`
  - `#[derive(Default, Clone)] pub struct ToolPaths { pub custom: HashMap<ToolId, PathBuf>, pub bundled_dir: Option<PathBuf> }`
  - `pub struct ResolvedTool { pub id: ToolId, pub path: PathBuf, pub source: ToolSource }`
  - `pub fn resolve(id: ToolId, paths: &ToolPaths) -> Result<ResolvedTool, ToolError>`
  - `pub enum ToolError { CustomPathInvalid { tool: ToolId, path: PathBuf }, NotFound(ToolId), Failed { tool: ToolId, summary: String, details: String }, Io(io::Error), Json(String) }`. Its `Display` is human readable, for example: `"mkvmerge path set in Settings → Tool Paths does not exist: C:\…\mkvmerge.exe. Fix or clear it."`
  - `pub fn tool_status(paths: &ToolPaths) -> Vec<ToolStatus>`, where `ToolStatus { id, source: Option<ToolSource>, path: Option<PathBuf>, version: Option<String>, error: Option<String> }`
  - Every public function from the current `ffmpeg.rs` and `mkvtoolnix.rs` (`probe_audio_tracks`, `extract_audio`, `merge_audio`, `enumerate_tracks`, `extract_subtitle_track`, `extract_all_subtitles`, `extract_subtitles_by_language`, `merge_subtitles`, `merge_subtitles_async`, `remove_tracks`, `remove_all_subtitles`, `remove_all_audio`, plus types `Track`, `MkvInfo`, `VideoFileInfo`, `ExternalSub`, `SubtitleMergeSpec`, `FfprobeStream`, …) with **a leading `paths: &ToolPaths` parameter added**, and a new `pub fn set_track_flags(paths: &ToolPaths, file: &Path, track_number_1based: u32, props: &[(String, String)]) -> Result<(), ToolError>` that replaces the inline mkvpropedit code in `commands/subtitle.rs:222` and `routes/subtitle.rs:294`

- [ ] **Step 1: Write failing tests** in `tests/tools.rs`. Use a temp dir with fake executables: a shell script on Unix, and a copy of a tiny compiled test binary or a `.cmd` on Windows.
  - `custom_path_wins`, `bundled_used_when_no_custom`, `system_used_when_no_bundled` (prepend a temp dir to PATH)
  - `custom_path_missing_is_error_not_fallback` (Review Focus 5): custom points to a missing file and a bundled one exists. The result is `Err(CustomPathInvalid{..})`.
  - `mkv_exit_code_1_is_success_with_warnings` (ported from server's `mkv_succeeded`)
  - `tools_handle_unicode_space_paths` (Review Focus 2): with real mkvmerge and ffmpeg (test gated `#[ignore]` unless `SUBFORGE_REAL_TOOLS=1`), enumerate tracks and extract audio for a copy of `tests/fixtures/media/sample.mkv` placed at test time in a temp folder named `مجلد اختبار`
- [ ] **Step 2:** Run the tests. Expected: FAIL.
- [ ] **Step 3: Implement.** Diff the two copies of each file. Keep the union of fixes: server's `mkv_succeeded` / `mkv_error_text`, src-tauri's request timeouts, and so on. Replace `AppConfig::load()` lookups with `resolve(id, paths)`. Write the reconciliation list into the PR description.
- [ ] **Step 4:** Run `cargo test` and `SUBFORGE_REAL_TOOLS=1 cargo test -- --include-ignored` locally. Expected: PASS.
- [ ] **Step 5: Commit** with `feat(core): tool resolution and merged ffmpeg/mkvtoolnix wrappers`.

### Task 6: Merged translation providers

**Files:**
- Create: `crates/subforge-core/src/translate.rs`
- Source to merge: `src-tauri/src/tools/translate.rs` and `server/src/tools/translate.rs`, minus the Python extract/rebuild that Task 4 replaced

**Interfaces:**
- Consumes: `lines::SubtitleLine`.
- Produces: the existing functions with unchanged signatures: `detect_ollama`, `translate_ollama`, `translate_openai`, `detect_libretranslate`, `translate_libretranslate`, `translate_deepl`, `translate_google`, `test_deepl_key`, `test_openai_key`, `test_google_key`, plus the types `OllamaModel`, `OllamaTagResponse`, `LibreLanguage` and `TranslateError` (without `PythonNotFound` / `ScriptFailed`).

- [ ] **Step 1: Write failing tests.** Port existing unit tests from both copies, then add `openai_prompt_names_source_language` (src-tauri behaviour wins) and `libretranslate_falls_back_to_api_languages` using a local `axum` or `httpmock` stub.
- [ ] **Step 2:** Run the tests. Expected: FAIL.
- [ ] **Step 3: Implement** by taking src-tauri's copy (it has timeouts, the source language in the prompt, and the `/api/languages` fallback) and porting anything only server has. List the differences in the PR.
- [ ] **Step 4:** Run `cargo test`. Expected: PASS.
- [ ] **Step 5: Commit** with `feat(core): merge translation providers`.

### Task 7: Switch `src-tauri` to the core, async commands, config migration

**Files:**
- Modify: `src-tauri/Cargo.toml` (add `subforge-core`), `src-tauri/src/tools/mod.rs` (becomes an adapter), `src-tauri/src/commands/{subtitle,translate,audio,batch,deps}.rs`, `src-tauri/src/config.rs`, `server/src/config.rs` (new fields only), `src-tauri/src/lib.rs`
- Delete: `src-tauri/src/tools/{pysubs2,translate,ffmpeg,mkvtoolnix}.rs`. `whisper.rs`, `find_python` and `script_path` stay until Task 10.

**Interfaces:**
- Consumes: Tasks 1–6.
- Produces:
  - In `src-tauri/src/tools/mod.rs`: `pub fn tool_paths() -> subforge_core::tools::ToolPaths` built from `AppConfig` (`mkvmerge_path`, `mkvextract_path`, `ffmpeg_path`, `ffprobe_path`, new `mkvpropedit_path`) plus `bundled_dir()`. Also `pub fn set_bundled_dir(PathBuf)` (a `OnceLock`, set in `lib.rs` `setup` from `app.path().resource_dir()?.join("bin")` if that directory exists).
  - Config fields (both config.rs files, `#[serde(default)]`): `mkvpropedit_path: Option<String>`, `config_version: u32`
  - `pub fn migrate(cfg: &mut AppConfig, env: &SearchEnv) -> bool` in `src-tauri/src/config.rs`. When `config_version < 1`, it clears each tool path that, after canonicalisation, equals `find_executable(name, env)`, then sets `config_version = 1`. It returns whether anything changed. Call it at startup and save if it returns true.
  - New command `get_tool_status() -> Vec<ToolStatus>` (async)
  - Command names and argument shapes unchanged. `convert_subtitle`, `shift_subtitle` and `stretch_subtitle` now call `subforge_core::subtitle::*_file` via `spawn_blocking`. Translate commands call `lines::extract_lines` / `rebuild_lines(&input, &translated, &output, cfg.fix_rtl_punctuation)` directly (no temp JSON).

- [ ] **Step 1: Write failing tests** in `src-tauri/src/config.rs`: `migrate_clears_autodetected_paths` (fake exe on a temp PATH equal to the stored value is cleared), `migrate_keeps_hand_picked_paths`, `migrate_runs_once`.
- [ ] **Step 2:** Run `cargo test` in `src-tauri`. Expected: FAIL.
- [ ] **Step 3: Implement.** Rewire the commands. Audit every `#[tauri::command]`: any non-`async fn` that spawns a process, does network or file I/O on user media becomes `async` and uses `tokio::task::spawn_blocking` for sync core calls. Record the audited list in the PR.
- [ ] **Step 4:** Run `cargo test` and `cargo clippy -- -D warnings` in `src-tauri`, then `npm test`. Run `npm run tauri:dev` and convert an ASS file to SRT from the UI. Expected: PASS, and the output file is correct.
- [ ] **Step 5: Commit** with `refactor(tauri): use subforge-core; async commands; tool-path migration`.

### Task 8: Switch `server` to the core

**Files:**
- Modify: `server/Cargo.toml`, `server/src/tools/mod.rs` (adapter: `tool_paths()` from server config; `bundled_dir` from env `SUBFORGE_BUNDLED_DIR`, else `<exe_dir>/bin` if it exists), `server/src/routes/{subtitle,translate,audio,batch,deps,tools}.rs`
- Delete: `server/src/tools/{pysubs2,translate,ffmpeg,mkvtoolnix}.rs`

**Interfaces:**
- Consumes: Tasks 1–6, and the same adapter shape as Task 7. `server/src/tools/whisper.rs` and the transcribe route stay as they are until Task 10.
- Produces: unchanged REST routes, plus `GET /api/tools/status` returning `Vec<ToolStatus>`.

- [ ] **Step 1: Write a failing test:** a route test `tools_status_reports_sources` (axum `oneshot`), plus port any existing route tests that referenced deleted modules.
- [ ] **Step 2:** Run `cargo test` in `server`. Expected: FAIL.
- [ ] **Step 3: Implement.** Replace `tokio::task::block_in_place` calls around former Python helpers with direct core calls (or `spawn_blocking` for the file-heavy ones).
- [ ] **Step 4:** Run `cargo test` and `cargo clippy -- -D warnings` in `server`, then `grep -rn "tools::pysubs2\|tools::translate" server/src src-tauri/src`. Expected: tests PASS and grep finds nothing. `find_python`/`script_path` remain only for whisper until Task 10.
- [ ] **Step 5: Commit** with `refactor(server): use subforge-core; remove duplicated tool code`.

### Task 9: Whisper model catalog and download job

**Files:**
- Create: `crates/subforge-core/src/whisper/{mod,models}.rs`, `tests/whisper_models.rs`

**Interfaces:**
- Produces:
  - `pub const MODELS: &[ModelSpec]` where `ModelSpec { name: &'static str, file: &'static str, size_mb: u32, sha256: Option<&'static str> }`. The set is tiny, tiny.en, base, base.en, small, small.en, medium, medium.en, large-v3, large-v3-turbo, plus `large-v3-turbo-q5_0`. Fill sha256 from the Hugging Face file metadata, or leave it `None` when the source does not publish one.
  - `pub const VAD_MODEL: ModelSpec` (Silero, verified URL)
  - `pub fn default_cache_dir() -> PathBuf` returns `dirs::cache_dir()/subforge/whisper-ggml`. This is a new directory, so it never collides with the old `whisper-models`.
  - `pub fn legacy_cache_dir() -> PathBuf` returns `…/subforge/whisper-models`
  - `pub fn list_downloaded(cache: &Path) -> Vec<String>`, which counts only complete files: no `.part` file, and the size matches when known
  - `pub async fn download(name: &str, cache: &Path, base_url: &str, progress: ProgressFn<'_>, cancel: CancellationToken) -> Result<PathBuf, WhisperError>`. It streams to `<file>.part`, resumes with `Range` when a `.part` exists and the server answers 206 (otherwise it restarts), verifies sha256 when known, renames atomically, and then downloads `VAD_MODEL` too if it is missing. The progress message is `"{done_mb}/{total_mb} MB · {speed} MB/s"`.
  - `pub enum WhisperError { UnknownModel(String), Network{summary, details}, Checksum{file}, Cancelled, Io(io::Error), Engine(String), Tool(ToolError) }`

- [ ] **Step 1: Write failing tests** against a local `axum` stub server that serves a 5 MB body, supports `Range`, and can be made to stall:
  - `download_reports_monotonic_progress_to_100`
  - `cancelled_download_leaves_no_model` (Review Focus 4): cancel at about 50%. `list_downloaded` is empty and the `.part` file still exists.
  - `resume_appends_from_partial` (Review Focus 4): the stub records that a `Range: bytes=N-` header was received, and the final sha256 matches
  - `checksum_mismatch_deletes_file`
  - `vad_model_fetched_with_first_model`
- [ ] **Step 2:** Run the tests. Expected: FAIL.
- [ ] **Step 3: Implement** with `reqwest` streaming (`bytes_stream`), checking `cancel.is_cancelled()` between chunks, and throttling progress callbacks to at most 10 per second.
- [ ] **Step 4:** Run `cargo test`. Expected: PASS.
- [ ] **Step 5: Commit** with `feat(core): whisper.cpp model catalog and resumable download`.

### Task 10: whisper.cpp transcription (CPU) wired end to end

**Files:**
- Create: `crates/subforge-core/src/whisper/{transcribe,device}.rs`, `tests/whisper_transcribe.rs`, `tests/fixtures/media/speech_en_10s.opus` (generated with `espeak-ng`, text: "This is a SubForge speech test. The quick brown fox jumps over the lazy dog.", CC0, with the generation command in `tests/fixtures/media/README.md`)
- Modify: `src-tauri/src/commands/transcribe.rs`, `server/src/routes/transcribe.rs`, `src/stores/transcribeStore.ts`, `src/views/TranscribeView.vue`, `src/api/schemas.ts`, `src/i18n/{en,ar}.json`
- Delete: `src-tauri/src/tools/whisper.rs`, `server/src/tools/whisper.rs`

**Interfaces:**
- Consumes: `tools::{resolve, ToolPaths}` (ffmpeg decode), `models::*`, `ProgressFn`, `CancellationToken`.
- Produces:
  - `pub struct TranscribeOptions { pub input: PathBuf, pub output: PathBuf, pub model: String, pub device: Device, pub language: Option<String>, pub format: OutFormat, pub word_timestamps: bool, pub vad_filter: bool, pub cache_dir: Option<PathBuf> }`, `pub enum Device { Auto, Cpu, Gpu }`, `pub enum OutFormat { Srt, Vtt }`. The serde form is lowercase. `"cuda"` deserialises to `Auto`.
  - `pub struct TranscribeOutcome { pub device_used: String, pub segments: usize, pub detected_language: Option<String> }`
  - `pub fn transcribe(paths: &ToolPaths, opts: &TranscribeOptions, progress: ProgressFn<'_>, cancel: CancellationToken) -> Result<TranscribeOutcome, WhisperError>`. This call blocks, so callers run it in `spawn_blocking`. Audio is decoded with `ffmpeg -i <in> -ac 1 -ar 16000 -f f32le -` piped into memory. Progress comes from whisper-rs's progress callback, and cancellation from the abort callback.
  - `pub struct EngineInfo { pub engine: String /* "whisper.cpp <ver>" */, pub gpu: Option<String> /* device name */, pub devices: Vec<String> /* "auto","cpu",["gpu"] */ }` and `pub fn engine_info() -> EngineInfo`
  - Tauri commands, same names: `get_whisper_info -> EngineInfo` (async), `list_whisper_models` (async, now via `list_downloaded`), `download_whisper_model(model, cache_dir) -> String job_id`. That last one **changes** to start a job and return its id immediately, with progress over the existing `job-progress` / `job-completed` events. New: `cancel_job` is reused for cancelling. New: `whisper_legacy_cache() -> Option<String>` (the path, if the old directory exists) and `delete_whisper_legacy_cache()`. The server mirrors these as routes under the same names.
  - Frontend: `WhisperInfo` becomes `{ engine: string; gpu: string | null; devices: string[] }`. `downloadModel` tracks the job's progress percentage and shows it with a cancel button. The device `<select>` lists `auto`, `cpu`, and `gpu` only if present (labelled with the GPU name). The model list comes from a `list_whisper_catalog` command that returns `MODELS` names and sizes. When `whisper_legacy_cache()` is non-null, a one-time dismissible note offers a "Delete old models" button with a confirm dialog.

- [ ] **Step 1: Write failing tests:**
  - Rust: `transcribe_tiny_cpu_produces_srt` (gated `#[ignore]` unless `SUBFORGE_WHISPER_TEST=1`; it uses the cached `tiny` model and the speech fixture). It asserts that there is at least one segment, every `end_ms <= 11_000`, the times are monotonic, the lowercase text contains `"fox"` or `"subforge"`, and `device_used == "cpu"`.
  - Rust: `transcribe_cancel_returns_cancelled`
  - Rust: `device_cuda_string_deserialises_to_auto`
  - vitest `transcribeStore.test.ts`: `downloadModel starts job and tracks progress`, `device list hides gpu when not detected`
- [ ] **Step 2:** Run the tests. Expected: FAIL.
- [ ] **Step 3: Implement.** Add `whisper-rs` with no GPU feature yet. Rewire the commands and routes. Commands run in `spawn_blocking`, and `JobType::Transcribe` progress comes from the core callback.
- [ ] **Step 4:** Run `SUBFORGE_WHISPER_TEST=1 cargo test -- --include-ignored` in core, `cargo test` in `src-tauri` and `server`, and `npm test`. In `tauri:dev`, download `base` and watch the progress bar. The window must stay responsive (drag it during the download), and cancel must work. Expected: PASS.
- [ ] **Step 5: Commit** with `feat: built-in whisper.cpp speech-to-text with background model download`.

### Task 11: Remove Python everywhere

**Files:**
- Modify: `crates/subforge-deps/src/{model,check,detect,install,context,scripts,lib}.rs` (drop the `Python`, `Pysubs2` and `FasterWhisper` DepIds, `find_python`, venv logic, pip install and `locate_script`; keep FFmpeg/MKVToolNix and the OS package-manager install), `src/components/{DependencyList,DependencyDialog,DependenciesPanel,SetupInstructions}.vue` and their tests, `src/stores/depsStore.ts` and its test, `src/api/schemas.ts`, `src/i18n/{en,ar}.json`, `src-tauri/tauri.conf.json` (remove `resources` script entries), `README.md`, `CONTRIBUTING.md`, `.github/ISSUE_TEMPLATE/bug_report.md`, `packaging/aur/subforge/PKGBUILD`, `packaging/aur/subforge-bin/PKGBUILD`, `packaging/aur/README.md`
- Delete: `scripts/subtitle_ops.py`, `scripts/subtitle_lines.py`, `scripts/transcribe.py`, `scripts/test_subtitle_lines.py`, `scripts/__pycache__/`

**Interfaces:**
- Produces: `DepReport` without Python rows. The frontend `DepId` union is updated to match.

- [ ] **Step 1: Write failing tests.** Update `depsStore.test.ts` and `DependencyList.test.ts` to expect no Python rows. In `subforge-deps`, add `report_has_no_python_entries`.
- [ ] **Step 2:** Run `npm test` and `cargo test --features tokio` in `crates/subforge-deps`. Expected: FAIL.
- [ ] **Step 3: Implement** the removals. In AUR PKGBUILDs, `depends` becomes `ffmpeg mkvtoolnix-cli webkit2gtk-4.1` (keep the existing GUI deps) with no python. Add `CHANGELOG.md` entries for "No Python required", "Whisper models must be re-downloaded", "Old venv at `~/.local/share/subforge/venv` can be deleted", and "Intentional differences" from Task 4.
- [ ] **Step 4:** Run `npm test`, `npm run lint`, `npx vue-tsc --noEmit` and all `cargo test`s. Then run `grep -rni "python\|pysubs2\|faster.whisper" src src-tauri/src server/src crates/*/src src-tauri/tauri.conf.json packaging README.md`. Expected: PASS, and the only grep hits are in the CHANGELOG/README migration note.
- [ ] **Step 5: Commit** with `chore: remove Python and its packages from SubForge`.

### Task 12: Settings → Tool Paths (Browse, effective source, reset)

**Files:**
- Create: `src/components/ToolPathField.vue`, `src/components/ToolPathField.test.ts`
- Modify: `src/views/SettingsView.vue:109-125, 330-370`, `src/stores/appStore.ts` (add `toolStatus` + `loadToolStatus()` calling `get_tool_status`), `src/api/schemas.ts`, `src/i18n/{en,ar}.json`

**Interfaces:**
- Consumes: `get_tool_status` (Tasks 7/8) returning `{ id, source: "custom"|"bundled"|"system"|null, path, version, error }[]`.
- Produces: `<ToolPathField :tool-id="'mkvmerge'" v-model="config.mkvmerge_path" :status="status" />`. In Tauri mode, clicking the input or the **Browse** button opens `open({ multiple:false, directory:false, filters: Windows ? [{name, extensions:["exe"]}] : [] })` from `@tauri-apps/plugin-dialog`. In web mode, the existing `WebFilePicker.vue` is used. A badge shows "Bundled" / "Custom" / "System", and a muted line shows the version. **Reset to bundled** sets the model to `undefined`. After a selection, the store saves the config and reloads `toolStatus`, and the field shows `status.error` in red if one is present.

- [ ] **Step 1: Write failing tests** (vitest with mocked `apiInvoke` and dialog): `clicking input opens picker in tauri mode`, `selected path is written to model`, `shows bundled badge`, `shows error text for invalid custom path`, `reset clears model`. Update the `autoDetectTools` behaviour test so it **does not** write config paths.
- [ ] **Step 2:** Run `npm test`. Expected: FAIL.
- [ ] **Step 3: Implement.** Render five fields (add mkvpropedit). Auto-detect only refreshes `toolStatus`.
- [ ] **Step 4:** Run `npm test`, `npm run lint` and `npx vue-tsc --noEmit`. In `tauri:dev`, click the empty ffmpeg field and confirm the picker opens. Expected: PASS.
- [ ] **Step 5: Commit** with `feat(settings): file picker and effective source for tool paths`.

### Task 13: GPU acceleration (Vulkan) with CPU fallback

**Files:**
- Modify: `crates/subforge-core/Cargo.toml` (feature `gpu-vulkan` → `whisper-rs/vulkan`), `src-tauri/Cargo.toml` and `server/Cargo.toml` (forward the feature), `crates/subforge-core/src/whisper/device.rs`, `packaging/bundled-tools.json` (Vulkan loader entry for Windows), `.github/workflows/release.yml` (install the Vulkan SDK on the build runners)

**Interfaces:**
- Produces: `pub fn select_device(requested: Device, available_gpu: Option<&str>) -> (Device, String /* device_used */)` and the `engine_info().gpu` population.

- [ ] **Step 1: Write failing tests:** `select_device_falls_back_to_cpu` (Review Focus 3): `(Auto, None)` gives `(Cpu, "cpu")`, `(Gpu, None)` gives `(Cpu, "cpu")`, and `(Auto, Some("RTX"))` gives `(Gpu, "gpu: RTX")`.
- [ ] **Step 2:** Run the tests. Expected: FAIL.
- [ ] **Step 3: Implement.** Enumerate devices through the ggml backend registry exposed by whisper-rs. If that API is unavailable, try GPU context creation and fall back. On Windows, the release bundles the redistributable Vulkan loader (`vulkan-1.dll`, Apache-2.0) next to the exe, so the app starts even where no Vulkan runtime is installed. With no ICD, it reports zero devices and uses CPU. On Linux, `libvulkan.so.1` is a package dependency for `.deb`/`.rpm`/AUR and is bundled in the AppImage.
- [ ] **Step 4:** Run `cargo test --features gpu-vulkan` locally, where the user's machine has a GPU. Transcribe the speech fixture and confirm that `device_used` starts with `gpu`. Then repeat with `--device cpu`. Expected: PASS for both.
- [ ] **Step 5: Commit** with `feat(whisper): Vulkan GPU acceleration with automatic CPU fallback`.

### Task 14: Bundled tools, self-test mode, fixtures

**Files:**
- Create: `packaging/bundled-tools.json`, `scripts/fetch-bundled-tools.mjs`, `crates/subforge-core/src/selftest.rs`, `src-tauri/binaries/.gitignore`, `THIRD_PARTY_NOTICES.md`
- Modify: `src-tauri/tauri.conf.json` (`bundle.resources`: `"binaries/bin/": "bin/"`, `"binaries/licenses/": "licenses/"`; the deb/rpm `depends` lists `ffmpeg`, `mkvtoolnix`, `libvulkan1`/`vulkan-loader`), `src-tauri/src/lib.rs` (the `--self-test` arg), `.gitignore`

**Interfaces:**
- Consumes: everything above.
- Produces:
  - `bundled-tools.json`: `{ "<platform>": { "<tool-group>": { "version", "url", "sha256", "archive": "zip|7z|tar.xz|appimage", "files": { "<path-in-archive>": "bin/<name>" }, "license_files": [...] } } }` for `windows-x64` (FFmpeg GPL essentials build, MKVToolNix portable `.7z`, Vulkan loader) and `linux-x64` (static FFmpeg build, MKVToolNix binaries + required libs extracted from the official AppImage)
  - `node scripts/fetch-bundled-tools.mjs <platform>` downloads into a cache dir, verifies sha256 (and fails on a mismatch), extracts to `src-tauri/binaries/bin/` and `src-tauri/binaries/licenses/`, and on Linux sets the executable bit. It is idempotent.
  - `pub fn selftest::run(paths: &ToolPaths, fixtures: &Path, out: &Path, whisper_cache: &Path) -> SelfTestReport`. `SelfTestReport { steps: Vec<StepResult{ name, ok, millis, detail }>, ok: bool }` is written as JSON to `out/selftest.json`. The steps follow spec §7: assert that every resolved tool has `source == Bundled`, list tracks, extract the subtitle and audio, convert ASS→SRT→VTT and compare with goldens, shift and stretch against goldens, translate extract/rebuild with an in-process stub provider that uppercases text and appends `" [AR]"` with RTL fix on, download or restore `tiny` and transcribe the speech fixture, mux back with mkvmerge, and verify with `mkvmerge -J`.
  - `subforge --self-test <fixtures_dir> <out_dir> [--whisper-cache <dir>]`: in `lib.rs`, detected before the window is created. It resolves the bundled dir exactly like normal startup (resource dir + `bin`), runs `selftest::run`, prints the report, and exits 0 or 1 without opening a window.

- [ ] **Step 1: Write failing tests:** `selftest_report_fails_when_tool_not_bundled` (with a `ToolPaths` without `bundled_dir`, `report.ok == false` and the first failing step is `tools_bundled`), `selftest_json_shape`, and a script test `node scripts/fetch-bundled-tools.mjs --verify-only` that checks the manifest has sha256 for every entry.
- [ ] **Step 2:** Run the tests. Expected: FAIL.
- [ ] **Step 3: Implement.** Copy the fixture media into `tests/fixtures/media/مجلد اختبار/` at test time rather than committing Unicode paths, so that git on Windows behaves. Write the license files and notices.
- [ ] **Step 4:** Run `node scripts/fetch-bundled-tools.mjs linux-x64 && npm run tauri build -- --bundles appimage`, then `./SubForge*.AppImage --self-test tests/fixtures "/tmp/sf out"`. Expected: exit 0, and every step in `selftest.json` has `ok: true`.
- [ ] **Step 5: Commit** with `feat: bundle ffmpeg/mkvtoolnix and add --self-test mode`.

### Task 15: E2E CI on Windows and Linux, release wiring

**Files:**
- Create: `.github/workflows/e2e.yml`
- Modify: `.github/workflows/release.yml` (run `fetch-bundled-tools.mjs` before `tauri-action`, install the Vulkan SDK, and enable the `gpu-vulkan` feature), `.github/workflows/ci.yml` (add the core crate's tests on Windows and Linux), `README.md` (install section: no requirements on Windows or AppImage), `CHANGELOG.md`, the version via `npm run check:versions` conventions (minor bump to 0.3.0 in `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` and `server/Cargo.toml`)

**Interfaces:**
- Consumes: the `--self-test` CLI and the fetch script.

- [ ] **Step 1: Write the workflow.** It runs on `pull_request` and `push: main`. In the `windows-latest` job:
  1. checkout, node 20, rust stable, `rust-cache` (src-tauri)
  2. `actions/cache` for the fetch-script cache and `whisper-ggml/ggml-tiny.bin` + the VAD model
  3. `node scripts/fetch-bundled-tools.mjs windows-x64`
  4. `npm ci && npm run tauri build -- --bundles nsis` (no GPU feature in e2e, so the job doesn't need the Vulkan SDK; the loader-bundling check is covered by step 6)
  5. silent install: `Start-Process <setup.exe> -ArgumentList '/S','/D=C:\Sub Forge Test' -Wait`
  6. `Test-Path 'C:\Sub Forge Test\bin\mkvmerge.exe'`, `ffmpeg.exe`, `ffprobe.exe`, `mkvextract.exe` and `mkvpropedit.exe`
  7. remove every `ffmpeg`/`mkvmerge` from PATH for the following steps, so the bundled check is meaningful
  8. `& 'C:\Sub Forge Test\subforge.exe' --self-test "$PWD\crates\subforge-core\tests\fixtures" "$env:RUNNER_TEMP\سب فورج out" --whisper-cache "$env:RUNNER_TEMP\wcache"`. The step fails on a non-zero exit. Also assert that the JSON's transcribe step has `device_used == "cpu"` (Review Focus 3).
  9. GUI smoke: `Start-Process 'C:\Sub Forge Test\subforge.exe'`, sleep 15 s, check the process is still alive, then stop it
  10. on failure, `upload-artifact` the out dir and `%APPDATA%`/`%LOCALAPPDATA%` subforge logs

  An `ubuntu-22.04` job does the same with the AppImage. It runs `--self-test` under `xvfb-run` for the GUI smoke step, and uses `APPIMAGE_EXTRACT_AND_RUN=1`.
- [ ] **Step 2:** Push the branch and open a draft PR so the workflow runs. Expected: the first run is red only where real defects exist. Fix the causes, with no retries.
- [ ] **Step 3:** Update `release.yml` and the docs, and bump the version. Run `npm run check:versions`. Expected: PASS.
- [ ] **Step 4:** Confirm that `e2e.yml` is green on both OSes twice in a row on the PR.
- [ ] **Step 5: Commit** with `ci: installed-build e2e on Windows and Linux; release bundles tools`.

---

## Spec coverage check

| Spec section | Task(s) |
|---|---|
| §1 Shared core crate; deps crate loses Python | 1, 5, 6, 7, 8, 11 |
| §2 Native subtitle engine, formats, lossless ASS, encodings, parity, synthetic fixtures | 1, 2, 3, 4 |
| §3 whisper.cpp, models, download job, VAD, device selector, legacy cache note | 9, 10, 13 |
| §4 Resolution order, auto-detect migration, `ffprobe_path` wiring, `mkvpropedit_path`, per-package bundling, manifest, licenses, deps dialog trimmed | 5, 7, 11, 13, 14 |
| §5 Tool Paths UI | 12 |
| §6 No main-thread blocking; readable errors | 5 (ToolError), 7 (audit), 9, 10 |
| §7 Windows e2e (+ Linux) | 14, 15 |
| Migration and cleanup; CHANGELOG; version bump | 11, 15 |
