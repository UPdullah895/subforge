import { defineStore } from "pinia";
import { ref } from "vue";

export type ToastKind = "success" | "error" | "info";

export interface Toast {
  id: number;
  kind: ToastKind;
  message: string;
  detail?: string;
  /** Raw tool output, shown in a collapsed "Details" section. */
  details?: string;
}

const MAX_TOASTS = 4;
// Errors stay longer: people need time to read (and copy) them.
const DURATION_MS: Record<ToastKind, number> = { success: 5000, info: 5000, error: 12000 };

export const useToastStore = defineStore("toast", () => {
  const toasts = ref<Toast[]>([]);
  const timers = new Map<number, ReturnType<typeof setTimeout>>();
  let nextId = 1;

  function dismiss(id: number) {
    const timer = timers.get(id);
    if (timer) clearTimeout(timer);
    timers.delete(id);
    toasts.value = toasts.value.filter((t) => t.id !== id);
  }

  function push(kind: ToastKind, message: string, detail?: string, details?: string): number {
    const id = nextId++;
    toasts.value.push({ id, kind, message, detail, details: details || undefined });
    while (toasts.value.length > MAX_TOASTS) dismiss(toasts.value[0].id);
    timers.set(
      id,
      setTimeout(() => dismiss(id), DURATION_MS[kind])
    );
    return id;
  }

  return {
    toasts,
    push,
    dismiss,
    success: (message: string, detail?: string) => push("success", message, detail),
    error: (message: string, detail?: string, details?: string | null) =>
      push("error", message, detail, details ?? undefined),
    info: (message: string, detail?: string) => push("info", message, detail),
  };
});
