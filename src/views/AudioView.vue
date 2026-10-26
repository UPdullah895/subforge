<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { open, save } from "../utils/dialog";
import { dirOf, fileStem, joinPath } from "../utils/subtitleNaming";
import { useAudioStore } from "../stores/audioStore";
import DropOverlay from "../components/DropOverlay.vue";
import { useFileDrop } from "../composables/useFileDrop";
import { splitFilesAndFolders } from "../utils/paths";
import { useToastStore } from "../stores/toastStore";
import { useAppStore } from "../stores/appStore";

const { t } = useI18n();
import AudioTrackTable from "../components/AudioTrackTable.vue";
import ErrorDetails from "../components/ErrorDetails.vue";
import {
  FolderOpen,
  Loader2,
  AlertTriangle,
  Volume2,
  Merge,
  VolumeX,
  CheckCircle2,
  XCircle,
  X,
  Info,
  Play,
} from "lucide-vue-next";

const audioStore = useAudioStore();
const toastStore = useToastStore();
const appStore = useAppStore();
const selectedFiles = ref<string[]>([]);
const activeFile = ref<string | null>(null);
const batchLoading = ref(false);

const fileCount = computed(() => selectedFiles.value.length);

const activeFileName = computed(() => {
  if (!activeFile.value) return null;
  const parts = activeFile.value.split(/[/\\]/);
  return parts[parts.length - 1];
});

const { dragging: dropActive } = useFileDrop({
  extensions: ["mkv", "mp4", "avi", "mov", "wmv", "flv", "webm"],
  onDrop: onFilesDropped,
});

async function onFilesDropped(dropped: string[]) {
  const { files, folders } = await splitFilesAndFolders(dropped);
  if (folders.length > 0) toastStore.info(t("drop.folderNotSupported"));
  for (const path of files) {
    if (!selectedFiles.value.includes(path)) selectedFiles.value.push(path);
  }
  if (!activeFile.value && selectedFiles.value.length > 0) {
    activeFile.value = selectedFiles.value[0];
    await audioStore.loadTracks(activeFile.value);
  }
}

async function pickFiles() {
  const selected = await open({
    multiple: true,
    directory: false,
    filters: [
      { name: "Video files", extensions: ["mkv", "mp4", "avi", "mov", "wmv", "flv", "webm"] },
    ],
  });
  if (Array.isArray(selected)) {
    selectedFiles.value.push(...selected);
  } else if (selected && typeof selected === "string") {
    selectedFiles.value.push(selected);
  }
  // Auto-load tracks for the first file if none active
  if (selectedFiles.value.length > 0 && !activeFile.value) {
    activeFile.value = selectedFiles.value[0];
    await audioStore.loadTracks(activeFile.value);
  }
}

async function selectFile(path: string) {
  activeFile.value = path;
  await audioStore.loadTracks(path);
}

function removeFile(index: number) {
  const removed = selectedFiles.value[index];
  selectedFiles.value.splice(index, 1);
  if (activeFile.value === removed) {
    activeFile.value = selectedFiles.value[0] || null;
    if (activeFile.value) {
      audioStore.loadTracks(activeFile.value);
    } else {
      audioStore.selectedFile = null;
      audioStore.tracks = [];
    }
  }
}

function clearFiles() {
  selectedFiles.value = [];
  activeFile.value = null;
  audioStore.selectedFile = null;
  audioStore.tracks = [];
}

async function extractTrack(track: { id: number; codec: string; language: string }) {
  if (!activeFile.value) return;
  const format = "aac";
  const lang = track.language || "und";
  // Next to the video as `{video}.{lang}.aac`, so it can't clobber another
  // episode's audio and sorts beside its source.
  const suggested = joinPath(dirOf(activeFile.value), `${fileStem(activeFile.value)}.${lang}.${format}`);

  const outputPath = await save({
    defaultPath: suggested,
    filters: [
      { name: "Audio files", extensions: ["aac", "mp3", "opus", "flac", "wav"] },
    ],
  });

  if (outputPath && typeof outputPath === "string") {
    const ext = outputPath.split(".").pop()?.toLowerCase();
    await audioStore.extractTrack(track.id, outputPath, ext || format);
  }
}

async function mergeAudio() {
  const video = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "Video files", extensions: ["mkv", "mp4", "avi"] }],
  });
  if (!video || typeof video !== "string") return;

  const audio = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "Audio files", extensions: ["aac", "mp3", "opus", "flac", "wav"] }],
  });
  if (!audio || typeof audio !== "string") return;

  const outputPath = await save({
    defaultPath: joinPath(dirOf(video), `${fileStem(video)}.merged.mkv`),
    filters: [{ name: "MKV", extensions: ["mkv"] }],
  });

  if (outputPath && typeof outputPath === "string") {
    await audioStore.mergeAudio(video, audio, outputPath);
  }
}

