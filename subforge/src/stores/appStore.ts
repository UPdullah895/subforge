import { defineStore } from "pinia";
import { apiInvoke } from "../api/client";
import { ref } from "vue";


export interface AppConfig {
  theme: string;
  mkvmerge_path?: string;
  mkvextract_path?: string;
  ffmpeg_path?: string;
  ffprobe_path?: string;
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
  const sidebarCollapsed = ref(false);
  const toolRegistry = ref<ToolRegistry | null>(null);
  const config = ref<AppConfig | null>(null);

  async function loadConfig() {
    config.value = await apiInvoke<AppConfig>("load_config");
    theme.value = config.value.theme;
  }

  async function saveConfig() {
    if (config.value) {
      await apiInvoke("save_config", { config: config.value });
    }
  }

  async function discoverTools() {
    toolRegistry.value = await apiInvoke<ToolRegistry>("get_tool_registry");
  }

  async function toggleTheme() {
    theme.value = theme.value === "dark" ? "light" : "dark";
    if (config.value) {
      config.value.theme = theme.value;
      await saveConfig();
    }
  }

  function toggleSidebar() {
    sidebarCollapsed.value = !sidebarCollapsed.value;
  }

  return {
    theme,
    sidebarCollapsed,
    toolRegistry,
    config,
    loadConfig,
    saveConfig,
    discoverTools,
    toggleTheme,
    toggleSidebar,
  };
});
