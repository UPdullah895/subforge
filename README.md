# SubForge

[![CI](https://github.com/UPdullah895/subforge/actions/workflows/ci.yml/badge.svg)](https://github.com/UPdullah895/subforge/actions/workflows/ci.yml)

> Open-source subtitle & audio toolkit — built with Tauri 2, Vue 3, and Rust.

SubForge is a free, cross-platform desktop application for subtitle manipulation, audio extraction, speech-to-text transcription, and machine translation. It also ships as a standalone server (REST + WebSocket API) for headless/self-hosted use.

---

## Install

### Windows
Download the `.msi` or `-setup.exe` from the [Releases page](https://github.com/UPdullah895/subforge/releases) and run it.
Windows SmartScreen may warn about an unrecognised publisher — choose **More info → Run anyway**.

### Arch Linux / CachyOS / Manjaro (AUR)
AUR packages (`subforge-bin` prebuilt, `subforge` from source) are being set up and are not on the AUR yet. Until they appear, use the AppImage or `.deb` from the [Releases page](https://github.com/UPdullah895/subforge/releases) (see "Other Linux" below), or [build from source](#build-from-source).

<!--
MAINTAINER: after submitting the AUR packages (see packaging/aur/README.md),
replace the paragraph above with:

yay -S subforge-bin   # prebuilt, fast
yay -S subforge       # builds from source
-->

### Other Linux
Download the `.AppImage` (any distro), `.deb` (Debian/Ubuntu) or `.rpm` (Fedora/openSUSE) from the Releases page.
```bash
chmod +x subforge_*.AppImage && ./subforge_*.AppImage
```

### macOS
Not published. Build from source (below); support is best-effort.

---

## First launch: what SubForge needs

SubForge relies on a few free programs. **On startup it checks for them and tells you exactly what is missing** — with an **Install** button, the exact command to copy, and a link to the official download page. You can also review them any time under **Settings → Required Programs**.

| Program | Needed for | Required? | Where to get it |
|---|---|---|---|
| MKVToolNix (`mkvmerge`, `mkvextract`) | Reading, extracting and merging tracks | Yes | [mkvtoolnix.download](https://mkvtoolnix.download/downloads.html) |
| FFmpeg (`ffmpeg`, `ffprobe`) | Audio extraction, conversion, merging | Yes | [ffmpeg.org](https://ffmpeg.org/download.html) |
| Python 3.9+ | Subtitle, translation and transcription helpers | Yes | [python.org](https://www.python.org/downloads/) |
| `pysubs2` (Python package) | Subtitle format conversion and retiming | Yes | installed for you |
| `faster-whisper` (Python package) | Speech-to-text | Optional | installed for you |
| `mkvpropedit` (MKVToolNix) | Editing track names/flags in place | Optional | comes with MKVToolNix |

**How the one-click install works**
- **Windows:** uses `winget` for MKVToolNix, FFmpeg and Python.
- **Linux:** uses your package manager (`pacman`, `apt`, `dnf`, `zypper`) through a graphical password prompt (`pkexec`). If `pkexec` is not available, SubForge shows the `sudo` command to run yourself.
- **macOS:** uses Homebrew if installed.
- **Python packages** go into a private environment (`~/.local/share/subforge/venv`, or `%LOCALAPPDATA%\subforge\venv` on Windows, `~/Library/Application Support/subforge/venv` on macOS), so your system Python is never modified.

Prefer to do it by hand?
```bash
# Arch / CachyOS
sudo pacman -S mkvtoolnix-cli ffmpeg python
# Debian / Ubuntu
sudo apt install mkvtoolnix ffmpeg python3 python3-venv python3-pip
# Fedora
sudo dnf install mkvtoolnix ffmpeg-free python3 python3-pip
# Windows (PowerShell)
winget install MoritzBunkus.MKVToolNix Gyan.FFmpeg Python.Python.3.12
```

The simplest way to get the Python packages (`pysubs2`, `faster-whisper`) is the **Install** button in SubForge, which uses its private environment. To do it by hand, create the same environment SubForge uses (a plain `pip install` into system Python fails on modern distros with `externally-managed-environment`):
```bash
# Linux
python3 -m venv --system-site-packages ~/.local/share/subforge/venv && ~/.local/share/subforge/venv/bin/pip install pysubs2 faster-whisper
# macOS (data directory is ~/Library/Application Support, not ~/.local/share)
python3 -m venv --system-site-packages "$HOME/Library/Application Support/subforge/venv" && "$HOME/Library/Application Support/subforge/venv/bin/pip" install pysubs2 faster-whisper
```
```powershell
# Windows (PowerShell)
py -3 -m venv --system-site-packages $env:LOCALAPPDATA\subforge\venv; & $env:LOCALAPPDATA\subforge\venv\Scripts\pip install pysubs2 faster-whisper
```

Run `cargo run --example check` inside `crates/subforge-deps` to print the same dependency report in your terminal.

### Server mode
`subforge-server` shows the same dependency dialog in the web UI. Because installing software on a server is sensitive, without `SUBFORGE_ALLOW_INSTALL=1` the server refuses installs and returns a message containing the command to run on the server yourself; set `SUBFORGE_ALLOW_INSTALL=1` to let the Install button run it.

---

## Features

### Subtitle & audio tools
- **Track enumeration** — open any MKV/MP4/AVI file and inspect all embedded tracks (video, audio, subtitle).
- **Subtitle extraction** — extract individual subtitle tracks to SRT/ASS/VTT via MKVToolNix.
- **Subtitle merge** — mux external SRT/ASS/VTT files into MKV containers.
- **Audio extraction/merge** — extract, transcode (AAC, MP3, OPUS, FLAC), and remux audio via FFmpeg.
- **Subtitle format conversion & retiming** — convert between SRT/ASS/SSA/VTT, shift timestamps via pysubs2.
- **Dependency check** — on startup, checks for MKVToolNix, FFmpeg (including ffprobe), Python and `pysubs2`, and offers to install anything that is missing.

### Batch operations
- **Batch directory scan** — recursively scan directories for video files and preview tracks for each.
- **Batch rename** — match files against TMDB/TVmaze (auto) or a manual pattern, with a rename preview.
- **Batch extraction, merge, and translation** — apply an operation across many files with a live job monitor.

### Speech-to-text
- **faster-whisper transcription** — transcribe audio/video to SRT or VTT, tiny → large-v3 models, GPU acceleration when available, word-level timestamps, VAD filtering, language auto-detection.

### Translation
- **Ollama (local LLM), LibreTranslate, DeepL, OpenAI, Google Translate** — batched, context-aware subtitle translation with automatic engine detection where applicable.
- **Online subtitle search before translating** — before running machine translation on a file, SubForge searches [Subsource](https://subsource.net) (on by default) and, if you supply your own account, [OpenSubtitles.com](https://www.opensubtitles.com) (file-hash matching for exact-release accuracy) for an existing human-made subtitle in the target language. If one is found it's used directly; if not, translation runs as normal. Toggle this off, or configure OpenSubtitles.com, from Settings → Subtitle Sources.

### Interface
- Dark theme, fully responsive layout, English/Arabic localization.

---

## Tech Stack

| Layer | Technology |
|---|---|
| Desktop shell | Tauri 2 (Rust + OS WebView) |
| Server | Axum (REST + WebSocket), for headless/self-hosted deployments |
| Frontend | Vue 3 + TypeScript + Vite |
| Styling | Tailwind CSS v4 |
| State | Pinia |
| Icons | Lucide Vue |
| Backend | Rust with `serde`, `anyhow`, `thiserror` |

---

## Build from source

Prerequisites: [Node.js](https://nodejs.org/) 18+, [Rust](https://rustup.rs/), the Tauri system libraries for your OS ([guide](https://tauri.app/start/prerequisites/)), plus the programs in the table above.

```bash
git clone https://github.com/UPdullah895/subforge.git
cd subforge
npm install
npm run tauri dev        # development window
npm run tauri build      # installers in src-tauri/target/release/bundle
```
On Wayland, if you see a protocol error, use `npm run tauri:dev:wayland`.

Optional: set `SUBFORGE_SUBSOURCE_API_KEY` at build time to enable Subsource subtitle search. Without it everything else works and Subsource search is silently skipped.

### Linux app-menu entry (development checkouts)
```bash
./scripts/install-desktop-launcher.sh
```
This registers a launcher that runs a release build if present, otherwise `npm run tauri:dev`. You can also run `./scripts/launch-subforge.sh` (desktop) or `./scripts/launch-subforge.sh --server` (server mode) directly.

---

## Project Structure

```
subforge/
├── src/                     # Vue 3 frontend
│   ├── views/                # SettingsView, SubtitleView, AudioView, BatchView, ...
│   ├── components/           # AutoTranslateWizard, TrackTable, JobMonitor, ...
│   ├── stores/                # Pinia stores (appStore, batchStore, translateStore, subtitleSourcesStore, ...)
│   ├── i18n/                  # en.json / ar.json
│   └── api/                   # apiInvoke abstraction (Tauri IPC or REST, same frontend code either way)
├── src-tauri/               # Tauri desktop backend (Rust)
│   ├── src/commands/          # Tauri commands
│   └── tauri.conf.json
├── server/                  # Standalone Axum REST + WebSocket server (Rust)
│   └── src/routes/
├── crates/                  # Shared Rust crates used by both backends
│   ├── subforge-deps/          # Dependency detection + one-click install
│   ├── subforge-metadata/     # TMDB/TVmaze rename matching, filename parsing
│   └── subforge-subtitle-sources/  # OpenSubtitles/Subsource clients + fallback orchestration
├── scripts/                 # Launcher scripts, pysubs2/whisper CLI wrappers
└── package.json
```

---

## Architecture

```
┌──────────────────────────────┐
│       Vue 3 + Tailwind        │
│   (same frontend, both modes) │
├───────────────┬────────────────┤
│  Tauri IPC     │   REST/WS      │
│  (desktop)     │   (server)     │
├───────────────┴────────────────┤
│           Rust Backend          │
│  commands/ or routes/, shared   │
│  crates/ (metadata, subtitle-   │
│  sources), tools/ (mkvtoolnix,  │
│  ffmpeg, pysubs2)                │
├──────────────────────────────────┤
│  mkvmerge / mkvextract / ffmpeg  │
└──────────────────────────────────┘
```

---

## Roadmap

| Goal | Status |
|---|---|
| Foundation — scaffold, track enum, single-file extraction | ✅ Done |
| Batch operations — merge, remove, convert, retime | ✅ Done |
| Speech-to-Text — faster-whisper integration | ✅ Done |
| Translation — Ollama, LibreTranslate, cloud APIs | ✅ Done |
| Online subtitle search before auto-translate | ✅ Done |
| Server Mode — REST + WebSocket API | ✅ Done |
| Docker packaging for server mode | ⏳ Planned |
| Polish — OCR, auto-sync | ⏳ Planned |

---

## License

GPL-3.0-only — see [LICENSE](LICENSE).

---

## Troubleshooting

### Wayland protocol error on Linux

If you see:
```
Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display.
```

Run the app with DMA-BUF rendering disabled:
```bash
npm run tauri:dev:wayland
# Equivalent to:
# WEBKIT_DISABLE_DMABUF_RENDERER=1 WEBKIT_DISABLE_COMPOSITING_MODE=1 npm run tauri dev
```

Or set the variables manually:
```bash
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1
npm run tauri dev
```

### "Required Programs" keeps showing something as missing
Click **Re-check** after installing. On Windows, if you installed a tool in a custom location, add its folder to `PATH` and restart SubForge. Run `cargo run --example check` in `crates/subforge-deps` to see exactly what SubForge detects.

### Windows: installed tool not found right after `winget install`
SubForge also looks in the standard install folders (WinGet links, Program Files, Python), so a fresh install should be found after **Re-check**. A restart is only needed for tools installed in custom locations: add their folder to `PATH` and reopen SubForge, because new `PATH` entries are only picked up by new processes.

---

*Built with ❤️ for fansubbers, content creators, and media enthusiasts.*
