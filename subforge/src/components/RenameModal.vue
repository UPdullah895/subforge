<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { useI18n } from "vue-i18n";
import { Search, Loader2, AlertTriangle, Check } from "lucide-vue-next";
import Modal from "./Modal.vue";
import { useRenameStore, type Episode } from "../stores/renameStore";

const { t } = useI18n();

const props = defineProps<{
  open: boolean;
  files: string[];
}>();

const emit = defineEmits<{
  (e: "close"): void;
  (e: "applied"): void;
}>();

const renameStore = useRenameStore();
const applying = ref(false);

const tokens = ["{title}", "{s}", "{e}", "{episodeTitle}", "{quality}"];

function fileName(path: string): string {
  return path.split(/[/\\]/).pop() || path;
}

function fileBaseName(path: string): string {
  const name = fileName(path);
  const idx = name.lastIndexOf(".");
  return idx > 0 ? name.slice(0, idx) : name;
}

watch(
  () => props.open,
  async (isOpen) => {
    if (!isOpen) return;
    renameStore.reset();
    for (const f of props.files) {
      renameStore.manualNames[f] = fileBaseName(f);
    }
    if (props.files.length > 0) {
      await renameStore.guessQueryFrom(props.files[0]);
    }
  }
);

watch(
  () => renameStore.mode,
  () => {
    renameStore.results = [];
    renameStore.selectedSeries = null;
    renameStore.episodes = [];
    renameStore.previewRows = [];
  }
);

watch(
  [() => renameStore.pattern, () => renameStore.episodes, () => renameStore.mode],
  () => {
    if (renameStore.mode === "manual" || renameStore.selectedSeries) {
      renameStore.buildPreview(props.files);
    }
  }
);

watch(
  () => renameStore.manualNames,
  () => {
    if (renameStore.mode === "manual") {
      renameStore.buildPreview(props.files);
    }
  },
  { deep: true }
);

async function onSelectSeries(series: (typeof renameStore.results)[number]) {
  await renameStore.selectSeries(series);
  await renameStore.buildPreview(props.files);
}

function insertToken(token: string) {
  renameStore.pattern += token;
}

function onEpisodeOverride(oldPath: string, event: Event) {
  const value = (event.target as HTMLSelectElement).value;
  const [season, episode] = value.split("x").map(Number);
  const ep = renameStore.episodes.find((e) => e.season === season && e.episode === episode);
  if (ep) {
    renameStore.setRowOverride(oldPath, ep);
  }
}

const canConfirm = computed(() =>
  renameStore.previewRows.some((r) => r.new_name)
);

async function confirmRename() {
  applying.value = true;
  try {
    await renameStore.apply();
    emit("applied");
    emit("close");
  } finally {
    applying.value = false;
  }
}
</script>

