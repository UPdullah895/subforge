import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import {
  useUpdateStore,
  compareVersions,
  isNewer,
  parseVersion,
  LATEST_RELEASE_API,
} from "./updateStore";

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

function release(tag: string, extra: Record<string, unknown> = {}): Response {
  return {
    ok: true,
    status: 200,
    json: async () => ({
      tag_name: tag,
      html_url: `https://github.com/UPdullah895/subforge/releases/tag/${tag}`,
      ...extra,
    }),
  } as unknown as Response;
}

describe("version comparison", () => {
  it("parses a leading v, a pre-release suffix and build metadata", () => {
    expect(parseVersion("v1.2.3")).toEqual({ core: [1, 2, 3], pre: [] });
    expect(parseVersion("1.2.3-beta.2")).toEqual({ core: [1, 2, 3], pre: ["beta", "2"] });
    expect(parseVersion("1.2.3+build.5")).toEqual({ core: [1, 2, 3], pre: [] });
    expect(parseVersion("not-a-version")).toBeNull();
  });

  it("orders releases the way semver does", () => {
    expect(compareVersions("0.4.0", "0.3.0")).toBe(1);
    expect(compareVersions("v0.3.0", "0.3.0")).toBe(0);
    expect(compareVersions("0.3.0", "0.10.0")).toBe(-1);
    expect(compareVersions("1.0", "1.0.0")).toBe(0);
    expect(compareVersions("1.0.1", "1.0")).toBe(1);
    // A pre-release is older than the release it leads to.
    expect(compareVersions("0.4.0-beta.1", "0.4.0")).toBe(-1);
    expect(compareVersions("0.4.0-beta.10", "0.4.0-beta.2")).toBe(1);
    expect(compareVersions("0.4.0-alpha", "0.4.0-beta")).toBe(-1);
  });

  it("never claims an update for a version it cannot parse", () => {
    expect(isNewer("garbage", "0.3.0")).toBe(false);
    expect(isNewer("0.3.1", "0.3.0")).toBe(true);
  });
});

describe("updateStore", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.stubGlobal("localStorage", memoryStorage());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("offers a newer release from GitHub", async () => {
    const fetchMock = vi.fn().mockResolvedValue(release("v9.9.9"));
    vi.stubGlobal("fetch", fetchMock);

    const store = useUpdateStore();
    store.currentVersion = "0.3.0";
    await store.check(true);

    expect(fetchMock).toHaveBeenCalledWith(LATEST_RELEASE_API, {
      headers: { Accept: "application/vnd.github+json" },
    });
    expect(store.available).toEqual({
      version: "9.9.9",
      url: "https://github.com/UPdullah895/subforge/releases/tag/v9.9.9",
    });
    expect(store.lastError).toBeNull();
  });

  it("stays quiet on the current version and on older ones", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(release("v0.3.0")));
    const store = useUpdateStore();
    store.currentVersion = "0.3.0";
    await store.check(true);
    expect(store.available).toBeNull();

    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(release("v0.2.9")));
    await store.check(true);
    expect(store.available).toBeNull();
  });

  it("ignores drafts and pre-releases", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(release("v9.9.9", { prerelease: true })));
    const store = useUpdateStore();
    store.currentVersion = "0.3.0";
    await store.check(true);
    expect(store.available).toBeNull();
  });

  it("dismiss keeps the banner away for that version only", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(release("v9.9.9")));
    const store = useUpdateStore();
    store.currentVersion = "0.3.0";
    await store.check(true);
    store.dismiss();
    expect(store.available).toBeNull();

    // Same version again: still hidden.
    await store.check(true);
    expect(store.available).toBeNull();

    // A newer one shows up again.
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(release("v9.9.10")));
    await store.check(true);
    expect(store.available?.version).toBe("9.9.10");
  });

  it("throttles the check to once a day", async () => {
    const fetchMock = vi.fn().mockResolvedValue(release("v0.3.0"));
    vi.stubGlobal("fetch", fetchMock);

    const store = useUpdateStore();
    store.currentVersion = "0.3.0";
    await store.check(true);
    await store.check();
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("never throws when GitHub cannot be reached", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));
    const store = useUpdateStore();
    store.currentVersion = "0.3.0";
    await expect(store.check(true)).resolves.toBeUndefined();
    expect(store.available).toBeNull();
    expect(store.lastError).toBe("offline");
    expect(store.checking).toBe(false);
  });

  it("treats a non-200 answer as a failure, not as an update", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: false, status: 403, json: async () => ({}) } as unknown as Response)
    );
    const store = useUpdateStore();
    store.currentVersion = "0.3.0";
    await store.check(true);
    expect(store.available).toBeNull();
    expect(store.lastError).toContain("403");
  });
});
