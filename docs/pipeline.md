# Library automation: `pipeline.json`

SubForge can watch a library and finish it by itself: find or produce a subtitle, translate it,
check it against a readability rule-set, mux it into the episode, verify the track, and only then
remove the sidecar. This page is the configuration reference and the two-minute setup.

Everything below is off until a policy file exists. A fresh server plans nothing.

## The file

`<config dir>/pipeline.json` — on Linux `~/.config/subforge/pipeline.json`, in Docker
`/config/pipeline.json`. `SUBFORGE_PIPELINE_CONFIG` points somewhere else.

```json
{
  "watch_dirs": ["/media/tv", "/media/movies"],
  "recursive": true,
  "policy": {
    "target_languages": ["ar"],
    "prefer_human_subtitle": true,
    "translate_engine": "ollama",
    "delete_source_after_mux": false,
    "dry_run": true,
    "rule_profile": "netflix_adult"
  },
  "arr": [
    { "kind": "sonarr", "url": "http://192.168.3.5:8989", "api_key": "…" }
  ]
}
```

| Field | Meaning |
|---|---|
| `watch_dirs` | Folders to watch. Empty is fine when `arr` is configured. |
| `recursive` | Descend into `Series/Season 01/`. Default `true`. |
| `policy.target_languages` | Languages the library should end up with. Empty = nothing to do. |
| `policy.prefer_human_subtitle` | Search the providers before machine-translating. Default `true`. |
| `policy.translate_engine` | `ollama`, `openai`, `libretranslate`, `deepl`, `google`. Unset = the saved config's engine, then Ollama. |
| `policy.delete_source_after_mux` | Remove the external subtitle **after** a verified mux. Default `false`. |
| `policy.dry_run` | Log the plan, touch nothing. Default `true` — so a new library is never modified before you have read a plan. |
| `policy.rule_profile` | `off` (default), `netflix_adult`, `netflix_kids`, `bbc`. |
| `arr[]` | Sonarr/Radarr instances to follow over their SignalR feed. The API key is masked in `/api/pipeline/config`. |

## Three ways a file gets processed

**1. The \*arr feed (recommended).** With an `arr` entry, the server opens one outbound WebSocket
per instance to `<url>/signalr/messages?access_token=<key>`. On any episode/movie change it asks the
*arr's REST history what was imported and processes those paths. Nothing to configure on the *arr
side, and events that arrive while the server is down are picked up by the history sweep on
reconnect.

**2. A webhook.** In Sonarr/Radarr: *Settings → Connect → + → Webhook*, URL
`http://<subforge-host>:8080/api/hooks/sonarr` (or `…/radarr`), method `POST`, and tick
*On Import* / *On Import Complete* / *On Upgrade*. This is the cheapest path: the payload carries
`mediaInfo.subtitles`, so an episode that already has the language is acknowledged and skipped
without touching a file or a GPU.

**3. The folder watcher.** With `watch_dirs` set, new files are noticed after they stop changing for
three seconds. Partial downloads (`*.part`, `*.tmp`, `*.crdownload`, `*.!qb`, dotfiles) are ignored.

## What a run looks like

```
GET  /api/pipeline/config        → the policy, the watch folders, the *arr instances (key masked)
POST /api/pipeline/plan {"file": "…"}  → the ordered steps for one file, without running them
GET  /api/jobs                   → every job with class, source, stage, status and message
```

A step list for an episode that has English and needs Arabic, with a rule profile set:

```
search:ar → translate:ar → lint:ar → mux:ar → verify:ar
```

An episode with **no** subtitle track at all starts with `transcribe` instead of `search`, and the
transcript feeds the translate step.

A job that fails reports why and leaves the file alone: muxing is atomic (temporary beside the
episode, original moved aside, verified, then either the original is restored or the backup is
deleted), so an episode is never left half-rewritten.

## Environment

| Variable | Used for |
|---|---|
| `SUBFORGE_CONFIG_DIR` | Where `pipeline.json`, `config.json` and `pipeline-work/` live. |
| `SUBFORGE_HOST` | Listen address, e.g. `0.0.0.0:8080` in Docker. |
| `SUBFORGE_PIPELINE_CONFIG` | A policy file somewhere else. |
| `SUBFORGE_OLLAMA_MODEL` | The model the `ollama` engine uses (the config's `openai_model` is the fallback). |
| `SUBFORGE_WHISPER_MODEL` | Whisper model name; otherwise the largest model already downloaded is used. |
| `SUBFORGE_BUNDLED_DIR` | Where `mkvmerge`/`ffmpeg` live, when they are not on `PATH`. |
| `SUBFORGE_MAX_HEAVY_JOBS` | How many transcription/translation jobs may run at once. Default `1`. |
| `SUBFORGE_MAX_LIGHT_JOBS` | The same for cheap jobs (extract, merge, shift). Default `2`. |

## Docker

```yaml
services:
  subforge:
    image: ghcr.io/updullah895/subforge-server:latest
    environment:
      SUBFORGE_HOST: 0.0.0.0:8080
      SUBFORGE_OLLAMA_MODEL: gemma3:4b
      PUID: 1000
      PGID: 1000
    volumes:
      - ./config:/config          # pipeline.json, config.json, pipeline-work/
      - /mnt/media:/media:rw      # the library SubForge is allowed to change
    ports: ["8080:8080"]
    restart: unless-stopped
```

`OLLAMA_HOST` (or the host in `config.json`) must point at an Ollama that the container can reach —
`http://host.docker.internal:11434` is the usual answer on Linux with an extra host entry.

## Turning it on safely

1. Write the policy with `"dry_run": true` and `"target_languages": ["ar"]`.
2. Start the server and read the log (or `POST /api/pipeline/plan` for one file). You will see lines
   like `needs 1 language(s); plan: search:ar -> translate:ar -> mux:ar -> verify:ar`.
3. When the plans look right, set `"dry_run": false`. The first live run is the moment to have a
   backup of one episode, not of the library.

## Notes that save an evening

- **A GPU out of memory** is reported as `out_of_memory` with the model name and the megabytes it
  asked for. Close other GPU applications, pick a 4B model, or run the engine on the CPU.
- **A model that collapses cues** (some small "translation" models merge several lines into one)
  shows up as an unparseable-response error. A general instruction-following model is the safer
  choice for subtitles.
- **No speech detected** means the transcription produced nothing — a music-only file, a silent
  track, or the wrong audio stream. Nothing is muxed in that case.
