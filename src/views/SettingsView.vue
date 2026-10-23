<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { useI18n } from "vue-i18n";
import { localizedLanguageName, TRANSLATE_LANG_CODES } from "../utils/languageNames";
import { useAppStore } from "../stores/appStore";
import { useTranslateStore } from "../stores/translateStore";
import { useSubtitleSourcesStore } from "../stores/subtitleSourcesStore";
import { apiInvoke } from "../api/client";
import SetupInstructions from "../components/SetupInstructions.vue";
import DependenciesPanel from "../components/DependenciesPanel.vue";
import ToolPathField from "../components/ToolPathField.vue";
import {
  Palette,
  Languages,
  FolderOpen,
  Wrench,
  Info,
  Save,
  RotateCcw,
  Search,
  KeyRound,
  Eye,
  EyeOff,
  CheckCircle2,
  XCircle,
  Loader2,
  HelpCircle,
} from "lucide-vue-next";
import { open as openDialog } from "../utils/dialog";

const { t, locale } = useI18n();

const appStore = useAppStore();
const translateStore = useTranslateStore();
const subtitleSourcesStore = useSubtitleSourcesStore();

const osApiKey = ref("");
const osUsername = ref("");
const osPassword = ref("");

type ProviderId = "tmdb" | "tvmaze" | "deepl" | "openai" | "google" | "ollama";

const providers: { id: ProviderId; label: string; hasKey: boolean }[] = [
  { id: "tmdb", label: "TMDB", hasKey: true },
  { id: "tvmaze", label: "TVmaze", hasKey: false },
  { id: "deepl", label: "DeepL", hasKey: true },
  { id: "openai", label: "OpenAI", hasKey: true },
  { id: "google", label: "Google Translate", hasKey: true },
  { id: "ollama", label: "Ollama", hasKey: false },
];

const showKey = reactive<Record<ProviderId, boolean>>({
  tmdb: false,
  tvmaze: false,
  deepl: false,
  openai: false,
  google: false,
  ollama: false,
});

const testStatus = reactive<Record<ProviderId, "idle" | "testing" | "success" | "error">>({
  tmdb: "idle",
  tvmaze: "idle",
  deepl: "idle",
  openai: "idle",
  google: "idle",
  ollama: "idle",
});

const showSetupFor = ref<ProviderId | null>(null);

async function testProvider(provider: ProviderId) {
  testStatus[provider] = "testing";
  try {
    await apiInvoke("test_provider_config", { provider });
    testStatus[provider] = "success";
  } catch {
    testStatus[provider] = "error";
  }
}

const engines = [
  { id: "ollama", label: "Ollama" },
  { id: "libretranslate", label: "LibreTranslate" },
  { id: "deepl", label: "DeepL" },
  { id: "openai", label: "OpenAI" },
  { id: "google", label: "Google Translate" },
];

const languageOptions = computed(() => [
  { code: "auto", label: t("translate.autoDetectLanguage") },
  ...TRANSLATE_LANG_CODES.map((code) => ({ code, label: localizedLanguageName(code, locale.value) })),
]);
// "Auto-detect" only makes sense for the source language.
const targetLanguageOptions = computed(() => languageOptions.value.filter((l) => l.code !== "auto"));

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

// Only refreshes what is shown; it never writes paths into the config (a
// PATH-equal custom path would shadow the bundled tools).
async function autoDetectTools() {
  await appStore.loadToolStatus();
}

const TOOL_IDS = ["mkvmerge", "mkvextract", "mkvpropedit", "ffmpeg", "ffprobe"] as const;
type ToolId = (typeof TOOL_IDS)[number];

function statusFor(id: ToolId) {
  return appStore.toolStatus.find((s) => s.id === id) ?? null;
}

function setToolPath(id: ToolId, value: string | undefined) {
  if (appStore.config) appStore.config[`${id}_path` as const] = value;
}

async function onToolPathChange() {
  await appStore.saveConfig();
  await appStore.loadToolStatus();
}

async function saveSettings() {
  await appStore.saveConfig();
  saved.value = true;
  setTimeout(() => (saved.value = false), 2000);
  await appStore.loadToolStatus();
}

