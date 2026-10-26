import { onBeforeUnmount, onMounted, ref } from "vue";
import { isTauri } from "../api/client";

export interface FileDropOptions {
  /**
   * Extensions to accept (lowercase, no dot). A path with no extension is
   * passed through too: it may be a folder, and only the backend can tell
   * (`utils/paths.ts`). Empty/omitted accepts anything.
   */
  extensions?: string[];
  onDrop: (paths: string[]) => void;
}

/**
 * Window-wide file drop for the desktop app.
 *
 * Tauri intercepts native drops before the web page sees them, so this must
 * use `onDragDropEvent` rather than HTML5 `drop` events. In browser/server
 * mode there are no client-side paths to hand to the backend, so it does
 * nothing and the Browse buttons stay the way in.
 */
export function useFileDrop(options: FileDropOptions) {
  const dragging = ref(false);
  let unlisten: (() => void) | null = null;
  let stopped = false;

  function accepts(path: string): boolean {
    const exts = options.extensions;
    if (!exts || exts.length === 0) return true;
    const match = /\.([^./\\]+)$/.exec(path);
    if (!match) return true; // a folder
    return exts.includes(match[1].toLowerCase());
  }

  onMounted(async () => {
    if (!isTauri()) return;
    try {
      const { getCurrentWebview } = await import("@tauri-apps/api/webview");
      const stop = await getCurrentWebview().onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "enter" || payload.type === "over") {
          dragging.value = true;
        } else if (payload.type === "leave") {
          dragging.value = false;
        } else if (payload.type === "drop") {
          dragging.value = false;
          const paths = payload.paths.filter(accepts);
          if (paths.length > 0) options.onDrop(paths);
        }
      });
      // The view may have been unmounted while the listener was being set up.
      if (stopped) stop();
      else unlisten = stop;
    } catch {
      // Not a Tauri webview, or the API is unavailable: drag & drop is simply
      // not offered, and every view still has a Browse button.
    }
  });

  onBeforeUnmount(() => {
    stopped = true;
    unlisten?.();
    unlisten = null;
    dragging.value = false;
  });

  return { dragging, supported: isTauri() };
}
