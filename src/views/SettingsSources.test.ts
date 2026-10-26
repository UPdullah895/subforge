import { describe, it, expect, vi, beforeEach } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { i18n } from "../i18n";

vi.mock("../api/client", () => ({ apiInvoke: vi.fn(), isTauri: () => false }));
vi.mock("../utils/dialog", () => ({ open: vi.fn() }));

import { apiInvoke } from "../api/client";
import SettingsView from "./SettingsView.vue";

const status = [
  { id: "ffmpeg", source: "bundled", path: "/b/ffmpeg", version: "7", error: null },
];

function setup(overrides: Record<string, unknown> = {}) {
  setActivePinia(createPinia());
  vi.mocked(apiInvoke).mockImplementation(async (cmd: string) => {
    if (cmd === "load_config") return { theme: "dark", language: "en" };
    if (cmd === "get_tool_status") return status;
    if (cmd === "get_opensubtitles_status") {
      return {
        configured: true,
        username: "updullah",
        api_key: "secret-key-123",
        password_set: true,
      };
    }
    if (cmd === "check_subsource_status") {
      return { configured: false, reachable: false, reason: "not_configured" };
    }
    if (cmd in overrides) {
      const value = overrides[cmd];
      if (value instanceof Error) throw value;
      return value;
    }
    return undefined;
  });
  return mount(SettingsView, {
    global: {
      plugins: [i18n],
      stubs: { DependenciesPanel: true, SetupInstructions: true },
    },
  });
}

describe("SettingsView subtitle sources", () => {
  beforeEach(() => vi.clearAllMocks());

  it("fills the OpenSubtitles form from the stored credentials", async () => {
    const w = setup();
    await flushPromises();

    const username = w.get('[data-testid="input-opensubtitles-username"]')
      .element as HTMLInputElement;
    expect(username.value).toBe("updullah");

    const key = w.get('[data-testid="input-opensubtitles-key"]').element as HTMLInputElement;
    expect(key.value).toBe("secret-key-123");
    // Masked until the eye is clicked.
    expect(key.type).toBe("password");
    await w.get('[data-testid="input-opensubtitles-key-reveal"]').trigger("click");
    expect(key.type).toBe("text");
  });

  it("does not overwrite stored credentials with an empty form", async () => {
    const w = setup();
    await flushPromises();
    await w.get('[data-testid="input-opensubtitles-key"]').setValue("");
    await w.get('[data-testid="input-opensubtitles-username"]').setValue("");

    await w.get('[data-testid="opensubtitles-save"]').trigger("click");
    await flushPromises();

    const calls = vi.mocked(apiInvoke).mock.calls.map((c) => c[0]);
    expect(calls).not.toContain("set_opensubtitles_credentials");
    expect(w.text()).toContain("Enter the OpenSubtitles API key and username");
  });

  it("tests the API key currently in the form, before it is saved", async () => {
    const w = setup();
    await flushPromises();
    await w.get('[data-testid="input-tmdb-key"]').setValue("typed-not-saved");
    vi.mocked(apiInvoke).mockClear();

    const testButton = w.findAll("button").find((b) => b.text().includes("Test"))!;
    await testButton.trigger("click");
    await flushPromises();

    expect(apiInvoke).toHaveBeenCalledWith("test_provider_config", {
      provider: "tmdb",
      apiKey: "typed-not-saved",
    });
  });

  it("says Not configured for Subsource instead of Unreachable", async () => {
    const w = setup();
    await flushPromises();
    const label = w.get('[data-testid="subsource-status"]');
    expect(label.text()).toBe("Not configured");
  });

  it("shows a save failure instead of silently dropping it", async () => {
    const w = setup({ save_config: new Error("disk is full") });
    await flushPromises();
    const input = w.get('[data-testid="input-ffmpeg"]');
    await input.setValue("/new/ffmpeg");
    await input.trigger("change");
    await flushPromises();

    const banner = w.get('[data-testid="save-error"]');
    expect(banner.text()).toContain("disk is full");
  });
});
