import { defineStore } from "pinia";
import { apiInvoke } from "../api/client";
import { ref } from "vue";
import { useJobQueue } from "../composables/useJobQueue";

export interface WhisperInfo {
  faster_whisper_version: string;
  cuda_available: boolean;
  devices: string[];
}

export const useTranscribeStore = defineStore("transcribe", () => {
  const whisperInfo = ref<WhisperInfo | null>(null);
  const downloadedModels = ref<string[]>([]);
  const loadingInfo = ref(false);
  const downloadingModel = ref<string | null>(null);
  const error = ref<string | null>(null);
  const { jobs, addJob, updateJobProgress, completeJob, clearCompleted, removeJob } = useJobQueue();

  const allModels = [
    "tiny",
    "tiny.en",
    "base",
    "base.en",
    "small",
    "small.en",
    "medium",
    "medium.en",
    "large-v1",
    "large-v2",
    "large-v3",
  ];

  async function loadInfo() {
    loadingInfo.value = true;
    error.value = null;
    try {
      whisperInfo.value = await apiInvoke<WhisperInfo>("get_whisper_info");
    } catch (e) {
      error.value = `Failed to load Whisper info: ${e instanceof Error ? e.message : String(e)}`;
    } finally {
      loadingInfo.value = false;
    }
  }

  async function loadModels() {
    try {
      downloadedModels.value = await apiInvoke<string[]>("list_whisper_models", {
        cacheDir: null,
      });
    } catch (e) {
      error.value = `Failed to list models: ${e instanceof Error ? e.message : String(e)}`;
    }
  }

  async function downloadModel(name: string) {
    downloadingModel.value = name;
    error.value = null;
    try {
      await apiInvoke("download_whisper_model", {
        model: name,
        cacheDir: null,
      });
      await loadModels();
    } catch (e) {
      error.value = `Failed to download model: ${e instanceof Error ? e.message : String(e)}`;
    } finally {
      downloadingModel.value = null;
    }
  }

  async function startTranscription(options: {
    input: string;
    output: string;
    model: string;
    device: string;
    language?: string;
    format: string;
    wordTimestamps: boolean;
    vadFilter: boolean;
  }) {
    error.value = null;
    try {
      const jobId = await apiInvoke<string>("transcribe_audio", {
        request: {
          input: options.input,
          output: options.output,
          model: options.model,
          device: options.device,
          language: options.language || null,
          format: options.format,
          word_timestamps: options.wordTimestamps,
          vad_filter: options.vadFilter,
          cache_dir: null,
        },
      });

      addJob(jobId, options.input);
    } catch (e) {
      error.value = `Failed to start transcription: ${e instanceof Error ? e.message : String(e)}`;
    }
  }

  return {
    whisperInfo,
    downloadedModels,
    loadingInfo,
    downloadingModel,
    error,
    jobs,
    allModels,
    loadInfo,
    loadModels,
    downloadModel,
    startTranscription,
    updateJobProgress,
    completeJob,
    clearCompleted,
    removeJob,
  };
});
