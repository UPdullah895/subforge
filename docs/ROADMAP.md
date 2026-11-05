# SubForge roadmap — from a subtitle tool to a media-server workbench

This is the plan from what exists today to the product goal: **a tool a media-server owner
(Jellyfin/Emby/Plex) can point at a library, let an AI drive, and trust to fill in missing
subtitles and audio by itself.**

It is a map, not a promise of dates. Each phase ends with a *done-when* that can be checked
by running the app, and every phase is useful on its own.

---

## 1. Where SubForge is today

Verified against the code in this repository (`crates/`, `src-tauri/`, `server/`, `src/`).

| Capability | State | Where |
|---|---|---|
| Read every track in an MKV/MP4/AVI (video, audio, subtitle, languages) | ✅ | `tools/mkvtoolnix.rs::enumerate_tracks`, `tools/ffmpeg.rs::probe_media` |
| Extract subtitles (one, all, by language) | ✅ | `mkvtoolnix.rs::extract_*` |
| Merge external subtitles into a container, with language / track name / default & SDH flags | ✅ | `mkvtoolnix.rs::merge_subtitles` |
| Remove subtitle or audio tracks by kind/language; edit track flags in place | ✅ | `mkvtoolnix.rs::remove_tracks`, `set_track_flags` |
| Audio extract / transcode (AAC, MP3, OPUS, FLAC) / merge one extra track | ✅ | `tools/ffmpeg.rs::extract_audio`, `merge_audio` |
| Convert SRT/ASS/SSA/VTT, shift & stretch timings, keep ASS styles/fonts | ✅ | `subtitle/` |
| Credit/ad cleanup with preview and `.bak` | ✅ | `subtitle/clean.rs` |
| Speech-to-text (whisper.cpp, GPU via Vulkan, CPU fallback) | ✅ | `whisper/` |
| Machine translation — Ollama, LibreTranslate, DeepL, OpenAI, Google | ✅ | `translate.rs` |
| Find an existing human subtitle before translating (Subsource / OpenSubtitles) | ✅ | `crates/subforge-subtitle-sources` |
| Sync check + ASR-based offset detection (delay, drift, wrong episode) | ✅ | `subtitle/sync.rs` |
| Batch: scan a directory, extract, merge, rename from TMDB/TVmaze | ◑ partial | `routes/batch.rs`, `rename*` |
| Batch translate across many files | ◑ partial | `BatchView` + `translate` |
| Server mode: REST + WebSocket, same UI in a browser | ✅ | `server/` |
| Job queue: list, pause, resume, cancel | ◑ partial | `server/src/jobs/mod.rs` — in memory, no concurrency limit, lost on restart |
| **Control from an AI (MCP)** | ❌ missing | — |
| **Automatic library pipeline** (detect missing language → translate → mux → verify → delete source) | ❌ missing | — |
| **Multi-language audio dubbing / alignment / mixing** | ❌ missing | merge_audio copies one track, no offset detection, no mixing |
| **OCR** (image subtitles: PGS/VobSub, or burned-in text) | ❌ missing | subtitle module is text formats only |
| **Video splitting** (chapters, duration, scene/silence) | ❌ missing | — |
| **AI vision content scan → per-segment warnings** | ❌ missing | — |
| **Video + waveform + cue editor** (Subtitle Composer-like) | ❌ missing | no player, no waveform in the UI |
| Docker packaging for server mode | ❌ missing | — |

Legend: ✅ done · ◑ partial · ❌ missing.

Reusable parts worth calling out, because they shorten later phases:

- `subtitle/sync.rs` already transcribes short windows and matches them to cues to compute a
  **median offset** — that is most of the work behind *dub* alignment too.
- `mkvtoolnix.rs` already knows track languages and flags, which is exactly what "does this
  episode have Arabic?" needs.
- `server/` already exposes ~35 endpoints; an MCP server is a thin typed wrapper, not a rewrite.

---

## 2. Target architecture

```
Desktop (Tauri + Vue)      Web UI (browser)      AI agents (MCP clients)
        │                        │                        │
        └──────────┬─────────────┴───────────┬────────────┘
                   ▼                         ▼
        REST + WebSocket (exists)      MCP server (new: stdio + HTTP)
                   │                         │
                   └───────────┬─────────────┘
                               ▼
                    Scheduler  (new: one heavy job at a time,
                   │            priority, persistence, resume)
                   ▼
        Library watcher + policy engine (new)
                   │
                   ▼
   crates/subforge-core  — subtitle · tools(ffmpeg/mkvtoolnix) · whisper ·
                           translate · sync   +   new: audio-align · ocr ·
                           vision · split · pipeline
```

New crates: `subforge-mcp`, `subforge-pipeline` (policy + steps + verification),
`subforge-ocr`, `subforge-vision`. `subforge-core` keeps the primitives.

---

## 3. Phases

### P0 — Job scheduler and queue (everything else leans on this) — ~1–2 weeks

