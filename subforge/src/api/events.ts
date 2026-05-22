import { listen } from "@tauri-apps/api/event";

const isTauri =
  typeof window !== "undefined" &&
  !!(window as unknown as Record<string, unknown>).__TAURI__;

let ws: WebSocket | null = null;
let retryCount = 0;
const MAX_RETRIES = 5;
const wsListeners: Map<string, ((payload: unknown) => void)[]> = new Map();

function ensureWs(): void {
  if (ws && (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING)) return;

  const wsUrl =
    (import.meta.env.VITE_WS_URL as string | undefined) ||
    `${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/ws`;
  ws = new WebSocket(wsUrl);

  ws.onopen = () => {
    retryCount = 0;
  };

  ws.onmessage = (event) => {
    try {
      const data = JSON.parse(event.data as string);
      const eventType = data.type as string;
      const payload = data.payload;
      wsListeners.get(eventType)?.forEach((h) => h(payload));
    } catch {
      // ignore malformed messages
    }
  };

  ws.onerror = () => {
    ws = null;
  };

  ws.onclose = () => {
    ws = null;
    if (retryCount < MAX_RETRIES) {
      const delay = Math.min(1000 * 2 ** retryCount, 30000);
      retryCount++;
      setTimeout(ensureWs, delay);
    }
  };
}

export async function apiListen<T>(
  event: string,
  handler: (payload: T) => void
): Promise<() => void> {
  if (isTauri) {
    return listen(event, (e) => handler(e.payload as T));
  }

  ensureWs();

  if (ws!.readyState === WebSocket.CONNECTING) {
    await new Promise<void>((resolve, reject) => {
      ws!.addEventListener("open", () => resolve(), { once: true });
      ws!.addEventListener("error", () => reject(new Error("WebSocket connection failed")), {
        once: true,
      });
    });
  }

  if (!wsListeners.has(event)) {
    wsListeners.set(event, []);
  }
  wsListeners.get(event)!.push(handler as (payload: unknown) => void);

  return () => {
    const handlers = wsListeners.get(event);
    if (handlers) {
      const idx = handlers.indexOf(handler as (payload: unknown) => void);
      if (idx !== -1) handlers.splice(idx, 1);
    }
  };
}
