<script setup lang="ts">
import { isTauri } from "../api/client";

// decorations: false means the WM draws no frame, so it also gives us no
// edge/corner resize grips — Tauri requires the app to trigger resizing
// itself via startResizeDragging() from its own hit-test regions.
const isDesktop = isTauri();

type Direction = "North" | "South" | "East" | "West" | "NorthEast" | "NorthWest" | "SouthEast" | "SouthWest";

async function startResize(direction: Direction) {
  if (!isDesktop) return;
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().startResizeDragging(direction);
  } catch {
    // no-op outside Tauri
  }
}
</script>

<template>
  <template v-if="isDesktop">
    <div class="resize-handle resize-n" @mousedown="startResize('North')" />
    <div class="resize-handle resize-s" @mousedown="startResize('South')" />
    <div class="resize-handle resize-e" @mousedown="startResize('East')" />
    <div class="resize-handle resize-w" @mousedown="startResize('West')" />
    <div class="resize-handle resize-ne" @mousedown="startResize('NorthEast')" />
    <div class="resize-handle resize-nw" @mousedown="startResize('NorthWest')" />
    <div class="resize-handle resize-se" @mousedown="startResize('SouthEast')" />
    <div class="resize-handle resize-sw" @mousedown="startResize('SouthWest')" />
  </template>
</template>

<style scoped>
.resize-handle {
  position: fixed;
  z-index: 100;
}

.resize-n,
.resize-s {
  left: 6px;
  right: 6px;
  height: 4px;
  cursor: ns-resize;
}
.resize-n {
  top: 0;
}
.resize-s {
  bottom: 0;
}

.resize-e,
.resize-w {
  top: 6px;
  bottom: 6px;
  width: 4px;
  cursor: ew-resize;
}
.resize-e {
  right: 0;
}
.resize-w {
  left: 0;
}

.resize-ne,
.resize-nw,
.resize-se,
.resize-sw {
  width: 8px;
  height: 8px;
}
.resize-ne {
  top: 0;
  right: 0;
  cursor: nesw-resize;
}
.resize-nw {
  top: 0;
  left: 0;
  cursor: nwse-resize;
}
.resize-se {
  bottom: 0;
  right: 0;
  cursor: nwse-resize;
}
.resize-sw {
  bottom: 0;
  left: 0;
  cursor: nesw-resize;
}
</style>
