<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { useI18n } from "vue-i18n";
import { open } from "../utils/dialog";

import { apiListen } from "../api/events";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { useTranscribeStore } from "../stores/transcribeStore";

const { t } = useI18n();
const toastStore = useToastStore();
import { JOB_STATUS, type JobStatus } from "../constants/jobStatus";
import { useAppStore } from "../stores/appStore";
import ErrorBanner from "../components/ErrorBanner.vue";
import DropOverlay from "../components/DropOverlay.vue";
import { useFileDrop } from "../composables/useFileDrop";
import { splitFilesAndFolders } from "../utils/paths";
import { useToastStore } from "../stores/toastStore";
import ErrorDetails from "../components/ErrorDetails.vue";
import Modal from "../components/Modal.vue";
import {
  FolderOpen,
  Mic,
  Loader2,
  CheckCircle2,
  Download,
  X,
  XCircle,
  Play,
  Info,
  AlertTriangle,
  RefreshCw,
  Trash2,
} from "lucide-vue-next";

const transcribeStore = useTranscribeStore();
const appStore = useAppStore();

const selectedFiles = ref<string[]>([]);
const selectedModel = ref("tiny");
const selectedDevice = ref("auto");
const language = ref("");
const outputFormat = ref("srt");
const wordTimestamps = ref(false);
const vadFilter = ref(true);
const transcribing = ref(false);
const confirmLegacyDelete = ref(false);

const fileCount = computed(() => selectedFiles.value.length);

const isModelDownloaded = computed(() =>
  transcribeStore.downloadedModels.includes(selectedModel.value)
);

const downloadPercent = computed(() => Math.round(transcribeStore.download?.progress ?? 0));

async function deleteLegacy() {
  confirmLegacyDelete.value = false;
  await transcribeStore.deleteLegacyCache();
}

function buildOutputPath(inputPath: string, format: string): string {
  const lastSlash = Math.max(inputPath.lastIndexOf("/"), inputPath.lastIndexOf("\\"));
  const dir = inputPath.substring(0, lastSlash + 1);
  const name = inputPath.substring(lastSlash + 1);
  const base = name.replace(/\.[^.]+$/, "");
  return `${dir}${base}.${format}`;
}

const MEDIA_EXTS = [
  "mkv", "mp4", "avi", "mov", "wmv", "flv", "webm",
  "mp3", "wav", "flac", "aac", "m4a", "ogg", "opus",
];

const { dragging: dropActive } = useFileDrop({
  extensions: MEDIA_EXTS,
  onDrop: async (paths) => {
    const { files, folders } = await splitFilesAndFolders(paths);
    if (folders.length > 0) toastStore.info(t("drop.folderNotSupported"));
    for (const path of files) {
      if (!selectedFiles.value.includes(path)) selectedFiles.value.push(path);
    }
  },
});

async function pickFiles() {
  const selected = await open({
    multiple: true,
    directory: false,
    filters: [
      { name: "Media files", extensions: ["mkv", "mp4", "avi", "mov", "wmv", "flv", "webm", "mp3", "wav", "flac", "aac", "m4a"] },
      { name: "Audio files", extensions: ["mp3", "wav", "flac", "aac", "m4a", "ogg", "opus"] },
      { name: "Video files", extensions: ["mkv", "mp4", "avi", "mov", "wmv", "flv", "webm"] },
    ],
  });
  if (Array.isArray(selected)) {
    selectedFiles.value.push(...selected);
  } else if (selected && typeof selected === "string") {
    selectedFiles.value.push(selected);
  }
}

function removeFile(index: number) {
  selectedFiles.value.splice(index, 1);
}

function clearFiles() {
  selectedFiles.value = [];
}

