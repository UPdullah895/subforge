import { defineStore } from "pinia";
import { apiInvoke } from "../api/client";
import { ref } from "vue";


export type Language = "en" | "ar";

export interface AppConfig {
  theme: string;
  language: Language;
  mkvmerge_path?: string;
  mkvextract_path?: string;
  ffmpeg_path?: string;
  ffprobe_path?: string;
  default_engine?: string;
  default_source_lang?: string;
  default_target_lang?: string;
  default_output_dir?: string;
}

export interface ToolInfo {
  name: string;
  path?: string;
  version?: string;
  available: boolean;
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
  const config = ref<AppConfig | null>(null);
  const error = ref<string | null>(null);

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

  async function saveConfig() {
    try {
      if (config.value) {
        await apiInvoke("save_config", { config: config.value });
      }
    } catch (e) {
      error.value = `Failed to save settings: ${e instanceof Error ? e.message : String(e)}`;
    }
  }

  async function discoverTools() {
    try {
      toolRegistry.value = await apiInvoke<ToolRegistry>("get_tool_registry");
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
    config,
    error,
    loadConfig,
    saveConfig,
    discoverTools,
    toggleTheme,
    setLanguage,
    toggleSidebar,
  };
});
