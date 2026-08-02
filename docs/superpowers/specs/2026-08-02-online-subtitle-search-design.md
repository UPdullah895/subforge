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
2. **OpenSubtitles.com** — optional, requires the user's own credentials (see "OpenSubtitles auth
   model" below — this changed from the original "just an API key" framing after checking their
   live docs). Supports file-hash matching, which is more accurate than title search because it
   matches the exact release (correct timing/frame count). Skipped silently (not an error) when
   no credentials are configured.

### OpenSubtitles auth model (corrected during research)

Their live docs (`https://opensubtitles.stoplight.io/docs/opensubtitles-api`) state: *"Using one
key per application is mandatory — if you develop an application requiring users to create their
own API key we will ban their access."* Their `/download` endpoint requires **both** an `Api-Key`
header (identifies the consuming application) **and** an `Authorization: Bearer <JWT>` header,
where the JWT comes only from `POST /api/v1/login` with a **username + password** — not from an
API key.

The conventional fix (what Bazarr does) is one API key embedded by the app's maintainer, with
each end user logging in via username/password for their personal quota. **SubForge deliberately
does not do this**: because SubForge is fully open source, the project embeds no OpenSubtitles
credential of its own (unlike Subsource, above). Instead, **each user supplies all three**: their
own OpenSubtitles.com API key (created from their own account's Profile → API Consumers page),
username, and password. This is a disclosed, deliberate departure from OpenSubtitles' stated
preference — since every user brings their own key rather than sharing one app-wide key, the
blast radius of any single misuse/ban is that one user's own key, not a shared SubForge-wide key
affecting everyone.

Flow: `POST /api/v1/login` (headers: `Api-Key`, `Content-Type: application/json`, `User-Agent`;
body `{"username", "password"}`) → response `{token, base_url, user: {allowed_downloads, ...},
status}`. Cache the JWT for 24h (their documented validity window); re-login on expiry or on a
401/`"invalid"` response. Login itself is rate-limited (1/sec, 10/min, 30/hour per their docs) —
never retry a login in a loop.

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

- `subsource.rs` — Subsource API client. Base URL `https://api.subsource.net`, auth header
  `X-API-Key`, documented rate limits 60/min, 1800/hour, 7200/day. The public docs page
  (`subsource.net/api-docs`) is a JS-rendered app that blocks automated fetching (403), so the
  exact request/response JSON field names must be confirmed with a live "Try it out" call (or by
  inspecting the official `.NET` wrapper at `github.com/moviecollection/sub-source`, which exposes
  the same shape as `SearchMoviesAsync({Query, SearchType}) → movie.id`, then
  `GetSubtitlesAsync({MovieId, Language}) → subtitle list`, then `GetSubtitleByIdAsync(id)`, then
  `DownloadSubtitleAsync(id) → zip file`) as the first task of implementation, not assumed here.
- `opensubtitles.rs` — OpenSubtitles.com REST client, verified against their live Stoplight docs:
  - Base URL `https://api.opensubtitles.com/api/v1`. Every request needs `Api-Key` and
    `User-Agent` (or `X-User-Agent`) headers; `Authorization: Bearer <JWT>` is added once logged
    in.
  - `login(api_key, username, password) -> {token, base_url, user}` — `POST /login`, body
    `{"username", "password"}`. Note the response can carry a different `base_url` (e.g.
    `vip-api.opensubtitles.com`) that subsequent requests should use.
  - Hash computation: `moviehash = filesize + sum_u64_le(first 64KiB) + sum_u64_le(last 64KiB)`,
    unsigned 64-bit wraparound arithmetic, data read as little-endian `u64` (8192 values per
    64KiB chunk), formatted as a zero-padded lowercase 16-hex-digit string. Files under 128KiB
    can't be hashed (both regions overlap the same bytes when the file is between 64KiB and
    128KiB). This is the same "OSHash"/"moviehash" algorithm documented at
    `opensubtitles.github.io/oshash` and used by Subliminal/`subliminal` and other established
    clients — cross-checked across two independent sources during planning, not assumed from one.
  - `search(moviehash?, query?, languages, parent_imdb_id?/parent_tmdb_id?, season_number?,
    episode_number?, year?, type) -> GET /subtitles` — response
    `{total_pages, total_count, per_page, page, data: [{id, type, attributes: {subtitle_id,
    language, download_count, hearing_impaired, hd, feature_details: {title, year,
    season_number, episode_number, ...}, files: [{file_id, cd_number, file_name}]}}]}`. When
    `moviehash` is sent, matches get `moviehash_match: true` and sort first.
  - `download(file_id, jwt) -> POST /download` — requires **both** `Api-Key` and
    `Authorization: Bearer` headers per their docs (their separate "Getting started" page
    describes a small anonymous quota that appears to contradict this, so the client always
    logs in first rather than relying on undocumented anonymous behavior). Body `{"file_id"}`,
    response `{link, file_name, requests, remaining, message, reset_time_utc}` — `link` is a
    temporary URL (≤3 hours) to fetch the actual SRT.
  - Distinct handling for HTTP 429 / rate-limit response headers (`ratelimit-remaining`,
    `x-ratelimit-remaining-second`) vs. other failures — both map to "treat as miss, fall
    through" per the orchestration logic below, never a crash or blocking retry loop.
