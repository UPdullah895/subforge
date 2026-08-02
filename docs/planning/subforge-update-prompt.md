# SubForge Update — Batch Rename, Auto-Translate, External Subs Detection, Unified API Settings, Ollama Fix

## Context (for reference, don't re-derive)

SubForge stack:
- Frontend: Vue 3 (Composition API, `<script setup>`) + TypeScript, Pinia, vue-i18n (en/ar), Tailwind v4, Vite, lucide-vue-next.
- Desktop shell: Tauri v2 — Rust backend (`src-tauri/`) with `#[tauri::command]` handlers, invoked via `invoke()`.
- Alternate deployment: `server/` — standalone Rust/Axum HTTP+WebSocket backend, same frontend, dispatched via the `isTauri()` branch in `src/api/client.ts` / `src/api/events.ts`.
- External tools: mkvmerge/mkvextract, ffmpeg/ffprobe, faster-whisper, Ollama/LibreTranslate/DeepL/OpenAI as translation engines.

**Hard constraint: every new feature must work in BOTH the Tauri desktop build and the Axum server build.** Wherever business logic isn't trivial UI glue, put it in a shared Rust crate the two backends both depend on, rather than duplicating logic in `src-tauri` and `server/` separately.

Reference screenshot: the current Batch Operations table has columns File / Video / Audio / Subs, a "Scan Directory" button, and an "Extract Subs" button. This update adds a "Rename" and a "Translate" button next to "Extract Subs", plus a new "External Subs" column.

---

## 1. Shared metadata/rename crate

Create `crates/subforge-metadata/` (adjust path to match your existing workspace layout) with:

```
crates/subforge-metadata/
  Cargo.toml
  src/
    lib.rs
    tmdb.rs       // TMDB HTTP client
    tvmaze.rs     // TVmaze HTTP client (no auth required)
    parser.rs     // filename -> {series_guess, season, episode, quality/tags}
    matcher.rs     // series_guess + provider results -> ranked matches
    renamer.rs      // apply naming pattern + sanitize for filesystem
    cache.rs         // local cache of {provider, series_id} -> episode list
```

- `tmdb.rs`: search endpoint (`/search/tv`), episode endpoint (`/tv/{id}/season/{s}/episode/{e}`), always request `language=en-US` (renamed files should use English titles per product decision — better for cross-tool recognition).
- `tvmaze.rs`: no API key needed at all; note this explicitly in the Settings UI (see section 4) so users don't get confused looking for a TVmaze key field.
- `parser.rs`: try a **chain** of regexes against the filename stem in order, stop at first match:
  - `[Ss](\d{1,2})[Ee](\d{1,3})`
  - `(\d{1,2})x(\d{1,3})`
  - `[Ee]pisode[.\s_-]?(\d{1,3})`
  Also extract a separate "tags" capture (resolution like `1080p`/`720p`, codec like `x264`/`HEVC`, release group in brackets) so it can optionally be preserved in the new filename.
