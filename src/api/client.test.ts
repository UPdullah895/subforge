import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { apiInvoke } from "./client";
import { ApiError, errorDetails } from "./errors";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

describe("apiInvoke (Tauri mode)", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  });
  afterEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  it("turns a {message, details} rejection into an ApiError", async () => {
    invokeMock.mockRejectedValue({ message: "mkvmerge failed: bad file", details: "raw stderr" });
    const err = (await apiInvoke("enumerate_tracks", { filePath: "x" }).catch((e) => e)) as ApiError;
    expect(err).toBeInstanceOf(ApiError);
    expect(err.message).toBe("mkvmerge failed: bad file");
    expect(errorDetails(err)).toBe("raw stderr");
  });

  it("passes plain string rejections through unchanged", async () => {
    invokeMock.mockRejectedValue("plain failure");
    await expect(apiInvoke("save_config", {})).rejects.toBe("plain failure");
  });
});

function respond(body: string, init: { status?: number } = {}) {
  return Promise.resolve(new Response(body, { status: init.status ?? 200 }));
}

describe("apiInvoke (browser / REST mode)", () => {
  const fetchMock = vi.fn();

  beforeEach(() => {
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
  });
  afterEach(() => vi.unstubAllGlobals());

  const lastCall = () => {
    const [url, init] = fetchMock.mock.calls[0];
    return { url: url as string, init: init as RequestInit | undefined };
  };

  it("turns camelCase named args into the snake_case body the server expects", async () => {
    fetchMock.mockReturnValue(respond("[]"));
    await apiInvoke("enumerate_tracks", { filePath: "/v/a.mkv", trackId: 3 });
    expect(JSON.parse(lastCall().init!.body as string)).toEqual({ file_path: "/v/a.mkv", track_id: 3 });
  });

  it("unwraps a single struct argument (request / options / config)", async () => {
    fetchMock.mockReturnValue(respond("[]"));
    await apiInvoke("scan_directory", { options: { path: "/lib", recursive: true } });
    expect(JSON.parse(lastCall().init!.body as string)).toEqual({ path: "/lib", recursive: true });

    fetchMock.mockClear();
    fetchMock.mockReturnValue(respond(""));
    await apiInvoke("save_config", { config: { theme: "dark" } });
    expect(JSON.parse(lastCall().init!.body as string)).toEqual({ theme: "dark" });
  });

  it("keeps array arguments wrapped", async () => {
    fetchMock.mockReturnValue(respond("1"));
    await apiInvoke("apply_rename", { mapping: [{ old_path: "a", new_path: "b" }] });
    expect(JSON.parse(lastCall().init!.body as string)).toEqual({
      mapping: [{ old_path: "a", new_path: "b" }],
    });
  });

  it("keeps the credentials wrapper the server handler expects", async () => {
    fetchMock.mockReturnValue(respond(""));
    await apiInvoke("set_opensubtitles_credentials", { credentials: { api_key: "k" } });
    expect(JSON.parse(lastCall().init!.body as string)).toEqual({ credentials: { api_key: "k" } });
  });

  it("uses GET for load_config and get_tool_registry (POST on /api/config is 'save')", async () => {
    fetchMock.mockReturnValue(respond('{"theme":"dark"}'));
    await apiInvoke("load_config");
    expect(lastCall().url).toBe("/api/config");
    expect(lastCall().init).toBeUndefined();
  });

  it("fetches get_tool_status with GET from /api/tools/status", async () => {
    fetchMock.mockReturnValue(
      respond(
        '[{"id":"ffmpeg","source":"bundled","path":"/x","version":"7","error":null},{"id":"ffprobe","source":null,"path":null,"version":null,"error":null}]'
      )
    );
    const rows = await apiInvoke<{ id: string }[]>("get_tool_status");
    expect(lastCall().url).toBe("/api/tools/status");
    expect(lastCall().init).toBeUndefined();
    expect(rows.map((r) => r.id)).toEqual(["ffmpeg", "ffprobe"]);
  });

  it("treats an empty 200 body as success, not a JSON error", async () => {
    fetchMock.mockReturnValue(respond(""));
    await expect(apiInvoke("extract_audio_track", { filePath: "x" })).resolves.toBeUndefined();
  });

  it("extracts the message from {error} bodies and falls back to readable status text", async () => {
    fetchMock.mockReturnValueOnce(respond('{"error":"already exists"}', { status: 400 }));
    await expect(apiInvoke("apply_rename", { mapping: [] })).rejects.toThrow("already exists");

    fetchMock.mockReturnValueOnce(respond("", { status: 500 }));
    await expect(apiInvoke("extract_audio_track", { filePath: "x" })).rejects.toThrow(/server hit an error/);
  });

  it("keeps {error, details} bodies as an ApiError with details", async () => {
    fetchMock.mockReturnValueOnce(
      respond('{"error":"mkvmerge failed: bad file","details":"mkvmerge v102.0\\nError: bad file"}', {
        status: 422,
      })
    );
    const err = (await apiInvoke("enumerate_tracks", { filePath: "x" }).catch((e) => e)) as ApiError;
    expect(err).toBeInstanceOf(ApiError);
    expect(err.message).toBe("mkvmerge failed: bad file");
    expect(errorDetails(err)).toContain("v102.0");
  });

  it("explains schema mismatches instead of dumping the validation JSON", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    fetchMock.mockReturnValue(respond('[{"id":"not-a-number"}]'));
    await expect(apiInvoke("enumerate_tracks", { filePath: "x" })).rejects.toThrow(/unexpected response/);
  });

  it("accepts null for Rust Option fields and keeps unknown fields", async () => {
    fetchMock.mockReturnValue(
      respond(
        JSON.stringify([
          { id: 1, track_type: "subtitles", codec: "SRT", language: "eng", track_name: null, default_track: true, forced_track: false, hearing_impaired: true },
        ])
      )
    );
    const tracks = await apiInvoke<Array<Record<string, unknown>>>("enumerate_tracks", { filePath: "x" });
    expect(tracks[0].track_name).toBeNull();
    expect(tracks[0].hearing_impaired).toBe(true);
  });
});
