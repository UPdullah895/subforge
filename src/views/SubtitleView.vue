<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { open, save } from "../utils/dialog";
import { apiInvoke } from "../api/client";
import { startAndWait } from "../api/jobs";
import { baseName, dirOf, fileStem, joinPath, planSubtitleFilenames } from "../utils/subtitleNaming";
import { useToastStore } from "../stores/toastStore";
import { useSubtitleStore } from "../stores/subtitleStore";
import { useAppStore } from "../stores/appStore";

const { t } = useI18n();
const toastStore = useToastStore();
import TrackTable from "../components/TrackTable.vue";
import ErrorDetails from "../components/ErrorDetails.vue";
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
  Info,
} from "lucide-vue-next";

const subtitleStore = useSubtitleStore();
const appStore = useAppStore();
const activeTab = ref<"tracks" | "merge" | "convert">("tracks");
const selectedFiles = ref<string[]>([]);
const activeFile = ref<string | null>(null);
const batchLoading = ref(false);

const fileCount = computed(() => selectedFiles.value.length);

const activeFileName = computed(() => {
  if (!activeFile.value) return null;
  const parts = activeFile.value.split(/[/\\]/);
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

async function pickFiles() {
  const selected = await open({
    multiple: true,
    directory: false,
    filters: [{ name: t("subtitle.openVideos"), extensions: ["mkv", "mp4", "avi", "mov", "wmv", "flv", "webm"] }],
  });
  if (Array.isArray(selected)) {
    selectedFiles.value.push(...selected);
  } else if (selected && typeof selected === "string") {
    selectedFiles.value.push(selected);
  }
  if (selectedFiles.value.length > 0 && !activeFile.value) {
    activeFile.value = selectedFiles.value[0];
    await subtitleStore.loadTracks(activeFile.value);
  }
}

async function selectFile(path: string) {
  activeFile.value = path;
  await subtitleStore.loadTracks(path);
}

function removeFile(index: number) {
  const removed = selectedFiles.value[index];
  selectedFiles.value.splice(index, 1);
  if (activeFile.value === removed) {
    activeFile.value = selectedFiles.value[0] || null;
    if (activeFile.value) {
      subtitleStore.loadTracks(activeFile.value);
    } else {
      subtitleStore.selectedFile = null;
      subtitleStore.tracks = [];
    }
  }
}

function clearFiles() {
  selectedFiles.value = [];
  activeFile.value = null;
  subtitleStore.selectedFile = null;
  subtitleStore.tracks = [];
}

async function extractTrack(track: { id: number; language: string; track_name?: string }) {
  if (!activeFile.value) return;
  // `{video}.{lang}[.sdh|.forced].srt` next to the video: the same convention
  // batch extraction uses, so media servers pick the file up and extracting
  // from several episodes never collides.
  const plan = planSubtitleFilenames(fileStem(activeFile.value), subtitleTracks.value);
  const suggested = joinPath(dirOf(activeFile.value), plan.get(track.id) ?? `track_${track.id}.srt`);

  const outputPath = await save({
    defaultPath: suggested,
    filters: [{ name: "Subtitle files", extensions: ["srt", "ass", "ssa", "vtt"] }],
  });

  if (outputPath && typeof outputPath === "string") {
    await subtitleStore.extractTrack(track.id, outputPath);
  }
}

async function batchExtractAllSubtitles() {
  if (selectedFiles.value.length === 0) return;
  batchLoading.value = true;
  for (const file of selectedFiles.value) {
    await subtitleStore.loadTracks(file);
    const plan = planSubtitleFilenames(fileStem(file), subtitleTracks.value);
    for (const track of subtitleTracks.value) {
      const output = joinPath(dirOf(file), plan.get(track.id) ?? `track_${track.id}.srt`);
      await subtitleStore.extractTrack(track.id, output);
    }
  }
  batchLoading.value = false;
}

async function pickMergeVideo() {
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [{ name: t("subtitle.videoFile"), extensions: ["mkv", "mp4"] }],
  });
  if (selected && typeof selected === "string") mergeVideoPath.value = selected;
}

