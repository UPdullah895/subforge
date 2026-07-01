<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import Modal from "./Modal.vue";
import { apiInvoke } from "../api/client";
import { useBatchStore } from "../stores/batchStore";
import { trackDisplayName } from "../utils/languageNames";
import { AlertTriangle, Loader2 } from "lucide-vue-next";

const { t } = useI18n();

const props = defineProps<{
  open: boolean;
  files: string[];
}>();

const emit = defineEmits<{
  (e: "close"): void;
}>();

const batchStore = useBatchStore();

const languageOptions = [
  { code: "en", label: "English" },
  { code: "ar", label: "Arabic" },
  { code: "es", label: "Spanish" },
  { code: "fr", label: "French" },
  { code: "de", label: "German" },
  { code: "ja", label: "Japanese" },
  { code: "ko", label: "Korean" },
  { code: "zh", label: "Chinese" },
  { code: "ru", label: "Russian" },
  { code: "pt", label: "Portuguese" },
  { code: "it", label: "Italian" },
  { code: "tr", label: "Turkish" },
  { code: "hi", label: "Hindi" },
  { code: "fa", label: "Persian" },
  { code: "ur", label: "Urdu" },
];

const deleteOriginals = ref(false);
const merging = ref(false);

// file path -> Set of sub paths checked for that file
const selections = reactive<Record<string, Set<string>>>({});
// sub path -> current language override (defaults to the detected lang)
const subLang = reactive<Record<string, string>>({});
// sub path -> whether it should be tagged/named as hearing-impaired
const subHearingImpaired = reactive<Record<string, boolean>>({});
// file path -> chosen default audio track id
const defaultAudioTrackId = reactive<Record<string, number | null>>({});
// file path -> "existing:<id>" | "new:<subPath>" | ""
const defaultSubtitleChoice = reactive<Record<string, string>>({});
// file path -> user explicitly opened the per-file override controls
const manualOverride = reactive<Record<string, boolean>>({});

// One collective choice applied to every file at once, instead of picking a
// default audio/subtitle language per clip — files that don't have a track
// in the chosen language get flagged instead of silently guessing.
const globalDefaultAudioLang = ref<string | null>(null);
const globalDefaultSubtitleLang = ref<string | null>(null);

interface FileRow {
  path: string;
  name: string;
  subs: { lang: string; path: string; flavor?: string }[];
  audioTracks: { id: number; language: string; trackName?: string; isDefault: boolean }[];
  subtitleTracks: { id: number; language: string; trackName?: string; isDefault: boolean }[];
}

const rows = computed<FileRow[]>(() =>
  props.files
    .map((path): FileRow | null => {
      const file = batchStore.scannedFiles.find((f) => f.path === path);
      if (!file) return null;
      const subs = file.external_subs
        .filter((s) => s.status === "ok")
        .map((s) => ({ lang: s.lang, path: s.path, flavor: s.flavor }));
      if (subs.length === 0) return null;
      const audioTracks = file.tracks
        .filter((tr) => tr.track_type === "audio")
        .map((tr) => ({
          id: tr.id,
          language: tr.properties?.language || "und",
          trackName: tr.properties?.track_name,
          isDefault: tr.properties?.default_track ?? false,
        }));
      const subtitleTracks = file.tracks
        .filter((tr) => tr.track_type === "subtitles")
        .map((tr) => ({
          id: tr.id,
          language: tr.properties?.language || "und",
          trackName: tr.properties?.track_name,
          isDefault: tr.properties?.default_track ?? false,
        }));
      return { path, name: file.name, subs, audioTracks, subtitleTracks };
    })
    .filter((r): r is FileRow => r !== null)
);

function selectedCount(path: string): number {
  return selections[path]?.size ?? 0;
}

function showAudioDefaultPicker(row: FileRow): boolean {
  return row.audioTracks.length > 1;
}

function showSubtitleDefaultPicker(row: FileRow): boolean {
  return row.subtitleTracks.length + selectedCount(row.path) > 1;
}

