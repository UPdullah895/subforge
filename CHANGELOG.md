# Changelog

All notable changes to this project are documented here. Format based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [SemVer](https://semver.org/).

## [Unreleased]

### Changed
- Speech-to-text runs on the built-in whisper.cpp engine (CPU) instead of Python faster-whisper. Models are GGML files in `<cache>/subforge/whisper-ggml`; the old faster-whisper models can be deleted from the Transcribe page.
- Model downloads run as background jobs with a progress bar and a cancel button; the app no longer freezes while a model downloads. A model must be downloaded before transcribing (it is no longer fetched on first use).
- Model list: `large-v1` and `large-v2` are gone; `large-v3-turbo` and `large-v3-turbo-q5_0` are new.
- The Device choice is Auto / CPU (GPU support follows); a saved `cuda` value is treated as Auto.

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
