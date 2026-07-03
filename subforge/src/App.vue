<script setup lang="ts">
import { ref, watch, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { useAppStore } from "./stores/appStore";
import TitleBar from "./components/TitleBar.vue";
import ResizeHandles from "./components/ResizeHandles.vue";
import AppSidebar from "./components/AppSidebar.vue";
import SubtitleView from "./views/SubtitleView.vue";
import AudioView from "./views/AudioView.vue";
import BatchView from "./views/BatchView.vue";
import TranscribeView from "./views/TranscribeView.vue";
import TranslateView from "./views/TranslateView.vue";
import SettingsView from "./views/SettingsView.vue";

const appStore = useAppStore();
const { locale } = useI18n();
const currentView = ref<"subtitle" | "audio" | "batch" | "transcribe" | "translate" | "settings">(
  "subtitle"
);

// style.css themes via a [data-theme="light"] selector on the root element —
// this is the only place that attribute gets set from app state.
watch(
  () => appStore.theme,
  (theme) => {
    document.documentElement.dataset.theme = theme;
  },
  { immediate: true }
);

watch(
  () => appStore.language,
  (language) => {
    locale.value = language;
    document.documentElement.dir = language === "ar" ? "rtl" : "ltr";
    document.documentElement.lang = language;
  },
  { immediate: true }
);

onMounted(() => {
  appStore.loadConfig();
});
</script>

<template>
  <div class="flex flex-col h-full bg-bg-primary">
    <ResizeHandles />
    <TitleBar />
    <div class="flex flex-1 min-h-0">
      <AppSidebar :current="currentView" @navigate="currentView = $event" />
      <main class="flex-1 min-w-0 overflow-hidden">
        <SubtitleView v-if="currentView === 'subtitle'" />
        <AudioView v-else-if="currentView === 'audio'" />
        <BatchView v-else-if="currentView === 'batch'" />
        <TranscribeView v-else-if="currentView === 'transcribe'" />
        <TranslateView v-else-if="currentView === 'translate'" />
        <SettingsView v-else-if="currentView === 'settings'" />
        <div v-else class="flex flex-col items-center justify-center h-full text-center">
          <div
            class="w-16 h-16 rounded-2xl bg-bg-secondary border border-border flex items-center justify-center mb-4"
          >
            <span class="text-2xl">🚧</span>
          </div>
          <h2 class="text-lg font-medium text-text-primary">Coming Soon</h2>
          <p class="text-sm text-text-secondary mt-1">This feature is planned for a future phase.</p>
        </div>
      </main>
    </div>
  </div>
</template>
