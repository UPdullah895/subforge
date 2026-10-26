# Foundation: Self-Contained SubForge (no Python, bundled tools, Windows-tested)

**Date:** 2026-10-23
**Status:** Draft, awaiting user review
**Project 1 of 6** in the roadmap agreed on 2026-10-22:
1. Foundation (this document)
2. Subtitles: merge the Subtitles page into Batch and add a subtitle editor
3. Built-in local translation (benchmark Hunyuan-MT / Gemma / others via llama.cpp)
4. Video segment editor (built-in player, cut/blur/mask, subtitles kept in sync; "segment list" layout chosen)
5. Dub Sync (automate the Remy/Romeo dub-alignment workflow, no AI)
6. Docker "Bazarr replacement" (watch library, fetch free subs, else translate locally)

## Why

The user's main complaint: SubForge "performs very little of the work on its own." It depends
on Python, two Python packages, FFmpeg and MKVToolNix. That causes:

- **Unexplained errors.** A missing package or a Python version mismatch surfaces as a vague
  failure. This is worst on Windows, which most users run.
- **The Speech to Text model download "doesn't work, and the app crashes."** Root cause found:
  `download_whisper_model` (`src-tauri/src/commands/transcribe.rs:20`) is a synchronous Tauri
  command, so a multi-GB download runs on the main thread with no progress reporting. The window
  goes unresponsive and looks crashed. The download is also a Python subprocess, so it fails
  whenever Python or faster-whisper is not set up.
- **Manual tool paths.** Settings → Tool Paths requires typing paths by hand. There is no file
  picker.
- **Little real Windows testing.** CI compiles and runs unit tests on Windows, but nobody
  installs the app and runs real jobs there.

There is also a structural problem. Every tool wrapper exists twice, once in `src-tauri/src/tools/`
and once in `server/src/tools/` (ffmpeg, mkvtoolnix, pysubs2, translate, whisper). The copies
have already drifted (translate.rs is 762 vs 719 lines), so fixes land in one and not the other.

## Goal

SubForge works out of the box on Windows and Linux. The user installs nothing else, there is no
Python, and nothing freezes silently. When something fails, the message says what happened and
what to do.

## Goals

1. **Remove Python completely.** No interpreter, no venv, no pip, no `scripts/*.py` shipped.
2. **Native subtitle engine in Rust.** It replaces `subtitle_ops.py` (convert, shift, stretch)
   and `subtitle_lines.py` (extract lines for translation, rebuild with translated lines, RTL fix,
   Arabic punctuation, reflow, drawing detection, italic/leading-tag handling).
3. **Built-in speech-to-text with whisper.cpp.** It replaces `transcribe.py` and faster-whisper.
4. **Bundle FFmpeg (ffmpeg + ffprobe) and MKVToolNix** (mkvmerge, mkvextract, mkvpropedit) where
   the package format allows it (see section 4).
5. **One shared core crate** for all tool and engine logic, used by both the desktop app and the
   server. The duplicated `tools/` directories go away.
6. **No blocking work on the main thread.** Every long operation runs async or as a job, with
   progress and cancellation.
7. **Browse buttons** with a native file picker for every tool path in Settings.
8. **Automated Windows end-to-end check in CI** that installs the real installer and runs real
   jobs.

## Non-goals (handled by later projects)

- Subtitles/Batch merge and the subtitle editor (project 2).
- Local translation models (project 3). The existing online translation providers keep working
  unchanged, except that their line extract/rebuild step moves from Python to Rust.
- Segment editor, Dub Sync, Docker image (projects 4 to 6).
- Rewriting FFmpeg or MKVToolNix functionality natively. The user explicitly chose bundling.
- macOS publishing (stays best-effort, build-from-source).

## Anti-goals (a failure even if it technically works)

- Any user-visible regression in subtitle output. ASS files in particular must round-trip
  without losing styles, override tags, `[Fonts]`/`[Graphics]` sections, script info, comments,
  or layer/margin/effect fields. Arabic ASS subtitles with embedded fonts are the user's primary
  material.
- Translated Arabic output that loses today's RTL fixes or punctuation handling.
- A Speech to Text that is only fast on a GPU the user does not have. CPU must work on every
  machine.
- Installers that silently ignore a tool path the user set by hand.
- A Windows CI check that is so flaky it gets ignored.

## Constraints

- Stack stays as-is: Tauri 2, Vue 3 + Pinia + vue-i18n (en + ar), Rust. License GPL-3.0-only,
  so bundling GPL FFmpeg builds and MKVToolNix is compatible. Bundled tools must ship with their
  license texts, plus a source-offer note where the license requires one.
