# SubForge

[![CI](https://github.com/UPdullah895/subforge/actions/workflows/ci.yml/badge.svg)](https://github.com/UPdullah895/subforge/actions/workflows/ci.yml)

> Open-source subtitle & audio toolkit — built with Tauri 2, Vue 3, and Rust.

SubForge is a free, cross-platform desktop application for subtitle manipulation, audio extraction, speech-to-text transcription, and machine translation. It also ships as a standalone server (REST + WebSocket API) for headless/self-hosted use.

---

## Download

Prebuilt installers for **Linux** and **Windows** are published on the [Releases page](https://github.com/UPdullah895/subforge/releases) for every tagged version:

| Platform | Formats |
|---|---|
| Linux | `.AppImage`, `.deb`, `.rpm` |
| Windows | `.msi`, `.exe` (NSIS installer) |
| macOS | Not currently published — build from source (see below); best-effort support only |

No setup needed to run a downloaded build — just install and launch. Building from source has a couple of extra prerequisites (below).

---

## Features

### Subtitle & audio tools
- **Track enumeration** — open any MKV/MP4/AVI file and inspect all embedded tracks (video, audio, subtitle).
- **Subtitle extraction** — extract individual subtitle tracks to SRT/ASS/VTT via MKVToolNix.
- **Subtitle merge** — mux external SRT/ASS/VTT files into MKV containers.
- **Audio extraction/merge** — extract, transcode (AAC, MP3, OPUS, FLAC), and remux audio via FFmpeg.
- **Subtitle format conversion & retiming** — convert between SRT/ASS/SSA/VTT, shift timestamps via pysubs2.
- **Tool discovery** — automatically detects `mkvmerge`, `mkvextract`, and `ffmpeg` on startup.

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

## Prerequisites (building from source)

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/)
- [MKVToolNix](https://mkvtoolnix.download/) (`mkvmerge` + `mkvextract`)
- [FFmpeg](https://ffmpeg.org/) (required for audio extraction/merge)
- Optional: set `SUBFORGE_SUBSOURCE_API_KEY` in the environment at build time (a Subsource API key created from your Subsource profile) to enable Subsource subtitle search. Without it, the build succeeds and everything else works normally — Subsource search is just silently disabled (falls through to translation).

### Install MKVToolNix

**Arch Linux / CachyOS:**
```bash
sudo pacman -S mkvtoolnix-cli
```

**Ubuntu / Debian:**
```bash
sudo apt-get install mkvtoolnix
```

**Fedora:**
```bash
sudo dnf install mkvtoolnix
```

**macOS:**
```bash
brew install mkvtoolnix ffmpeg
```

### Install pysubs2 (for subtitle conversion)
```bash
pip install pysubs2 faster-whisper
```

Optional (for speaker diarization):
```bash
pip install whisper-diarization pyannote.audio
```

---

## Development

```bash
# Clone the repo
git clone https://github.com/UPdullah895/subforge.git
cd subforge

# Install frontend dependencies
npm install

# Run the Tauri dev server (opens a native window)
npm run tauri dev

# On Wayland (if you see a Wayland protocol error):
npm run tauri:dev:wayland

# Or build for production
npm run tauri build
```

### Desktop Launcher (Linux)

Two options:

**Quick, no-install:** double-click [`SubForge.desktop`](SubForge.desktop) at the repo root directly in your file manager. If it doesn't run on first double-click, right-click it once → "Allow Launching" (varies slightly by desktop environment).

**Proper app-menu entry:**
```bash
./scripts/install-desktop-launcher.sh
```
This copies the icon and registers a launcher in your application menu. Both launchers auto-detect a release binary (after `npm run tauri build`) and fall back to `npm run tauri:dev` for development.

You can also run the launcher script directly:
```bash
./scripts/launch-subforge.sh           # Desktop mode
./scripts/launch-subforge.sh --server  # Server mode
```

### Project Structure

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

GPL-3.0 — see the project plan for full licensing rationale.

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

---

*Built with ❤️ for fansubbers, content creators, and media enthusiasts.*
