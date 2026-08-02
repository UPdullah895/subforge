<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { Download, Headphones } from "lucide-vue-next";
import type { AudioTrackInfo } from "../stores/audioStore";

const { t } = useI18n();

const props = defineProps<{
  tracks: AudioTrackInfo[];
}>();

const emit = defineEmits<{
  (e: "extract", track: AudioTrackInfo): void;
}>();

function trackLabel(track: AudioTrackInfo) {
  const parts: string[] = [];
  if (track.title) parts.push(track.title);
  parts.push(track.language.toUpperCase());
  if (track.codec_long_name) parts.push(track.codec_long_name);
  else parts.push(track.codec);
  return parts.join(" · ");
}
</script>

<template>
  <div class="overflow-x-auto rounded-lg border border-border">
    <table class="w-full text-sm">
      <thead>
        <tr class="bg-bg-tertiary text-text-secondary text-left">
          <th class="px-4 py-3 font-medium">{{ t("audioTrackTable.id") }}</th>
          <th class="px-4 py-3 font-medium">{{ t("audioTrackTable.codec") }}</th>
          <th class="px-4 py-3 font-medium">{{ t("audioTrackTable.details") }}</th>
          <th class="px-4 py-3 font-medium text-right">{{ t("audioTrackTable.action") }}</th>
        </tr>
      </thead>
      <tbody class="divide-y divide-border">
        <tr
          v-for="track in props.tracks"
          :key="track.id"
          class="hover:bg-bg-hover transition-colors"
        >
          <td class="px-4 py-3 text-text-muted font-mono">{{ track.id }}</td>
          <td class="px-4 py-3 text-text-secondary font-mono text-xs">{{ track.codec }}</td>
          <td class="px-4 py-3">
            <div class="flex items-center gap-2">
              <Headphones class="w-4 h-4 text-text-muted" />
              <span class="text-text-secondary">{{ trackLabel(track) }}</span>
            </div>
          </td>
          <td class="px-4 py-3 text-right">
            <button
              class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md bg-accent/10 text-accent text-xs font-medium hover:bg-accent hover:text-white transition-colors"
              @click="emit('extract', track)"
            >
              <Download class="w-3.5 h-3.5" />
              {{ t("common.extract") }}
            </button>
          </td>
        </tr>
        <tr v-if="props.tracks.length === 0">
          <td colspan="4" class="px-4 py-8 text-center text-text-muted">
            {{ t("audioTrackTable.empty") }}
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
