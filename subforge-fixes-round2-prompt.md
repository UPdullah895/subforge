# SubForge Update — Round 2 Fixes (Extraction, Merge, Rename, TVmaze Bug, Ollama)

## Context

Follow-up to the previous update (rename modal, auto-translate wizard, external subs detection, unified API settings, Ollama friendly-error fix). That work is mostly in place but testing surfaced 7 issues below. Same hard constraint as before: shared logic lives in the common Rust crate consumed by both `src-tauri` and `server/`, not duplicated per-backend.

---

## 1. Bug: extracted subtitle filenames collide across episodes

**Symptom:** batch-extracting subs currently produces a generic name like `3_eng.srt` (looks like it's derived from the *track index*, not the source video), so every episode in the batch writes to the same filename and overwrites the previous one.

**Fix:** the extraction output filename must be derived from the **source video's base name**, not the track index:

```
{video_basename}.{lang_code}.srt
```
e.g. `Among Us - S01E01.eng.srt`, `Among Us - S01E02.eng.srt`.

This also matters because it should match the exact naming convention the "External Subs" detection column (from the previous update) already scans for — align the two so subtitles this app extracts are automatically picked back up as "external subs" if re-scanned. Audit both code paths (extraction output naming + external-subs detection regex) and make sure they use the **same shared constant/function** for building/parsing this filename pattern, not two independently-maintained string templates.

If a video has multiple subtitle tracks in the same language (e.g. two English tracks, one SDH), append a disambiguator: `{video_basename}.{lang_code}.{n}.srt` only when a collision would otherwise occur, not by default.

---

## 2. Feature: language picker before extraction

**Current behavior:** extraction dumps every detected subtitle track. **Wanted:** a step where the user sees the detected tracks (language + any flags like forced/SDH/track title from the mkv) and picks which one(s) to actually extract — e.g. just English, when the goal is translation.

**UX:** when "Extract Subs" is pressed (batch or single-file), show a small selection step first:
- List of subtitle tracks detected **across the selected files** (union of languages found), as checkboxes, each showing language name + a count like "10/10 files have this track" so the user knows if a language is only present in some files.
- Default: nothing pre-checked, or pre-check the language matching the app's current UI/translation-target language as a sensible default — your call, just make it obvious rather than silently extracting everything.
- Confirm triggers extraction for only the checked language(s), applying the naming fix from section 1.

This can be a small step inside the existing Extract flow (e.g. a lightweight popover/modal before the actual extraction starts) rather than a whole new page.

---

## 3. Feature: missing "merge external subtitles" button

**Gap:** the External Subs column (from the previous update) detects external `.srt`/`.ass` files sitting next to videos and shows status badges, but there's currently no way to actually mux them into the video — only the auto-translate pipeline merges (and only for subs *it* generated). Add an explicit, standalone action for existing external subtitle files.

**Add:** a "Merge Subs" button in the Batch toolbar (next to Rename / Auto-Translate), operating on the currently selected files:
- For each selected file, look at its already-detected `externalSubs` list (green-status ones).
- If a file has more than one external sub, let the user pick which language(s) to merge in (checkbox list per file, or a shared "merge all detected" shortcut if they're all green).
- Call mkvmerge to mux the chosen `.srt`/`.ass` file(s) into the `.mkv`, setting the track language metadata from the filename's language code.
- On success, update that file's `externalSubs` entry status and the Subs count column; on failure, set status to error with the specific mkvmerge error message so it shows in the row's alert strip (per the existing red-badge/alert-strip pattern from the previous update).
- Add a "Delete original external file after merge" checkbox (default off) for users who want to clean up loose `.srt` files afterward.

---

## 4. Feature: refresh/rescan without re-picking the folder

**Problem:** if the user adds a new file to the folder they're already working in, they have to go through the full "Scan Directory" picker flow again to see it.

**Fix:** remember the last-scanned directory path (in the Pinia store / persisted setting), and add a small **refresh icon button** right next to "Scan Directory" in the toolbar that just re-runs the scan against the remembered path — no file dialog. Keep "Scan Directory" itself for picking a *different* folder. Disable/hide the refresh button if no directory has been scanned yet this session.

---

## 5. Feature: pattern-based batch naming in Manual rename mode

**Current:** Manual mode requires typing every new filename by hand, one input per row.

**Wanted:** let the user type **one naming pattern** and apply it to all selected rows at once, using the same token syntax as Auto mode (e.g. `{name} - S{season:00}E{episode:00}`), where `{name}` is a free-text field the user fills in once (e.g. "Among Us") and `{season}`/`{episode}` are parsed from each file's current name via the existing `parser.rs` logic — no need to hit an external API since there's no title lookup involved.

**UX:** in Manual mode, add a "Pattern" input + "Name" input above the per-row list, with a small "Apply to all" button. Applying it fills in every row's editable field with the computed result — but the per-row fields should **stay individually editable afterward**, so the user can tweak one-off exceptions without losing the bulk-generated base. Don't remove the manual per-row editing; the pattern is a fast starting point, not a replacement.

---

## 6. Bug: TVmaze auto-rename throws `missing required key seriesTitle`

```
invalid args `seriesTitle` for command `preview_rename`: command preview_rename missing required key seriesTitle
```

This means the frontend `invoke('preview_rename', {...})` call in the **TVmaze** branch of the rename flow isn't passing a `seriesTitle` argument that the Rust `preview_rename` command requires, while the **TMDB** branch presumably does pass it (since only TVmaze triggers this).

**Fix — audit both branches for parameter parity:**
- Find every call site that invokes `preview_rename` (or the equivalent Axum endpoint call) for both Auto — TMDB and Auto — TVmaze, and diff the argument objects being sent.
- Since TVmaze search results and TMDB search results likely come back as differently-shaped objects (different field names for the series title), make sure whichever object is selected from the TVmaze results list gets mapped to the same shared `SeriesMatch` shape (with a `title`/`seriesTitle` field populated) before being handed to the rename preview call — don't let two different provider response shapes leak into the shared command signature.
- Double check the Rust side too: if `preview_rename` takes a struct with `series_title: String` (or `seriesTitle` depending on your naming), make sure the field isn't `Option<String>` on one path and required on another by accident — it should be one consistent, always-required field regardless of provider.
- Add a quick manual test: run the exact same rename flow once via TMDB and once via TVmaze for the same batch of files, confirm both produce a preview successfully.

---

## 7. Bug: Ollama connection error still happening

```
HTTP request failed: error sending request for url (http://localhost:11434/v1/chat/completions)
```

This is the same error as before — the earlier fix (friendly error message + configurable base URL + Test Connection button) may not have actually been wired into the code path that fires during real translation jobs. Things to check:

1. **Grep the whole codebase for `localhost:11434`** (or `11434` generally). If the Ollama base URL is hardcoded in more than one place (e.g. the Test Connection check reads from Settings, but the actual translation-pipeline call still uses a hardcoded default instead of the saved setting), that mismatch would explain why fixing/testing the setting doesn't actually change behavior. All Ollama calls — test connection, model listing, and the real translate/completions call — must read from the **same single source of truth** (the saved provider config).
2. Confirm the friendly-error catch (`is_connect()` check from the previous fix) actually wraps the `/v1/chat/completions` call specifically, not just the test-connection call. If it was only added to the Test button's code path, the real translation call would still surface the raw reqwest error exactly like this.
3. As a basic runtime check (not a code fix, just to isolate the bug): confirm Ollama is actually running (`ollama serve` / check `http://localhost:11434/api/tags` responds) while reproducing this, to rule out "it's simply not running" as the whole story before assuming it's purely a config-wiring bug.
4. Once fixed, the user-facing result on a real connection failure during translation should be the friendly localized message from the previous prompt, shown per-file in the translate wizard's progress list (per-file error display was already speced there) — not a modal-breaking raw error.

---

## Acceptance checklist (round 2)

- [ ] Batch subtitle extraction produces uniquely-named files per episode; extraction and external-subs detection share one naming convention function.
- [ ] Extraction shows a language picker (based on detected tracks) before running, instead of extracting everything.
- [ ] A "Merge Subs" button exists and mux es detected external subtitle files into their videos, with per-file language selection and error surfacing via the existing alert-strip pattern.
- [ ] A refresh button re-scans the last-used directory without reopening the file picker.
- [ ] Manual rename mode supports a pattern applied to all rows at once, while still allowing per-row overrides afterward.
- [ ] Auto — TVmaze rename works end-to-end without the `seriesTitle` argument error; TMDB and TVmaze both go through one shared, consistently-shaped `SeriesMatch` object.
- [ ] Ollama base URL is read from one single saved config value everywhere it's used (test, translate, model list); the friendly connection-error message actually appears during real translation failures, not just in the Settings test button.
