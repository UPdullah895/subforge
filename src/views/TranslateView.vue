<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from "vue";
import { useI18n } from "vue-i18n";
import { open } from "../utils/dialog";
import { baseName, externalSubFilename, parseSubtitleFilename } from "../utils/subtitleNaming";
import { apiListen } from "../api/events";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { useTranslateStore, type EngineType, mapJobErrorMessage } from "../stores/translateStore";
import { isDeeplSupported } from "../utils/languageNames";

const { t } = useI18n();
import { JOB_STATUS, type JobStatus } from "../constants/jobStatus";
import { useAppStore } from "../stores/appStore";
import ErrorBanner from "../components/ErrorBanner.vue";
import SetupInstructions from "../components/SetupInstructions.vue";
import {
  FolderOpen,
  Languages,
  Loader2,
  CheckCircle2,
  XCircle,
  X,
  Play,
  Pause,
  Square,
  RefreshCw,
  Brain,
  Globe,
  Globe2,
  Key,
  Zap,
  Info,
  HelpCircle,
  ChevronDown,
  ChevronUp,
} from "lucide-vue-next";

const translateStore = useTranslateStore();
const appStore = useAppStore();

const selectedFiles = ref<string[]>([]);
const translating = ref(false);
const enginesExpanded = ref(false);
const showEngineHelp = ref<EngineType | null>(null);

const fileCount = computed(() => selectedFiles.value.length);

const activeJob = computed(() =>
  translateStore.jobs.find((j) => j.status === JOB_STATUS.RUNNING || j.status === JOB_STATUS.PENDING)
);

const pausedJobId = ref<string | null>(null);

async function toggleActiveJobPause() {
  if (!activeJob.value) return;
  const id = activeJob.value.id;
  if (pausedJobId.value === id) {
    await translateStore.resumeJob(id);
    pausedJobId.value = null;
  } else {
    await translateStore.pauseJob(id);
    pausedJobId.value = id;
  }
}

async function stopActiveJob() {
  if (!activeJob.value) return;
  await translateStore.cancelJob(activeJob.value.id);
  pausedJobId.value = null;
}

const engines: { id: EngineType; icon: typeof Brain }[] = [
  { id: "ollama", icon: Brain },
  { id: "libretranslate", icon: Globe },
  { id: "deepl", icon: Zap },
  { id: "openai", icon: Key },
  { id: "google", icon: Globe2 },
];

const engineKeyPrefix: Record<EngineType, string> = {
  ollama: "ollama",
  libretranslate: "libre",
  deepl: "deepl",
  openai: "openai",
  google: "google",
};

const languageOptions = [
  { code: "auto", label: "Auto-detect" },
  { code: "en", label: "English" },
  { code: "ar", label: "Arabic" },
  { code: "es", label: "Spanish" },
  { code: "fr", label: "French" },
  { code: "de", label: "German" },
  { code: "ja", label: "Japanese" },
  { code: "ko", label: "Korean" },
  { code: "zh", label: "Chinese" },
  { code: "ru", label: "Russian" },
  { code: "pt", label: "Portuguese" },
  { code: "it", label: "Italian" },
  { code: "tr", label: "Turkish" },
  { code: "hi", label: "Hindi" },
  { code: "fa", label: "Persian" },
  { code: "ur", label: "Urdu" },
];

const selectableLangs = new Set(languageOptions.map((l) => l.code));

function detectLangFromFileName(name: string): string | null {
  const lang = parseSubtitleFilename(name).lang;
  return lang && selectableLangs.has(lang) ? lang : null;
}

function buildOutputPath(inputPath: string, targetLang: string): string {
  const lastSlash = Math.max(inputPath.lastIndexOf("/"), inputPath.lastIndexOf("\\"));
  const dir = inputPath.substring(0, lastSlash + 1);
  const name = inputPath.substring(lastSlash + 1);

  // `Show.eng.sdh.srt` -> `Show.ar.sdh.srt`: swap the language tag, keep the
  // SDH/forced flavor, so the result is still `{video}.{lang}[.flavor].ext`.
  const { stem, flavor, ext } = parseSubtitleFilename(name);
  return `${dir}${externalSubFilename(stem, targetLang, ext, flavor)}`;
}

