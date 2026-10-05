<script setup lang="ts">
import { ref } from "vue";
import { useI18n } from "vue-i18n";
import { Check, Copy, Download } from "lucide-vue-next";
import type { DepStatus } from "../stores/depsStore";
import { openExternal } from "../utils/openExternal";

defineProps<{
  items: DepStatus[];
  installing: string | null;
  logs: Record<string, string[]>;
  errors: Record<string, string>;
}>();

// Ids installed through pip; they depend on Python being present first.
const PIP_IDS = new Set(["pysubs2", "faster_whisper"]);

const emit = defineEmits<{ (e: "install", id: string): void }>();

const { t, te } = useI18n();
const copied = ref<string | null>(null);

function purpose(item: DepStatus): string {
  const key = `deps.purpose.${item.id}`;
  return te(key) ? t(key) : item.purpose;
}

function dotClass(item: DepStatus): string {
  if (item.state === "ok") return "bg-success";
  return item.severity === "required" ? "bg-error" : "bg-warning";
}

async function copy(item: DepStatus) {
  if (!item.install_command) return;
  try {
    await navigator.clipboard.writeText(item.install_command);
    copied.value = item.id;
    setTimeout(() => {
      if (copied.value === item.id) copied.value = null;
    }, 1500);
  } catch {
    // Clipboard unavailable (insecure context); the command is still visible to select.
  }
}
</script>

<template>
  <ul class="space-y-3">
    <li
      v-for="item in items"
      :key="item.id"
      class="p-3 rounded-lg bg-bg-tertiary border border-border space-y-2"
    >
      <div class="flex items-start justify-between gap-3">
        <div class="min-w-0">
          <div class="flex items-center gap-2">
            <span :class="['w-2 h-2 rounded-full shrink-0', dotClass(item)]" />
            <span class="text-sm font-medium text-text-primary">{{ item.label }}</span>
            <span
              v-if="item.severity === 'optional'"
              class="text-[10px] uppercase tracking-wide text-text-muted border border-border rounded px-1.5 py-0.5"
              >{{ t("deps.optional") }}</span
            >
          </div>
          <p class="text-xs text-text-secondary mt-1">{{ purpose(item) }}</p>
          <p v-if="item.version" class="text-[11px] text-text-muted mt-0.5 truncate">{{ item.version }}</p>
        </div>
        <span
          :data-testid="`status-${item.id}`"
          :class="['text-xs font-medium shrink-0', item.state === 'ok' ? 'text-success' : 'text-text-secondary']"
          >{{ t(`deps.state.${item.state}`) }}</span
        >
      </div>

      <template v-if="item.state !== 'ok'">
        <div v-if="item.install_command" class="space-y-1">
          <p class="text-[11px] text-text-muted">{{ t("deps.command") }}</p>
          <code
            class="block text-xs px-2 py-1.5 rounded bg-bg-primary border border-border text-text-primary break-all select-all"
            dir="ltr"
            >{{ item.install_command }}</code
          >
        </div>
        <p v-if="item.install_command && !item.can_auto_install" class="text-xs text-warning">
          {{ t("deps.manualHint") }}
        </p>
        <p v-else-if="!item.install_command && !item.can_auto_install" class="text-xs text-text-muted">
          {{ PIP_IDS.has(item.id) ? t("deps.needPython") : t("deps.noPackageManager") }}
        </p>

        <div class="flex flex-wrap items-center gap-2">
          <button
            v-if="item.can_auto_install"
            :data-testid="`install-${item.id}`"
            :disabled="installing !== null"
            class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-accent text-white text-xs font-medium hover:bg-accent-hover active:bg-accent-active transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            @click="emit('install', item.id)"
          >
            <Download class="w-3.5 h-3.5" />
            {{ installing === item.id ? t("deps.installing") : t("deps.install") }}
          </button>
          <button
            v-if="item.install_command"
            :data-testid="`copy-${item.id}`"
            class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-bg-secondary border border-border text-text-secondary text-xs hover:bg-bg-hover transition-colors"
            @click="copy(item)"
          >
            <component :is="copied === item.id ? Check : Copy" class="w-3.5 h-3.5" />
            {{ copied === item.id ? t("deps.copied") : t("deps.copy") }}
          </button>
          <button
            :data-testid="`download-${item.id}`"
            class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-bg-secondary border border-border text-text-secondary text-xs hover:bg-bg-hover transition-colors"
            @click="openExternal(item.download_url)"
          >
            {{ t("deps.download") }}
          </button>
        </div>

        <p v-if="errors[item.id]" :data-testid="`error-${item.id}`" class="text-xs text-error break-words">
          {{ errors[item.id] }}
        </p>
        <details v-if="logs[item.id]?.length" class="text-xs text-text-muted">
          <summary class="cursor-pointer">{{ t("deps.details") }}</summary>
          <pre class="mt-1 max-h-40 overflow-auto whitespace-pre-wrap" dir="ltr">{{ logs[item.id].join("\n") }}</pre>
        </details>
      </template>
    </li>
  </ul>
</template>
