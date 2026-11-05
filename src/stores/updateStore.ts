import { defineStore } from "pinia";

// SubForge has no auto-updater: nothing tells a user that a newer release exists,
// so this is the smallest useful thing — ask GitHub once a day and offer a link.
// It never downloads or installs anything; the About/Linux install steps in the
// README stay the source of truth for how to update.
export const LATEST_RELEASE_API =
  "https://api.github.com/repos/UPdullah895/subforge/releases/latest";
export const RELEASES_PAGE = "https://github.com/UPdullah895/subforge/releases/latest";

const STORAGE_KEY = "subforge.update";
const CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;

export interface AvailableUpdate {
  version: string;
  url: string;
}

interface StoredState {
  lastCheck: number;
  dismissed: string | null;
}

/**
 * localStorage is absent in a sandboxed webview or a non-DOM runtime, and that is not an
 * error. Prefer `window.localStorage`: under Node (tests) `globalThis.localStorage` can be a
 * stub that shadows the DOM one.
 */
function storage(): Storage | null {
  // `window` first: Node also defines a `localStorage` global that is unavailable unless
  // started with --localstorage-file, and touching it warns on stderr.
  try {
    const fromWindow = (globalThis as { window?: Window }).window?.localStorage;
    if (fromWindow) return fromWindow;
  } catch {
    // Sandboxed webview: fall back to the bare global.
  }
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

function readState(): StoredState {
  try {
    const raw = storage()?.getItem(STORAGE_KEY) ?? null;
    if (!raw) return { lastCheck: 0, dismissed: null };
    const parsed = JSON.parse(raw) as Partial<StoredState>;
    return {
      lastCheck: typeof parsed.lastCheck === "number" ? parsed.lastCheck : 0,
      dismissed: typeof parsed.dismissed === "string" ? parsed.dismissed : null,
    };
  } catch {
    return { lastCheck: 0, dismissed: null };
  }
}

function writeState(state: StoredState): void {
  try {
    storage()?.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    // Private mode or a sandboxed webview: the throttling is a nicety, not a requirement.
  }
}

interface ParsedVersion {
  core: number[];
  pre: string[];
}

/** Parses `v1.2.3`, `1.2.3-beta.1`, `1.2` — returns null when it is not a version at all. */
export function parseVersion(value: string): ParsedVersion | null {
  const trimmed = value.trim().replace(/^v/i, "").split("+")[0];
  if (!trimmed) return null;
  const dash = trimmed.indexOf("-");
  const corePart = dash === -1 ? trimmed : trimmed.slice(0, dash);
  const prePart = dash === -1 ? "" : trimmed.slice(dash + 1);
  const core = corePart.split(".").map((part) => Number(part));
  if (!core.length || core.some((n) => !Number.isInteger(n) || n < 0)) return null;
  return { core, pre: prePart ? prePart.split(".") : [] };
}

function comparePreRelease(a: string[], b: string[]): number {
  // A release without a pre-release suffix is newer than any pre-release of the same core.
  if (!a.length && !b.length) return 0;
  if (!a.length) return 1;
  if (!b.length) return -1;
  const len = Math.max(a.length, b.length);
  for (let i = 0; i < len; i += 1) {
    if (a[i] === undefined) return -1;
    if (b[i] === undefined) return 1;
    const aNum = /^\d+$/.test(a[i]);
    const bNum = /^\d+$/.test(b[i]);
    if (aNum && bNum) {
      const diff = Number(a[i]) - Number(b[i]);
      if (diff !== 0) return diff < 0 ? -1 : 1;
    } else if (aNum !== bNum) {
      // Numeric identifiers are older than alphanumeric ones (semver §11).
      return aNum ? -1 : 1;
    } else if (a[i] !== b[i]) {
      return a[i] < b[i] ? -1 : 1;
    }
  }
  return 0;
}

/** 1 when `a` is newer than `b`, -1 when older, 0 when equal (unparsable values compare as 0). */
export function compareVersions(a: string, b: string): number {
  const left = parseVersion(a);
  const right = parseVersion(b);
  if (!left || !right) return 0;
  const len = Math.max(left.core.length, right.core.length);
  for (let i = 0; i < len; i += 1) {
    const diff = (left.core[i] ?? 0) - (right.core[i] ?? 0);
    if (diff !== 0) return diff < 0 ? -1 : 1;
  }
  return comparePreRelease(left.pre, right.pre);
}

export function isNewer(candidate: string, current: string): boolean {
  return compareVersions(candidate, current) > 0;
}

function currentAppVersion(): string {
  // Injected by Vite (see vite.config.ts); the fallback keeps a plain `vite preview` working.
  return typeof __APP_VERSION__ === "string" ? __APP_VERSION__ : "0.0.0";
}

export const useUpdateStore = defineStore("update", {
  state: () => ({
    currentVersion: currentAppVersion(),
    checking: false,
    available: null as AvailableUpdate | null,
    lastError: null as string | null,
  }),

  getters: {
    /** True when a newer release is known and the user has not dismissed it. */
    hasUpdate: (state) => state.available !== null,
  },

  actions: {
    /**
     * Asks GitHub for the newest release. Throttled to once a day; `force` is for a
     * manual "check now" and for tests. Every failure is silent by design: a desktop
     * app that cannot reach GitHub must not nag or block.
     */
    async check(force = false): Promise<void> {
      if (this.checking) return;
      const stored = readState();
      if (!force && stored.lastCheck && Date.now() - stored.lastCheck < CHECK_INTERVAL_MS) return;

      this.checking = true;
      try {
        const response = await fetch(LATEST_RELEASE_API, {
          headers: { Accept: "application/vnd.github+json" },
        });
        writeState({ ...stored, lastCheck: Date.now() });
        if (!response.ok) throw new Error(`GitHub API returned ${response.status}`);
        const release = (await response.json()) as {
          tag_name?: string;
          html_url?: string;
          draft?: boolean;
          prerelease?: boolean;
        };
        const tag = (release.tag_name ?? "").trim();
        if (!tag || release.draft || release.prerelease) return;
        if (!isNewer(tag, this.currentVersion)) {
          this.available = null;
          this.lastError = null;
          return;
        }
        const version = tag.replace(/^v/i, "");
        if (stored.dismissed === version) return;
        this.available = { version, url: release.html_url || RELEASES_PAGE };
        this.lastError = null;
      } catch (error) {
        this.lastError = error instanceof Error ? error.message : String(error);
      } finally {
        this.checking = false;
      }
    },

    /** Hides the banner for this version; the next release shows it again. */
    dismiss(): void {
      const stored = readState();
      writeState({ ...stored, dismissed: this.available?.version ?? stored.dismissed });
      this.available = null;
    },
  },
});
