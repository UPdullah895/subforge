<script setup lang="ts">
import { FileDown } from "lucide-vue-next";

withDefaults(defineProps<{ show: boolean; label?: string }>(), { label: "" });
</script>

<template>
  <div
    v-if="show"
    class="pointer-events-none absolute inset-0 z-40 m-2 flex items-center justify-center rounded-xl border-2 border-dashed border-accent bg-bg-primary/85 backdrop-blur-sm"
    data-testid="drop-overlay"
  >
    <div class="flex flex-col items-center gap-2 text-accent">
      <FileDown class="w-10 h-10" />
      <p class="text-sm font-medium" dir="auto">{{ label }}</p>
    </div>
  </div>
</template>
