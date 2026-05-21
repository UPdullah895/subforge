import { defineStore } from "pinia";
import { apiInvoke } from "../api/client";
import { ref } from "vue";
import { JOB_STATUS, type JobStatus } from "../constants/jobStatus";

export interface TrackInfo {
  id: number;
  track_type: string;
  codec: string;
  language: string;
  track_name?: string;
  default_track: boolean;
  forced_track: boolean;
}

export interface ExtractJob {
  id: string;
  trackId: number;
  status: JobStatus;
  message?: string;
}

export const useSubtitleStore = defineStore("subtitle", () => {
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
    const job: ExtractJob = {
      id: `${trackId}-${Date.now()}`,
      trackId,
      status: JOB_STATUS.RUNNING,
    };
    extractJobs.value.push(job);

    try {
      await apiInvoke("extract_subtitle", {
        filePath: selectedFile.value,
        trackId,
        outputPath,
      });
      job.status = JOB_STATUS.COMPLETED;
      job.message = `Saved to ${outputPath}`;
    } catch (e) {
      job.status = JOB_STATUS.FAILED;
      job.message = String(e);
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
