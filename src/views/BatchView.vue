<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { open } from "../utils/dialog";
import { useBatchStore } from "../stores/batchStore";
import DropOverlay from "../components/DropOverlay.vue";
import { useFileDrop } from "../composables/useFileDrop";
import { useAppStore } from "../stores/appStore";
import { dirOf } from "../utils/subtitleNaming";

const { t } = useI18n();
import BatchFileList from "../components/BatchFileList.vue";
import JobMonitor from "../components/JobMonitor.vue";
import RenameModal from "../components/RenameModal.vue";
import AutoTranslateWizard from "../components/AutoTranslateWizard.vue";
import LanguagePickerModal from "../components/LanguagePickerModal.vue";
import MergeSubsModal from "../components/MergeSubsModal.vue";
import {
  FolderOpen,
  Loader2,
  AlertTriangle,
  FileVideo,
  Download,
  Info,
  Pencil,
  Languages,
  RefreshCw,
  Combine,
} from "lucide-vue-next";

const batchStore = useBatchStore();
const appStore = useAppStore();

const selectedCount = computed(() => batchStore.selectedFiles.size);
const totalCount = computed(() => batchStore.scannedFiles.length);

const renameModalOpen = ref(false);
const renameFiles = ref<string[]>([]);

function openRenameModal(files: string[]) {
  renameFiles.value = files;
  renameModalOpen.value = true;
}

function openBatchRename() {
  openRenameModal(Array.from(batchStore.selectedFiles));
}

const translateModalOpen = ref(false);
const translateFiles = ref<string[]>([]);

function openTranslateModal(files: string[]) {
  translateFiles.value = files;
  translateModalOpen.value = true;
}

function openBatchTranslate() {
  openTranslateModal(Array.from(batchStore.selectedFiles));
}

const { dragging: dropActive } = useFileDrop({
  onDrop: async (paths) => {
    // A dropped file means "scan the folder it lives in"; a folder is scanned
    // directly. In both cases the first path wins (batch is per-directory).
    const first = paths[0];
    const hasExtension = /\.[^./\\]+$/.test(first);
    const dir = hasExtension ? dirOf(first) : first;
    await batchStore.scanDirectory(dir);
  },
});

async function pickDirectory() {
  const selected = await open({
    multiple: false,
    directory: true,
  });
  if (selected && typeof selected === "string") {
    await batchStore.scanDirectory(selected);
  }
}

const langPickerOpen = ref(false);

function runBatchExtract() {
  langPickerOpen.value = true;
}

async function confirmExtractLanguages(languages: string[], destination: "beside" | "choose") {
  langPickerOpen.value = false;
  if (destination === "beside") {
    await batchStore.batchExtract("", languages);
    return;
  }
  const outputDir = await open({
    multiple: false,
    directory: true,
    title: "Select output directory for extracted subtitles",
  });
  if (outputDir && typeof outputDir === "string") {
    await batchStore.batchExtract(outputDir, languages);
  }
}

const selectedFileInfos = computed(() =>
  batchStore.scannedFiles.filter((f) => batchStore.selectedFiles.has(f.path))
);

const mergeSubsModalOpen = ref(false);
const mergeSubsFiles = ref<string[]>([]);

function openMergeSubs(files: string[]) {
  mergeSubsFiles.value = files;
  mergeSubsModalOpen.value = true;
}

function openBatchMergeSubs() {
  openMergeSubs(Array.from(batchStore.selectedFiles));
}

function toggleSelection(path: string) {
  batchStore.toggleSelection(path);
}

function toggleAll() {
  if (selectedCount.value === totalCount.value) {
    batchStore.deselectAll();
  } else {
    batchStore.selectAll();
  }
}

onMounted(() => {
  appStore.discoverTools();
});
</script>

