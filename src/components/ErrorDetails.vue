<script setup lang="ts">
import { useI18n } from "vue-i18n";

// Raw tool output (stderr etc.) behind a user-facing error, collapsed by default.
defineProps<{ details?: string | null }>();
const { t } = useI18n();
</script>

<template>
  <details v-if="details" data-testid="error-details" class="mt-1 text-xs text-text-muted">
    <summary class="cursor-pointer select-none">{{ t("common.details") }}</summary>
    <pre
      dir="ltr"
      class="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-all select-text text-[11px] text-text-secondary"
      >{{ details }}</pre
    >
  </details>
</template>
