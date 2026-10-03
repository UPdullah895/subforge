<script setup lang="ts">
import { AlertTriangle, CheckCircle2, Info, X } from "lucide-vue-next";
import { useI18n } from "vue-i18n";
import { useToastStore } from "../stores/toastStore";

const { t } = useI18n();
const toastStore = useToastStore();

const icons = { success: CheckCircle2, error: AlertTriangle, info: Info };
const tones = {
  success: "border-success/30 text-success",
  error: "border-error/40 text-error",
  info: "border-info/30 text-info",
};
</script>

<template>
  <!-- role=status/alert so screen readers announce results of actions that
       otherwise only change something off-screen. -->
  <div
    class="fixed bottom-4 end-4 z-[60] flex flex-col gap-2 w-[min(24rem,calc(100vw-2rem))] pointer-events-none"
    aria-live="polite"
  >
    <TransitionGroup name="toast">
      <div
        v-for="toast in toastStore.toasts"
        :key="toast.id"
        :role="toast.kind === 'error' ? 'alert' : 'status'"
        class="pointer-events-auto flex items-start gap-3 px-4 py-3 rounded-lg bg-bg-secondary border shadow-lg"
        :class="tones[toast.kind]"
      >
        <component :is="icons[toast.kind]" class="w-4 h-4 mt-0.5 shrink-0" />
        <div class="flex-1 min-w-0">
          <p class="text-sm font-medium text-text-primary break-words">{{ toast.message }}</p>
          <p
            v-if="toast.detail"
            dir="auto"
            class="mt-0.5 text-xs text-text-secondary break-all line-clamp-3"
            :title="toast.detail"
          >
            {{ toast.detail }}
          </p>
        </div>
        <button
          class="text-text-muted hover:text-text-primary shrink-0"
          :aria-label="t('common.close')"
          @click="toastStore.dismiss(toast.id)"
        >
          <X class="w-4 h-4" />
        </button>
      </div>
    </TransitionGroup>
  </div>
</template>

<style scoped>
.toast-enter-active,
.toast-leave-active {
  transition: all 0.2s ease;
}
.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateY(8px);
}
@media (prefers-reduced-motion: reduce) {
  .toast-enter-active,
  .toast-leave-active {
    transition: none;
  }
}
</style>
