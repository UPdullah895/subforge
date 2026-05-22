import { invoke } from "@tauri-apps/api/core";
import { commandSchemas } from "./schemas";

const isTauri =
  typeof window !== "undefined" &&
  !!(window as unknown as Record<string, unknown>).__TAURI__;

export async function apiInvoke<T>(
  cmd: string,
  args?: Record<string, unknown>
): Promise<T> {
  if (isTauri) {
    return invoke<T>(cmd, args);
  }

  // Map command to REST endpoint
  const endpoint = cmdToEndpoint(cmd);
  const resp = await fetch(endpoint, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(args?.request ?? args),
  });

  if (!resp.ok) {
    const text = await resp.text();
    throw new Error(text || `HTTP ${resp.status}`);
  }

  const data = await resp.json();
  const schema = commandSchemas[cmd];
  return schema ? (schema.parse(data) as T) : (data as T);
}

export async function apiGet<T>(cmd: string, params?: Record<string, string>): Promise<T> {
  if (isTauri) {
    return invoke<T>(cmd, params);
  }

  const endpoint = cmdToEndpoint(cmd);
  const query = params ? "?" + new URLSearchParams(params).toString() : "";
  const resp = await fetch(endpoint + query);

  if (!resp.ok) {
    const text = await resp.text();
    throw new Error(text || `HTTP ${resp.status}`);
  }

  return resp.json();
}

function cmdToEndpoint(cmd: string): string {
  // Map Tauri command names to REST paths
  const map: Record<string, string> = {
    enumerate_tracks: "/api/subtitle/enumerate",
    extract_subtitle: "/api/subtitle/extract",
    merge_subtitle_into_video: "/api/subtitle/merge",
    remove_subtitle_tracks: "/api/subtitle/remove",
    convert_subtitle_format: "/api/subtitle/convert",
    shift_subtitle_timing: "/api/subtitle/shift",
    list_audio_tracks: "/api/audio/tracks",
    extract_audio_track: "/api/audio/extract",
    merge_audio_track: "/api/audio/merge",
    remove_audio_tracks: "/api/audio/remove",
    scan_directory: "/api/batch/scan",
    get_whisper_info: "/api/transcribe/info",
    list_whisper_models: "/api/transcribe/models",
    download_whisper_model: "/api/transcribe/download",
    transcribe_audio: "/api/transcribe",
    detect_ollama: "/api/translate/detect/ollama",
    detect_libretranslate: "/api/translate/detect/libretranslate",
    translate_subtitles: "/api/translate",
    get_tool_registry: "/api/health",
    load_config: "/api/health",
    save_config: "/api/health",
  };
  return map[cmd] || `/api/${cmd.replace(/_/g, "/")}`;
}