async function batchExtractAllAudio() {
  if (selectedFiles.value.length === 0) return;
  batchLoading.value = true;
  for (const file of selectedFiles.value) {
    await audioStore.loadTracks(file);
    for (const track of audioStore.tracks) {
      const lastSlash = Math.max(file.lastIndexOf("/"), file.lastIndexOf("\\"));
      const dir = file.substring(0, lastSlash + 1);
      const base = file.substring(lastSlash + 1).replace(/\.[^.]+$/, "");
      const lang = track.language || "und";
      const output = `${dir}${base}_audio_${track.id}_${lang}.aac`;
      await audioStore.extractTrack(track.id, output, "aac");
    }
  }
  batchLoading.value = false;
}

async function batchRemoveAllAudio() {
  if (selectedFiles.value.length === 0) return;
  batchLoading.value = true;
  for (const file of selectedFiles.value) {
    const lastSlash = Math.max(file.lastIndexOf("/"), file.lastIndexOf("\\"));
    const dir = file.substring(0, lastSlash + 1);
    const base = file.substring(lastSlash + 1).replace(/\.[^.]+$/, "");
    const output = `${dir}${base}_noaudio.mkv`;
    await audioStore.loadTracks(file);
    await audioStore.removeAllAudio(output);
  }
  batchLoading.value = false;
}

onMounted(() => {
  appStore.discoverTools();
});
</script>

