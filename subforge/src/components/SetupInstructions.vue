<script setup lang="ts">
import { X } from "lucide-vue-next";
import { useI18n } from "vue-i18n";

const { t } = useI18n();

defineProps<{
  title: string;
  what: string;
  setup: string;
  security: string;
}>();

const emit = defineEmits<{
  (e: "close"): void;
}>();
</script>

<template>
  <div class="p-4 rounded-lg bg-bg-secondary border border-border space-y-2">
    <div class="flex items-center justify-between">
      <h3 class="text-sm font-semibold text-text-primary">{{ title }}</h3>
      <button
        class="text-text-muted hover:text-text-secondary transition-colors"
        @click="emit('close')"
      >
        <X class="w-4 h-4" />
      </button>
    </div>
    <p class="text-xs text-text-secondary">
      <strong class="text-text-primary">{{ t("translate.what") }}:</strong>
      {{ what }}
    </p>
    <p class="text-xs text-text-secondary">
      <strong class="text-text-primary">{{ t("translate.setup") }}:</strong>
      {{ setup }}
    </p>
    <p class="text-xs text-text-secondary">
      <strong class="text-text-primary">{{ t("translate.security") }}:</strong>
      {{ security }}
    </p>
  </div>
</template>
