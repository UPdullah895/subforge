import { defineStore } from "pinia";
import { apiInvoke } from "../api/client";
import { ref } from "vue";


export interface WhisperInfo {
  faster_whisper_version: string;
  cuda_available: boolean;
  devices: string[];
}

export interface TranscribeJob {
  id: string;
  status: "pending" | "running" | "completed" | "failed";
  input_file: string;
  progress: number;
  message?: string;
}

export const useTranscribeStore = defineStore("transcribe", () => {
  const whisperInfo = ref<WhisperInfo | null>(null);
  const downloadedModels = ref<string[]>([]);
  const loadingInfo = ref(false);
  const downloadingModel = ref<string | null>(null);
  const jobs = ref<TranscribeJob[]>([]);

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
    try {
      whisperInfo.value = await apiInvoke<WhisperInfo>("get_whisper_info");
    } catch (e) {
      console.error("Failed to load whisper info:", e);
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
      console.error("Failed to list models:", e);
    }
  }

  async function downloadModel(name: string) {
    downloadingModel.value = name;
    try {
      await apiInvoke("download_whisper_model", {
        model: name,
        cacheDir: null,
      });
      await loadModels();
    } catch (e) {
      console.error("Failed to download model:", e);
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

      jobs.value.push({
        id: jobId,
        status: "pending",
        input_file: options.input,
        progress: 0,
      });
    } catch (e) {
      console.error("Failed to start transcription:", e);
    }
  }

  function updateJobProgress(jobId: string, progress: number, message?: string) {
    const job = jobs.value.find((j) => j.id === jobId);
    if (job) {
      job.progress = progress;
      job.status = "running";
      if (message) job.message = message;
    }
  }

  function completeJob(jobId: string, status: string, message?: string) {
    const job = jobs.value.find((j) => j.id === jobId);
    if (job) {
      job.status = status as "completed" | "failed";
      job.progress = 100;
      if (message) job.message = message;
    }
  }

  function clearCompleted() {
    jobs.value = jobs.value.filter((j) => j.status === "running" || j.status === "pending");
  }

  return {
    whisperInfo,
    downloadedModels,
    loadingInfo,
    downloadingModel,
    jobs,
    allModels,
    loadInfo,
    loadModels,
    downloadModel,
    startTranscription,
    updateJobProgress,
    completeJob,
    clearCompleted,
  };
});