The goal "one heavy operation at a time, and vision only after transcription" is a scheduler
property, not a UI trick.

- [ ] Global concurrency limit (default **1** heavy job) with per-kind weights: transcription and
      vision never run together; cheap jobs (probe, convert, mux) may run while a heavy one runs.
- [ ] Priority: user-initiated > watcher-initiated; per-library fairness so one library cannot starve another.
- [ ] Persist the queue (SQLite) and resume pending jobs after a restart or crash.
- [ ] Per-file lock + temp-file convention (`file.mkv.part`) so two jobs cannot write the same episode.
- [ ] Progress + stage name on the existing WebSocket; `job.stage` and `job.eta`.

**Done when:** two batch jobs are started → the second shows `queued`, the first finishes, and
killing the process mid-job leaves the queue intact and resumable.

### P1 — MCP server (AI control) — ~1–2 weeks

- [ ] `subforge-mcp` on the official Rust MCP SDK (`rmcp`), transports: **stdio** (local agents) and
      **streamable HTTP** (network agents), both mapping onto existing core functions — not onto
      shell commands.
- [ ] Tools (structured JSON in/out, every heavy one returns a `job_id`):
      `scan_library`, `list_tracks`, `find_missing_languages`, `extract_subtitles`,
      `transcribe`, `translate`, `search_online_subtitles`, `merge_subtitles`, `remove_tracks`,
      `clean_subtitles`, `convert_subtitle`, `shift_subtitle`, `align_audio`, `split_video`,
      `ocr_subtitles`, `vision_scan`, `job_status`, `job_cancel`, `run_policy`, `report`.
- [ ] Resources: library inventory, job queue, settings schema. Prompts: "fill Arabic for this
      library", "check sync for season 2".
- [ ] Guard rails: tools that destroy data (`remove_tracks`, replace/mux, delete source) require an
      explicit `confirm: true` or a `dry_run` first.

**Done when:** Claude Desktop (or any MCP client) lists the tools and completes
*detect missing → translate → mux → verify → clean up* on one episode without a shell command.

### P2 — Library pipeline: the media-server loop — ~2–3 weeks

- [ ] Episode identity: reuse `subforge-metadata` to parse `SxxExx`, series, title; map Jellyfin
      layout (`Series/Season NN/Series S01E01.mkv`).
- [ ] Language inventory per file: video/audio/subtitle languages + flags → "missing ar, has en".
- [ ] Policy file (per library): target languages, prefer human subtitle vs machine translation,
      engines allowed, timeouts, size limits, dry-run, what to do with the source subtitle.
- [ ] Steps, each resumable and idempotent: `acquire` (online search → else `transcribe` → `translate`)
      → `mux` (mkvmerge, language + SDH + default flags) → `verify` (re-probe: track present,
      duration matches, sync check passes) → `publish` (atomic replace + `.bak`) → `cleanup`
      (delete the external subtitle **only after** verify).
- [ ] Watcher: `notify` + debounce, initial scan, ignore `.part`/`.tmp`/files still growing.
- [ ] Safety: dry-run mode on by default for a new library; quarantine folder for failures;
      one JSON log line per step; never modify a file that another process has open.
- [ ] Notification hook (optional): webhook or Jellyfin "scan library" request after a successful publish.

**Done when:** an episode with no Arabic subtitle is dropped into a watched folder; within N
minutes the MKV carries an `ar` track, the external file is gone, the log explains every step, and
a deliberately broken file leaves the original untouched.

### P3 — Multi-language audio: align, mix, mux — ~3–4 weeks

The hard, differentiating phase.

- [ ] Audio inventory + language tagging; several dub tracks per file.
- [ ] Offset detection per dub track, two signals combined:
      1. ASR windows (reuse `subtitle/sync.rs`) matched across languages via cue/anchor alignment;
      2. cross-correlation of music/SFX passages (FFT in Rust, no AI needed) for a sample-accurate offset.
- [ ] Drift detection and per-scene correction (cuts/splits where the dub re-edited the episode).
- [ ] Mixer: keep the original bed and duck it under the dub
      (`sidechaincompress`, or `amix` with a volume envelope); modes: *dub only*, *dub + original
      background*, *original SFX preserved*.
- [ ] Mux with `mkvmerge`: language, track name, default/forced flags, per-track channels.
- [ ] Manual editor: waveform + per-segment offset nudge, "snap to nearest cue" and A/B preview.
- [ ] Optional AI assist: a video-language model (LFM3-VL, Qwen-VL) checks mouth movement against
      audio energy on disagreements — used only as a tie-breaker, never as the primary signal.

**Done when:** a Japanese episode plus an Arabic dub file becomes an MKV with aligned `ar` audio
(±40 ms on music passages, no visible drift over 24 minutes), original `ja` track intact, and a
manual nudge is possible without leaving the app.

### P4 — OCR: image subtitles and burned-in text — ~2 weeks

