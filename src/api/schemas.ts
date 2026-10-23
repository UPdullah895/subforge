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
  state: z.enum(["ok", "missing", "outdated"]),
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

  get_opensubtitles_status: z.looseObject({ configured: z.boolean() }),

  check_subsource_status: z.looseObject({ reachable: z.boolean() }),

  search_online_subtitle: z.discriminatedUnion("status", [
    z.looseObject({
      status: z.literal("found"),
      subtitle_path: z.string(),
      source: z.string(),
      matched_via: z.string(),
    }),
    z.looseObject({ status: z.literal("not_found") }),
  ]),
};
