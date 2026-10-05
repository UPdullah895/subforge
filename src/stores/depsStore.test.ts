import { describe, it, expect, vi, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { useDepsStore, type DepReport, type DepStatus } from "./depsStore";

vi.mock("../api/client", () => ({ apiInvoke: vi.fn() }));
vi.mock("../api/events", () => ({ apiListen: vi.fn() }));

import { apiInvoke } from "../api/client";
import { apiListen } from "../api/events";

function item(over: Partial<DepStatus> & Pick<DepStatus, "id">): DepStatus {
  return {
    label: over.id,
    severity: "required",
    state: "ok",
    purpose: "",
    download_url: "https://example.com",
    can_auto_install: false,
    ...over,
  };
}

function report(items: DepStatus[]): DepReport {
  return {
    os: "linux",
    items,
    has_missing_required: items.some((i) => i.severity === "required" && i.state !== "ok"),
  };
}

describe("depsStore", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("check stores the report and opens the dialog when required deps are missing", async () => {
    vi.mocked(apiInvoke).mockResolvedValue(report([item({ id: "ffmpeg", state: "missing" })]));
    const s = useDepsStore();
    await s.check();
    expect(apiInvoke).toHaveBeenCalledWith("check_dependencies");
    expect(s.report?.items).toHaveLength(1);
    expect(s.showDialog).toBe(true);
    expect(s.showBanner).toBe(false);
  });

  it("does not open for optional-only gaps", async () => {
    vi.mocked(apiInvoke).mockResolvedValue(
      report([item({ id: "faster_whisper", severity: "optional", state: "missing" })])
    );
    const s = useDepsStore();
    await s.check();
    expect(s.showDialog).toBe(false);
    expect(s.showBanner).toBe(false);
  });

  it("dismiss swaps the dialog for the banner and reopen reverses it", async () => {
    vi.mocked(apiInvoke).mockResolvedValue(report([item({ id: "ffmpeg", state: "missing" })]));
    const s = useDepsStore();
    await s.check();
    s.dismiss();
    expect(s.showDialog).toBe(false);
    expect(s.showBanner).toBe(true);
    s.reopen();
    expect(s.showDialog).toBe(true);
  });

  it("check failure leaves report null without throwing", async () => {
    vi.mocked(apiInvoke).mockRejectedValue(new Error("offline"));
    const s = useDepsStore();
    await s.check();
    expect(s.report).toBeNull();
    expect(s.showDialog).toBe(false);
  });

  it("install updates the report from the reply and clears installing", async () => {
    vi.mocked(apiInvoke)
      .mockResolvedValueOnce(report([item({ id: "ffmpeg", state: "missing", can_auto_install: true })]))
      .mockResolvedValueOnce(report([item({ id: "ffmpeg", state: "ok" })]));
    const s = useDepsStore();
    await s.check();
    const ok = await s.install("ffmpeg");
    expect(ok).toBe(true);
    expect(apiInvoke).toHaveBeenLastCalledWith("install_dependency", { id: "ffmpeg" });
    expect(s.report?.has_missing_required).toBe(false);
    expect(s.installing).toBeNull();
  });

  it("install failure records the message (string or Error) and re-checks", async () => {
    vi.mocked(apiInvoke)
      .mockResolvedValueOnce(report([item({ id: "ffmpeg", state: "missing", can_auto_install: true })]))
      .mockRejectedValueOnce("pkexec was dismissed")
      .mockResolvedValueOnce(report([item({ id: "ffmpeg", state: "missing", can_auto_install: true })]));
    const s = useDepsStore();
    await s.check();
    const ok = await s.install("ffmpeg");
    expect(ok).toBe(false);
    expect(s.errors["ffmpeg"]).toBe("pkexec was dismissed");
    expect(s.installing).toBeNull();
  });

  it("installAll installs in order and picks up items unlocked by earlier installs", async () => {
    vi.mocked(apiInvoke)
      // initial check: python missing (auto), pysubs2 missing (not yet auto)
      .mockResolvedValueOnce(
        report([
          item({ id: "python", state: "missing", can_auto_install: true }),
          item({ id: "pysubs2", state: "missing", can_auto_install: false }),
        ])
      )
      // after installing python, pysubs2 becomes installable
      .mockResolvedValueOnce(
        report([
          item({ id: "python", state: "ok" }),
          item({ id: "pysubs2", state: "missing", can_auto_install: true }),
        ])
      )
      // after installing pysubs2 everything is ok
      .mockResolvedValueOnce(report([item({ id: "python", state: "ok" }), item({ id: "pysubs2", state: "ok" })]));
    const s = useDepsStore();
    await s.check();
    await s.installAll();
    const installs = vi.mocked(apiInvoke).mock.calls.filter((c) => c[0] === "install_dependency");
    expect(installs.map((c) => (c[1] as { id: string }).id)).toEqual(["python", "pysubs2"]);
    expect(s.report?.has_missing_required).toBe(false);
  });

  it("installAll stops at the first failure", async () => {
    vi.mocked(apiInvoke)
      .mockResolvedValueOnce(
        report([
          item({ id: "mkvmerge", state: "missing", can_auto_install: true }),
          item({ id: "ffmpeg", state: "missing", can_auto_install: true }),
        ])
      )
      .mockRejectedValueOnce(new Error("boom"))
      .mockResolvedValueOnce(
        report([
          item({ id: "mkvmerge", state: "missing", can_auto_install: true }),
          item({ id: "ffmpeg", state: "missing", can_auto_install: true }),
        ])
      );
    const s = useDepsStore();
    await s.check();
    await s.installAll();
    const installs = vi.mocked(apiInvoke).mock.calls.filter((c) => c[0] === "install_dependency");
    expect(installs).toHaveLength(1);
  });

  it("listen appends installer output to the per-dependency log", async () => {
    let handler: (p: { job_id: string; message?: string }) => void = () => {};
    vi.mocked(apiListen).mockImplementation(async (_event, h) => {
      handler = h as typeof handler;
      return () => {};
    });
    const s = useDepsStore();
    await s.listen();
    handler({ job_id: "ffmpeg", message: "downloading" });
    handler({ job_id: "ffmpeg", message: "done" });
    expect(s.logs["ffmpeg"]).toEqual(["downloading", "done"]);
  });
});
