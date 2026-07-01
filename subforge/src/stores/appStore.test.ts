import { describe, it, expect, vi, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { useAppStore } from "./appStore";

vi.mock("../api/client", () => ({
  apiInvoke: vi.fn(),
}));

import { apiInvoke } from "../api/client";

describe("appStore", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("loadConfig falls back to dark theme when backend fails", async () => {
    vi.mocked(apiInvoke).mockRejectedValue(new Error("backend unavailable"));
    const store = useAppStore();
    await store.loadConfig();
    expect(store.config).toEqual({ theme: "dark", language: "en" });
    expect(store.theme).toBe("dark");
    expect(store.error).toBeNull();
  });

  it("loadConfig applies theme from backend", async () => {
    vi.mocked(apiInvoke).mockResolvedValue({ theme: "light" });
    const store = useAppStore();
    await store.loadConfig();
    expect(store.theme).toBe("light");
  });

  it("saveConfig sets error when backend fails", async () => {
    vi.mocked(apiInvoke)
      .mockResolvedValueOnce({ theme: "dark" }) // loadConfig
      .mockRejectedValueOnce(new Error("disk full")); // saveConfig
    const store = useAppStore();
    await store.loadConfig();
    await store.saveConfig();
    expect(store.error).toMatch(/Failed to save settings/);
  });

  it("toggleTheme switches between dark and light", async () => {
    vi.mocked(apiInvoke).mockResolvedValue({ theme: "dark" });
    const store = useAppStore();
    await store.loadConfig();
    await store.toggleTheme();
    expect(store.theme).toBe("light");
    await store.toggleTheme();
    expect(store.theme).toBe("dark");
  });
});
