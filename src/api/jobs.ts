import { apiListen } from "./events";
import { isTauri } from "./client";

export interface JobResult {
  job_id: string;
  status: string;
  message?: string;
  /** Raw tool / network output of a failed job, when the backend has it. */
  details?: string | null;
  output_path?: string;
}

interface ProgressEvent {
  job_id: string;
  progress?: number;
}

export interface StartAndWaitOptions {
  onProgress?: (percent: number) => void;
  /** Receives the job id as soon as the backend has accepted the job. */
  onStarted?: (jobId: string) => void;
}

const TERMINAL = new Set(["completed", "failed", "cancelled"]);
const POLL_MS = 2500;

// Handlers answer with a bare id (desktop) or, on older servers, `{ job_id }`.
function jobIdOf(started: string | { job_id: string }): string {
  return typeof started === "string" ? started : started.job_id;
}

/**
 * Starts a background job and resolves when it finishes.
 *
 * The completion event is subscribed to *before* the job is started and
 * buffered by id: a short job can finish before the start request even
 * returns, and a listener attached afterwards would wait forever. In server
 * mode the job status endpoint is also polled as a safety net for a dropped
 * WebSocket.
 */
export async function startAndWait(
  start: () => Promise<string | { job_id: string }>,
  options: StartAndWaitOptions = {}
): Promise<JobResult> {
  const completed = new Map<string, JobResult>();
  let wantedId: string | null = null;
  let finish: ((r: JobResult) => void) | null = null;

  const offProgress = await apiListen<ProgressEvent>("job-progress", (p) => {
    if ((wantedId === null || p.job_id === wantedId) && typeof p.progress === "number") {
      options.onProgress?.(p.progress);
    }
  });
  const offCompleted = await apiListen<JobResult>("job-completed", (p) => {
    completed.set(p.job_id, p);
    if (p.job_id === wantedId) finish?.(p);
  });

  let poll: ReturnType<typeof setInterval> | null = null;
  try {
    const jobId = jobIdOf(await start());
    wantedId = jobId;
    options.onStarted?.(jobId);

    return await new Promise<JobResult>((resolve) => {
      finish = resolve;
      const already = completed.get(jobId);
      if (already) return resolve(already);

      if (!isTauri()) {
        poll = setInterval(async () => {
          try {
            const resp = await fetch(`/api/jobs/${encodeURIComponent(jobId)}`);
            if (!resp.ok) return;
            const job = (await resp.json()) as {
              status: string;
              message?: string | null;
              output_file?: string | null;
            };
            if (TERMINAL.has(job.status)) {
              resolve({
                job_id: jobId,
                status: job.status,
                message: job.message ?? undefined,
                output_path: job.output_file ?? undefined,
              });
            }
          } catch {
            /* transient; the event or the next poll will settle it */
          }
        }, POLL_MS);
      }
    });
  } finally {
    if (poll) clearInterval(poll);
    offProgress();
    offCompleted();
  }
}
