import { ref } from "vue";
import { JOB_STATUS, type JobStatus } from "../constants/jobStatus";
import { apiInvoke } from "../api/client";

export interface Job {
  id: string;
  status: JobStatus;
  input_file: string;
  progress: number;
  message?: string;
  /** Raw tool output of a failed job. */
  details?: string;
}

export function useJobQueue() {
  const jobs = ref<Job[]>([]);

  function addJob(id: string, input_file: string): void {
    jobs.value.push({ id, status: JOB_STATUS.PENDING, input_file, progress: 0 });
  }

  function updateJobProgress(jobId: string, progress: number, message?: string): void {
    const job = jobs.value.find((j) => j.id === jobId);
    if (job) {
      job.progress = progress;
      job.status = JOB_STATUS.RUNNING;
      if (message) job.message = message;
    }
  }

  function completeJob(jobId: string, status: JobStatus, message?: string, details?: string | null): void {
    const job = jobs.value.find((j) => j.id === jobId);
    if (job) {
      job.status = status;
      job.progress = 100;
      if (message) job.message = message;
      if (details) job.details = details;
    }
  }

  function clearCompleted(): void {
    jobs.value = jobs.value.filter(
      (j) => j.status === JOB_STATUS.PENDING || j.status === JOB_STATUS.RUNNING
    );
  }

  function clearAll(): void {
    jobs.value = [];
  }

  function removeJob(id: string): void {
    jobs.value = jobs.value.filter((j) => j.id !== id);
  }

  async function cancelJob(jobId: string): Promise<void> {
    try {
      await apiInvoke<void>("cancel_job", { request: { job_id: jobId } });
    } catch (e) {
      // Expected if job already finished or never existed
      console.debug(`Cancel job failed (expected if job finished): ${jobId}`, e);
    }
  }

  async function pauseJob(jobId: string): Promise<void> {
    try {
      await apiInvoke<void>("pause_job", { request: { job_id: jobId } });
    } catch (e) {
      console.debug(`Pause job failed: ${jobId}`, e);
    }
  }

  async function resumeJob(jobId: string): Promise<void> {
    try {
      await apiInvoke<void>("resume_job", { request: { job_id: jobId } });
    } catch (e) {
      console.debug(`Resume job failed: ${jobId}`, e);
    }
  }

  return { jobs, addJob, updateJobProgress, completeJob, clearCompleted, clearAll, removeJob, cancelJob, pauseJob, resumeJob };
}
