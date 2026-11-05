import { describe, it, expect, vi, beforeEach } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { i18n } from "../i18n";
import UpdateBanner from "./UpdateBanner.vue";
import { useUpdateStore } from "../stores/updateStore";

vi.mock("../utils/openExternal", () => ({ openExternal: vi.fn().mockResolvedValue(undefined) }));

import { openExternal } from "../utils/openExternal";

// jsdom in this setup has no usable localStorage, and the store only needs get/set.
function memoryStorage(): Storage {
  const data = new Map<string, string>();
  return {
    get length() {
      return data.size;
    },
    clear: () => data.clear(),
    getItem: (key: string) => data.get(key) ?? null,
    key: (index: number) => Array.from(data.keys())[index] ?? null,
    removeItem: (key: string) => void data.delete(key),
    setItem: (key: string, value: string) => void data.set(key, value),
  } as Storage;
}

describe("UpdateBanner", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.stubGlobal("localStorage", memoryStorage());
    vi.clearAllMocks();
    // No network in unit tests: a failed check leaves the banner hidden.
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));
  });

  it("is hidden while there is no newer release", async () => {
    const w = mount(UpdateBanner, { global: { plugins: [i18n] } });
    await w.vm.$nextTick();
    expect(w.find('[data-testid="update-banner"]').exists()).toBe(false);
  });

  it("names the new version and opens the release page", async () => {
    const update = useUpdateStore();
    update.available = { version: "9.9.9", url: "https://example.test/release" };
    const w = mount(UpdateBanner, { global: { plugins: [i18n] } });
    await w.vm.$nextTick();

    const banner = w.get('[data-testid="update-banner"]');
    expect(banner.text()).toContain("9.9.9");

    await w.get('[data-testid="update-open"]').trigger("click");
    expect(openExternal).toHaveBeenCalledWith("https://example.test/release");
  });

  it("dismiss hides the banner", async () => {
    const update = useUpdateStore();
    update.available = { version: "9.9.9", url: "https://example.test/release" };
    const w = mount(UpdateBanner, { global: { plugins: [i18n] } });
    await w.vm.$nextTick();

    await w.get('[data-testid="update-dismiss"]').trigger("click");
    expect(update.available).toBeNull();
    expect(w.find('[data-testid="update-banner"]').exists()).toBe(false);
  });
});
