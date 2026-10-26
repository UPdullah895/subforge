<script setup lang="ts">
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { apiInvoke } from "../api/client";
import { open } from "../utils/dialog";
import { baseName } from "../utils/subtitleNaming";
import {
  AlertTriangle,
  CheckCircle2,
  Clock,
  FileVideo,
  FolderOpen,
  Loader2,
  Mic,
  Wand2,
} from "lucide-vue-next";

const { t } = useI18n();

interface FpsFix {
  from: number;
  to: number;
}
interface SyncIssue {
  kind: string;
  message: string;
}
interface SyncCheck {
  verdict: "good" | "offset" | "framerate" | "truncated" | "uncertain";
  cue_count: number;
  subtitle_start_ms: number;
  subtitle_end_ms: number;
  video_duration_ms: number | null;
  video_fps: number | null;
  suggested_fix: FpsFix | null;
  suggested_shift_ms: number | null;
  issues: SyncIssue[];
}
interface DeepCheck {
  verdict: SyncCheck["verdict"];
  matches: {
    subtitle_start_ms: number;
    asr_start_ms: number;
    offset_ms: number;
    score: number;
    subtitle_text: string;
  }[];
  offset_ms: number | null;
  suggested_shift_ms: number | null;
  suggested_fix: FpsFix | null;
  reason: string;
}
interface SyncResponse {
  check: SyncCheck;
  deep: DeepCheck | null;
  model: string | null;
}

const subtitlePath = ref("");
const videoPath = ref("");
const checking = ref(false);
const deepening = ref(false);
const error = ref<string | null>(null);
const result = ref<SyncResponse | null>(null);
const overwrite = ref(false);
const fixing = ref(false);
const fixedPath = ref<string | null>(null);

const verdictLabel = computed(() => {
  const verdict = result.value?.deep?.verdict ?? result.value?.check.verdict;
  return verdict ? t(`subtitle.sync.verdict_${verdict}`) : "";
});

const suggested = computed(() => {
  const deep = result.value?.deep;
  const check = result.value?.check;
  const fix = deep?.suggested_fix ?? check?.suggested_fix;
  const shift = deep?.suggested_shift_ms ?? check?.suggested_shift_ms;
  return {
    from: fix?.from ?? null,
    to: fix?.to ?? null,
    shift: shift ?? null,
  };
});

const hasFix = computed(
  () => suggested.value.shift !== null || (suggested.value.from !== null && suggested.value.to !== null)
);

const verdictTone = computed(() => {
  const verdict = result.value?.deep?.verdict ?? result.value?.check.verdict;
  if (verdict === "good") return "text-success";
  if (verdict === "uncertain") return "text-text-secondary";
  return "text-warning";
});

function seconds(ms: number | null | undefined): string {
  if (ms === null || ms === undefined) return "—";
  return `${(ms / 1000).toFixed(1)} s`;
}

async function pickSubtitle() {
  const selected = await open({
    multiple: false,
    title: t("subtitle.clean.pickFile"),
    filters: [{ name: "Subtitles", extensions: ["srt", "ass", "ssa", "vtt"] }],
  });
  if (typeof selected === "string") {
    subtitlePath.value = selected;
    result.value = null;
    fixedPath.value = null;
  }
}

async function pickVideo() {
  const selected = await open({
    multiple: false,
    title: t("subtitle.openVideos"),
    filters: [{ name: "Video", extensions: ["mkv", "mp4", "avi", "mov", "wmv", "flv", "webm"] }],
  });
  if (typeof selected === "string") {
    videoPath.value = selected;
    result.value = null;
    fixedPath.value = null;
  }
}

async function run(deep: boolean) {
  if (!subtitlePath.value || !videoPath.value) return;
  const flag = deep ? deepening : checking;
  flag.value = true;
  error.value = null;
  fixedPath.value = null;
  try {
    result.value = await apiInvoke<SyncResponse>("check_subtitle_sync", {
      request: { subtitle: subtitlePath.value, video: videoPath.value, deep },
    });
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    flag.value = false;
  }
}

async function applyFix() {
  if (!result.value || !hasFix.value) return;
  fixing.value = true;
  error.value = null;
  try {
    const response = await apiInvoke<{ output_path: string }>("fix_subtitle_sync", {
      request: {
        input: subtitlePath.value,
        output: null,
        from_fps: suggested.value.from,
        to_fps: suggested.value.to,
        shift_ms: suggested.value.shift,
        overwrite: overwrite.value,
      },
    });
    fixedPath.value = response.output_path;
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    fixing.value = false;
  }
}
</script>

