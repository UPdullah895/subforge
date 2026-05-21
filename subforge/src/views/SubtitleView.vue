<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { open, save } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { useSubtitleStore } from "../stores/subtitleStore";
import { useAppStore } from "../stores/appStore";
import TrackTable from "../components/TrackTable.vue";
import {
  FolderOpen,
  FileVideo,
  Loader2,
  AlertTriangle,
  CheckCircle2,
  XCircle,
  X,
  Merge,
  ArrowLeftRight,
} from "lucide-vue-next";

const subtitleStore = useSubtitleStore();
const appStore = useAppStore();
const activeTab = ref<"tracks" | "merge" | "convert">("tracks");

const fileName = computed(() => {
  if (!subtitleStore.selectedFile) return null;
  const parts = subtitleStore.selectedFile.split(/[/\\]/);
  return parts[parts.length - 1];
});

const subtitleTracks = computed(() =>
  subtitleStore.tracks.filter((t) => t.track_type === "subtitles")
);

// Merge tab state
const mergeVideoPath = ref("");
const mergeSubtitlePath = ref("");
const mergeLanguage = ref("eng");
const mergeLoading = ref(false);
const mergeError = ref<string | null>(null);
const mergeSuccess = ref(false);

// Convert tab state
const convertInputPath = ref("");
const convertOutputPath = ref("");
const convertFormat = ref("ass");
const shiftMs = ref(0);
const convertLoading = ref(false);
const convertError = ref<string | null>(null);
const convertSuccess = ref(false);

async function pickFile() {
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "Video files", extensions: ["mkv", "mp4", "avi", "mov", "wmv", "flv", "webm"] }],
  });
  if (selected && typeof selected === "string") {
    await subtitleStore.loadTracks(selected);
  }
}

async function extractTrack(track: { id: number; language: string; track_name?: string }) {
  const ext = ".srt";
  const lang = track.language || "und";
  const name = track.track_name || `track_${track.id}`;
  const suggested = `${name}_${lang}${ext}`;

  const outputPath = await save({
    defaultPath: suggested,
    filters: [{ name: "Subtitle files", extensions: ["srt", "ass", "ssa", "vtt"] }],
  });

  if (outputPath && typeof outputPath === "string") {
    await subtitleStore.extractTrack(track.id, outputPath);
  }
}

async function pickMergeVideo() {
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "Video files", extensions: ["mkv", "mp4"] }],
  });
  if (selected && typeof selected === "string") mergeVideoPath.value = selected;
}

async function pickMergeSubtitle() {
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "Subtitle files", extensions: ["srt", "ass", "ssa", "vtt"] }],
  });
  if (selected && typeof selected === "string") mergeSubtitlePath.value = selected;
}

async function runMerge() {
  if (!mergeVideoPath.value || !mergeSubtitlePath.value) return;
  mergeLoading.value = true;
  mergeError.value = null;
  mergeSuccess.value = false;

  try {
    const output = await save({
      defaultPath: "merged.mkv",
      filters: [{ name: "MKV", extensions: ["mkv"] }],
    });
    if (output && typeof output === "string") {
      await invoke("merge_subtitle_into_video", {
        videoPath: mergeVideoPath.value,
        subtitlePath: mergeSubtitlePath.value,
        outputPath: output,
        language: mergeLanguage.value || null,
      });
      mergeSuccess.value = true;
    }
  } catch (e) {
    mergeError.value = String(e);
  } finally {
    mergeLoading.value = false;
  }
}

async function pickConvertInput() {
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "Subtitle files", extensions: ["srt", "ass", "ssa", "vtt"] }],
  });
  if (selected && typeof selected === "string") convertInputPath.value = selected;
}

async function runConvert() {
  if (!convertInputPath.value) return;
  convertLoading.value = true;
  convertError.value = null;
  convertSuccess.value = false;

  try {
    const output = await save({
      defaultPath: `output.${convertFormat.value}`,
      filters: [{ name: "Subtitle files", extensions: ["srt", "ass", "ssa", "vtt"] }],
    });
    if (output && typeof output === "string") {
      await invoke("convert_subtitle_format", {
        input: convertInputPath.value,
        output,
        format: convertFormat.value,
      });
      convertOutputPath.value = output;
      convertSuccess.value = true;
    }
  } catch (e) {
    convertError.value = String(e);
  } finally {
    convertLoading.value = false;
  }
}

