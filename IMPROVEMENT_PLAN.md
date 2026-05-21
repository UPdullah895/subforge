# SubForge — Improvement Plan

A phased roadmap of code quality, workflow, and architecture improvements. Each phase builds on the previous one and ends with a clear done-when checklist. Implement phases in order.

---

## Status Overview

| # | Phase | Focus | Effort | Status |
|---|-------|-------|--------|--------|
| — | Critical bugs | 4 functional bug fixes | Done | ✅ |
| 1 | Workflow Foundation | git, ESLint, Prettier, .env | ~1 hr | ⬜ |
| 2 | Shared Job Queue | Composable + constants | ~2 hrs | ⬜ |
| 3 | Error Handling | Error refs + UI feedback | ~2 hrs | ⬜ |
| 4 | Testing | Vitest + regression tests | ~3 hrs | ⬜ |
| 5 | Architecture Hardening | WebSocket reliability, Zod validation | ~3 hrs | ⬜ |
| 6 | CI/CD | GitHub Actions build + release | ~1 hr | ⬜ |

---

## ✅ Critical Bugs — Already Fixed

These were fixed before this plan was written. Listed here for the record.

| Bug | File | Fix Applied |
|-----|------|-------------|
| "Clear completed" button did nothing | `src/components/JobMonitor.vue:29` | Lowercased `"Completed"` → `"completed"`, `"Failed"` → `"failed"` to match store union types. Same fix applied to icon `v-if` conditions. |
| Batch jobs mapped to wrong files on second run | `src/stores/batchStore.ts:80` | Replaced `files[jobs.value.length]` with `files[i]` via `forEach` — stable index regardless of prior job count. |
| WebSocket `apiListen` could hang forever | `src/api/events.ts:46` | Added `reject` path to the connecting-wait promise; added `onerror` handler to `ensureWs` that resets `ws = null`. |
| Theme preference not saved on toggle | `src/stores/appStore.ts:51` | Made `toggleTheme` async and awaited `saveConfig()`. |

---

## Phase 1 — Workflow Foundation

**Goal:** Make the project trackable and stylistically consistent before any further changes. Everything after this phase will be version-controlled and linted.

### Steps

**1. Initialize git**
```bash
cd subforge
git init
git add .
git commit -m "Initial commit"
```

**2. Install ESLint + Prettier**
```bash
npm install --save-dev \
  eslint \
  @typescript-eslint/eslint-plugin \
  @typescript-eslint/parser \
  eslint-plugin-vue \
  prettier \
  eslint-config-prettier \
  eslint-plugin-prettier
```

**3. Create `subforge/.eslintrc.json`**
```json
{
  "root": true,
  "env": { "browser": true, "es2022": true, "node": true },
  "extends": [
    "eslint:recommended",
    "plugin:@typescript-eslint/recommended",
    "plugin:vue/vue3-recommended",
    "prettier"
  ],
  "parser": "vue-eslint-parser",
  "parserOptions": {
    "parser": "@typescript-eslint/parser",
    "ecmaVersion": 2022,
    "sourceType": "module"
  },
  "rules": {
    "@typescript-eslint/no-explicit-any": "warn",
    "vue/multi-word-component-names": "off"
  }
}
```

**4. Create `subforge/.prettierrc.json`**
```json
{
  "semi": true,
  "singleQuote": false,
  "tabWidth": 2,
  "trailingComma": "es5",
  "printWidth": 100
}
```

**5. Add scripts to `subforge/package.json`**
```json
"scripts": {
  "lint": "eslint . --ext .ts,.vue",
  "lint:fix": "eslint . --ext .ts,.vue --fix",
  "format": "prettier --write \"src/**/*.{ts,vue,css}\""
}
```

**6. Create `subforge/.env.example`**
```
# Ollama local server (default: http://localhost:11434)
VITE_OLLAMA_HOST=http://localhost:11434

# LibreTranslate local server (default: http://localhost:5000)
VITE_LIBRE_HOST=http://localhost:5000

# WebSocket URL for server mode (default: derived from location.host)
# VITE_WS_URL=ws://localhost:3001
```

