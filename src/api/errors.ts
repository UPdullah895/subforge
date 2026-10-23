/**
 * An error from a backend call. `message` is the human-readable sentence;
 * `details` is the raw tool output (stderr etc.) for a "Details" expander.
 */
export class ApiError extends Error {
  readonly details?: string;

  constructor(message: string, details?: string | null) {
    super(message);
    this.name = "ApiError";
    if (details) this.details = details;
  }
}

/**
 * Normalises what a Tauri command rejects with. Commands that run tools reject
 * with `{ message, details }`; the rest reject with a plain string, which is
 * passed through unchanged so existing `String(e)` handling keeps working.
 */
export function toApiError(raw: unknown): unknown {
  if (raw && typeof raw === "object" && !(raw instanceof Error)) {
    const o = raw as { message?: unknown; details?: unknown };
    if (typeof o.message === "string") {
      return new ApiError(o.message, typeof o.details === "string" ? o.details : undefined);
    }
  }
  return raw;
}

/** The human-readable message of any thrown value. */
export function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** Raw tool output attached to a thrown value, if any. */
export function errorDetails(e: unknown): string | undefined {
  return e instanceof ApiError ? e.details : undefined;
}
