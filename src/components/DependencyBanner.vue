<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { useDepsStore } from "../stores/depsStore";

const { t } = useI18n();
const deps = useDepsStore();
</script>

<template>
  <div
    v-if="deps.showBanner"
    data-testid="deps-banner"
    class="flex items-center justify-between gap-3 px-4 py-2 bg-warning/15 border-b border-warning/40 text-xs text-text-primary"
  >
    <span>{{ t("deps.bannerText") }}</span>
    <button
      data-testid="deps-fix"
      class="px-3 py-1 rounded-md bg-warning text-black font-medium hover:opacity-90 transition-opacity"
      @click="deps.reopen()"
    >
      {{ t("deps.fixNow") }}
    </button>
  </div>
</template>