const showGlobalAudioPicker = computed(() => rows.value.some(showAudioDefaultPicker));
const showGlobalSubtitlePicker = computed(() => rows.value.some(showSubtitleDefaultPicker));

function mostCommon(values: string[]): string | null {
  if (values.length === 0) return null;
  const counts = new Map<string, number>();
  for (const v of values) counts.set(v, (counts.get(v) ?? 0) + 1);
  return [...counts.entries()].sort((a, b) => b[1] - a[1])[0][0];
}

const audioLangOptions = computed(() => {
  const langs = new Set<string>();
  for (const row of rows.value) {
    if (!showAudioDefaultPicker(row)) continue;
    for (const tr of row.audioTracks) langs.add(tr.language);
  }
  return [...langs];
});

const subtitleLangOptions = computed(() => {
  const langs = new Set<string>();
  for (const row of rows.value) {
    if (!showSubtitleDefaultPicker(row)) continue;
    for (const tr of row.subtitleTracks) langs.add(tr.language);
    for (const sub of row.subs) if (isChecked(row.path, sub.path)) langs.add(subLang[sub.path] || sub.lang);
  }
  return [...langs];
});

// A row is flagged when the collective choice doesn't apply to it — e.g. it
// simply doesn't have a track in the language every other file has, so it
// needs a manual pick instead of silently falling back to something else.
function audioFlagged(row: FileRow): boolean {
  if (!showAudioDefaultPicker(row) || !globalDefaultAudioLang.value) return false;
  return !row.audioTracks.some((tr) => tr.language === globalDefaultAudioLang.value);
}

function subtitleFlagged(row: FileRow): boolean {
  if (!showSubtitleDefaultPicker(row) || !globalDefaultSubtitleLang.value) return false;
  const inExisting = row.subtitleTracks.some((tr) => tr.language === globalDefaultSubtitleLang.value);
  const inNew = row.subs.some(
    (s) => isChecked(row.path, s.path) && subLang[s.path] === globalDefaultSubtitleLang.value
  );
  return !inExisting && !inNew;
}

function isFlagged(row: FileRow): boolean {
  return audioFlagged(row) || subtitleFlagged(row);
}

const flaggedRows = computed(() => rows.value.filter(isFlagged));

function showOverrideControls(row: FileRow): boolean {
  return manualOverride[row.path] === true || isFlagged(row);
}

function toggleManualOverride(path: string) {
  manualOverride[path] = !manualOverride[path];
}

watch(globalDefaultAudioLang, (lang) => {
  if (!lang) return;
  for (const row of rows.value) {
    if (!showAudioDefaultPicker(row)) continue;
    const match = row.audioTracks.find((tr) => tr.language === lang);
    if (match) defaultAudioTrackId[row.path] = match.id;
  }
});

watch(globalDefaultSubtitleLang, (lang) => {
  if (!lang) return;
  for (const row of rows.value) {
    if (!showSubtitleDefaultPicker(row)) continue;
    const existingMatch = row.subtitleTracks.find((tr) => tr.language === lang);
    if (existingMatch) {
      defaultSubtitleChoice[row.path] = `existing:${existingMatch.id}`;
      continue;
    }
    const newMatch = row.subs.find((s) => isChecked(row.path, s.path) && subLang[s.path] === lang);
    if (newMatch) defaultSubtitleChoice[row.path] = `new:${newMatch.path}`;
  }
});

