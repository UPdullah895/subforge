# SubForge

[![CI](https://github.com/updullah/subforge/actions/workflows/ci.yml/badge.svg)](https://github.com/updullah/subforge/actions/workflows/ci.yml)

> Open-source subtitle & audio toolkit — built with Tauri 2, Vue 3, and Rust.

SubForge is a free, cross-platform desktop application for subtitle manipulation, audio extraction, speech-to-text transcription, and machine translation. This repository contains through **Phase 4**: AI and machine translation on top of the Phase 1–3 foundation.

---

## Features

### Phase 1 — Foundation
- **Track enumeration** — Open any MKV/MP4/AVI file and inspect all embedded tracks (video, audio, subtitle).
- **Subtitle extraction** — Extract individual subtitle tracks to SRT/ASS/VTT via MKVToolNix.
- **Tool discovery** — Automatically detects `mkvmerge`, `mkvextract`, and `ffmpeg` on startup.
- **Dark theme** — Fully dark UI with Tailwind CSS v4 custom design tokens.
- **Responsive layout** — Collapsible sidebar, touch-friendly controls.

### Phase 2 — Batch Operations & Audio
- **Batch directory scan** — Recursively scan directories for video files and preview tracks for each.
- **Batch subtitle extraction** — Extract subtitles from multiple files in one operation.
- **Subtitle merge** — Mux external SRT/ASS/VTT files into MKV containers.
- **Audio extraction** — Extract and transcode audio tracks (AAC, MP3, OPUS, FLAC) via FFmpeg.
- **Audio merge** — Mux external audio into existing video containers.
- **Track removal** — Strip all subtitle or all audio tracks from MKV files.
- **Subtitle format conversion** — Convert between SRT, ASS, SSA, and VTT via pysubs2.
- **Subtitle retiming** — Shift all subtitle timestamps by a configurable millisecond offset.
- **Job monitor** — Background job tracking with real-time progress events.

### Phase 3 — Speech-to-Text
- **faster-whisper transcription** — Transcribe audio/video to SRT or VTT subtitles.
- **Model management** — Select from tiny → large-v3; download and cache models.
- **GPU acceleration** — Auto-detect CUDA and run on GPU when available.
- **Word-level timestamps** — Optional per-word timing in output.
- **VAD filter** — Voice Activity Detection to reduce hallucinations on silent segments.
- **Batch transcription queue** — Process multiple files with progress bars.
- **Language auto-detection** — Automatically detect spoken language or force a specific one.

### Phase 4 — Translation
- **Ollama (local LLM)** — Context-aware subtitle translation via Ollama's OpenAI-compatible API. Batched lines with surrounding context for coherent output.
- **LibreTranslate** — Self-hosted or public traditional machine translation.
- **DeepL** — Cloud API translation (BYOK, supports free and Pro tiers).
- **OpenAI** — Cloud API translation via GPT models (BYOK).
- **Engine auto-detection** — Detect running Ollama and LibreTranslate instances.
- **Language pair configuration** — Source and target language for all engines.
- **Background job queue** — Progress tracking for batch translation operations.
- **Model selector** — Choose from available Ollama models dynamically.

---

## Tech Stack

| Layer | Technology |
|---|---|
| Desktop shell | Tauri 2 (Rust + OS WebView) |
| Frontend | Vue 3 + TypeScript + Vite |
| Styling | Tailwind CSS v4 |
| State | Pinia |
| Icons | Lucide Vue |
| Backend | Rust with `serde`, `anyhow`, `thiserror` |

---

## Prerequisites

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/)
- [MKVToolNix](https://mkvtoolnix.download/) (`mkvmerge` + `mkvextract`)
- [FFmpeg](https://ffmpeg.org/) (required for audio extraction/merge)
- Optional: set `SUBFORGE_SUBSOURCE_API_KEY` in the environment at build time (a Subsource API key created from your Subsource profile) to enable Subsource subtitle search. Without it, the build succeeds and everything else works normally — Subsource search is just silently disabled (falls through to translation). See `docs/superpowers/specs/2026-08-02-online-subtitle-search-design.md`.

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
# Clone or navigate to the project
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

Install a `.desktop` entry so you can launch SubForge from your app menu without a terminal:

```bash
./scripts/install-desktop-launcher.sh
```

This copies the icon and registers a launcher that auto-detects:
1. A release binary (after `npm run tauri build`)
2. Or falls back to `npm run tauri:dev` for development

You can also run the launcher script directly:

```bash
./scripts/launch-subforge.sh           # Desktop mode
./scripts/launch-subforge.sh --server  # Server mode
```

### Project Structure

```
subforge/
├── src/                    # Vue 3 frontend
│   ├── views/
│   │   ├── SubtitleView.vue
│   │   ├── AudioView.vue
│   │   └── BatchView.vue
│   ├── components/
│   │   ├── AppSidebar.vue
│   │   ├── TrackTable.vue
│   │   ├── AudioTrackTable.vue
│   │   ├── BatchFileList.vue
│   │   └── JobMonitor.vue
│   ├── stores/
│   │   ├── appStore.ts
│   │   ├── subtitleStore.ts
│   │   ├── audioStore.ts
│   │   └── batchStore.ts
│   ├── composables/
│   ├── App.vue
│   ├── main.ts
│   └── style.css
├── src-tauri/              # Rust backend
│   ├── src/
│   │   ├── main.rs
│   │   ├── lib.rs
│   │   ├── commands/
│   │   │   ├── mod.rs
│   │   │   ├── subtitle.rs
│   │   │   ├── audio.rs
│   │   │   └── batch.rs
│   │   ├── tools/
│   │   │   ├── mod.rs
│   │   │   ├── mkvtoolnix.rs
│   │   │   ├── ffmpeg.rs
│   │   │   └── pysubs2.rs
│   │   ├── jobs/
│   │   │   └── mod.rs
│   │   └── config.rs
│   ├── Cargo.toml
│   └── tauri.conf.json
├── scripts/
│   └── subtitle_ops.py     # pysubs2 CLI wrapper
└── package.json
```

---

## Architecture

```
┌─────────────────────────────┐
│      Vue 3 + Tailwind       │
│    SubtitleView.vue etc.    │
├─────────────────────────────┤
│     Tauri IPC (invoke)      │
├─────────────────────────────┤
│         Rust Backend        │
│  commands/subtitle.rs       │
│  tools/mkvtoolnix.rs        │
│  config.rs                  │
├─────────────────────────────┤
│   mkvmerge / mkvextract     │
└─────────────────────────────┘
```

---

## Roadmap

| Phase | Goal | Status |
|---|---|---|
| **1** | Foundation — scaffold, track enum, single-file extraction | ✅ Done |
| **2** | Batch operations — merge, remove, convert, retime | ✅ Done |
| **3** | Speech-to-Text — faster-whisper integration | ✅ Done |
| **4** | Translation — Ollama, LibreTranslate, cloud APIs | ✅ Done |
| **5** | Docker & Server Mode — REST + WebSocket API | ⏳ Planned |
| **6** | Polish — OCR, auto-sync, packaging, CI/CD | ⏳ Planned |

---

## License

GPL-3.0 — See the project plan for full licensing rationale.

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
