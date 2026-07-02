import { defineStore } from "pinia";
import { apiInvoke } from "../api/client";
import { ref, watch } from "vue";
import { useJobQueue } from "../composables/useJobQueue";
import { useAppStore } from "./appStore";
import { i18n } from "../i18n";
import { englishName } from "../utils/languageNames";

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

export type EngineType = "ollama" | "libretranslate" | "deepl" | "openai" | "google";

/**
 * Maps stable, locale-agnostic error codes emitted by the Rust backend (e.g.
 * "connect_failed") to a friendly, localized message. Used everywhere a job's
 * raw error message is surfaced to the user, not just the Settings/Detect
 * probes — a real translation job failing to reach Ollama mid-batch must show
 * the same friendly message as the Test Connection button.
 */
export function mapJobErrorMessage(message?: string): string | undefined {
  if (message === "connect_failed") {
    return i18n.global.t("translate.errorOllamaUnreachable");
  }
  if (message?.startsWith("deepl_unsupported_lang:")) {
    const code = message.substring("deepl_unsupported_lang:".length);
    const langName = englishName(code) ?? code;
    return i18n.global.t("translate.errorDeeplUnsupported", { lang: langName });
  }
  return message;
}

export const useTranslateStore = defineStore("translate", () => {
  const appStore = useAppStore();

  const selectedEngine = ref<EngineType>("ollama");
  const ollamaHost = ref(import.meta.env.VITE_OLLAMA_HOST ?? "http://localhost:11434");
  const ollamaModels = ref<string[]>([]);
  const selectedOllamaModel = ref("");
  const libreHost = ref(import.meta.env.VITE_LIBRE_HOST ?? "http://localhost:5000");
  const libreLanguages = ref<LibreLanguage[]>([]);
  const sourceLang = ref("en");
  const targetLang = ref("es");
  // Persisted via appStore.config (see hydrate/persist watchers below), not
  // held only for the session anymore — previously these reset every restart.
  const deeplKey = ref("");
  const deeplPro = ref(false);
  const openaiKey = ref("");
  const openaiModel = ref("gpt-4o-mini");
  const googleApiKey = ref("");
  const { jobs, addJob, updateJobProgress, completeJob, clearCompleted } = useJobQueue();
  const error = ref<string | null>(null);
  const previewOriginal = ref<string[]>([]);
  const previewTranslated = ref<string[]>([]);

  let hydrating = false;

  watch(
    () => appStore.config,
    (cfg) => {
      if (!cfg) return;
      hydrating = true;
      if (cfg.ollama_base_url) ollamaHost.value = cfg.ollama_base_url;
      if (cfg.libretranslate_base_url) libreHost.value = cfg.libretranslate_base_url;
      if (cfg.deepl_api_key) deeplKey.value = cfg.deepl_api_key;
      deeplPro.value = cfg.deepl_pro ?? false;
      if (cfg.openai_api_key) openaiKey.value = cfg.openai_api_key;
      if (cfg.openai_model) openaiModel.value = cfg.openai_model;
      if (cfg.google_translate_api_key) googleApiKey.value = cfg.google_translate_api_key;
      hydrating = false;
    },
    { immediate: true }
  );

  watch(
    [ollamaHost, libreHost, deeplKey, deeplPro, openaiKey, openaiModel, googleApiKey],
    async () => {
      if (hydrating || !appStore.config) return;
      appStore.config.ollama_base_url = ollamaHost.value;
      appStore.config.libretranslate_base_url = libreHost.value;
      appStore.config.deepl_api_key = deeplKey.value;
      appStore.config.deepl_pro = deeplPro.value;
      appStore.config.openai_api_key = openaiKey.value;
      appStore.config.openai_model = openaiModel.value;
      appStore.config.google_translate_api_key = googleApiKey.value;
      await appStore.saveConfig();
    }
  );

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
      const msg = e instanceof Error ? e.message : String(e);
      error.value =
        msg === "connect_failed"
          ? mapJobErrorMessage(msg)!
          : `Cannot reach Ollama at ${ollamaHost.value}: ${msg}`;
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
    } else if (selectedEngine.value === "google") {
      options.api_key = googleApiKey.value;
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
    googleApiKey,
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
