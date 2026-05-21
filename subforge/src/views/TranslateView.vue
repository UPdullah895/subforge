<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { open, save } from "@tauri-apps/plugin-dialog";
import { apiListen } from "../api/events";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { useTranslateStore, type EngineType } from "../stores/translateStore";
import { useAppStore } from "../stores/appStore";
import {
  FolderOpen,
  Languages,
  Loader2,
  CheckCircle2,
  XCircle,
  X,
  Play,
  RefreshCw,
  Brain,
  Globe,
  Key,
  Zap,
} from "lucide-vue-next";

const translateStore = useTranslateStore();
const appStore = useAppStore();

const selectedFile = ref("");
const translating = ref(false);

const fileName = computed(() => {
  if (!selectedFile.value) return null;
  const parts = selectedFile.value.split(/[/\\]/);
  return parts[parts.length - 1];
});

const engines: { id: EngineType; label: string; icon: typeof Brain; desc: string }[] = [
  { id: "ollama", label: "Ollama", icon: Brain, desc: "Local LLM via Ollama" },
  { id: "libretranslate", label: "LibreTranslate", icon: Globe, desc: "Self-hosted MT" },
  { id: "deepl", label: "DeepL", icon: Zap, desc: "Cloud API (BYOK)" },
  { id: "openai", label: "OpenAI", icon: Key, desc: "Cloud API (BYOK)" },
];

async function pickFile() {
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "Subtitle files", extensions: ["srt", "ass", "ssa", "vtt"] }],
  });
  if (selected && typeof selected === "string") {
    selectedFile.value = selected;
  }
}

async function startTranslation() {
  if (!selectedFile.value) return;

  const output = await save({
    defaultPath: `translated.srt`,
    filters: [{ name: "Subtitle files", extensions: ["srt", "ass", "ssa", "vtt"] }],
  });

  if (!output || typeof output !== "string") return;

  translating.value = true;
  await translateStore.startTranslation(selectedFile.value, output);
  translating.value = false;
}

let unlistenProgress: UnlistenFn | null = null;
let unlistenCompleted: UnlistenFn | null = null;

