<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { open } from "@tauri-apps/plugin-dialog";

import { apiListen } from "../api/events";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { useTranscribeStore } from "../stores/transcribeStore";
import { JOB_STATUS, type JobStatus } from "../constants/jobStatus";
import { useAppStore } from "../stores/appStore";
import ErrorBanner from "../components/ErrorBanner.vue";
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

const fileCount = computed(() => selectedFiles.value.length);

const isModelDownloaded = computed(() =>
  transcribeStore.downloadedModels.includes(selectedModel.value)
);

function buildOutputPath(inputPath: string, format: string): string {
  const lastSlash = Math.max(inputPath.lastIndexOf("/"), inputPath.lastIndexOf("\\"));
  const dir = inputPath.substring(0, lastSlash + 1);
  const name = inputPath.substring(lastSlash + 1);
  const base = name.replace(/\.[^.]+$/, "");
  return `${dir}${base}.${format}`;
}

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
      transcribeStore.completeJob(job_id, status as JobStatus, message);
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
      <div class="flex items-center gap-2">
        <h1 class="text-xl font-semibold text-text-primary">Speech to Text</h1>
        <span
          class="text-text-muted hover:text-text-secondary cursor-help"
          title="Transcribe audio and video to subtitles using faster-whisper. Choose a model, set options, and run."
        >
          <Info class="w-4 h-4" />
        </span>
      </div>
      <button
        class="inline-flex items-center gap-2 px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover active:bg-accent-active transition-colors"
        @click="pickFiles"
      >
        <FolderOpen class="w-4 h-4" />
        Open Media
      </button>
    </div>

    <!-- Content -->
    <div class="flex-1 overflow-auto p-6 space-y-6">
      <ErrorBanner :message="transcribeStore.error" @dismiss="transcribeStore.error = null" />

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

      <!-- File List -->
      <div v-if="fileCount > 0" class="space-y-2">
        <div class="flex items-center justify-between">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
            Files ({{ fileCount }})
          </h2>
          <button
            class="text-xs text-text-muted hover:text-text-secondary transition-colors"
            @click="clearFiles"
          >
            Clear all
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
                class="inline-flex items-center justify-center w-10 h-10 rounded-lg bg-bg-tertiary text-text-primary hover:bg-bg-hover transition-colors disabled:opacity-50 shrink-0"
                title="Download this model"
                @click="transcribeStore.downloadModel(selectedModel)"
              >
                <Loader2 v-if="transcribeStore.downloadingModel === selectedModel" class="w-4 h-4 animate-spin" />
                <Download v-else class="w-4 h-4" />
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
        :disabled="fileCount === 0 || transcribing"
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
              <CheckCircle2 v-if="job.status === JOB_STATUS.COMPLETED" class="w-4 h-4 text-success shrink-0" />
              <XCircle v-else-if="job.status === JOB_STATUS.FAILED" class="w-4 h-4 text-error shrink-0" />
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
        <h3 class="text-base font-medium text-text-primary">No media selected</h3>
        <p class="text-sm text-text-secondary mt-1 max-w-sm">
          Open an audio or video file to transcribe it to subtitles.
        </p>
        <button
          class="mt-4 inline-flex items-center gap-2 px-4 py-2 rounded-lg bg-bg-tertiary text-text-primary text-sm font-medium hover:bg-bg-hover transition-colors"
          @click="pickFiles"
        >
          <FolderOpen class="w-4 h-4" />
          Browse
        </button>
      </div>
    </div>
  </div>
</template>
