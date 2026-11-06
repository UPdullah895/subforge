import { describe, expect, it } from "vitest";
import { isFatalEngineError, mapJobErrorMessage } from "./translateStore";

// The body Ollama actually returned when a 12B model met an 8 GB card that had a game on it.
const OOM =
  "out_of_memory: Ollama could not load 'gemma3:12b': the GPU is out of memory. " +
  "It asked for about 463 MB more of GPU memory. Close other GPU apps, choose a smaller model, or run on the CPU.";

describe("mapJobErrorMessage", () => {
  it("turns a GPU out-of-memory into an actionable sentence, not raw JSON", () => {
    const mapped = mapJobErrorMessage(OOM);
    expect(mapped).toBeTruthy();
    expect(mapped).not.toContain("cudaMalloc");
    expect(mapped).not.toContain("out_of_memory");
    // Localised: English in this test locale, and it names the way out.
    expect(mapped).toContain("GPU");
  });

  it("still maps an unreachable engine", () => {
    expect(mapJobErrorMessage("connect_failed")).toContain("Ollama");
  });

  it("leaves an unrelated message alone", () => {
    expect(mapJobErrorMessage("subtitle has no lines")).toBe("subtitle has no lines");
    expect(mapJobErrorMessage(undefined)).toBeUndefined();
  });
});

describe("isFatalEngineError", () => {
  it("stops a batch on the two errors retrying cannot fix", () => {
    expect(isFatalEngineError(OOM)).toBe(true);
    expect(isFatalEngineError("connect_failed")).toBe(true);
  });

  it("treats a per-file failure as non-fatal", () => {
    expect(isFatalEngineError("subtitle has no lines")).toBe(false);
    expect(isFatalEngineError("API error: Ollama returned HTTP 400")).toBe(false);
  });
});