async function pickMergeSubtitle() {
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [{ name: t("subtitle.subtitleFile"), extensions: ["srt", "ass", "ssa", "vtt"] }],
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
      // Next to the video, e.g. "Show - S01E03.merged.mkv".
      defaultPath: joinPath(dirOf(mergeVideoPath.value), `${fileStem(mergeVideoPath.value)}.merged.mkv`),
      filters: [{ name: "MKV", extensions: ["mkv"] }],
      title: t("subtitle.mergeSubtitle"),
    });
    if (output && typeof output === "string") {
      // The merge runs in the background; report its real outcome rather than
      // claiming success the moment it was accepted.
      const result = await startAndWait(() =>
        apiInvoke<string>("merge_subtitle_into_video", {
          videoPath: mergeVideoPath.value,
          subtitlePath: mergeSubtitlePath.value,
          outputPath: output,
          language: mergeLanguage.value || null,
        })
      );
      if (result.status === "completed") mergeSuccess.value = true;
      else if (result.status === "cancelled") mergeError.value = t("toast.jobCancelled");
      else mergeError.value = result.message || t("toast.jobFailed");
    }
  } catch (e) {
    mergeError.value = e instanceof Error ? e.message : String(e);
  } finally {
    mergeLoading.value = false;
  }
}

async function pickConvertInput() {
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [{ name: t("subtitle.subtitleFileLabel"), extensions: ["srt", "ass", "ssa", "vtt"] }],
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
      // `Show.eng.srt` -> `Show.eng.ass`, in the same folder.
      defaultPath: joinPath(dirOf(convertInputPath.value), `${fileStem(convertInputPath.value)}.${convertFormat.value}`),
      filters: [{ name: t("subtitle.subtitleFileLabel"), extensions: ["srt", "ass", "ssa", "vtt"] }],
      title: t("subtitle.convertFormat"),
    });
    if (output && typeof output === "string") {
      await apiInvoke("convert_subtitle_format", {
        input: convertInputPath.value,
        output,
        format: convertFormat.value,
      });
      convertOutputPath.value = output;
      convertSuccess.value = true;
      toastStore.success(t("subtitle.convertDone"), baseName(output));
    }
  } catch (e) {
    convertError.value = e instanceof Error ? e.message : String(e);
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
      // Same folder and name as the input: a retimed subtitle belongs next to
      // its video under the name media servers expect (the dialog asks before
      // replacing).
      defaultPath: convertInputPath.value,
      filters: [{ name: "Subtitle files", extensions: ["srt", "ass", "ssa", "vtt"] }],
    });
    if (output && typeof output === "string") {
      await apiInvoke("shift_subtitle_timing", {
        input: convertInputPath.value,
        output,
        milliseconds: shiftMs.value,
      });
      convertOutputPath.value = output;
      convertSuccess.value = true;
      toastStore.success(t("subtitle.retimeDone"), baseName(output));
    }
  } catch (e) {
    convertError.value = e instanceof Error ? e.message : String(e);
  } finally {
    convertLoading.value = false;
  }
}

// Frame rates the subtitle/video pair most often disagree on.
const FPS_PRESETS = [23.976, 24, 25, 29.97, 30];
const stretchFromFps = ref(25);
const stretchToFps = ref(23.976);

