import { defineStore } from "pinia";
import { ref } from "vue";
import { apiInvoke } from "../api/client";
import { useAppStore } from "./appStore";

export interface OpenSubtitlesCredentials {
  api_key: string;
  username: string;
  password: string;
}

export interface OnlineSubtitleSearchRequest {
  video_path: string;
  title: string;
  year?: number;
  season?: number;
  episode?: number;
  language: string;
}

export type OnlineSubtitleSearchResult =
  | { status: "found"; subtitle_path: string; source: string; matched_via: string }
  | { status: "not_found" };

export const useSubtitleSourcesStore = defineStore("subtitleSources", () => {
  const opensubtitlesConfigured = ref(false);
  const subsourceReachable = ref<boolean | null>(null);
  const testStatus = ref<"idle" | "testing" | "success" | "error">("idle");
  const testError = ref<string | null>(null);

  const appStore = useAppStore();

  function searchEnabled(): boolean {
    return appStore.config?.search_online_before_translate ?? true;
  }

  async function setSearchEnabled(value: boolean) {
    if (!appStore.config) return;
    appStore.config.search_online_before_translate = value;
    await appStore.saveConfig();
  }

  async function loadStatus() {
    try {
      const status = await apiInvoke<{ configured: boolean }>("get_opensubtitles_status");
      opensubtitlesConfigured.value = status.configured;
    } catch {
      opensubtitlesConfigured.value = false;
    }
    try {
      const status = await apiInvoke<{ reachable: boolean }>("check_subsource_status");
      subsourceReachable.value = status.reachable;
    } catch {
      subsourceReachable.value = false;
    }
  }

  async function setOpenSubtitlesCredentials(credentials: OpenSubtitlesCredentials) {
    await apiInvoke("set_opensubtitles_credentials", { credentials });
    opensubtitlesConfigured.value = true;
  }

  async function clearOpenSubtitlesCredentials() {
    await apiInvoke("clear_opensubtitles_credentials");
    opensubtitlesConfigured.value = false;
  }

  async function testOpenSubtitlesConnection() {
    testStatus.value = "testing";
    testError.value = null;
    try {
      await apiInvoke("test_provider_config", { provider: "opensubtitles" });
      testStatus.value = "success";
    } catch (e) {
      testStatus.value = "error";
      testError.value = e instanceof Error ? e.message : String(e);
    }
  }

  async function searchOnlineSubtitle(
    request: OnlineSubtitleSearchRequest
  ): Promise<OnlineSubtitleSearchResult> {
    return apiInvoke<OnlineSubtitleSearchResult>("search_online_subtitle", { request });
  }

  return {
    opensubtitlesConfigured,
    subsourceReachable,
    testStatus,
    testError,
    searchEnabled,
    setSearchEnabled,
    loadStatus,
    setOpenSubtitlesCredentials,
    clearOpenSubtitlesCredentials,
    testOpenSubtitlesConnection,
    searchOnlineSubtitle,
  };
});
