<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useAppStore } from "./stores/appStore";
import AppSidebar from "./components/AppSidebar.vue";
import SubtitleView from "./views/SubtitleView.vue";
import AudioView from "./views/AudioView.vue";
import BatchView from "./views/BatchView.vue";
import TranscribeView from "./views/TranscribeView.vue";
import TranslateView from "./views/TranslateView.vue";

const appStore = useAppStore();
const currentView = ref<"subtitle" | "audio" | "batch" | "transcribe" | "translate">("subtitle");

onMounted(() => {
  appStore.loadConfig();
});
</script>

<template>
  <div class="flex h-full bg-bg-primary">
    <AppSidebar :current="currentView" @navigate="currentView = $event" />
    <main class="flex-1 min-w-0 overflow-hidden">
      <SubtitleView v-if="currentView === 'subtitle'" />
      <AudioView v-else-if="currentView === 'audio'" />
      <BatchView v-else-if="currentView === 'batch'" />
      <TranscribeView v-else-if="currentView === 'transcribe'" />
      <TranslateView v-else-if="currentView === 'translate'" />
      <div
        v-else
        class="flex flex-col items-center justify-center h-full text-center"
      >
        <div class="w-16 h-16 rounded-2xl bg-bg-secondary border border-border flex items-center justify-center mb-4">
          <span class="text-2xl">🚧</span>
        </div>
        <h2 class="text-lg font-medium text-text-primary">Coming Soon</h2>
        <p class="text-sm text-text-secondary mt-1">This feature is planned for a future phase.</p>
      </div>
    </main>
  </div>
</template>