<template>
  <Modal :open="open" :title="t('rename.title')" @close="emit('close')">
    <div class="space-y-4">
      <!-- Mode toggle -->
      <div class="flex items-center gap-2">
        <button
          v-for="m in (['manual', 'tmdb', 'tvmaze'] as const)"
          :key="m"
          :class="[
            'px-3 py-1.5 rounded-md text-xs font-medium transition-colors',
            renameStore.mode === m
              ? 'bg-accent text-white'
              : 'bg-bg-tertiary text-text-secondary hover:bg-bg-hover',
          ]"
          @click="renameStore.mode = m"
        >
          {{ t(`rename.mode.${m}`) }}
        </button>
      </div>

      <!-- Manual mode -->
      <div v-if="renameStore.mode === 'manual'" class="space-y-2">
        <div v-for="f in files" :key="f" class="flex items-center gap-2">
          <span class="text-xs text-text-muted truncate w-1/3" :title="f">{{ fileName(f) }}</span>
          <input
            v-model="renameStore.manualNames[f]"
            type="text"
            class="flex-1 px-3 py-2 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
          />
        </div>
      </div>

      <!-- Auto mode -->
      <div v-else class="space-y-4">
        <div class="flex items-center gap-2">
          <input
            v-model="renameStore.query"
            type="text"
            :placeholder="t('rename.searchPlaceholder')"
            class="flex-1 px-3 py-2 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
            @keyup.enter="renameStore.search"
          />
          <button
            class="inline-flex items-center gap-1.5 px-3 py-2 rounded-lg bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors"
            @click="renameStore.search"
          >
            <Search class="w-3.5 h-3.5" />
            {{ t("rename.search") }}
          </button>
        </div>

        <div v-if="renameStore.loading && renameStore.results.length === 0" class="flex justify-center py-4">
          <Loader2 class="w-5 h-5 text-accent animate-spin" />
        </div>

        <!-- Search results -->
        <div v-if="!renameStore.selectedSeries && renameStore.results.length > 0" class="space-y-1.5 max-h-48 overflow-auto">
          <button
            v-for="r in renameStore.results"
            :key="r.id"
            class="w-full flex items-center gap-3 px-3 py-2 rounded-lg bg-bg-primary border border-border hover:border-accent text-left transition-colors"
            @click="onSelectSeries(r)"
          >
            <img
              v-if="r.poster_url"
              :src="r.poster_url"
              class="w-8 h-11 object-cover rounded"
              alt=""
            />
            <div>
              <p class="text-sm text-text-primary">{{ r.name }}</p>
              <p v-if="r.year" class="text-xs text-text-muted">{{ r.year }}</p>
            </div>
          </button>
        </div>

        <!-- Selected series + pattern + preview -->
        <div v-if="renameStore.selectedSeries" class="space-y-3">
          <div class="flex items-center justify-between px-3 py-2 rounded-lg bg-bg-primary border border-border">
            <span class="text-sm text-text-primary">{{ renameStore.selectedSeries.name }}</span>
            <button
              class="text-xs text-text-muted hover:text-text-secondary"
              @click="renameStore.selectedSeries = null"
            >
              {{ t("rename.change") }}
            </button>
          </div>

          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("rename.pattern") }}</label>
            <input
              v-model="renameStore.pattern"
              type="text"
              class="w-full px-3 py-2 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
            />
            <div class="flex flex-wrap gap-1.5 mt-2">
              <button
                v-for="tok in tokens"
                :key="tok"
                class="px-2 py-1 rounded bg-bg-tertiary text-text-secondary text-xs hover:bg-bg-hover transition-colors"
                @click="insertToken(tok)"
              >
                {{ tok }}
              </button>
            </div>
          </div>
        </div>
      </div>

      <div
        v-if="renameStore.error"
        class="flex items-start gap-2 px-3 py-2 rounded-lg bg-error/10 border border-error/20 text-xs text-error"
      >
        <AlertTriangle class="w-4 h-4 shrink-0 mt-0.5" />
        {{ renameStore.error }}
      </div>

      <!-- Preview table -->
      <div v-if="renameStore.previewRows.length > 0" class="space-y-1.5">
        <h3 class="text-xs font-semibold text-text-secondary uppercase tracking-wide">
          {{ t("rename.preview") }}
        </h3>
        <div class="max-h-56 overflow-auto space-y-1.5">
          <div
            v-for="row in renameStore.previewRows"
            :key="row.old_path"
            :class="[
              'px-3 py-2 rounded-lg border text-xs',
              row.new_name ? 'bg-bg-primary border-border' : 'bg-warning/10 border-warning/30',
            ]"
          >
            <div class="flex items-center gap-2">
              <span class="text-text-muted truncate flex-1">{{ fileName(row.old_path) }}</span>
              <span class="text-text-muted">&rarr;</span>
              <span v-if="row.new_name" class="text-text-primary truncate flex-1">{{ row.new_name }}</span>
              <span v-else class="text-warning flex-1">{{ t("rename.unmatched") }}</span>
            </div>
            <select
              v-if="!row.new_name && renameStore.episodes.length > 0"
              class="mt-1.5 w-full px-2 py-1 rounded bg-bg-secondary border border-border text-xs text-text-primary"
              @change="onEpisodeOverride(row.old_path, $event)"
            >
              <option value="">{{ t("rename.pickEpisode") }}</option>
              <option
                v-for="ep in renameStore.episodes as Episode[]"
                :key="`${ep.season}x${ep.episode}`"
                :value="`${ep.season}x${ep.episode}`"
              >
                S{{ String(ep.season).padStart(2, "0") }}E{{ String(ep.episode).padStart(2, "0") }} - {{ ep.title }}
              </option>
            </select>
          </div>
        </div>
      </div>
    </div>

    <template #footer>
      <button
        class="px-4 py-2 rounded-lg bg-bg-tertiary text-text-secondary text-sm font-medium hover:bg-bg-hover transition-colors"
        @click="emit('close')"
      >
        {{ t("common.cancel") }}
      </button>
      <button
        :disabled="!canConfirm || applying"
        class="inline-flex items-center gap-2 px-4 py-2 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        @click="confirmRename"
      >
        <Loader2 v-if="applying" class="w-4 h-4 animate-spin" />
        <Check v-else class="w-4 h-4" />
        {{ t("rename.confirm") }}
      </button>
    </template>
  </Modal>
</template>
