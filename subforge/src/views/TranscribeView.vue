<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { open, save } from "@tauri-apps/plugin-dialog";

import { apiListen } from "../api/events";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { useTranscribeStore } from "../stores/transcribeStore";
import { useAppStore } from "../stores/appStore";
import {
  FolderOpen,
  Mic,
  Loader2,
  CheckCircle2,
  Download,



  X,
  XCircle,
  Play,
} from "lucide-vue-next";

const transcribeStore = useTranscribeStore();
const appStore = useAppStore();

const selectedFile = ref("");
const selectedModel = ref("tiny");
const selectedDevice = ref("auto");
const language = ref("");
const outputFormat = ref("srt");
const wordTimestamps = ref(false);
const vadFilter = ref(true);
const transcribing = ref(false);

const fileName = computed(() => {
  if (!selectedFile.value) return null;
  const parts = selectedFile.value.split(/[/\\]/);
  return parts[parts.length - 1];
});

const isModelDownloaded = computed(() =>
  transcribeStore.downloadedModels.includes(selectedModel.value)
);

async function pickFile() {
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [
      { name: "Media files", extensions: ["mkv", "mp4", "avi", "mov", "wmv", "flv", "webm", "mp3", "wav", "flac", "aac", "m4a"] },
    { name: "Audio files", extensions: ["mp3", "wav", "flac", "aac", "m4a", "ogg", "opus"] },
    { name: "Video files", extensions: ["mkv", "mp4", "avi", "mov", "wmv", "flv", "webm"] },
    ],
  });
  if (selected && typeof selected === "string") {
    selectedFile.value = selected;
  }
}

async function startTranscription() {
  if (!selectedFile.value) return;

  const output = await save({
    defaultPath: `transcription.${outputFormat.value}`,
    filters: [{ name: "Subtitle files", extensions: ["srt", "vtt"] }],
  });

  if (!output || typeof output !== "string") return;

  transcribing.value = true;
  await transcribeStore.startTranscription({
    input: selectedFile.value,
    output,
    model: selectedModel.value,
    device: selectedDevice.value,
    language: language.value || undefined,
    format: outputFormat.value,
    wordTimestamps: wordTimestamps.value,
    vadFilter: vadFilter.value,
  });
  transcribing.value = false;
}

let unlistenProgress: UnlistenFn | null = null;
let unlistenCompleted: UnlistenFn | null = null;

onMounted(async () => {
  appStore.discoverTools();
  await transcribeStore.loadInfo();
  await transcribeStore.loadModels();

  unlistenProgress = await apiListen<{ job_id: string; progress: number; message?: string }>(
    "job-progress",
    ({ job_id, progress, message }) => {
      transcribeStore.updateJobProgress(job_id, progress, message);
    }
  );

  unlistenCompleted = await apiListen<{ job_id: string; status: string; message?: string }>(
    "job-completed",
    ({ job_id, status, message }) => {
      transcribeStore.completeJob(job_id, status, message);
    }
  );
});

onUnmounted(() => {
  if (unlistenProgress) unlistenProgress();
  if (unlistenCompleted) unlistenCompleted();
});
</script>

