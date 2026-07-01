<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import Modal from "./Modal.vue";
import { apiInvoke } from "../api/client";
import { apiListen } from "../api/events";
import { useBatchStore } from "../stores/batchStore";
import { useAppStore } from "../stores/appStore";
import { useTranslateStore, type EngineType, mapJobErrorMessage } from "../stores/translateStore";
import { CheckCircle2, XCircle, Loader2 } from "lucide-vue-next";

const { t } = useI18n();

const props = defineProps<{
  open: boolean;
  files: string[];
}>();

const emit = defineEmits<{
  (e: "close"): void;
}>();

const batchStore = useBatchStore();
const translateStore = useTranslateStore();
const appStore = useAppStore();

const languageOptions = [
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

// mkvmerge reports track languages as ISO 639-2 codes (e.g. "ita"), not the
// ISO 639-1 codes used in the language dropdown (e.g. "it") — map each
// dropdown code to every 639-2 variant mkvmerge might report so track
// matching actually honors the user's chosen source language.
const LANG_ALIASES: Record<string, string[]> = {
  en: ["en", "eng"],
  ar: ["ar", "ara"],
  es: ["es", "spa"],
  fr: ["fr", "fre", "fra"],
  de: ["de", "ger", "deu"],
  ja: ["ja", "jpn"],
  ko: ["ko", "kor"],
  zh: ["zh", "chi", "zho"],
  ru: ["ru", "rus"],
  pt: ["pt", "por"],
  it: ["it", "ita"],
  tr: ["tr", "tur"],
  hi: ["hi", "hin"],
  fa: ["fa", "per", "fas"],
  ur: ["ur", "urd"],
};

const sourceLang = ref("en");
const targetLangs = ref<string[]>(["ar"]);
const autoMerge = ref(true);
const setDefaultSubtitle = ref(false);
const provider = ref<EngineType>("ollama");
const starting = ref(false);
const ollamaAvailable = ref(false);
const libreAvailable = ref(false);
const probed = ref(false);

// path -> chosen track id, only used when a file has more than one subtitle
// track matching the chosen source language (see `ambiguousFiles`).
const selectedTrackId = reactive<Record<string, number>>({});

type FileStage = "queued" | "extracting" | "translating" | "merging" | "done" | "error";

interface FileJob {
  path: string;
  lang: string;
  name: string;
  jobId?: string;
  stage: FileStage;
  progress: number;
  message?: string;
}

const fileJobs = ref<FileJob[]>([]);
let unlistenProgress: (() => void) | null = null;
let unlistenCompleted: (() => void) | null = null;

const providerOptions = computed(() => {
  const list: { id: EngineType; label: string }[] = [];
  if (ollamaAvailable.value) list.push({ id: "ollama", label: "Ollama" });
  if (libreAvailable.value) list.push({ id: "libretranslate", label: "LibreTranslate" });
  if (translateStore.deeplKey) list.push({ id: "deepl", label: "DeepL" });
  if (translateStore.openaiKey) list.push({ id: "openai", label: "OpenAI" });
  if (translateStore.googleApiKey) list.push({ id: "google", label: "Google Translate" });
  return list;
});

const running = computed(() => fileJobs.value.length > 0);

interface TrackCandidate {
  id: number;
  label: string;
}

function trackLabel(tr: {
  id: number;
  properties?: { track_name?: string; forced_track?: boolean; hearing_impaired?: boolean };
}): string {
  const parts: string[] = [`#${tr.id}`];
  if (tr.properties?.track_name) parts.push(tr.properties.track_name);
  if (tr.properties?.forced_track) parts.push(t("autoTranslate.forced"));
  if (tr.properties?.hearing_impaired) parts.push(t("autoTranslate.sdh"));
  return parts.join(" — ");
}

// Files where more than one subtitle track matches the chosen source
// language (e.g. a regular track and an SDH track) — the pipeline needs a
// single specific track, so these need an explicit pick instead of silently
// defaulting to the first match.
const ambiguousFiles = computed(() => {
  const result: { path: string; name: string; candidates: TrackCandidate[] }[] = [];
  const aliases = LANG_ALIASES[sourceLang.value] ?? [sourceLang.value];
  for (const path of props.files) {
    const file = batchStore.scannedFiles.find((f) => f.path === path);
    if (!file) continue;
    const matches = file.tracks.filter(
      (tr) => tr.track_type === "subtitles" && aliases.includes((tr.properties?.language || "").toLowerCase())
    );
    if (matches.length > 1) {
      result.push({ path, name: file.name, candidates: matches.map((tr) => ({ id: tr.id, label: trackLabel(tr) })) });
    }
  }
  return result;
});

watch(ambiguousFiles, (files) => {
  for (const f of files) {
    if (selectedTrackId[f.path] == null) {
      selectedTrackId[f.path] = f.candidates[0].id;
    }
  }
});

const showSetDefaultSubtitle = computed(() =>
  props.files.some((path) => (batchStore.scannedFiles.find((f) => f.path === path)?.subtitle_count ?? 0) > 0)
);

watch(
  () => props.open,
  async (isOpen) => {
    if (!isOpen) return;
    fileJobs.value = [];
    probed.value = false;
    for (const key of Object.keys(selectedTrackId)) delete selectedTrackId[key];

    sourceLang.value = appStore.config?.default_source_lang || "en";
    targetLangs.value = appStore.config?.default_target_langs?.length
      ? [...appStore.config.default_target_langs]
      : ["ar"];

    const [ollama, libre] = await Promise.all([
      translateStore.detectOllama(),
      translateStore.detectLibreTranslate(),
    ]);
    ollamaAvailable.value = ollama;
    libreAvailable.value = libre;
    probed.value = true;

    const preferred = appStore.config?.default_engine as EngineType | undefined;
    if (preferred && providerOptions.value.some((p) => p.id === preferred)) {
      provider.value = preferred;
    } else if (providerOptions.value.length > 0) {
      provider.value = providerOptions.value[0].id;
    }
  },
  { immediate: true }
);

function toggleTargetLang(code: string) {
  const idx = targetLangs.value.indexOf(code);
  if (idx >= 0) targetLangs.value.splice(idx, 1);
  else targetLangs.value.push(code);
}

function findSubtitleTrack(path: string): number | null {
  if (selectedTrackId[path] != null) return selectedTrackId[path];
  const file = batchStore.scannedFiles.find((f) => f.path === path);
  if (!file) return null;
  const subs = file.tracks.filter((tr) => tr.track_type === "subtitles");
  if (subs.length === 0) return null;
  const aliases = LANG_ALIASES[sourceLang.value] ?? [sourceLang.value];
  const matching = subs.find((tr) => aliases.includes((tr.properties?.language || "").toLowerCase()));
  return (matching ?? subs[0]).id;
}

async function persistDefaults() {
  if (!appStore.config) return;
  appStore.config.default_engine = provider.value;
  appStore.config.default_source_lang = sourceLang.value;
  appStore.config.default_target_langs = [...targetLangs.value];
  await appStore.saveConfig();
}

async function startPipeline() {
  starting.value = true;
  await persistDefaults();

  fileJobs.value = [];
  for (const path of props.files) {
    const file = batchStore.scannedFiles.find((f) => f.path === path);
    for (const lang of targetLangs.value) {
      fileJobs.value.push({
        path,
        lang,
        name: `${file?.name ?? path} → ${lang.toUpperCase()}`,
        stage: "queued",
        progress: 0,
      });
    }
  }

  unlistenProgress = await apiListen<{
    job_id: string;
    stage?: string;
    progress: number;
    message?: string;
  }>("job-progress", ({ job_id, stage, progress, message }) => {
    const job = fileJobs.value.find((f) => f.jobId === job_id);
    if (job) {
      if (stage) job.stage = stage as FileStage;
      job.progress = progress;
      if (message) job.message = message;
    }
  });

  unlistenCompleted = await apiListen<{
    job_id: string;
    status: string;
    stage?: string;
    message?: string;
  }>("job-completed", ({ job_id, status, message }) => {
    const job = fileJobs.value.find((f) => f.jobId === job_id);
    if (job) {
      job.stage = status === "completed" ? "done" : "error";
      job.progress = 100;
      const mapped = mapJobErrorMessage(message);
      if (mapped) job.message = mapped;
    }
  });

  for (const job of fileJobs.value) {
    const trackId = findSubtitleTrack(job.path);
    if (trackId === null) {
      job.stage = "error";
      job.message = t("autoTranslate.noSubtitleTrack");
      continue;
    }

    const request: Record<string, unknown> = {
      video_path: job.path,
      track_id: trackId,
      engine: provider.value,
      source_lang: sourceLang.value,
      target_lang: job.lang,
      auto_merge: autoMerge.value,
      set_default_subtitle: autoMerge.value && setDefaultSubtitle.value,
    };
    if (provider.value === "ollama") request.host = translateStore.ollamaHost;
    else if (provider.value === "libretranslate") request.host = translateStore.libreHost;
    else if (provider.value === "deepl") request.api_key = translateStore.deeplKey;
    else if (provider.value === "openai") {
      request.api_key = translateStore.openaiKey;
      request.model = translateStore.openaiModel;
    } else if (provider.value === "google") request.api_key = translateStore.googleApiKey;

    try {
      const jobId = await apiInvoke<string>("run_translation_pipeline", { request });
      job.jobId = jobId;
      job.stage = "extracting";
      await batchStore.registerPipelineJob(jobId, job.path, job.lang);
    } catch (e) {
      job.stage = "error";
      job.message = mapJobErrorMessage(e instanceof Error ? e.message : String(e));
    }
  }
}

function close() {
  if (unlistenProgress) unlistenProgress();
  if (unlistenCompleted) unlistenCompleted();
  unlistenProgress = null;
  unlistenCompleted = null;
  fileJobs.value = [];
  starting.value = false;
  emit("close");
}

function stageLabel(stage: FileStage): string {
  return t(`autoTranslate.stage.${stage}`);
}
</script>

<template>
  <Modal :open="open" :title="t('autoTranslate.title')" @close="close">
    <div class="space-y-5">
      <div v-if="!running">
        <div v-if="probed && providerOptions.length === 0" class="p-3 rounded-lg bg-warning/10 border border-warning/20 text-xs text-warning">
          {{ t("autoTranslate.noProviders") }}
        </div>
        <template v-else>
          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("autoTranslate.provider") }}</label>
            <select
              v-model="provider"
              class="w-full px-4 py-2.5 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
            >
              <option v-for="p in providerOptions" :key="p.id" :value="p.id">{{ p.label }}</option>
            </select>
          </div>

          <div class="mt-4">
            <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("translate.source") }}</label>
            <select
              v-model="sourceLang"
              class="w-full px-4 py-2.5 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
            >
              <option v-for="lang in languageOptions" :key="lang.code" :value="lang.code">{{ lang.label }}</option>
            </select>
          </div>

          <div class="mt-4">
            <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("autoTranslate.targetLanguages") }}</label>
            <div class="grid grid-cols-3 gap-1.5">
              <label
                v-for="lang in languageOptions"
                :key="lang.code"
                class="flex items-center gap-1.5 px-2 py-1.5 rounded-md bg-bg-primary border border-border text-xs text-text-secondary cursor-pointer"
              >
                <input
                  type="checkbox"
                  class="accent-accent"
                  :checked="targetLangs.includes(lang.code)"
                  @change="toggleTargetLang(lang.code)"
                />
                {{ lang.label }}
              </label>
            </div>
          </div>

          <div v-if="ambiguousFiles.length > 0" class="mt-4 space-y-2">
            <p class="text-xs font-medium text-text-secondary">{{ t("autoTranslate.ambiguousTrack") }}</p>
            <div
              v-for="f in ambiguousFiles"
              :key="f.path"
              class="p-2.5 rounded-lg bg-bg-secondary border border-border"
            >
              <p class="text-xs text-text-primary truncate mb-1">{{ f.name }}</p>
              <select
                v-model="selectedTrackId[f.path]"
                class="w-full px-3 py-1.5 rounded-md bg-bg-primary border border-border text-xs text-text-primary focus:outline-none focus:border-accent"
              >
                <option v-for="c in f.candidates" :key="c.id" :value="c.id">{{ c.label }}</option>
              </select>
            </div>
          </div>

          <label class="flex items-center gap-2 text-xs text-text-secondary mt-4">
            <input v-model="autoMerge" type="checkbox" class="accent-accent" />
            {{ t("autoTranslate.autoMerge") }}
          </label>
          <label v-if="autoMerge && showSetDefaultSubtitle" class="flex items-center gap-2 text-xs text-text-secondary mt-2">
            <input v-model="setDefaultSubtitle" type="checkbox" class="accent-accent" />
            {{ t("autoTranslate.setDefaultSubtitle") }}
          </label>

          <p class="text-xs text-text-muted mt-3">
            {{ t("autoTranslate.filesCount", { count: files.length * targetLangs.length }) }}
          </p>
        </template>
      </div>

      <div v-else class="space-y-2">
        <div
          v-for="job in fileJobs"
          :key="`${job.path}::${job.lang}`"
          class="p-3 rounded-lg bg-bg-secondary border border-border"
        >
          <div class="flex items-center justify-between gap-2">
            <span class="text-sm text-text-primary truncate">{{ job.name }}</span>
            <span class="inline-flex items-center gap-1.5 text-xs shrink-0">
              <CheckCircle2 v-if="job.stage === 'done'" class="w-3.5 h-3.5 text-success" />
              <XCircle v-else-if="job.stage === 'error'" class="w-3.5 h-3.5 text-error" />
              <Loader2 v-else class="w-3.5 h-3.5 text-accent animate-spin" />
              <span :class="job.stage === 'error' ? 'text-error' : 'text-text-secondary'">
                {{ stageLabel(job.stage) }}
              </span>
            </span>
          </div>
          <div class="mt-2 h-1.5 rounded-full bg-bg-tertiary overflow-hidden">
            <div
              class="h-full bg-accent transition-all"
              :class="{ 'bg-error': job.stage === 'error' }"
              :style="{ width: `${job.progress}%` }"
            />
          </div>
          <p v-if="job.message" class="text-xs text-text-muted mt-1.5">{{ job.message }}</p>
        </div>
      </div>
    </div>

    <template #footer>
      <button
        class="inline-flex items-center gap-1.5 px-3 py-2 rounded-lg bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors"
        @click="close"
      >
        {{ running ? t("common.close") : t("common.cancel") }}
      </button>
      <button
        v-if="!running"
        :disabled="starting || providerOptions.length === 0 || files.length === 0 || targetLangs.length === 0"
        class="inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-accent text-white text-xs font-medium hover:bg-accent-hover active:bg-accent-active transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        @click="startPipeline"
      >
        {{ t("autoTranslate.start") }}
      </button>
    </template>
  </Modal>
</template>
