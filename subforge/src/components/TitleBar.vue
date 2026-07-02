<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { isTauri } from "../api/client";
import { Minus, Square, Copy, X } from "lucide-vue-next";

// This same frontend bundle is served plain by the Axum server in a browser
// tab (no window to control) as well as embedded in the Tauri desktop shell
// — the titlebar (and every Tauri window-control call) only makes sense in
// the latter, so the whole component is gated on this single check.
const isDesktop = isTauri();

const isMaximized = ref(false);
let unlistenResize: (() => void) | null = null;

// `getCurrentWindow()` touches Tauri's window API — only ever call it behind
// `isDesktop`, and always through try/catch, so this component can't throw
// if it's ever evaluated somewhere the Tauri internals aren't present.
async function getWindow() {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  return getCurrentWindow();
}

async function refreshMaximizedState() {
  if (!isDesktop) return;
  try {
    const win = await getWindow();
    isMaximized.value = await win.isMaximized();
  } catch {
    // Not running under Tauri (or the call failed) — leave state as-is.
  }
}

async function minimize() {
  if (!isDesktop) return;
  try {
    const win = await getWindow();
    await win.minimize();
  } catch {
    // no-op outside Tauri
  }
}

async function toggleMaximize() {
  if (!isDesktop) return;
  try {
    const win = await getWindow();
    await win.toggleMaximize();
    isMaximized.value = await win.isMaximized();
  } catch {
    // no-op outside Tauri
  }
}

async function closeWindow() {
  if (!isDesktop) return;
  try {
    const win = await getWindow();
    await win.close();
  } catch {
    // no-op outside Tauri
  }
}

onMounted(async () => {
  if (!isDesktop) return;
  await refreshMaximizedState();
  try {
    const win = await getWindow();
    const unlisten = await win.onResized(() => {
      refreshMaximizedState();
    });
    unlistenResize = unlisten;
  } catch {
    // no-op outside Tauri
  }
});

onUnmounted(() => {
  if (unlistenResize) unlistenResize();
});
</script>

<template>
  <div
    v-if="isDesktop"
    data-tauri-drag-region
    class="flex items-center justify-between h-8 bg-bg-secondary border-b border-border select-none shrink-0"
    @dblclick="toggleMaximize"
  >
    <div data-tauri-drag-region class="flex-1 h-full flex items-center px-3">
      <span class="text-xs font-medium text-text-secondary">SubForge</span>
    </div>

    <div class="flex items-stretch h-full shrink-0">
      <button
        class="w-11 flex items-center justify-center text-text-secondary hover:bg-bg-hover transition-colors"
        :aria-label="'Minimize'"
        @click="minimize"
      >
        <Minus class="w-3.5 h-3.5" />
      </button>
      <button
        class="w-11 flex items-center justify-center text-text-secondary hover:bg-bg-hover transition-colors"
        :aria-label="isMaximized ? 'Restore' : 'Maximize'"
        @click="toggleMaximize"
      >
        <Copy v-if="isMaximized" class="w-3 h-3" />
        <Square v-else class="w-3 h-3" />
      </button>
      <button
        class="w-11 flex items-center justify-center text-text-secondary hover:bg-error hover:text-white transition-colors"
        :aria-label="'Close'"
        @click="closeWindow"
      >
        <X class="w-4 h-4" />
      </button>
    </div>
  </div>
</template>
