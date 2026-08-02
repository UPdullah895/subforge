# Online Subtitle Search Before Auto-Translation

**Date:** 2026-08-02
**Status:** Approved for planning

## Goal

Before SubForge's existing auto-translation pipeline runs for a video/episode, try to find an
existing human-made subtitle in the target language from online sources. If found, download and
use it instead of translating. If not found (or the configured source is unavailable), fall back
to the current auto-translate flow unchanged.

## Sources

1. **Subsource** — primary, default-on, no user setup. Uses an app-embedded API key (per the
   product decision that Subsource currently has no restrictive per-user quota). Title/season/
   episode/year search only.
2. **OpenSubtitles.com** — optional, requires the user's own free API key (free tier is
   5 downloads/24h and tied to the key, so an app-embedded key is not viable). Supports file-hash
   matching, which is more accurate than title search because it matches the exact release
   (correct timing/frame count). Skipped silently (not an error) when no key is configured.

## Fallback order

1. If an OpenSubtitles key is configured → hash-match first.
2. Try Subsource title/season/episode search.
3. If nothing found on any available source → fall back to auto-translation.
4. If both sources return a match, prefer the OpenSubtitles hash match.

The whole step is gated by a user-facing toggle, "Search for existing subtitles before
translating," default **on**. Turning it off skips both sources entirely.

## Scope: both backends

SubForge ships two backends that both currently exist in the repo: the Tauri desktop app
(`src-tauri/`) and a separate Axum REST server (`server/`) used for a still-in-progress "server
mode" (Docker/self-hosted). Both are in scope for this feature, mirroring how `rename.rs`
(TMDB/TVmaze) already wraps the shared `subforge-metadata` crate in each binary today.

## Architecture

New shared crate `crates/subforge-subtitle-sources`, following the existing
`subforge-metadata` pattern (already depended on by both `src-tauri/Cargo.toml` and
`server/Cargo.toml`):

- `subsource.rs` — Subsource API client: search by title/year/season/episode, download the
  matched file. Exact endpoint shapes to be confirmed against Subsource's docs during planning —
  not fabricated here.
- `opensubtitles.rs` — OpenSubtitles.com REST client (`https://opensubtitles.stoplight.io/`):
  - Hash computation following their documented (non-standard) algorithm exactly — must be
    verified against their spec and, ideally, checked against published test vectors during
    planning, not assumed.
  - `search_by_hash(hash, filesize, lang)`.
  - `search_by_title(title, year, season, episode, lang)` fallback for when no hash match exists
    even with a key configured.
  - `download(file_id, api_key)`.
  - Distinct handling for HTTP 429 (rate limit) vs. other failures.
- `mod.rs` — orchestration: a `find_subtitle(request, opensubtitles_key: Option<String>)`
  function implementing the fallback order above. Pure business logic — it receives an
  already-resolved API key as a parameter rather than reading credential storage itself, so it
  stays backend-agnostic and testable with mocked HTTP.

Both binaries get thin wrapper layers over this crate:

- `src-tauri/src/commands/subtitle_sources.rs` — Tauri commands.
- `server/src/routes/subtitle_sources.rs` — Axum routes, registered in `server/src/main.rs`'s
  `api_routes()` alongside the existing `/rename/*` group.

All HTTP calls are async (`reqwest`, already a dependency of both binaries and the shared
metadata crate) and never block a UI/request thread. Every source call is wrapped so network
errors, 429s, and missing/invalid keys fall through to "not found" (triggering auto-translate)
rather than failing the job or crashing — mirroring the existing per-file try/catch isolation in
`AutoTranslateWizard.vue`'s `startPipeline()`.

## Credential storage — forked by binary

The original requirement (OS-keychain storage for the OpenSubtitles key, verified equivalent on
Linux/Windows/macOS) assumed a single desktop binary. With server mode also in scope, this
necessarily forks:

- **Tauri desktop app**: new `src-tauri/src/credentials.rs` module wrapping the `keyring` crate
  (confirmed support: Secret Service/libsecret on Linux, Credential Manager on Windows, Keychain
  on macOS). Behind a small trait so unit tests inject a mock instead of touching the real OS
  credential store. The raw key is never sent back to the frontend after it's saved — only a
  `configured: bool` status. Commands: `set_opensubtitles_key`, `test_opensubtitles_connection`,
  `clear_opensubtitles_key`, `get_opensubtitles_status`.
  - Linux build dependency: `libsecret-1-dev` (apt) / `libsecret` (pacman), added to
    `.github/workflows/ci.yml`'s `rust-tauri` job and `.github/workflows/release.yml`'s Linux
    step, alongside the existing `libwebkit2gtk-4.1-dev` etc.
  - `cargo check` in CI only needs the dev headers at compile time, not a running Secret Service
    daemon, so CI stays green without a desktop session.
- **Axum server** (headless, Docker-oriented): the key joins the existing plaintext
  `AppConfig.opensubtitles_api_key: Option<String>` field, exactly like `tmdb_api_key`,
  `deepl_api_key`, etc. today (confirmed both binaries already share one `config.json` on disk
  for these). Adds an `OPENSUBTITLES_API_KEY` env var override, read at startup and taking
  precedence over the config file value — a common self-hosted/Docker convention, and new
  compared to the server's current config handling (no other key has an env override today).
  - This does **not** meet "stored securely" for server mode — no coherent alternative exists
    for a headless process. It's consistent with how the server already treats every other
    secret, and is a deliberate, disclosed deviation from the original spec's letter, driven by
    the decision to cover both backends.

