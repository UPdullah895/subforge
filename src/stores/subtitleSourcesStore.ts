import { defineStore } from "pinia";
import { ref } from "vue";
import { apiInvoke } from "../api/client";
import { useAppStore } from "./appStore";

export interface OpenSubtitlesCredentials {
  api_key: string;
  username: string;
  password: string;
}

export interface OpenSubtitlesStatus {
  configured: boolean;
  /** Present on the desktop app; never returned by the HTTP API. */
  username?: string | null;
  api_key?: string | null;
  password_set?: boolean;
}

export type SubsourceReason =
  | "ok"
  | "rate_limited"
  | "invalid_key"
  | "unreachable"
  | "not_configured";

export interface SubsourceStatus {
  configured: boolean;
  reachable: boolean;
  reason: SubsourceReason;
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
  const opensubtitlesUsername = ref("");
  const opensubtitlesApiKey = ref("");
  const opensubtitlesPasswordSet = ref(false);

  const subsourceReachable = ref<boolean | null>(null);
  const subsourceStatus = ref<SubsourceStatus | null>(null);

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
      const status = await apiInvoke<OpenSubtitlesStatus>("get_opensubtitles_status");
      opensubtitlesConfigured.value = status.configured;
      opensubtitlesUsername.value = status.username ?? "";
      opensubtitlesApiKey.value = status.api_key ?? "";
      opensubtitlesPasswordSet.value = status.password_set ?? status.configured;
    } catch {
      opensubtitlesConfigured.value = false;
    }
    try {
      const status = await apiInvoke<SubsourceStatus>("check_subsource_status");
      subsourceStatus.value = status;
      subsourceReachable.value = status.reachable;
    } catch {
      subsourceStatus.value = { configured: false, reachable: false, reason: "unreachable" };
      subsourceReachable.value = false;
    }
  }

  async function setOpenSubtitlesCredentials(credentials: OpenSubtitlesCredentials) {
    await apiInvoke("set_opensubtitles_credentials", { credentials });
    await loadStatus();
  }

  async function clearOpenSubtitlesCredentials() {
    await apiInvoke("clear_opensubtitles_credentials");
    opensubtitlesUsername.value = "";
    opensubtitlesApiKey.value = "";
    opensubtitlesPasswordSet.value = false;
    opensubtitlesConfigured.value = false;
    testStatus.value = "idle";
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
    opensubtitlesUsername,
    opensubtitlesApiKey,
    opensubtitlesPasswordSet,
    subsourceReachable,
    subsourceStatus,
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