- Existing user-facing behaviour, Tauri command names and REST routes stay backward compatible
  unless this document says otherwise. The frontend calls go through `apiInvoke`, which serves
  both Tauri and server mode.
- Existing user config (`config.rs`: `mkvmerge_path`, `mkvextract_path`, `ffmpeg_path`, and so
  on) must keep working after upgrade.
- Platforms: Windows x64 and Linux x64 are first-class. macOS must still compile.

## Design

### 1. Shared core crate: `crates/subforge-core`

A new crate that owns everything both front ends need:

- `tools::ffmpeg`, `tools::mkvtoolnix`: the current wrappers, merged into one copy. Use the
  more complete or newer of the two diverged copies as the base and port over any fix found only
  in the other. The builder diffs them and lists what was reconciled in the PR.
- `tools::resolve`: tool path resolution (section 4).
- `subtitle`: the native subtitle engine (section 2).
- `translate`: the line extract/rebuild pipeline and provider code currently in
  `tools/translate.rs` (both copies merged).
- `transcribe`: whisper.cpp integration (section 3).
- Progress and cancellation primitives (a progress callback or channel, plus a cancellation
  token) that `src-tauri` adapts to Tauri events and `server` adapts to its WebSocket job
  updates.

`src-tauri/src/tools/` and `server/src/tools/` shrink to thin adapters, or are deleted.
`crates/subforge-deps` stays as the dependency checker but loses all Python detection and
installation (`find_python`, venv handling, pip installs, `PythonRequired`). The dependency
report keeps FFmpeg and MKVToolNix entries and reports whether each one is bundled, custom or
found on the system.

### 2. Native subtitle engine

Formats: SRT, ASS, SSA and WebVTT. These are the four the UI offers today in
`SubtitleView.vue`. Extra formats are not required. If the chosen library adds them for free,
they may be exposed, but they need tests.

Operations to reach parity with the Python scripts:

| Python today | Rust replacement |
|---|---|
| `subtitle_ops.py convert` | convert between any pair of the four formats |
| `subtitle_ops.py shift` | shift all cues by N ms (negative allowed, clamp at 0) |
| `subtitle_ops.py stretch` | rescale timings by `from_fps / to_fps` |
| `subtitle_lines.py extract` | per-cue `{id, start, end, text}` where `text` = visible plain text (override tags removed, `\N`/`\n` breaks joined, `\h` → space, whitespace collapsed); vector drawings (`\p1..9`) give empty text |
| `subtitle_lines.py rebuild` | re-insert translated text into the original file keeping timings, styles and leading override tags; reflow the translation into the original's line count with balanced splits; re-apply all-italic wrapping; optional RTL fix (RLE/PDF embedding per line, Arabic punctuation conversion) |

Requirements:
- **Lossless ASS/SSA round-trip.** Parse into a model that keeps unknown sections and fields
  verbatim. A load→save with no edits must produce byte-identical output, apart from
  normalizing line endings if the original was mixed.
- **Encoding.** Read UTF-8 (with or without BOM), UTF-16 LE/BE, and legacy single-byte files.
  Detect legacy encodings, with Windows-1256 (Arabic) as an important case. Write UTF-8.
  Report the detected encoding so later projects can show it.
- **Tests.** Port `scripts/test_subtitle_lines.py` cases to Rust. Add golden-file tests that
  convert each sample to and from every format, using ASS samples with Arabic text, embedded fonts
  and drawings. Do one-time parity checks of the RTL fix, reflow and punctuation against the
  Python output, then freeze the results as Rust fixtures.
- **Parity means "at least as good as Python", not "identical" (my call).** Where Python output
  is lossy or wrong (for example an ASS field pysubs2 drops, or a bad reflow split), the Rust
  engine fixes it. Each intentional difference is listed in the PR and the CHANGELOG, with a
  test showing the old and new output.
- **Fixtures are synthetic and redistributable (my call).** Hand-written ASS/SRT/VTT files
  cover the needed features: Arabic dialogue, override tags, italics, multi-line cues, `\p1`
  drawings, comments, and a `[Fonts]` section. Embedded fonts use an OFL-licensed font such as
  Noto Naskh Arabic. No copyrighted episode subtitles or fonts go into the repo. The user's own
  files may be used locally for extra checking, but are never committed.

Library choice is the builder's call. Writing the ASS/SRT/VTT parsers in-house is acceptable,
and probably the safest way to get a byte-exact ASS round-trip. Whichever path is chosen, the
round-trip and encoding tests above are the acceptance bar.

### 3. Speech to text: whisper.cpp

- Integrate via the `whisper-rs` bindings or equivalent. Inputs are decoded to 16 kHz mono PCM
  with the bundled FFmpeg.
