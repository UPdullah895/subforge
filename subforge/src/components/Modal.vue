<script setup lang="ts">
import { X } from "lucide-vue-next";

defineProps<{
  open: boolean;
  title: string;
}>();

const emit = defineEmits<{
  (e: "close"): void;
}>();
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      @click.self="emit('close')"
    >
      <div
        class="w-full max-w-2xl max-h-[85vh] flex flex-col rounded-xl bg-bg-secondary border border-border shadow-xl"
      >
        <div class="flex items-center justify-between px-5 py-4 border-b border-border">
          <h2 class="text-base font-semibold text-text-primary">{{ title }}</h2>
          <button
            class="text-text-muted hover:text-text-primary transition-colors"
            @click="emit('close')"
          >
            <X class="w-5 h-5" />
          </button>
        </div>
        <div class="flex-1 overflow-auto p-5">
          <slot />
        </div>
        <div
          v-if="$slots.footer"
          class="px-5 py-4 border-t border-border flex items-center justify-end gap-2"
        >
          <slot name="footer" />
        </div>
      </div>
    </div>
  </Teleport>
</template>
