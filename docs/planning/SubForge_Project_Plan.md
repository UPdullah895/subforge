# SubForge — Open-Source Subtitle & Audio Toolkit

## Project Plan & Architecture Document

---

## 1. Vision

SubForge is a free, open-source, cross-platform application that unifies subtitle manipulation, audio extraction, speech-to-text transcription, and machine translation into a single cohesive tool. It targets media enthusiasts, fansubbers, content creators, and home-server operators who need powerful batch-processing capabilities without paying for fragmented commercial software.

**Primary platform:** Linux
**Secondary platform:** Windows
**Tertiary access:** Docker self-hosted server with a mobile-responsive web UI

---

## 2. Core Feature Set

### 2.1 Subtitle Operations

- **Merge subtitles** — Combine multiple subtitle files (SRT, ASS/SSA, VTT) into a single track, or mux external subtitle files into MKV/MP4 containers.
- **Remove subtitles** — Strip specific subtitle tracks from video files by language, index, or format.
- **Batch extract subtitles** — Given a file with 30+ embedded tracks, extract all (or a filtered subset) in one operation, outputting individual files named by language and index.
- **Format conversion** — Convert between SRT ↔ ASS/SSA ↔ VTT ↔ MicroDVD ↔ TTML.
- **Retime & sync** — Shift timing, adjust framerate, or auto-sync subtitles to a reference audio track.
- **OCR hardcoded subtitles** — Extract burned-in subtitles from video frames via PaddleOCR.

### 2.2 Translation

- **AI / LLM translation (Ollama)** — Context-aware subtitle translation using locally-hosted models via Ollama's OpenAI-compatible API. Sends subtitle batches with surrounding context for coherent output.
- **Traditional machine translation (LibreTranslate)** — Self-hosted, offline-capable translation powered by Argos Translate. No external API keys required.
- **Cloud API support (optional)** — Pluggable backend for DeepL, Google Translate, or OpenAI for users who prefer cloud services. Clearly marked as optional and BYOK (bring your own key).

### 2.3 Speech-to-Text

- **Audio → Subtitles** — Transcribe audio tracks to SRT/VTT using faster-whisper (CTranslate2-optimized Whisper). Supports 100+ languages, word-level timestamps, and GPU acceleration (CUDA/Vulkan).
- **Batch transcription** — Queue multiple files for unattended processing.
- **Speaker diarization** — Optionally identify and label different speakers using whisper-diarization.

### 2.4 Audio Operations

- **Extract audio tracks** — Pull specific audio tracks from video containers by language or index.
- **Merge audio tracks** — Mux external audio files into existing video containers.
- **Remove audio tracks** — Strip unwanted audio streams.
- **Audio format conversion** — Transcode between formats (AAC, OPUS, FLAC, MP3) during extraction or merge.

---

## 3. Technology Stack

### 3.1 Application Framework

| Component | Choice | Rationale |
|---|---|---|
| **Desktop shell** | **Tauri 2** (Rust + WebView) | ~96% smaller binaries than Electron, native OS WebView, cross-platform (Linux/Windows/macOS), Rust backend for safe CLI orchestration. Already used successfully in OpenGG. |
| **Frontend** | **Vue 3 + TypeScript + Vite** | Familiar stack, excellent Tauri integration, fast HMR, rich ecosystem. |
| **UI library** | **Shadcn-vue + Tailwind CSS** | Clean design tokens, dark theme, RTL-ready, no vendor lock-in. |
| **Docker web UI** | Same Vue 3 frontend served via a lightweight HTTP server (e.g. Caddy or nginx), with a Python/Rust API backend exposed over REST + WebSocket for job progress. |

### 3.2 Backend CLI Tools (Orchestrated by Rust)

These are the proven open-source tools that SubForge wraps via CLI subprocess calls from Rust. Each tool is battle-tested, well-maintained, and freely licensed.

#### Subtitle & Container Manipulation