function resetDefaults() {
  // Reset preferences only; keep API keys, provider URLs and credentials so
  // pressing Save afterwards doesn't silently wipe them.
  appStore.config = {
    ...appStore.config,
    theme: "dark",
    language: "en",
    default_engine: "ollama",
    default_source_lang: "en",
    default_target_lang: "ar",
    default_output_dir: undefined,
    mkvmerge_path: undefined,
    mkvextract_path: undefined,
    mkvpropedit_path: undefined,
    ffmpeg_path: undefined,
    ffprobe_path: undefined,
  };
  void appStore.loadToolStatus();
}

function setupContentFor(id: ProviderId) {
  if (id === "tmdb" || id === "tvmaze") {
    return {
      what: t(`settings.${id}Desc`),
      setup: t(`settings.${id}Setup`),
      security: t(`settings.${id}Security`),
    };
  }
  return {
    what: t(`translate.${id}Desc`),
    setup: t(`translate.${id}Setup`),
    security: t(`translate.${id}Security`),
  };
}

onMounted(() => {
  appStore.loadConfig();
  appStore.loadToolStatus();
  subtitleSourcesStore.loadStatus();
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
                <option v-for="lang in targetLanguageOptions" :key="lang.code" :value="lang.code">
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
          <label class="flex items-start gap-2 cursor-pointer">
            <input
              type="checkbox"
              class="mt-0.5 accent-accent"
              :checked="appStore.config.fix_rtl_punctuation !== false"
              @change="appStore.config.fix_rtl_punctuation = ($event.target as HTMLInputElement).checked"
            />
            <span>
              <span class="block text-sm text-text-primary">{{ t("settings.fixRtlPunctuation") }}</span>
              <span class="block text-xs text-text-muted">{{ t("settings.fixRtlPunctuationHint") }}</span>
            </span>
          </label>
        </div>
      </section>

      <DependenciesPanel />

      <!-- Tool Paths -->
      <section class="space-y-3">
        <div class="flex items-center gap-2">
          <Wrench class="w-4 h-4 text-accent" />
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">{{ t("settings.toolPaths") }}</h2>
        </div>
        <div class="p-4 rounded-lg bg-bg-secondary border border-border space-y-3">
          <div class="flex items-center justify-between gap-3">
            <p class="text-xs text-text-muted">
              {{ t("settings.toolPathsDesc") }}
            </p>
            <button
              class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors shrink-0"
              @click="autoDetectTools"
            >
              <Search class="w-3.5 h-3.5" />
              {{ t("settings.autoDetect") }}
            </button>
          </div>
          <ToolPathField
            v-for="id in TOOL_IDS"
            :key="id"
            :tool-id="id"
            :model-value="appStore.config[`${id}_path` as const]"
            :status="statusFor(id)"
            @update:model-value="setToolPath(id, $event)"
            @change="onToolPathChange"
          />
        </div>
      </section>

      <!-- API & Providers -->
      <section class="space-y-3">
        <div class="flex items-center gap-2">
          <KeyRound class="w-4 h-4 text-accent" />
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">{{ t("settings.apiProviders") }}</h2>
        </div>
        <p class="text-xs text-text-muted -mt-1">{{ t("settings.apiProvidersDesc") }}</p>
        <div class="space-y-3">
          <div
            v-for="provider in providers"
            :key="provider.id"
            class="p-4 rounded-lg bg-bg-secondary border border-border space-y-3"
          >
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-2">
                <p class="text-sm font-medium text-text-primary">{{ provider.label }}</p>
                <button
                  class="text-text-muted hover:text-text-secondary transition-colors"
                  :title="t('settings.setupInstructions')"
                  @click="showSetupFor = showSetupFor === provider.id ? null : provider.id"
                >
                  <HelpCircle class="w-3.5 h-3.5" />
                </button>
              </div>
              <div class="flex items-center gap-2">
                <span v-if="testStatus[provider.id] === 'success'" class="inline-flex items-center gap-1 text-xs text-success">
                  <CheckCircle2 class="w-3.5 h-3.5" />
                  {{ t("settings.testSuccess") }}
                </span>
                <span v-else-if="testStatus[provider.id] === 'error'" class="inline-flex items-center gap-1 text-xs text-error">
                  <XCircle class="w-3.5 h-3.5" />
                  {{ t("settings.testFailed") }}
                </span>
                <button
                  class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors disabled:opacity-50"
                  :disabled="testStatus[provider.id] === 'testing'"
                  @click="testProvider(provider.id)"
                >
                  <Loader2 v-if="testStatus[provider.id] === 'testing'" class="w-3.5 h-3.5 animate-spin" />
                  {{ testStatus[provider.id] === "testing" ? t("settings.testing") : t("settings.testConnection") }}
                </button>
              </div>
            </div>

            <!-- TMDB -->
            <div v-if="provider.id === 'tmdb' && appStore.config">
              <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("settings.tmdbApiKey") }}</label>
              <div class="relative">
                <input
                  v-model="appStore.config.tmdb_api_key"
                  :type="showKey.tmdb ? 'text' : 'password'"
                  :placeholder="t('settings.tmdbPlaceholder')"
                  class="w-full px-4 py-2.5 pr-10 rounded-lg bg-bg-primary border border-border text-sm text-text-primary placeholder-text-muted focus:outline-none focus:border-accent"
                />
                <button
                  class="absolute right-3 top-1/2 -translate-y-1/2 text-text-muted hover:text-text-secondary"
                  @click="showKey.tmdb = !showKey.tmdb"
                >
                  <EyeOff v-if="showKey.tmdb" class="w-4 h-4" />
                  <Eye v-else class="w-4 h-4" />
                </button>
              </div>
            </div>

            <!-- TVmaze -->
            <p v-else-if="provider.id === 'tvmaze'" class="text-xs text-text-muted">
              {{ t("settings.tvmazeNoKey") }}
            </p>

            <!-- DeepL -->
            <div v-else-if="provider.id === 'deepl'" class="space-y-3">
              <div>
                <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("translate.apiKey") }}</label>
                <div class="relative">
                  <input
                    v-model="translateStore.deeplKey"
                    :type="showKey.deepl ? 'text' : 'password'"
                    :placeholder="t('translate.deeplPlaceholder')"
                    class="w-full px-4 py-2.5 pr-10 rounded-lg bg-bg-primary border border-border text-sm text-text-primary placeholder-text-muted focus:outline-none focus:border-accent"
                  />
                  <button
                    class="absolute right-3 top-1/2 -translate-y-1/2 text-text-muted hover:text-text-secondary"
                    @click="showKey.deepl = !showKey.deepl"
                  >
                    <EyeOff v-if="showKey.deepl" class="w-4 h-4" />
                    <Eye v-else class="w-4 h-4" />
                  </button>
                </div>
              </div>
              <label class="flex items-center gap-2 text-xs text-text-secondary">
                <input v-model="translateStore.deeplPro" type="checkbox" class="accent-accent" />
                {{ t("translate.proAccount") }}
              </label>
            </div>

            <!-- OpenAI -->
            <div v-else-if="provider.id === 'openai'" class="space-y-3">
              <div>
                <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("translate.apiKey") }}</label>
                <div class="relative">
                  <input
                    v-model="translateStore.openaiKey"
                    :type="showKey.openai ? 'text' : 'password'"
                    :placeholder="t('translate.openaiPlaceholder')"
                    class="w-full px-4 py-2.5 pr-10 rounded-lg bg-bg-primary border border-border text-sm text-text-primary placeholder-text-muted focus:outline-none focus:border-accent"
                  />
                  <button
                    class="absolute right-3 top-1/2 -translate-y-1/2 text-text-muted hover:text-text-secondary"
                    @click="showKey.openai = !showKey.openai"
                  >
                    <EyeOff v-if="showKey.openai" class="w-4 h-4" />
                    <Eye v-else class="w-4 h-4" />
                  </button>
                </div>
              </div>
              <div>
                <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("translate.modelName") }}</label>
                <input
                  v-model="translateStore.openaiModel"
                  type="text"
                  class="w-full px-4 py-2.5 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
                />
              </div>
            </div>

            <!-- Google Translate -->
            <div v-else-if="provider.id === 'google'">
              <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("translate.apiKey") }}</label>
              <div class="relative">
                <input
                  v-model="translateStore.googleApiKey"
                  :type="showKey.google ? 'text' : 'password'"
                  :placeholder="t('translate.googlePlaceholder')"
                  class="w-full px-4 py-2.5 pr-10 rounded-lg bg-bg-primary border border-border text-sm text-text-primary placeholder-text-muted focus:outline-none focus:border-accent"
                />
                <button
                  class="absolute right-3 top-1/2 -translate-y-1/2 text-text-muted hover:text-text-secondary"
                  @click="showKey.google = !showKey.google"
                >
                  <EyeOff v-if="showKey.google" class="w-4 h-4" />
                  <Eye v-else class="w-4 h-4" />
                </button>
              </div>
            </div>

            <!-- Ollama -->
            <div v-else-if="provider.id === 'ollama'">
              <label class="block text-xs font-medium text-text-secondary mb-1.5">{{ t("translate.hostUrl") }}</label>
              <input
                v-model="translateStore.ollamaHost"
                type="text"
                class="w-full px-4 py-2.5 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
              />
            </div>

            <SetupInstructions
              v-if="showSetupFor === provider.id"
              :title="t('translate.about', { engine: provider.label })"
              v-bind="setupContentFor(provider.id)"
              @close="showSetupFor = null"
            />
          </div>
        </div>
      </section>

      <!-- Subtitle Sources -->
      <section>
        <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">{{ t("settings.subtitleSources") }}</h2>
        <p class="text-xs text-text-muted mt-1 mb-4">{{ t("settings.subtitleSourcesDesc") }}</p>

        <label class="flex items-center gap-2 text-sm text-text-secondary mb-4">
          <input
            type="checkbox"
            class="accent-accent"
            :checked="subtitleSourcesStore.searchEnabled()"
            @change="subtitleSourcesStore.setSearchEnabled(($event.target as HTMLInputElement).checked)"
          />
          {{ t("settings.searchOnlineToggle") }}
        </label>

        <div class="flex items-center justify-between px-3 py-2 rounded-lg bg-bg-primary border border-border mb-3">
          <span class="text-sm text-text-primary">{{ t("settings.subsourceStatus") }}</span>
          <span
            :class="subtitleSourcesStore.subsourceReachable ? 'text-success' : 'text-error'"
            class="text-xs"
          >
            {{ subtitleSourcesStore.subsourceReachable ? t("settings.subsourceReachable") : t("settings.subsourceUnreachable") }}
          </span>
        </div>

        <div class="p-3 rounded-lg bg-bg-secondary border border-border space-y-2">
          <div class="flex items-center justify-between">
            <span class="text-sm text-text-primary">{{ t("settings.openSubtitlesTitle") }}</span>
            <span v-if="subtitleSourcesStore.opensubtitlesConfigured" class="text-xs text-success">
              {{ t("settings.openSubtitlesConfigured") }}
            </span>
          </div>
          <p class="text-xs text-text-muted">
            {{ t("settings.openSubtitlesHelp") }}
            <a href="https://www.opensubtitles.com" target="_blank" rel="noopener" class="text-accent hover:underline">{{ t("settings.openSubtitlesRegisterLink") }}</a>
            &middot;
            <a href="https://opensubtitles.stoplight.io" target="_blank" rel="noopener" class="text-accent hover:underline">{{ t("settings.openSubtitlesDocsLink") }}</a>
          </p>
          <input
            v-model="osApiKey"
            type="text"
            :placeholder="t('settings.openSubtitlesApiKey')"
            class="w-full px-3 py-2 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
          />
          <input
            v-model="osUsername"
            type="text"
            :placeholder="t('settings.openSubtitlesUsername')"
            class="w-full px-3 py-2 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
          />
          <input
            v-model="osPassword"
            type="password"
            :placeholder="t('settings.openSubtitlesPassword')"
            class="w-full px-3 py-2 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
          />
          <div class="flex items-center gap-2">
            <button
              class="px-3 py-1.5 rounded-md bg-accent text-white text-xs font-medium hover:bg-accent-hover transition-colors"
              @click="async () => { await subtitleSourcesStore.setOpenSubtitlesCredentials({ api_key: osApiKey, username: osUsername, password: osPassword }); await subtitleSourcesStore.testOpenSubtitlesConnection(); }"
            >
              {{ t("settings.openSubtitlesSave") }}
            </button>
            <button
              v-if="subtitleSourcesStore.opensubtitlesConfigured"
              class="px-3 py-1.5 rounded-md bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors"
              @click="subtitleSourcesStore.clearOpenSubtitlesCredentials"
            >
              {{ t("settings.openSubtitlesClear") }}
            </button>
            <span v-if="subtitleSourcesStore.testStatus === 'success'" class="text-xs text-success">{{ t("settings.testSuccess") }}</span>
            <span v-else-if="subtitleSourcesStore.testStatus === 'error'" class="text-xs text-error">{{ subtitleSourcesStore.testError }}</span>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>
