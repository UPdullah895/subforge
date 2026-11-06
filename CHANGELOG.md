# Changelog

All notable changes to this project are documented here. Format based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [SemVer](https://semver.org/).

## [Unreleased]

### Added
- **`subforge-pipeline`** — planning *and* execution for library automation: the executor walks the plan through a `StepRunner` seam (so search/transcribe/translate can be faked in tests or wired to the real engines in the server), stops a language at its first failure, and never lets the cleanup step run when the verify step failed; a `DryRun` runner records the whole plan without touching a file. The planning half: a `Policy` (target languages, search-before-translate, delete-source-after-mux, dry-run), a normalised `Inventory` of a file's subtitle/audio languages (ISO 639-2 ↔ 639-1, region suffixes, `und`), and `plan(file, inventory, policy)` which returns the ordered steps *search → translate/transcribe → mux → verify → cleanup*. No file is touched: the whole decision is testable, and can be shown as a dry run before anything is written.
- **`subforge-mcp`** — a Model Context Protocol server (stdio **and** streamable HTTP, `--http ADDR`), so an agent can drive SubForge: seven tools (`list_tracks`, `find_missing_languages`, `extract_subtitles`, `merge_subtitles`, `convert_subtitle`, `shift_subtitle`, `probe_media`), each mapped onto the shared core. Verified with a real MCP client: initialise, list, extract, convert, retime, mux, and re-read the merged file to confirm the new track's language and name.
- `scripts/install-appimage.sh`: installs the Linux AppImage for the current user — copies it to `~/.local/bin/subforge`, adds the menu entry and the icons, and supports `--uninstall`. An AppImage installs none of that by itself, so a user who is not a developer previously had no launcher icon and no `subforge` command.
- The release workflow publishes `SHA256SUMS.txt` for the installers of every release, so a download can be verified with `sha256sum -c` before it is run.
- An update notice: the app asks GitHub once a day whether a newer release exists and shows a bar with a link when there is one (dismissible per version, no download). The version comes from `package.json` through a Vite define, so the web UI served by `server/` shows it too.
- **macOS builds.** The release workflow now also produces an unsigned arm64 `.dmg` for Apple Silicon (no Vulkan there: speech-to-text uses the CPU, and FFmpeg/MKVToolNix come from Homebrew rather than the bundle). CI compiles `src-tauri` on macOS on every PR so it cannot rot.

### Changed
- README: the Linux section states the real runtime requirements (glibc 2.35+, FUSE 2, with the `--appimage-extract-and-run` fallback), how to get a launcher, and how to update; the old "any distro / nothing else installed" claim was wrong for older distributions.
- Releases attach the GPL/LGPL corresponding sources as one `third-party-sources.tar.xz` instead of eleven loose archives, so the page shows the installers first.

### Fixed
- **Speech to text crashed on CPUs without AVX-512.** whisper.cpp's ggml defaults to `-march=native`, so the release inherited the AVX-512 support of the CI runner and died with `SIGILL` inside `ggml_cpu` while loading a model — on a Ryzen 5000 or any Intel 12th–14th gen, the Speech to Text page was unusable and the app could not say why. The build now pins a portable baseline (`packaging/ci/ggml-portable.cmake`: AVX2/FMA/F16C, no AVX-512), CI asserts `GGML_NATIVE=OFF` in whisper's CMake cache, and `scripts/ci/check-no-avx512.mjs` fails the build if ggml/whisper code contains any AVX-512 instruction.
- The AUR `PKGBUILD`s carry real `sha256sum`s instead of `SKIP`, so `subforge-bin` and `subforge` can be built and submitted (`makepkg --verifysource` passes for v0.3.0).
- The desktop entry uses a single main category (`AudioVideo;`) and sets `StartupWMClass`, so `desktop-file-validate` is clean and the window groups under its own launcher icon. Same fix in `scripts/install-desktop-launcher.sh`, which writes the entry for source checkouts.

## [0.3.0] - 2026-10-26

### Added
- **Sync** tab: checks a subtitle against its video. The instant check compares the cue span with the real duration and detects a 25/23.976 (or 24/23.976, 25/24, 30/29.97) frame-rate mismatch, with a one-click fix; it also reports bad timings, overlaps and a subtitle that runs past the end of the video. The optional deep check transcribes three short speech windows with the built-in engine, matches the words to nearby cues and reports a constant delay, growing drift or a likely wrong episode, with the shift/frame-rate fix to apply.
- Online subtitle search returns a timing verdict, so the auto-translate wizard warns when a found subtitle does not fit the video, and an existing same-named subtitle is no longer overwritten (the download is written as `{name}.{lang}.found.srt`).
- **Drag and drop** in the desktop app: drop videos on the Subtitle, Audio or Transcribe pages, a folder on Batch, or a subtitle file on the Convert/Clean tabs. A dropped folder is recognised as a folder (it has no extension) and explained instead of being passed to mkvmerge as a video; Batch no longer scans a whole folder when a single file is dropped.
- **Clean** tab for subtitles: removes translator credits, social-media handles, links and channel advertisements from SRT, ASS, SSA and VTT. Every match is previewed first with the reason it matched, any line can be kept, and overwriting the original keeps a `.bak` copy. ASS styles, `[Fonts]`, `[Graphics]` and drawings are left untouched.
- Subsource search can use your own API key from **Settings → Subtitle Sources**; a key set there takes precedence over the one (if any) built into the app.
- The Windows installer and the AppImage bundle FFmpeg 9.0.2 and MKVToolNix 103.0 (plus the Vulkan loader), so they work without installing anything else. The `.deb`, `.rpm` and AUR packages keep using the system packages. Licences are installed in `licenses/`; see `THIRD_PARTY_NOTICES.md`.
- GPU speech-to-text through Vulkan (NVIDIA, AMD and Intel). Device **Auto** uses a GPU when one is found and falls back to the CPU otherwise, including on machines with no GPU or no Vulkan driver; **GPU** and **CPU** can also be chosen explicitly.
- Licence and source compliance for the bundled programs: every bundled component's licence text ships in `licenses/`, each release attaches the corresponding GPL/LGPL source archives, and `THIRD_PARTY_NOTICES.md` includes a written offer for the rest.
- Hidden `subforge --self-test <fixtures_dir> <out_dir> [--whisper-cache <dir>]` mode that runs the core operations against the installed build and writes `selftest.json` (used by CI).
- End-to-end CI on Windows and Linux: every pull request builds the Windows installer and the AppImage the way releases are built, installs the Windows build silently into a folder with a space, runs the self-test on a runner without a GPU (test files under Arabic folder names; FFmpeg/MKVToolNix must come from the bundle; transcription must fall back to the CPU), and checks that the app window opens. The Linux job also checks the `.rpm` dependencies.

### Fixed
- OpenSubtitles settings show the saved API key and username when you return to the page (the password stays write-only), the key is masked with a reveal button, and clicking **Save** on an empty form no longer wipes a working configuration.
- TMDB and Subsource API keys save automatically and **Test** checks the value currently in the form, so neither has to be saved first. Save failures are shown instead of being silently dropped.
- Subsource reports **Not configured** when there is no key, instead of the misleading **Unreachable**; an invalid key and rate limiting get their own messages.
- The "Required Programs" dialog no longer appears on installs that bundle FFmpeg and MKVToolNix. It now uses the same lookup as every tool call (custom path, then bundled, then system), shows where each tool was found, and appears when a custom tool path in Settings is broken.
- Only one transcription runs at a time; others wait with a "Queued" status and can be cancelled while waiting (several at once could run out of memory and crash the app).
- Translating an ASS/SSA track keeps it ASS/SSA (`{video}.{lang}.ass`) with its styles, instead of writing ASS text into a `.srt` file.
- SRT files with a blank line inside a subtitle, or stray text between subtitles, load instead of being rejected (same result as pysubs2).
- MKVToolNix errors show the actual reason instead of the version banner, and an unreadable or unrecognised file is reported as an error instead of "no tracks". The raw tool output is available under "Details" in error messages.
- Settings shows an error when a custom tool path does not answer its version command, and version checks give up after 10 seconds instead of hanging.
- Model downloads show the downloaded size and speed.
- Linux: MKVToolNix calls no longer fail on paths with non-ASCII characters (e.g. Arabic folder names) when SubForge runs under a non-UTF-8 locale such as `C`/`POSIX`.

### Security
- API keys (TMDB, DeepL, OpenAI, Google Translate, Subsource) are stored in the OS keyring instead of `config.json`, and existing keys are migrated on first launch (a key stays in the file if the keyring is unavailable, so nothing is lost). `config.json` is now owner-only (`0600`). Server mode has no keychain and keeps its keys in `config.json`; running both modes on one machine means entering them again in the other mode.

### Removed
- **No Python required.** Speech-to-text is the built-in whisper.cpp engine, and the subtitle conversion, retiming and translation helpers are native. SubForge no longer detects, installs or bundles Python, `pysubs2` or `faster-whisper`.
- The old virtual environment at `~/.local/share/subforge/venv` (`%LOCALAPPDATA%\subforge\venv` on Windows) is no longer used and can be deleted.

### Migration
- **Whisper models must be re-downloaded.** Models use the new GGML format; the old cache can be deleted from the Speech to Text page.
- Speech to Text no longer downloads a model automatically on first use. Download it first (with progress and a cancel button).

### Intentional differences
- Comment lines in ASS/SSA are no longer sent for translation; untouched cues and the ASS header/sections/spacing are preserved byte-for-byte (pysubs2 rewrote them).
- WebVTT NOTE/STYLE/REGION blocks are dropped on write; on edited cues, inline `<c>`, `<lang>`, ruby and timestamp tags are dropped (speaker `<v>` labels are kept).
- ASS drawing detection: `{\p1 }`/`{\p1abc}` are not treated as drawings and `{\p01}` is (pysubs2 regex differed in these rare cases).
- Server API: convert/shift/stretch return a readable error body; converting to an unknown format returns 400 instead of 500; track removal now actually removes the selected tracks; new `GET /api/tools/status`; the tool registry includes mkvpropedit.

### Changed
- Server API: tool failures answer `{"error": "...", "details": "..."}` with status 422 (tool failed) or 503 (tool missing or custom path broken) instead of a bare status code.
- Speech-to-text runs on the built-in whisper.cpp engine instead of Python faster-whisper. Models are GGML files in `<cache>/subforge/whisper-ggml`; the old faster-whisper models can be deleted from the Speech to Text page.
- Model downloads run as background jobs with a progress bar and a cancel button; the app no longer freezes while a model downloads.
- Model list: `large-v1` and `large-v2` are gone; `large-v3-turbo` and `large-v3-turbo-q5_0` are new.
- The Device choice is Auto / CPU / GPU; a saved `cuda` value is treated as Auto.
- The `.rpm` accepts either `ffmpeg` or `ffmpeg-free`, so it installs on stock Fedora.

## [0.2.0] - 2026-10-05

### Added
- Startup dependency check with install guidance (MKVToolNix, FFmpeg, Python, pysubs2, faster-whisper).
- One-click install via winget (Windows), the system package manager through pkexec (Linux), Homebrew (macOS), and a private virtualenv for Python packages.
- "Required Programs" panel in Settings and a banner when something required is missing.
- AUR package recipes `subforge` and `subforge-bin` (in `packaging/aur/`); `.deb`/`.rpm` now declare their dependencies.
- Windows CI jobs.

### Changed
- Tools are located in well-known install folders as well as `PATH` (fixes detection right after `winget install` on Windows).
- Child processes no longer flash a console window on Windows.
- Helper scripts are found in standard Linux package locations.

## [0.1.0] - 2026-08-02
- Initial release.