watch(
  () => props.open,
  (isOpen) => {
    if (!isOpen) return;
    deleteOriginals.value = false;
    for (const key of Object.keys(selections)) delete selections[key];
    for (const key of Object.keys(subLang)) delete subLang[key];
    for (const key of Object.keys(subHearingImpaired)) delete subHearingImpaired[key];
    for (const key of Object.keys(defaultAudioTrackId)) delete defaultAudioTrackId[key];
    for (const key of Object.keys(defaultSubtitleChoice)) delete defaultSubtitleChoice[key];
    for (const key of Object.keys(manualOverride)) delete manualOverride[key];

    for (const row of rows.value) {
      for (const sub of row.subs) {
        subLang[sub.path] = sub.lang;
        subHearingImpaired[sub.path] = sub.flavor === "sdh";
      }
      const defaultAudio = row.audioTracks.find((tr) => tr.isDefault);
      defaultAudioTrackId[row.path] = defaultAudio?.id ?? row.audioTracks[0]?.id ?? null;
      const defaultSub = row.subtitleTracks.find((tr) => tr.isDefault);
      defaultSubtitleChoice[row.path] = defaultSub ? `existing:${defaultSub.id}` : "";
    }

    // Seed the collective picker with whatever language is already the
    // default across the most files, so opening the modal doesn't require
    // re-picking something that's already consistent everywhere.
    globalDefaultAudioLang.value = mostCommon(
      rows.value.filter(showAudioDefaultPicker).map((r) => r.audioTracks.find((tr) => tr.isDefault)?.language ?? "")
    );
    globalDefaultSubtitleLang.value = mostCommon(
      rows.value.filter(showSubtitleDefaultPicker).map((r) => r.subtitleTracks.find((tr) => tr.isDefault)?.language ?? "")
    );
  }
);

function isChecked(path: string, subPath: string): boolean {
  return selections[path]?.has(subPath) ?? false;
}

function toggle(path: string, subPath: string) {
  if (!selections[path]) selections[path] = new Set();
  if (selections[path].has(subPath)) selections[path].delete(subPath);
  else selections[path].add(subPath);
}

function mergeAllDetected() {
  for (const row of rows.value) {
    selections[row.path] = new Set(row.subs.map((s) => s.path));
  }
}

const hasAnySelection = computed(() =>
  Object.values(selections).some((set) => set.size > 0)
);

function trackNamePreview(subPath: string): string {
  return trackDisplayName(subLang[subPath] || "und", subHearingImpaired[subPath] ?? false);
}

async function applyDefaultOverrides(row: FileRow, mergedPath: string) {
  if (showAudioDefaultPicker(row)) {
    const wanted = defaultAudioTrackId[row.path];
    const current = row.audioTracks.find((tr) => tr.isDefault);
    if (wanted != null && wanted !== current?.id) {
      if (current) {
        await apiInvoke("set_track_metadata", {
          request: { file_path: mergedPath, track_id: current.id, default_flag: false },
        });
      }
      await apiInvoke("set_track_metadata", {
        request: { file_path: mergedPath, track_id: wanted, default_flag: true },
      });
    }
  }

  if (showSubtitleDefaultPicker(row)) {
    const choice = defaultSubtitleChoice[row.path];
    const currentDefault = row.subtitleTracks.find((tr) => tr.isDefault);
    if (choice.startsWith("existing:")) {
      const id = Number(choice.split(":")[1]);
      if (currentDefault && currentDefault.id !== id) {
        await apiInvoke("set_track_metadata", {
          request: { file_path: mergedPath, track_id: currentDefault.id, default_flag: false },
        });
      }
      await apiInvoke("set_track_metadata", {
        request: { file_path: mergedPath, track_id: id, default_flag: true },
      });
    } else if (choice.startsWith("new:") && currentDefault) {
      // The newly merged track already got `--default-track-flag yes` from
      // the merge call itself — just make sure the previous default among
      // the file's existing subtitle tracks isn't left also marked default.
      await apiInvoke("set_track_metadata", {
        request: { file_path: mergedPath, track_id: currentDefault.id, default_flag: false },
      });
    }
  }
}

