import { describe, it, expect, vi, beforeEach } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { i18n } from "../i18n";

vi.mock("../api/client", () => ({ apiInvoke: vi.fn(), isTauri: () => false }));
vi.mock("../utils/dialog", () => ({ open: vi.fn() }));

import { apiInvoke } from "../api/client";
import { open } from "../utils/dialog";
import SyncCheckPanel from "./SyncCheckPanel.vue";

const check = (overrides: Record<string, unknown> = {}) => ({
  verdict: "good",
  cue_count: 42,
  subtitle_start_ms: 1000,
  subtitle_end_ms: 104_000,
  video_duration_ms: 100_000,
  video_fps: 23.976,
  suggested_fix: null,
  suggested_shift_ms: null,
  issues: [],
  ...overrides,
});

function setup(response: Record<string, unknown>) {
  setActivePinia(createPinia());
  vi.mocked(apiInvoke).mockImplementation(async (cmd: string) => {
    if (cmd === "check_subtitle_sync") return response;
    if (cmd === "fix_subtitle_sync")
      return { output_path: "/tmp/episode.synced.srt", backup_path: null, cues: 42 };
    return undefined;
  });
  vi.mocked(open).mockResolvedValueOnce("/tmp/episode.srt").mockResolvedValueOnce("/tmp/episode.mkv");
  return mount(SyncCheckPanel, { global: { plugins: [i18n] } });
}

async function pickBoth(w: ReturnType<typeof setup>) {
  await w.get('[data-testid="sync-pick-subtitle"]').trigger("click");
  await flushPromises();
  await w.get('[data-testid="sync-pick-video"]').trigger("click");
  await flushPromises();
}

describe("SyncCheckPanel", () => {
  beforeEach(() => vi.clearAllMocks());

  it("reports a frame-rate mismatch and offers the fix", async () => {
    const w = setup({
      check: check({
        verdict: "framerate",
        suggested_fix: { from: 25, to: 23.976 },
        issues: [{ kind: "framerate", message: "The subtitle spans 4.2% of the video" }],
      }),
      deep: null,
      model: null,
    });
    await pickBoth(w);
    await w.get('[data-testid="sync-check"]').trigger("click");
    await flushPromises();

    expect(w.get('[data-testid="sync-result"]').text()).toContain("Frame-rate mismatch");
    expect(w.get('[data-testid="sync-result"]').text()).toContain("25 → 23.976 fps");
  });

  it("sends the suggested frame-rate fix", async () => {
    const w = setup({
      check: check({ verdict: "framerate", suggested_fix: { from: 25, to: 23.976 } }),
      deep: null,
      model: null,
    });
    await pickBoth(w);
    await w.get('[data-testid="sync-check"]').trigger("click");
    await flushPromises();
    await w.get('[data-testid="sync-apply"]').trigger("click");
    await flushPromises();

    const call = vi.mocked(apiInvoke).mock.calls.find((c) => c[0] === "fix_subtitle_sync")!;
    const request = (call[1] as { request: Record<string, unknown> }).request;
    expect(request.from_fps).toBe(25);
    expect(request.to_fps).toBe(23.976);
    expect(w.get('[data-testid="sync-done"]').text()).toContain("/tmp/episode.synced.srt");
  });

  it("turns a deep-check offset into a shift", async () => {
    const w = setup({
      check: check(),
      deep: {
        verdict: "offset",
        matches: [
          {
            subtitle_start_ms: 22_000,
            asr_start_ms: 0,
            offset_ms: 2000,
            score: 0.9,
            subtitle_text: "hello there",
          },
        ],
        offset_ms: 2000,
        suggested_shift_ms: -2000,
        suggested_fix: null,
        reason: "offset",
      },
      model: "tiny",
    });
    await pickBoth(w);
    await w.get('[data-testid="sync-deep"]').trigger("click");
    await flushPromises();

    expect(w.get('[data-testid="sync-result"]').text()).toContain("Off by a constant delay");
    expect(w.get('[data-testid="sync-result"]').text()).toContain("shift -2.0 s");

    await w.get('[data-testid="sync-apply"]').trigger("click");
    await flushPromises();
    const call = vi.mocked(apiInvoke).mock.calls.find((c) => c[0] === "fix_subtitle_sync")!;
    expect((call[1] as { request: { shift_ms: number } }).request.shift_ms).toBe(-2000);
  });

  it("shows no fix button when the subtitle is in sync", async () => {
    const w = setup({ check: check(), deep: null, model: null });
    await pickBoth(w);
    await w.get('[data-testid="sync-check"]').trigger("click");
    await flushPromises();
    expect(w.find('[data-testid="sync-apply"]').exists()).toBe(false);
  });
});
