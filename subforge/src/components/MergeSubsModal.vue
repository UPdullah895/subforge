<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import Modal from "./Modal.vue";
import { apiInvoke } from "../api/client";
import { useBatchStore } from "../stores/batchStore";
import { Loader2 } from "lucide-vue-next";

const { t } = useI18n();

const props = defineProps<{
  open: boolean;
  files: string[];
}>();

const emit = defineEmits<{
  (e: "close"): void;
}>();

const batchStore = useBatchStore();

const deleteOriginals = ref(false);
const merging = ref(false);

// path -> Set of langs checked for that file
const selections = reactive<Record<string, Set<string>>>({});

interface FileRow {
  path: string;
  name: string;
  subs: { lang: string; path: string }[];
}

const rows = computed<FileRow[]>(() =>
  props.files
    .map((path): FileRow | null => {
      const file = batchStore.scannedFiles.find((f) => f.path === path);
      if (!file) return null;
      const subs = file.external_subs
        .filter((s) => s.status === "ok")
        .map((s) => ({ lang: s.lang, path: s.path }));
      return { path, name: file.name, subs };
    })
    .filter((r): r is FileRow => r !== null && r.subs.length > 0)
);

watch(
  () => props.open,
  (isOpen) => {
    if (!isOpen) return;
    deleteOriginals.value = false;
    for (const key of Object.keys(selections)) delete selections[key];
  }
);

function isChecked(path: string, lang: string): boolean {
  return selections[path]?.has(lang) ?? false;
}

function toggle(path: string, lang: string) {
  if (!selections[path]) selections[path] = new Set();
  if (selections[path].has(lang)) selections[path].delete(lang);
  else selections[path].add(lang);
}

function mergeAllDetected() {
  for (const row of rows.value) {
    selections[row.path] = new Set(row.subs.map((s) => s.lang));
  }
}

const hasAnySelection = computed(() =>
  Object.values(selections).some((set) => set.size > 0)
);

async function confirmMerge() {
  merging.value = true;
  try {
    for (const row of rows.value) {
      const langs = selections[row.path];
      if (!langs || langs.size === 0) continue;
      const subs = row.subs.filter((s) => langs.has(s.lang)).map((s) => ({ path: s.path, lang: s.lang }));
      try {
        await apiInvoke("merge_external_subs", {
          request: { video_path: row.path, subs, delete_originals: deleteOriginals.value },
        });
      } catch (e) {
        const message = e instanceof Error ? e.message : String(e);
        for (const sub of subs) {
          batchStore.upsertExternalSub(row.path, sub.lang, "error", message);
        }
      }
    }
    await batchStore.rescan();
  } finally {
    merging.value = false;
    emit("close");
  }
}
</script>

<template>
  <Modal :open="open" :title="t('mergeSubs.title')" @close="emit('close')">
    <div class="space-y-3">
      <div v-if="rows.length === 0" class="text-sm text-text-muted py-4 text-center">
        {{ t("mergeSubs.noneAvailable") }}
      </div>
      <template v-else>
        <button
          class="text-xs text-accent hover:underline"
          @click="mergeAllDetected"
        >
          {{ t("mergeSubs.mergeAllDetected") }}
        </button>
        <div
          v-for="row in rows"
          :key="row.path"
          class="p-3 rounded-lg bg-bg-secondary border border-border space-y-2"
        >
          <p class="text-sm text-text-primary truncate" :title="row.path">{{ row.name }}</p>
          <div class="flex flex-wrap gap-3">
            <label
              v-for="sub in row.subs"
              :key="sub.lang"
              class="flex items-center gap-1.5 text-xs text-text-secondary"
            >
              <input
                type="checkbox"
                class="rounded border-border bg-bg-primary text-accent focus:ring-accent"
                :checked="isChecked(row.path, sub.lang)"
                @change="toggle(row.path, sub.lang)"
              />
              <span class="uppercase">{{ sub.lang }}</span>
            </label>
          </div>
        </div>

        <label class="flex items-center gap-2 text-xs text-text-secondary pt-1">
          <input v-model="deleteOriginals" type="checkbox" class="accent-accent" />
          {{ t("mergeSubs.deleteOriginals") }}
        </label>
      </template>
    </div>

    <template #footer>
      <button
        class="inline-flex items-center gap-1.5 px-3 py-2 rounded-lg bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors"
        @click="emit('close')"
      >
        {{ t("common.cancel") }}
      </button>
      <button
        :disabled="!hasAnySelection || merging"
        class="inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-accent text-white text-xs font-medium hover:bg-accent-hover active:bg-accent-active transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        @click="confirmMerge"
      >
        <Loader2 v-if="merging" class="w-3.5 h-3.5 animate-spin" />
        {{ t("mergeSubs.confirm") }}
      </button>
    </template>
  </Modal>
</template>
