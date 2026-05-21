import { defineStore } from "pinia";
import { apiInvoke } from "../api/client";
import { ref } from "vue";
import { useJobQueue } from "../composables/useJobQueue";

interface Track {
  id: number;
  track_type: string;
  codec: string;
  properties?: {
    language?: string;
    track_name?: string;
  };
}

export interface VideoFileInfo {
  path: string;
  name: string;
  tracks: Track[];
  subtitle_count: number;
  audio_count: number;
  video_count: number;
}

export interface ScanOptions {
  path: string;
  recursive: boolean;
  extensions: string[];
}

export const useBatchStore = defineStore("batch", () => {
  const scannedFiles = ref<VideoFileInfo[]>([]);
  const selectedFiles = ref<Set<string>>(new Set());
  const loading = ref(false);
  const error = ref<string | null>(null);
  const { jobs, addJob, clearAll } = useJobQueue();

  async function scanDirectory(path: string, recursive: boolean = true) {
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
      scannedFiles.value = result;
      selectedFiles.value = new Set(result.map((f) => f.path));
    } catch (e) {
      error.value = String(e);
      scannedFiles.value = [];
    } finally {
      loading.value = false;
    }
  }

  async function batchExtract(outputDir: string) {
    const files = Array.from(selectedFiles.value);
    if (files.length === 0) return;

    try {
      const jobIds = await apiInvoke<string[]>("batch_extract_subtitles", {
        options: { files, output_dir: outputDir },
      });

      jobIds.forEach((id, i) => {
        addJob(id, files[i] || "");
      });
    } catch (e) {
      error.value = String(e);
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

  return {
    scannedFiles,
    selectedFiles,
    loading,
    error,
    jobs,
    scanDirectory,
    batchExtract,
    toggleSelection,
    selectAll,
    deselectAll,
    clearJobs,
  };
});