- [ ] PGS/VobSub tracks: `mkvextract` → `.sup`/`.idx` → OCR (tesseract with per-language
      traineddata, bundled like FFmpeg) → SRT with the original timings; review UI with the frame
      next to each line.
- [ ] Burned-in text: frame sampling (scene-change + 1 fps), text-region detection, OCR, dedupe,
      cue building from frame timestamps; mark low-confidence lines.
- [ ] Arabic/Persian/Urdu shaping and RTL punctuation handled explicitly (the subtitles this tool
      cares about most are the ones OCR tools usually mangle).

**Done when:** an anime episode with PGS subtitles yields an SRT at ≥95 % character accuracy on a
30-line sample, and burned-in OP/ED text is detected and transcribed.

### P5 — Splitting + AI content scan — ~2 weeks

- [ ] Split: by chapters (`mkvmerge -J`), fixed duration, silence/scene boundaries (`ffmpeg scdet`),
      always keyframe-aligned and stream-copied (no re-encode), output naming per policy.
- [ ] Vision scan: sample frames (dedupe by frame diff), ask a local VLM (LFM3-VL / Qwen-VL via
      Ollama) a strict taxonomy (nudity, sexual content, gore, violence, drugs) → JSON with
      timestamps + confidence; human-review screen to accept/reject.
- [ ] Segment warnings: attach per-segment markers as MKV chapters (plus optional Jellyfin `.nfo`
      or a sidecar JSON), so a player can skip warned ranges.
- [ ] Runs at low priority **after** transcription completes (guaranteed by P0), never in parallel.

**Done when:** a season is split into segments where each segment reports its flagged ranges, the
whole run finishes overnight with the queue never running two heavy jobs at once, and a sample of
50 flags has a measured false-positive rate.

### P6 — Visual subtitle editor (Subtitle Composer-like) — ~3–4 weeks

The part that makes the tool pleasant instead of batch-only.

- [ ] Playback inside the app. MKV cannot be played by WebKit reliably, so: build a **proxy**
      (ffmpeg → fragmented MP4, 480p, audio kept) on first open, cached next to the config dir;
      later, optional libmpv embedding for native playback and frame accuracy.
- [ ] Waveform strip from `ffmpeg` peak data, draggable zoom, cue blocks over the audio.
- [ ] Cue list: inline edit, drag start/end with snapping to frames and to neighbours, split/merge,
      shift-all, keyboard-first workflow, full undo/redo.
- [ ] Side-by-side: video above, waveform right, cues left, source text below — the layout the
      screenshot in the brief shows.
- [ ] Optional: live ASS preview (fonts, positioning) using the existing ASS parser.

**Done when:** open an episode and its SRT, drag a cue onto the right word, undo it, save, and the
file is valid for every player — without launching another program.

### P7 — Ship it — ~1 week, split across phases

- [ ] Docker image for server mode (multi-stage, GPU optional, `SUBFORGE_*` env config).
- [ ] Docs: MCP tools reference, policy file reference, Jellyfin cookbook.
- [ ] Packaging: the AUR packages this branch prepares, plus a Flatpak consideration for Linux.

---

## 4. Order and dependencies

```
P0 ──► P1 ──► P2 ──► P3
 │      │      └──► P4 ──► P6
 │      └──► P5 (needs P0 for sequencing, P2 for segments)
 └──► P7 (Docker can start once P2 is stable)
```

Rough total: **3–4 months** at a steady single-developer pace, with P0–P2 (≈5–6 weeks) already
delivering the "media server fills its own subtitles" promise.

---

## 5. Risks and things to decide

| Risk | Why it matters | Mitigation |
|---|---|---|
| MKV playback in the webview | Blocks the editor (P6) | proxy transcode first; libmpv later |
| Dub alignment accuracy | The headline feature must not be "almost right" | two independent signals + manual nudge; measure on real dubs before claiming support |
| Vision false positives | Wrong warnings are worse than none | taxonomy + confidence + human review; ship as markers, not cut points |
| Automation touching a library | One bad write loses an episode | dry-run default, `.bak`, quarantine, verify-before-delete |
| Local AI on 8 GB VRAM | LFM3-VL / Qwen-VL + whisper compete for the GPU | queue serialises them; small quantised models by default |
| Bundling tesseract/OCR data | New licences and size | treat like the existing bundled FFmpeg/MKVToolNix pipeline (`packaging/bundled-tools.json`) |

Decisions needed before the map is final:

1. **Automation trigger**: watch the library folders directly, or talk to Jellyfin's API/plugin?
2. **Dub alignment**: start deterministic (ASR + cross-correlation), or invest in an AI model from the start?
3. **MCP transport**: local stdio only, or network HTTP as well (agents on another machine)?
4. **Editor playback**: is a 480p proxy acceptable, or is native playback (libmpv) a requirement?
5. **Multiple target languages per episode** (e.g. ar + en in one pass) or one language per policy run?