async function confirmMerge() {
  merging.value = true;
  try {
    for (const row of rows.value) {
      const selectedPaths = selections[row.path];
      if (!selectedPaths || selectedPaths.size === 0) continue;
      const subs = row.subs
        .filter((s) => selectedPaths.has(s.path))
        .map((s) => ({
          path: s.path,
          lang: subLang[s.path] || s.lang,
          hearing_impaired: subHearingImpaired[s.path] ?? false,
          make_default: defaultSubtitleChoice[row.path] === `new:${s.path}`,
        }));
      try {
        const result = await apiInvoke<{ output_path: string }>("merge_external_subs", {
          request: { video_path: row.path, subs, delete_originals: deleteOriginals.value },
        });
        await applyDefaultOverrides(row, result.output_path);
      } catch (e) {
        const message = e instanceof Error ? e.message : String(e);
        for (const sub of subs) {
          batchStore.upsertExternalSub(row.path, sub.lang, "error", message);
        }
      }
    }
    await batchStore.rescan();
  } finally {
    merging.value = false;
    emit("close");
  }
}
</script>

<template>
  <Modal :open="open" :title="t('mergeSubs.title')" @close="emit('close')">
    <div class="space-y-3">
      <div v-if="rows.length === 0" class="text-sm text-text-muted py-4 text-center">
        {{ t("mergeSubs.noneAvailable") }}
      </div>
      <template v-else>
        <div v-if="showGlobalAudioPicker || showGlobalSubtitlePicker" class="grid grid-cols-2 gap-3 p-3 rounded-lg bg-bg-secondary border border-border">
          <div v-if="showGlobalAudioPicker">
            <label class="block text-[11px] font-medium text-text-secondary mb-1">{{ t("mergeSubs.defaultAudioForAll") }}</label>
            <select
              v-model="globalDefaultAudioLang"
              class="w-full px-3 py-1.5 rounded-md bg-bg-primary border border-border text-xs text-text-primary focus:outline-none focus:border-accent"
            >
              <option :value="null">{{ t("mergeSubs.pickLanguage") }}</option>
              <option v-for="lang in audioLangOptions" :key="lang" :value="lang">{{ lang.toUpperCase() }}</option>
            </select>
          </div>
          <div v-if="showGlobalSubtitlePicker">
            <label class="block text-[11px] font-medium text-text-secondary mb-1">{{ t("mergeSubs.defaultSubtitleForAll") }}</label>
            <select
              v-model="globalDefaultSubtitleLang"
              class="w-full px-3 py-1.5 rounded-md bg-bg-primary border border-border text-xs text-text-primary focus:outline-none focus:border-accent"
            >
              <option :value="null">{{ t("mergeSubs.pickLanguage") }}</option>
              <option v-for="lang in subtitleLangOptions" :key="lang" :value="lang">{{ lang.toUpperCase() }}</option>
            </select>
          </div>
        </div>

        <div
          v-if="flaggedRows.length > 0"
          class="flex items-center gap-2 p-2.5 rounded-lg bg-warning/10 border border-warning/20 text-xs text-warning"
        >
          <AlertTriangle class="w-3.5 h-3.5 shrink-0" />
          {{ t("mergeSubs.needsAttention", { count: flaggedRows.length }) }}
        </div>

        <button
          class="text-xs text-accent hover:underline"
          @click="mergeAllDetected"
        >
          {{ t("mergeSubs.mergeAllDetected") }}
        </button>
        <div
          v-for="row in rows"
          :key="row.path"
          class="p-3 rounded-lg bg-bg-secondary border border-border space-y-3"
        >
          <div class="flex items-center justify-between gap-2">
            <span class="flex items-center gap-1.5 min-w-0">
              <AlertTriangle v-if="isFlagged(row)" class="w-3.5 h-3.5 text-warning shrink-0" :title="t('mergeSubs.rowNeedsAttention')" />
              <p class="text-sm text-text-primary truncate" :title="row.path">{{ row.name }}</p>
            </span>
            <button
              v-if="showAudioDefaultPicker(row) || showSubtitleDefaultPicker(row)"
              class="text-[11px] text-accent hover:underline shrink-0"
              @click="toggleManualOverride(row.path)"
            >
              {{ manualOverride[row.path] ? t("mergeSubs.useCollective") : t("mergeSubs.adjustManually") }}
            </button>
          </div>

          <div class="space-y-2">
            <div
              v-for="sub in row.subs"
              :key="sub.path"
              class="flex flex-wrap items-center gap-2 p-2 rounded-md bg-bg-primary/50"
            >
              <input
                type="checkbox"
                class="rounded border-border bg-bg-primary text-accent focus:ring-accent"
                :checked="isChecked(row.path, sub.path)"
                @change="toggle(row.path, sub.path)"
              />
              <span v-if="sub.flavor" class="text-[10px] uppercase px-1.5 py-0.5 rounded bg-bg-tertiary text-text-muted">
                {{ sub.flavor }}
              </span>
              <select
                v-model="subLang[sub.path]"
                class="px-2 py-1 rounded-md bg-bg-primary border border-border text-xs text-text-primary focus:outline-none focus:border-accent"
              >
                <option v-for="lang in languageOptions" :key="lang.code" :value="lang.code">{{ lang.label }}</option>
              </select>
              <label class="flex items-center gap-1 text-[11px] text-text-secondary">
                <input v-model="subHearingImpaired[sub.path]" type="checkbox" class="accent-accent" />
                {{ t("mergeSubs.hearingImpaired") }}
              </label>
              <span class="text-[11px] text-text-muted truncate">{{ trackNamePreview(sub.path) }}</span>
            </div>
          </div>

          <div v-if="showOverrideControls(row) && showAudioDefaultPicker(row)" class="pt-1 border-t border-border">
            <p class="text-[11px] font-medium text-text-secondary mb-1">{{ t("mergeSubs.defaultAudio") }}</p>
            <div class="flex flex-wrap gap-3">
              <label
                v-for="tr in row.audioTracks"
                :key="tr.id"
                class="flex items-center gap-1.5 text-xs text-text-secondary"
              >
                <input
                  v-model.number="defaultAudioTrackId[row.path]"
                  type="radio"
                  :value="tr.id"
                  class="accent-accent"
                />
                {{ tr.trackName || tr.language.toUpperCase() }}
              </label>
            </div>
          </div>

          <div v-if="showOverrideControls(row) && showSubtitleDefaultPicker(row)" class="pt-1 border-t border-border">
            <p class="text-[11px] font-medium text-text-secondary mb-1">{{ t("mergeSubs.defaultSubtitle") }}</p>
            <div class="flex flex-wrap gap-3">
              <label
                v-for="tr in row.subtitleTracks"
                :key="tr.id"
                class="flex items-center gap-1.5 text-xs text-text-secondary"
              >
                <input
                  v-model="defaultSubtitleChoice[row.path]"
                  type="radio"
                  :value="`existing:${tr.id}`"
                  class="accent-accent"
                />
                {{ tr.trackName || tr.language.toUpperCase() }}
              </label>
              <label
                v-for="sub in row.subs.filter((s) => isChecked(row.path, s.path))"
                :key="sub.path"
                class="flex items-center gap-1.5 text-xs text-text-secondary"
              >
                <input
                  v-model="defaultSubtitleChoice[row.path]"
                  type="radio"
                  :value="`new:${sub.path}`"
                  class="accent-accent"
                />
                {{ trackNamePreview(sub.path) }} ({{ t("mergeSubs.newTrack") }})
              </label>
            </div>
          </div>
        </div>

        <label class="flex items-center gap-2 text-xs text-text-secondary pt-1">
          <input v-model="deleteOriginals" type="checkbox" class="accent-accent" />
          {{ t("mergeSubs.deleteOriginals") }}
        </label>
      </template>
    </div>

    <template #footer>
      <button
        class="inline-flex items-center gap-1.5 px-3 py-2 rounded-lg bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors"
        @click="emit('close')"
      >
        {{ t("common.cancel") }}
      </button>
      <button
        :disabled="!hasAnySelection || merging"
        class="inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-accent text-white text-xs font-medium hover:bg-accent-hover active:bg-accent-active transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        @click="confirmMerge"
      >
        <Loader2 v-if="merging" class="w-3.5 h-3.5 animate-spin" />
        {{ t("mergeSubs.confirm") }}
      </button>
    </template>
  </Modal>
</template>
