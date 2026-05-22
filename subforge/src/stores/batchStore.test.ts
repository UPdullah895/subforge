import { describe, it, expect, vi, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { useBatchStore } from "./batchStore";

vi.mock("../api/client", () => ({
  apiInvoke: vi.fn(),
}));

import { apiInvoke } from "../api/client";

describe("batchStore", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("batchExtract maps jobs to correct files (index-drift regression)", async () => {
    const store = useBatchStore();
    store.selectedFiles = new Set(["a.mkv", "b.mkv"]);
    vi.mocked(apiInvoke).mockResolvedValue(["job-id-0", "job-id-1"]);

    await store.batchExtract("/out");

    expect(store.jobs).toHaveLength(2);
    expect(store.jobs[0].input_file).toBe("a.mkv");
    expect(store.jobs[1].input_file).toBe("b.mkv");
  });

  it("batchExtract creates one job per file", async () => {
    const store = useBatchStore();
    store.selectedFiles = new Set(["a.mkv", "b.mkv", "c.mkv"]);
    vi.mocked(apiInvoke).mockResolvedValue(["job-id-0", "job-id-1", "job-id-2"]);

    await store.batchExtract("/out");

    expect(store.jobs).toHaveLength(3);
  });

  it("batchExtract sets error when backend fails", async () => {
    const store = useBatchStore();
    store.selectedFiles = new Set(["a.mkv"]);
    vi.mocked(apiInvoke).mockRejectedValue(new Error("backend error"));

    await store.batchExtract("/out");

    expect(store.error).toBeTruthy();
    expect(store.jobs).toHaveLength(0);
  });
});