async function runStretch() {
  if (!convertInputPath.value || stretchFromFps.value === stretchToFps.value) return;
  convertLoading.value = true;
  convertError.value = null;
  convertSuccess.value = false;

  try {
    const output = await save({
      defaultPath: convertInputPath.value,
      filters: [{ name: "Subtitle files", extensions: ["srt", "ass", "ssa", "vtt"] }],
    });
    if (output && typeof output === "string") {
      await apiInvoke("stretch_subtitle_timing", {
        input: convertInputPath.value,
        output,
        fromFps: stretchFromFps.value,
        toFps: stretchToFps.value,
      });
      convertOutputPath.value = output;
      convertSuccess.value = true;
      toastStore.success(t("subtitle.retimeDone"), baseName(output));
    }
  } catch (e) {
    convertError.value = e instanceof Error ? e.message : String(e);
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
      <div class="flex items-center gap-2">
        <h1 class="text-xl font-semibold text-text-primary">{{ t("subtitle.title") }}</h1>
        <span
          class="text-text-muted hover:text-text-secondary cursor-help"
          :title="t('subtitle.tooltip')"
        >
          <Info class="w-4 h-4" />
        </span>
      </div>
      <button
        class="inline-flex items-center gap-2 px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover active:bg-accent-active transition-colors"
        @click="pickFiles"
      >
        <FolderOpen class="w-4 h-4" />
        {{ t("subtitle.openVideos") }}
      </button>
    </div>

    <!-- Tabs -->
    <div class="flex items-center gap-1 px-6 pt-4 border-b border-border">
      <button
        :class="[
          'px-4 py-2 text-sm font-medium rounded-t-lg transition-colors',
          activeTab === 'tracks'
            ? 'bg-bg-secondary text-accent border-b-2 border-accent'
            : 'text-text-secondary hover:text-text-primary',
        ]"
        @click="activeTab = 'tracks'"
      >
        {{ t("subtitle.tabs.tracks") }}
      </button>
      <button
        :class="[
          'px-4 py-2 text-sm font-medium rounded-t-lg transition-colors',
          activeTab === 'merge'
            ? 'bg-bg-secondary text-accent border-b-2 border-accent'
            : 'text-text-secondary hover:text-text-primary',
        ]"
        @click="activeTab = 'merge'"
      >
        {{ t("subtitle.tabs.merge") }}
      </button>
      <button
        :class="[
          'px-4 py-2 text-sm font-medium rounded-t-lg transition-colors',
          activeTab === 'convert'
            ? 'bg-bg-secondary text-accent border-b-2 border-accent'
            : 'text-text-secondary hover:text-text-primary',
        ]"
        @click="activeTab = 'convert'"
      >
        {{ t("subtitle.tabs.convert") }}
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
        <!-- File List -->
        <div v-if="fileCount > 0" class="space-y-3">
          <div class="flex items-center justify-between">
            <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
              {{ t("subtitle.filesCount", { count: fileCount }) }}
            </h2>
            <div class="flex items-center gap-2">
              <button
                :disabled="batchLoading"
                class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors disabled:opacity-50"
                @click="batchExtractAllSubtitles"
              >
                <FileVideo class="w-3.5 h-3.5" />
                {{ t("subtitle.extractAllSubtitles") }}
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
              <FileVideo class="w-4 h-4 text-accent shrink-0" />
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
          <FileVideo class="w-5 h-5 text-accent shrink-0" />
          <div class="min-w-0">
            <p class="text-sm font-medium text-text-primary truncate">{{ activeFileName }}</p>
            <p class="text-xs text-text-muted">
              {{ subtitleTracks.length }} subtitle · {{ subtitleStore.tracks.filter(t => t.track_type === 'audio').length }} audio · {{ subtitleStore.tracks.filter(t => t.track_type === 'video').length }} video
            </p>
          </div>
        </div>

        <div v-if="subtitleStore.loading || batchLoading" class="flex items-center justify-center py-12">
          <Loader2 class="w-6 h-6 text-accent animate-spin" />
          <span class="ml-3 text-sm text-text-secondary">{{ t("common.processing") }}</span>
        </div>

        <div v-else-if="subtitleStore.error" class="flex items-start gap-3 px-4 py-4 rounded-lg bg-error/10 border border-error/20">
          <AlertTriangle class="w-5 h-5 text-error shrink-0 mt-0.5" />
          <div>
            <p class="text-sm font-medium text-error">{{ t("common.failedToReadFile") }}</p>
            <p class="text-xs text-error/80 mt-1">{{ subtitleStore.error }}</p>
            <ErrorDetails :details="subtitleStore.errorDetails" />
          </div>
        </div>

        <div v-else-if="subtitleStore.tracks.length > 0">
          <div class="flex items-center justify-between mb-3">
            <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">{{ t("subtitle.tracksHeading") }}</h2>
            <span class="text-xs text-text-muted">{{ t("subtitle.totalTracks", { count: subtitleStore.tracks.length }) }}</span>
          </div>
          <TrackTable :tracks="subtitleStore.tracks" @extract="extractTrack" />
        </div>

        <div v-else class="flex flex-col items-center justify-center py-16 text-center">
          <div class="w-16 h-16 rounded-2xl bg-bg-secondary border border-border flex items-center justify-center mb-4">
            <FileVideo class="w-8 h-8 text-text-muted" />
          </div>
          <h3 class="text-base font-medium text-text-primary">{{ t("common.noFileSelected") }}</h3>
          <p class="text-sm text-text-secondary mt-1 max-w-sm">{{ t("subtitle.noFileSelectedDesc") }}</p>
          <button class="mt-4 inline-flex items-center gap-2 px-4 py-2 rounded-lg bg-bg-tertiary text-text-primary text-sm font-medium hover:bg-bg-hover transition-colors" @click="pickFiles">
            <FolderOpen class="w-4 h-4" /> {{ t("common.browse") }}
          </button>
        </div>

        <div v-if="subtitleStore.extractJobs.length > 0" class="space-y-2">
          <div class="flex items-center justify-between">
            <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">{{ t("subtitle.recentJobs") }}</h2>
            <button class="text-xs text-text-muted hover:text-text-secondary" @click="subtitleStore.clearJobs">{{ t("common.clear") }}</button>
          </div>
          <div class="space-y-2">
            <div v-for="job in subtitleStore.extractJobs" :key="job.id" class="flex items-center gap-3 px-4 py-3 rounded-lg bg-bg-secondary border border-border">
              <CheckCircle2 v-if="job.status === 'completed'" class="w-4 h-4 text-success shrink-0" />
              <XCircle v-else-if="job.status === 'failed'" class="w-4 h-4 text-error shrink-0" />
              <Loader2 v-else class="w-4 h-4 text-accent animate-spin shrink-0" />
              <div class="flex-1 min-w-0">
                <p class="text-sm text-text-primary">{{ t("subtitle.trackJob", { id: job.trackId }) }}</p>
                <p v-if="job.message" class="text-xs text-text-secondary truncate">{{ job.message }}</p>
                <ErrorDetails :details="job.details" />
              </div>
              <button class="text-text-muted hover:text-text-secondary" @click="subtitleStore.extractJobs = subtitleStore.extractJobs.filter((j) => j.id !== job.id)">
                <X class="w-4 h-4" />
              </button>
            </div>
          </div>
        </div>
      </template>

      <!-- Merge Tab -->
      <template v-if="activeTab === 'merge'">
        <div class="max-w-xl space-y-4">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">{{ t("subtitle.mergeSubtitle") }}</h2>

          <div class="space-y-3">
            <div>
              <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("subtitle.videoFile") }}</label>
              <button class="flex items-center gap-2 w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary hover:bg-bg-hover transition-colors text-left" @click="pickMergeVideo">
                <FileVideo class="w-4 h-4 text-text-muted" />
                <span class="truncate" dir="auto" :title="mergeVideoPath">{{ mergeVideoPath ? baseName(mergeVideoPath) : t("subtitle.selectVideoFile") }}</span>
              </button>
            </div>

            <div>
              <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("subtitle.subtitleFile") }}</label>
              <button class="flex items-center gap-2 w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary hover:bg-bg-hover transition-colors text-left" @click="pickMergeSubtitle">
                <Merge class="w-4 h-4 text-text-muted" />
                <span class="truncate" dir="auto" :title="mergeSubtitlePath">{{ mergeSubtitlePath ? baseName(mergeSubtitlePath) : t("subtitle.selectSubtitleFile") }}</span>
              </button>
            </div>

            <div>
              <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("subtitle.languageCode") }}</label>
              <input v-model="mergeLanguage" type="text" placeholder="eng" class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary placeholder-text-muted focus:outline-none focus:border-accent" />
            </div>

            <button
              :disabled="!mergeVideoPath || !mergeSubtitlePath || mergeLoading"
              class="w-full inline-flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover active:bg-accent-active transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              @click="runMerge"
            >
              <Loader2 v-if="mergeLoading" class="w-4 h-4 animate-spin" />
              <Merge v-else class="w-4 h-4" />
              {{ t("subtitle.mergeIntoMkv") }}
            </button>

            <div v-if="mergeError" class="flex items-start gap-3 px-4 py-3 rounded-lg bg-error/10 border border-error/20">
              <AlertTriangle class="w-4 h-4 text-error shrink-0 mt-0.5" />
              <p class="text-xs text-error">{{ mergeError }}</p>
            </div>
            <div v-if="mergeSuccess" class="flex items-start gap-3 px-4 py-3 rounded-lg bg-success/10 border border-success/20">
              <CheckCircle2 class="w-4 h-4 text-success shrink-0 mt-0.5" />
              <p class="text-xs text-success">{{ t("subtitle.mergCompletedSuccessfully") }}</p>
            </div>
          </div>
        </div>
      </template>

      <!-- Convert Tab -->
      <template v-if="activeTab === 'convert'">
        <div class="max-w-xl space-y-6">
          <div class="space-y-4">
            <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">{{ t("subtitle.convertFormat") }}</h2>

            <div>
              <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("subtitle.subtitleFileLabel") }}</label>
              <button class="flex items-center gap-2 w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary hover:bg-bg-hover transition-colors text-left" @click="pickConvertInput">
                <ArrowLeftRight class="w-4 h-4 text-text-muted" />
                <span class="truncate" dir="auto" :title="convertInputPath">{{ convertInputPath ? baseName(convertInputPath) : t("subtitle.selectSubtitleFile") }}</span>
              </button>
            </div>

            <div>
              <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("subtitle.targetFormat") }}</label>
              <select v-model="convertFormat" class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary focus:outline-none focus:border-accent">
                <option value="srt">SRT</option>
                <option value="ass">ASS</option>
                <option value="ssa">SSA</option>
                <option value="vtt">VTT</option>
              </select>
            </div>

            <button
              :disabled="!convertInputPath || convertLoading"
              class="w-full inline-flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover active:bg-accent-active transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              @click="runConvert"
            >
              <Loader2 v-if="convertLoading" class="w-4 h-4 animate-spin" />
              <ArrowLeftRight v-else class="w-4 h-4" />
              {{ t("subtitle.convert") }}
            </button>
          </div>

          <div class="border-t border-border pt-6 space-y-4">
            <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">{{ t("subtitle.retimeShift") }}</h2>

            <div>
              <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("subtitle.shiftMilliseconds") }}</label>
              <input
                v-model.number="shiftMs"
                type="number"
                step="100"
                class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
              />
              <p class="text-xs text-text-muted mt-1">{{ t("subtitle.shiftHint") }}</p>
            </div>

            <button
              :disabled="!convertInputPath || convertLoading"
              class="w-full inline-flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg bg-bg-tertiary text-text-primary text-sm font-medium hover:bg-bg-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              @click="runShift"
            >
              <Loader2 v-if="convertLoading" class="w-4 h-4 animate-spin" />
              {{ t("subtitle.shiftTiming") }}
            </button>
          </div>

          <!-- Frame-rate drift: starts in sync, ends up seconds or minutes off -->
          <div class="border-t border-border pt-6 space-y-4">
            <div>
              <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
                {{ t("subtitle.fixDriftTitle") }}
              </h2>
              <p class="text-xs text-text-muted mt-1">{{ t("subtitle.fixDriftHint") }}</p>
            </div>

            <div class="grid grid-cols-2 gap-3">
              <div>
                <label class="block text-xs font-medium text-text-secondary mb-1.5" for="stretch-from">
                  {{ t("subtitle.fpsFrom") }}
                </label>
                <select
                  id="stretch-from"
                  v-model.number="stretchFromFps"
                  class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
                >
                  <option v-for="fps in FPS_PRESETS" :key="fps" :value="fps">{{ fps }} fps</option>
                </select>
              </div>
              <div>
                <label class="block text-xs font-medium text-text-secondary mb-1.5" for="stretch-to">
                  {{ t("subtitle.fpsTo") }}
                </label>
                <select
                  id="stretch-to"
                  v-model.number="stretchToFps"
                  class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
                >
                  <option v-for="fps in FPS_PRESETS" :key="fps" :value="fps">{{ fps }} fps</option>
                </select>
              </div>
            </div>
            <p v-if="stretchFromFps === stretchToFps" class="text-xs text-warning">{{ t("subtitle.fpsSame") }}</p>

            <button
              :disabled="!convertInputPath || convertLoading || stretchFromFps === stretchToFps"
              class="w-full inline-flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg bg-bg-tertiary text-text-primary text-sm font-medium hover:bg-bg-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              @click="runStretch"
            >
              <Loader2 v-if="convertLoading" class="w-4 h-4 animate-spin" />
              {{ t("subtitle.fixDriftButton") }}
            </button>
          </div>

          <div v-if="convertError" class="flex items-start gap-3 px-4 py-3 rounded-lg bg-error/10 border border-error/20">
            <AlertTriangle class="w-4 h-4 text-error shrink-0 mt-0.5" />
            <p class="text-xs text-error">{{ convertError }}</p>
          </div>
          <div v-if="convertSuccess" class="flex items-start gap-3 px-4 py-3 rounded-lg bg-success/10 border border-success/20">
            <CheckCircle2 class="w-4 h-4 text-success shrink-0 mt-0.5" />
            <p class="text-xs text-success">{{ t("subtitle.operationCompleted", { path: convertOutputPath }) }}</p>
          </div>
        </div>
      </template>
    </div>
  </div>
</template>
