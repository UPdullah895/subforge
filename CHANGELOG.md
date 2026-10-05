# Changelog

All notable changes to this project are documented here. Format based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [SemVer](https://semver.org/).

## [Unreleased]

## [0.2.0] - 2026-10-05

### Added
- Startup dependency check with install guidance (MKVToolNix, FFmpeg, Python, pysubs2, faster-whisper).
- One-click install via winget (Windows), the system package manager through pkexec (Linux), Homebrew (macOS), and a private virtualenv for Python packages.
- "Required Programs" panel in Settings and a banner when something required is missing.
- AUR packages `subforge` and `subforge-bin`; `.deb`/`.rpm` now declare their dependencies.
- Windows CI jobs.

### Changed
- Tools are located in well-known install folders as well as `PATH` (fixes detection right after `winget install` on Windows).
- Child processes no longer flash a console window on Windows.
- Helper scripts are found in standard Linux package locations.

## [0.1.0] - 2026-08-02
- Initial release.
