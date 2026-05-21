import { defineStore } from "pinia";
import { apiInvoke } from "../api/client";
import { ref } from "vue";
import { JOB_STATUS, type JobStatus } from "../constants/jobStatus";

export interface AudioTrackInfo {
  id: number;
  codec: string;
  codec_long_name?: string;
  language: string;
  title?: string;
}

export interface AudioJob {
  id: string;
  type: "extract" | "merge" | "remove";
  status: JobStatus;
  message?: string;
}

export const useAudioStore = defineStore("audio", () => {
  const selectedFile = ref<string | null>(null);
  const tracks = ref<AudioTrackInfo[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);
  const jobs = ref<AudioJob[]>([]);

  async function loadTracks(filePath: string) {
    loading.value = true;
    error.value = null;
    try {
      const result = await apiInvoke<AudioTrackInfo[]>("list_audio_tracks", {
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

  async function extractTrack(trackIndex: number, outputPath: string, format?: string) {
    const job: AudioJob = {
      id: `extract-${trackIndex}-${Date.now()}`,
      type: "extract",
      status: JOB_STATUS.RUNNING,
    };
    jobs.value.push(job);

    try {
      await apiInvoke("extract_audio_track", {
        filePath: selectedFile.value,
        trackIndex,
        outputPath,
        format,
      });
      job.status = JOB_STATUS.COMPLETED;
      job.message = `Saved to ${outputPath}`;
    } catch (e) {
      job.status = JOB_STATUS.FAILED;
      job.message = String(e);
    }
  }

  async function mergeAudio(videoPath: string, audioPath: string, outputPath: string) {
    const job: AudioJob = {
      id: `merge-${Date.now()}`,
      type: "merge",
      status: JOB_STATUS.RUNNING,
    };
    jobs.value.push(job);

    try {
      await apiInvoke("merge_audio_track", {
        videoPath,
        audioPath,
        outputPath,
      });
      job.status = JOB_STATUS.COMPLETED;
      job.message = `Saved to ${outputPath}`;
    } catch (e) {
      job.status = JOB_STATUS.FAILED;
      job.message = String(e);
    }
  }

  async function removeAllAudio(outputPath: string) {
    const job: AudioJob = {
      id: `remove-${Date.now()}`,
      type: "remove",
      status: JOB_STATUS.RUNNING,
    };
    jobs.value.push(job);

    try {
      await apiInvoke("remove_audio_tracks", {
        filePath: selectedFile.value,
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
    jobs.value = [];
  }

  return {
    selectedFile,
    tracks,
    loading,
    error,
    jobs,
    loadTracks,
    extractTrack,
    mergeAudio,
    removeAllAudio,
    clearJobs,
  };
});