**7. Update `translateStore.ts` to read env vars**
```typescript
const ollamaHost = ref(import.meta.env.VITE_OLLAMA_HOST || "http://localhost:11434");
const libreHost = ref(import.meta.env.VITE_LIBRE_HOST || "http://localhost:5000");
```

### Done When
- [ ] `npm run lint` exits 0 with no errors
- [ ] `npm run format` produces no diff
- [ ] `git log` shows at least one commit
- [ ] `.env.example` is committed; `.env` is in `.gitignore`

---

## Phase 2 — Shared Job Queue Composable

**Goal:** Remove the 5× duplication of job tracking logic. A single bug fix or feature addition in the composable propagates everywhere automatically.

### What's Duplicated Today

`subtitleStore.ts`, `audioStore.ts`, `batchStore.ts`, `transcribeStore.ts`, and `translateStore.ts` each independently define:
- `jobs` ref
- `updateJobProgress(jobId, progress, message?)`
- `completeJob(jobId, status, message?)`
- `clearCompleted()`

### Steps

**1. Create `subforge/src/constants/jobStatus.ts`**
```typescript
export const JOB_STATUS = {
  PENDING: "pending",
  RUNNING: "running",
  COMPLETED: "completed",
  FAILED: "failed",
} as const;

export type JobStatus = (typeof JOB_STATUS)[keyof typeof JOB_STATUS];
```

**2. Create `subforge/src/composables/useJobQueue.ts`**
```typescript
import { ref } from "vue";
import { JOB_STATUS, type JobStatus } from "../constants/jobStatus";

export interface Job {
  id: string;
  status: JobStatus;
  input_file: string;
  progress: number;
  message?: string;
}

export function useJobQueue() {
  const jobs = ref<Job[]>([]);

  function addJob(id: string, input_file: string): void {
    jobs.value.push({ id, status: JOB_STATUS.PENDING, input_file, progress: 0 });
  }

  function updateJobProgress(jobId: string, progress: number, message?: string): void {
    const job = jobs.value.find((j) => j.id === jobId);
    if (job) {
      job.progress = progress;
      job.status = JOB_STATUS.RUNNING;
      if (message) job.message = message;
    }
  }

  function completeJob(jobId: string, status: JobStatus, message?: string): void {
    const job = jobs.value.find((j) => j.id === jobId);
    if (job) {
      job.status = status;
      job.progress = 100;
      if (message) job.message = message;
    }
  }

  function clearCompleted(): void {
    jobs.value = jobs.value.filter(
      (j) => j.status === JOB_STATUS.PENDING || j.status === JOB_STATUS.RUNNING
    );
  }

  return { jobs, addJob, updateJobProgress, completeJob, clearCompleted };
}
```

**3. Refactor each store** — replace the local `jobs` ref and job functions with the composable. Pattern (repeat for all 5 stores):
```typescript
// Remove these from the store:
// const jobs = ref<...[]>([]);
// function updateJobProgress(...) { ... }
// function completeJob(...) { ... }
// function clearCompleted() { ... }

// Add at the top of the store function:
const { jobs, addJob, updateJobProgress, completeJob, clearCompleted } = useJobQueue();
```

**4. Update `JobMonitor.vue` and all view files** — replace any remaining magic status string literals with `JOB_STATUS.COMPLETED`, `JOB_STATUS.FAILED`, etc.

### Done When
- [ ] `src/composables/useJobQueue.ts` exists
- [ ] `src/constants/jobStatus.ts` exists
- [ ] No store defines its own `jobs` ref or job-tracking functions
- [ ] No file contains the string `"completed"` or `"failed"` as a bare string literal (use `JOB_STATUS.COMPLETED` etc.)
- [ ] `npm run lint` still exits 0

---

## Phase 3 — Error Handling & User Feedback

**Goal:** Users see actionable error messages when operations fail. Silent no-ops are eliminated.

### Current State

Most `catch` blocks across all stores do one of:
- `console.error(e)` only — user sees nothing
- Set `ollamaModels.value = []` silently — user doesn't know why the list is empty
- `appStore.loadConfig()` throws uncaught if the backend is unavailable on startup

