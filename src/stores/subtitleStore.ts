import { defineStore } from "pinia";
import { apiInvoke } from "../api/client";
import { reactive, ref } from "vue";
import { JOB_STATUS, type JobStatus } from "../constants/jobStatus";
import { useToastStore } from "./toastStore";
import { i18n } from "../i18n";
import { baseName } from "../utils/subtitleNaming";

export interface TrackInfo {
  id: number;
  track_type: string;
  codec: string;
  language: string;
  track_name?: string;
  default_track: boolean;
  forced_track: boolean;
  hearing_impaired?: boolean;
}

export interface ExtractJob {
  id: string;
  trackId: number;
  status: JobStatus;
  message?: string;
}

export const useSubtitleStore = defineStore("subtitle", () => {
  const toast = useToastStore();
  const selectedFile = ref<string | null>(null);
  const tracks = ref<TrackInfo[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);
  const extractJobs = ref<ExtractJob[]>([]);

  async function loadTracks(filePath: string) {
    loading.value = true;
    error.value = null;
    try {
      const result = await apiInvoke<TrackInfo[]>("enumerate_tracks", {
        filePath,
      });
      tracks.value = result;
      selectedFile.value = filePath;
    } catch (e) {
      error.value = String(e);
      tracks.value = [];
    } finally {
      loading.value = false;
    }
  }

  async function extractTrack(trackId: number, outputPath: string) {
    if (!selectedFile.value) {
      error.value = "No file selected.";
      return;
    }
    // reactive(): the status/message mutated later must be seen by the UI.
    const job: ExtractJob = reactive<ExtractJob>({
      id: `${trackId}-${Date.now()}`,
      trackId,
      status: JOB_STATUS.RUNNING,
    });
    extractJobs.value.push(job);

    try {
      await apiInvoke("extract_subtitle", {
        filePath: selectedFile.value,
        trackId,
        outputPath,
      });
      job.status = JOB_STATUS.COMPLETED;
      job.message = `Saved to ${outputPath}`;
      toast.success(i18n.global.t("toast.subtitleSaved"), baseName(outputPath));
    } catch (e) {
      job.status = JOB_STATUS.FAILED;
      job.message = e instanceof Error ? e.message : String(e);
      toast.error(i18n.global.t("toast.extractFailed"), job.message);
    }
  }

  function clearJobs() {
    extractJobs.value = [];
  }

  return {
    selectedFile,
    tracks,
    loading,
    error,
    extractJobs,
    loadTracks,
    extractTrack,
    clearJobs,
  };
});
