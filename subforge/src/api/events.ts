import { listen } from "@tauri-apps/api/event";

const isTauri =
  typeof window !== "undefined" &&
  !!(window as unknown as Record<string, unknown>).__TAURI__;

let ws: WebSocket | null = null;
const wsListeners: Map<string, ((payload: unknown) => void)[]> = new Map();

function ensureWs(): WebSocket {
  if (!ws || ws.readyState !== WebSocket.OPEN) {
    const protocol = location.protocol === "https:" ? "wss:" : "ws:";
    ws = new WebSocket(`${protocol}//${location.host}/ws`);

    ws.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data);
        const eventType = data.type as string;
        const payload = data.payload;
        const handlers = wsListeners.get(eventType);
        if (handlers) {
          handlers.forEach((h) => h(payload));
        }
      } catch {
        // ignore malformed messages
      }
    };

    ws.onerror = () => {
      ws = null;
    };

    ws.onclose = () => {
      ws = null;
    };
  }
  return ws;
}

export async function apiListen<T>(
  event: string,
  handler: (payload: T) => void
): Promise<() => void> {
  if (isTauri) {
    return listen(event, (e) => handler(e.payload as T));
  }

  const ws = ensureWs();
  if (ws.readyState === WebSocket.CONNECTING) {
    await new Promise<void>((resolve, reject) => {
      ws.onopen = () => resolve();
      ws.onerror = () => reject(new Error("WebSocket connection failed"));
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