### Steps

**1. Add `error` ref to stores missing it**

`appStore`, `subtitleStore`, `audioStore`, `transcribeStore`, `translateStore` all need:
```typescript
const error = ref<string | null>(null);
```
Return it from the store. `batchStore` already does this correctly — use it as the reference pattern.

**2. Update every `catch` block** to set `error.value`:
```typescript
// Before
} catch (e) {
  console.error("Failed to detect Ollama:", e);
  ollamaModels.value = [];
}

// After
} catch (e) {
  error.value = `Failed to connect to Ollama: ${e instanceof Error ? e.message : String(e)}`;
  ollamaModels.value = [];
}
```

**3. Make `appStore.loadConfig` fail gracefully**
```typescript
async function loadConfig() {
  try {
    config.value = await apiInvoke<AppConfig>("load_config");
    theme.value = config.value.theme;
  } catch {
    config.value = { theme: "dark" };
    theme.value = "dark";
  }
}
```

**4. Add null guards to `audioStore` and `subtitleStore`**

At the top of every action that uses `selectedFile.value`:
```typescript
if (!selectedFile.value) {
  error.value = "No file selected.";
  return;
}
```

**5. Create `subforge/src/components/ErrorBanner.vue`**

A dismissible banner component that each view can include:
```vue
<script setup lang="ts">
const props = defineProps<{ message: string | null }>();
const emit = defineEmits<{ dismiss: [] }>();
</script>

<template>
  <div
    v-if="message"
    class="flex items-center gap-3 px-4 py-3 rounded-lg bg-red-900/20 border border-red-500/30 text-red-400 text-sm"
  >
    <span class="flex-1">{{ message }}</span>
    <button @click="emit('dismiss')" class="text-red-400 hover:text-red-300">✕</button>
  </div>
</template>
```

Include it at the top of each view, bound to the store's `error` ref.

### Done When
- [ ] Every store exports an `error` ref
- [ ] Every `catch` block sets `error.value` with a human-readable message
- [ ] App startup no longer throws if `load_config` fails
- [ ] `ErrorBanner.vue` appears in each view when an error is set
- [ ] Triggering an operation without a selected file shows "No file selected" in the UI

---

## Phase 4 — Testing

**Goal:** Automated regression coverage that prevents fixed bugs from coming back and validates store logic without manual testing.

### Steps

**1. Install Vitest**
```bash
npm install --save-dev vitest @vue/test-utils jsdom @vitest/ui
```

**2. Create `subforge/vitest.config.ts`**
```typescript
import { defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  plugins: [vue()],
  test: {
    environment: "jsdom",
    globals: true,
  },
});
```

**3. Add test script to `package.json`**
```json
"test": "vitest run",
"test:watch": "vitest"
```

**4. Write tests for `useJobQueue` composable** — `src/composables/useJobQueue.test.ts`

Cover:
- `addJob` creates a job with `PENDING` status and 0 progress
- `updateJobProgress` only updates the matching job ID
- `completeJob` sets status and progress = 100
- `clearCompleted` removes `completed` and `failed` jobs, keeps `pending` and `running` (regression for the capitalization bug)

**5. Write tests for `batchStore`** — regression for the index-drift bug

Verify that calling `batchExtract` twice maps jobs to the correct files both times.

**6. Write tests for `appStore`**

- `loadConfig` failure leaves a working default config
- `toggleTheme` alternates between `"dark"` and `"light"`

### Done When
- [ ] `npm test` runs and exits 0
- [ ] Tests cover all four previously-fixed bugs as regression cases
- [ ] Tests for `useJobQueue` cover all four functions

---

## Phase 5 — Architecture Hardening

**Goal:** Make the WebSocket path (used in server/Docker mode) as reliable as the Tauri IPC path; validate server responses at the boundary.

### Steps

**1. Add WebSocket reconnection to `events.ts`**

