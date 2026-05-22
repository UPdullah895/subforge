import { describe, it, expect, beforeEach } from "vitest";
import { useJobQueue } from "./useJobQueue";
import { JOB_STATUS } from "../constants/jobStatus";

describe("useJobQueue", () => {
  let queue: ReturnType<typeof useJobQueue>;

  beforeEach(() => {
    queue = useJobQueue();
  });

  it("addJob creates a pending job", () => {
    queue.addJob("job-1", "file.srt");
    expect(queue.jobs.value).toHaveLength(1);
    expect(queue.jobs.value[0]).toMatchObject({
      id: "job-1",
      status: JOB_STATUS.PENDING,
      input_file: "file.srt",
      progress: 0,
    });
  });

  it("updateJobProgress updates matching job", () => {
    queue.addJob("job-1", "file.srt");
    queue.updateJobProgress("job-1", 50, "halfway");
    expect(queue.jobs.value[0].progress).toBe(50);
    expect(queue.jobs.value[0].message).toBe("halfway");
  });

  it("completeJob sets status and progress to 100", () => {
    queue.addJob("job-1", "file.srt");
    queue.completeJob("job-1", JOB_STATUS.COMPLETED);
    expect(queue.jobs.value[0].status).toBe(JOB_STATUS.COMPLETED);
    expect(queue.jobs.value[0].progress).toBe(100);
  });

  it("completeJob sets FAILED status", () => {
    queue.addJob("job-1", "file.srt");
    queue.completeJob("job-1", JOB_STATUS.FAILED, "something broke");
    expect(queue.jobs.value[0].status).toBe(JOB_STATUS.FAILED);
    expect(queue.jobs.value[0].message).toBe("something broke");
  });

  it("clearCompleted removes only completed jobs (regression)", () => {
    queue.addJob("job-1", "a.srt");
    queue.addJob("job-2", "b.srt");
    queue.completeJob("job-1", JOB_STATUS.COMPLETED);
    queue.clearCompleted();
    expect(queue.jobs.value).toHaveLength(1);
    expect(queue.jobs.value[0].id).toBe("job-2");
  });

  it("clearAll empties the list", () => {
    queue.addJob("job-1", "a.srt");
    queue.addJob("job-2", "b.srt");
    queue.clearAll();
    expect(queue.jobs.value).toHaveLength(0);
  });
});