<template>
  <div class="relative flex flex-col h-full">
    <DropOverlay :show="dropActive" :label="t('drop.audioHint')" />
    <!-- Header -->
    <div class="flex items-center justify-between px-6 py-4 border-b border-border">
      <div class="flex items-center gap-2">
        <h1 class="text-xl font-semibold text-text-primary">{{ t("audio.title") }}</h1>
        <span
          class="text-text-muted hover:text-text-secondary cursor-help"
          :title="t('audio.tooltip')"
        >
          <Info class="w-4 h-4" />
        </span>
      </div>
      <div class="flex items-center gap-2">
        <button
          class="inline-flex items-center gap-2 px-4 py-2.5 rounded-lg bg-bg-tertiary text-text-primary text-sm font-medium hover:bg-bg-hover transition-colors"
          @click="mergeAudio"
        >
          <Merge class="w-4 h-4" />
          {{ t("audio.mergeAudio") }}
        </button>
        <button
          class="inline-flex items-center gap-2 px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover active:bg-accent-active transition-colors"
          @click="pickFiles"
        >
          <FolderOpen class="w-4 h-4" />
          {{ t("audio.openVideos") }}
        </button>
      </div>
    </div>

    <!-- Content -->
    <div class="flex-1 overflow-auto p-6 space-y-6">
      <!-- Tool Status -->
      <div
        v-if="appStore.toolRegistry"
        class="flex items-center gap-4 px-4 py-3 rounded-lg bg-bg-secondary border border-border"
      >
        <div class="flex items-center gap-2">
          <div
            :class="[
              'w-2 h-2 rounded-full',
              appStore.toolRegistry.ffmpeg.available ? 'bg-success' : 'bg-error',
            ]"
          />
          <span class="text-xs text-text-secondary">
            {{ appStore.toolRegistry.ffmpeg.available ? "ffmpeg" : t("audio.notFound") }}
          </span>
        </div>
      </div>

      <!-- File List -->
      <div v-if="fileCount > 0" class="space-y-3">
        <div class="flex items-center justify-between">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
            {{ t("audio.filesCount", { count: fileCount }) }}
          </h2>
          <div class="flex items-center gap-2">
            <button
              :disabled="batchLoading"
              class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors disabled:opacity-50"
              @click="batchExtractAllAudio"
            >
              <Play class="w-3.5 h-3.5" />
              {{ t("audio.extractAllAudio") }}
            </button>
            <button
              :disabled="batchLoading"
              class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-error/10 text-error text-xs font-medium hover:bg-error hover:text-white transition-colors disabled:opacity-50"
              @click="batchRemoveAllAudio"
            >
              <VolumeX class="w-3.5 h-3.5" />
              {{ t("audio.removeAllAudio") }}
            </button>
            <button
              class="text-xs text-text-muted hover:text-text-secondary transition-colors"
              @click="clearFiles"
            >
              {{ t("common.clearAll") }}
            </button>
          </div>
        </div>
        <div class="space-y-1.5">
          <div
            v-for="(file, i) in selectedFiles"
            :key="file"
            :class="[
              'flex items-center gap-2 px-3 py-2 rounded-lg border cursor-pointer transition-colors',
              activeFile === file
                ? 'border-accent bg-accent/10'
                : 'border-border bg-bg-secondary hover:bg-bg-hover',
            ]"
            @click="selectFile(file)"
          >
            <Volume2 class="w-4 h-4 text-accent shrink-0" />
            <p class="flex-1 text-sm text-text-primary truncate">
              {{ file.split(/[/\\]/).pop() }}
            </p>
            <button
              class="text-text-muted hover:text-error transition-colors"
              @click.stop="removeFile(i)"
            >
              <X class="w-3.5 h-3.5" />
            </button>
          </div>
        </div>
      </div>

      <!-- Active File Info -->
      <div
        v-if="activeFileName"
        class="flex items-center gap-3 px-4 py-3 rounded-lg bg-bg-secondary border border-border"
      >
        <Volume2 class="w-5 h-5 text-accent shrink-0" />
        <div class="min-w-0">
          <p class="text-sm font-medium text-text-primary truncate">{{ activeFileName }}</p>
          <p class="text-xs text-text-muted">{{ t("audio.trackCount", { count: audioStore.tracks.length }) }}</p>
        </div>
      </div>

      <!-- Loading -->
      <div v-if="audioStore.loading || batchLoading" class="flex items-center justify-center py-12">
        <Loader2 class="w-6 h-6 text-accent animate-spin" />
        <span class="ml-3 text-sm text-text-secondary">{{ t("common.processing") }}</span>
      </div>

      <!-- Error -->
      <div
        v-else-if="audioStore.error"
        class="flex items-start gap-3 px-4 py-4 rounded-lg bg-error/10 border border-error/20"
      >
        <AlertTriangle class="w-5 h-5 text-error shrink-0 mt-0.5" />
        <div>
          <p class="text-sm font-medium text-error">{{ t("common.failedToReadFile") }}</p>
          <p class="text-xs text-error/80 mt-1">{{ audioStore.error }}</p>
          <ErrorDetails :details="audioStore.errorDetails" />
        </div>
      </div>

      <!-- Tracks -->
      <div v-else-if="audioStore.tracks.length > 0">
        <div class="flex items-center justify-between mb-3">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
            {{ t("audio.audioTracks") }}
          </h2>
        </div>
        <AudioTrackTable :tracks="audioStore.tracks" @extract="extractTrack" />
      </div>

      <!-- Empty State -->
      <div
        v-else
        class="flex flex-col items-center justify-center py-16 text-center"
      >
        <div
          class="w-16 h-16 rounded-2xl bg-bg-secondary border border-border flex items-center justify-center mb-4"
        >
          <Volume2 class="w-8 h-8 text-text-muted" />
        </div>
        <h3 class="text-base font-medium text-text-primary">{{ t("common.noFileSelected") }}</h3>
        <p class="text-sm text-text-secondary mt-1 max-w-sm">
          {{ t("audio.noFileSelectedDesc") }}
        </p>
        <button
          class="mt-4 inline-flex items-center gap-2 px-4 py-2 rounded-lg bg-bg-tertiary text-text-primary text-sm font-medium hover:bg-bg-hover transition-colors"
          @click="pickFiles"
        >
          <FolderOpen class="w-4 h-4" />
          {{ t("common.browse") }}
        </button>
      </div>

      <!-- Jobs -->
      <div v-if="audioStore.jobs.length > 0" class="space-y-2">
        <div class="flex items-center justify-between">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
            {{ t("audio.recentJobs") }}
          </h2>
          <button
            class="text-xs text-text-muted hover:text-text-secondary transition-colors"
            @click="audioStore.clearJobs"
          >
            {{ t("common.clear") }}
          </button>
        </div>
        <div class="space-y-2">
          <div
            v-for="job in audioStore.jobs"
            :key="job.id"
            class="flex items-center gap-3 px-4 py-3 rounded-lg bg-bg-secondary border border-border"
          >
            <CheckCircle2 v-if="job.status === 'completed'" class="w-4 h-4 text-success shrink-0" />
            <XCircle v-else-if="job.status === 'failed'" class="w-4 h-4 text-error shrink-0" />
            <Loader2 v-else class="w-4 h-4 text-accent animate-spin shrink-0" />
            <div class="flex-1 min-w-0">
              <p class="text-sm text-text-primary capitalize">{{ job.type }}</p>
              <p v-if="job.message" class="text-xs text-text-secondary truncate">
                {{ job.message }}
              </p>
              <ErrorDetails :details="job.details" />
            </div>
            <button
              class="text-text-muted hover:text-text-secondary transition-colors"
              @click="audioStore.jobs = audioStore.jobs.filter((j) => j.id !== job.id)"
            >
              <X class="w-4 h-4" />
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
