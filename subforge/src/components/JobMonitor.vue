<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { apiListen } from "../api/events";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { Loader2, CheckCircle2, XCircle } from "lucide-vue-next";

interface JobEvent {
  job_id: string;
  status?: string;
  message?: string;
  progress?: number;
}

const jobs = ref<Map<string, JobEvent>>(new Map());
let unlisten: UnlistenFn | null = null;

onMounted(async () => {
  unlisten = await apiListen<JobEvent>("job-completed", (payload) => {
    jobs.value.set(payload.job_id, payload);
  });
});

onUnmounted(() => {
  if (unlisten) unlisten();
});

function clearCompleted() {
  for (const [id, job] of jobs.value) {
    if (job.status === "completed" || job.status === "failed") {
      jobs.value.delete(id);
    }
  }
}
</script>

<template>
  <div v-if="jobs.size > 0" class="space-y-2">
    <div class="flex items-center justify-between">
      <h3 class="text-sm font-semibold text-text-primary uppercase tracking-wide">
        Background Jobs
      </h3>
      <button
        @click="clearCompleted"
        class="text-xs text-text-muted hover:text-text-secondary transition-colors"
      >
        Clear completed
      </button>
    </div>
    <div class="space-y-2 max-h-64 overflow-y-auto">
      <div
        v-for="[id, job] in jobs"
        :key="id"
        class="flex items-center gap-3 px-4 py-3 rounded-lg bg-bg-secondary border border-border"
      >
        <CheckCircle2
          v-if="job.status === 'completed'"
          class="w-4 h-4 text-success shrink-0"
        />
        <XCircle
          v-else-if="job.status === 'failed'"
          class="w-4 h-4 text-error shrink-0"
        />
        <Loader2 v-else class="w-4 h-4 text-accent animate-spin shrink-0" />
        <div class="flex-1 min-w-0">
          <p class="text-xs text-text-muted font-mono truncate">{{ id.slice(0, 24) }}…</p>
          <p v-if="job.message" class="text-xs text-text-secondary truncate">
            {{ job.message }}
          </p>
        </div>
      </div>
    </div>
  </div>
</template>
