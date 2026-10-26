<script setup lang="ts">
import { ref } from "vue";
import { Eye, EyeOff } from "lucide-vue-next";

withDefaults(
  defineProps<{
    modelValue?: string;
    placeholder?: string;
    testid?: string;
    /** Shows a "saved" tick under the field. */
    saved?: boolean;
    savedLabel?: string;
  }>(),
  { modelValue: "", placeholder: "", testid: "", saved: false, savedLabel: "" }
);

const emit = defineEmits<{ "update:modelValue": [string] }>();

// Keys are masked by default; the eye reveals the value only until the field
// loses focus or the component unmounts.
const revealed = ref(false);

function onInput(event: Event) {
  emit("update:modelValue", (event.target as HTMLInputElement).value);
}
</script>

<template>
  <div>
    <div class="relative">
      <input
        :value="modelValue"
        :type="revealed ? 'text' : 'password'"
        :placeholder="placeholder"
        :data-testid="testid"
        autocomplete="off"
        autocapitalize="off"
        spellcheck="false"
        class="w-full px-4 py-2.5 pr-10 rounded-lg bg-bg-primary border border-border text-sm text-text-primary placeholder-text-muted focus:outline-none focus:border-accent"
        @input="onInput"
      />
      <button
        type="button"
        class="absolute right-3 top-1/2 -translate-y-1/2 text-text-muted hover:text-text-secondary"
        :aria-label="revealed ? 'Hide' : 'Show'"
        :data-testid="testid ? `${testid}-reveal` : undefined"
        @click="revealed = !revealed"
      >
        <EyeOff v-if="revealed" class="w-4 h-4" />
        <Eye v-else class="w-4 h-4" />
      </button>
    </div>
    <p v-if="saved" class="mt-1 text-xs text-success">{{ savedLabel }}</p>
  </div>
</template>
