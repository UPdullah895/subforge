<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { Package, RefreshCw } from "lucide-vue-next";
import { useDepsStore } from "../stores/depsStore";
import DependencyList from "./DependencyList.vue";

const { t } = useI18n();
const deps = useDepsStore();
</script>

<template>
  <section class="space-y-3">
    <div class="flex items-center justify-between">
      <div class="flex items-center gap-2">
        <Package class="w-4 h-4 text-accent" />
        <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">{{ t("deps.title") }}</h2>
      </div>
      <button
        class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors"
        :disabled="deps.checking || deps.installing !== null"
        @click="deps.check()"
      >
        <RefreshCw :class="['w-3.5 h-3.5', deps.checking && 'animate-spin']" />
        {{ t("deps.recheck") }}
      </button>
    </div>
    <div class="p-4 rounded-lg bg-bg-secondary border border-border space-y-3">
      <p v-if="!deps.report" class="text-xs text-text-muted">{{ t("deps.checking") }}</p>
      <template v-else>
        <p v-if="!deps.missing.length" class="text-xs text-success">{{ t("deps.allGood") }}</p>
        <DependencyList
          :items="deps.report.items"
          :installing="deps.installing"
          :logs="deps.logs"
          :errors="deps.errors"
          @install="deps.install($event)"
        />
      </template>
    </div>
  </section>
</template>
