import { describe, it, expect, vi, beforeEach } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { i18n } from "../i18n";

vi.mock("../api/client", () => ({ apiInvoke: vi.fn(), isTauri: () => false }));
vi.mock("../utils/dialog", () => ({ open: vi.fn() }));

import { apiInvoke } from "../api/client";
import SettingsView from "./SettingsView.vue";
import { useAppStore } from "../stores/appStore";

const status = [
  { id: "ffmpeg", source: "bundled", path: "/b/ffmpeg", version: "7", error: null },
  { id: "ffprobe", source: "system", path: "/usr/bin/ffprobe", version: "7", error: null },
];

function setup() {
  setActivePinia(createPinia());
  vi.mocked(apiInvoke).mockImplementation(async (cmd: string) => {
    if (cmd === "load_config") return { theme: "dark", language: "en", ffmpeg_path: "/c/ffmpeg" };
    if (cmd === "get_tool_status") return status;
    return undefined;
  });
  return mount(SettingsView, {
    global: {
      plugins: [i18n],
      stubs: { DependenciesPanel: true, SetupInstructions: true },
    },
  });
}

const calls = () => vi.mocked(apiInvoke).mock.calls.map((c) => c[0]);

describe("SettingsView tool paths", () => {
  beforeEach(() => vi.clearAllMocks());

  it("Auto-detect only refreshes status: config untouched, no save_config", async () => {
    const w = setup();
    await flushPromises();
    const store = useAppStore();
    const before = JSON.stringify(store.config);
    vi.mocked(apiInvoke).mockClear();
    const btn = w.findAll("button").find((b) => b.text().includes("Auto-detect"))!;
    await btn.trigger("click");
    await flushPromises();
    expect(JSON.stringify(store.config)).toBe(before);
    expect(calls()).toContain("get_tool_status");
    expect(calls()).not.toContain("save_config");
  });

  it("a field change saves the config before reloading tool status", async () => {
    const w = setup();
    await flushPromises();
    vi.mocked(apiInvoke).mockClear();
    const input = w.get('[data-testid="input-ffmpeg"]');
    await input.setValue("/new/ffmpeg");
    await input.trigger("change");
    await flushPromises();
    expect(calls()).toEqual(["save_config", "get_tool_status"]);
    expect(useAppStore().config?.ffmpeg_path).toBe("/new/ffmpeg");
  });

  it("Reset clears config paths and badges without reloading; Save restores badges", async () => {
    const w = setup();
    await flushPromises();
    const store = useAppStore();
    store.config!.mkvpropedit_path = "/x/mkvpropedit";
    expect(w.find('[data-testid="badge-ffmpeg"]').exists()).toBe(true);
    vi.mocked(apiInvoke).mockClear();
    const reset = w.findAll("button").find((b) => b.text().trim() === "Reset")!;
    await reset.trigger("click");
    await flushPromises();
    expect(store.config?.mkvpropedit_path).toBeUndefined();
    expect(calls()).not.toContain("get_tool_status");
    expect(calls()).not.toContain("save_config");
    expect(w.find('[data-testid="badge-ffmpeg"]').exists()).toBe(false);
    const save = w.findAll("button").find((b) => b.text().trim() === "Save")!;
    await save.trigger("click");
    await flushPromises();
    expect(calls()).toEqual(["save_config", "get_tool_status"]);
    expect(w.get('[data-testid="badge-ffmpeg"]').text()).toBe("Bundled");
  });
});
