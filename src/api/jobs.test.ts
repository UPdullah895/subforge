import { beforeEach, describe, expect, it, vi } from "vitest";

type Handler = (payload: unknown) => void;
const handlers = new Map<string, Handler[]>();

vi.mock("./events", () => ({
  apiListen: vi.fn(async (event: string, handler: Handler) => {
    handlers.set(event, [...(handlers.get(event) ?? []), handler]);
    return () => handlers.set(event, (handlers.get(event) ?? []).filter((h) => h !== handler));
  }),
}));
vi.mock("./client", () => ({ isTauri: () => true }));

import { startAndWait } from "./jobs";

const emit = (event: string, payload: unknown) => handlers.get(event)?.forEach((h) => h(payload));

describe("startAndWait", () => {
  beforeEach(() => handlers.clear());

  it("resolves when the completion event arrives after the start request returned", async () => {
    const result = startAndWait(async () => "job-1");
    await Promise.resolve();
    await new Promise((r) => setTimeout(r, 0));
    emit("job-completed", { job_id: "job-1", status: "completed", output_path: "/x.mkv" });
    await expect(result).resolves.toMatchObject({ status: "completed", output_path: "/x.mkv" });
  });

  it("does not miss a job that finishes before the start request returns", async () => {
    const result = startAndWait(async () => {
      // The backend finishes the job and emits its event before replying.
      emit("job-completed", { job_id: "fast", status: "completed" });
      return "fast";
    });
    await expect(result).resolves.toMatchObject({ job_id: "fast", status: "completed" });
  });

  it("ignores other jobs' events and accepts the legacy { job_id } reply", async () => {
    const result = startAndWait(async () => ({ job_id: "mine" }));
    await new Promise((r) => setTimeout(r, 0));
    emit("job-completed", { job_id: "someone-else", status: "failed" });
    emit("job-completed", { job_id: "mine", status: "failed", message: "boom" });
    await expect(result).resolves.toMatchObject({ job_id: "mine", status: "failed", message: "boom" });
  });

  it("reports progress and unsubscribes when done", async () => {
    const onProgress = vi.fn();
    const result = startAndWait(async () => "p", { onProgress });
    await new Promise((r) => setTimeout(r, 0));
    emit("job-progress", { job_id: "p", progress: 42 });
    emit("job-completed", { job_id: "p", status: "completed" });
    await result;
    expect(onProgress).toHaveBeenCalledWith(42);
    expect(handlers.get("job-completed")).toHaveLength(0);
  });
});
