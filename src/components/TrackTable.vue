<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { Download, FileText, Film, Headphones, AlertCircle } from "lucide-vue-next";
import type { TrackInfo } from "../stores/subtitleStore";

const { t } = useI18n();

const props = defineProps<{
  tracks: TrackInfo[];
}>();

const emit = defineEmits<{
  (e: "extract", track: TrackInfo): void;
}>();

function trackTypeIcon(type: string) {
  switch (type) {
    case "video":
      return Film;
    case "audio":
      return Headphones;
    case "subtitles":
      return FileText;
    default:
      return AlertCircle;
  }
}

function trackTypeLabel(type: string) {
  const key = type.toLowerCase() as "video" | "audio" | "subtitles";
  return t(`trackType.${key}`) || type.charAt(0).toUpperCase() + type.slice(1);
}

function trackLabel(track: TrackInfo) {
  const parts: string[] = [];
  if (track.track_name) parts.push(track.track_name);
  parts.push(track.language.toUpperCase());
  if (track.default_track) parts.push(t("trackTable.default"));
  if (track.forced_track) parts.push(t("trackTable.forced"));
  return parts.join(" · ");
}
</script>

<template>
  <div class="overflow-x-auto rounded-lg border border-border">
    <table class="w-full text-sm">
      <thead>
        <tr class="bg-bg-tertiary text-text-secondary text-left">
          <th class="px-4 py-3 font-medium">{{ t("trackTable.id") }}</th>
          <th class="px-4 py-3 font-medium">{{ t("trackTable.type") }}</th>
          <th class="px-4 py-3 font-medium">{{ t("trackTable.codec") }}</th>
          <th class="px-4 py-3 font-medium">{{ t("trackTable.details") }}</th>
          <th class="px-4 py-3 font-medium text-right">{{ t("trackTable.action") }}</th>
        </tr>
      </thead>
      <tbody class="divide-y divide-border">
        <tr
          v-for="track in props.tracks"
          :key="track.id"
          class="hover:bg-bg-hover transition-colors"
        >
          <td class="px-4 py-3 text-text-muted font-mono">{{ track.id }}</td>
          <td class="px-4 py-3">
            <div class="flex items-center gap-2">
              <component :is="trackTypeIcon(track.track_type)" class="w-4 h-4 text-text-muted" />
              <span class="text-text-primary">{{ trackTypeLabel(track.track_type) }}</span>
            </div>
          </td>
          <td class="px-4 py-3 text-text-secondary font-mono text-xs">{{ track.codec }}</td>
          <td class="px-4 py-3">
            <span class="text-text-secondary">{{ trackLabel(track) }}</span>
          </td>
          <td class="px-4 py-3 text-right">
            <button
              v-if="track.track_type === 'subtitles'"
              class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md bg-accent/10 text-accent text-xs font-medium hover:bg-accent hover:text-white transition-colors"
              @click="emit('extract', track)"
            >
              <Download class="w-3.5 h-3.5" />
              {{ t("common.extract") }}
            </button>
            <span v-else class="text-text-muted text-xs">—</span>
          </td>
        </tr>
        <tr v-if="props.tracks.length === 0">
          <td colspan="5" class="px-4 py-8 text-center text-text-muted">
            {{ t("trackTable.empty") }}
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
