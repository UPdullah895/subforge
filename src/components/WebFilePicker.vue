<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import {
  ArrowUp,
  CheckSquare,
  File as FileIcon,
  Folder,
  FolderOpen,
  HardDrive,
  Home,
  Square,
} from "lucide-vue-next";
import Modal from "./Modal.vue";
import { pickerRequest } from "../utils/dialog";

interface FsEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
}
interface FsRoot {
  name: string;
  path: string;
}
interface FsListing {
  path: string;
  parent: string | null;
  entries: FsEntry[];
  roots: FsRoot[];
  truncated: boolean;
}

const { t } = useI18n();

const LAST_DIR_KEY = "subforge.lastPickerDir";

const req = computed(() => pickerRequest.value);
const listing = ref<FsListing | null>(null);
const loading = ref(false);
const error = ref<string | null>(null);
const pathInput = ref("");
const showHidden = ref(false);
const showAll = ref(false);
const selected = ref<string[]>([]);
const fileName = ref("");
const filterText = ref("");
const listEl = ref<HTMLElement | null>(null);
const crumbsEl = ref<HTMLElement | null>(null);

const allowedExts = computed(() => {
  const exts = req.value?.filters.flatMap((f) => f.extensions.map((e) => e.toLowerCase())) ?? [];
  return new Set(exts);
});
const hasFilter = computed(() => allowedExts.value.size > 0);

function ext(name: string): string {
  const i = name.lastIndexOf(".");
  return i > 0 ? name.slice(i + 1).toLowerCase() : "";
}

const visibleEntries = computed(() => {
  if (!listing.value) return [];
  const q = filterText.value.trim().toLowerCase();
  return listing.value.entries.filter((e) => {
    if (q && !e.name.toLowerCase().includes(q)) return false;
    if (e.is_dir) return true;
    if (req.value?.directory) return false; // folder mode: files are noise
    if (!hasFilter.value || showAll.value) return true;
    return allowedExts.value.has(ext(e.name));
  });
});

const hiddenByFilter = computed(() => {
  if (!listing.value || !hasFilter.value || showAll.value || req.value?.directory) return 0;
  return listing.value.entries.filter((e) => !e.is_dir && !allowedExts.value.has(ext(e.name)))
    .length;
});

const isSave = computed(() => req.value?.mode === "save");
const isDirMode = computed(() => !!req.value?.directory);

const title = computed(() => {
  if (req.value?.title) return req.value.title;
  if (isSave.value) return t("filePicker.saveTitle");
  if (isDirMode.value) return t("filePicker.folderTitle");
  return req.value?.multiple ? t("filePicker.filesTitle") : t("filePicker.fileTitle");
});

// Clickable path segments: "/" › home › me › Videos (or "C:\" › Users › …).
const breadcrumbs = computed(() => {
  if (!listing.value) return [];
  const full = listing.value.path;
  const win = full.includes("\\") && !full.includes("/");
  const sep = win ? "\\" : "/";
  const parts = full.split(sep).filter(Boolean);
  const crumbs: { label: string; path: string }[] = [];
  let acc = "";
  parts.forEach((part, i) => {
    acc = win ? (i === 0 ? `${part}\\` : `${acc.replace(/\\$/, "")}\\${part}`) : `${acc}/${part}`;
    crumbs.push({ label: part, path: acc });
  });
  return win ? crumbs : [{ label: "/", path: "/" }, ...crumbs];
});

const existingNames = computed(() => new Set(listing.value?.entries.map((e) => e.name) ?? []));
const willOverwrite = computed(() => isSave.value && existingNames.value.has(fileName.value.trim()));

const canConfirm = computed(() => {
  if (!listing.value) return false;
  if (isSave.value) return fileName.value.trim().length > 0;
  if (isDirMode.value) return true;
  return selected.value.length > 0;
});

const confirmLabel = computed(() => {
  if (isSave.value) return willOverwrite.value ? t("filePicker.overwrite") : t("filePicker.save");
  if (isDirMode.value) return t("filePicker.selectFolder");
  if (req.value?.multiple && selected.value.length > 0)
    return t("filePicker.openN", { n: selected.value.length });
  return t("filePicker.open");
});