<template>
  <div class="max-w-3xl space-y-6">
    <div class="space-y-1">
      <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
        {{ t("subtitle.sync.title") }}
      </h2>
      <p class="text-xs text-text-muted">{{ t("subtitle.sync.desc") }}</p>
    </div>

    <div class="space-y-3">
      <div>
        <label class="block text-xs font-medium text-text-secondary mb-1.5">
          {{ t("subtitle.subtitleFileLabel") }}
        </label>
        <button
          class="flex items-center gap-2 w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary hover:bg-bg-hover transition-colors text-left"
          data-testid="sync-pick-subtitle"
          @click="pickSubtitle"
        >
          <FolderOpen class="w-4 h-4 text-text-muted" />
          <span class="truncate" dir="auto" :title="subtitlePath">
            {{ subtitlePath ? baseName(subtitlePath) : t("subtitle.clean.pickFile") }}
          </span>
        </button>
      </div>
      <div>
        <label class="block text-xs font-medium text-text-secondary mb-1.5">
          {{ t("subtitle.sync.videoFile") }}
        </label>
        <button
          class="flex items-center gap-2 w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary hover:bg-bg-hover transition-colors text-left"
          data-testid="sync-pick-video"
          @click="pickVideo"
        >
          <FileVideo class="w-4 h-4 text-text-muted" />
          <span class="truncate" dir="auto" :title="videoPath">
            {{ videoPath ? baseName(videoPath) : t("subtitle.sync.pickVideo") }}
          </span>
        </button>
      </div>
    </div>

    <div class="flex flex-wrap items-center gap-3">
      <button
        :disabled="!subtitlePath || !videoPath || checking"
        class="inline-flex items-center gap-2 px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        data-testid="sync-check"
        @click="run(false)"
      >
        <Loader2 v-if="checking" class="w-4 h-4 animate-spin" />
        <Clock v-else class="w-4 h-4" />
        {{ checking ? t("subtitle.sync.checking") : t("subtitle.sync.check") }}
      </button>
      <button
        :disabled="!subtitlePath || !videoPath || deepening"
        class="inline-flex items-center gap-2 px-4 py-2.5 rounded-lg bg-bg-tertiary text-text-primary text-sm font-medium hover:bg-bg-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        data-testid="sync-deep"
        :title="t('subtitle.sync.deepHint')"
        @click="run(true)"
      >
        <Loader2 v-if="deepening" class="w-4 h-4 animate-spin" />
        <Mic v-else class="w-4 h-4" />
        {{ deepening ? t("subtitle.sync.listening") : t("subtitle.sync.deep") }}
      </button>
    </div>
    <p class="text-xs text-text-muted -mt-3">{{ t("subtitle.sync.deepHint") }}</p>

    <div v-if="result" class="rounded-lg border border-border bg-bg-secondary p-4 space-y-3" data-testid="sync-result">
      <div class="flex items-center justify-between">
        <span class="text-sm font-medium" :class="verdictTone">{{ verdictLabel }}</span>
        <span class="text-xs text-text-muted">
          {{ t("subtitle.sync.range", {
            start: seconds(result.check.subtitle_start_ms),
            end: seconds(result.check.subtitle_end_ms),
            duration: seconds(result.check.video_duration_ms),
          }) }}
        </span>
      </div>

      <ul v-if="result.check.issues.length" class="space-y-1">
        <li
          v-for="issue in result.check.issues"
          :key="issue.kind"
          class="flex items-start gap-2 text-xs text-text-secondary"
        >
          <AlertTriangle class="w-3.5 h-3.5 text-warning shrink-0 mt-0.5" />
          <span dir="auto">{{ issue.message }}</span>
        </li>
      </ul>

      <div v-if="result.deep" class="border-t border-border pt-3 space-y-2">
        <p class="text-xs text-text-secondary">
          {{ t("subtitle.sync.deepResult", {
            model: result.model,
            count: result.deep.matches.length,
            offset: seconds(result.deep.offset_ms),
          }) }}
        </p>
        <table v-if="result.deep.matches.length" class="w-full text-xs">
          <tbody>
            <tr v-for="match in result.deep.matches.slice(0, 5)" :key="match.subtitle_start_ms">
              <td class="py-1 pr-3 text-text-muted whitespace-nowrap">
                {{ seconds(match.subtitle_start_ms) }}
              </td>
              <td class="py-1 pr-3 text-text-primary" dir="auto">{{ match.subtitle_text }}</td>
              <td class="py-1 text-right" :class="Math.abs(match.offset_ms) < 400 ? 'text-success' : 'text-warning'">
                {{ match.offset_ms > 0 ? "+" : "" }}{{ seconds(match.offset_ms) }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <div v-if="hasFix" class="border-t border-border pt-3 space-y-3">
        <p class="text-xs text-text-secondary">
          {{ t("subtitle.sync.fixSummary", {
            fps: suggested.from && suggested.to ? `${suggested.from} → ${suggested.to} fps` : "—",
            shift: seconds(suggested.shift),
          }) }}
        </p>
        <label class="flex items-center gap-2 text-xs text-text-secondary">
          <input v-model="overwrite" type="checkbox" class="accent-accent" />
          {{ t("subtitle.sync.overwrite") }}
        </label>
        <button
          :disabled="fixing"
          class="inline-flex items-center gap-2 px-4 py-2.5 rounded-lg bg-bg-tertiary text-text-primary text-sm font-medium hover:bg-bg-hover transition-colors disabled:opacity-50"
          data-testid="sync-apply"
          @click="applyFix"
        >
          <Loader2 v-if="fixing" class="w-4 h-4 animate-spin" />
          <Wand2 v-else class="w-4 h-4" />
          {{ fixing ? t("subtitle.sync.applying") : t("subtitle.sync.apply") }}
        </button>
      </div>
    </div>

    <div
      v-if="fixedPath"
      class="flex items-start gap-3 px-4 py-3 rounded-lg bg-success/10 border border-success/20"
      data-testid="sync-done"
    >
      <CheckCircle2 class="w-4 h-4 text-success shrink-0 mt-0.5" />
      <p class="text-xs text-success">{{ t("subtitle.sync.done", { path: fixedPath }) }}</p>
    </div>

    <div
      v-if="error"
      class="flex items-start gap-3 px-4 py-3 rounded-lg bg-error/10 border border-error/20"
    >
      <AlertTriangle class="w-4 h-4 text-error shrink-0 mt-0.5" />
      <p class="text-xs text-error whitespace-pre-wrap">{{ error }}</p>
    </div>
  </div>
</template>
