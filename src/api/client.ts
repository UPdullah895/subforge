import { invoke } from "@tauri-apps/api/core";
import { commandSchemas } from "./schemas";
import { ApiError, toApiError } from "./errors";

// `__TAURI_INTERNALS__` is injected by every Tauri v2 webview regardless of
// the `withGlobalTauri` config flag (which only controls `__TAURI__`), so
// it's the reliable marker for "are we actually running inside Tauri, or is
// this the same frontend served plain by the Axum server in a browser".
export function isTauri(): boolean {
  return typeof window !== "undefined" && !!(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
}

export async function apiInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri()) {
    try {
      return await invoke<T>(cmd, args);
    } catch (e) {
      throw toApiError(e);
    }
  }

  // Map command to REST endpoint
  const endpoint = cmdToEndpoint(cmd);
  const resp = GET_COMMANDS.has(cmd)
    ? await fetch(endpoint)
    : await fetch(endpoint, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(restBody(cmd, args)),
      });

  if (!resp.ok) throw await responseError(resp);

  const data = await readBody(resp);
  const schema = commandSchemas[cmd];
  if (!schema) return data as T;
  const parsed = schema.safeParse(data);
  if (!parsed.success) {
    // Keep the detail for debugging; show people something they can act on.
    console.warn(`Unexpected response shape from ${cmd}`, parsed.error.issues);
    throw new Error(`The server sent an unexpected response for "${cmd}". Please update SubForge or report this.`);
  }
  return parsed.data as T;
}

// Tauri commands take either named camelCase args (`{ filePath }`) or one
// struct under a wrapper key (`{ request }`, `{ options }`, `{ config }`,
// `{ credentials }`); the REST handlers expect the bare snake_case payload as
// the body. A single plain-object argument is therefore always a wrapper.
function restBody(cmd: string, args?: Record<string, unknown>): unknown {
  if (!args || KEEP_WRAPPED.has(cmd)) return args;
  const entries = Object.entries(args);
  if (entries.length === 1) {
    const [, value] = entries[0];
    if (value !== null && typeof value === "object" && !Array.isArray(value)) return value;
  }
  return Object.fromEntries(
    entries.map(([k, v]) => [k.replace(/[A-Z]/g, (c) => `_${c.toLowerCase()}`), v])
  );
}

// REST handlers whose body really is `{ <wrapper>: {...} }`.
const KEEP_WRAPPED = new Set(["set_opensubtitles_credentials"]);

// Commands whose REST route is a GET (the POST on the same path is "save").
const GET_COMMANDS = new Set(["load_config", "get_tool_registry", "get_tool_status", "check_dependencies"]);

// Many handlers answer a bare `200 OK` with no body (the Tauri equivalents
// return `()`); treat an empty body as "no value" instead of a JSON error.
async function readBody(resp: Response): Promise<unknown> {
  const text = await resp.text();
  return text.trim() === "" ? undefined : JSON.parse(text);
}

// Handlers answer with `{"error": "...", "details"?: "..."}`, a plain-text body,
// or just a status code; reduce all three to a sentence (plus raw details).
async function responseError(resp: Response): Promise<ApiError> {
  const text = (await resp.text()).trim();
  if (text) {
    try {
      const body = JSON.parse(text) as { error?: unknown; details?: unknown };
      if (typeof body.error === "string" && body.error) {
        return new ApiError(body.error, typeof body.details === "string" ? body.details : undefined);
      }
    } catch {
      return new ApiError(text);
    }
  }
  const reasons: Record<number, string> = {
    400: "The request was rejected (bad input or unreadable file).",
    404: "Not found.",
    422: "Some of the information sent was invalid.",
    500: "The server hit an error while processing this. Check that the file exists and the tools are installed.",
    502: "A remote service could not be reached.",
    503: "A required tool is unavailable.",
  };
  return new ApiError(reasons[resp.status] ?? `Request failed (HTTP ${resp.status}).`);
}

export async function apiGet<T>(cmd: string, params?: Record<string, string>): Promise<T> {
  if (isTauri()) {
    try {
      return await invoke<T>(cmd, params);
    } catch (e) {
      throw toApiError(e);
    }
  }

  const endpoint = cmdToEndpoint(cmd);
  const query = params ? "?" + new URLSearchParams(params).toString() : "";
  const resp = await fetch(endpoint + query);

  if (!resp.ok) throw await responseError(resp);

  return (await readBody(resp)) as T;
}

function cmdToEndpoint(cmd: string): string {
  // Map Tauri command names to REST paths
  const map: Record<string, string> = {
    enumerate_tracks: "/api/subtitle/enumerate",
    extract_subtitle: "/api/subtitle/extract",
    extract_all_subtitles: "/api/subtitle/extract-all",
    set_track_metadata: "/api/subtitle/meta",
    merge_subtitle_into_video: "/api/subtitle/merge",
    remove_subtitle_tracks: "/api/subtitle/remove",
    convert_subtitle_format: "/api/subtitle/convert",
    shift_subtitle_timing: "/api/subtitle/shift",
    stretch_subtitle_timing: "/api/subtitle/stretch",
    clean_subtitle: "/api/subtitle/clean",
    check_subtitle_sync: "/api/subtitle/check-sync",
    fix_subtitle_sync: "/api/subtitle/fix-sync",
    list_audio_tracks: "/api/audio/tracks",
    extract_audio_track: "/api/audio/extract",
    merge_audio_track: "/api/audio/merge",
    remove_audio_tracks: "/api/audio/remove",
    scan_directory: "/api/batch/scan",
    batch_extract_subtitles: "/api/batch/extract",
    merge_external_subs: "/api/batch/merge-subs",
    get_whisper_info: "/api/transcribe/info",
    list_whisper_models: "/api/transcribe/models",
    download_whisper_model: "/api/transcribe/download",
    list_whisper_catalog: "/api/transcribe/catalog",
    whisper_legacy_cache: "/api/transcribe/legacy-cache",
    delete_whisper_legacy_cache: "/api/transcribe/legacy-cache/delete",
    transcribe_audio: "/api/transcribe",
    detect_ollama: "/api/translate/detect/ollama",
    detect_libretranslate: "/api/translate/detect/libretranslate",
    translate_subtitles: "/api/translate",
    run_translation_pipeline: "/api/translate/pipeline",
    get_tool_registry: "/api/tools/registry",
    get_tool_status: "/api/tools/status",
    check_dependencies: "/api/deps",
    install_dependency: "/api/deps/install",
    load_config: "/api/config",
    save_config: "/api/config",
    parse_filename: "/api/rename/parse",
    search_series: "/api/rename/search",
    get_episodes: "/api/rename/episodes",
    preview_rename: "/api/rename/preview",
    apply_rename: "/api/rename/apply",
    test_provider_config: "/api/providers/test",
    cancel_job: "/api/jobs/cancel",
    pause_job: "/api/jobs/pause",
    resume_job: "/api/jobs/resume",
    search_online_subtitle: "/api/subtitle-sources/search",
    set_opensubtitles_credentials: "/api/subtitle-sources/opensubtitles/credentials",
    clear_opensubtitles_credentials: "/api/subtitle-sources/opensubtitles/clear",
    get_opensubtitles_status: "/api/subtitle-sources/opensubtitles/status",
    check_subsource_status: "/api/subtitle-sources/subsource/status",
  };
  return map[cmd] || `/api/${cmd.replace(/_/g, "/")}`;
}
