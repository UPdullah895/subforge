<script setup lang="ts">
import { ref, computed } from "vue";
import { FileVideo, ChevronDown, ChevronRight } from "lucide-vue-next";
import type { VideoFileInfo } from "../stores/batchStore";

const props = defineProps<{
  files: VideoFileInfo[];
  selected: Set<string>;
}>();

const emit = defineEmits<{
  (e: "toggle", path: string): void;
  (e: "toggleAll"): void;
}>();

const expanded = ref<Set<string>>(new Set());

function toggleExpand(path: string) {
  if (expanded.value.has(path)) {
    expanded.value.delete(path);
  } else {
    expanded.value.add(path);
  }
}

const allSelected = computed(() => {
  return props.files.length > 0 && props.files.every((f) => props.selected.has(f.path));
});

function toggleAll() {
  emit("toggleAll");
}
</script>

<template>
  <div class="overflow-x-auto rounded-lg border border-border">
    <table class="w-full text-sm">
      <thead>
        <tr class="bg-bg-tertiary text-text-secondary text-left">
          <th class="px-4 py-3 w-10">
            <input
              type="checkbox"
              :checked="allSelected"
              @change="toggleAll"
              class="rounded border-border bg-bg-primary text-accent focus:ring-accent"
            />
          </th>
          <th class="px-4 py-3 font-medium">File</th>
          <th class="px-4 py-3 font-medium text-center">Video</th>
          <th class="px-4 py-3 font-medium text-center">Audio</th>
          <th class="px-4 py-3 font-medium text-center">Subs</th>
          <th class="px-4 py-3 font-medium w-10"></th>
        </tr>
      </thead>
      <tbody class="divide-y divide-border">
        <template v-for="file in props.files" :key="file.path">
          <tr class="hover:bg-bg-hover transition-colors">
            <td class="px-4 py-3">
              <input
                type="checkbox"
                :checked="props.selected.has(file.path)"
                @change="emit('toggle', file.path)"
                class="rounded border-border bg-bg-primary text-accent focus:ring-accent"
              />
            </td>
            <td class="px-4 py-3">
              <div class="flex items-center gap-2">
                <FileVideo class="w-4 h-4 text-text-muted shrink-0" />
                <span class="text-text-primary truncate max-w-xs">{{ file.name }}</span>
              </div>
            </td>
            <td class="px-4 py-3 text-center text-text-muted">{{ file.video_count }}</td>
            <td class="px-4 py-3 text-center text-text-muted">{{ file.audio_count }}</td>
            <td class="px-4 py-3 text-center text-text-muted">{{ file.subtitle_count }}</td>
            <td class="px-4 py-3">
              <button @click="toggleExpand(file.path)" class="text-text-muted hover:text-text-primary">
                <ChevronDown v-if="expanded.has(file.path)" class="w-4 h-4" />
                <ChevronRight v-else class="w-4 h-4" />
              </button>
            </td>
          </tr>
          <tr v-if="expanded.has(file.path)" class="bg-bg-secondary">
            <td colspan="6" class="px-4 py-3">
              <div class="text-xs text-text-secondary space-y-1">
                <p class="font-mono text-text-muted">{{ file.path }}</p>
                <div class="flex flex-wrap gap-2 mt-2">
                  <span
                    v-for="track in file.tracks"
                    :key="track.id"
                    class="px-2 py-0.5 rounded bg-bg-tertiary text-text-secondary"
                  >
                    #{{ track.id }} {{ track.track_type }} · {{ track.codec }}
                    <span v-if="track.properties?.language">· {{ track.properties.language }}</span>
                  </span>
                </div>
              </div>
            </td>
          </tr>
        </template>
        <tr v-if="props.files.length === 0">
          <td colspan="6" class="px-4 py-8 text-center text-text-muted">
            No files found. Scan a directory to get started.
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
