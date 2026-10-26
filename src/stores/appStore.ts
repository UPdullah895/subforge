import { defineStore } from "pinia";
import { apiInvoke } from "../api/client";
import { ref } from "vue";


export type Language = "en" | "ar";

export interface AppConfig {
  theme: string;
  language: Language;
  mkvmerge_path?: string;
  mkvextract_path?: string;
  mkvpropedit_path?: string;
  config_version?: number;
  ffmpeg_path?: string;
  ffprobe_path?: string;
  default_engine?: string;
  default_source_lang?: string;
  default_target_lang?: string;
  default_target_langs?: string[];
  default_output_dir?: string;
  tmdb_api_key?: string;
  deepl_api_key?: string;
  deepl_pro?: boolean;
  openai_api_key?: string;
  openai_model?: string;
  google_translate_api_key?: string;
  subsource_api_key?: string;
  ollama_base_url?: string;
  libretranslate_base_url?: string;
  search_online_before_translate?: boolean;
  fix_rtl_punctuation?: boolean;
}

export interface ToolInfo {
  name: string;
  path?: string;
  version?: string;
  available: boolean;
}

export interface ToolStatus {
  id: "ffmpeg" | "ffprobe" | "mkvmerge" | "mkvextract" | "mkvpropedit";
  source: "custom" | "bundled" | "system" | null;
  path?: string | null;
  version?: string | null;
  error?: string | null;
}

export interface ToolRegistry {
  mkvmerge: ToolInfo;
  mkvextract: ToolInfo;
  ffmpeg: ToolInfo;
  ffprobe: ToolInfo;
}

export const useAppStore = defineStore("app", () => {
  const theme = ref("dark");
  const language = ref<Language>("en");
  const sidebarCollapsed = ref(false);
  const toolRegistry = ref<ToolRegistry | null>(null);
  const toolStatus = ref<ToolStatus[]>([]);
  const config = ref<AppConfig | null>(null);
  const error = ref<string | null>(null);
  /** Last settings-save failure; rendered in Settings so it is never silent. */
  const saveError = ref<string | null>(null);

  async function loadConfig() {
    try {
      config.value = await apiInvoke<AppConfig>("load_config");
      theme.value = config.value.theme;
      language.value = config.value.language ?? "en";
    } catch {
      config.value = { theme: "dark", language: "en" };
      theme.value = "dark";
      language.value = "en";
    }
  }

  async function saveConfig(): Promise<boolean> {
    if (!config.value) return false;
    try {
      await apiInvoke("save_config", { config: config.value });
      saveError.value = null;
      return true;
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      saveError.value = message;
      error.value = `Failed to save settings: ${message}`;
      return false;
    }
  }

  async function discoverTools() {
    try {
      toolRegistry.value = await apiInvoke<ToolRegistry>("get_tool_registry");
    } catch (e) {
      error.value = `Failed to detect tools: ${e instanceof Error ? e.message : String(e)}`;
    }
  }

  async function loadToolStatus() {
    try {
      toolStatus.value = await apiInvoke<ToolStatus[]>("get_tool_status");
    } catch (e) {
      error.value = `Failed to detect tools: ${e instanceof Error ? e.message : String(e)}`;
    }
  }

  async function toggleTheme() {
    theme.value = theme.value === "dark" ? "light" : "dark";
    if (config.value) {
      config.value.theme = theme.value;
      await saveConfig();
    }
  }

  async function setLanguage(next: Language) {
    language.value = next;
    if (config.value) {
      config.value.language = next;
      await saveConfig();
    }
  }

  function toggleSidebar() {
    sidebarCollapsed.value = !sidebarCollapsed.value;
  }

  return {
    theme,
    language,
    sidebarCollapsed,
    toolRegistry,
    toolStatus,
    config,
    error,
    saveError,
    loadConfig,
    saveConfig,
    discoverTools,
    loadToolStatus,
    toggleTheme,
    setLanguage,
    toggleSidebar,
  };
});
