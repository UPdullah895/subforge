import { defineStore } from "pinia";
import { apiInvoke } from "../api/client";
import { ref } from "vue";


export interface OllamaInfo {
  available: boolean;
  models: string[];
}

export interface LibreLanguage {
  code: string;
  name: string;
}

export interface LibreInfo {
  available: boolean;
  languages: LibreLanguage[];
}

export interface TranslateJob {
  id: string;
  status: "pending" | "running" | "completed" | "failed";
  input_file: string;
  progress: number;
  message?: string;
}

export type EngineType = "ollama" | "libretranslate" | "deepl" | "openai";

export const useTranslateStore = defineStore("translate", () => {
  const selectedEngine = ref<EngineType>("ollama");
  const ollamaHost = ref(import.meta.env.VITE_OLLAMA_HOST ?? "http://localhost:11434");
  const ollamaModels = ref<string[]>([]);
  const selectedOllamaModel = ref("");
  const libreHost = ref(import.meta.env.VITE_LIBRE_HOST ?? "http://localhost:5000");
  const libreLanguages = ref<LibreLanguage[]>([]);
  const sourceLang = ref("en");
  const targetLang = ref("es");
  const deeplKey = ref("");
  const deeplPro = ref(false);
  const openaiKey = ref("");
  const openaiModel = ref("gpt-4o-mini");
  const jobs = ref<TranslateJob[]>([]);
  const previewOriginal = ref<string[]>([]);
  const previewTranslated = ref<string[]>([]);

  async function detectOllama() {
    try {
      const info = await apiInvoke<OllamaInfo>("detect_ollama", {
        host: ollamaHost.value,
      });
      ollamaModels.value = info.models;
      if (info.models.length > 0 && !selectedOllamaModel.value) {
        selectedOllamaModel.value = info.models[0];
      }
      return info.available;
    } catch {
      ollamaModels.value = [];
      return false;
    }
  }

  async function detectLibreTranslate() {
    try {
      const info = await apiInvoke<LibreInfo>("detect_libretranslate", {
        host: libreHost.value,
      });
      libreLanguages.value = info.languages;
      return info.available;
    } catch {
      libreLanguages.value = [];
      return false;
    }
  }

  async function startTranslation(input: string, output: string) {
    const options: Record<string, unknown> = {
      engine: selectedEngine.value,
      input,
      output,
      source_lang: sourceLang.value,
      target_lang: targetLang.value,
    };

    if (selectedEngine.value === "ollama") {
      options.host = ollamaHost.value;
      options.model = selectedOllamaModel.value || null;
    } else if (selectedEngine.value === "libretranslate") {
      options.host = libreHost.value;
    } else if (selectedEngine.value === "deepl") {
      options.api_key = deeplKey.value;
    } else if (selectedEngine.value === "openai") {
      options.api_key = openaiKey.value;
      options.model = openaiModel.value;
    }

    try {
      const jobId = await apiInvoke<string>("translate_subtitles", {
        request: options,
      });
      jobs.value.push({
        id: jobId,
        status: "pending",
        input_file: input,
        progress: 0,
      });
      return jobId;
    } catch (e) {
      console.error("Failed to start translation:", e);
      return null;
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

  function setPreview(original: string[], translated: string[]) {
    previewOriginal.value = original;
    previewTranslated.value = translated;
  }

  return {
    selectedEngine,
    ollamaHost,
    ollamaModels,
    selectedOllamaModel,
    libreHost,
    libreLanguages,
    sourceLang,
    targetLang,
    deeplKey,
    deeplPro,
    openaiKey,
    openaiModel,
    jobs,
    previewOriginal,
    previewTranslated,
    detectOllama,
    detectLibreTranslate,
    startTranslation,
    updateJobProgress,
    completeJob,
    clearCompleted,
    setPreview,
  };
});
