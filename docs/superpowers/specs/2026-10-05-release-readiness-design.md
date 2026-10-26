# Release Readiness: Dependency Checker, Installer, Windows Support, AUR & GitHub

**Date:** 2026-10-05
**Status:** Draft — awaiting user review

## Goal

Make SubForge ready to publish on GitHub and the AUR, work on Linux and Windows, and never leave
the user guessing: on startup it detects every missing dependency, tells the user exactly what is
missing, why it matters, and where/how to get it — and can install it for them.

## Out of scope

Docker packaging, macOS publishing (best-effort only), bundling ffmpeg/mkvtoolnix/Python into
installers. Pushing, tagging, and submitting to the AUR are done by the user (or on explicit
request), never automatically.

## 1. Dependency checker (`deps` module)

Shared by `src-tauri` and `server` (new crate `crates/subforge-deps`, so the logic is written once).

| Id | Kind | Severity | Used for |
|---|---|---|---|
| `mkvmerge`, `mkvextract` | system tool | required | track listing, extract, merge |
| `mkvpropedit` | system tool | optional | editing track flags |
| `ffmpeg`, `ffprobe` | system tool | required | audio extraction/merge, probing |
| `python` (>= 3.9) | interpreter | required | subtitle convert/retime, translate scripts |
| `pysubs2` | python package | required | subtitle format conversion |
| `faster-whisper` | python package | optional | speech-to-text |

Each result: `{ id, label, severity, state: ok|missing|outdated, path?, version?, purpose,
install: { methods[], download_url, manual_steps } }`.

Detection:
- Search `PATH`, then well-known locations (Windows: `Program Files\MKVToolNix`, winget links
  dir, `Program Files\ffmpeg`; Linux: `/usr/local/bin`, `~/.local/bin`).
- Python: prefer SubForge's own venv (section 3), then `python3`/`python`/`py -3`; skip the
  Windows Microsoft Store stub (a `python.exe` that exits with a store redirect).
- Package checks run `python -c "import <pkg>"` against the selected interpreter.
- The existing four-tool `ToolRegistry` is replaced by this; its Tauri command/REST route keep
  working (backward compatible field names) and gain the new entries.

## 2. Startup dialog and Settings panel

- On launch the app runs the check. If any **required** item is missing, a modal opens; if only
  optional items are missing, a dismissible banner shows instead.
- Each missing item shows: what it is, what breaks without it, an **Install** button, a
  **Copy command** button, and a **Download page** link.
- A **Re-check** button re-runs detection. Dismissing is allowed; features needing a missing
  item show an inline "Install X" notice instead of a vague error.
- Settings gets a "Dependencies" panel with the same data. i18n strings added to `en.json` and
  `ar.json`.
- Server mode exposes `GET /api/deps` and `POST /api/deps/install` using the same logic; the
  web UI shows the same dialog.

## 3. One-click installation

Install runs in the backend as a job with streamed output (reuses the existing job/WebSocket
progress mechanism), and re-checks when finished.

**System tools (mkvtoolnix, ffmpeg):**
- **Windows:** `winget install` (`MKVToolNix.MKVToolNix`, `Gyan.FFmpeg`). If winget is absent,
  fall back to showing the official download pages. After install, detection searches the known
  install dirs so a restart is not required.
- **Linux:** detect the package manager (`pacman`, `apt`, `dnf`, `zypper`) and run the install
  through `pkexec` (graphical polkit password prompt — the app never sees the password and does
  not require a terminal). If `pkexec` is missing or the user cancels, show the exact `sudo`
  command to copy.
- **macOS:** `brew install` if Homebrew exists, otherwise link to its install page.

**Python packages (pysubs2, faster-whisper):** installed into a dedicated virtual environment at
the app data dir (`~/.local/share/subforge/venv`, `%LOCALAPPDATA%\SubForge\venv`) using the
detected Python. This avoids PEP 668 "externally-managed-environment" failures on Arch/Debian and
never touches the system Python. If `python -m venv` is unavailable (e.g. Debian without
`python3-venv`), the installer installs that package first via the system path above. All
scripts run with the venv interpreter when it exists.

Safety: only a fixed allowlist of package names/ids is ever passed to a package manager; nothing
user-supplied is interpolated into commands (arguments passed as an argv array, never via a
shell).

## 4. Windows / cross-platform fixes

- Audit and fix Unix-only assumptions (e.g. `~/.cache` model dir in `scripts/transcribe.py`,
  path separators, `which` implementation, `CREATE_NO_WINDOW` for spawned processes so console
  windows don't flash).
- CI runs `cargo test` and `vitest` on Windows as well as Linux; release workflow keeps
  producing `.msi`/`.exe`/`.AppImage`/`.deb`/`.rpm`.
- Fix the launch scripts: `scripts/launch-subforge.sh` is Linux-only, so add a
  `scripts/launch-subforge.ps1` (or document that Windows users use the installer).

## 5. AUR packaging

`packaging/aur/subforge/PKGBUILD` (builds from the tagged GitHub tarball with cargo + npm) and
`packaging/aur/subforge-bin/PKGBUILD` (repackages the release `.deb`). Both:
- `depends`: `webkit2gtk-4.1`, `gtk3`, `mkvtoolnix-cli`, `ffmpeg`, `python`
- `optdepends`: `python-faster-whisper` note, `ollama`, `libretranslate`
- install the desktop file, icon, LICENSE, and the `scripts/` resources to the path the app's
  `script_path` lookup expects.
- `.SRCINFO` generated with `makepkg --printsrcinfo`. Validated with `makepkg` locally and
  `namcap` if installed.

## 6. GitHub readiness

- README rewritten: install per platform (release downloads, AUR, source), a dependency table,
  first-run walkthrough, troubleshooting (Wayland, missing tools, antivirus/SmartScreen on
  Windows), fixed project-structure section.
- Add `CONTRIBUTING.md`, `SECURITY.md`, `CHANGELOG.md`, issue/PR templates.
- Housekeeping: ignore `__pycache__/`, `.superpowers/`, `.claude/`; remove or relocate the
  root `SubForge.desktop` (superseded by `install-desktop-launcher.sh` and the AUR file).
- Fix the stale `Exec=` path issue found earlier (installed entry pointed at the pre-flatten
  path) so the installer is the only source of that file.
- Add a release version-consistency check (package.json / tauri.conf.json / Cargo.toml match).

## Testing

- Unit tests for detection (fake `PATH`, fake Python, missing/old versions) and for the install
  command builder (per OS/package manager, allowlist enforcement).
- Vitest component tests for the dialog states (all ok, required missing, optional missing,
  installing, failed).
- Manual verification on this machine (Arch/CachyOS) by removing tools from `PATH`; Windows
  verified through the CI Windows job.

## Risks

- Windows behaviour can't be exercised locally; mitigated by the CI Windows job and keeping
  Windows-specific code small and isolated.
- `pkexec` availability varies on minimal installs; the copy-command fallback covers it.
