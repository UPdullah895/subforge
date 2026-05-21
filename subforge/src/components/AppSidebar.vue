<script setup lang="ts">
import { useAppStore } from "../stores/appStore";
import {
  Subtitles,
  Music,
  Mic,
  Languages,
  Settings,
  PanelLeftClose,
  PanelLeftOpen,
  FolderSync,
} from "lucide-vue-next";

const props = defineProps<{
  current: string;
}>();

const emit = defineEmits<{
  (e: "navigate", view: "subtitle" | "audio" | "batch" | "transcribe" | "translate"): void;
}>();

const app = useAppStore();

const navItems = [
  { id: "subtitle" as const, label: "Subtitles", icon: Subtitles, active: true },
  { id: "audio" as const, label: "Audio", icon: Music, active: true },
  { id: "batch" as const, label: "Batch", icon: FolderSync, active: true },
  { id: "transcribe" as const, label: "Speech to Text", icon: Mic, active: true },
  { id: "translate" as const, label: "Translate", icon: Languages, active: true },
];
</script>

<template>
  <aside
    :class="[
      'flex flex-col bg-bg-secondary border-r border-border transition-all duration-300',
      app.sidebarCollapsed ? 'w-16' : 'w-60',
    ]"
  >
    <!-- Header -->
    <div class="flex items-center h-14 px-4 border-b border-border">
      <div class="flex items-center gap-3 overflow-hidden">
        <div class="w-8 h-8 rounded-lg bg-accent flex items-center justify-center shrink-0">
          <Subtitles class="w-5 h-5 text-white" />
        </div>
        <span
          :class="[
            'font-bold text-lg text-text-primary whitespace-nowrap transition-opacity',
            app.sidebarCollapsed ? 'opacity-0 w-0' : 'opacity-100',
          ]"
        >
          SubForge
        </span>
      </div>
    </div>

    <!-- Nav -->
    <nav class="flex-1 py-3 px-2 space-y-1">
      <button
        v-for="item in navItems"
        :key="item.id"
        :class="[
          'flex items-center gap-3 w-full px-3 py-2.5 rounded-lg text-sm font-medium transition-colors',
          props.current === item.id
            ? 'bg-accent/10 text-accent'
            : item.active
              ? 'text-text-secondary hover:bg-bg-hover hover:text-text-primary'
              : 'text-text-muted cursor-not-allowed opacity-60',
        ]"
        :title="app.sidebarCollapsed ? item.label : undefined"
        @click="item.active ? emit('navigate', item.id) : undefined"
      >
        <component :is="item.icon" class="w-5 h-5 shrink-0" />
        <span
          :class="[
            'whitespace-nowrap transition-opacity',
            app.sidebarCollapsed ? 'opacity-0 w-0' : 'opacity-100',
          ]"
        >
          {{ item.label }}
        </span>
        <span
          v-if="!item.active && !app.sidebarCollapsed"
          class="ml-auto text-xs px-1.5 py-0.5 rounded bg-bg-tertiary text-text-muted"
        >
          Soon
        </span>
      </button>
    </nav>

    <!-- Footer -->
    <div class="p-2 border-t border-border space-y-1">
      <button
        class="flex items-center gap-3 w-full px-3 py-2.5 rounded-lg text-sm font-medium text-text-secondary hover:bg-bg-hover hover:text-text-primary transition-colors"
        :title="app.sidebarCollapsed ? 'Expand' : 'Collapse'"
        @click="app.toggleSidebar"
      >
        <PanelLeftClose v-if="!app.sidebarCollapsed" class="w-5 h-5 shrink-0" />
        <PanelLeftOpen v-else class="w-5 h-5 shrink-0" />
        <span
          :class="[
            'whitespace-nowrap transition-opacity',
            app.sidebarCollapsed ? 'opacity-0 w-0' : 'opacity-100',
          ]"
        >
          Collapse
        </span>
      </button>
      <button
        class="flex items-center gap-3 w-full px-3 py-2.5 rounded-lg text-sm font-medium text-text-secondary hover:bg-bg-hover hover:text-text-primary transition-colors"
        :title="app.sidebarCollapsed ? 'Settings' : undefined"
      >
        <Settings class="w-5 h-5 shrink-0" />
        <span
          :class="[
            'whitespace-nowrap transition-opacity',
            app.sidebarCollapsed ? 'opacity-0 w-0' : 'opacity-100',
          ]"
        >
          Settings
        </span>
      </button>
    </div>
  </aside>
</template>