onMounted(async () => {
  appStore.discoverTools();
  await translateStore.detectOllama();
  await translateStore.detectLibreTranslate();

  unlistenProgress = await apiListen<{ job_id: string; progress: number; message?: string }>(
    "job-progress",
    ({ job_id, progress, message }) => {
      translateStore.updateJobProgress(job_id, progress, message);
    }
  );

  unlistenCompleted = await apiListen<{ job_id: string; status: string; message?: string }>(
    "job-completed",
    ({ job_id, status, message }) => {
      translateStore.completeJob(job_id, status, message);
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
        <h1 class="text-xl font-semibold text-text-primary">Translate</h1>
        <p class="text-sm text-text-secondary mt-0.5">
          Translate subtitles with AI or machine translation
        </p>
      </div>
      <button
        class="inline-flex items-center gap-2 px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover active:bg-accent-active transition-colors"
        @click="pickFile"
      >
        <FolderOpen class="w-4 h-4" />
        Open Subtitle
      </button>
    </div>

    <!-- Content -->
    <div class="flex-1 overflow-auto p-6 space-y-6">
      <!-- File Info -->
      <div
        v-if="fileName"
        class="flex items-center gap-3 px-4 py-3 rounded-lg bg-bg-secondary border border-border"
      >
        <Languages class="w-5 h-5 text-accent shrink-0" />
        <div class="min-w-0">
          <p class="text-sm font-medium text-text-primary truncate">{{ fileName }}</p>
          <p class="text-xs text-text-muted">Ready to translate</p>
        </div>
      </div>

      <!-- Engine Selector -->
      <div class="space-y-3">
        <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
          Translation Engine
        </h2>
        <div class="grid grid-cols-2 md:grid-cols-4 gap-3">
          <button
            v-for="engine in engines"
            :key="engine.id"
            :class="[
              'flex flex-col items-center gap-2 p-4 rounded-lg border transition-colors text-left',
              translateStore.selectedEngine === engine.id
                ? 'border-accent bg-accent/10 text-accent'
                : 'border-border bg-bg-secondary text-text-secondary hover:bg-bg-hover',
            ]"
            @click="translateStore.selectedEngine = engine.id"
          >
            <component :is="engine.icon" class="w-5 h-5" />
            <span class="text-sm font-medium">{{ engine.label }}</span>
            <span class="text-xs opacity-70">{{ engine.desc }}</span>
          </button>
        </div>
      </div>

      <!-- Engine Settings -->
      <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
        <!-- Ollama Settings -->
        <div v-if="translateStore.selectedEngine === 'ollama'" class="space-y-3">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">Ollama</h2>
          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">Host URL</label>
            <div class="flex items-center gap-2">
              <input
                v-model="translateStore.ollamaHost"
                type="text"
                class="flex-1 px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
              />
              <button
                class="inline-flex items-center gap-1.5 px-3 py-2.5 rounded-lg bg-bg-tertiary text-text-primary text-xs font-medium hover:bg-bg-hover transition-colors"
                @click="translateStore.detectOllama"
              >
                <RefreshCw class="w-3.5 h-3.5" />
                Detect
              </button>
            </div>
          </div>
          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">Model</label>
            <select
              v-model="translateStore.selectedOllamaModel"
              class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
            >
              <option v-for="m in translateStore.ollamaModels" :key="m" :value="m">{{ m }}</option>
            </select>
            <p v-if="translateStore.ollamaModels.length === 0" class="text-xs text-warning mt-1">
              No models found. Make sure Ollama is running.
            </p>
          </div>
        </div>

        <!-- LibreTranslate Settings -->
        <div v-else-if="translateStore.selectedEngine === 'libretranslate'" class="space-y-3">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">LibreTranslate</h2>
          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">Host URL</label>
            <div class="flex items-center gap-2">
              <input
                v-model="translateStore.libreHost"
                type="text"
                class="flex-1 px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
              />
              <button
                class="inline-flex items-center gap-1.5 px-3 py-2.5 rounded-lg bg-bg-tertiary text-text-primary text-xs font-medium hover:bg-bg-hover transition-colors"
                @click="translateStore.detectLibreTranslate"
              >
                <RefreshCw class="w-3.5 h-3.5" />
                Detect
              </button>
            </div>
          </div>
        </div>

        <!-- DeepL Settings -->
        <div v-else-if="translateStore.selectedEngine === 'deepl'" class="space-y-3">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">DeepL</h2>
          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">API Key</label>
            <input
              v-model="translateStore.deeplKey"
              type="password"
              placeholder="DeepL-Auth-Key xxx..."
              class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary placeholder-text-muted focus:outline-none focus:border-accent"
            />
          </div>
          <label class="flex items-center gap-2 cursor-pointer">
            <input
              v-model="translateStore.deeplPro"
              type="checkbox"
              class="rounded border-border bg-bg-primary text-accent focus:ring-accent"
            />
            <span class="text-sm text-text-secondary">Pro account (not free tier)</span>
          </label>
        </div>

        <!-- OpenAI Settings -->
        <div v-else-if="translateStore.selectedEngine === 'openai'" class="space-y-3">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">OpenAI</h2>
          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">API Key</label>
            <input
              v-model="translateStore.openaiKey"
              type="password"
              placeholder="sk-..."
              class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary placeholder-text-muted focus:outline-none focus:border-accent"
            />
          </div>
          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">Model</label>
            <input
              v-model="translateStore.openaiModel"
              type="text"
              class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
            />
          </div>
        </div>

        <!-- Language Pair -->
        <div class="space-y-3">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">Languages</h2>
          <div class="grid grid-cols-2 gap-3">
            <div>
              <label class="block text-xs font-medium text-text-secondary mb-1.5">Source</label>
              <input
                v-model="translateStore.sourceLang"
                type="text"
                placeholder="auto or en, ja..."
                class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary placeholder-text-muted focus:outline-none focus:border-accent"
              />
            </div>
            <div>
              <label class="block text-xs font-medium text-text-secondary mb-1.5">Target</label>
              <input
                v-model="translateStore.targetLang"
                type="text"
                placeholder="es, de, fr..."
                class="w-full px-4 py-2.5 rounded-lg bg-bg-secondary border border-border text-sm text-text-primary placeholder-text-muted focus:outline-none focus:border-accent"
              />
            </div>
          </div>
        </div>
      </div>

      <!-- Translate Button -->
      <button
        :disabled="!selectedFile || translating"
        class="w-full md:w-auto inline-flex items-center justify-center gap-2 px-6 py-3 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover active:bg-accent-active transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        @click="startTranslation"
      >
        <Loader2 v-if="translating" class="w-4 h-4 animate-spin" />
        <Play v-else class="w-4 h-4" />
        {{ translating ? "Starting..." : "Translate" }}
      </button>

      <!-- Jobs -->
      <div v-if="translateStore.jobs.length > 0" class="space-y-2">
        <div class="flex items-center justify-between">
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
            Translation Jobs
          </h2>
          <button
            class="text-xs text-text-muted hover:text-text-secondary transition-colors"
            @click="translateStore.clearCompleted"
          >
            Clear completed
          </button>
        </div>
        <div class="space-y-2">
          <div
            v-for="job in translateStore.jobs"
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
                @click="translateStore.jobs = translateStore.jobs.filter((j) => j.id !== job.id)"
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
        v-if="!selectedFile && translateStore.jobs.length === 0"
        class="flex flex-col items-center justify-center py-16 text-center"
      >
        <div class="w-16 h-16 rounded-2xl bg-bg-secondary border border-border flex items-center justify-center mb-4">
          <Languages class="w-8 h-8 text-text-muted" />
        </div>
        <h3 class="text-base font-medium text-text-primary">No subtitle selected</h3>
        <p class="text-sm text-text-secondary mt-1 max-w-sm">
          Open a subtitle file and choose a translation engine to get started.
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
