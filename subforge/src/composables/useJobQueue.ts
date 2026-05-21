import { ref } from "vue";
import { JOB_STATUS, type JobStatus } from "../constants/jobStatus";

export interface Job {
  id: string;
  status: JobStatus;
  input_file: string;
  progress: number;
  message?: string;
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

  function completeJob(jobId: string, status: JobStatus, message?: string): void {
    const job = jobs.value.find((j) => j.id === jobId);
    if (job) {
      job.status = status;
      job.progress = 100;
      if (message) job.message = message;
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

  return { jobs, addJob, updateJobProgress, completeJob, clearCompleted, clearAll };
}
