<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { apiInvoke } from "../api/client";
import { open } from "../utils/dialog";
import { baseName } from "../utils/subtitleNaming";
import {
  AlertTriangle,
  CheckCircle2,
  FolderOpen,
  Loader2,
  ScanLine,
  Trash2,
} from "lucide-vue-next";

const { t } = useI18n();

const props = withDefaults(defineProps<{ initialPath?: string }>(), { initialPath: "" });

export interface RemovedCue {
  index: number;
  start_ms: number;
  end_ms: number;
  text: string;
  rule: string;
}

interface CleanResponse {
  removed: RemovedCue[];
  kept: number;
  output_path: string | null;
  backup_path: string | null;
}

const inputPath = ref("");
const scanning = ref(false);
const applying = ref(false);
const result = ref<CleanResponse | null>(null);
const error = ref<string | null>(null);
const donePath = ref<string | null>(null);
const backupPath = ref<string | null>(null);
const overwrite = ref(false);
/** Indexes the user un-ticked: keep them even though a rule matched. */
const keptIndexes = ref<number[]>([]);

// A subtitle dropped on the Clean tab arrives as a prop; treat it exactly like
// picking the file (without leaving stale results on screen).
watch(
  () => props.initialPath,
  (path) => {
    if (!path) return;
    inputPath.value = path;
    result.value = null;
    error.value = null;
    donePath.value = null;
    backupPath.value = null;
    keptIndexes.value = [];
  }
);

const removals = computed(() => result.value?.removed ?? []);

const ruleLabel = (rule: string) => {
  const known = ["link", "email", "handle", "credit"];
  return known.includes(rule) ? t(`subtitle.clean.rule_${rule}`) : rule;
};

function time(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000));
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
}

function isKept(index: number): boolean {
  return keptIndexes.value.includes(index);
}

function toggleKeep(index: number) {
  keptIndexes.value = isKept(index)
    ? keptIndexes.value.filter((i) => i !== index)
    : [...keptIndexes.value, index];
}

async function pickFile() {
  const selected = await open({
    multiple: false,
    title: t("subtitle.clean.pickFile"),
    filters: [{ name: "Subtitles", extensions: ["srt", "ass", "ssa", "vtt"] }],
  });
  if (selected && typeof selected === "string") {
    inputPath.value = selected;
    result.value = null;
    error.value = null;
    donePath.value = null;
    backupPath.value = null;
    keptIndexes.value = [];
  }
}

async function scan() {
  if (!inputPath.value) return;
  scanning.value = true;
  error.value = null;
  result.value = null;
  donePath.value = null;
  backupPath.value = null;
  keptIndexes.value = [];
  try {
    result.value = await apiInvoke<CleanResponse>("clean_subtitle", {
      request: { input: inputPath.value, apply: false },
    });
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    scanning.value = false;
  }
}

async function apply() {
  if (!inputPath.value || !result.value) return;
  applying.value = true;
  error.value = null;
  try {
    const response = await apiInvoke<CleanResponse>("clean_subtitle", {
      request: {
        input: inputPath.value,
        output: overwrite.value ? inputPath.value : null,
        keep: keptIndexes.value,
        apply: true,
      },
    });
    donePath.value = response.output_path;
    backupPath.value = response.backup_path;
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    applying.value = false;
  }
}
</script>

