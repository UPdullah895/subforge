import { describe, it, expect, vi, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";

vi.mock("../api/client", () => ({
  apiInvoke: vi.fn(),
}));
vi.mock("../api/jobs", () => ({
  startAndWait: vi.fn(),
}));

import { apiInvoke } from "../api/client";
import { startAndWait, type JobResult, type StartAndWaitOptions } from "../api/jobs";
import { useTranscribeStore } from "./transcribeStore";

describe("transcribeStore", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
    try {
      localStorage.clear();
    } catch {
      /* no storage in this environment */
    }
  });

  it("downloadModel starts job and tracks progress", async () => {
    const store = useTranscribeStore();
    vi.mocked(apiInvoke).mockImplementation(async (cmd: string) => {
      if (cmd === "download_whisper_model") return "download-1";
      if (cmd === "list_whisper_models") return ["base"];
      return undefined;
    });

    let release: (r: JobResult) => void = () => {};
    let opts: StartAndWaitOptions = {};
    vi.mocked(startAndWait).mockImplementation(async (start, options = {}) => {
      opts = options;
      const id = (await start()) as string;
      options.onStarted?.(id);
      return new Promise<JobResult>((resolve) => {
        release = resolve;
      });
    });

    const done = store.downloadModel("base");
    await vi.waitFor(() => expect(store.download?.jobId).toBe("download-1"));
    expect(apiInvoke).toHaveBeenCalledWith("download_whisper_model", { model: "base", cacheDir: null });
    expect(store.downloadingModel).toBe("base");
    expect(store.download?.progress).toBe(0);

    opts.onProgress?.(42.5);
    expect(store.download?.progress).toBe(42.5);

    // Cancel goes through the shared job-control command with the tracked id.
    await store.cancelDownload();
    expect(apiInvoke).toHaveBeenCalledWith("cancel_job", { request: { job_id: "download-1" } });

    release({ job_id: "download-1", status: "completed" });
    await done;
    expect(store.download).toBeNull();
    expect(store.downloadingModel).toBeNull();
    expect(store.downloadedModels).toEqual(["base"]);
    expect(store.error).toBeNull();
  });

  it("downloadModel reports a failed job and stays quiet on cancel", async () => {
    const store = useTranscribeStore();
    vi.mocked(apiInvoke).mockResolvedValue([]);
    vi.mocked(startAndWait).mockResolvedValueOnce({ job_id: "d", status: "failed", message: "HTTP 404" });
    await store.downloadModel("tiny");
    expect(store.error).toContain("HTTP 404");

    store.error = null;
    vi.mocked(startAndWait).mockResolvedValueOnce({ job_id: "d", status: "cancelled", message: "Cancelled" });
    await store.downloadModel("tiny");
    expect(store.error).toBeNull();
    expect(store.download).toBeNull();
  });

  it("device list hides gpu when not detected", async () => {
    const store = useTranscribeStore();
    vi.mocked(apiInvoke).mockResolvedValueOnce({ engine: "whisper.cpp 1.8.2", gpu: null, devices: ["auto", "cpu"] });
    await store.loadInfo();
    expect(store.deviceOptions).toEqual([
      { value: "auto", gpuName: null },
      { value: "cpu", gpuName: null },
    ]);

    // A GPU entry without a detected device name is still hidden.
    vi.mocked(apiInvoke).mockResolvedValueOnce({ engine: "whisper.cpp", gpu: null, devices: ["auto", "cpu", "gpu"] });
    await store.loadInfo();
    expect(store.deviceOptions.map((d) => d.value)).toEqual(["auto", "cpu"]);

    vi.mocked(apiInvoke).mockResolvedValueOnce({
      engine: "whisper.cpp",
      gpu: "AMD Radeon RX 7800 XT",
      devices: ["auto", "cpu", "gpu"],
    });
    await store.loadInfo();
    expect(store.deviceOptions).toEqual([
      { value: "auto", gpuName: null },
      { value: "cpu", gpuName: null },
      { value: "gpu", gpuName: "AMD Radeon RX 7800 XT" },
    ]);
  });

  it("model list comes from the catalog", async () => {
    const store = useTranscribeStore();
    vi.mocked(apiInvoke).mockResolvedValueOnce([
      { name: "tiny", size_mb: 74 },
      { name: "base", size_mb: 141 },
    ]);
    await store.loadCatalog();
    expect(apiInvoke).toHaveBeenCalledWith("list_whisper_catalog");
    expect(store.allModels).toEqual(["tiny", "base"]);
    expect(store.catalog[1]).toEqual({ name: "base", size_mb: 141 });
  });

  it("legacy cache note shows once, can be dismissed, and deletes on request", async () => {
    const store = useTranscribeStore();
    vi.mocked(apiInvoke).mockResolvedValueOnce("/home/u/.cache/subforge/whisper-models");
    await store.loadLegacyCache();
    expect(store.showLegacyNote).toBe(true);

    vi.mocked(apiInvoke).mockResolvedValueOnce(undefined);
    await store.deleteLegacyCache();
    expect(apiInvoke).toHaveBeenLastCalledWith("delete_whisper_legacy_cache");
    expect(store.legacyCache).toBeNull();
    expect(store.showLegacyNote).toBe(false);

    vi.mocked(apiInvoke).mockResolvedValueOnce("/old");
    await store.loadLegacyCache();
    store.dismissLegacyNote();
    expect(store.showLegacyNote).toBe(false);

    // Dismissal sticks for a fresh store.
    setActivePinia(createPinia());
    const again = useTranscribeStore();
    vi.mocked(apiInvoke).mockResolvedValueOnce("/old");
    await again.loadLegacyCache();
    expect(again.showLegacyNote).toBe(false);
  });
});
