<script setup lang="ts">
import { AlertTriangle, X } from "lucide-vue-next";
defineProps<{ message: string | null }>();
const emit = defineEmits<{ dismiss: [] }>();
</script>

<template>
  <div
    v-if="message"
    class="flex items-start gap-3 px-4 py-4 rounded-lg bg-error/10 border border-error/20"
  >
    <AlertTriangle class="w-5 h-5 text-error shrink-0 mt-0.5" />
    <div class="flex-1 min-w-0">
      <p class="text-sm font-medium text-error">{{ message }}</p>
    </div>
    <button
      class="text-error/60 hover:text-error transition-colors shrink-0"
      @click="emit('dismiss')"
    >
      <X class="w-4 h-4" />
    </button>
  </div>
</template>