Replace the current `onclose` no-op with exponential backoff:
```typescript
let retryCount = 0;
const MAX_RETRIES = 5;

ws.onclose = () => {
  ws = null;
  if (retryCount < MAX_RETRIES) {
    const delay = Math.min(1000 * 2 ** retryCount, 30000);
    retryCount++;
    setTimeout(() => {
      ensureWs();
      // Re-register all existing listeners
      for (const [event, handlers] of wsListeners) {
        for (const handler of handlers) {
          wsListeners.get(event) // already in map, ws.onmessage will dispatch
        }
      }
    }, delay);
  }
};
```

Reset `retryCount = 0` in `ws.onopen`.

**2. Support `VITE_WS_URL` env var in `events.ts`**
```typescript
const wsUrl = import.meta.env.VITE_WS_URL ||
  `${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/ws`;
ws = new WebSocket(wsUrl);
```

**3. Install Zod**
```bash
npm install zod
```

**4. Create `subforge/src/api/schemas.ts`**

Define Zod schemas for every backend response shape that is currently cast with `as T`. Examples:
```typescript
import { z } from "zod";

export const OllamaInfoSchema = z.object({
  available: z.boolean(),
  models: z.array(z.string()),
});

export const ToolRegistrySchema = z.object({
  mkvmerge: z.object({ name: z.string(), available: z.boolean(), path: z.string().optional(), version: z.string().optional() }),
  // ... other tools
});
```

**5. Update `api/client.ts`** to validate through schemas in server (non-Tauri) mode:
```typescript
// Only in non-Tauri path — Tauri IPC is trusted
export async function apiInvoke<T>(command: string, args?: Record<string, unknown>, schema?: ZodSchema<T>): Promise<T> {
  // ... existing fetch call
  const data = await res.json();
  return schema ? schema.parse(data) : data as T;
}
```

**6. Mark API keys as session-only in `translateStore.ts`**

Add a comment block above the key refs:
```typescript
// Session-only credentials — never include in saveConfig().
// In server mode (Phase 5), these must be handled backend-side only.
const deeplKey = ref("");
const openaiKey = ref("");
```

### Done When
- [ ] Killing the dev WebSocket server and restarting it reconnects automatically
- [ ] `VITE_WS_URL` overrides the default WebSocket URL
- [ ] Zod schemas exist for all backend response types
- [ ] Malformed server responses throw a `ZodError` instead of silently mistyping

---

## Phase 6 — CI/CD

**Goal:** Every push is automatically type-checked, linted, and tested. Tagged releases produce distributable packages.

### Steps

**1. Create `.github/workflows/ci.yml`**
```yaml
name: CI

on:
  push:
    branches: ["**"]
  pull_request:
    branches: ["**"]

jobs:
  frontend:
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: subforge
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: npm
          cache-dependency-path: subforge/package-lock.json
      - run: npm ci
      - run: npm run lint
      - run: npx vue-tsc --noEmit
      - run: npm test

  rust:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo check
        working-directory: subforge/src-tauri
      - run: cargo check
        working-directory: subforge/server
```

**2. Create `.github/workflows/release.yml`**
```yaml
name: Release

on:
  push:
    tags: ["v*"]

jobs:
  build:
    strategy:
      matrix:
        include:
          - os: ubuntu-latest
            target: linux
          - os: windows-latest
            target: windows
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 20
      - uses: dtolnay/rust-toolchain@stable
      - uses: tauri-apps/tauri-action@v0
        with:
          projectPath: subforge
          tagName: ${{ github.ref_name }}
          releaseName: SubForge ${{ github.ref_name }}
          releaseBody: "See CHANGELOG for details."
```

**3. Add build badge to `README.md`**
```markdown
[![CI](https://github.com/<your-username>/subforge/actions/workflows/ci.yml/badge.svg)](https://github.com/<your-username>/subforge/actions/workflows/ci.yml)
```

### Done When
- [ ] A push to any branch triggers the CI workflow
- [ ] CI jobs for frontend (lint, typecheck, test) and Rust (`cargo check`) all pass
- [ ] Pushing a `v0.1.0` tag triggers the release workflow and produces GitHub Release artifacts

---

*This document is the implementation companion to `SubForge_Project_Plan.md`. Update the Status column above as phases are completed.*