## Metadata: title/season/episode/year for the search

Today, `RenameModal.vue`'s `renameStore` resolves `{selectedSeries: {name, year, provider, id},
previewRows: [{old_path, new_name, season, episode}]}` right before `apply()` renames files on
disk — then discards it. This feature persists that match:

- `renameStore.apply()` writes `{title, year, season, episode, provider, seriesId}` into a new
  `batchStore.resolvedMetadata: Record<string, ResolvedMetadata>` map, keyed by each file's
  **new** path (paths change on rename).
- The search step (triggered from `AutoTranslateWizard.vue`) looks up the current file path in
  `resolvedMetadata` first.
- If absent (file was never renamed/matched via TMDB/TVmaze — e.g. manual rename or skipped),
  falls back to `subforge_metadata::parser::parse_filename()` for a best-effort
  title/season/episode guess. No year in this fallback path; Subsource title search runs with
  reduced precision, OpenSubtitles hash-match is unaffected (hash doesn't need title/year at
  all).

## Downloaded subtitle placement

A found subtitle is written next to the video using the same `{video_stem}.{lang}.{ext}` naming
convention `extract_all_subtitles` already writes and `detect_external_subs`
(`src-tauri/src/commands/batch.rs`) already reads back on a later scan — so a subtitle found
online is indistinguishable, to the rest of the app, from one extracted locally. If `auto_merge`
is on, it's then muxed into the video the same way `run_translation_pipeline`'s merge stage does
today, via `mkvtoolnix::merge_subtitles_async`.

## Frontend

- New Pinia store `subtitleSourcesStore.ts` (8th store, alongside `appStore`, `subtitleStore`,
  `audioStore`, `batchStore`, `renameStore`, `transcribeStore`, `translateStore`). Holds: the
  on/off toggle (persisted via `AppConfig.search_online_before_translate`, default `true`),
  OpenSubtitles `configured: bool` status (never the raw key), Subsource reachability status.
- New "Subtitle Sources" section in `SettingsView.vue`, matching the existing "API & Providers"
  card pattern (`providers` array + `testStatus` reactive + `test_provider_config` command). The
  `"opensubtitles"` provider joins the existing match in
  `src-tauri/src/commands/providers.rs::test_provider_config` (reading the key from the keyring
  module instead of `AppConfig`, unlike every other arm there). Subsource shows as always-active
  with a status indicator only (reachable/unreachable ping), no config. Help text for
  OpenSubtitles links to registration + API docs and explains the personal-key requirement,
  using the app's existing i18n system (`en.json`/`ar.json`) rather than being hardcoded to one
  language.
- `AutoTranslateWizard.vue`'s `FileStage` type gains a `"searching"` stage before
  `"extracting"`/`"translating"`. When the toggle is on, `startPipeline()` calls
  `search_online_subtitle` per file/lang first; on a hit it downloads and (if `autoMerge` is on)
  merges via the same `mkvtoolnix::merge_subtitles_async` call the translate pipeline already
  uses, then marks the job `"done"` — skipping translation entirely. On a miss, stage becomes
  `"translating..."` and falls through to the existing `run_translation_pipeline` call unchanged.

## Testing

- Rust unit tests for the hash algorithm against OpenSubtitles' published test vectors.
- Rust unit tests for the fallback-order orchestration logic (`mod.rs`) with mocked HTTP
  responses: OS-key-present-and-hash-hits, OS-key-present-but-misses-falls-to-Subsource,
  no-OS-key-uses-Subsource-only, both-miss-falls-through-to-not-found, 429-treated-as-miss,
  network-error-treated-as-miss.
- Credential module tests use the mock trait, never the real OS keychain — keeps CI green
  without a desktop session or Secret Service daemon.
- Manual verification checklist (from the original request), scoped per the environment
  available:
  - [ ] Builds and runs on Linux (primary target, verifiable directly).
  - [ ] Builds on Windows via CI (`release.yml` already has a `windows-latest` matrix entry);
        interactive run/verification is the user's to do.
  - [ ] macOS: best-effort only — no macOS hardware or CI runner is available in this
        environment. Code will be written platform-neutral and every crate used (`keyring`,
        `reqwest`) confirmed via its docs to officially support macOS, but actual
        compile/run/keychain verification on macOS is out of reach here and is the user's to do.
  - [ ] Subsource-only path (no OpenSubtitles key).
  - [ ] OpenSubtitles hash-match path (valid key configured).
  - [ ] Graceful fallback to auto-translate when both sources fail/miss.
  - [ ] OpenSubtitles key persists across app restarts (keychain on desktop, config file/env var
        on server).
  - [ ] Rate-limit/network-failure handling doesn't crash the app or hang the UI.

## Explicitly out of scope

- Migrating the other existing API keys (TMDB, DeepL, OpenAI, Google) to the keychain on the
  desktop app — only the new OpenSubtitles key gets this treatment, per the original request.
  This leaves a visible inconsistency (one key in the keychain, the rest in plaintext) that is
  not being resolved here.
- Adding macOS to `release.yml`'s build matrix (the user chose best-effort/code-review-only for
  macOS over expanding CI).
- Any UI for manually picking between multiple online-search candidates — matches are chosen
  automatically per the fallback-order rule; this fits the "automatic before-translation" framing
  of the request and keeps batch runs unattended.
