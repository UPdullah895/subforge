import { defineStore } from "pinia";
import { apiInvoke } from "../api/client";
import { startAndWait } from "../api/jobs";
import { computed, ref } from "vue";
import { useJobQueue } from "../composables/useJobQueue";

/** Built-in whisper.cpp engine, as reported by `get_whisper_info`. */
export interface WhisperInfo {
  engine: string;
  /** GPU device name when a usable GPU was detected. */
  gpu: string | null;
  devices: string[];
}

export interface CatalogModel {
  name: string;
  size_mb: number;
}

export interface ModelDownload {
  model: string;
  jobId: string | null;
  progress: number;
}

export interface DeviceOption {
  value: "auto" | "cpu" | "gpu";
  /** Set for the `gpu` entry: the name to show next to it. */
  gpuName: string | null;
}

const LEGACY_NOTE_KEY = "subforge.whisperLegacyNoteDismissed";

function readDismissed(): boolean {
  try {
    return localStorage.getItem(LEGACY_NOTE_KEY) === "1";
  } catch {
    return false;
  }
}

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export const useTranscribeStore = defineStore("transcribe", () => {
  const whisperInfo = ref<WhisperInfo | null>(null);
  // True when the engine could not be queried at all (e.g. the server is unreachable).
  const whisperMissing = ref(false);
  const catalog = ref<CatalogModel[]>([]);
  const downloadedModels = ref<string[]>([]);
  const loadingInfo = ref(false);
  const download = ref<ModelDownload | null>(null);
  const legacyCache = ref<string | null>(null);
  const legacyNoteDismissed = ref(readDismissed());
  const error = ref<string | null>(null);
  const { jobs, addJob, updateJobProgress, completeJob, clearCompleted, removeJob, cancelJob } = useJobQueue();

  const allModels = computed(() => catalog.value.map((m) => m.name));
  const downloadingModel = computed(() => download.value?.model ?? null);
  const showLegacyNote = computed(() => legacyCache.value !== null && !legacyNoteDismissed.value);

  const deviceOptions = computed<DeviceOption[]>(() => {
    const devices = whisperInfo.value?.devices ?? ["auto", "cpu"];
    const options: DeviceOption[] = [];
    for (const d of ["auto", "cpu"] as const) {
      if (devices.includes(d)) options.push({ value: d, gpuName: null });
    }
    const gpu = whisperInfo.value?.gpu;
    if (devices.includes("gpu") && gpu) options.push({ value: "gpu", gpuName: gpu });
    return options;
  });

  async function loadInfo() {
    loadingInfo.value = true;
    whisperMissing.value = false;
    try {
      whisperInfo.value = await apiInvoke<WhisperInfo>("get_whisper_info");
    } catch (e) {
      whisperInfo.value = null;
      whisperMissing.value = true;
      console.warn("Whisper info unavailable", e);
    } finally {
      loadingInfo.value = false;
    }
  }

  async function loadCatalog() {
    try {
      catalog.value = await apiInvoke<CatalogModel[]>("list_whisper_catalog");
    } catch (e) {
      error.value = `Failed to list models: ${errorText(e)}`;
    }
  }

  async function loadModels() {
    try {
      downloadedModels.value = await apiInvoke<string[]>("list_whisper_models", {
        cacheDir: null,
      });
    } catch (e) {
      error.value = `Failed to list models: ${errorText(e)}`;
    }
  }

  /** Downloads `name` as a background job, tracking its progress until it ends. */
  async function downloadModel(name: string) {
    if (download.value) return;
    download.value = { model: name, jobId: null, progress: 0 };
    error.value = null;
    try {
      const result = await startAndWait(
        () => apiInvoke<string>("download_whisper_model", { model: name, cacheDir: null }),
        {
          onStarted: (jobId) => {
            if (download.value) download.value.jobId = jobId;
          },
          onProgress: (pct) => {
            if (download.value) download.value.progress = pct;
          },
        }
      );
      if (result.status === "failed") {
        error.value = `Failed to download model: ${result.message ?? "unknown error"}`;
        if (result.details) console.warn("Model download details:", result.details);
      }
      await loadModels();
    } catch (e) {
      error.value = `Failed to download model: ${errorText(e)}`;
    } finally {
      download.value = null;
    }
  }

  async function cancelDownload() {
    const jobId = download.value?.jobId;
    if (jobId) await cancelJob(jobId);
  }

  async function loadLegacyCache() {
    try {
      legacyCache.value = (await apiInvoke<string | null>("whisper_legacy_cache")) ?? null;
    } catch (e) {
      legacyCache.value = null;
      console.warn("Legacy model cache check failed", e);
    }
  }

  async function deleteLegacyCache() {
    try {
      await apiInvoke("delete_whisper_legacy_cache");
      legacyCache.value = null;
    } catch (e) {
      error.value = errorText(e);
    }
  }

  function dismissLegacyNote() {
    legacyNoteDismissed.value = true;
    try {
      localStorage.setItem(LEGACY_NOTE_KEY, "1");
    } catch {
      /* storage unavailable: the note stays hidden for this session only */
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
      error.value = `Failed to start transcription: ${errorText(e)}`;
    }
  }

  return {
    whisperInfo,
    whisperMissing,
    catalog,
    downloadedModels,
    loadingInfo,
    download,
    downloadingModel,
    deviceOptions,
    legacyCache,
    showLegacyNote,
    error,
    jobs,
    allModels,
    loadInfo,
    loadCatalog,
    loadModels,
    downloadModel,
    cancelDownload,
    loadLegacyCache,
    deleteLegacyCache,
    dismissLegacyNote,
    startTranscription,
    updateJobProgress,
    completeJob,
    clearCompleted,
    removeJob,
    cancelJob,
  };
});