async function runShift() {
  if (!convertInputPath.value) return;
  convertLoading.value = true;
  convertError.value = null;
  convertSuccess.value = false;

  try {
    const output = await save({
      defaultPath: "shifted.srt",
      filters: [{ name: "Subtitle files", extensions: ["srt", "ass", "ssa", "vtt"] }],
    });
    if (output && typeof output === "string") {
      await invoke("shift_subtitle_timing", {
        input: convertInputPath.value,
        output,
        milliseconds: shiftMs.value,
      });
      convertOutputPath.value = output;
      convertSuccess.value = true;
    }
  } catch (e) {
    convertError.value = String(e);
  } finally {
    convertLoading.value = false;
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
        <h1 class="text-xl font-semibold text-text-primary">Subtitles</h1>
        <p class="text-sm text-text-secondary mt-0.5">
          Extract, merge, and convert subtitle tracks
        </p>
      </div>
      <button
        @click="pickFile"
        class="inline-flex items-center gap-2 px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover active:bg-accent-active transition-colors"
      >
        <FolderOpen class="w-4 h-4" />
        Open Video
      </button>
    </div>

    <!-- Tabs -->
    <div class="flex items-center gap-1 px-6 pt-4 border-b border-border">
      <button
        @click="activeTab = 'tracks'"
        :class="[
          'px-4 py-2 text-sm font-medium rounded-t-lg transition-colors',
          activeTab === 'tracks'
            ? 'bg-bg-secondary text-accent border-b-2 border-accent'
            : 'text-text-secondary hover:text-text-primary',
        ]"
      >
        Tracks
      </button>
      <button
        @click="activeTab = 'merge'"
        :class="[
          'px-4 py-2 text-sm font-medium rounded-t-lg transition-colors',
          activeTab === 'merge'
            ? 'bg-bg-secondary text-accent border-b-2 border-accent'
            : 'text-text-secondary hover:text-text-primary',
        ]"
      >
        Merge
      </button>
      <button
        @click="activeTab = 'convert'"
        :class="[
          'px-4 py-2 text-sm font-medium rounded-t-lg transition-colors',
          activeTab === 'convert'
            ? 'bg-bg-secondary text-accent border-b-2 border-accent'
            : 'text-text-secondary hover:text-text-primary',
        ]"
      >
        Convert & Retime
      </button>
    </div>

    <!-- Content -->
    <div class="flex-1 overflow-auto p-6 space-y-6">
      <!-- Tool Status -->
      <div
        v-if="appStore.toolRegistry"
        class="flex items-center gap-4 px-4 py-3 rounded-lg bg-bg-secondary border border-border"
      >
        <div class="flex items-center gap-2">
          <div :class="['w-2 h-2 rounded-full', appStore.toolRegistry.mkvmerge.available ? 'bg-success' : 'bg-error']" />
          <span class="text-xs text-text-secondary">mkvmerge</span>
        </div>
        <div class="flex items-center gap-2">
          <div :class="['w-2 h-2 rounded-full', appStore.toolRegistry.mkvextract.available ? 'bg-success' : 'bg-error']" />
          <span class="text-xs text-text-secondary">mkvextract</span>
        </div>
        <div class="flex items-center gap-2">
          <div :class="['w-2 h-2 rounded-full', appStore.toolRegistry.ffmpeg.available ? 'bg-success' : 'bg-error']" />
          <span class="text-xs text-text-secondary">ffmpeg</span>
        </div>
      </div>

      <!-- Tracks Tab -->
      <template v-if="activeTab === 'tracks'">
        <div
          v-if="fileName"
          class="flex items-center gap-3 px-4 py-3 rounded-lg bg-bg-secondary border border-border"
        >
          <FileVideo class="w-5 h-5 text-accent shrink-0" />
          <div class="min-w-0">
            <p class="text-sm font-medium text-text-primary truncate">{{ fileName }}</p>
            <p class="text-xs text-text-muted">
              {{ subtitleTracks.length }} subtitle · {{ subtitleStore.tracks.filter(t => t.track_type === 'audio').length }} audio · {{ subtitleStore.tracks.filter(t => t.track_type === 'video').length }} video
            </p>
          </div>
        </div>

        <div v-if="subtitleStore.loading" class="flex items-center justify-center py-12">
          <Loader2 class="w-6 h-6 text-accent animate-spin" />
          <span class="ml-3 text-sm text-text-secondary">Reading tracks...</span>
        </div>

        <div v-else-if="subtitleStore.error" class="flex items-start gap-3 px-4 py-4 rounded-lg bg-error/10 border border-error/20">
          <AlertTriangle class="w-5 h-5 text-error shrink-0 mt-0.5" />
          <div>
            <p class="text-sm font-medium text-error">Failed to read file</p>
            <p class="text-xs text-error/80 mt-1">{{ subtitleStore.error }}</p>
          </div>
        </div>

        <div v-else-if="subtitleStore.tracks.length > 0">
          <div class="flex items-center justify-between mb-3">
            <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">Tracks</h2>
            <span class="text-xs text-text-muted">{{ subtitleStore.tracks.length }} total</span>
          </div>
          <TrackTable :tracks="subtitleStore.tracks" @extract="extractTrack" />
        </div>

        <div v-else class="flex flex-col items-center justify-center py-16 text-center">
          <div class="w-16 h-16 rounded-2xl bg-bg-secondary border border-border flex items-center justify-center mb-4">
            <FileVideo class="w-8 h-8 text-text-muted" />
          </div>
          <h3 class="text-base font-medium text-text-primary">No file selected</h3>
          <p class="text-sm text-text-secondary mt-1 max-w-sm">Open a video file to enumerate its tracks and extract subtitles.</p>
          <button @click="pickFile" class="mt-4 inline-flex items-center gap-2 px-4 py-2 rounded-lg bg-bg-tertiary text-text-primary text-sm font-medium hover:bg-bg-hover transition-colors">
            <FolderOpen class="w-4 h-4" /> Browse
          </button>
        </div>

        <div v-if="subtitleStore.extractJobs.length > 0" class="space-y-2">
          <div class="flex items-center justify-between">
            <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">Recent Jobs</h2>
            <button @click="subtitleStore.clearJobs" class="text-xs text-text-muted hover:text-text-secondary">Clear</button>
          </div>
          <div class="space-y-2">
            <div v-for="job in subtitleStore.extractJobs" :key="job.id" class="flex items-center gap-3 px-4 py-3 rounded-lg bg-bg-secondary border border-border">
              <CheckCircle2 v-if="job.status === 'completed'" class="w-4 h-4 text-success shrink-0" />
              <XCircle v-else-if="job.status === 'failed'" class="w-4 h-4 text-error shrink-0" />
              <Loader2 v-else class="w-4 h-4 text-accent animate-spin shrink-0" />
              <div class="flex-1 min-w-0">
                <p class="text-sm text-text-primary">Track #{{ job.trackId }}</p>
                <p v-if="job.message" class="text-xs text-text-secondary truncate">{{ job.message }}</p>
              </div>
              <button @click="subtitleStore.extractJobs = subtitleStore.extractJobs.filter((j) => j.id !== job.id)" class="text-text-muted hover:text-text-secondary">
                <X class="w-4 h-4" />
              </button>
            </div>
          </div>
        </div>
      </template>

      <!-- Merge Tab -->
      <template v-if="activeTab === 'merge'">
        <div class="max-w-xl space-y-4">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">Merge Subtitle into Video</h2>

          <div class="space-y-3">
            <div>
              <label class="block text-xs font-medium text-text-secondary mb-1.5">Video file</label>
              <button @click="pickMergeVideo" class="flex items-center gap-2 w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary hover:bg-bg-hover transition-colors text-left">
                <FileVideo class="w-4 h-4 text-text-muted" />
                <span class="truncate">{{ mergeVideoPath || "Select video file..." }}</span>
              </button>
            </div>

            <div>
              <label class="block text-xs font-medium text-text-secondary mb-1.5">Subtitle file</label>
              <button @click="pickMergeSubtitle" class="flex items-center gap-2 w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary hover:bg-bg-hover transition-colors text-left">
                <Merge class="w-4 h-4 text-text-muted" />
                <span class="truncate">{{ mergeSubtitlePath || "Select subtitle file..." }}</span>
              </button>
            </div>

            <div>
              <label class="block text-xs font-medium text-text-secondary mb-1.5">Language code</label>
              <input v-model="mergeLanguage" type="text" placeholder="eng" class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary placeholder-text-muted focus:outline-none focus:border-accent" />
            </div>

            <button
              @click="runMerge"
              :disabled="!mergeVideoPath || !mergeSubtitlePath || mergeLoading"
              class="w-full inline-flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover active:bg-accent-active transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <Loader2 v-if="mergeLoading" class="w-4 h-4 animate-spin" />
              <Merge v-else class="w-4 h-4" />
              Merge into MKV
            </button>

            <div v-if="mergeError" class="flex items-start gap-3 px-4 py-3 rounded-lg bg-error/10 border border-error/20">
              <AlertTriangle class="w-4 h-4 text-error shrink-0 mt-0.5" />
              <p class="text-xs text-error">{{ mergeError }}</p>
            </div>
            <div v-if="mergeSuccess" class="flex items-start gap-3 px-4 py-3 rounded-lg bg-success/10 border border-success/20">
              <CheckCircle2 class="w-4 h-4 text-success shrink-0 mt-0.5" />
              <p class="text-xs text-success">Merge completed successfully.</p>
            </div>
          </div>
        </div>
      </template>

      <!-- Convert Tab -->
      <template v-if="activeTab === 'convert'">
        <div class="max-w-xl space-y-6">
          <div class="space-y-4">
            <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">Convert Format</h2>

            <div>
              <label class="block text-xs font-medium text-text-secondary mb-1.5">Subtitle file</label>
              <button @click="pickConvertInput" class="flex items-center gap-2 w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary hover:bg-bg-hover transition-colors text-left">
                <ArrowLeftRight class="w-4 h-4 text-text-muted" />
                <span class="truncate">{{ convertInputPath || "Select subtitle file..." }}</span>
              </button>
            </div>

            <div>
              <label class="block text-xs font-medium text-text-secondary mb-1.5">Target format</label>
              <select v-model="convertFormat" class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary focus:outline-none focus:border-accent">
                <option value="srt">SRT</option>
                <option value="ass">ASS</option>
                <option value="ssa">SSA</option>
                <option value="vtt">VTT</option>
              </select>
            </div>

            <button
              @click="runConvert"
              :disabled="!convertInputPath || convertLoading"
              class="w-full inline-flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover active:bg-accent-active transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <Loader2 v-if="convertLoading" class="w-4 h-4 animate-spin" />
              <ArrowLeftRight v-else class="w-4 h-4" />
              Convert
            </button>
          </div>

          <div class="border-t border-border pt-6 space-y-4">
            <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">Retime / Shift</h2>

            <div>
              <label class="block text-xs font-medium text-text-secondary mb-1.5">Shift milliseconds</label>
              <input
                v-model.number="shiftMs"
                type="number"
                step="100"
                class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
              />
              <p class="text-xs text-text-muted mt-1">Positive = forward, Negative = backward</p>
            </div>

            <button
              @click="runShift"
              :disabled="!convertInputPath || convertLoading"
              class="w-full inline-flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg bg-bg-tertiary text-text-primary text-sm font-medium hover:bg-bg-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <Loader2 v-if="convertLoading" class="w-4 h-4 animate-spin" />
              Shift Timing
            </button>
          </div>

          <div v-if="convertError" class="flex items-start gap-3 px-4 py-3 rounded-lg bg-error/10 border border-error/20">
            <AlertTriangle class="w-4 h-4 text-error shrink-0 mt-0.5" />
            <p class="text-xs text-error">{{ convertError }}</p>
          </div>
          <div v-if="convertSuccess" class="flex items-start gap-3 px-4 py-3 rounded-lg bg-success/10 border border-success/20">
            <CheckCircle2 class="w-4 h-4 text-success shrink-0 mt-0.5" />
            <p class="text-xs text-success">Operation completed. Saved to {{ convertOutputPath }}</p>
          </div>
        </div>
      </template>
    </div>
  </div>
</template>
