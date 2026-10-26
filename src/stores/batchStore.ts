import { defineStore } from "pinia";
import { apiInvoke } from "../api/client";
import { apiListen } from "../api/events";
import { ref } from "vue";
import { useJobQueue } from "../composables/useJobQueue";
import { mapJobErrorMessage } from "./translateStore";
import { useToastStore } from "./toastStore";
import { i18n } from "../i18n";

interface Track {
  id: number;
  track_type: string;
  codec: string;
  properties?: {
    language?: string;
    track_name?: string;
    default_track?: boolean;
    forced_track?: boolean;
    hearing_impaired?: boolean;
  };
}

export interface ExternalSub {
  lang: string;
  status: "ok" | "pending" | "error";
  error_message?: string;
  path: string;
  flavor?: "forced" | "sdh";
}

export interface VideoFileInfo {
  path: string;
  name: string;
  tracks: Track[];
  subtitle_count: number;
  audio_count: number;
  video_count: number;
  external_subs: ExternalSub[];
}

export interface ResolvedMetadata {
  title: string;
  year?: number;
  season?: number;
  episode?: number;
  provider: "tmdb" | "tvmaze";
  seriesId: string;
}

export interface ScanOptions {
  path: string;
  recursive: boolean;
  extensions: string[];
}

export const useBatchStore = defineStore("batch", () => {
  const toast = useToastStore();
  const scannedFiles = ref<VideoFileInfo[]>([]);
  const selectedFiles = ref<Set<string>>(new Set());
  const loading = ref(false);
  const error = ref<string | null>(null);
  const lastScannedPath = ref<string | null>(null);
  const { jobs, addJob, clearAll } = useJobQueue();
  const resolvedMetadata = ref<Record<string, ResolvedMetadata>>({});

  const pipelineJobs = new Map<string, { path: string; lang: string }>();
  let pipelineListenersRegistered = false;

  function withoutExt(path: string): string {
    const slash = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
    const dot = path.lastIndexOf(".");
    return dot > slash ? path.slice(0, dot) : path;
  }

  function upsertExternalSub(
    path: string,
    lang: string,
    status: ExternalSub["status"],
    errorMessage?: string
  ) {
    const file = scannedFiles.value.find((f) => f.path === path);
    if (!file) return;
    let sub = file.external_subs.find((s) => s.lang === lang);
    if (!sub) {
      sub = { lang, status, path: `${withoutExt(path)}.${lang}.srt` };
      file.external_subs.push(sub);
    } else {
      sub.status = status;
    }
    if (errorMessage) sub.error_message = errorMessage;
  }

  async function ensurePipelineListeners() {
    if (pipelineListenersRegistered) return;
    pipelineListenersRegistered = true;

    await apiListen<{ job_id: string; stage?: string }>("job-progress", (payload) => {
      const mapping = pipelineJobs.get(payload.job_id);
      if (!mapping) return;
      upsertExternalSub(mapping.path, mapping.lang, payload.stage === "error" ? "error" : "pending");
    });

    await apiListen<{ job_id: string; status: string; message?: string }>(
      "job-completed",
      (payload) => {
        const mapping = pipelineJobs.get(payload.job_id);
        if (!mapping) return;
        if (payload.status === "completed") {
          upsertExternalSub(mapping.path, mapping.lang, "ok");
        } else {
          upsertExternalSub(mapping.path, mapping.lang, "error", mapJobErrorMessage(payload.message));
        }
        pipelineJobs.delete(payload.job_id);
      }
    );
  }

  async function registerPipelineJob(jobId: string, path: string, lang: string) {
    await ensurePipelineListeners();
    pipelineJobs.set(jobId, { path, lang });
    upsertExternalSub(path, lang, "pending");
  }

  async function scanDirectory(path: string, recursive: boolean = true, keepSelection = false) {
    loading.value = true;
    error.value = null;
    try {
      const result = await apiInvoke<VideoFileInfo[]>("scan_directory", {
        options: {
          path,
          recursive,
          extensions: ["mkv", "mp4", "avi", "mov", "wmv", "flv", "webm"],
        },
      });
      // Directory order is arbitrary (E03, E02, E01...); show episodes in
      // natural order, so "E2" sorts before "E10".
      const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });
      const sorted = [...result].sort((a, b) => collator.compare(a.path, b.path));
      scannedFiles.value = sorted;
      const previous = selectedFiles.value;
      selectedFiles.value = new Set(
        sorted.map((f) => f.path).filter((p) => !keepSelection || previous.has(p))
      );
      lastScannedPath.value = path;
    } catch (e) {
      error.value = String(e);
      scannedFiles.value = [];
    } finally {
      loading.value = false;
    }
  }

  async function rescan() {
    if (lastScannedPath.value) {
      await scanDirectory(lastScannedPath.value);
    }
  }

  async function batchExtract(outputDir: string, languages: string[] = []) {
    const files = Array.from(selectedFiles.value);
    if (files.length === 0) return;

    try {
      const result = await apiInvoke<string[]>("batch_extract_subtitles", {
        options: { files, output_dir: outputDir, languages },
      });

      // The desktop backend answers with one background job id per file; the
      // server backend extracts synchronously and answers with the written
      // subtitle paths. Handle both.
      const returnedPaths = result.some((r) => /[\\/]/.test(r));
      if (returnedPaths) {
        toast.success(
          i18n.global.t("toast.batchExtracted", { n: result.length }),
          outputDir || i18n.global.t("extractLangPicker.beside")
        );
        // New files now sit next to the videos: refresh so the External Subs
        // column shows them.
        if (lastScannedPath.value) await scanDirectory(lastScannedPath.value, true, true);
      } else if (result.length > 0) {
        result.forEach((id, i) => addJob(id, files[i] || ""));
      } else {
        toast.info(i18n.global.t("toast.batchExtractedNone"));
      }
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
      toast.error(i18n.global.t("toast.extractFailed"), error.value);
    }
  }

  function toggleSelection(path: string) {
    if (selectedFiles.value.has(path)) {
      selectedFiles.value.delete(path);
    } else {
      selectedFiles.value.add(path);
    }
  }

  function selectAll() {
    selectedFiles.value = new Set(scannedFiles.value.map((f) => f.path));
  }

  function deselectAll() {
    selectedFiles.value = new Set();
  }

  function clearJobs() {
    clearAll();
  }

  function setResolvedMetadata(path: string, metadata: ResolvedMetadata) {
    resolvedMetadata.value[path] = metadata;
  }

  return {
    scannedFiles,
    selectedFiles,
    loading,
    error,
    jobs,
    lastScannedPath,
    resolvedMetadata,
    scanDirectory,
    rescan,
    batchExtract,
    toggleSelection,
    selectAll,
    deselectAll,
    clearJobs,
    setResolvedMetadata,
    registerPipelineJob,
    upsertExternalSub,
  };
});