- **Models.** These are GGML models from the official whisper.cpp Hugging Face repository.
  Offer: tiny, base, small, medium, large-v3, large-v3-turbo. Offer the `.en` variants where
  they exist and quantized variants where useful; the builder picks a sensible short list. The
  current faster-whisper model list in `transcribeStore.ts` changes accordingly. Old
  faster-whisper model caches are not reused. After upgrade, show a one-line note that models
  must be downloaded again, and offer to delete the old cache directory
  (`<cache>/subforge/whisper-models`), but only if the user confirms.
- **Download.** The download is a real async job: streaming HTTP with a progress percentage
  and speed, cancellation, resume-or-restart on failure, write to a temp file then rename, and
  verify the checksum when the repository publishes one. It must never run on the main thread.
  This fixes the reported freeze and crash.
- **Acceleration (my call).** Release builds include a GPU backend that works without the user
  installing a toolkit. Vulkan is the default candidate for Windows and Linux. If no usable GPU
  is present at runtime, fall back to CPU automatically. CPU must always work. The UI device
  selector shows what was actually detected, replacing the CUDA-only detection in
  `get_whisper_info`. CUDA as an extra build is optional and can come later.
- **Output parity.** SRT and VTT output, language auto-detect or forced language, word
  timestamps, and VAD. Same options as `TranscribeOptions` today.
- **VAD (my call).** Keep it, using whisper.cpp's VAD support with its small Silero VAD model.
  That model is a few MB and is downloaded automatically, as part of the same job, the first time
  any whisper model is downloaded. It doesn't appear as a separate step, so there is still nothing
  extra for the user to install. CI caches it alongside `tiny`.
- **Device selector (my call).** The values become `auto` / `cpu` / `gpu`. `auto` is the
  default: use the GPU if one is usable, otherwise the CPU. The `gpu` option is shown only when a
  usable GPU was detected, labelled with its name. Saved configs that contain `cuda` migrate to
  `auto`.
- Transcription runs as a job with progress (percentage of audio processed) and can be
  cancelled.

### 4. Bundled FFmpeg and MKVToolNix

**Resolution order** for every tool (ffmpeg, ffprobe, mkvmerge, mkvextract, mkvpropedit):
1. The user's custom path from Settings, if set. If that path is invalid, report it as an error.
   Never silently fall back from a path the user chose.
2. SubForge's bundled copy.
3. The system `PATH` and the well-known locations `subforge-deps` already searches.

`mkvpropedit` gets a config field. `ffprobe_path` already exists in `config.rs` but is ignored:
the code always uses `which("ffprobe")`. Wire it in.

**Paths filled in by "Auto-detect" (my call).** Settings has an "Auto-detect" button
(`autoDetectTools` in `SettingsView.vue`) that copies detected system paths into the config. Users
who clicked it now hold explicit paths that are really just "whatever was on PATH". Under rule 1
those would block the bundled tools forever. Handle it this way:
- On the first run after upgrade, a one-time config migration clears any stored tool path that
  equals what system detection resolves for that tool on that machine. It keeps any other path,
  because that path was chosen by hand. Record that the migration ran, in a config version field,
  so it never runs again.
- The Auto-detect button stops writing paths into the config. It only displays what was found.
  The effective-source display in section 5 replaces its purpose.

**Per package format (my call):**

| Package | How tools are provided |
|---|---|
| Windows `.msi` / `-setup.exe` | Bundled. Release CI downloads pinned FFmpeg (GPL build, for example gyan.dev or BtbN) and the MKVToolNix portable build, verifies their checksums, and adds them as Tauri resources or sidecars. |
| AppImage | Bundled (static FFmpeg build plus MKVToolNix binaries taken from its official AppImage or an equivalent portable build). |
| `.deb`, `.rpm`, AUR | Not bundled. Declare package dependencies (`ffmpeg`, `mkvtoolnix`/`mkvtoolnix-cli`) so the system package manager installs them automatically. This follows distro norms and avoids duplicate copies. |
| Server binary (non-Docker) | Uses system tools. The Docker image in project 6 bundles them. |

Pinned versions and download URLs live in one manifest file in the repo, so bumping a version is
a one-line change. Bundled tool license texts are installed next to the binaries and listed on
the About screen or in the third-party notices.

**Startup dependency dialog.** It stays, but on bundled builds it should never appear unless a
custom path is broken. Remove all Python rows and the Python-package install flow. The OS
package-manager install flow (`winget`/`pacman`/…) stays only for `.deb`/`.rpm`/AUR/source
builds where a system tool is missing.

### 5. Settings: Tool Paths

- Every tool path field gets a **Browse** button that opens a native file picker through
  `@tauri-apps/plugin-dialog`, which is already a dependency. Clicking the empty input itself
  also opens the picker, which is what the user expected.