- `renamer.rs`: token-based pattern engine. Support at minimum: `{title}`, `{s}` (zero-padded season), `{e}` (zero-padded episode), `{episodeTitle}`, `{quality}`. Sanitize output against Windows-reserved characters (`\ / : * ? " < > |`) even on Linux/macOS builds, since project targets "runs on every OS."
- `cache.rs`: simple on-disk cache (JSON file under the app's data dir is fine — SQLite only if you already have it wired up elsewhere) keyed by `(provider, series_id)` storing the full episode list, so opening a folder with 10 episodes of the same show doesn't fire 10 API searches.

Both `src-tauri` and `server/` depend on this crate and expose thin wrappers:
- Tauri: `#[tauri::command] async fn search_series(provider, query) -> Vec<SeriesMatch>`, `get_episodes(provider, series_id) -> Vec<Episode>`, `preview_rename(files, series_id, provider, pattern) -> Vec<RenamePreviewRow>`, `apply_rename(mapping: Vec<{old_path, new_path}>) -> RenameResult`.
- Axum: equivalent `POST /api/rename/search`, `/api/rename/episodes`, `/api/rename/preview`, `/api/rename/apply`.

---

## 2. Feature: Rename modal (manual + automatic)

**Entry point:** new "Rename" button in the Batch toolbar next to "Extract Subs" (operates on the selected files), plus a per-row "Rename" option in each row's expand chevron menu for single-file rename.

**Modal UX:**

1. Mode toggle at top: **Manual** / **Auto — TMDB** / **Auto — TVmaze**.
2. **Manual mode:** simple table, one editable text input per selected file pre-filled with the current name (minus extension), Apply button.
3. **Auto mode:**
   - Search box pre-filled with a best-guess series name derived from `parser.rs` (strip season/episode/tags/extension from the first selected file).
   - Results list from the chosen provider (poster thumbnail if TMDB, year, ) — user clicks to select the correct series.
   - Once selected: fetch full episode list (via cache if already fetched), auto-map each selected file's parsed `season`/`episode` to the corresponding episode title.
   - Naming pattern input field with quick-insert token buttons (`{title}` `{s}` `{e}` `{episodeTitle}` `{quality}`), default pattern: `{title} - S{s}E{e} - {episodeTitle}`.
   - Live preview table: **Old Name → New Name**, one row per file. Rows where season/episode couldn't be parsed, or no matching episode was found, are highlighted (e.g. amber background) with an inline dropdown so the user can manually pick the right episode for that row instead of blocking the whole batch.
   - Confirm button calls `apply_rename` with the final old→new path mapping.
4. All new UI strings need both `en.json` and `ar.json` entries in the i18n files — do not hardcode English or Arabic strings in components.

---

## 3. Feature: One-click auto-translate workflow

**Entry point:** new "Translate" button in the Batch toolbar (and per-row), opens a wizard modal — separate from the existing manual Translate page, this is the "just do it for me" fast path.

**Wizard steps (single modal, not separate pages):**

1. **Provider** — dropdown populated only with providers that currently have a valid, saved API key/connection in Settings (see section 4). If none are configured, show an inline message with a link straight to Settings > API Keys instead of an empty dropdown.
2. **Languages** — "From" and "To" language selectors (searchable dropdowns, ISO 639-1 based).
3. **Auto-merge toggle** — "Merge into video automatically when done" (default: ON). If off, translated `.srt`/`.ass` files are left alongside the video, not muxed in.
4. **Start** button kicks off the pipeline.

**Backend orchestration** (shared crate function, e.g. `run_translation_pipeline`):
```
for each file:
  1. extract_subs (reuse existing extraction code path — don't duplicate it)
  2. translate(text, provider, source_lang, target_lang)
  3. if auto_merge: mux translated subtitle back into the mkv via mkvmerge
  4. emit progress event: { file, stage: "extracting"|"translating"|"merging"|"done"|"error", error?: string }
```
Emit progress via the existing event mechanism (Tauri events / WebSocket in server mode — reuse `src/api/events.ts`'s existing dispatch, don't build a second event path).

**Frontend:** after starting, show a per-file progress list with status badges (queued/extracting/translating/merging/done/error) and a cancel button. On error, show the specific error inline per file (see the Ollama fix in section 6 for what a *good* error message looks like — don't just dump the raw HTTP error).

---

## 4. Feature: Unified "API Keys" settings section

New section in Settings (e.g. Settings > API & Providers) listing every external tool the app can use, one row/card per provider:

| Provider | Field(s) | Notes to show in UI |
|---|---|---|
| TMDB | API key | Used for auto-rename matching |
| TVmaze | *(none)* | Free, no key needed — show this explicitly so users don't go hunting for a key field |
| DeepL | API key | Note free vs. pro endpoint differ by key format; auto-detect or add a toggle |
| OpenAI | API key + model name/selector | |
| Google Translate | API key | |
| Ollama | Base URL (default `http://localhost:11434`), no key | See section 6 — must be user-editable, not hardcoded |

Each provider row must include:
- Masked input field for the key (show/hide toggle).
- **"Test" button** that pings the provider (for TMDB: a lightweight search call; for Ollama: `GET {base_url}/api/tags`; for DeepL/OpenAI/Google: their respective lightest validation call) and shows a clear ✅/❌ result inline, not just a raw error.
- **Actual written instructions for how to get the key**, expandable inline (not just an icon or a link that may break) — see section 6 for the exact content needed per provider. This replaces the current confusing "!" icon pattern.

**Storage:** persist keys locally on the user's device.
- Tauri build: use `@tauri-apps/plugin-store` (or `tauri-plugin-stronghold` if you want OS-keychain-backed secrecy — flag this as a decision point, plain JSON store is simpler but not encrypted; Stronghold is more secure but heavier to wire up. Pick whichever matches the project's current security bar and note the choice in the PR description).
- Server build: since there's no OS keychain to reach for, store in a local config file server-side and document that anyone with filesystem access to the server can read it — this is an inherent limitation of self-hosted mode, not something to solve here.
- Expose via Pinia `useSettingsStore` (`apiKeys: Record<Provider, {key?: string, baseUrl?: string, verified: boolean}>`), backed by `#[tauri::command] save_provider_config / get_provider_config / test_provider_config`.

---

## 5. Feature: External subtitle detection + status column

**Detection:** during directory scan, for each video file, look for sibling files matching `{base_name}.{lang_code}.{ext}` (`.srt` and `.ass` at minimum), where `lang_code` matches an ISO 639-1 code (e.g. `Among Us - S01E01.ar.srt`).

**Data model addition** (per file/episode row):
```ts
externalSubs: Array<{
  lang: string;              // 'ar', 'en', ...
  status: 'ok' | 'pending' | 'error';
  errorMessage?: string;
  path: string;
}>
```

**Table UI:** new "External Subs" column in the Batch table, showing one small colored badge per detected language:
- 🟢 green = detected and parsed successfully
- 🟡 yellow = currently being worked on (extraction/translation/merge in progress from the section-3 pipeline)
- 🔴 red = error

When a row has any red badge, render an expandable alert strip directly under that table row (spanning the row width) showing the specific `errorMessage` — don't make the user hover or click into a separate panel to find out what went wrong.

---

## 6. Bug fix: Ollama connection error + Translate page instructions

**Bug:** `HTTP request failed: error sending request for url (http://localhost:11434/v1/chat/completions)` — this is a raw `reqwest` connection-refused error leaking to the UI unformatted. Root cause is almost always Ollama not running, or a wrong host/port.

Fix in the Ollama client code:
- Catch the connection-refused case specifically (`reqwest::Error::is_connect()`), and surface a friendly, localized (en/ar) message instead of the raw error, e.g.:
  - EN: "Couldn't reach Ollama. Make sure it's running on your machine (`ollama serve`) and that the address in Settings is correct."
  - AR: "تعذر الاتصال بـ Ollama. تأكد إنه شغّال على جهازك (`ollama serve`) وإن العنوان بالإعدادات صحيح."
- Add the "Test" button described in section 4 for the Ollama row specifically, so users find out *before* starting a real translation job, not after it fails mid-batch.
- Make the base URL field actually editable in Settings (currently appears hardcoded to `localhost:11434`) so users running Ollama on another machine/port aren't stuck.

**Translate page instructions fix:** the current per-provider "how to set this up" section has broken links and an unclear "!" icon that's apparently meant to signal a download requirement. Replace with plain, verified step lists, expandable per provider (same component can be reused from section 4's "how to get this key" panels — build it once, use it in both places):

- **Ollama:**
  1. Download Ollama from the official site: `https://ollama.com/download`
  2. Run `ollama pull <model-name>` in a terminal (suggest a specific model, e.g. `llama3.1`) to download a model.
  3. Make sure the Ollama service is running in the background before starting a translation.
- **DeepL:** sign up and get a key at `https://www.deepl.com/pro-api` — note the free tier has a monthly character limit.
- **OpenAI:** get a key at `https://platform.openai.com/api-keys` — note usage is billed per token.
- **Google Translate:** create/enable the Cloud Translation API and get credentials from the Google Cloud Console (`https://console.cloud.google.com/apis/credentials`).

Verify every URL actually resolves before shipping — don't reuse the currently broken links as-is. Remove the ambiguous icon-only indicator entirely; replace with a labeled, always-visible "Setup instructions" expander per provider.

---

## Acceptance checklist

- [ ] Rename modal supports manual, TMDB, and TVmaze modes; unmatched rows are fixable inline, not blocking.
- [ ] Naming pattern is user-editable with token buttons; output filenames are sanitized cross-platform.
- [ ] Translate wizard only lists providers with a verified key/connection; pipeline emits per-file progress; auto-merge toggle respected.
- [ ] Settings has one unified API/provider section covering TMDB, TVmaze (no-key noted), DeepL, OpenAI, Google Translate, Ollama, each with Test button and real written setup instructions (no broken links, no bare icons).
- [ ] Keys/config persist locally and are read on both Tauri and server builds.
- [ ] Batch table shows an "External Subs" column with green/yellow/red badges; red badges show an inline error strip under the row.
- [ ] Ollama connection failures show a friendly, actionable, localized message instead of the raw reqwest error; a Test Connection action exists in Settings.
- [ ] All new strings exist in both `en.json` and `ar.json`.
- [ ] Shared logic lives in a crate consumed by both `src-tauri` and `server/`, not duplicated.
