import { z } from "zod";

// Rust `Option<T>` serializes as `null`, not as a missing key, so every
// optional field is `.nullish()`. Objects are `looseObject` so fields the
// schema doesn't list (e.g. `external_subs`, API keys, `language`) pass
// through instead of being silently stripped by `parse`.

const ToolInfoSchema = z.looseObject({
  name: z.string(),
  available: z.boolean(),
  path: z.string().nullish(),
  version: z.string().nullish(),
});

const DepStatusSchema = z.looseObject({
  id: z.string(),
  label: z.string(),
  severity: z.enum(["required", "optional"]),
  state: z.enum(["ok", "missing", "outdated", "invalid"]),
  source: z.enum(["custom", "bundled", "system"]).nullish(),
  error: z.string().nullish(),
  path: z.string().nullish(),
  version: z.string().nullish(),
  purpose: z.string(),
  download_url: z.string(),
  install_command: z.string().nullish(),
  can_auto_install: z.boolean(),
});

const DepReportSchema = z.looseObject({
  os: z.string(),
  items: z.array(DepStatusSchema),
  has_missing_required: z.boolean(),
  has_invalid_custom: z.boolean().optional(),
});

export const commandSchemas: Record<string, z.ZodTypeAny> = {
  enumerate_tracks: z.array(
    z.looseObject({
      id: z.number(),
      track_type: z.string(),
      codec: z.string(),
      language: z.string(),
      track_name: z.string().nullish(),
      default_track: z.boolean(),
      forced_track: z.boolean(),
    })
  ),

  list_audio_tracks: z.array(
    z.looseObject({
      id: z.number(),
      codec: z.string(),
      codec_long_name: z.string().nullish(),
      language: z.string(),
      title: z.string().nullish(),
    })
  ),

  detect_ollama: z.looseObject({
    available: z.boolean(),
    models: z.array(z.string()),
  }),

  detect_libretranslate: z.looseObject({
    available: z.boolean(),
    languages: z.array(z.looseObject({ code: z.string(), name: z.string() })),
  }),

  get_whisper_info: z.looseObject({
    engine: z.string(),
    gpu: z.string().nullish(),
    devices: z.array(z.string()),
  }),

  list_whisper_models: z.array(z.string()),

  list_whisper_catalog: z.array(z.looseObject({ name: z.string(), size_mb: z.number() })),

  download_whisper_model: z.string(),

  whisper_legacy_cache: z.string().nullish(),

  get_tool_registry: z.looseObject({
    mkvmerge: ToolInfoSchema,
    mkvextract: ToolInfoSchema,
    ffmpeg: ToolInfoSchema,
    ffprobe: ToolInfoSchema,
  }),

  get_tool_status: z.array(
    z.looseObject({
      id: z.string(),
      source: z.enum(["custom", "bundled", "system"]).nullable(),
      path: z.string().nullish(),
      version: z.string().nullish(),
      error: z.string().nullish(),
    })
  ),

  check_dependencies: DepReportSchema,
  install_dependency: DepReportSchema,

  load_config: z.looseObject({
    theme: z.string(),
    mkvmerge_path: z.string().nullish(),
    mkvextract_path: z.string().nullish(),
    mkvpropedit_path: z.string().nullish(),
    config_version: z.number().nullish(),
    ffmpeg_path: z.string().nullish(),
    ffprobe_path: z.string().nullish(),
    default_engine: z.string().nullish(),
    default_source_lang: z.string().nullish(),
    default_target_lang: z.string().nullish(),
    default_output_dir: z.string().nullish(),
    search_online_before_translate: z.boolean().nullish(),
  }),

  scan_directory: z.array(
    z.looseObject({
      path: z.string(),
      name: z.string(),
      tracks: z.array(
        z.looseObject({
          id: z.number(),
          track_type: z.string(),
          codec: z.string(),
          properties: z
            .looseObject({
              language: z.string().nullish(),
              track_name: z.string().nullish(),
            })
            .nullish(),
        })
      ),
      subtitle_count: z.number(),
      audio_count: z.number(),
      video_count: z.number(),
    })
  ),

  batch_extract_subtitles: z.array(z.string()),

  check_subtitle_sync: z.looseObject({
    check: z.looseObject({
      verdict: z.enum(["good", "offset", "framerate", "truncated", "uncertain"]),
      cue_count: z.number(),
      subtitle_start_ms: z.number(),
      subtitle_end_ms: z.number(),
      video_duration_ms: z.number().nullish(),
      video_fps: z.number().nullish(),
      suggested_fix: z.looseObject({ from: z.number(), to: z.number() }).nullish(),
      suggested_shift_ms: z.number().nullish(),
      issues: z.array(z.looseObject({ kind: z.string(), message: z.string() })),
    }),
    deep: z
      .looseObject({
        verdict: z.enum(["good", "offset", "framerate", "truncated", "uncertain"]),
        matches: z.array(
          z.looseObject({
            subtitle_start_ms: z.number(),
            asr_start_ms: z.number(),
            offset_ms: z.number(),
            score: z.number(),
            subtitle_text: z.string(),
          })
        ),
        offset_ms: z.number().nullish(),
        suggested_shift_ms: z.number().nullish(),
        suggested_fix: z.looseObject({ from: z.number(), to: z.number() }).nullish(),
        reason: z.string(),
      })
      .nullish(),
    model: z.string().nullish(),
  }),

  fix_subtitle_sync: z.looseObject({
    output_path: z.string(),
    backup_path: z.string().nullish(),
    cues: z.number(),
  }),

  clean_subtitle: z.looseObject({
    removed: z.array(
      z.looseObject({
        index: z.number(),
        start_ms: z.number(),
        end_ms: z.number(),
        text: z.string(),
        rule: z.string(),
      })
    ),
    kept: z.number(),
    output_path: z.string().nullish(),
    backup_path: z.string().nullish(),
  }),

  get_opensubtitles_status: z.looseObject({
    configured: z.boolean(),
    username: z.string().nullish(),
    api_key: z.string().nullish(),
    password_set: z.boolean().nullish(),
  }),

  check_subsource_status: z.looseObject({
    configured: z.boolean(),
    reachable: z.boolean(),
    reason: z.enum(["ok", "rate_limited", "invalid_key", "unreachable", "not_configured"]),
  }),

  search_online_subtitle: z.discriminatedUnion("status", [
    z.looseObject({
      status: z.literal("found"),
      subtitle_path: z.string(),
      source: z.string(),
      matched_via: z.string(),
      sync_verdict: z.string().nullish(),
    }),
    z.looseObject({ status: z.literal("not_found") }),
  ]),
};