<template>
  <div class="relative flex flex-col h-full">
    <DropOverlay :show="dropActive" :label="t('drop.batchHint')" />
    <!-- Header -->
    <div class="flex items-center justify-between px-6 py-4 border-b border-border">
      <div class="flex items-center gap-2">
        <h1 class="text-xl font-semibold text-text-primary">{{ t("batch.title") }}</h1>
        <span
          class="text-text-muted hover:text-text-secondary cursor-help"
          :title="t('batch.tooltip')"
        >
          <Info class="w-4 h-4" />
        </span>
      </div>
      <div class="flex items-center gap-2">
        <button
          v-if="batchStore.lastScannedPath"
          :disabled="batchStore.loading"
          class="inline-flex items-center gap-1.5 px-3 py-2.5 rounded-lg bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          :title="t('batch.refresh')"
          @click="batchStore.rescan"
        >
          <RefreshCw class="w-4 h-4" :class="{ 'animate-spin': batchStore.loading }" />
        </button>
        <button
          class="inline-flex items-center gap-2 px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover active:bg-accent-active transition-colors"
          @click="pickDirectory"
        >
          <FolderOpen class="w-4 h-4" />
          {{ t("batch.scanDirectory") }}
        </button>
      </div>
    </div>

    <!-- Content -->
    <div class="flex-1 overflow-auto p-6 space-y-6">
      <!-- Loading -->
      <div v-if="batchStore.loading" class="flex items-center justify-center py-12">
        <Loader2 class="w-6 h-6 text-accent animate-spin" />
        <span class="ml-3 text-sm text-text-secondary">{{ t("batch.scanning") }}</span>
      </div>

      <!-- Error -->
      <div
        v-else-if="batchStore.error"
        class="flex items-start gap-3 px-4 py-4 rounded-lg bg-error/10 border border-error/20"
      >
        <AlertTriangle class="w-5 h-5 text-error shrink-0 mt-0.5" />
        <div>
          <p class="text-sm font-medium text-error">{{ t("common.scanFailed") }}</p>
          <p class="text-xs text-error/80 mt-1">{{ batchStore.error }}</p>
        </div>
      </div>

      <!-- Results -->
      <template v-else-if="batchStore.scannedFiles.length > 0">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-3">
            <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
              {{ t("batch.files") }}
            </h2>
            <span class="text-xs text-text-muted">
              {{ t("batch.selected", { count: selectedCount, total: totalCount }) }}
            </span>
          </div>
          <div class="flex items-center gap-2">
            <button
              :disabled="selectedCount === 0"
              class="inline-flex items-center gap-1.5 px-3 py-2 rounded-md bg-accent/10 text-accent text-xs font-medium hover:bg-accent hover:text-white transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              @click="runBatchExtract"
            >
              <Download class="w-3.5 h-3.5" />
              {{ t("batch.extractSubs") }}
            </button>
            <button
              :disabled="selectedCount === 0"
              class="inline-flex items-center gap-1.5 px-3 py-2 rounded-md bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              @click="openBatchRename"
            >
              <Pencil class="w-3.5 h-3.5" />
              {{ t("batch.rename") }}
            </button>
            <button
              :disabled="selectedCount === 0"
              class="inline-flex items-center gap-1.5 px-3 py-2 rounded-md bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              @click="openBatchTranslate"
            >
              <Languages class="w-3.5 h-3.5" />
              {{ t("batch.autoTranslate") }}
            </button>
            <button
              :disabled="selectedCount === 0"
              class="inline-flex items-center gap-1.5 px-3 py-2 rounded-md bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              @click="openBatchMergeSubs"
            >
              <Combine class="w-3.5 h-3.5" />
              {{ t("batch.mergeSubs") }}
            </button>
          </div>
        </div>

        <BatchFileList
          :files="batchStore.scannedFiles"
          :selected="batchStore.selectedFiles"
          @toggle="toggleSelection"
          @toggle-all="toggleAll"
          @rename="(path) => openRenameModal([path])"
          @auto-translate="(path) => openTranslateModal([path])"
          @merge-subs="(path) => openMergeSubs([path])"
        />
      </template>

      <!-- Empty State -->
      <div
        v-else
        class="flex flex-col items-center justify-center py-16 text-center"
      >
        <div
          class="w-16 h-16 rounded-2xl bg-bg-secondary border border-border flex items-center justify-center mb-4"
        >
          <FileVideo class="w-8 h-8 text-text-muted" />
        </div>
        <h3 class="text-base font-medium text-text-primary">{{ t("batch.noFilesScanned") }}</h3>
        <p class="text-sm text-text-secondary mt-1 max-w-sm">
          {{ t("batch.noFilesScannedDesc") }}
        </p>
        <button
          class="mt-4 inline-flex items-center gap-2 px-4 py-2 rounded-lg bg-bg-tertiary text-text-primary text-sm font-medium hover:bg-bg-hover transition-colors"
          @click="pickDirectory"
        >
          <FolderOpen class="w-4 h-4" />
          {{ t("common.browse") }}
        </button>
      </div>

      <JobMonitor />
    </div>

    <RenameModal
      :open="renameModalOpen"
      :files="renameFiles"
      @close="renameModalOpen = false"
      @applied="batchStore.rescan"
    />
    <AutoTranslateWizard
      :open="translateModalOpen"
      :files="translateFiles"
      @close="translateModalOpen = false"
    />
    <LanguagePickerModal
      :open="langPickerOpen"
      :files="selectedFileInfos"
      @close="langPickerOpen = false"
      @confirm="confirmExtractLanguages"
    />
    <MergeSubsModal
      :open="mergeSubsModalOpen"
      :files="mergeSubsFiles"
      @close="mergeSubsModalOpen = false"
    />
  </div>
</template>
