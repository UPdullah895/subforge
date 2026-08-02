# OpenTool / SubForge — Cross-Platform & Design Analysis

## Cross-Platform Status

SubForge is **already cross-platform by design**:
- **Tauri 2** wraps the OS-native WebView (WebKitGTK on Linux, WebView2 on Windows, WKWebView on macOS)
- **Rust backend** compiles natively on all three platforms
- **Vue 3 frontend** is browser-standard HTML/CSS/JS — runs anywhere the WebView runs

No platform-specific code was found in the frontend or backend. The project is ready for Windows, macOS, and Linux.

## Design Assessment

**Verdict: Good foundation with room for improvement.**

### What's good

1. **Clean layered architecture** — Rust backend (commands/tools/jobs) → Tauri IPC → Vue 3 frontend (stores/composables/components). Clear separation of concerns.

2. **Proper job queue composable** — `useJobQueue.ts` provides reusable progress/job tracking across all stores, avoiding the duplication seen in video-tools.

3. **TypeScript + Zod** — Type safety throughout the frontend, with runtime validation schemas for API responses.

4. **Tailwind CSS v4** — Modern, responsive, maintainable styling. Dark theme built-in.

5. **Pinia stores** — Centralized state management with proper patterns (actions, getters, async operations).

6. **Testing infrastructure** — Vitest configured, tests written for composables and stores, CI pipeline (GitHub Actions).

7. **Zod validation** — `zod` schemas for backend responses prevent silent type mismatches (see `IMPROVEMENT_PLAN.md` Phase 5).

### What could be improved

1. **Job queue still duplicated across stores** — The IMPROVEMENT_PLAN.md Phase 2 correctly identifies this. `subtitleStore.ts`, `audioStore.ts`, `batchStore.ts`, `transcribeStore.ts`, and `translateStore.ts` each independently define their own `jobs` ref and job functions. The `useJobQueue` composable exists but hasn't been retrofitted into all stores yet.

2. **Error handling incomplete** — Phase 3 of the improvement plan notes that most `catch` blocks only call `console.error(e)`. Users don't see actionable error messages. The `ErrorBanner.vue` component is planned but not yet integrated.

3. **WebSocket reliability** — The WebSocket path (used in server mode) lacks reconnection with exponential backoff (planned in Phase 5).

4. **Tool discovery** — Currently checks `$PATH` for `mkvmerge`/`ffmpeg` at startup. No settings UI for custom paths (e.g., Windows users who install tools to non-standard locations).

5. **Docker mode not yet implemented** — Phase 5 (REST + WebSocket API, docker-compose) is planned but not started.

## Summary & Recommendation

| Aspect | video-tools | SubForge |
|--------|------------|----------|
| Cross-platform | No (GTK3/Linux) | Yes (Tauri 2) |
| Architecture | Flat Python files | Rust + Vue 3 clean layers |
| Job queue | Raw threads | Composable (wip) |
| Tests | None | Vitest + CI |
| i18n | Arabic hardcoded | English, i18n-ready |
| Server mode | None | Planned (Docker) |
| UI | GTK3 Notebooks | Vue 3 + Tailwind responsive |

**Continue with SubForge.** It already has 4 phases complete, a robust cross-platform foundation, and a clear improvement plan addressing its remaining design gaps. Starting over would waste the significant work already done.
