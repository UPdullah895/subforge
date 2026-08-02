<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import Modal from "./Modal.vue";
import type { VideoFileInfo } from "../stores/batchStore";

const { t } = useI18n();

const props = defineProps<{
  open: boolean;
  files: VideoFileInfo[];
}>();

const emit = defineEmits<{
  (e: "close"): void;
  (e: "confirm", languages: string[]): void;
}>();

const checked = ref<Set<string>>(new Set());

interface LangRow {
  lang: string;
  count: number;
}

const rows = computed<LangRow[]>(() => {
  const counts = new Map<string, number>();
  for (const file of props.files) {
    const langsInFile = new Set(
      file.tracks
        .filter((tr) => tr.track_type === "subtitles")
        .map((tr) => tr.properties?.language || "und")
    );
    for (const lang of langsInFile) {
      counts.set(lang, (counts.get(lang) || 0) + 1);
    }
  }
  return Array.from(counts.entries())
    .map(([lang, count]) => ({ lang, count }))
    .sort((a, b) => b.count - a.count || a.lang.localeCompare(b.lang));
});

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) checked.value = new Set();
  }
);

function toggle(lang: string) {
  if (checked.value.has(lang)) checked.value.delete(lang);
  else checked.value.add(lang);
}

function confirm() {
  emit("confirm", Array.from(checked.value));
}
</script>

<template>
  <Modal :open="open" :title="t('extractLangPicker.title')" @close="emit('close')">
    <div class="space-y-3">
      <p class="text-xs text-text-muted">{{ t("extractLangPicker.description") }}</p>
      <div v-if="rows.length === 0" class="text-sm text-text-muted py-4 text-center">
        {{ t("extractLangPicker.noTracks") }}
      </div>
      <label
        v-for="row in rows"
        :key="row.lang"
        class="flex items-center justify-between gap-3 p-2.5 rounded-lg bg-bg-secondary border border-border cursor-pointer"
      >
        <span class="flex items-center gap-2">
          <input
            type="checkbox"
            class="rounded border-border bg-bg-primary text-accent focus:ring-accent"
            :checked="checked.has(row.lang)"
            @change="toggle(row.lang)"
          />
          <span class="text-sm text-text-primary uppercase">{{ row.lang }}</span>
        </span>
        <span class="text-xs text-text-muted">
          {{ t("extractLangPicker.fileCoverage", { count: row.count, total: files.length }) }}
        </span>
      </label>
    </div>

    <template #footer>
      <button
        class="inline-flex items-center gap-1.5 px-3 py-2 rounded-lg bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors"
        @click="emit('close')"
      >
        {{ t("common.cancel") }}
      </button>
      <button
        :disabled="checked.size === 0"
        class="inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-accent text-white text-xs font-medium hover:bg-accent-hover active:bg-accent-active transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        @click="confirm"
      >
        {{ t("extractLangPicker.confirm") }}
      </button>
    </template>
  </Modal>
</template>
