import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { apiInvoke } from "../api/client";
import { apiListen } from "../api/events";

export type DepState = "ok" | "missing" | "outdated";
export type DepSeverity = "required" | "optional";

export interface DepStatus {
  id: string;
  label: string;
  severity: DepSeverity;
  state: DepState;
  path?: string | null;
  version?: string | null;
  purpose: string;
  download_url: string;
  install_command?: string | null;
  can_auto_install: boolean;
}

export interface DepReport {
  os: string;
  items: DepStatus[];
  has_missing_required: boolean;
}

function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export const useDepsStore = defineStore("deps", () => {
  const report = ref<DepReport | null>(null);
  const checking = ref(false);
  const installing = ref<string | null>(null);
  const logs = ref<Record<string, string[]>>({});
  const errors = ref<Record<string, string>>({});
  const dialogDismissed = ref(false);

  const missing = computed(() => report.value?.items.filter((i) => i.state !== "ok") ?? []);
  const missingRequired = computed(() => missing.value.filter((i) => i.severity === "required"));
  const showDialog = computed(() => !!report.value?.has_missing_required && !dialogDismissed.value);
  const showBanner = computed(() => !!report.value?.has_missing_required && dialogDismissed.value);

  async function check() {
    checking.value = true;
    try {
      report.value = await apiInvoke<DepReport>("check_dependencies");
    } catch {
      // Backend unreachable: leave the previous report (or none) in place.
    } finally {
      checking.value = false;
    }
  }

  async function install(id: string): Promise<boolean> {
    installing.value = id;
    logs.value[id] = [];
    delete errors.value[id];
    try {
      report.value = await apiInvoke<DepReport>("install_dependency", { id });
      return true;
    } catch (e) {
      errors.value[id] = messageOf(e);
      await check();
      return false;
    } finally {
      installing.value = null;
    }
  }

  // Re-evaluates after every install: installing Python unlocks the pip packages.
  async function installAll() {
    const tried = new Set<string>();
    for (;;) {
      const next = missing.value.find((i) => i.can_auto_install && !tried.has(i.id));
      if (!next) return;
      tried.add(next.id);
      if (!(await install(next.id))) return;
    }
  }

  function dismiss() {
    dialogDismissed.value = true;
  }

  function reopen() {
    dialogDismissed.value = false;
  }

  async function listen(): Promise<() => void> {
    return apiListen<{ job_id: string; message?: string }>("dep-install-progress", (p) => {
      (logs.value[p.job_id] ??= []).push(p.message ?? "");
    });
  }

  return {
    report,
    checking,
    installing,
    logs,
    errors,
    dialogDismissed,
    missing,
    missingRequired,
    showDialog,
    showBanner,
    check,
    install,
    installAll,
    dismiss,
    reopen,
    listen,
  };
});