async function pickFiles() {
  const selected = await open({
    multiple: true,
    directory: false,
    filters: [{ name: "Subtitle files", extensions: ["srt", "ass", "ssa", "vtt"] }],
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

watch(selectedFiles, (files) => {
  if (files.length === 0) return;
  // Detect language from the first file that has a language code
  for (const path of files) {
    const parts = path.split(/[/\\]/);
    const name = parts[parts.length - 1];
    const detected = detectLangFromFileName(name);
    if (detected) {
      translateStore.sourceLang = detected;
      break;
    }
  }
}, { deep: true });

async function startTranslation() {
  if (selectedFiles.value.length === 0) return;

  translating.value = true;
  for (const file of selectedFiles.value) {
    const output = buildOutputPath(file, translateStore.targetLang);
    await translateStore.startTranslation(file, output);
  }
  translating.value = false;
}

let unlistenProgress: UnlistenFn | null = null;
let unlistenCompleted: UnlistenFn | null = null;

async function probeSelectedEngine() {
  if (translateStore.selectedEngine === "ollama") {
    await translateStore.detectOllama();
  } else if (translateStore.selectedEngine === "libretranslate") {
    await translateStore.detectLibreTranslate();
  }
  // deepl and openai need no detection probe
}

watch(() => translateStore.selectedEngine, () => probeSelectedEngine());

onMounted(async () => {
  appStore.discoverTools();
  await appStore.loadConfig();

  // Apply defaults from config
  if (appStore.config) {
    if (appStore.config.default_engine) {
      translateStore.selectedEngine = appStore.config.default_engine as EngineType;
    }
    if (appStore.config.default_source_lang) {
      translateStore.sourceLang = appStore.config.default_source_lang;
    }
    if (appStore.config.default_target_lang) {
      translateStore.targetLang = appStore.config.default_target_lang;
    }
  }

  await probeSelectedEngine();

  unlistenProgress = await apiListen<{ job_id: string; progress: number; message?: string }>(
    "job-progress",
    ({ job_id, progress, message }) => {
      translateStore.updateJobProgress(job_id, progress, message);
    }
  );

  unlistenCompleted = await apiListen<{ job_id: string; status: string; message?: string }>(
    "job-completed",
    ({ job_id, status, message }) => {
      translateStore.completeJob(job_id, status as JobStatus, mapJobErrorMessage(message));
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
        <h1 class="text-xl font-semibold text-text-primary">{{ t("translate.title") }}</h1>
        <span
          class="text-text-muted hover:text-text-secondary cursor-help"
          :title="t('translate.tooltip')"
        >
          <Info class="w-4 h-4" />
        </span>
      </div>
      <button
        class="inline-flex items-center gap-2 px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover active:bg-accent-active transition-colors"
        @click="pickFiles"
      >
        <FolderOpen class="w-4 h-4" />
        {{ t("translate.openSubtitles") }}
      </button>
    </div>

    <!-- Content -->
    <div class="flex-1 overflow-auto p-6 space-y-6">
      <ErrorBanner :message="translateStore.error" @dismiss="translateStore.error = null" />

      <!-- File List -->
      <div v-if="fileCount > 0" class="space-y-2">
        <div class="flex items-center justify-between">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
            {{ t("translate.filesCount", { count: fileCount }) }}
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
            <Languages class="w-4 h-4 text-accent shrink-0" />
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

      <!-- Engine Selector (Collapsible) -->
      <div class="space-y-3">
        <div class="flex items-center justify-between">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
            {{ t("translate.translationEngine") }}
          </h2>
          <button
            class="inline-flex items-center gap-1 text-xs text-text-secondary hover:text-text-primary transition-colors"
            @click="enginesExpanded = !enginesExpanded"
          >
            <component :is="enginesExpanded ? ChevronUp : ChevronDown" class="w-4 h-4" />
            {{ enginesExpanded ? t("translate.collapse") : t("translate.change") }}
          </button>
        </div>

        <!-- Collapsed: show only selected engine -->
        <div
          v-if="!enginesExpanded"
          class="inline-flex items-center gap-2 px-3 py-2 rounded-lg border border-accent bg-accent/10 text-accent"
        >
          <component
            :is="engines.find((e) => e.id === translateStore.selectedEngine)?.icon"
            class="w-4 h-4"
          />
          <span class="text-sm font-medium">
            {{ t(`translate.${translateStore.selectedEngine}`) }}
          </span>
        </div>

        <!-- Expanded: full grid -->
        <div v-else class="grid grid-cols-2 md:grid-cols-4 gap-3">
          <button
            v-for="engine in engines"
            :key="engine.id"
            :class="[
              'relative flex flex-col items-center gap-2 p-4 rounded-lg border transition-colors text-left',
              translateStore.selectedEngine === engine.id
                ? 'border-accent bg-accent/10 text-accent'
                : 'border-border bg-bg-secondary text-text-secondary hover:bg-bg-hover',
            ]"
            @click="translateStore.selectedEngine = engine.id"
          >
            <component :is="engine.icon" class="w-5 h-5" />
            <span class="text-sm font-medium">{{ t(`translate.${engine.id}`) }}</span>
            <button
              class="absolute top-2 right-2 text-text-muted hover:text-text-primary transition-colors"
              :title="t('translate.about', { engine: t(`translate.${engine.id}`) })"
              @click.stop="showEngineHelp = showEngineHelp === engine.id ? null : engine.id"
            >
              <HelpCircle class="w-3.5 h-3.5" />
            </button>
          </button>
        </div>

        <!-- Engine Help Panel -->
        <SetupInstructions
          v-if="showEngineHelp"
          :title="t(`translate.${showEngineHelp}`)"
          :what="t(`translate.${engineKeyPrefix[showEngineHelp]}Desc`)"
          :setup="t(`translate.${engineKeyPrefix[showEngineHelp]}Setup`)"
          :security="t(`translate.${engineKeyPrefix[showEngineHelp]}Security`)"
          @close="showEngineHelp = null"
        />
      </div>

      <!-- Engine Settings -->
      <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
        <!-- Ollama Settings -->
        <div v-if="translateStore.selectedEngine === 'ollama'" class="space-y-3">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">{{ t("translate.ollamaSection") }}</h2>
          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("translate.hostUrl") }}</label>
            <div class="flex items-center gap-2">
              <input
                v-model="translateStore.ollamaHost"
                type="text"
                class="flex-1 px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
              />
              <button
                class="inline-flex items-center justify-center w-10 h-10 rounded-lg bg-bg-tertiary text-text-primary hover:bg-bg-hover transition-colors shrink-0"
                :title="t('translate.autoDetect')"
                @click="translateStore.detectOllama"
              >
                <RefreshCw class="w-4 h-4" />
              </button>
            </div>
          </div>
          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("translate.modelLabel") }}</label>
            <select
              v-model="translateStore.selectedOllamaModel"
              class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
            >
              <option v-for="m in translateStore.ollamaModels" :key="m" :value="m">{{ m }}</option>
            </select>
            <p v-if="translateStore.ollamaModels.length === 0" class="text-xs text-warning mt-1">
              {{ t("translate.noModels") }}
            </p>
          </div>
        </div>

        <!-- LibreTranslate Settings -->
        <div v-else-if="translateStore.selectedEngine === 'libretranslate'" class="space-y-3">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">{{ t("translate.libreSection") }}</h2>
          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("translate.hostUrl") }}</label>
            <div class="flex items-center gap-2">
              <input
                v-model="translateStore.libreHost"
                type="text"
                class="flex-1 px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
              />
              <button
                class="inline-flex items-center justify-center w-10 h-10 rounded-lg bg-bg-tertiary text-text-primary hover:bg-bg-hover transition-colors shrink-0"
                :title="t('translate.autoDetectLibre')"
                @click="translateStore.detectLibreTranslate"
              >
                <RefreshCw class="w-4 h-4" />
              </button>
            </div>
          </div>
        </div>

        <!-- DeepL Settings -->
        <div v-else-if="translateStore.selectedEngine === 'deepl'" class="space-y-3">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">{{ t("translate.deeplSection") }}</h2>
          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("translate.apiKey") }}</label>
            <input
              v-model="translateStore.deeplKey"
              type="password"
              :placeholder="t('translate.deeplPlaceholder')"
              class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary placeholder-text-muted focus:outline-none focus:border-accent"
            />
          </div>
          <label class="flex items-center gap-2 cursor-pointer">
            <input
              v-model="translateStore.deeplPro"
              type="checkbox"
              class="rounded border-border bg-bg-primary text-accent focus:ring-accent"
            />
            <span class="text-sm text-text-secondary">{{ t("translate.proAccount") }}</span>
          </label>
        </div>

        <!-- OpenAI Settings -->
        <div v-else-if="translateStore.selectedEngine === 'openai'" class="space-y-3">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">{{ t("translate.openaiSection") }}</h2>
          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("translate.apiKey") }}</label>
            <input
              v-model="translateStore.openaiKey"
              type="password"
              :placeholder="t('translate.openaiPlaceholder')"
              class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary placeholder-text-muted focus:outline-none focus:border-accent"
            />
          </div>
          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("translate.modelName") }}</label>
            <input
              v-model="translateStore.openaiModel"
              type="text"
              class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
            />
          </div>
        </div>

        <!-- Google Translate Settings -->
        <div v-else-if="translateStore.selectedEngine === 'google'" class="space-y-3">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">{{ t("translate.googleSection") }}</h2>
          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("translate.apiKey") }}</label>
            <input
              v-model="translateStore.googleApiKey"
              type="password"
              :placeholder="t('translate.googlePlaceholder')"
              class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary placeholder-text-muted focus:outline-none focus:border-accent"
            />
          </div>
        </div>

        <!-- Language Pair -->
        <div class="space-y-3">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">{{ t("translate.languages") }}</h2>
          <div class="grid grid-cols-2 gap-3">
            <div>
              <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("translate.source") }}</label>
              <select
                v-model="translateStore.sourceLang"
                class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
              >
                <option
                  v-for="lang in languageOptions"
                  :key="lang.code"
                  :value="lang.code"
                  :disabled="translateStore.selectedEngine === 'deepl' && !isDeeplSupported(lang.code)"
                >
                  {{ lang.label }}
                  {{ translateStore.selectedEngine === "deepl" && !isDeeplSupported(lang.code) ? t("translate.deeplUnsupportedSuffix") : "" }}
                </option>
              </select>
            </div>
            <div>
              <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("translate.target") }}</label>
              <select
                v-model="translateStore.targetLang"
                class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
              >
                <option
                  v-for="lang in languageOptions"
                  :key="lang.code"
                  :value="lang.code"
                  :disabled="translateStore.selectedEngine === 'deepl' && !isDeeplSupported(lang.code)"
                >
                  {{ lang.label }}
                  {{ translateStore.selectedEngine === "deepl" && !isDeeplSupported(lang.code) ? t("translate.deeplUnsupportedSuffix") : "" }}
                </option>
              </select>
            </div>
          </div>
        </div>
      </div>

      <!-- Translate Button -->
      <button
        :disabled="fileCount === 0 || translating"
        class="w-full md:w-auto inline-flex items-center justify-center gap-2 px-6 py-3 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover active:bg-accent-active transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        @click="startTranslation"
      >
        <Loader2 v-if="translating" class="w-4 h-4 animate-spin" />
        <Play v-else class="w-4 h-4" />
        {{ translating ? t("translate.translating") : t("translate.translate") }}
      </button>

      <!-- Active Job Progress Banner -->
      <div
        v-if="activeJob"
        class="sticky bottom-0 p-4 rounded-lg bg-bg-secondary border border-accent/30 space-y-2"
      >
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-2">
            <Loader2 class="w-4 h-4 text-accent animate-spin" />
            <span class="text-sm font-medium text-text-primary">
              {{ activeJob.message || t("translate.activeJobProgress") }}
            </span>
          </div>
          <div class="flex items-center gap-2">
            <span class="text-sm font-bold text-accent">{{ Math.round(activeJob.progress) }}%</span>
            <button
              class="p-1.5 rounded hover:bg-bg-hover text-text-secondary transition-colors"
              :title="pausedJobId === activeJob.id ? t('jobMonitor.resume') : t('jobMonitor.pause')"
              @click="toggleActiveJobPause"
            >
              <Play v-if="pausedJobId === activeJob.id" class="w-3.5 h-3.5" />
              <Pause v-else class="w-3.5 h-3.5" />
            </button>
            <button
              class="p-1.5 rounded hover:bg-bg-hover text-text-secondary transition-colors"
              :title="t('jobMonitor.stop')"
              @click="stopActiveJob"
            >
              <Square class="w-3.5 h-3.5" />
            </button>
          </div>
        </div>
        <div class="w-full bg-bg-tertiary rounded-full h-2 overflow-hidden">
          <div
            class="h-full rounded-full bg-accent transition-all duration-300"
            :style="{ width: `${activeJob.progress}%` }"
          />
        </div>
      </div>

      <!-- Jobs -->
      <div v-if="translateStore.jobs.length > 0" class="space-y-2">
        <div class="flex items-center justify-between">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
            {{ t("translate.translationJobs") }}
          </h2>
          <button
            class="text-xs text-text-muted hover:text-text-secondary transition-colors"
            @click="translateStore.clearCompleted"
          >
            {{ t("jobMonitor.clearCompleted") }}
          </button>
        </div>
        <div class="space-y-2">
          <div
            v-for="job in translateStore.jobs"
            :key="job.id"
            class="px-4 py-3 rounded-lg bg-bg-secondary border border-border"
          >
            <div class="flex items-center gap-3 mb-2">
              <CheckCircle2 v-if="job.status === JOB_STATUS.COMPLETED" class="w-4 h-4 text-success shrink-0" />
              <XCircle
                v-else-if="job.status === JOB_STATUS.FAILED || job.status === JOB_STATUS.CANCELLED"
                class="w-4 h-4 text-error shrink-0"
              />
              <Loader2 v-else class="w-4 h-4 text-accent animate-spin shrink-0" />
              <div class="flex-1 min-w-0">
                <p class="text-xs text-text-primary truncate" dir="auto" :title="job.input_file">{{ baseName(job.input_file) }}</p>
                <p v-if="job.message" class="text-xs text-text-secondary truncate">{{ job.message }}</p>
              </div>
              <button
                class="text-text-muted hover:text-text-secondary transition-colors"
                @click="translateStore.jobs = translateStore.jobs.filter((j) => j.id !== job.id)"
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
        v-if="fileCount === 0 && translateStore.jobs.length === 0"
        class="flex flex-col items-center justify-center py-16 text-center"
      >
        <div class="w-16 h-16 rounded-2xl bg-bg-secondary border border-border flex items-center justify-center mb-4">
          <Languages class="w-8 h-8 text-text-muted" />
        </div>
        <h3 class="text-base font-medium text-text-primary">{{ t("translate.noSubtitleSelected") }}</h3>
        <p class="text-sm text-text-secondary mt-1 max-w-sm">
          {{ t("translate.noSubtitleSelectedDesc") }}
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
