import { z } from "zod";

const ToolInfoSchema = z.object({
  name: z.string(),
  available: z.boolean(),
  path: z.string().optional(),
  version: z.string().optional(),
});

export const commandSchemas: Record<string, z.ZodTypeAny> = {
  enumerate_tracks: z.array(
    z.object({
      id: z.number(),
      track_type: z.string(),
      codec: z.string(),
      language: z.string(),
      track_name: z.string().optional(),
      default_track: z.boolean(),
      forced_track: z.boolean(),
    })
  ),

  list_audio_tracks: z.array(
    z.object({
      id: z.number(),
      codec: z.string(),
      codec_long_name: z.string().optional(),
      language: z.string(),
      title: z.string().optional(),
    })
  ),

  detect_ollama: z.object({
    available: z.boolean(),
    models: z.array(z.string()),
  }),

  detect_libretranslate: z.object({
    available: z.boolean(),
    languages: z.array(z.object({ code: z.string(), name: z.string() })),
  }),

  get_whisper_info: z.object({
    faster_whisper_version: z.string(),
    cuda_available: z.boolean(),
    devices: z.array(z.string()),
  }),

  list_whisper_models: z.array(z.string()),

  get_tool_registry: z.object({
    mkvmerge: ToolInfoSchema,
    mkvextract: ToolInfoSchema,
    ffmpeg: ToolInfoSchema,
    ffprobe: ToolInfoSchema,
  }),

  load_config: z.object({
    theme: z.string(),
    mkvmerge_path: z.string().optional(),
    mkvextract_path: z.string().optional(),
    ffmpeg_path: z.string().optional(),
    ffprobe_path: z.string().optional(),
    default_engine: z.string().optional(),
    default_source_lang: z.string().optional(),
    default_target_lang: z.string().optional(),
    default_output_dir: z.string().optional(),
  }),

  scan_directory: z.array(
    z.object({
      path: z.string(),
      name: z.string(),
      tracks: z.array(
        z.object({
          id: z.number(),
          track_type: z.string(),
          codec: z.string(),
          properties: z
            .object({
              language: z.string().optional(),
              track_name: z.string().optional(),
            })
            .optional(),
        })
      ),
      subtitle_count: z.number(),
      audio_count: z.number(),
      video_count: z.number(),
    })
  ),

  batch_extract_subtitles: z.array(z.string()),
};
