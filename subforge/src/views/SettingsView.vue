<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useAppStore } from "../stores/appStore";
import {
  Palette,
  Languages,
  FolderOpen,
  Wrench,
  Info,
  Save,
  RotateCcw,
} from "lucide-vue-next";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

const { t } = useI18n();

const appStore = useAppStore();

const engines = [
  { id: "ollama", label: "Ollama" },
  { id: "libretranslate", label: "LibreTranslate" },
  { id: "deepl", label: "DeepL" },
  { id: "openai", label: "OpenAI" },
];

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

const saved = ref(false);

async function pickOutputDir() {
  const selected = await openDialog({
    multiple: false,
    directory: true,
    title: t("settings.defaultOutputDirectory"),
  });
  if (selected && typeof selected === "string") {
    appStore.config!.default_output_dir = selected;
  }
}

async function saveSettings() {
  await appStore.saveConfig();
  saved.value = true;
  setTimeout(() => (saved.value = false), 2000);
}

function resetDefaults() {
  appStore.config = {
    theme: "dark",
    language: "en",
    default_engine: "ollama",
    default_source_lang: "en",
    default_target_lang: "ar",
    default_output_dir: undefined,
    mkvmerge_path: undefined,
    mkvextract_path: undefined,
    ffmpeg_path: undefined,
    ffprobe_path: undefined,
  };
}

onMounted(() => {
  appStore.loadConfig();
});
</script>

