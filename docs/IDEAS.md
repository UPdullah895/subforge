# IDEAS — what the ecosystem already solved, and what SubForge should take from it

Research notes, 2026-11-06. Every claim below links to a page that was opened during this
research; numbers are quoted from those pages, not from memory. Licences are checked because a
licence decides the architecture more often than a benchmark does.

How to read an entry: **what** · **why it serves the goal** (a media server that arrives with
its subtitles and dubs already done) · **where it lands in SubForge** · **effort** (S ≤ 1 day,
M ≤ 1 week, L more).

---

## The six bets worth making first

| # | Bet | Why it is the highest leverage | Effort |
|---|-----|-------------------------------|--------|
| 1 | Learn to speak *arr and Jellyfin, instead of only watching folders | Removes polling, gives series/episode identity, and is the only way to respect "monitored only" | M |
| 2 | A timing/readability linter with real rule-sets (Netflix, BBC) | Bazarr finds subtitles; almost nothing *fixes* them. This is the visible quality jump | M |
| 3 | Word-level timing from WhisperX for ja + ar | Those two wav2vec2 alignment models already exist, and they are your exact pair | M |
| 4 | Deterministic dub alignment: fingerprint → GCC-PHAT → DTW | Matches the locked decision (deterministic first, model only as arbiter) and is cheap to compute | M |
| 5 | MCP tools at the level an agent actually thinks ("process this episode") | Seven file-level tools make an agent write a script; four task-level tools make it a colleague | S–M |
| 6 | A linuxserver-shaped image + Unraid/TrueNAS templates | Where the audience lives; without it the Docker story is a `git clone` | S |

---

## A. Integration surface — stop guessing what the library wants