async function startTranscription() {
  if (selectedFiles.value.length === 0) return;

  transcribing.value = true;
  for (const file of selectedFiles.value) {
    const output = buildOutputPath(file, outputFormat.value);
    await transcribeStore.startTranscription({
      input: file,
      output,
      model: selectedModel.value,
      device: selectedDevice.value,
      language: language.value || undefined,
      format: outputFormat.value,
      wordTimestamps: wordTimestamps.value,
      vadFilter: vadFilter.value,
    });
  }
  transcribing.value = false;
}

let unlistenProgress: UnlistenFn | null = null;
let unlistenCompleted: UnlistenFn | null = null;

async function loadEngine() {
  await transcribeStore.loadInfo();
  if (transcribeStore.whisperMissing) return;
  await Promise.all([
    transcribeStore.loadCatalog(),
    transcribeStore.loadModels(),
    transcribeStore.loadLegacyCache(),
  ]);
}

onMounted(async () => {
  appStore.discoverTools();
  await loadEngine();

  unlistenProgress = await apiListen<{ job_id: string; progress: number; message?: string }>(
    "job-progress",
    ({ job_id, progress, message }) => {
      transcribeStore.updateJobProgress(job_id, progress, message);
    }
  );

  unlistenCompleted = await apiListen<{ job_id: string; status: string; message?: string; details?: string | null }>(
    "job-completed",
    ({ job_id, status, message, details }) => {
      transcribeStore.completeJob(job_id, status as JobStatus, message, details);
    }
  );
});

async function retrySetup() {
  await loadEngine();
}

onUnmounted(() => {
  if (unlistenProgress) unlistenProgress();
  if (unlistenCompleted) unlistenCompleted();
});
</script>