- `mod.rs` — orchestration: a `find_subtitle(request, opensubtitles_credentials:
  Option<OpenSubtitlesCredentials>)` function (where `OpenSubtitlesCredentials { api_key,
  username, password }`) implementing the fallback order above. Pure business logic — it
  receives already-resolved credentials as a parameter rather than reading credential storage
  itself, so it stays backend-agnostic and testable with mocked HTTP.

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
Linux/Windows/macOS) assumed a single desktop binary and a single key value. With server mode
also in scope, and the auth-model correction above (three fields — API key, username, password —
instead of one), this necessarily forks:

- **Tauri desktop app**: new `src-tauri/src/credentials.rs` module wrapping the `keyring` crate
  (confirmed support: Secret Service/libsecret on Linux, Credential Manager on Windows, Keychain
  on macOS). All three fields (`api_key`, `username`, `password`) are stored as one
  JSON-serialized secret under a single keyring `Entry` (service `"subforge"`, account
  `"opensubtitles"`), behind a small trait so unit tests inject a mock instead of touching the
  real OS credential store. None of the three raw values are ever sent back to the frontend
  after saving — only a `configured: bool` status. Commands: `set_opensubtitles_credentials`,
  `test_opensubtitles_connection`, `clear_opensubtitles_credentials`, `get_opensubtitles_status`.
  - Linux build dependency: `libsecret-1-dev` (apt) / `libsecret` (pacman), added to
    `.github/workflows/ci.yml`'s `rust-tauri` job and `.github/workflows/release.yml`'s Linux
    step, alongside the existing `libwebkit2gtk-4.1-dev` etc.
  - `cargo check` in CI only needs the dev headers at compile time, not a running Secret Service
    daemon, so CI stays green without a desktop session.
- **Axum server** (headless, Docker-oriented): the three fields join the existing plaintext
  `AppConfig` as `opensubtitles_api_key: Option<String>`, `opensubtitles_username: Option<String>`,
  `opensubtitles_password: Option<String>`, exactly like `tmdb_api_key`, `deepl_api_key`, etc.
  today (confirmed both binaries already share one `config.json` on disk for these). Adds
  `OPENSUBTITLES_API_KEY` / `OPENSUBTITLES_USERNAME` / `OPENSUBTITLES_PASSWORD` env var
  overrides, read at startup and taking precedence over the config file values — a common
  self-hosted/Docker convention, and new compared to the server's current config handling (no
  other key has an env override today).
  - This does **not** meet "stored securely" for server mode — no coherent alternative exists
    for a headless process. It's consistent with how the server already treats every other
    secret, and is a deliberate, disclosed deviation from the original spec's letter, driven by
    the decision to cover both backends.
- The cached JWT (from login) is kept **in memory only** in both binaries (process-lifetime
  `Mutex<Option<CachedToken>>` state, not persisted to disk or keychain) — it's a derived,
  short-lived (24h) credential, re-obtainable from the stored username/password, so persisting it
  separately would just be another place for a secret to leak without benefit.

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
  OpenSubtitles `configured: bool` status (never the raw credentials), Subsource reachability
  status.