| Tool | GitHub | Role | License |
|---|---|---|---|
| **MKVToolNix** (`mkvmerge`, `mkvextract`, `mkvinfo`) | [moritz-wundke/MKVToolNix](https://mkvtoolnix.org/) | Merge, extract, and remove subtitle/audio tracks from MKV containers. The backbone for all muxing operations. | GPL-2.0 |
| **FFmpeg** | [FFmpeg/FFmpeg](https://github.com/FFmpeg/FFmpeg) | Universal media Swiss Army knife — extract/merge audio, transcode, handle MP4/MKV/TS containers, extract audio for Whisper input. | LGPL-2.1 / GPL |
| **pysubs2** | [tkarabela/pysubs2](https://github.com/tkarabela/pysubs2) | Python library for parsing, editing, converting, retiming subtitle files (SRT, ASS/SSA, VTT, MicroDVD, TMP, TTML). Zero dependencies, MIT licensed. | MIT |
| **Subtitle Edit CLI** | [SubtitleEdit/subtitleedit-cli](https://github.com/SubtitleEdit/subtitleedit-cli) | Batch conversion between 300+ subtitle formats, merge short lines, fix common errors. Complements pysubs2 for edge-case formats. | GPL-3.0 |

#### Speech-to-Text

| Tool | GitHub | Role | License |
|---|---|---|---|
| **faster-whisper** | [SYSTRAN/faster-whisper](https://github.com/SYSTRAN/faster-whisper) | CTranslate2-based Whisper reimplementation — up to 4× faster than OpenAI's original with 8-bit quantization on CPU and GPU. Primary ASR engine. | MIT |
| **whisper-diarization** | [MahmoudAshraf97/whisper-diarization](https://github.com/MahmoudAshraf97/whisper-diarization) | Speaker diarization pipeline on top of faster-whisper for multi-speaker labeling. | MIT |
| **Silero VAD** | [snakers4/silero-vad](https://github.com/snakers4/silero-vad) | Voice Activity Detection to split audio into speech segments before transcription, improving accuracy and speed. | MIT |

#### Translation

| Tool | GitHub | Role | License |
|---|---|---|---|
| **Ollama** | [ollama/ollama](https://github.com/ollama/ollama) | Local LLM server with OpenAI-compatible API. Runs quantized models (Qwen3, Gemma, Mistral, Aya Expanse) for context-aware subtitle translation. | MIT |
| **LLM-Subtrans** | [machinewrapped/llm-subtrans](https://github.com/machinewrapped/llm-subtrans) | Reference implementation for LLM-based subtitle translation with batching, context windows, and resume support. Code patterns and prompt strategies are reusable. | MIT |
| **chatgpt-subtitle-translator** | [Cerlancism/chatgpt-subtitle-translator](https://github.com/Cerlancism/chatgpt-subtitle-translator) | Efficient line-based SRT translation via any OpenAI-compatible API (including Ollama). Structured output support, multi-pass agentic translation. | MIT |
| **LibreTranslate** | [LibreTranslate/LibreTranslate](https://github.com/LibreTranslate/LibreTranslate) | Self-hosted, offline-capable machine translation REST API powered by Argos Translate. No external dependencies. Ships as a Docker image. | AGPL-3.0 |
| **translator-ollama (CALT)** | [lin16303/translator-ollama](https://github.com/lin16303/translator-ollama) | Context-aware translation using local Ollama models with self-refinement algorithms and confidence scoring. Good reference for quality assessment. | MIT |

#### OCR (Hardcoded Subtitle Extraction)

| Tool | GitHub | Role | License |
|---|---|---|---|
| **VideOCR** | [timminator/VideOCR](https://github.com/timminator/VideOCR) | Extract hardcoded subtitles from video via PaddleOCR or Google Lens hybrid mode. Supports 200+ languages, smart frame deduplication, and automatic subtitle merging. | GPL-3.0 |
| **PaddleOCR** | [PaddlePaddle/PaddleOCR](https://github.com/PaddlePaddle/PaddleOCR) | State-of-the-art multilingual OCR engine. Runs fully locally. | Apache-2.0 |

#### Batch Processing & Sync Reference

| Tool | GitHub | Role | License |
|---|---|---|---|
| **MKVToolNix Batch Tool** | [iPzard/mkvtoolnix-batch-tool](https://github.com/iPzard/mkvtoolnix-batch-tool) | Reference architecture for batch subtitle merge/extract workflows across directory trees. Windows-focused but patterns are portable. | MIT |
| **Sushi Batch** | [C1fer/sushi-batch](https://github.com/C1fer/sushi-batch) | Batch subtitle synchronization — auto-sync subtitles between different video releases using audio fingerprinting. | MIT |
| **subgen** | [McCloudS/subgen](https://github.com/McCloudS/subgen) | Auto-generate subtitles for Jellyfin/Plex/Emby libraries using Whisper. Good reference for media-server integration patterns. | MIT |

---

## 4. Architecture

```
┌─────────────────────────────────────────────────────┐
│                    SubForge UI                      │
│           Vue 3 + TypeScript + Tailwind             │
│    ┌──────────┬──────────┬──────────┬──────────┐    │
│    │ Subtitle │ Translate│  Audio   │  Speech  │    │
│    │  Editor  │  Panel   │  Mixer   │  to Text │    │
│    └──────────┴──────────┴──────────┴──────────┘    │
├─────────────────────────────────────────────────────┤
│              Tauri IPC Bridge (invoke)               │
├─────────────────────────────────────────────────────┤
│                  Rust Backend Core                    │
│  ┌─────────────┬────────────────┬────────────────┐  │
│  │ Job Queue & │  CLI Process   │  Config &      │  │
│  │  Scheduler  │  Orchestrator  │  State Store   │  │
│  └─────────────┴────────────────┴────────────────┘  │
│        │               │                │            │
│  ┌─────┴───────────────┴────────────────┴─────┐     │
│  │          Subprocess Execution Layer          │     │
│  │  mkvmerge · mkvextract · ffmpeg · python    │     │
│  │  faster-whisper · pysubs2 · ollama-api      │     │
│  └─────────────────────────────────────────────┘     │
└─────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────┐
│               Docker Server Mode                     │
│  ┌───────────────────────────────────────────────┐  │
│  │  Caddy/nginx → Same Vue 3 SPA                 │  │
│  │  Python/Rust REST API (FastAPI or Axum)        │  │
│  │  WebSocket for real-time job progress           │  │
│  │  Mobile-responsive layout (touch-friendly)      │  │
│  ├───────────────────────────────────────────────┤  │
│  │  Service containers:                           │  │
│  │    subforge-core (job runner)                  │  │
│  │    faster-whisper (GPU-enabled optional)        │  │
│  │    libretranslate (translation engine)          │  │
│  │    ollama (optional, user-managed)              │  │
│  └───────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
```

### 4.1 Desktop Mode (Tauri 2)

The Rust backend manages a job queue and spawns CLI subprocesses for each operation. The frontend communicates via Tauri's `invoke` IPC. Long-running jobs (transcription, batch extraction) emit progress events over Tauri's event system for real-time UI updates.

Key Rust responsibilities:
- Discover and validate external tool availability (mkvtoolnix, ffmpeg, python, ollama).
- Parse `mkvmerge -J` JSON output to enumerate tracks in video files.
- Manage temporary files and cleanup.
- Serialize/deserialize job definitions for queue persistence.

### 4.2 Docker / Server Mode

A `docker-compose.yml` orchestrates multiple containers. The same Vue 3 frontend is served as a static SPA, but the backend switches from Tauri IPC to a REST + WebSocket API (using Axum in Rust or FastAPI in Python). Users access the interface from any browser, including mobile phones on the local network.

The Docker version bundles all dependencies (ffmpeg, mkvtoolnix, faster-whisper, pysubs2, libretranslate) so there's zero manual setup.

### 4.3 Mobile Access

The Docker web UI is built mobile-first with responsive Tailwind layouts. Core interactions (upload file → configure job → start → monitor progress → download result) are optimized for touch. No native mobile app is needed; the browser interface is sufficient.

---

## 5. Development Phases

### Phase 1 — Foundation (Weeks 1–4)

**Goal:** A working Tauri 2 shell that can enumerate tracks in a video file and perform basic subtitle extraction.

- Scaffold Tauri 2 + Vue 3 + TypeScript project.
- Build the Rust subprocess orchestrator for `mkvmerge -J` (track enumeration) and `mkvextract` (subtitle extraction).
- Implement the file picker and track list UI.
- Single-file subtitle extraction working end-to-end.
- Basic dark theme with the design system.

### Phase 2 — Batch Operations (Weeks 5–8)

**Goal:** Batch extraction, merge, and removal of subtitles and audio tracks across multiple files.

- Directory scanner that recursively finds video files.
- Batch job queue with progress tracking.
- Subtitle merge (external SRT/ASS → MKV) via `mkvmerge`.
- Audio extraction and merge via `ffmpeg`.
- Track removal (subtitle and audio) via `mkvmerge`.
- Subtitle format conversion via `pysubs2` (SRT ↔ ASS ↔ VTT).
- Retime/shift subtitles.

### Phase 3 — Speech-to-Text (Weeks 9–12)

**Goal:** Transcribe audio to subtitles using faster-whisper.

- Integrate faster-whisper via Python subprocess (or direct ctranslate2 bindings).
- Model management UI (download/select Whisper models: tiny → large-v3).
- GPU detection and configuration (CUDA, Vulkan).
- Batch transcription queue.
- Word-level timestamps and optional speaker diarization.
- Output as SRT or VTT with configurable segment length.

### Phase 4 — Translation (Weeks 13–16)

**Goal:** Translate subtitles via Ollama (AI), LibreTranslate (traditional), or cloud APIs.

- Ollama integration: detect running instance, list available models, send batched subtitle context.
- LibreTranslate integration: REST API calls for traditional MT.
- Translation engine selection UI with language pair configuration.
- Side-by-side preview (original ↔ translated).
- Batch translation queue.
- Optional cloud API support (DeepL, OpenAI) with BYOK key management.

### Phase 5 — Docker & Server Mode (Weeks 17–20)

**Goal:** A `docker-compose` deployment that serves the full application via a web browser.

- Extract the backend into a standalone REST + WebSocket API (Axum or FastAPI).
- Write `Dockerfile` and `docker-compose.yml` with service containers.
- File upload/download via the web interface.
- Mobile-responsive layout testing.
- GPU passthrough configuration for faster-whisper (nvidia-container-toolkit).
- Health checks and basic auth.

### Phase 6 — Polish & Release (Weeks 21–24)

**Goal:** Public beta release.

- OCR hardcoded subtitle extraction (PaddleOCR integration).
- Auto-sync subtitles to audio (Sushi-based approach).
- Settings persistence, theme customization, language/locale support.
- AppImage + .deb + .rpm packaging for Linux.
- NSIS installer for Windows.
- Documentation site, README, and contribution guide.
- GitHub Actions CI/CD for automated builds and releases.

---

## 6. Monetization Strategy

The project is free and not-for-profit at its core. All features are available to all users. Optional monetization paths that don't compromise the open-source mission:

| Path | Description |
|---|---|
| **Donations** | GitHub Sponsors, Open Collective, Ko-fi. Prominent but non-intrusive links in the app's About page and README. |
| **Premium Docker hosting** | Offer a managed cloud-hosted version (similar to LibreTranslate's portal model) for users who don't want to self-host. Monthly subscription covers server costs + margin. |
| **Priority support tier** | Paid tier for businesses or studios that need guaranteed response times on bug reports or feature requests. |
| **Bounty board** | Community-funded feature bounties via platforms like IssueHunt or Polar. |

**What will NOT happen:** No ads, no telemetry, no feature gating, no proprietary forks.

---

## 7. Licensing

| Component | Recommended License |
|---|---|
| SubForge core application | **GPL-3.0** (compatible with MKVToolNix GPL-2.0, VideOCR GPL-3.0, and all MIT dependencies) |
| Vue 3 frontend (if separable) | **MIT** (allows embedding in the GPL application while being independently reusable) |
| Docker configuration | **MIT** |
| Documentation | **CC-BY-4.0** |

GPL-3.0 is chosen because the project wraps GPL-licensed tools (MKVToolNix, Subtitle Edit CLI, VideOCR) and the copyleft requirement ensures the project and all derivatives remain free and open.

---

## 8. Repository Structure

```
subforge/
├── src-tauri/              # Rust backend (Tauri 2)
│   ├── src/
│   │   ├── main.rs
│   │   ├── commands/       # Tauri invoke handlers
│   │   │   ├── subtitle.rs
│   │   │   ├── audio.rs
│   │   │   ├── transcribe.rs
│   │   │   └── translate.rs
│   │   ├── jobs/           # Job queue and scheduler
│   │   ├── tools/          # CLI tool wrappers
│   │   │   ├── mkvtoolnix.rs
│   │   │   ├── ffmpeg.rs
│   │   │   ├── whisper.rs
│   │   │   └── pysubs2.rs
│   │   └── config.rs
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/                    # Vue 3 frontend
│   ├── components/
│   ├── views/
│   │   ├── SubtitleView.vue
│   │   ├── AudioView.vue
│   │   ├── TranscribeView.vue
│   │   └── TranslateView.vue
│   ├── composables/
│   ├── stores/             # Pinia state management
│   └── App.vue
├── server/                 # Docker server API
│   ├── src/                # Axum REST + WebSocket API
│   └── Dockerfile
├── docker/
│   ├── docker-compose.yml
│   ├── docker-compose.gpu.yml
│   └── Caddyfile
├── scripts/                # Python helper scripts
│   ├── transcribe.py       # faster-whisper wrapper
│   ├── translate_ollama.py # Ollama translation pipeline
│   └── subtitle_ops.py    # pysubs2 operations
├── docs/
├── .github/
│   └── workflows/
│       ├── build.yml       # CI build + test
│       └── release.yml     # Tagged release packaging
├── LICENSE
├── README.md
└── package.json
```

---

## 9. External Tool Dependency Matrix

This table maps each SubForge feature to the external tools it requires, helping users understand what to install and helping the app's setup wizard validate the environment.

| Feature | mkvtoolnix | ffmpeg | faster-whisper | pysubs2 | Ollama | LibreTranslate | PaddleOCR |
|---|---|---|---|---|---|---|---|
| Extract subtitles | ✅ | — | — | — | — | — | — |
| Merge subtitles | ✅ | — | — | — | — | — | — |
| Remove subtitles | ✅ | — | — | — | — | — | — |
| Extract audio | — | ✅ | — | — | — | — | — |
| Merge audio | — | ✅ | — | — | — | — | — |
| Remove audio | ✅ | ✅ | — | — | — | — | — |
| Convert subtitle format | — | — | — | ✅ | — | — | — |
| Retime subtitles | — | — | — | ✅ | — | — | — |
| Audio → Subtitles | — | ✅ | ✅ | — | — | — | — |
| AI translation | — | — | — | — | ✅ | — | — |
| Traditional translation | — | — | — | — | — | ✅ | — |
| OCR hardcoded subs | — | ✅ | — | — | — | — | ✅ |

---

## 10. Docker Compose Reference

```yaml
version: "3.8"

services:
  subforge:
    build: ./server
    ports:
      - "8080:8080"
    volumes:
      - ./media:/data/media
      - ./output:/data/output
    environment:
      - OLLAMA_HOST=http://ollama:11434
      - LIBRETRANSLATE_HOST=http://libretranslate:5000
    depends_on:
      - libretranslate

  libretranslate:
    image: libretranslate/libretranslate:latest
    ports:
      - "5000:5000"
    volumes:
      - lt-models:/home/libretranslate/.local

  # Optional: uncomment if you want local Ollama
  # ollama:
  #   image: ollama/ollama:latest
  #   ports:
  #     - "11434:11434"
  #   volumes:
  #     - ollama-models:/root/.ollama
  #   deploy:
  #     resources:
  #       reservations:
  #         devices:
  #           - capabilities: [gpu]

volumes:
  lt-models:
  # ollama-models:
```

For GPU-accelerated transcription, an override file (`docker-compose.gpu.yml`) adds NVIDIA runtime configuration to the subforge service.

---

## 11. Key GitHub Repositories — Quick Reference

| Repository | Stars | What to Take From It |
|---|---|---|
| [SYSTRAN/faster-whisper](https://github.com/SYSTRAN/faster-whisper) | 14k+ | Primary ASR engine, API patterns |
| [LibreTranslate/LibreTranslate](https://github.com/LibreTranslate/LibreTranslate) | 14k+ | Self-hosted translation API, Docker patterns |
| [ollama/ollama](https://github.com/ollama/ollama) | 130k+ | Local LLM server, OpenAI-compatible API |
| [SubtitleEdit/subtitleedit](https://github.com/SubtitleEdit/subtitleedit) | 12k+ | Format support reference, batch conversion patterns |
| [machinewrapped/llm-subtrans](https://github.com/machinewrapped/llm-subtrans) | 2k+ | LLM translation prompt engineering, batching strategy |
| [Cerlancism/chatgpt-subtitle-translator](https://github.com/Cerlancism/chatgpt-subtitle-translator) | 2k+ | Structured output translation, Ollama compatibility |
| [tkarabela/pysubs2](https://github.com/tkarabela/pysubs2) | 700+ | Subtitle parsing/editing library, MIT licensed |
| [iPzard/mkvtoolnix-batch-tool](https://github.com/iPzard/mkvtoolnix-batch-tool) | 600+ | Batch muxing UI/UX patterns |
| [timminator/VideOCR](https://github.com/timminator/VideOCR) | 400+ | Hardcoded subtitle OCR pipeline |
| [C1fer/sushi-batch](https://github.com/C1fer/sushi-batch) | 200+ | Audio-based subtitle sync patterns |
| [McCloudS/subgen](https://github.com/McCloudS/subgen) | 1k+ | Media server integration, auto-subtitle generation |
| [tauri-apps/tauri](https://github.com/tauri-apps/tauri) | 90k+ | Application framework |

---

## 12. Risk & Mitigation

| Risk | Impact | Mitigation |
|---|---|---|
| faster-whisper model size (1–3 GB) | Large download for first-time users | Ship with tiny model, offer in-app download manager for larger models. Docker pre-bundles selected model. |
| Ollama requires significant VRAM | Users without GPU get slow translation | LibreTranslate as zero-GPU fallback. Cloud API as opt-in alternative. |
| GPL license complexity | Contributor friction | Clear CONTRIBUTING.md, CLA-free, DCO sign-off only. |
| Cross-platform CLI path differences | Windows vs Linux tool discovery | Rust backend abstracts tool paths with `which`/`where` + configurable overrides in settings. |
| WebKitGTK rendering differences on Linux | UI bugs on older distros | AppImage bundles WebKitGTK 4.1. Minimum supported: Ubuntu 22.04+, Fedora 38+, Arch current. |

---

## 13. Success Metrics

- First public release within 6 months of development start.
- 500+ GitHub stars within the first year.
- Packaged for at least 3 Linux distribution formats (AppImage, .deb, Flatpak).
- Docker image with under 5-minute setup time.
- Community contributions from at least 5 external developers.
- Featured on r/selfhosted, r/linux, or equivalent communities.

---

*This document is a living plan. Update it as architectural decisions evolve and new tools emerge.*