<template>
  <div class="flex flex-col h-full">
    <!-- Header -->
    <div class="flex items-center justify-between px-6 py-4 border-b border-border">
      <div>
        <h1 class="text-xl font-semibold text-text-primary">Speech to Text</h1>
        <p class="text-sm text-text-secondary mt-0.5">
          Transcribe audio and video to subtitles with Whisper
        </p>
      </div>
      <button
        class="inline-flex items-center gap-2 px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover active:bg-accent-active transition-colors"
        @click="pickFile"
      >
        <FolderOpen class="w-4 h-4" />
        Open Media
      </button>
    </div>

    <!-- Content -->
    <div class="flex-1 overflow-auto p-6 space-y-6">
      <!-- Environment Info -->
      <div
        v-if="transcribeStore.whisperInfo"
        class="flex items-center gap-4 px-4 py-3 rounded-lg bg-bg-secondary border border-border"
      >
        <div class="flex items-center gap-2">
          <div
            :class="[
              'w-2 h-2 rounded-full',
              transcribeStore.whisperInfo.cuda_available ? 'bg-success' : 'bg-warning',
            ]"
          />
          <span class="text-xs text-text-secondary">
            {{ transcribeStore.whisperInfo.cuda_available ? "CUDA available" : "CPU only" }}
          </span>
        </div>
        <span class="text-xs text-text-muted">
          faster-whisper {{ transcribeStore.whisperInfo.faster_whisper_version }}
        </span>
      </div>

      <!-- File Info -->
      <div
        v-if="fileName"
        class="flex items-center gap-3 px-4 py-3 rounded-lg bg-bg-secondary border border-border"
      >
        <Mic class="w-5 h-5 text-accent shrink-0" />
        <div class="min-w-0">
          <p class="text-sm font-medium text-text-primary truncate">{{ fileName }}</p>
          <p class="text-xs text-text-muted">Ready to transcribe</p>
        </div>
      </div>

      <!-- Settings -->
      <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
        <div class="space-y-4">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
            Model
          </h2>

          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">Whisper model</label>
            <div class="flex items-center gap-2">
              <select
                v-model="selectedModel"
                class="flex-1 px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
              >
                <option v-for="m in transcribeStore.allModels" :key="m" :value="m">
                  {{ m }} {{ transcribeStore.downloadedModels.includes(m) ? "✓" : "" }}
                </option>
              </select>
              <button
                v-if="!isModelDownloaded"
                :disabled="transcribeStore.downloadingModel === selectedModel"
                class="inline-flex items-center gap-1.5 px-3 py-2.5 rounded-lg bg-bg-tertiary text-text-primary text-xs font-medium hover:bg-bg-hover transition-colors disabled:opacity-50"
                @click="transcribeStore.downloadModel(selectedModel)"
              >
                <Loader2 v-if="transcribeStore.downloadingModel === selectedModel" class="w-3.5 h-3.5 animate-spin" />
                <Download v-else class="w-3.5 h-3.5" />
                Download
              </button>
            </div>
            <p v-if="!isModelDownloaded" class="text-xs text-warning mt-1.5">
              This model is not cached. It will be downloaded on first use (may take several minutes for large models).
            </p>
          </div>

          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">Device</label>
            <select
              v-model="selectedDevice"
              class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
            >
              <option value="auto">Auto-detect</option>
              <option value="cpu">CPU</option>
              <option value="cuda" :disabled="!transcribeStore.whisperInfo?.cuda_available">CUDA</option>
            </select>
          </div>

          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">Language</label>
            <input
              v-model="language"
              type="text"
              placeholder="Auto-detect (e.g. en, ja, de)"
              class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary placeholder-text-muted focus:outline-none focus:border-accent"
            />
          </div>
        </div>

        <div class="space-y-4">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
            Output
          </h2>

          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">Format</label>
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
              <span class="text-sm text-text-secondary">Word-level timestamps</span>
            </label>
            <label class="flex items-center gap-2 cursor-pointer">
              <input
                v-model="vadFilter"
                type="checkbox"
                class="rounded border-border bg-bg-primary text-accent focus:ring-accent"
              />
              <span class="text-sm text-text-secondary">Voice Activity Detection filter</span>
            </label>
          </div>
        </div>
      </div>

      <!-- Transcribe Button -->
      <button
        :disabled="!selectedFile || transcribing"
        class="w-full md:w-auto inline-flex items-center justify-center gap-2 px-6 py-3 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover active:bg-accent-active transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        @click="startTranscription"
      >
        <Loader2 v-if="transcribing" class="w-4 h-4 animate-spin" />
        <Play v-else class="w-4 h-4" />
        {{ transcribing ? "Starting..." : "Transcribe" }}
      </button>

      <!-- Jobs -->
      <div v-if="transcribeStore.jobs.length > 0" class="space-y-2">
        <div class="flex items-center justify-between">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
            Transcription Jobs
          </h2>
          <button
            class="text-xs text-text-muted hover:text-text-secondary transition-colors"
            @click="transcribeStore.clearCompleted"
          >
            Clear completed
          </button>
        </div>
        <div class="space-y-2">
          <div
            v-for="job in transcribeStore.jobs"
            :key="job.id"
            class="px-4 py-3 rounded-lg bg-bg-secondary border border-border"
          >
            <div class="flex items-center gap-3 mb-2">
              <CheckCircle2 v-if="job.status === 'completed'" class="w-4 h-4 text-success shrink-0" />
              <XCircle v-else-if="job.status === 'failed'" class="w-4 h-4 text-error shrink-0" />
              <Loader2 v-else class="w-4 h-4 text-accent animate-spin shrink-0" />
              <div class="flex-1 min-w-0">
                <p class="text-xs text-text-muted font-mono truncate">{{ job.id.slice(0, 24) }}…</p>
                <p v-if="job.message" class="text-xs text-text-secondary truncate">{{ job.message }}</p>
              </div>
              <button
                class="text-text-muted hover:text-text-secondary transition-colors"
                @click="transcribeStore.jobs = transcribeStore.jobs.filter((j) => j.id !== job.id)"
              >
                <X class="w-4 h-4" />
              </button>
            </div>
            <div class="w-full bg-bg-tertiary rounded-full h-1.5 overflow-hidden">
              <div
                :class="[
                  'h-full rounded-full transition-all duration-300',
                  job.status === 'failed' ? 'bg-error' : 'bg-accent',
                ]"
                :style="{ width: `${job.progress}%` }"
              />
            </div>
          </div>
        </div>
      </div>

      <!-- Empty State -->
      <div
        v-if="!selectedFile && transcribeStore.jobs.length === 0"
        class="flex flex-col items-center justify-center py-16 text-center"
      >
        <div class="w-16 h-16 rounded-2xl bg-bg-secondary border border-border flex items-center justify-center mb-4">
          <Mic class="w-8 h-8 text-text-muted" />
        </div>
        <h3 class="text-base font-medium text-text-primary">No media selected</h3>
        <p class="text-sm text-text-secondary mt-1 max-w-sm">
          Open an audio or video file to transcribe it to subtitles.
        </p>
        <button
          class="mt-4 inline-flex items-center gap-2 px-4 py-2 rounded-lg bg-bg-tertiary text-text-primary text-sm font-medium hover:bg-bg-hover transition-colors"
          @click="pickFile"
        >
          <FolderOpen class="w-4 h-4" />
          Browse
        </button>
      </div>
    </div>
  </div>
</template>
