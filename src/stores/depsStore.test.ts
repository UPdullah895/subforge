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
      report([item({ id: "mkvpropedit", severity: "optional", state: "missing" })])
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

  it("installAll installs every auto-installable required item in order", async () => {
    const both = (a: DepStatus["state"], b: DepStatus["state"]) =>
      report([
        item({ id: "mkvmerge", state: a, can_auto_install: a !== "ok" }),
        item({ id: "ffmpeg", state: b, can_auto_install: b !== "ok" }),
      ]);
    vi.mocked(apiInvoke)
      .mockResolvedValueOnce(both("missing", "missing"))
      .mockResolvedValueOnce(both("ok", "missing"))
      .mockResolvedValueOnce(both("ok", "ok"));
    const s = useDepsStore();
    await s.check();
    await s.installAll();
    const installs = vi.mocked(apiInvoke).mock.calls.filter((c) => c[0] === "install_dependency");
    expect(installs.map((c) => (c[1] as { id: string }).id)).toEqual(["mkvmerge", "ffmpeg"]);
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

  it("installAll skips optional items", async () => {
    vi.mocked(apiInvoke)
      .mockResolvedValueOnce(
        report([
          item({ id: "mkvpropedit", severity: "optional", state: "missing", can_auto_install: true }),
          item({ id: "ffmpeg", state: "missing", can_auto_install: true }),
        ])
      )
      .mockResolvedValueOnce(
        report([
          item({ id: "mkvpropedit", severity: "optional", state: "missing", can_auto_install: true }),
          item({ id: "ffmpeg", state: "ok" }),
        ])
      );
    const s = useDepsStore();
    await s.check();
    await s.installAll();
    const installs = vi.mocked(apiInvoke).mock.calls.filter((c) => c[0] === "install_dependency");
    expect(installs.map((c) => (c[1] as { id: string }).id)).toEqual(["ffmpeg"]);
  });

  it("install ignores a call while another install is running", async () => {
    let resolveFirst: (r: DepReport) => void = () => {};
    vi.mocked(apiInvoke).mockReturnValueOnce(new Promise<DepReport>((r) => (resolveFirst = r)));
    const s = useDepsStore();
    const first = s.install("ffmpeg");
    expect(s.installing).toBe("ffmpeg");
    expect(await s.install("mkvmerge")).toBe(false);
    expect(apiInvoke).toHaveBeenCalledTimes(1);
    resolveFirst(report([item({ id: "ffmpeg", state: "ok" })]));
    expect(await first).toBe(true);
    expect(s.installing).toBeNull();
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