**A1. Consume Sonarr/Radarr "On Import" instead of watching the folder.**
Sonarr runs a custom script on import with the episode identity in environment variables:
`sonarr_eventtype`, `sonarr_series_tvdbid`, `sonarr_episodefile_path`, `sonarr_episodefile_seasonnumber`,
and so on — [wiki.servarr.com/sonarr/custom-scripts](https://wiki.servarr.com/sonarr/custom-scripts).
The Connect settings also offer a webhook target
([wiki.servarr.com/sonarr/settings](https://wiki.servarr.com/sonarr/settings)).
*Where*: `POST /api/hooks/sonarr` (+ radarr) that maps the payload to a file + policy and submits
one pipeline job. Keep the folder watcher for people who are not on *arr.
*Why it matters*: the watcher cannot know whether an episode is monitored, cannot tell a sample
file from the episode, and races with the download client. *Effort*: M.

**A2. Speak Jellyfin too, but not as a C# plugin.**
Jellyfin plugins are assemblies dropped into a `plugins` folder and loaded in-process
([jellyfin.org/docs/general/server/plugins](https://jellyfin.org/docs/general/server/plugins/)).
A Rust/Tauri tool should not aim there; the integration that costs one route is the Jellyfin
Webhooks plugin's `ItemAdded` payload, which carries the item path and library name.
*Where*: the same `/api/hooks/jellyfin` handler, tolerant of extra fields. *Effort*: S (after A1).

**A3. Provider access is a licensing problem before it is a code problem.**
OpenSubtitles' REST API requires one API key per *application* (a "consumer"), explicitly
forbids shipping a product that asks each user for their own key, allows 5 downloads per IP per
24 h unauthenticated, and 100/day while the consumer is marked "Under Development"; the download
counter resets at midnight UTC ([opensubtitles.stoplight.io](https://opensubtitles.stoplight.io/docs/opensubtitles-api/e3750fd63a100-getting-started)).
*Where*: our own consumer key, optional OpenSubtitles login for real volume, and a visible
"downloads left today" counter so the 5-per-IP limit never looks like a bug.
*Also*: subliminal (MIT, 2.7k★) is the reference for how providers are ranked and scored —
[github.com/Diaoul/subliminal](https://github.com/Diaoul/subliminal). *Effort*: M.

**A4. Alert on the *arr side when a job finishes, not only in our UI.**
A `Connect → Webhook` in the other direction closes the loop (Sonarr shows "Arabic subtitle
added"). Cheap, and it is the thing that makes a pipeline feel installed rather than bolted on.
*Effort*: S.

---

## B. Subtitle quality — the differentiator

**B1. A linter with named rule-sets, run between "subtitle exists" and "mux it".**
The numbers, from Netflix's own style guides: **42 characters per line, maximum two lines,
20 characters/second for adult content, 17 for children**, minimum duration 5/6 s (20 frames at
24 fps), maximum 7 s, gaps of 2 frames with 3–11 frame gaps closed up, and in-time within 1–2
frames of the first audio frame
([English (USA) guide](https://partnerhelp.netflixstudios.com/hc/en-us/articles/217350977-English-USA-Timed-Text-Style-Guide),
[timing guidelines](https://partnerhelp.netflixstudios.com/hc/en-us/articles/360051554394-Timed-Text-Style-Guide-Subtitle-Timing-Guidelines)).
BBC guidance is a different profile, not a variant: **160–180 words per minute, gaps ≥ 1 s
(preferably 1.5 s), 37 characters on teletext**, EBU-TT-D for online
([bbc.co.uk subtitle guide](https://www.bbc.co.uk/accessibility/forproducts/guides/subtitles/)).
*Where*: `subforge-subtitle` gets a `rules` module; the pipeline gains `Step::Lint { language,
profile }` before `Mux`; auto-fix (split long cues on sentence boundaries, close short gaps,
extend too-short cues) is opt-in per policy. *Why it matters*: nobody in the Bazarr space does
this, and it is measurable. *Effort*: M.

**B2. Translate *to a budget*, not just to a language.**
When a translated line exceeds the profile's CPS budget, re-ask the model for a shorter line
with the character limit in the prompt, and only then split the cue. This turns the linter from a
report into a fixer. *Where*: `engines::translator` second pass. *Effort*: S–M.

**B3. Structure-preserving everything.** Any edit — translate, retime, lint — must keep cue
numbers, styling and ASS positioning intact. Our `lines::rebuild_lines` already rebuilds from the
original file; keep that invariant and test it (dream: a "no lost cues" assertion on every
translated file). *Effort*: S.

---

## C. Speech and dubbing

**C1. Two ASR engines by design, not one.**
whisper.cpp (MIT, 55k★) is the portable path: Metal/CoreML, CUDA, **Vulkan, ROCm, OpenVINO,
SYCL** — [github.com/ggml-org/whisper.cpp](https://github.com/ggml-org/whisper.cpp). faster-whisper
(MIT, 26.5k★) is the throughput path: batch 8 fp16 on an RTX 3070 Ti 8 GB transcribed a 13-minute
clip in 17 s (≈46× realtime) but needs CUDA 12 + cuDNN 9 and has no ROCm/Metal —
[github.com/SYSTRAN/faster-whisper](https://github.com/SYSTRAN/faster-whisper).
*Where*: keep whisper.cpp in the desktop bundle; add a `faster-whisper` sidecar *service* the
server can use when it is present, never a hard dependency. *Effort*: M.

**C2. WhisperX-style word timestamps, and its ja/ar alignment models.**
WhisperX (BSD-2, 25k★) reports "70× realtime with large-v2", under 8 GB VRAM at beam 5, VAD
pre-cut to suppress hallucination, and its alignment table ships
`jonatasgrosman/wav2vec2-large-xlsr-53-japanese` and `…-arabic` —
[github.com/m-bain/whisperX](https://github.com/m-bain/whisperX). That is precisely the
Japanese→Arabic case. *Where*: a `word-timings` capability that the retiming step consumes.
*Effort*: L (needs a Python/ONNX bridge; the pragmatic path is ONNX wav2vec2 under `ort`).

**C3. Deterministic track alignment, in three cheap stages.**
Fingerprint for the coarse offset (chromaprint/`fpcalc`, or BBC's Apache-2.0
[audio-offset-finder](https://github.com/bbc/audio-offset-finder)), **GCC-PHAT** to refine it to
sample accuracy, **DTW on CENS/MFCC** for the case where the dub was time-compressed
([librosa DTW](https://librosa.org/doc/latest/api/generated/librosa.sequence.dtw.html)).
A local probe on this machine recovered an exact −1.234 s offset (normalised peak 1.000) from
synthetic signals, and `ffmpeg`'s `axcorrelate` + `adelay` confirmed the arithmetic.
*Why*: this is the "deterministic first" decision made real; the AI arbiter then only has to
adjudicate lines where the two signals disagree. *Effort*: M for offset, L for per-line retime.

**C4. Mix a dub like a broadcaster, not with `volume`.**
A verified-working ffmpeg chain: the dub as sidechain into `sidechaincompress`
(threshold 0.05, ratio 8, attack 20 ms, release 250 ms), then `amix=inputs=2:normalize=0` —
`normalize` defaults to 1 and silently drops the mix ~6 dB — then two-pass `loudnorm` to
**−23 LUFS / ≤ −1 dBTP** (EBU R 128, [tech.ebu.ch R128](https://tech.ebu.ch/files/live/sites/tech/files/shared/r/r128.pdf)).
*Where*: `Step::MixAudio`, after a dub exists. *Effort*: M.

**C5. TTS: pick the licence first, the voice second.**
Kokoro-82M is **Apache-2.0** (54 voices, 8 languages, **no Arabic**,
[hf.co/hexgrad/Kokoro-82M](https://huggingface.co/hexgrad/Kokoro-82M)); XTTS-v2 covers Arabic and
Japanese with 6-second cloning but its CPML permits **non-commercial use of the model *and its
outputs*** ([LICENSE.txt](https://huggingface.co/coqui/XTTS-v2/raw/main/LICENSE.txt)); F5-TTS
weights are `cc-by-nc-4.0`; Piper ships voices under mixed restrictive terms
([piper1-gpl](https://github.com/OHF-Voice/piper1-gpl)); OpenAI's `/v1/audio/speech` is
**deprecated with shutdown 2027-01-06** in favour of the Realtime API
([developers.openai.com TTS guide](https://developers.openai.com/api/docs/guides/text-to-speech)).
*Where*: TTS as an **optional add-on** with the licence printed in the UI, never bundled; the
default path stays "align the dub you already have", which has no licence problem at all.
*Effort*: M per engine, and a legal review before any of it ships.

---

## D. OCR, splitting, moderation

**D1. Copy the architecture of the best burned-in-subtitle extractor, not its language.**
video-subtitle-extractor (Apache-2.0, 9.7k★) does region detection → OCR → frame-grouping, runs
fully locally (PaddleOCR), and its own table warns that the "precise" mode is *frame-by-frame and
very slow* while "auto" picks a light model on CPU and an accurate one on GPU —
[github.com/YaoFANGUK/video-subtitle-extractor](https://github.com/YaoFANGUK/video-subtitle-extractor).
*Where*: `Step::OcrSubtitle` with a subtitle-band detector + confidence threshold + identical-text
dedupe; the heavy OCR stays out of the desktop bundle, exposed as an MCP tool. *Effort*: L.

**D2. Scene detection is a frame-difference, not a model.**
PySceneDetect's `ContentDetector(threshold=27.0, min_scene_len=15, weights=hue/sat/lum=1.0)`
compares HSV+luma deltas, with `AdaptiveDetector` as the two-pass variant for fast camera moves
([scenedetect.com detectors](https://www.scenedetect.com/docs/api/detectors.html)). This is
implementable in Rust on decoded frames we already produce for OCR — no Python dependency.
*Where*: `Step::DetectScenes` feeding chapters (`mkvpropedit`) and the "split long video" feature.
*Effort*: M.

**D3. Moderation: a report with timestamps, never an automatic cut.**
This is the one place where a false positive is worse than no feature, so the shape matters more
than the model: scan → timestamped candidate list with confidence → human confirmation → optional
cut. NudeNet is 2.5k★ but **AGPL-3.0** ([github.com/notAI-tech/NudeNet](https://github.com/notAI-tech/NudeNet)),
so it cannot be bundled into a closed binary; the workable route is an optional user-installed
detector, or a permissively licensed ONNX model (licence to be confirmed before adoption).
*Effort*: M.

---

## E. Distribution — where the audience actually is

**E1. A linuxserver-shaped image, because that is the contract users expect.**
`PUID`/`PGID` mapping (still recommended over `--user`), a `/config` volume, a `/media` volume,
`UMASK`, and an image that does not own the user's files —
[docs.linuxserver.io PUID/PGID](https://docs.linuxserver.io/general/understanding-puid-and-pgid/).
*Where*: our `Dockerfile` already runs unprivileged and takes `SUBFORGE_CONFIG_DIR`; adding
PUID/PGID/UMASK remapping in the entrypoint is small, and it is the difference between "works on
my machine" and "works on Unraid". *Effort*: S.

**E2. Multi-arch from one build.** `docker buildx build --platform linux/amd64,linux/arm64`
produces a manifest list ([docs.docker.com multi-platform](https://docs.docker.com/build/building/multi-platform/)).
arm64 is what NAS boxes and Apple Silicon are; without it the image is x86-only.
Caveat: whisper.cpp's Vulkan/ROCm paths are x86-centric, so ARM means CPU-only transcription.
*Effort*: M (cross-compiling Rust + whisper.cpp for arm64 is the real work).

**E3. Templates: Unraid Community Applications first.**
A CA template is one XML with `<Name>`, `<Repository>`, `<Category>`, `<Icon>`, `<Overview>`,
`<Project>` ([ca.unraid.net/xml-field-reference](https://ca.unraid.net/submit/help/xml-field-reference)).
TrueNAS SCALE and Synology take the same compose file with different wrappers. One file, three
catalogues. *Effort*: S each.

**E4. Desktop distribution follow-ups.** Flatpak is a poor fit for a tool that must read and write
arbitrary media paths without a portal dance; AppImage + deb/rpm + AUR (already done) covers
Linux, and `tauri-plugin-updater` covers updates. Say no to Flatpak in the docs with the reason,
so the question stops being asked. *Effort*: S (documentation).

---

## F. Agent ergonomics (MCP)

**F1. Say the current protocol version.** The transport page itself points at the **2026-07-28**
revision ([modelcontextprotocol.io/specification/latest/basic/transports](https://modelcontextprotocol.io/specification/latest/basic/transports));
we answer `2025-03-26`. Clients negotiate down, so this is not broken — but claiming an older
version costs features and looks stale. *Effort*: S.

**F2. Origin validation (done).** The spec says a Streamable HTTP server **MUST** validate `Origin`
to prevent DNS rebinding; implemented in `crates/subforge-mcp/src/http.rs` with a loopback
allowlist and `SUBFORGE_MCP_ALLOWED_ORIGINS`.

**F3. Tools at the altitude of intent.** Compare with a Jellyfin MCP server whose tools are
`refresh_library`, `analyze_tv_structure`, `get_stats` — tasks, not syscalls
([github.com/FrankBurmo/jellyfin-mcp](https://github.com/FrankBurmo/jellyfin-mcp)). Our seven tools
are `list_tracks`/`merge_subtitles`-level. Add: `process_episode(file, languages)`,
`library_audit(dir, languages)` → coverage report, `plan_policy(file)` → the dry-run plan,
`apply_policy(policy_json)`. *Why*: an agent can then do the job in one call and explain it.
*Effort*: S (they wrap what already exists).

**F4. Long jobs need progress and cancellation.** Every pipeline step is minutes long; MCP has
progress notifications and cancellation in the spec. Without them an agent either times out or
polls. *Effort*: M.

---

## G. Do-not-adopt list (licence or lifecycle traps)

| Thing | Why not |
|---|---|
| aeneas (forced alignment) | AGPL-3.0, no commits since 2020 — call it only as an optional external tool |
| NudeNet | AGPL-3.0 — cannot be bundled |
| pyvideotrans | GPL-3.0 — read it for ideas, do not copy code |
| XTTS-v2 / F5-TTS | CPML non-commercial **including outputs** / `cc-by-nc-4.0` |
| Piper voices | "personal use and TTS research" wording; per-voice terms vary |
| OpenAI `/v1/audio/speech` | deprecated, shutdown 2027-01-06 — target the Realtime API |
| User-supplied OpenSubtitles keys | the API forbids it for applications; one consumer key per app |

---

## H. Competitor map — what they solve, and the gap they leave

**H1. VideoLingo (Apache-2.0, 19k★) is the closest thing to the same product.**
Its README describes "Netflix-level subtitle cutting, translation, alignment and dubbing", a
Streamlit pipeline that runs one operation at a time, local Qwen3-ASR + ForcedAligner, a
`custom_terms.xlsx` glossary, JSON-structured LLM output with response caching and retry, and TTS
through OpenAI/Fish/GPT-SoVITS/Edge/F5
([github.com/Huanshere/VideoLingo](https://github.com/Huanshere/VideoLingo)).
Two honest limits are stated in its own docs: *"the dubbing workflow does not automatically assign
a separate voice to each speaker"* and *"speed adjustment does not guarantee natural delivery or
perfect synchronization."* **That is the gap.** Apache-2.0 also means its glossary and
JSON-schema-with-cache design can be borrowed outright.
*Take*: the termbase file, the cached structured output, "cut at sentence boundaries" as the
alignment strategy. *Beat it with*: per-speaker voices, a real rule-set linter, and a library
workflow instead of one video at a time.

**H2. pyVideoTrans (GPL-3.0, 19.6k★) sets the feature bar.**
ASR → subtitle translation → TTS → video synthesis in one click, **speaker diarization**,
**multi-role AI dubbing** (a different voice per speaker), zero-shot voice cloning (F5-TTS,
CosyVoice, GPT-SoVITS), stage-by-stage manual proofreading, and a toolkit with vocal separation
and audio/video alignment; GPU needs CUDA 12.8 + cuDNN 9.11
([github.com/jianchang512/pyvideotrans](https://github.com/jianchang512/pyvideotrans)).
*Read it for scope, do not copy code* (GPL-3.0). Its existence is why "dubbing with per-speaker
voices" belongs on our roadmap rather than in a wish list.

**H3. Subtitles that are merely out of sync are the cheapest quality win available.**
ffsubsync (MIT, 7.9k★, maintained) aligns a subtitle to a video by comparing VAD audio activity
([github.com/smacke/ffsubsync](https://github.com/smacke/ffsubsync)); alass does language-agnostic
alignment but is **GPL-3.0** and idle since 2023
([github.com/kaegi/alass](https://github.com/kaegi/alass)). *Where*: `Step::SyncExisting` before
translation — a downloaded subtitle that is 1.5 s off produces a translated subtitle that is
1.5 s off, and users blame the translation. Our `subtitle/sync.rs` already has the offset
detection; wiring it into the pipeline is small. *Effort*: S.

**H4. Be the backend other tools call.**
Lingarr (906★) is a separate self-hosted translation service that Bazarr talks to; Bazarr gained a
Lingarr integration on its own
([github.com/lingarr-translate/lingarr](https://github.com/lingarr-translate/lingarr)).
The lesson is distribution, not features: a clean HTTP + MCP surface lets SubForge be *the*
translation/dubbing engine for tools people already run, without winning a UI war first.
*Where*: our server API + MCP already are that surface; document them as one (OpenAPI + a
one-page "call it from n8n/Sonarr/Bazarr" recipe). *Effort*: S.

**H5. Bazarr's own issue list is a list of design rules for us.**
Open/recent issues cluster around: Whisper subtitles never passing a movie's minimum score, an
OpenAI-compatible translator failing on strict endpoints (Groq) because of hardcoded parameters,
picking the wrong audio stream when several exist, provider outages breaking the whole run, and
translation arriving through a bolt-on service
([issues](https://github.com/morpheus65535/bazarr/issues)).
*Rules for us*: (a) never gate a transcript behind a provider score; (b) expose every engine
parameter instead of hardcoding one vendor's defaults; (c) let the policy name the audio track or
pick by language + channel count, never "the first one"; (d) one provider failing must degrade to
the next, not fail the episode; (e) translation is a first-class step, not an integration.

---

## I. Two features the competitors prove are needed

**I1. Speaker diarization, because "a voice per speaker" is already table stakes.**
pyVideoTrans ships multi-role dubbing; VideoLingo explicitly does not. The gap between those two
sentences is the product. *Where*: a `Step::Diarize` producing speaker-labelled segments that the
translation prompt and the TTS voice selection both consume. *Caveat*: the best-known diarization
model (pyannote) is gated behind a Hugging Face token and its own terms — check before bundling.
*Effort*: L.

**I2. A termbase, because names are what make a translation look machine-made.**
VideoLingo's `custom_terms.xlsx` exists for exactly this. *Where*: `policy.json` gains
`terms: [{source, target, note}]`, injected into every batch prompt; the linter gains an
"entity preserved?" check. *Effort*: S — and it is the single cheapest visible quality upgrade
for anime and drama, where names repeat every episode.

---

## Roadmap mapping

| Idea | Phase | Notes |
|---|---|---|
| A1 */arr hooks* | P2 | unblocks "monitored only" and kills polling |
| B1 linter + B2 CPS-aware translation | P2/P3 | ships with the pipeline, not after it |
| A3 provider keys + counters | P2 | legal-first, before any provider work |
| F3/F4 MCP tools + progress | P1/P3 | wraps existing code |
| C1 dual ASR, C2 word timings, C3 alignment | P4 | the توزين milestone |
| E1/E2/E3 packaging | P2 | parallel, do it with the Docker work |
| D1 OCR, D2 scenes | P4 | after the pipeline is trusted |
| C4 mix, C5 TTS | P5 | licence review first |
| D3 moderation | P5 | report-only until proven |

---

## Research gaps (honest)

- The Docker/Unraid/TrueNAS pass ran on first-party pages only; Synology and TrueNAS template
  specifics were **not** verified in this round.
- No test on arm64 hardware — the ARM claims here are from documentation, not measurement.
- Whisper speed numbers come from the projects' own READMEs (plus a peer-reviewed benchmark for
  `small` on CPU); none were re-measured on this machine except the GCC-PHAT/ducking probes.
- The moderation model licence question is **open**; do not ship a detector until it is answered.

---
