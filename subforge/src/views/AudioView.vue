<script setup lang="ts">
import { computed, onMounted } from "vue";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useAudioStore } from "../stores/audioStore";
import { useAppStore } from "../stores/appStore";
import AudioTrackTable from "../components/AudioTrackTable.vue";
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
} from "lucide-vue-next";

const audioStore = useAudioStore();
const appStore = useAppStore();

const fileName = computed(() => {
  if (!audioStore.selectedFile) return null;
  const parts = audioStore.selectedFile.split(/[/\\]/);
  return parts[parts.length - 1];
});

async function pickFile() {
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [
      { name: "Video files", extensions: ["mkv", "mp4", "avi", "mov", "wmv", "flv", "webm"] },
    ],
  });
  if (selected && typeof selected === "string") {
    await audioStore.loadTracks(selected);
  }
}

async function extractTrack(track: { id: number; codec: string; language: string }) {
  const format = "aac";
  const lang = track.language || "und";
  const suggested = `audio_${track.id}_${lang}.${format}`;

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
    defaultPath: "merged.mkv",
    filters: [{ name: "MKV", extensions: ["mkv"] }],
  });

  if (outputPath && typeof outputPath === "string") {
    await audioStore.mergeAudio(video, audio, outputPath);
  }
}

async function removeAllAudio() {
  const outputPath = await save({
    defaultPath: fileName.value?.replace(/\.[^.]+$/, "_no_audio.mkv") || "output.mkv",
    filters: [{ name: "MKV", extensions: ["mkv"] }],
  });
  if (outputPath && typeof outputPath === "string") {
    await audioStore.removeAllAudio(outputPath);
  }
}

onMounted(() => {
  appStore.discoverTools();
});
</script>

<template>
  <div class="flex flex-col h-full">
    <!-- Header -->
    <div class="flex items-center justify-between px-6 py-4 border-b border-border">
      <div>
        <h1 class="text-xl font-semibold text-text-primary">Audio</h1>
        <p class="text-sm text-text-secondary mt-0.5">
          Extract, merge, and remove audio tracks
        </p>
      </div>
      <div class="flex items-center gap-2">
        <button
          @click="mergeAudio"
          class="inline-flex items-center gap-2 px-4 py-2.5 rounded-lg bg-bg-tertiary text-text-primary text-sm font-medium hover:bg-bg-hover transition-colors"
        >
          <Merge class="w-4 h-4" />
          Merge Audio
        </button>
        <button
          @click="pickFile"
          class="inline-flex items-center gap-2 px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover active:bg-accent-active transition-colors"
        >
          <FolderOpen class="w-4 h-4" />
          Open Video
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
            ffmpeg {{ appStore.toolRegistry.ffmpeg.available ? "" : "not found" }}
          </span>
        </div>
      </div>

      <!-- File Info -->
      <div
        v-if="fileName"
        class="flex items-center gap-3 px-4 py-3 rounded-lg bg-bg-secondary border border-border"
      >
        <Volume2 class="w-5 h-5 text-accent shrink-0" />
        <div class="min-w-0">
          <p class="text-sm font-medium text-text-primary truncate">{{ fileName }}</p>
          <p class="text-xs text-text-muted">{{ audioStore.tracks.length }} audio tracks</p>
        </div>
        <button
          @click="removeAllAudio"
          class="ml-auto inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md bg-error/10 text-error text-xs font-medium hover:bg-error hover:text-white transition-colors"
        >
          <VolumeX class="w-3.5 h-3.5" />
          Remove All
        </button>
      </div>

      <!-- Loading -->
      <div v-if="audioStore.loading" class="flex items-center justify-center py-12">
        <Loader2 class="w-6 h-6 text-accent animate-spin" />
        <span class="ml-3 text-sm text-text-secondary">Reading tracks...</span>
      </div>

      <!-- Error -->
      <div
        v-else-if="audioStore.error"
        class="flex items-start gap-3 px-4 py-4 rounded-lg bg-error/10 border border-error/20"
      >
        <AlertTriangle class="w-5 h-5 text-error shrink-0 mt-0.5" />
        <div>
          <p class="text-sm font-medium text-error">Failed to read file</p>
          <p class="text-xs text-error/80 mt-1">{{ audioStore.error }}</p>
        </div>
      </div>

      <!-- Tracks -->
      <div v-else-if="audioStore.tracks.length > 0">
        <div class="flex items-center justify-between mb-3">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
            Audio Tracks
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
        <h3 class="text-base font-medium text-text-primary">No file selected</h3>
        <p class="text-sm text-text-secondary mt-1 max-w-sm">
          Open a video file to view and extract its audio tracks.
        </p>
        <button
          @click="pickFile"
          class="mt-4 inline-flex items-center gap-2 px-4 py-2 rounded-lg bg-bg-tertiary text-text-primary text-sm font-medium hover:bg-bg-hover transition-colors"
        >
          <FolderOpen class="w-4 h-4" />
          Browse
        </button>
      </div>

      <!-- Jobs -->
      <div v-if="audioStore.jobs.length > 0" class="space-y-2">
        <div class="flex items-center justify-between">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
            Recent Jobs
          </h2>
          <button
            @click="audioStore.clearJobs"
            class="text-xs text-text-muted hover:text-text-secondary transition-colors"
          >
            Clear
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
            </div>
            <button
              @click="audioStore.jobs = audioStore.jobs.filter((j) => j.id !== job.id)"
              class="text-text-muted hover:text-text-secondary transition-colors"
            >
              <X class="w-4 h-4" />
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