<template>
  <div class="max-w-3xl space-y-6">
    <div class="space-y-1">
      <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
        {{ t("subtitle.clean.title") }}
      </h2>
      <p class="text-xs text-text-muted">{{ t("subtitle.clean.desc") }}</p>
    </div>

    <div>
      <label class="block text-xs font-medium text-text-secondary mb-1.5">
        {{ t("subtitle.subtitleFileLabel") }}
      </label>
      <button
        class="flex items-center gap-2 w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary hover:bg-bg-hover transition-colors text-left"
        data-testid="clean-pick-file"
        @click="pickFile"
      >
        <FolderOpen class="w-4 h-4 text-text-muted" />
        <span class="truncate" dir="auto" :title="inputPath">
          {{ inputPath ? baseName(inputPath) : t("subtitle.clean.pickFile") }}
        </span>
      </button>
    </div>

    <button
      :disabled="!inputPath || scanning"
      class="inline-flex items-center gap-2 px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
      data-testid="clean-scan"
      @click="scan"
    >
      <Loader2 v-if="scanning" class="w-4 h-4 animate-spin" />
      <ScanLine v-else class="w-4 h-4" />
      {{ scanning ? t("subtitle.clean.scanning") : t("subtitle.clean.scan") }}
    </button>

    <div
      v-if="result && removals.length === 0"
      class="flex items-start gap-3 px-4 py-3 rounded-lg bg-success/10 border border-success/20"
      data-testid="clean-empty"
    >
      <CheckCircle2 class="w-4 h-4 text-success shrink-0 mt-0.5" />
      <p class="text-xs text-success">{{ t("subtitle.clean.nothing") }}</p>
    </div>

    <div v-if="removals.length > 0" class="space-y-3" data-testid="clean-results">
      <p class="text-xs text-text-secondary">
        {{ t("subtitle.clean.found", { count: removals.length }) }}
      </p>

      <div class="rounded-lg border border-border overflow-hidden">
        <table class="w-full text-sm">
          <thead class="bg-bg-secondary text-xs text-text-muted">
            <tr>
              <th class="text-left font-medium px-3 py-2 w-20">{{ t("subtitle.clean.time") }}</th>
              <th class="text-left font-medium px-3 py-2">{{ t("subtitle.clean.text") }}</th>
              <th class="text-left font-medium px-3 py-2 w-24">{{ t("subtitle.clean.rule") }}</th>
              <th class="text-right font-medium px-3 py-2 w-24">{{ t("subtitle.clean.remove") }}</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="cue in removals"
              :key="cue.index"
              class="border-t border-border"
              :class="isKept(cue.index) ? 'opacity-50' : ''"
            >
              <td class="px-3 py-2 text-xs text-text-muted whitespace-nowrap">
                {{ time(cue.start_ms) }}
              </td>
              <td class="px-3 py-2 text-text-primary" dir="auto">{{ cue.text }}</td>
              <td class="px-3 py-2 text-xs text-text-muted">{{ ruleLabel(cue.rule) }}</td>
              <td class="px-3 py-2 text-right">
                <input
                  type="checkbox"
                  class="accent-accent"
                  :checked="!isKept(cue.index)"
                  :aria-label="t('subtitle.clean.remove')"
                  :data-testid="`clean-keep-${cue.index}`"
                  @change="toggleKeep(cue.index)"
                />
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <label class="flex items-center gap-2 text-xs text-text-secondary">
        <input v-model="overwrite" type="checkbox" class="accent-accent" />
        {{ t("subtitle.clean.overwrite") }}
      </label>

      <button
        :disabled="applying"
        class="inline-flex items-center gap-2 px-4 py-2.5 rounded-lg bg-bg-tertiary text-text-primary text-sm font-medium hover:bg-bg-hover transition-colors disabled:opacity-50"
        data-testid="clean-apply"
        @click="apply"
      >
        <Loader2 v-if="applying" class="w-4 h-4 animate-spin" />
        <Trash2 v-else class="w-4 h-4" />
        {{ applying ? t("subtitle.clean.applying") : t("subtitle.clean.apply") }}
      </button>
    </div>

    <div
      v-if="donePath"
      class="flex items-start gap-3 px-4 py-3 rounded-lg bg-success/10 border border-success/20"
      data-testid="clean-done"
    >
      <CheckCircle2 class="w-4 h-4 text-success shrink-0 mt-0.5" />
      <div class="text-xs text-success space-y-1">
        <p>{{ t("subtitle.clean.done", { path: donePath }) }}</p>
        <p v-if="backupPath">{{ t("subtitle.clean.backup", { path: backupPath }) }}</p>
      </div>
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
