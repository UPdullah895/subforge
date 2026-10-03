import { defineStore } from "pinia";
import { apiInvoke } from "../api/client";
import { reactive, ref } from "vue";
import { JOB_STATUS, type JobStatus } from "../constants/jobStatus";
import { useToastStore } from "./toastStore";
import { i18n } from "../i18n";
import { baseName } from "../utils/subtitleNaming";

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
  const toast = useToastStore();
  const selectedFile = ref<string | null>(null);
  const tracks = ref<AudioTrackInfo[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);
  const jobs = ref<AudioJob[]>([]);

  function finish(job: AudioJob, outputPath: string) {
    job.status = JOB_STATUS.COMPLETED;
    job.message = `Saved to ${outputPath}`;
    toast.success(i18n.global.t("toast.audioDone"), baseName(outputPath));
  }

  function fail(job: AudioJob, e: unknown) {
    job.status = JOB_STATUS.FAILED;
    job.message = e instanceof Error ? e.message : String(e);
    toast.error(i18n.global.t("toast.audioFailed"), job.message);
  }

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
      error.value = e instanceof Error ? e.message : String(e);
      tracks.value = [];
    } finally {
      loading.value = false;
    }
  }

  async function extractTrack(trackIndex: number, outputPath: string, format?: string) {
    if (!selectedFile.value) {
      error.value = "No file selected.";
      return;
    }
    // reactive(): the status/message mutated later must be seen by the UI.
    const job: AudioJob = reactive<AudioJob>({
      id: `extract-${trackIndex}-${Date.now()}`,
      type: "extract",
      status: JOB_STATUS.RUNNING,
    });
    jobs.value.push(job);

    try {
      await apiInvoke("extract_audio_track", {
        filePath: selectedFile.value,
        trackIndex,
        outputPath,
        format,
      });
      finish(job, outputPath);
    } catch (e) {
      fail(job, e);
    }
  }

  async function mergeAudio(videoPath: string, audioPath: string, outputPath: string) {
    // reactive(): the status/message mutated later must be seen by the UI.
    const job: AudioJob = reactive<AudioJob>({
      id: `merge-${Date.now()}`,
      type: "merge",
      status: JOB_STATUS.RUNNING,
    });
    jobs.value.push(job);

    try {
      await apiInvoke("merge_audio_track", {
        videoPath,
        audioPath,
        outputPath,
      });
      finish(job, outputPath);
    } catch (e) {
      fail(job, e);
    }
  }

  async function removeAllAudio(outputPath: string) {
    if (!selectedFile.value) {
      error.value = "No file selected.";
      return;
    }
    // reactive(): the status/message mutated later must be seen by the UI.
    const job: AudioJob = reactive<AudioJob>({
      id: `remove-${Date.now()}`,
      type: "remove",
      status: JOB_STATUS.RUNNING,
    });
    jobs.value.push(job);

    try {
      await apiInvoke("remove_audio_tracks", {
        filePath: selectedFile.value,
        outputPath,
      });
      finish(job, outputPath);
    } catch (e) {
      fail(job, e);
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
