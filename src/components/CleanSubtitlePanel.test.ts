import { describe, it, expect, vi, beforeEach } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { i18n } from "../i18n";

vi.mock("../api/client", () => ({ apiInvoke: vi.fn(), isTauri: () => false }));
vi.mock("../utils/dialog", () => ({ open: vi.fn() }));

import { apiInvoke } from "../api/client";
import { open } from "../utils/dialog";
import CleanSubtitlePanel from "./CleanSubtitlePanel.vue";

const removed = [
  { index: 0, start_ms: 1000, end_ms: 3000, text: "ترجمة: فريق المثال", rule: "credit" },
  { index: 2, start_ms: 7000, end_ms: 9000, text: "www.example.com", rule: "link" },
];

function setup(preview = { removed, kept: 1, output_path: null, backup_path: null }) {
  setActivePinia(createPinia());
  vi.mocked(apiInvoke).mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
    if (cmd !== "clean_subtitle") return undefined;
    const request = (args as { request: { apply: boolean } }).request;
    if (!request.apply) return preview;
    return { removed, kept: 1, output_path: "/tmp/episode.clean.srt", backup_path: null };
  });
  return mount(CleanSubtitlePanel, { global: { plugins: [i18n] } });
}

async function pickAndScan(w: ReturnType<typeof setup>) {
  vi.mocked(open).mockResolvedValue("/tmp/episode.srt");
  await w.get('[data-testid="clean-pick-file"]').trigger("click");
  await flushPromises();
  await w.get('[data-testid="clean-scan"]').trigger("click");
  await flushPromises();
}

describe("CleanSubtitlePanel", () => {
  beforeEach(() => vi.clearAllMocks());

  it("shows what the scan found, with the match reason", async () => {
    const w = setup();
    await pickAndScan(w);

    expect(w.get('[data-testid="clean-results"]').text()).toContain("ترجمة: فريق المثال");
    expect(w.get('[data-testid="clean-results"]').text()).toContain("www.example.com");
    expect(w.get('[data-testid="clean-results"]').text()).toContain("Credit/ad");
  });

  it("sends the un-ticked lines as keep on apply", async () => {
    const w = setup();
    await pickAndScan(w);
    // Un-tick the first line: it must be kept.
    await w.get('[data-testid="clean-keep-0"]').trigger("change");
    await w.get('[data-testid="clean-apply"]').trigger("click");
    await flushPromises();

    const applyCall = vi
      .mocked(apiInvoke)
      .mock.calls.find((c) => (c[1] as { request: { apply: boolean } }).request.apply)!;
    expect((applyCall[1] as { request: { keep: number[] } }).request.keep).toEqual([0]);
  });

  it("only overwrites the original when the option is ticked", async () => {
    const w = setup();
    await pickAndScan(w);
    await w.get('[data-testid="clean-apply"]').trigger("click");
    await flushPromises();

    const first = vi
      .mocked(apiInvoke)
      .mock.calls.find((c) => (c[1] as { request: { apply: boolean } }).request.apply)!;
    expect((first[1] as { request: { output: string | null } }).request.output).toBeNull();
  });

  it("reports an empty scan instead of showing an empty table", async () => {
    const w = setup({ removed: [], kept: 12, output_path: null, backup_path: null });
    await pickAndScan(w);
    expect(w.find('[data-testid="clean-empty"]').exists()).toBe(true);
    expect(w.find('[data-testid="clean-results"]').exists()).toBe(false);
  });

  it("shows where the cleaned file was written", async () => {
    const w = setup();
    await pickAndScan(w);
    await w.get('[data-testid="clean-apply"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="clean-done"]').text()).toContain("/tmp/episode.clean.srt");
  });
});
