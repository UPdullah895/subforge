import { describe, it, expect, vi, beforeEach } from "vitest";

const state = vi.hoisted(() => ({ tauri: true }));

vi.mock("../api/client", () => ({
  isTauri: () => state.tauri,
  apiInvoke: vi.fn(),
}));

import { apiInvoke } from "../api/client";
import { isDirectory, splitFilesAndFolders } from "./paths";

describe("path helpers", () => {
  beforeEach(() => {
    state.tauri = true;
    vi.clearAllMocks();
  });

  it("asks the backend inside Tauri", async () => {
    vi.mocked(apiInvoke).mockResolvedValueOnce(true);
    await expect(isDirectory("/v/Lazarus")).resolves.toBe(true);
    expect(apiInvoke).toHaveBeenCalledWith("path_is_dir", { path: "/v/Lazarus" });
  });

  it("never asks the backend outside Tauri", async () => {
    state.tauri = false;
    await expect(isDirectory("/v/Lazarus")).resolves.toBe(false);
    expect(apiInvoke).not.toHaveBeenCalled();
  });

  it("treats a failed check as a file", async () => {
    vi.mocked(apiInvoke).mockRejectedValueOnce(new Error("nope"));
    await expect(isDirectory("/v/x.mkv")).resolves.toBe(false);
  });

  it("splits dropped paths into files and folders", async () => {
    vi.mocked(apiInvoke)
      .mockResolvedValueOnce(true) // /v/Lazarus
      .mockResolvedValueOnce(false); // /v/Lazarus/e01.mkv
    const result = await splitFilesAndFolders(["/v/Lazarus", "/v/Lazarus/e01.mkv"]);
    expect(result).toEqual({
      folders: ["/v/Lazarus"],
      files: ["/v/Lazarus/e01.mkv"],
    });
  });
});
