<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { RefreshCw } from "lucide-vue-next";
import Modal from "./Modal.vue";
import DependencyList from "./DependencyList.vue";
import { useDepsStore } from "../stores/depsStore";

const { t } = useI18n();
const deps = useDepsStore();

// Required problems first, then optional ones, so the important rows lead.
const sorted = computed(() =>
  [...(deps.report?.items ?? [])]
    .filter((i) => i.state !== "ok")
    .sort((a, b) => Number(b.severity === "required") - Number(a.severity === "required"))
);
// Matches installAll, which only installs required items.
const canInstallAny = computed(() => deps.missingRequired.some((i) => i.can_auto_install));
</script>

<template>
  <Modal :open="deps.showDialog" :title="t('deps.title')" @close="deps.dismiss()">
    <div data-testid="deps-dialog" class="space-y-4">
      <p class="text-sm text-text-secondary">{{ t("deps.intro") }}</p>
      <DependencyList
        :items="sorted"
        :installing="deps.installing"
        :logs="deps.logs"
        :errors="deps.errors"
        @install="deps.install($event)"
      />
    </div>
    <template #footer>
      <button
        class="inline-flex items-center gap-1.5 px-3 py-2 rounded-lg bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors"
        :disabled="deps.checking || deps.installing !== null"
        @click="deps.check()"
      >
        <RefreshCw :class="['w-3.5 h-3.5', deps.checking && 'animate-spin']" />
        {{ t("deps.recheck") }}
      </button>
      <button
        data-testid="deps-later"
        class="px-3 py-2 rounded-lg bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors"
        @click="deps.dismiss()"
      >
        {{ t("deps.later") }}
      </button>
      <button
        v-if="canInstallAny"
        data-testid="deps-install-all"
        class="px-4 py-2 rounded-lg bg-accent text-white text-xs font-medium hover:bg-accent-hover active:bg-accent-active transition-colors disabled:opacity-50"
        :disabled="deps.installing !== null"
        @click="deps.installAll()"
      >
        {{ t("deps.installAll") }}
      </button>
    </template>
  </Modal>
</template>