- Each field shows its effective source: "Bundled", "Custom", or "System (path)", plus the
  detected version. It also has a **Reset to bundled** action.
- Validate on selection by running the tool's version command, and show an inline error if it
  fails.
- In server/web mode there is no native picker, so keep the text input. If a server-side file
  browser route already exists (`server/src/routes/fs.rs`), reuse it.
- New strings go in both `en.json` and `ar.json`.

### 6. No blocking on the main thread

Audit every `#[tauri::command]` in `src-tauri/src/commands/`. Any command that spawns a
process, does network I/O, or touches large files must be `async`, or must offload to a blocking
pool. Long-running operations use the existing job queue with progress events. Commands known to
need fixing: `download_whisper_model`, `get_whisper_info`, `list_whisper_models`, and the
subtitle convert/shift/stretch calls. The builder lists everything changed in the PR.

Errors returned to the UI are human-readable and actionable: name the tool or file, say what
failed, and say what to do. Raw stderr is still available in a "details" expander.

### 7. Windows end-to-end CI check

A new workflow job, on `windows-latest`, runs on every PR and on pushes to `main`:

1. Build the Windows installer exactly as the release workflow does, including bundled tools.
2. Install it silently into a path containing a space.
3. Run a scripted scenario against the installed build. The builder chooses the mechanism. Good
   candidates: a hidden CLI/self-test entry point in the app binary that runs the core
   operations through the same code paths, or the server binary built from the same core plus
   WebDriver (`tauri-driver`) for a UI smoke test. At minimum the scenario must:
   - use test media under a folder whose name contains Arabic characters and spaces;
   - list tracks of a small MKV and extract its subtitle and audio tracks (MKVToolNix and
     FFmpeg via the bundled copies, not anything on the runner's PATH; assert this);
   - convert ASS→SRT→VTT and shift/stretch, and compare against golden outputs;
   - run translation extract/rebuild with a stub provider (no network) and RTL fix on;
   - download the `tiny` whisper model, or restore it from the CI cache, and transcribe a
     ~10-second clip with known speech; assert non-empty output with sane timestamps;
   - mux tracks back with mkvmerge and verify the result with mkvmerge `-J`;
   - launch the GUI and confirm the main window opens without crashing (smoke level).
4. Upload logs and outputs as artifacts on failure.

Test media is tiny and generated or committed under a test-fixtures directory, with a licence
that allows redistribution. CI caches the whisper model and the bundled-tool downloads.
Retries must not be needed for a passing build. If a step is flaky, fix the cause.

The same scenario should also run on `ubuntu-latest` against the AppImage, because it costs
little once it exists.

## Migration and cleanup

- Delete `scripts/subtitle_ops.py`, `scripts/subtitle_lines.py`, `scripts/transcribe.py`,
  `scripts/test_subtitle_lines.py`, and the `resources` entries for them in
  `src-tauri/tauri.conf.json`.
- Remove `pysubs2`/`faster-whisper`/Python from: `subforge-deps`, the startup dialog, README
  (the requirements table, manual install commands and venv instructions), `packaging/aur`
  dependencies, the issue template, and CONTRIBUTING.
- Leave the SubForge venv directory on users' disks alone. Mention in release notes that it can
  be deleted.
- Update CHANGELOG. Bump the version as a minor release, because user-visible behaviour changes
  (whisper models must be re-downloaded).

## Acceptance criteria

- `grep -ri python` over `src-tauri/src`, `server/src`, `crates/`, `tauri.conf.json` and the
  installers finds nothing functional.
- A fresh Windows VM with nothing else installed: install SubForge, then extract, convert,
  translate (online provider), transcribe (download a model with visible progress) and mux all
  work. This is exactly what the CI check automates.
- Clicking a tool path field or its Browse button opens a file picker. A broken custom path shows
  a clear error and is never silently replaced.
- The UI never freezes during model download or transcription, and both can be cancelled.
- `src-tauri` and `server` contain no duplicated tool logic.
- All existing tests pass, and the new subtitle golden tests and Windows/Linux e2e jobs pass.

## Left to the builder

- Subtitle parsing library vs in-house parsers, as long as the round-trip and encoding tests
  pass.
- whisper.cpp binding crate, exact model short-list, and GPU backend details, as long as there
  is automatic CPU fallback and no toolkit install for users.
- Exact source and pinning mechanism for bundled FFmpeg/MKVToolNix builds.
- How the e2e scenario drives the app (self-test entry point vs server binary vs tauri-driver),
  as long as it exercises the installed build and the bundled tools.
- Internal module layout of `subforge-core` and how the work is split into PRs.