- New "Subtitle Sources" section in `SettingsView.vue`, matching the existing "API & Providers"
  card pattern (`providers` array + `testStatus` reactive + `test_provider_config` command). The
  OpenSubtitles card is a **login form** (API key, username, password fields — not a single key
  field, per the auth-model correction above), with a "Test connection" button that calls
  `set_opensubtitles_credentials` then `test_opensubtitles_connection`. The `"opensubtitles"`
  provider joins the existing match in
  `src-tauri/src/commands/providers.rs::test_provider_config` (reading from the keyring module
  instead of `AppConfig`, unlike every other arm there). Subsource shows as always-active with a
  status indicator only (reachable/unreachable ping), no config. Help text explains, via the
  app's existing i18n system (`en.json`/`ar.json`, not hardcoded to one language), why a personal
  OpenSubtitles account + API key are required (linking to registration and the API docs), and
  that this is a deliberate SubForge choice (no shared app key) rather than an OpenSubtitles
  requirement.
- `AutoTranslateWizard.vue`'s `FileStage` type gains a `"searching"` stage before
  `"extracting"`/`"translating"`. When the toggle is on, `startPipeline()` calls
  `search_online_subtitle` per file/lang first; on a hit it downloads and (if `autoMerge` is on)
  merges via the same `mkvtoolnix::merge_subtitles_async` call the translate pipeline already
  uses, then marks the job `"done"` — skipping translation entirely. On a miss, stage becomes
  `"translating..."` and falls through to the existing `run_translation_pipeline` call unchanged.

## Testing

- Rust unit tests for the hash algorithm against known-good fixtures (cross-checked hand
  computation, since OpenSubtitles' docs don't publish a downloadable test-vector file — see
  planning notes for the fixture used).
- Rust unit tests for the fallback-order orchestration logic (`mod.rs`) with mocked HTTP
  responses: OS-credentials-present-and-hash-hits, OS-credentials-present-but-misses-falls-to-
  Subsource, no-OS-credentials-uses-Subsource-only, both-miss-falls-through-to-not-found,
  429-treated-as-miss, network-error-treated-as-miss, expired-JWT-triggers-relogin-once.
- Credential module tests use the mock trait, never the real OS keychain — keeps CI green
  without a desktop session or Secret Service daemon.
- Manual verification checklist (from the original request), scoped per the environment
  available. The OpenSubtitles-dependent items additionally require the user to supply a real
  OpenSubtitles.com account (API key + username/password) for end-to-end testing — this can't be
  faked or embedded per the auth-model decision above:
  - [ ] Builds and runs on Linux (primary target, verifiable directly).
  - [ ] Builds on Windows via CI (`release.yml` already has a `windows-latest` matrix entry);
        interactive run/verification is the user's to do.
  - [ ] macOS: best-effort only — no macOS hardware or CI runner is available in this
        environment. Code will be written platform-neutral and every crate used (`keyring`,
        `reqwest`) confirmed via its docs to officially support macOS, but actual
        compile/run/keychain verification on macOS is out of reach here and is the user's to do.
  - [ ] Subsource-only path (no OpenSubtitles credentials).
  - [ ] OpenSubtitles hash-match path (valid credentials configured) — needs a real account.
  - [ ] Graceful fallback to auto-translate when both sources fail/miss.
  - [ ] OpenSubtitles credentials persist across app restarts (keychain on desktop, config
        file/env var on server).
  - [ ] Rate-limit/network-failure handling doesn't crash the app or hang the UI.

## Explicitly out of scope

- Migrating the other existing API keys (TMDB, DeepL, OpenAI, Google) to the keychain on the
  desktop app — only the new OpenSubtitles credentials get this treatment, per the original
  request. This leaves a visible inconsistency (one credential set in the keychain, the rest in
  plaintext) that is not being resolved here.
- Registering a shared SubForge-wide OpenSubtitles API key — deliberately not done; see
  "OpenSubtitles auth model" above. Subsource still needs one app-level key registered by the
  maintainer before this ships (unchanged from the original decision).
- Adding macOS to `release.yml`'s build matrix (the user chose best-effort/code-review-only for
  macOS over expanding CI).
- Any UI for manually picking between multiple online-search candidates — matches are chosen
  automatically per the fallback-order rule; this fits the "automatic before-translation" framing
  of the request and keeps batch runs unattended.