<template>
  <div class="relative flex flex-col h-full">
    <DropOverlay :show="dropActive" :label="t('drop.transcribeHint')" />
    <!-- Header -->
    <div class="flex items-center justify-between px-6 py-4 border-b border-border">
      <div class="flex items-center gap-2">
        <h1 class="text-xl font-semibold text-text-primary">{{ t("transcribe.title") }}</h1>
        <span
          class="text-text-muted hover:text-text-secondary cursor-help"
          :title="t('transcribe.tooltip')"
        >
          <Info class="w-4 h-4" />
        </span>
      </div>
      <button
        :disabled="transcribeStore.whisperMissing"
        class="inline-flex items-center gap-2 px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover active:bg-accent-active transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        @click="pickFiles"
      >
        <FolderOpen class="w-4 h-4" />
        {{ t("transcribe.openMedia") }}
      </button>
    </div>

    <!-- Content -->
    <div class="flex-1 overflow-auto p-6 space-y-6">
      <ErrorBanner
        :message="transcribeStore.error"
        :details="transcribeStore.errorDetails"
        @dismiss="transcribeStore.clearError()"
      />

      <!-- Engine status could not be read (e.g. web mode with the server down) -->
      <div
        v-if="transcribeStore.whisperMissing"
        class="p-4 rounded-lg bg-warning/10 border border-warning/30 space-y-3"
        role="alert"
      >
        <div class="flex items-start gap-3">
          <AlertTriangle class="w-5 h-5 text-warning shrink-0 mt-0.5" />
          <div class="space-y-1">
            <h2 class="text-sm font-semibold text-text-primary">{{ t("transcribe.engineUnavailableTitle") }}</h2>
            <p class="text-xs text-text-secondary">{{ t("transcribe.engineUnavailableBody") }}</p>
          </div>
        </div>
        <button
          class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors"
          @click="retrySetup"
        >
          <RefreshCw class="w-3.5 h-3.5" :class="transcribeStore.loadingInfo ? 'animate-spin' : ''" />
          {{ t("transcribe.checkAgain") }}
        </button>
      </div>

      <!-- Environment Info -->
      <div
        v-if="transcribeStore.whisperInfo"
        class="flex items-center gap-4 px-4 py-3 rounded-lg bg-bg-secondary border border-border"
      >
        <div class="flex items-center gap-2">
          <div
            :class="[
              'w-2 h-2 rounded-full',
              transcribeStore.whisperInfo.gpu ? 'bg-success' : 'bg-warning',
            ]"
          />
          <span class="text-xs text-text-secondary">
            {{
              transcribeStore.whisperInfo.gpu
                ? t("transcribe.gpuAvailable", { name: transcribeStore.whisperInfo.gpu })
                : t("transcribe.cpuOnly")
            }}
          </span>
        </div>
        <span class="text-xs text-text-muted" dir="ltr">
          {{ transcribeStore.whisperInfo.engine }}
        </span>
      </div>

      <!-- Old faster-whisper models: one-time, dismissible clean-up offer -->
      <div
        v-if="transcribeStore.showLegacyNote"
        class="flex items-start gap-3 p-4 rounded-lg bg-bg-secondary border border-border"
      >
        <Info class="w-5 h-5 text-accent shrink-0 mt-0.5" />
        <div class="flex-1 min-w-0 space-y-2">
          <h2 class="text-sm font-semibold text-text-primary">{{ t("transcribe.legacyTitle") }}</h2>
          <p class="text-xs text-text-secondary break-all">
            {{ t("transcribe.legacyBody", { path: transcribeStore.legacyCache }) }}
          </p>
          <div class="flex items-center gap-2">
            <button
              class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-error/10 text-error text-xs font-medium hover:bg-error/20 transition-colors"
              @click="confirmLegacyDelete = true"
            >
              <Trash2 class="w-3.5 h-3.5" />
              {{ t("transcribe.legacyDelete") }}
            </button>
            <button
              class="px-3 py-1.5 rounded-lg text-text-muted text-xs font-medium hover:text-text-secondary transition-colors"
              @click="transcribeStore.dismissLegacyNote()"
            >
              {{ t("transcribe.legacyDismiss") }}
            </button>
          </div>
        </div>
      </div>

      <Modal
        :open="confirmLegacyDelete"
        :title="t('transcribe.legacyConfirmTitle')"
        @close="confirmLegacyDelete = false"
      >
        <p class="text-sm text-text-secondary break-all">
          {{ t("transcribe.legacyConfirmBody", { path: transcribeStore.legacyCache }) }}
        </p>
        <template #footer>
          <button
            class="px-4 py-2 rounded-lg bg-bg-tertiary text-text-primary text-sm font-medium hover:bg-bg-hover transition-colors"
            @click="confirmLegacyDelete = false"
          >
            {{ t("common.cancel") }}
          </button>
          <button
            class="inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-error text-white text-sm font-medium hover:opacity-90 transition-opacity"
            @click="deleteLegacy"
          >
            <Trash2 class="w-4 h-4" />
            {{ t("transcribe.legacyDelete") }}
          </button>
        </template>
      </Modal>

      <!-- File List -->
      <div v-if="fileCount > 0" class="space-y-2">
        <div class="flex items-center justify-between">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
            {{ t("transcribe.files", { count: fileCount }) }}
          </h2>
          <button
            class="text-xs text-text-muted hover:text-text-secondary transition-colors"
            @click="clearFiles"
          >
            {{ t("common.clearAll") }}
          </button>
        </div>
        <div class="space-y-1.5">
          <div
            v-for="(file, i) in selectedFiles"
            :key="file"
            class="flex items-center gap-2 px-3 py-2 rounded-lg bg-bg-secondary border border-border"
          >
            <Mic class="w-4 h-4 text-accent shrink-0" />
            <p class="flex-1 text-sm text-text-primary truncate">
              {{ file.split(/[/\\]/).pop() }}
            </p>
            <button
              class="text-text-muted hover:text-error transition-colors"
              @click="removeFile(i)"
            >
              <X class="w-3.5 h-3.5" />
            </button>
          </div>
        </div>
      </div>

      <!-- Settings -->
      <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
        <div class="space-y-4">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
            {{ t("transcribe.model") }}
          </h2>

          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("transcribe.whisperModel") }}</label>
            <div class="flex items-center gap-2">
              <select
                v-model="selectedModel"
                class="flex-1 px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
              >
                <option v-for="m in transcribeStore.catalog" :key="m.name" :value="m.name">
                  {{ t("transcribe.modelOption", { name: m.name, size: m.size_mb }) }}
                  {{ transcribeStore.downloadedModels.includes(m.name) ? "✓" : "" }}
                </option>
              </select>
              <button
                v-if="!isModelDownloaded && !transcribeStore.download"
                class="inline-flex items-center justify-center w-10 h-10 rounded-lg bg-bg-tertiary text-text-primary hover:bg-bg-hover transition-colors disabled:opacity-50 shrink-0"
                :title="t('transcribe.downloadModel')"
                :aria-label="t('transcribe.downloadModel')"
                @click="transcribeStore.downloadModel(selectedModel)"
              >
                <Download class="w-4 h-4" />
              </button>
            </div>
            <div v-if="transcribeStore.download" class="mt-2 space-y-1.5" role="status">
              <div class="flex items-center gap-2">
                <Loader2 class="w-3.5 h-3.5 text-accent animate-spin shrink-0" />
                <span class="flex-1 text-xs text-text-secondary truncate">
                  {{ t("transcribe.downloading", { model: transcribeStore.download.model, percent: downloadPercent }) }}
                </span>
                <button
                  :disabled="!transcribeStore.download.jobId"
                  class="inline-flex items-center gap-1 px-2 py-1 rounded-md text-xs text-text-muted hover:text-error transition-colors disabled:opacity-50"
                  @click="transcribeStore.cancelDownload()"
                >
                  <X class="w-3.5 h-3.5" />
                  {{ t("transcribe.cancelDownload") }}
                </button>
              </div>
              <div
                class="w-full bg-bg-tertiary rounded-full h-1.5 overflow-hidden"
                role="progressbar"
                :aria-valuenow="downloadPercent"
                aria-valuemin="0"
                aria-valuemax="100"
              >
                <div
                  class="h-full rounded-full bg-accent transition-all duration-300"
                  :style="{ width: `${downloadPercent}%` }"
                />
              </div>
              <p
                v-if="transcribeStore.download.message"
                data-testid="download-message"
                dir="ltr"
                class="text-[11px] text-text-muted truncate"
              >
                {{ transcribeStore.download.message }}
              </p>
            </div>
            <p v-else-if="!isModelDownloaded" class="text-xs text-warning mt-1.5">
              {{ t("transcribe.modelNotCached") }}
            </p>
          </div>

          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("transcribe.device") }}</label>
            <select
              v-model="selectedDevice"
              class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
            >
              <option v-for="d in transcribeStore.deviceOptions" :key="d.value" :value="d.value">
                {{
                  d.value === "auto"
                    ? t("transcribe.autoDetect")
                    : d.value === "cpu"
                      ? t("transcribe.cpu")
                      : t("transcribe.gpu", { name: d.gpuName })
                }}
              </option>
            </select>
          </div>

          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("transcribe.language") }}</label>
            <input
              v-model="language"
              type="text"
              :placeholder="t('transcribe.languagePlaceholder')"
              class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary placeholder-text-muted focus:outline-none focus:border-accent"
            />
          </div>
        </div>

        <div class="space-y-4">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
            {{ t("transcribe.output") }}
          </h2>

          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("transcribe.format") }}</label>
            <select
              v-model="outputFormat"
              class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
            >
              <option value="srt">SRT</option>
              <option value="vtt">VTT</option>
            </select>
          </div>

          <div class="space-y-3">
            <label class="flex items-center gap-2 cursor-pointer">
              <input
                v-model="wordTimestamps"
                type="checkbox"
                class="rounded border-border bg-bg-primary text-accent focus:ring-accent"
              />
              <span class="text-sm text-text-secondary">{{ t("transcribe.wordLevelTimestamps") }}</span>
            </label>
            <label class="flex items-center gap-2 cursor-pointer">
              <input
                v-model="vadFilter"
                type="checkbox"
                class="rounded border-border bg-bg-primary text-accent focus:ring-accent"
              />
              <span class="text-sm text-text-secondary">{{ t("transcribe.vadFilter") }}</span>
            </label>
          </div>
        </div>
      </div>

      <!-- Transcribe Button -->
      <button
        :disabled="fileCount === 0 || transcribing || !isModelDownloaded"
        class="w-full md:w-auto inline-flex items-center justify-center gap-2 px-6 py-3 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover active:bg-accent-active transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        @click="startTranscription"
      >
        <Loader2 v-if="transcribing" class="w-4 h-4 animate-spin" />
        <Play v-else class="w-4 h-4" />
        {{ transcribing ? t("transcribe.starting") : t("transcribe.transcribe") }}
      </button>

      <!-- Jobs -->
      <div v-if="transcribeStore.jobs.length > 0" class="space-y-2">
        <div class="flex items-center justify-between">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
            {{ t("transcribe.transcriptionJobs") }}
          </h2>
          <button
            class="text-xs text-text-muted hover:text-text-secondary transition-colors"
            @click="transcribeStore.clearCompleted"
          >
            {{ t("jobMonitor.clearCompleted") }}
          </button>
        </div>
        <div class="space-y-2">
          <div
            v-for="job in transcribeStore.jobs"
            :key="job.id"
            class="px-4 py-3 rounded-lg bg-bg-secondary border border-border"
          >
            <div class="flex items-center gap-3 mb-2">
              <CheckCircle2 v-if="job.status === JOB_STATUS.COMPLETED" class="w-4 h-4 text-success shrink-0" />
              <XCircle v-else-if="job.status === JOB_STATUS.FAILED" class="w-4 h-4 text-error shrink-0" />
              <Loader2 v-else class="w-4 h-4 text-accent animate-spin shrink-0" />
              <div class="flex-1 min-w-0">
                <p class="text-xs text-text-muted font-mono truncate">{{ job.id.slice(0, 24) }}…</p>
                <p v-if="job.message" class="text-xs text-text-secondary truncate">{{ job.message }}</p>
                <ErrorDetails :details="job.details" />
              </div>
              <button
                class="text-text-muted hover:text-text-secondary transition-colors"
                @click="transcribeStore.removeJob(job.id)"
              >
                <X class="w-4 h-4" />
              </button>
            </div>
            <div class="w-full bg-bg-tertiary rounded-full h-1.5 overflow-hidden">
              <div
                :class="[
                  'h-full rounded-full transition-all duration-300',
                  job.status === JOB_STATUS.FAILED ? 'bg-error' : 'bg-accent',
                ]"
                :style="{ width: `${job.progress}%` }"
              />
            </div>
          </div>
        </div>
      </div>

      <!-- Empty State -->
      <div
        v-if="fileCount === 0 && transcribeStore.jobs.length === 0"
        class="flex flex-col items-center justify-center py-16 text-center"
      >
        <div class="w-16 h-16 rounded-2xl bg-bg-secondary border border-border flex items-center justify-center mb-4">
          <Mic class="w-8 h-8 text-text-muted" />
        </div>
        <h3 class="text-base font-medium text-text-primary">{{ t("transcribe.noMediaSelected") }}</h3>
        <p class="text-sm text-text-secondary mt-1 max-w-sm">
          {{ t("transcribe.noMediaSelectedDesc") }}
        </p>
        <button
          class="mt-4 inline-flex items-center gap-2 px-4 py-2 rounded-lg bg-bg-tertiary text-text-primary text-sm font-medium hover:bg-bg-hover transition-colors"
          @click="pickFiles"
        >
          <FolderOpen class="w-4 h-4" />
          {{ t("common.browse") }}
        </button>
      </div>
    </div>
  </div>
</template>
