# Changelog

All notable changes to this project are documented here. Format based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [SemVer](https://semver.org/).

## [Unreleased]

## [0.3.0] - 2026-10-23

### Added
- The Windows installer and the AppImage bundle FFmpeg 9.0.2 and MKVToolNix 103.0 (plus the Vulkan loader), so they work without installing anything else. The `.deb`, `.rpm` and AUR packages keep using the system packages. Licences are installed in `licenses/`; see `THIRD_PARTY_NOTICES.md`.
- GPU speech-to-text through Vulkan (NVIDIA, AMD and Intel). Device **Auto** uses a GPU when one is found and falls back to the CPU otherwise, including on machines with no GPU or no Vulkan driver; **GPU** and **CPU** can also be chosen explicitly.
- Licence and source compliance for the bundled programs: every bundled component's licence text ships in `licenses/`, each release attaches the corresponding GPL/LGPL source archives, and `THIRD_PARTY_NOTICES.md` includes a written offer for the rest.
- Hidden `subforge --self-test <fixtures_dir> <out_dir> [--whisper-cache <dir>]` mode that runs the core operations against the installed build and writes `selftest.json` (used by CI).
- End-to-end CI on Windows and Linux: every pull request builds the Windows installer and the AppImage the way releases are built, installs the Windows build silently into a folder with a space, runs the self-test on a runner without a GPU (test files under Arabic folder names; FFmpeg/MKVToolNix must come from the bundle; transcription must fall back to the CPU), and checks that the app window opens. The Linux job also checks the `.rpm` dependencies.

### Fixed
- Linux: MKVToolNix calls no longer fail on paths with non-ASCII characters (e.g. Arabic folder names) when SubForge runs under a non-UTF-8 locale such as `C`/`POSIX`.

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
