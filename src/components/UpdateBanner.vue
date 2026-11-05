<script setup lang="ts">
import { onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { Download, X } from "lucide-vue-next";
import { useUpdateStore } from "../stores/updateStore";
import { openExternal } from "../utils/openExternal";

const { t } = useI18n();
const update = useUpdateStore();

// Fire and forget: a failed or slow check must never delay the window.
onMounted(() => {
  void update.check();
});

async function openReleasePage(): Promise<void> {
  const url = update.available?.url;
  if (!url) return;
  try {
    await openExternal(url);
  } catch {
    // No browser handler (server mode, locked-down desktop) — the URL is in the tooltip.
  }
}
</script>

<template>
  <div
    v-if="update.available"
    data-testid="update-banner"
    class="flex items-center justify-between gap-3 px-4 py-2 bg-accent/15 border-b border-accent/40 text-xs text-text-primary"
    :title="update.available.url"
  >
    <span>{{ t("update.available", { version: update.available.version }) }}</span>
    <div class="flex items-center gap-2">
      <button
        data-testid="update-open"
        class="flex items-center gap-1.5 px-3 py-1 rounded-md bg-accent text-white font-medium hover:opacity-90 transition-opacity"
        @click="openReleasePage"
      >
        <Download :size="13" />
        {{ t("update.open") }}
      </button>
      <button
        data-testid="update-dismiss"
        class="p-1 rounded-md text-text-secondary hover:text-text-primary hover:bg-bg-secondary transition-colors"
        :aria-label="t('update.dismiss')"
        :title="t('update.dismiss')"
        @click="update.dismiss()"
      >
        <X :size="14" />
      </button>
    </div>
  </div>
</template>
