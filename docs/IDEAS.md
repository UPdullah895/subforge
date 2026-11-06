# IDEAS — what the ecosystem already solved, and what SubForge should take from it

Research notes, 2026-11-06. Every claim links to a page that was opened during this research;
numbers are quoted from those pages, not from memory. Licences are checked because a licence
decides the architecture more often than a benchmark does.

How to read an entry: **what** · **why it serves the goal** (a media server that arrives with its
subtitles and dubs already done) · **where it lands in SubForge** · **effort** (S ≤ 1 day,
M ≤ 1 week, L more).

---

## The seven bets worth making first

| # | Bet | Why it is the highest leverage | Effort |
|---|-----|-------------------------------|--------|
| 1 | Subscribe to the *arr **SignalR feed**, with webhooks and folder watching as fallbacks | The *arrs broadcast every change with IDs; Bazarr already does this, so users expect zero-config | M |
| 2 | A timing/readability linter with real rule-sets (Netflix, BBC) — `seconv --FixCommonErrors` on day one | Bazarr *finds* subtitles; almost nothing *fixes* them. This is the visible quality jump | M |
| 3 | Local movie-hash matching for providers | one hash match beats every name-matching heuristic, and we own the file so we can compute it | S |
| 4 | Word-level timing from WhisperX for ja + ar | those two wav2vec2 alignment models already exist, and they are your exact pair | M |
| 5 | Deterministic dub alignment: fingerprint → GCC-PHAT → DTW, with a quality gate | matches the locked decision, and the gate is what Bazarr's own bug report says is missing | M |
| 6 | MCP tools at the level an agent thinks ("process this episode") | seven file-level tools make an agent write a script; four task-level tools make it a colleague | S–M |
| 7 | One exclusive GPU lease shared by transcribe/OCR/vision | the roadmap's "never concurrent" rule, made mechanical instead of hoped for | S |

---

## A. Integration surface — stop guessing what the library wants

