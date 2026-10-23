<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { FolderOpen, RotateCcw } from "lucide-vue-next";
import { open as openDialog } from "../utils/dialog";
import type { ToolStatus } from "../stores/appStore";

const props = defineProps<{
  toolId: string;
  modelValue?: string | null;
  status?: ToolStatus | null;
}>();
const emit = defineEmits<{
  "update:modelValue": [value: string | undefined];
  change: [];
}>();

const { t } = useI18n();

const isWindows = typeof navigator !== "undefined" && /windows/i.test(navigator.userAgent);

const badge = computed(() => props.status?.source ?? "none");

async function browse() {
  const selected = await openDialog({
    multiple: false,
    directory: false,
    title: t(`settings.${props.toolId}`),
    filters: isWindows ? [{ name: t("settings.executables"), extensions: ["exe"] }] : [],
  });
  if (typeof selected === "string" && selected) {
    emit("update:modelValue", selected);
    emit("change");
  }
}

function reset() {
  emit("update:modelValue", undefined);
  emit("change");
}
</script>

<template>
  <div :data-testid="`tool-${toolId}`">
    <div class="flex items-center gap-2 mb-1.5">
      <label class="text-xs font-medium text-text-secondary">{{ t(`settings.${toolId}`) }}</label>
      <span
        :data-testid="`badge-${toolId}`"
        class="px-1.5 py-0.5 rounded text-[10px] font-medium bg-bg-tertiary text-text-secondary"
      >
        {{ t(`settings.source.${badge}`) }}
      </span>
      <span v-if="status?.version" class="text-[11px] text-text-muted truncate">{{ status.version }}</span>
    </div>
    <div class="flex gap-2">
      <input
        :data-testid="`input-${toolId}`"
        :value="modelValue ?? ''"
        type="text"
        readonly
        :placeholder="status?.path || t(`settings.${toolId}Placeholder`)"
        class="flex-1 min-w-0 px-4 py-2.5 rounded-lg bg-bg-primary border border-border text-sm text-text-primary placeholder-text-muted focus:outline-none focus:border-accent cursor-pointer"
        @click="browse"
      />
      <button
        :data-testid="`browse-${toolId}`"
        type="button"
        class="inline-flex items-center gap-1.5 px-3 rounded-lg bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors"
        @click="browse"
      >
        <FolderOpen class="w-3.5 h-3.5" />
        {{ t("common.browse") }}
      </button>
      <button
        v-if="modelValue"
        :data-testid="`reset-${toolId}`"
        type="button"
        class="inline-flex items-center gap-1.5 px-3 rounded-lg bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors"
        @click="reset"
      >
        <RotateCcw class="w-3.5 h-3.5" />
        {{ t("settings.resetToBundled") }}
      </button>
    </div>
    <p v-if="status?.error" :data-testid="`error-${toolId}`" class="mt-1 text-xs text-red-500">{{ status.error }}</p>
  </div>
</template>
