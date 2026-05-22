import { defineStore } from "pinia";
import { apiInvoke } from "../api/client";
import { ref } from "vue";
import { useJobQueue } from "../composables/useJobQueue";

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
  const { jobs, addJob, updateJobProgress, completeJob, clearCompleted } = useJobQueue();
  const error = ref<string | null>(null);
  const previewOriginal = ref<string[]>([]);
  const previewTranslated = ref<string[]>([]);

  async function detectOllama() {
    error.value = null;
    try {
      const info = await apiInvoke<OllamaInfo>("detect_ollama", {
        host: ollamaHost.value,
      });
      ollamaModels.value = info.models;
      if (info.models.length > 0 && !selectedOllamaModel.value) {
        selectedOllamaModel.value = info.models[0];
      }
      return info.available;
    } catch (e) {
      ollamaModels.value = [];
      error.value = `Cannot reach Ollama at ${ollamaHost.value}: ${e instanceof Error ? e.message : String(e)}`;
      return false;
    }
  }

  async function detectLibreTranslate() {
    error.value = null;
    try {
      const info = await apiInvoke<LibreInfo>("detect_libretranslate", {
        host: libreHost.value,
      });
      libreLanguages.value = info.languages;
      return info.available;
    } catch (e) {
      libreLanguages.value = [];
      error.value = `Cannot reach LibreTranslate at ${libreHost.value}: ${e instanceof Error ? e.message : String(e)}`;
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

    error.value = null;
    try {
      const jobId = await apiInvoke<string>("translate_subtitles", {
        request: options,
      });
      addJob(jobId, input);
      return jobId;
    } catch (e) {
      error.value = `Failed to start translation: ${e instanceof Error ? e.message : String(e)}`;
      return null;
    }
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
    error,
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