**A1. Subscribe to the *arrs' SignalR feed first, not last.**
Bazarr does not poll: it opens one outbound WebSocket to
`<arr>/signalr/messages?access_token=<api-key>` (legacy `/signalr?apikey=`), receives
`series`/`episode`/`movie` change events, never calls hub methods, and falls back to scheduled REST
sync when the feed is blocked. With the same key it then calls `RescanSeries`/`RescanMovie` so the
*arr UI shows the change
([wiki.bazarr.media — how Bazarr connects to Sonarr/Radarr](https://wiki.bazarr.media/Getting-Started/How-Bazarr-Connects-to-Sonarr-and-Radarr/)).
*Where*: `crates/subforge-pipeline` gains an `arr` module; the server opens one task per configured
arr, maps events to files, and keeps the folder watcher for people who are not on *arr at all.
*Why it matters*: zero configuration versus "go create a webhook", and it carries the changed
resource instead of a path we have to interpret. *Effort*: M.

**A2. Webhook as the second path, custom script as the third.**
Sonarr's `Download` trigger fires on grab / import / upgrade / rename with a JSON body that
already carries everything: `series.{tvdbId,tmdbId,imdbId,originalLanguage,type}`,
`episodes[].{seasonNumber,episodeNumber,tvdbId}`,
`episodeFiles[].{path,relativePath,releaseGroup,sceneName,quality,mediaInfo.subtitles,mediaInfo.audioLanguages}`,
`isUpgrade`, `release.releaseType` (`singleEpisode`/`seasonPack`). Note `episodeFiles[]` is an
**array even for one file** — older builds sent a singular `episodeFile`, which is a real
compatibility trap. Radarr is the same shape with a singular `movieFile`
([servarr wiki — Sonarr settings](https://wiki.servarr.com/sonarr/settings),
[payload reference](https://github.com/sirjmann92/remuXcode/blob/main/docs/sonarr-webhook-reference.md)).
*Where*: `POST /api/hooks/sonarr` + `/radarr`, accepting the full payload and stateless on first
contact. **Do not copy Bazarr's design**: its endpoint takes only `episodeFiles[].id`, looks the id
up in its own database, and silently skips the event when it has not synced that file yet — which
is why "the webhook fired and nothing happened" is a recurring complaint
([Bazarr library settings](https://wiki.bazarr.media/Additional-Configuration/Settings/Library/)).
*Effort*: S after A1.

**A3. Use `mediaInfo` to refuse work before doing it.**
The payload already tells you which subtitle and audio languages are embedded. If the target
language is there, the job is a no-op — no mkvmerge probe, no transcription, no GPU seconds.
For custom-script users, the same data arrives as environment variables
(`sonarr_eventtype`, `sonarr_episodefile_path`, `sonarr_episodefile_scenename`,
`sonarr_series_tvdbid`…), and they are exported into `$_SERVER` rather than `$_ENV`, a detail that
breaks naive scripts ([custom scripts](https://wiki.servarr.com/sonarr/custom-scripts)).
*Where*: a `subforge handle <path>` CLI so a custom script is one line, and the same code is what
the MCP `process_episode` tool calls. *Effort*: S.

**A4. Our policy file is below Bazarr's bar — close the gap now, not later.**
Bazarr's model is a **language profile** (ordered languages + `cutoff`: stop once this language is
present at ≥ score), attached per series/movie, plus "upgrade previously downloaded subtitles N
days back", exclude by tag/series-type, monitored-only, and **path mappings** for when the Docker
mount differs from the *arr's view
([library settings](https://wiki.bazarr.media/Additional-Configuration/Settings/Library/),
[subtitles settings](https://wiki.bazarr.media/Additional-Configuration/Settings/Subtitles/)).
Path mapping is the single most common setup failure in their documentation, so it is not a
nice-to-have.
*Where*: `Policy` gains `profiles: [{languages[], cutoff}]`, `upgrade_window_days`, `path_mappings`,
`min_match_score`; `prefer_human_subtitle` becomes a cutoff rule rather than a boolean. *Effort*: M.

**A5. Match by hash, not by name.**
Bazarr's scorer weights attributes (`hash`, `series`, `year`, `season`, `episode`, `source`,
`release_group`, `resolution`, `audio_codec`, `hearing_impaired`,
[score.py](https://github.com/morpheus65535/bazarr/blob/master/custom_libs/subliminal_patch/score.py))
and enforces `hash == sum(other weights) - 1`, so a hash match outranks everything — but the hash is
only trusted when the corroborating fields agree (`{series,season,episode,source}` for episodes).
*Where*: we already hold the file, so compute the OpenSubtitles `moviehash` locally (first and last
64 KiB + size) and send `moviehash` + `moviehash_match=true`, exactly the parameters their
`GET /subtitles` accepts. This is a small feature that beats every name-based tool.
*Effort*: S.

**A6. Providers: know the ceiling and the terms before writing an adapter.**

| Provider | What we get | Limits that shape the queue | Terms |
|---|---|---|---|
| OpenSubtitles | `imdb_id`/`tmdb_id`/`moviehash`/`season_number` search; `POST /login` → 24 h JWT; `POST /download` → **temporary link — store the file, not the link** | **5 req/s** per IP (50/s paid), `/login` 1/s, 429 + `ratelimit-*` headers · users signing in with their **own** account → a free key is enough, quota is theirs · anonymous downloads through your key → **paid plan, Light or above** · one key for anonymous *and* signed-in users → **Enterprise** · "Fetch subtitles nobody asked for, to fill a cache, a dataset or a mirror" is explicitly **No**, and mirroring is forbidden · **no user-supplied keys** (apps that require them are banned) · free-key attribution string: `Subtitles provided by <a href="https://opensubtitles.com">OpenSubtitles.com</a>`, shown where subtitles are picked ([getting started](https://opensubtitles.stoplight.io/docs/opensubtitles-api/e3750fd63a100-getting-started), [integrator terms](https://opensubtitles.stoplight.io/docs/opensubtitles-api/f43c0749230b3-terms-of-use-for-integrators)) |
| SubDL | `GET /api/v2/files/search?filename=<release.mkv>` returns the matched ids **and** the subtitles for that exact release, each with a `match_score` 0–1 (≥0.8 = confident), plus season-pack unpacking | free tier 2,000 searches and **50 downloads/day** ([api.subdl.com](https://subdl.com/developers)) | friendly to media tools (their own `client` parameter names `bazarr`, `kodi`, `stremio`) |
| SubSource | documented REST v1 with `X-API-Key`, ZIP downloads | 60 req/min, 1,800/hr, 7,200/day, 429 + headers ([api-docs](https://subsource.net/api-docs)) | usable as a real provider |
| Podnapisi | undocumented JSON via `Accept: application/json` on the advanced search | unknown | **opt-in and marked unofficial** — this is the pattern that gets scrapers broken |

Two failure modes account for most "OpenSubtitles returns nothing" reports and both are our bug to
avoid: a wrong `User-Agent` returns **403 `User-Agent header is wrong`**, and the client must follow
HTTP redirects or search silently returns nothing
([best practices](https://opensubtitles.stoplight.io/docs/opensubtitles-api/6ef2e232095c7-best-practices)).
Also: the ToS forbidding server-side caching of *their* subtitles does **not** forbid caching *our*
translation output — but it does rule out a shared "fetch any subtitle for anyone" endpoint.
*Effort*: M.

**A7. The provider trait should be two methods, with quota outside it.**
subliminal (MIT) is the de-facto abstraction: `list_subtitles(video, languages)` →
`download_subtitle(subtitle)`, registered as plugins, with rate limiting deliberately left to the
caller ([subliminal providers](https://subliminal.readthedocs.io/en/latest/api/providers.html)).
*Where*: our `SubtitleProvider` mirrors those two calls, and quota/backoff lives in the scheduler
**keyed by provider** — the place Bazarr had to retrofit "adaptive searching" into.
*Effort*: S.

**A8. Alert back when a job finishes.** A Sonarr/Radarr `Connect` webhook in the other direction
closes the loop ("Arabic subtitle added"), so the pipeline feels installed rather than bolted on.
*Effort*: S.

---

## B. Subtitle quality — the differentiator

**B1. Ship a linter, and start with `seconv`.**
The numbers, from Netflix's own guides: **42 characters per line, maximum two lines, 20
characters/second for adult content, 17 for children**; minimum duration 5/6 s (20 frames at
24 fps), maximum 7 s; gaps of 2 frames, with 3–11 frame gaps closed up; in-time within 1–2 frames of
the first audio frame ([English (USA) guide](https://partnerhelp.netflixstudios.com/hc/en-us/articles/217350977-English-USA-Timed-Text-Style-Guide),
[timing guidelines](https://partnerhelp.netflixstudios.com/hc/en-us/articles/360051554394-Timed-Text-Style-Guide-Subtitle-Timing-Guidelines)).
BBC is a different profile, not a variant: **160–180 wpm, gaps ≥ 1 s (preferably 1.5 s), 37
characters on teletext**, EBU-TT-D for online
([bbc.co.uk guide](https://www.bbc.co.uk/accessibility/forproducts/guides/subtitles/)).
*Shortcut*: Subtitle Edit's **`seconv x.srt subrip --FixCommonErrors`** does Netflix-grade cleanup
today, and its core library `libse` is BSD — running `seconv` as a separate process keeps our
licensing clean and gets us most of this feature on day one
([seconv docs](https://subtitleedit.github.io/subtitleedit/features/seconv.html)).
*Where*: `Step::Lint { language, profile }` before `Mux`; our own Rust rules follow where `seconv`
is not available (container, Windows without the binary). *Effort*: S to ship via `seconv`, M to own.

**B2. Translate *to a budget*.**
When a translated line exceeds the profile's CPS budget, re-ask the model with the character limit
in the prompt; only then split the cue. Turns the linter from a report into a fixer.
*Where*: `engines::translator` second pass. *Effort*: S–M.

**B3. Repair sync before translating, and never stretch without a gate.**
ffsubsync (MIT, 7.9k★) aligns a subtitle to a video by comparing VAD activity
([github.com/smacke/ffsubsync](https://github.com/smacke/ffsubsync)); alass is language-agnostic and
has an `alass-core` crate, but is **GPL-3.0** and idle since 2023
([github.com/kaegi/alass](https://github.com/kaegi/alass)) — subprocess only, or reimplement.
The cautionary tale: Bazarr passed ffsubsync `--gss` (0.90–1.10× stretch search) without a quality
gate, and a reporter measured ~9 % of sync events picking a spurious stretch, turning subtitles
that were 0–2 s off into 30–60 s off by the end of the episode
([bazarr#3599](https://github.com/morpheus65535/bazarr/issues/3599)).
*Where*: `Step::SyncExisting` — **offset-only by default**, stretch behind an explicit flag and a
quality gate, original kept, before/after offset logged. We can do what no name-matching tool can:
verify the aligned subtitle against our own whisper transcript. *Effort*: S–M.

**B4. Structure-preserving everything.** Any edit keeps cue numbers, styling and ASS positioning:
`lines::rebuild_lines` already rebuilds from the original; assert "no lost cues" on every output.
*Effort*: S.

---

## C. Speech and dubbing

**C1. Two ASR engines by design, not one.**
whisper.cpp (MIT, 55k★) is the portable path: Metal/CoreML, CUDA, **Vulkan, ROCm, OpenVINO, SYCL**
([whisper.cpp](https://github.com/ggml-org/whisper.cpp)). faster-whisper (MIT, 26.5k★) is the
throughput path: batch 8 fp16 on an RTX 3070 Ti 8 GB transcribed a 13-minute clip in 17 s (≈46×
realtime), but needs CUDA 12 + cuDNN 9 and has no ROCm/Metal
([faster-whisper](https://github.com/SYSTRAN/faster-whisper)). NeMo Parakeet/Canary are excellent
and cover **25 European languages only** — no Arabic, no Japanese
([parakeet-tdt-0.6b-v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3)), so the JP→AR path
cannot be NeMo.
*Where*: whisper.cpp stays in the desktop bundle; a `faster-whisper` sidecar is optional, never a
hard dependency. *Effort*: M.

**C2. WhisperX-style word timestamps, and its ja/ar alignment models.**
WhisperX (BSD-2, 25k★): "70× realtime with large-v2", < 8 GB VRAM at beam 5, VAD pre-cut against
hallucination, and its alignment table ships `jonatasgrosman/wav2vec2-large-xlsr-53-japanese` and
`…-arabic` ([whisperX](https://github.com/m-bain/whisperX)) — exactly the pair this product needs.
torchaudio's MMS_FA-style CTC alignment is the other option and needs no pronunciation dictionary,
because non-Latin scripts are romanised first
([forced alignment tutorial](https://docs.pytorch.org/audio/stable/tutorials/forced_alignment_tutorial.html)).
*Where*: a `word-timings` capability the retiming step consumes; pragmatic path is wav2vec2 ONNX
under `ort` rather than a Python sidecar. *Effort*: L.

**C3. Deterministic track alignment, in three cheap stages.**
Fingerprint for the coarse offset (chromaprint/`fpcalc`, or BBC's Apache-2.0
[audio-offset-finder](https://github.com/bbc/audio-offset-finder)), **GCC-PHAT** to refine to sample
accuracy, **DTW on CENS/MFCC** for a dub that was time-compressed
([librosa DTW](https://librosa.org/doc/latest/api/generated/librosa.sequence.dtw.html)).
A local probe here recovered an exact −1.234 s offset (normalised peak 1.000) and `ffmpeg`'s
`axcorrelate` + `adelay` confirmed the arithmetic.
*Why*: this is "deterministic first" made real; the model only adjudicates lines where the two
signals disagree. *Effort*: M for offset, L for per-line retime.

**C4. Mix a dub like a broadcaster.**
Verified chain: dub as sidechain into `sidechaincompress` (threshold 0.05, ratio 8, attack 20 ms,
release 250 ms), then `amix=inputs=2:normalize=0` — `normalize` defaults to 1 and silently drops the
mix ~6 dB — then two-pass `loudnorm` to **−23 LUFS / ≤ −1 dBTP** (EBU R 128,
[R128](https://tech.ebu.ch/files/live/sites/tech/files/shared/r/r128.pdf)). `rubberband`
(`tempo`, `transients=crisp`) is the better time-stretcher for fitting TTS into a slot.
*Effort*: M.

**C5. TTS: pick the licence first, the voice second.**
Kokoro-82M is **Apache-2.0** but has **no Arabic** ([Kokoro](https://huggingface.co/hexgrad/Kokoro-82M));
XTTS-v2 covers Arabic and Japanese with 6-second cloning, but its CPML permits **non-commercial use
of the model *and its outputs***
([LICENSE.txt](https://huggingface.co/coqui/XTTS-v2/raw/main/LICENSE.txt)); F5-TTS weights are
`cc-by-nc-4.0`; Piper's own docs say "personal use and TTS research only"
([piper1-gpl](https://github.com/OHF-Voice/piper1-gpl)); OpenAI's `/v1/audio/speech` is deprecated
with shutdown **2027-01-06** in favour of the Realtime API
([TTS guide](https://developers.openai.com/api/docs/guides/text-to-speech)).
*Where*: TTS as an optional add-on with the licence printed in the UI, never bundled; the default
path stays "align the dub you already have", which has no licence problem at all. *Effort*: M per
engine, plus a licence review before shipping.

---

## D. OCR, splitting, moderation

**D1. Probe first: OCR only when there is no text track.** `ffprobe -select_streams s` is free;
PaddleOCR on a full episode is not. *Effort*: S — and it removes most of this work from the queue.

**D2. Copy VideoSubFinder's split: find the frames, then read them.**
Frame differencing writes timestamped images plus an SRT of intervals (placeholder text); the OCR
stage then reads those images
([VideoSubFinder](https://github.com/SWHL/VideoSubFinder),
[RapidVideOCR](https://swhl.github.io/RapidVideOCR/main/en/)). Cropping to a band given as
fractions of the frame (`-le -re -be -te`) is the biggest accuracy/speed lever.
video-subtitle-extractor (Apache-2.0, 9.7k★) is the reference implementation for the rest:
keyframe → text detection → watermark filter → OCR → typo map → dedupe → SRT, with **Fast** (may
miss lines), **Auto** (mini on CPU, large on GPU) and **Precise** (per-frame, "very slow") modes
([VSE](https://github.com/YaoFANGUK/video-subtitle-extractor)).
*Where*: two steps, `Step::DetectSubtitleFrames` (CPU, cheap, resumable) and `Step::OcrFrames`
(GPU optional). Timing survives even if OCR fails. *Effort*: L.

**D3. Engine choice, with numbers.** On 1,477 annotated video frames: RapidOCR CER 0.430 / accuracy
56.98 %, EasyOCR 0.507 / 49.30 %, Claude-3 0.323 / 67.71 %, Gemini-1.5 Pro 0.239 / 76.13 %,
GPT-4o 0.238 / 76.22 % ([arXiv 2502.06445](https://arxiv.org/abs/2502.06445)) — general in-scene
text, so subtitle-band accuracy is much better; the point stands that **PP-OCR is the right default
on outlined hardsubs and Tesseract is not**, and that a VLM asked to transcribe freely will invent
dialogue. Use a VLM only to arbitrate low-confidence lines. *Effort*: S once D2 exists.

**D4. Expect OCR to be slow and plan for it.** A measured CPU-only run (i5-14600K, no dGPU): 0.6–1.2 s
per frame, ~7 minutes per 3–4 minutes of video, ~600 MB per worker
([hardsub-extractor](https://github.com/liuzhongze7/hardsub-extractor)). Vendor claims of 5–10
minutes per hour are GPU best case. *Where*: a queued, resumable job with per-frame progress —
never a synchronous endpoint. *Effort*: S (design), already implied by the scheduler.

**D5. Scene detection is a frame difference, not a model.** PySceneDetect's
`ContentDetector(threshold=27.0, min_scene_len=15)` is an HSV pixel-delta;
`AdaptiveDetector(adaptive_threshold=3.0, window_width=2, min_content_val=15.0)` is the rolling-average
variant that "helps mitigate false detections in situations such as fast camera motions"; the CLI
adds `-fs 2` (sample 33 % of frames), `-d N` (downscale) and `save-edl`
([detectors](https://www.scenedetect.com/docs/api/detectors.html),
[cli](https://www.scenedetect.com/docs/cli.html)). *Where*: `Step::DetectScenes`, feeding chapter
candidates, moderation windows and split points. *Effort*: M.

**D6. The free pre-pass before any of that.** `ffmpeg -vf "select='gt(scene,0.4)',metadata=print"`
for cut scores, `blackdetect=d=2:pix_th=0.10`, `silencedetect=noise=-30dB:d=1` — all CPU, all
seconds ([filters](https://ffmpeg.org/ffmpeg-filters.html#silencedetect)). Use them to propose
chapters and to subsample the vision pass to ~1 frame per scene instead of 1 frame per 5 seconds: the
difference between ~20,000 frames and ~1,500 per movie. *Effort*: S.

**D7. Write chapters without remuxing.** `mkvpropedit movie.mkv --chapters new-chapters.xml`
(header-only; also accepts the simple `CHAPTER01=00:00:00.000` format) and `mkvmerge` reads the same
XML ([mkvpropedit(1)](https://man.archlinux.org/man/mkvpropedit.1.en)). Write chapters to a **copy**
or a sidecar, and export EDL/MCF so users can hand-edit. *Effort*: S.

**D8. Moderation: a report with evidence, never a cut.** NudeNet (2.5k★, **AGPL-3.0**) offers
`320n`/`640m` models and 18 labels including `*_EXPOSED` / `*_COVERED` variants with per-box scores
([NudeNet](https://github.com/notAI-tech/NudeNet)), but the accuracy numbers are sobering: on
UnsafeBench its Sexual F1 is **0.650** real / 0.596 AI-generated / 0.624 perturbed, and it is the
most adversarially vulnerable classifier tested
([arXiv 2405.03486](https://arxiv.org/abs/2405.03486)); NSFW classifiers false-positive on
**21.5 %–47.9 %** of censored artistic nudes and flag male anatomy ~3× less reliably than female
([arXiv 2409.17156](https://arxiv.org/abs/2409.17156)). VLMs are worse as scanners: LLaVA-OneVision
8B violation recall **12.7 %** ([arXiv 2606.05748](https://arxiv.org/html/2606.05748)) — so the VLM
belongs in the *second* stage, describing a window a cheap detector already flagged.
*Where*: report `start/end, category, severity, label, score, model, threshold, frame, review_state`
next to the media (and as `.mcf`), with a human confirm before anything is cut. *Effort*: M.

**D9. Prior art to copy instead of reinvent.** Cleanplex (GPLv3) already does the shape: a
**23:00–06:00 scan window**, 1 frame / 5 s via hardware decode, NudeNet 320n then 640m on candidates
(threshold 0.6), optional SigLIP second stage, flagged frames grouped into segments and re-sampled
every 500 ms, and a **false-positive verifier that learns from rejections**
([Cleanplex](https://github.com/nazmolla/Cleanplex)). intro-skipper publishes segments through
Jellyfin's native MediaSegments using chapters, **Chromaprint**, silence and black frames
([intro-skipper](https://github.com/intro-skipper/intro-skipper)). The `.mcf` open standard exists
and is already imported by other players ([MovieContentFilter](https://github.com/delight-im/MovieContentFilter)).
*Our differentiator is not the detection math — it is the shared queue with transcription.* *Effort*:
M.

**D10. One GPU lease, held by exactly one of {transcribe, OCR, VLM}.**
Precedent: Cleanplex runs scans only in a night window with N workers; intro-skipper runs as a
Jellyfin scheduled task. *Where*: an exclusive lock with a heartbeat (file lock, not a DB flag) plus
priority transcription > OCR > vision, and per-stage progress so a preempted OCR job resumes at the
last completed chapter. *Effort*: S — and it makes the roadmap's "never concurrent" rule mechanical.

**D11. What actually fits in 8 GB.** Qwen3-VL 2B peaks at 4.1 GB, 4B at 5.6 GB, 8B at 7.2 GB
([arXiv 2609.11916](https://arxiv.org/html/2609.11916)); MiniCPM-V 4.5 needs 9 GB even at int4
([MiniCPM-V](https://github.com/OpenBMB/MiniCPM-V)). So: 4B for captioning, 8B only when nothing else
is resident — which is the same conclusion the GPU lease enforces.

---

## E. Where the file lands (and what it is called) decides whether the server sees it

Jellyfin parses suffixes on a sidecar name: `Film.en.srt`, `Film.en.sdh.srt`, `Film.en.forced.ass`,
`Film.default.en.srt`; the flags are `default`, `forced`/`foreign`, `sdh`/`cc`/`hi`, **`hi` alone
resolves as Hindi**, and flags are ignored on containers with more than one embedded stream
([Jellyfin external subtitles](https://jellyfin.org/docs/general/server/media/movies/#external-subtitles-and-audio-tracks)).
*Where*: since we mux and then delete the sidecar, the muxed track must carry the language and the
`forced`/SDH disposition set by mkvmerge — an untagged track shows up as "Unknown" and the user
thinks we failed. Always UTF-8, always `--language`, and set the track name to provider+score so a
human can audit it later. *Effort*: S.

---

## F. Distribution — where the audience actually is

**F1. A linuxserver-shaped image is the contract users expect.** `PUID`/`PGID` mapping (still
recommended over `--user`), `/config` and `/media` volumes, `UMASK`, and an image that never owns the
user's files ([PUID/PGID](https://docs.linuxserver.io/general/understanding-puid-and-pgid/)).
*Where*: our image already runs unprivileged and honours `SUBFORGE_CONFIG_DIR`; the entrypoint
remapping is small. *Effort*: S.

**F2. Multi-arch from one build.** `docker buildx build --platform linux/amd64,linux/arm64`
produces a manifest list ([docs.docker.com](https://docs.docker.com/build/building/multi-platform/)).
ARM is what NAS boxes and Apple Silicon are; caveat: Vulkan/ROCm are x86-centric, so ARM means
CPU-only transcription. *Effort*: M (cross-compiling Rust + whisper.cpp is the real work).

**F3. Unraid Community Applications first.** One XML: `<Name>`, `<Repository>`, `<Category>`,
`<Icon>`, `<Overview>`, `<Project>`
([xml field reference](https://ca.unraid.net/submit/help/xml-field-reference)). TrueNAS SCALE and
Synology wrap the same compose file. *Effort*: S each.

**F4. Say no to Flatpak in writing.** A tool that must read and write arbitrary media paths does not
fit a portal sandbox; AppImage + deb/rpm + AUR already covers Linux, and `tauri-plugin-updater`
covers updates. Document the reason so the question stops being asked. *Effort*: S.

---

## G. Agent ergonomics (MCP)

**G1. Say the current protocol version.** The transports page points at the **2026-07-28** revision
([spec](https://modelcontextprotocol.io/specification/latest/basic/transports)); we answer
`2025-03-26`. Clients negotiate down, so nothing is broken — but it costs features and looks stale.
*Effort*: S.

**G2. Origin validation (done).** The spec requires a Streamable HTTP server to validate `Origin`
against DNS rebinding; implemented in `crates/subforge-mcp/src/http.rs` with a loopback allowlist and
`SUBFORGE_MCP_ALLOWED_ORIGINS`.

**G3. Tools at the altitude of intent.** A Jellyfin MCP server exposes `refresh_library`,
`analyze_tv_structure`, `get_stats` — tasks, not syscalls
([jellyfin-mcp](https://github.com/FrankBurmo/jellyfin-mcp)). Our seven tools are
`list_tracks`/`merge_subtitles`-level. Add `process_episode(file, languages)`,
`library_audit(dir, languages)`, `plan_policy(file)`, `apply_policy(policy)`. *Effort*: S.

**G4. Long jobs need progress and cancellation.** Every step is minutes long; MCP has both in the
spec. Without them an agent times out or polls. *Effort*: M.

---

## H. Competitor map — what they solve, and the gap they leave

**H1. VideoLingo (Apache-2.0, 19k★) is the closest thing to the same product.** "Netflix-level
subtitle cutting, translation, alignment and dubbing", one operation at a time, local Qwen3-ASR +
ForcedAligner, a `custom_terms.xlsx` glossary, JSON-structured LLM output with response caching and
retry, TTS via OpenAI/Fish/GPT-SoVITS/Edge/F5 ([VideoLingo](https://github.com/Huanshere/VideoLingo)).
Two limits are in its own docs: *"the dubbing workflow does not automatically assign a separate
voice to each speaker"* and *"speed adjustment does not guarantee natural delivery or perfect
synchronization."* **That is the gap.** Apache-2.0 also means the glossary and the
JSON-schema-with-cache design can be borrowed outright.

**H2. pyVideoTrans (GPL-3.0, 19.6k★) sets the feature bar.** ASR → translation → TTS → synthesis in
one click, **speaker diarization**, **multi-role dubbing**, zero-shot voice cloning (F5-TTS,
CosyVoice, GPT-SoVITS), stage-by-stage proofreading, vocal separation, A/V alignment; GPU needs CUDA
12.8 + cuDNN 9.11 ([pyVideoTrans](https://github.com/jianchang512/pyvideotrans)). Read for scope,
never copy code.

**H3. Bazarr's issue list is a list of design rules for us.** Its open issues cluster around Whisper
never passing a movie's minimum score, an OpenAI-compatible translator failing on strict endpoints
because of hardcoded parameters, choosing the wrong audio stream when several exist, and providers
breaking the whole run ([issues](https://github.com/morpheus65535/bazarr/issues)).
*Rules*: never gate a real transcript behind a provider score; expose every engine parameter; name
the audio track by language/channels, never "the first one"; one provider failing degrades to the
next; translation is a first-class step.

**H4. Be the backend other tools call.** Lingarr is a separate self-hosted translation service that
Bazarr integrated with ([lingarr](https://github.com/lingarr-translate/lingarr)). A clean HTTP + MCP
surface lets SubForge be *the* translation/dubbing engine for tools people already run, without
winning a UI war. *Where*: our server API + MCP already are that surface — document it as one.
*Effort*: S.

**H5. Two features the competitors prove are needed.** **Speaker diarization** with a voice per
speaker (pyVideoTrans has it; VideoLingo admits it does not — that sentence is the product
opportunity; note pyannote's models are gated by their own terms, check before bundling), and a
**termbase** (`terms: [{source, target}]` in the policy, injected into every batch prompt, with an
"entity preserved?" lint check) — the cheapest visible quality upgrade for anime and drama, where
names repeat every episode. *Effort*: L and S respectively.

---

## I. Do-not-adopt list (licence or lifecycle traps)

| Thing | Why not |
|---|---|
| aeneas | AGPL-3.0, no commits since 2020 — external tool only, never bundled |
| NudeNet | AGPL-3.0 — cannot be bundled into a closed binary |
| pyVideoTrans | GPL-3.0 — read it for scope, do not copy code |
| alass / `alass-core` | GPL-3.0 — subprocess only, or reimplement; ffsubsync is the MIT alternative |
| XTTS-v2 / F5-TTS | CPML non-commercial **including outputs** / `cc-by-nc-4.0` |
| Piper voices | "personal use and TTS research" wording; per-voice terms vary |
| OpenAI `/v1/audio/speech` | deprecated, shutdown 2027-01-06 — target the Realtime API |
| User-supplied OpenSubtitles keys | the API bans applications that require them; one consumer key per app |
| Caching/mirroring OpenSubtitles subtitles | explicitly forbidden by their integrator terms |
| AnimeTosho | shutdown notice (2026-09-16): feeds and API stop; do not build on it |
| Podnapisi as a default provider | undocumented API and no terms — opt-in, marked unofficial |
| Tesseract on outlined hardsubs | measurably worse than PP-OCR/RapidOCR; fine only for clean black-on-white |

---

## J. Translation quality you can actually measure (and ship)

**J1. The licences decide which local MT model we may ship, not the benchmarks.**

| Model | Licence | Usable in a shipped product? |
|---|---|---|
| `google/madlad400-3b-mt` | **Apache-2.0** | yes — 400+ languages incl. Arabic, plain MT |
| `Qwen/Qwen2.5-7B-Instruct` | **Apache-2.0** | yes — the LLM path (glossary, context, tone) |
| `facebook/nllb-200-distilled-600M` | CC-BY-NC | no — research/dev only |
| `facebook/seamless-m4t-v2-large` | CC-BY-NC | no — 96 text / 101 speech languages; dev only |
| `CohereLabs/aya-expanse-8b` | CC-BY-NC | no — dev only |
| `Unbabel/XCOMET-XL`, `wmt22-cometkiwi-da` | CC-BY-NC-**SA** | **no — evaluation tooling, never a shipped feature** |

(Licences read from each model card through the Hugging Face API, 2026-11-06.)
*Where*: the "local model" engine offers madlad/Qwen; anything CC-BY-NC appears only as a developer
switch the release build does not expose. This is the XTTS-v2 trap one layer deeper, and it is easy
to walk into because NLLB is the obvious first answer for Arabic.

**J2. Measure with subtitle-specific checks we own, because the good metric is non-commercial.**
COMET / COMETKiwi are reference-free and XCOMET even localises errors
([Unbabel/COMET](https://github.com/Unbabel/COMET)), but their weights are CC-BY-NC-SA. So the
shipped quality gate is deterministic and free: per-cue **length ratio** (target/source), **CPS
compliance** against the chosen profile, **cue count preserved**, **no merged or dropped cues**,
**numbers/dates preserved**, **termbase terms present**, and a **script check** (an Arabic target
that comes back in Latin script is a failure, not a translation). Each is one assertion, and together
they catch what a user notices. *Effort*: S.

**J3. Give the model the movie, not just the line.**
A 2025 ACL student-workshop paper, *Translating Movie Subtitles by Large Language Models using
Movie-meta Information*, argues that meta information (title, genre, synopsis) is what lets an LLM
resolve the colloquial, context-dependent lines that make subtitles hard
([aclanthology.org/2025.acl-srw.22](https://aclanthology.org/2025.acl-srw.22.pdf)). We already store a
TMDB key and already learn the series/episode from the *arr payload, so a short metadata block in the
system prompt costs nothing. *Effort*: S.

**J4. Two passes, cached.** Translate, then a review pass that sees its own output plus the rules
(CPS budget, termbase, tone) and fixes only what breaks them. Cache by `(engine, model, prompt_hash,
batch)` so a retry never re-pays for work already done — the pattern VideoLingo ships. *Effort*: S–M.

**J5. A caution before designing around whisper's translate task.** Whisper's `translate` task is
documented as producing English; do not assume it can be pointed at Arabic. Check the whisper.cpp
arguments before putting it in a plan — the safe design is ASR in the source language plus our own MT
step.

---

## Roadmap mapping

| Idea | Phase | Notes |
|---|---|---|
| A1/A2/A3 *arr SignalR + webhooks + `handle` CLI | P2 | unblocks "monitored only", kills polling, skips redundant work via `mediaInfo` |
| A4 profiles/cutoff/path mappings, A5 hash, A6/A7 providers | P2 | config work that must precede provider work |
| B1 linter (via `seconv` first), B2 CPS-aware translation, B3 sync gate | P2/P3 | ships with the pipeline, not after it |
| G3/G4 MCP task tools + progress/cancellation | P1/P3 | wraps existing code |
| C1 dual ASR, C2 word timings, C3 alignment | P4 | the توزين milestone |
| E/F packaging (linuxserver image, multi-arch, Unraid) | P2 | parallel to the Docker work |
| D1/D2/D3 OCR, D5–D7 scenes/chapters | P4 | after the pipeline is trusted |
| J1 shipped-model licences, J2 quality gate, J3 metadata prompt | P3 | the translation quality story |
| C4 mix, C5 TTS, H5 diarization/termbase | P5 | licence review first |
| D8/D9/D10 moderation + GPU lease | P5 | report-only until proven; the lease belongs in P2 |

---

## Research gaps (honest)

- **Docker/Unraid/TrueNAS**: first-party pages only; Synology and TrueNAS template specifics were
  **not** verified. No test on arm64 hardware — ARM claims are documentation, not measurement.
- **Moderation model licence**: open question. NudeNet is AGPL; a permissively licensed alternative
  has not been confirmed, so no detector should ship until it is.
- **OCR accuracy on subtitle bands**: no published per-engine benchmark for burned-in subtitles was
  found; the CER table is general in-scene video text and understates subtitle accuracy.
- **Whisper speed**: numbers come from the projects' own READMEs plus one peer-reviewed CPU
  benchmark; only the GCC-PHAT/ducking probes were re-measured on this machine.
- **OpenSubtitles pricing**: the subscription page renders empty through a doc extractor, so no
  price is claimed here.