<template>
  <div class="flex flex-col h-full">
    <!-- Header -->
    <div class="flex items-center justify-between px-6 py-4 border-b border-border">
      <div class="flex items-center gap-2">
        <h1 class="text-xl font-semibold text-text-primary">{{ t("settings.title") }}</h1>
        <span
          class="text-text-muted hover:text-text-secondary cursor-help"
          :title="t('settings.tooltip')"
        >
          <Info class="w-4 h-4" />
        </span>
      </div>
      <div class="flex items-center gap-2">
        <button
          class="inline-flex items-center gap-1.5 px-3 py-2 rounded-lg bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors"
          @click="resetDefaults"
        >
          <RotateCcw class="w-3.5 h-3.5" />
          {{ t("settings.reset") }}
        </button>
        <button
          class="inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-accent text-white text-xs font-medium hover:bg-accent-hover active:bg-accent-active transition-colors"
          @click="saveSettings"
        >
          <Save class="w-3.5 h-3.5" />
          {{ saved ? t("settings.saved") : t("settings.save") }}
        </button>
      </div>
    </div>

    <!-- Content -->
    <div v-if="appStore.config" class="flex-1 overflow-auto p-6 space-y-8">
      <!-- Appearance -->
      <section class="space-y-3">
        <div class="flex items-center gap-2">
          <Palette class="w-4 h-4 text-accent" />
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">{{ t("settings.appearance") }}</h2>
        </div>
        <div class="p-4 rounded-lg bg-bg-secondary border border-border space-y-4">
          <div class="flex items-center justify-between">
            <div>
              <p class="text-sm font-medium text-text-primary">{{ t("settings.theme") }}</p>
              <p class="text-xs text-text-muted">{{ t("settings.themeDesc") }}</p>
            </div>
            <button
              class="px-3 py-1.5 rounded-md bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors"
              @click="appStore.toggleTheme"
            >
              {{ appStore.theme === "dark" ? t("settings.dark") : t("settings.light") }}
            </button>
          </div>
          <div class="border-t border-border pt-4 flex items-center justify-between">
            <div>
              <p class="text-sm font-medium text-text-primary">{{ t("settings.language") }}</p>
              <p class="text-xs text-text-muted">{{ t("settings.languageDesc") }}</p>
            </div>
            <select
              :value="appStore.language"
              class="px-3 py-1.5 rounded-md bg-bg-tertiary text-text-secondary text-xs font-medium border border-border focus:outline-none focus:border-accent"
              @change="(e) => appStore.setLanguage((e.target as HTMLSelectElement).value as 'en' | 'ar')"
            >
              <option value="en">English</option>
              <option value="ar">العربية</option>
            </select>
          </div>
        </div>
      </section>

      <!-- Translation Defaults -->
      <section class="space-y-3">
        <div class="flex items-center gap-2">
          <Languages class="w-4 h-4 text-accent" />
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">{{ t("settings.translationDefaults") }}</h2>
        </div>
        <div class="p-4 rounded-lg bg-bg-secondary border border-border space-y-4">
          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("settings.defaultEngine") }}</label>
            <select
              v-model="appStore.config.default_engine"
              class="w-full px-4 py-2.5 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
            >
              <option v-for="e in engines" :key="e.id" :value="e.id">{{ e.label }}</option>
            </select>
          </div>
          <div class="grid grid-cols-2 gap-3">
            <div>
              <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("settings.defaultSourceLanguage") }}</label>
              <select
                v-model="appStore.config.default_source_lang"
                class="w-full px-4 py-2.5 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
              >
                <option v-for="lang in languageOptions" :key="lang.code" :value="lang.code">
                  {{ lang.label }}
                </option>
              </select>
            </div>
            <div>
              <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("settings.defaultTargetLanguage") }}</label>
              <select
                v-model="appStore.config.default_target_lang"
                class="w-full px-4 py-2.5 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
              >
                <option v-for="lang in languageOptions" :key="lang.code" :value="lang.code">
                  {{ lang.label }}
                </option>
              </select>
            </div>
          </div>
          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("settings.defaultOutputDirectory") }}</label>
            <div class="flex items-center gap-2">
              <input
                :value="appStore.config.default_output_dir || t('settings.sameAsInputFile')"
                readonly
                class="flex-1 px-4 py-2.5 rounded-lg bg-bg-primary border border-border text-sm text-text-primary cursor-default"
              />
              <button
                class="inline-flex items-center gap-1.5 px-3 py-2.5 rounded-lg bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors shrink-0"
                @click="pickOutputDir"
              >
                <FolderOpen class="w-3.5 h-3.5" />
                {{ t("common.browse") }}
              </button>
            </div>
          </div>
        </div>
      </section>

      <!-- Tool Paths -->
      <section class="space-y-3">
        <div class="flex items-center gap-2">
          <Wrench class="w-4 h-4 text-accent" />
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">{{ t("settings.toolPaths") }}</h2>
        </div>
        <div class="p-4 rounded-lg bg-bg-secondary border border-border space-y-3">
          <p class="text-xs text-text-muted">
            {{ t("settings.toolPathsDesc") }}
          </p>
          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("settings.mkvmerge") }}</label>
            <input
              v-model="appStore.config.mkvmerge_path"
              type="text"
              :placeholder="t('settings.mkvmergePlaceholder')"
              class="w-full px-4 py-2.5 rounded-lg bg-bg-primary border border-border text-sm text-text-primary placeholder-text-muted focus:outline-none focus:border-accent"
            />
          </div>
          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("settings.mkvextract") }}</label>
            <input
              v-model="appStore.config.mkvextract_path"
              type="text"
              :placeholder="t('settings.mkvextractPlaceholder')"
              class="w-full px-4 py-2.5 rounded-lg bg-bg-primary border border-border text-sm text-text-primary placeholder-text-muted focus:outline-none focus:border-accent"
            />
          </div>
          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("settings.ffmpeg") }}</label>
            <input
              v-model="appStore.config.ffmpeg_path"
              type="text"
              :placeholder="t('settings.ffmpegPlaceholder')"
              class="w-full px-4 py-2.5 rounded-lg bg-bg-primary border border-border text-sm text-text-primary placeholder-text-muted focus:outline-none focus:border-accent"
            />
          </div>
          <div>
            <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("settings.ffprobe") }}</label>
            <input
              v-model="appStore.config.ffprobe_path"
              type="text"
              :placeholder="t('settings.ffprobePlaceholder')"
              class="w-full px-4 py-2.5 rounded-lg bg-bg-primary border border-border text-sm text-text-primary placeholder-text-muted focus:outline-none focus:border-accent"
            />
          </div>
        </div>
      </section>
    </div>
  </div>
</template>
