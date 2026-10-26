# SubForge

[![CI](https://github.com/UPdullah895/subforge/actions/workflows/ci.yml/badge.svg)](https://github.com/UPdullah895/subforge/actions/workflows/ci.yml)
[![E2E](https://github.com/UPdullah895/subforge/actions/workflows/e2e.yml/badge.svg)](https://github.com/UPdullah895/subforge/actions/workflows/e2e.yml)

> Open-source subtitle & audio toolkit — built with Tauri 2, Vue 3, and Rust.

SubForge is a free, cross-platform desktop application for subtitle manipulation, audio extraction, speech-to-text transcription, and machine translation. It also ships as a standalone server (REST + WebSocket API) for headless/self-hosted use.

---

## Install

### Windows
Download the `.msi` or `-setup.exe` from the [Releases page](https://github.com/UPdullah895/subforge/releases) and run it. Nothing else needs to be installed: FFmpeg and MKVToolNix come with the installer, and no Python is required.
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
Download the `.AppImage` (any distro), `.deb` (Debian/Ubuntu) or `.rpm` (Fedora/openSUSE) from the Releases page. The AppImage needs nothing else installed (FFmpeg and MKVToolNix are inside it); the `.deb` and `.rpm` install them as dependencies.
```bash
chmod +x subforge_*.AppImage && ./subforge_*.AppImage
```

### macOS
Not published. Build from source (below); support is best-effort.

---

## First launch: what SubForge needs

SubForge needs just two free programs: **FFmpeg** and **MKVToolNix**. They are installed automatically by the `.deb`, `.rpm` and AUR packages, and bundled in the Windows installer and the AppImage. No Python is required: subtitle conversion, translation helpers and speech-to-text (whisper.cpp) are built into SubForge.

If something is missing, **on startup SubForge tells you exactly what** — with an **Install** button, the exact command to copy, and a link to the official download page. You can also review them any time under **Settings → Required Programs**.

| Program | Needed for | Required? | Where to get it |
|---|---|---|---|
| MKVToolNix (`mkvmerge`, `mkvextract`) | Reading, extracting and merging tracks | Yes | [mkvtoolnix.download](https://mkvtoolnix.download/downloads.html) |
| FFmpeg (`ffmpeg`, `ffprobe`) | Audio extraction, conversion, merging | Yes | [ffmpeg.org](https://ffmpeg.org/download.html) |
| `mkvpropedit` (MKVToolNix) | Editing track names/flags in place | Optional | comes with MKVToolNix |

**How the one-click install works**
- **Windows:** uses `winget` for MKVToolNix and FFmpeg.
- **Linux:** uses your package manager (`pacman`, `apt`, `dnf`, `zypper`) through a graphical password prompt (`pkexec`). If `pkexec` is not available, SubForge shows the `sudo` command to run yourself.
- **macOS:** uses Homebrew if installed.

Prefer to do it by hand?
```bash
# Arch / CachyOS
sudo pacman -S mkvtoolnix-cli ffmpeg
# Debian / Ubuntu
sudo apt install mkvtoolnix ffmpeg
# Fedora
sudo dnf install mkvtoolnix ffmpeg-free
# Windows (PowerShell)
winget install MoritzBunkus.MKVToolNix Gyan.FFmpeg
```

> **Upgrading from an earlier version?** Python and its private environment (`~/.local/share/subforge/venv`, or `%LOCALAPPDATA%\subforge\venv` on Windows) are no longer used and can be deleted. Speech-to-text models use a new format and must be downloaded again; see the [changelog](CHANGELOG.md).

Developers can print the same dependency report in a terminal with `cargo run --example check` inside `crates/subforge-deps`.

### Server mode
`subforge-server` shows the same dependency dialog in the web UI. Because installing software on a server is sensitive, without `SUBFORGE_ALLOW_INSTALL=1` the server refuses installs and returns a message containing the command to run on the server yourself; set `SUBFORGE_ALLOW_INSTALL=1` to let the Install button run it.

---

## Features

### Subtitle & audio tools
- **Track enumeration** — open any MKV/MP4/AVI file and inspect all embedded tracks (video, audio, subtitle).
- **Subtitle extraction** — extract individual subtitle tracks to SRT/ASS/VTT via MKVToolNix.
- **Subtitle merge** — mux external SRT/ASS/VTT files into MKV containers.
- **Audio extraction/merge** — extract, transcode (AAC, MP3, OPUS, FLAC), and remux audio via FFmpeg.
- **Subtitle format conversion & retiming** — convert between SRT/ASS/SSA/VTT, shift and stretch timestamps (built-in, no external tools).
- **Credit & ad cleanup** — finds translator credits, social handles, links and channel ads in SRT/ASS/SSA/VTT, shows exactly which lines would go, and removes them only after you confirm (ASS styles, fonts and drawings are untouched).
- **Dependency check** — on startup, checks for MKVToolNix and FFmpeg (including ffprobe), and offers to install anything that is missing.

### Batch operations
- **Batch directory scan** — recursively scan directories for video files and preview tracks for each.
- **Batch rename** — match files against TMDB/TVmaze (auto) or a manual pattern, with a rename preview.
- **Batch extraction, merge, and translation** — apply an operation across many files with a live job monitor.

### Speech-to-text
- **Built-in whisper.cpp transcription** — transcribe audio/video to SRT or VTT with GGML models (tiny → large-v3-turbo) that you download once from the Speech to Text page (with progress and cancel), plus language auto-detection. Uses the GPU through Vulkan when one is available (Device: Auto / CPU / GPU) and the CPU otherwise; no Python needed.

### Translation
- **Ollama (local LLM), LibreTranslate, DeepL, OpenAI, Google Translate** — batched, context-aware subtitle translation with automatic engine detection where applicable.
- **Online subtitle search before translating** — before running machine translation on a file, SubForge searches [Subsource](https://subsource.net) (on by default) and, if you supply your own account, [OpenSubtitles.com](https://www.opensubtitles.com) (file-hash matching for exact-release accuracy) for an existing human-made subtitle in the target language. If one is found it's used directly; if not, translation runs as normal. Toggle this off, or configure OpenSubtitles.com or your own Subsource key, from Settings → Subtitle Sources. API keys are kept in the operating system's keyring, not in `config.json`.

### Interface
- **Drag and drop** — drop video files (or a folder on the Batch page, or a subtitle on the Convert/Clean tabs) straight onto the window.
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

GPU speech-to-text is the optional `gpu-vulkan` feature (`npm run tauri build -- --features gpu-vulkan`, as the releases are built). It needs the Vulkan headers and `glslc` (the [Vulkan SDK](https://vulkan.lunarg.com/), or e.g. `vulkan-headers` and `shaderc` on Arch).

To bundle FFmpeg and MKVToolNix the way the release installers do (Windows installer and AppImage), fetch the pinned, checksum-verified copies first with `node scripts/fetch-bundled-tools.mjs windows-x64` (or `linux-x64`); versions live in `packaging/bundled-tools.json` and the licences in `THIRD_PARTY_NOTICES.md`. Without this step a Windows build bundles no tools and uses the system ones, and an AppImage build fails (it needs the fetched files); `.deb`/`.rpm` never bundle them. `node scripts/fetch-bundled-tools.mjs --sources <dir>` downloads the corresponding GPL/LGPL source archives that each release attaches.

Subsource subtitle search needs a Subsource API key. You can paste your own under **Settings → Subtitle Sources** (it is stored in your OS keyring). Distributors can alternatively bake a default key in at build time with `SUBFORGE_SUBSOURCE_API_KEY`; a key set in Settings always wins, and without either Subsource search is skipped while OpenSubtitles and translation keep working.

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
│   ├── subforge-core/        # Subtitle engine, translation lines, whisper.cpp speech-to-text, tool wrappers
│   ├── subforge-deps/          # Dependency detection + one-click install
│   ├── subforge-metadata/     # TMDB/TVmaze rename matching, filename parsing
│   └── subforge-subtitle-sources/  # OpenSubtitles/Subsource clients + fallback orchestration
├── scripts/                 # Launcher scripts
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
│  ffmpeg, subforge-core)          │
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
| Speech-to-Text — built-in whisper.cpp | ✅ Done |
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
Click **Re-check** after installing. On Windows, if you installed a tool in a custom location, add its folder to `PATH` and restart SubForge. The **Required Programs** list shows the exact path and version SubForge found for each tool (developers can also run `cargo run --example check` in `crates/subforge-deps`).

### Windows: installed tool not found right after `winget install`
SubForge also looks in the standard install folders (WinGet links, Program Files), so a fresh install should be found after **Re-check**. A restart is only needed for tools installed in custom locations: add their folder to `PATH` and reopen SubForge, because new `PATH` entries are only picked up by new processes.

---

*Built with ❤️ for fansubbers, content creators, and media enthusiasts.*