function joinPath(dir: string, name: string): string {
  const sep = dir.includes("\\") && !dir.includes("/") ? "\\" : "/";
  return dir.endsWith(sep) ? `${dir}${name}` : `${dir}${sep}${name}`;
}

function dirname(p: string): string | undefined {
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  return i > 0 ? p.slice(0, i) : i === 0 ? p.slice(0, 1) : undefined;
}
function basename(p: string): string {
  return p.slice(Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\")) + 1);
}

function readLastDir(): string | undefined {
  try {
    return localStorage.getItem(LAST_DIR_KEY) ?? undefined;
  } catch {
    return undefined;
  }
}
function rememberDir(dir: string) {
  try {
    localStorage.setItem(LAST_DIR_KEY, dir);
  } catch {
    /* private mode etc. — remembering the folder is only a convenience */
  }
}

async function load(path?: string) {
  loading.value = true;
  error.value = null;
  try {
    const qs = new URLSearchParams();
    if (path) qs.set("path", path);
    if (showHidden.value) qs.set("show_hidden", "true");
    const resp = await fetch(`/api/fs/list?${qs.toString()}`);
    if (!resp.ok) throw new Error((await resp.text()) || `HTTP ${resp.status}`);
    listing.value = (await resp.json()) as FsListing;
    pathInput.value = listing.value.path;
    filterText.value = "";
    await nextTick();
    listEl.value?.scrollTo({ top: 0 });
    // Keep the current folder (the last crumb) in view on deep paths.
    if (crumbsEl.value) crumbsEl.value.scrollLeft = crumbsEl.value.scrollWidth;
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    loading.value = false;
  }
}

// Open (or re-open) whenever a new request arrives.
watch(
  req,
  (r) => {
    if (!r) return;
    selected.value = [];
    showAll.value = false;
    filterText.value = "";
    let start = r.defaultPath ? dirname(r.defaultPath) : undefined;
    if (r.defaultPath && r.directory) start = r.defaultPath;
    fileName.value = r.mode === "save" && r.defaultPath ? basename(r.defaultPath) : "";
    void load(start ?? readLastDir());
  },
  { immediate: true }
);

watch(showHidden, () => listing.value && load(listing.value.path));

function finish(value: string | string[] | null) {
  const r = pickerRequest.value;
  if (!r) return;
  if (listing.value && value) rememberDir(isDirMode.value ? listing.value.path : listing.value.path);
  pickerRequest.value = null;
  r.resolve(value);
}

function cancel() {
  finish(null);
}

function confirm() {
  if (!canConfirm.value || !listing.value) return;
  if (isSave.value) {
    finish(joinPath(listing.value.path, fileName.value.trim()));
  } else if (isDirMode.value) {
    finish(listing.value.path);
  } else {
    finish(req.value?.multiple ? [...selected.value] : (selected.value[0] ?? null));
  }
}

function onRow(entry: FsEntry) {
  if (entry.is_dir) {
    void load(entry.path);
    return;
  }
  if (isSave.value) {
    fileName.value = entry.name;
    return;
  }
  if (req.value?.multiple) {
    const i = selected.value.indexOf(entry.path);
    if (i === -1) selected.value.push(entry.path);
    else selected.value.splice(i, 1);
  } else {
    selected.value = [entry.path];
  }
}

function onRowDouble(entry: FsEntry) {
  if (entry.is_dir || isSave.value) return;
  if (!req.value?.multiple) {
    selected.value = [entry.path];
    confirm();
  }
}

const selectableVisible = computed(() => visibleEntries.value.filter((e) => !e.is_dir));
const allSelected = computed(
  () =>
    selectableVisible.value.length > 0 &&
    selectableVisible.value.every((e) => selected.value.includes(e.path))
);
function toggleAll() {
  if (allSelected.value) {
    const vis = new Set(selectableVisible.value.map((e) => e.path));
    selected.value = selected.value.filter((p) => !vis.has(p));
  } else {
    for (const e of selectableVisible.value) {
      if (!selected.value.includes(e.path)) selected.value.push(e.path);
    }
  }
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = bytes / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
}

function onKey(e: KeyboardEvent) {
  if (!pickerRequest.value) return;
  if (e.key === "Escape") {
    e.preventDefault();
    cancel();
  }
}
window.addEventListener("keydown", onKey);
onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <Modal :open="!!req" :title="title" @close="cancel">
    <div class="flex flex-col gap-3 h-[min(62vh,34rem)]" role="group" :aria-label="title">
      <!-- Location bar -->
      <div class="flex items-center gap-2">
        <button
          class="p-2 rounded-lg border border-border bg-bg-tertiary text-text-secondary hover:text-text-primary hover:bg-bg-hover disabled:opacity-40 disabled:cursor-not-allowed"
          :disabled="!listing?.parent"
          :title="t('filePicker.up')"
          :aria-label="t('filePicker.up')"
          @click="listing?.parent && load(listing.parent)"
        >
          <ArrowUp class="w-4 h-4 rtl:rotate-0" />
        </button>
        <input
          v-model="pathInput"
          dir="ltr"
          class="flex-1 min-w-0 px-3 py-2 text-sm rounded-lg bg-bg-tertiary border border-border text-text-primary font-mono focus:outline-none focus:border-accent"
          :aria-label="t('filePicker.path')"
          spellcheck="false"
          @keydown.enter.prevent="load(pathInput)"
        />
      </div>

      <!-- Quick locations -->
      <div class="flex flex-wrap items-center gap-1.5">
        <button
          v-for="root in listing?.roots ?? []"
          :key="root.path"
          class="inline-flex items-center gap-1.5 px-2.5 py-1 text-xs rounded-md border border-border bg-bg-tertiary text-text-secondary hover:text-text-primary hover:bg-bg-hover"
          @click="load(root.path)"
        >
          <Home v-if="root.name === 'Home'" class="w-3.5 h-3.5" />
          <HardDrive v-else class="w-3.5 h-3.5" />
          {{ root.name === "Home" ? t("filePicker.home") : root.name }}
        </button>
        <nav
          ref="crumbsEl"
          class="flex flex-1 items-center gap-0.5 text-xs text-text-muted min-w-0 overflow-x-auto whitespace-nowrap"
          dir="ltr"
          :aria-label="t('filePicker.path')"
        >
          <template v-for="(crumb, i) in breadcrumbs" :key="crumb.path">
            <span v-if="i > 0" aria-hidden="true">›</span>
            <button
              class="px-1 rounded hover:text-text-primary hover:bg-bg-hover shrink-0"
              :class="i === breadcrumbs.length - 1 ? 'text-text-primary font-medium' : ''"
              @click="load(crumb.path)"
            >
              {{ crumb.label }}
            </button>
          </template>
        </nav>
      </div>

      <!-- Filter / toggles -->
      <div class="flex flex-wrap items-center gap-3 text-xs text-text-secondary">
        <input
          v-model="filterText"
          class="flex-1 min-w-[8rem] px-3 py-1.5 text-sm rounded-lg bg-bg-tertiary border border-border text-text-primary focus:outline-none focus:border-accent"
          :placeholder="t('filePicker.filter')"
          :aria-label="t('filePicker.filter')"
        />
        <label class="inline-flex items-center gap-1.5 cursor-pointer select-none">
          <input v-model="showHidden" type="checkbox" class="accent-accent" />
          {{ t("filePicker.showHidden") }}
        </label>
        <label v-if="hasFilter && !isDirMode" class="inline-flex items-center gap-1.5 cursor-pointer select-none">
          <input v-model="showAll" type="checkbox" class="accent-accent" />
          {{ t("filePicker.allFiles") }}
        </label>
        <button
          v-if="req?.multiple && selectableVisible.length > 1"
          class="inline-flex items-center gap-1.5 text-accent hover:text-accent-hover"
          @click="toggleAll"
        >
          <CheckSquare v-if="allSelected" class="w-3.5 h-3.5" />
          <Square v-else class="w-3.5 h-3.5" />
          {{ allSelected ? t("filePicker.selectNone") : t("filePicker.selectAll") }}
        </button>
      </div>

      <p v-if="error" class="text-sm text-error" role="alert">{{ error }}</p>

      <!-- Listing -->
      <div
        ref="listEl"
        class="flex-1 min-h-[6rem] overflow-auto rounded-lg border border-border bg-bg-primary"
        :aria-busy="loading"
      >
        <p v-if="loading && !listing" class="p-4 text-sm text-text-muted">
          {{ t("filePicker.loading") }}
        </p>
        <p
          v-else-if="listing && visibleEntries.length === 0"
          class="p-4 text-sm text-text-muted"
        >
          {{ isDirMode ? t("filePicker.emptyFolderMode") : t("filePicker.empty") }}
        </p>
        <ul v-else role="listbox" :aria-multiselectable="!!req?.multiple" class="divide-y divide-border/50">
          <li v-for="entry in visibleEntries" :key="entry.path" role="option" :aria-selected="selected.includes(entry.path)">
            <button
              class="w-full flex items-center gap-3 px-3 py-2 text-start text-sm hover:bg-bg-hover focus:outline-none focus-visible:bg-bg-hover"
              :class="selected.includes(entry.path) ? 'bg-accent/15' : ''"
              @click="onRow(entry)"
              @dblclick="onRowDouble(entry)"
            >
              <component
                :is="entry.is_dir ? Folder : FileIcon"
                class="w-4 h-4 shrink-0"
                :class="entry.is_dir ? 'text-accent' : 'text-text-muted'"
              />
              <span class="flex-1 min-w-0 truncate" dir="auto" :title="entry.name">{{ entry.name }}</span>
              <span v-if="!entry.is_dir" class="text-xs text-text-muted tabular-nums shrink-0">
                {{ formatSize(entry.size) }}
              </span>
            </button>
          </li>
        </ul>
      </div>
      <p v-if="listing?.truncated" class="text-xs text-warning">{{ t("filePicker.truncated") }}</p>
      <p v-if="hiddenByFilter > 0" class="text-xs text-text-muted">
        {{ t("filePicker.hiddenByFilter", { n: hiddenByFilter }) }}
      </p>

      <!-- Save: file name -->
      <div v-if="isSave" class="flex flex-col gap-1">
        <label class="text-xs text-text-secondary" for="web-picker-filename">
          {{ t("filePicker.fileName") }}
        </label>
        <input
          id="web-picker-filename"
          v-model="fileName"
          dir="auto"
          class="px-3 py-2 text-sm rounded-lg bg-bg-tertiary border border-border text-text-primary focus:outline-none focus:border-accent"
          @keydown.enter.prevent="confirm"
        />
        <p v-if="willOverwrite" class="text-xs text-warning">{{ t("filePicker.willOverwrite") }}</p>
      </div>

      <p v-if="isDirMode && listing" class="flex items-center gap-2 text-xs text-text-secondary">
        <FolderOpen class="w-4 h-4 text-accent" />
        <span dir="ltr" class="font-mono truncate">{{ listing.path }}</span>
      </p>
    </div>

    <template #footer>
      <span
        v-if="req?.multiple && selected.length > 0"
        class="me-auto text-xs text-text-secondary"
      >
        {{ t("filePicker.selectedCount", { n: selected.length }) }}
      </span>
      <button
        class="px-4 py-2 text-sm rounded-lg border border-border bg-bg-tertiary text-text-secondary hover:bg-bg-hover"
        @click="cancel"
      >
        {{ t("common.cancel") }}
      </button>
      <button
        class="px-4 py-2 text-sm rounded-lg bg-accent text-white hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed"
        :disabled="!canConfirm"
        @click="confirm"
      >
        {{ confirmLabel }}
      </button>
    </template>
  </Modal>
</template>
