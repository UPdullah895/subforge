import { describe, it, expect, vi, beforeEach } from "vitest";
import { mount } from "@vue/test-utils";
import { i18n } from "../i18n";
import ToolPathField from "./ToolPathField.vue";
import type { ToolStatus } from "../stores/appStore";

vi.mock("../utils/dialog", () => ({ open: vi.fn() }));
import { open } from "../utils/dialog";

function mountField(props: Record<string, unknown> = {}) {
  return mount(ToolPathField, {
    props: { toolId: "ffmpeg", modelValue: undefined, status: null, ...props },
    global: { plugins: [i18n] },
  });
}

const status = (over: Partial<ToolStatus>): ToolStatus => ({
  id: "ffmpeg",
  source: "bundled",
  path: "/opt/sf/ffmpeg",
  version: "ffmpeg 7.1",
  error: null,
  ...over,
});

describe("ToolPathField", () => {
  beforeEach(() => vi.clearAllMocks());

  it("clicking the input opens the picker for a single file", async () => {
    vi.mocked(open).mockResolvedValue(null);
    const w = mountField();
    await w.get('[data-testid="input-ffmpeg"]').trigger("click");
    expect(open).toHaveBeenCalledWith(expect.objectContaining({ multiple: false, directory: false }));
  });

  it("clicking Browse opens the picker too", async () => {
    vi.mocked(open).mockResolvedValue(null);
    const w = mountField();
    await w.get('[data-testid="browse-ffmpeg"]').trigger("click");
    expect(open).toHaveBeenCalledTimes(1);
  });

  it("writes the selected path to the model and signals a change", async () => {
    vi.mocked(open).mockResolvedValue("/usr/bin/ffmpeg");
    const w = mountField();
    await w.get('[data-testid="browse-ffmpeg"]').trigger("click");
    await vi.waitFor(() => expect(w.emitted("update:modelValue")).toEqual([["/usr/bin/ffmpeg"]]));
    expect(w.emitted("change")).toHaveLength(1);
  });

  it("does nothing when the picker is cancelled", async () => {
    vi.mocked(open).mockResolvedValue(null);
    const w = mountField();
    await w.get('[data-testid="browse-ffmpeg"]').trigger("click");
    await Promise.resolve();
    expect(w.emitted("update:modelValue")).toBeUndefined();
    expect(w.emitted("change")).toBeUndefined();
  });

  it("shows the source badge and version", () => {
    const badge = (s: Partial<ToolStatus>) =>
      mountField({ status: status(s) }).get('[data-testid="badge-ffmpeg"]').text();
    expect(badge({})).toBe("Bundled");
    expect(badge({ source: "custom" })).toBe("Custom");
    expect(badge({ source: "system", path: null })).toBe("System");
    expect(badge({ source: null })).toBe("Not found");
    expect(mountField({ status: status({}) }).text()).toContain("ffmpeg 7.1");
  });

  it("shows the backend error text for an invalid custom path", () => {
    const w = mountField({
      modelValue: "/bad/ffmpeg",
      status: status({ source: "custom", error: "Not an executable: /bad/ffmpeg" }),
    });
    expect(w.get('[data-testid="error-ffmpeg"]').text()).toBe("Not an executable: /bad/ffmpeg");
  });

  it("reset clears the model and signals a change", async () => {
    const w = mountField({ modelValue: "/usr/bin/ffmpeg", status: status({ source: "custom" }) });
    await w.get('[data-testid="reset-ffmpeg"]').trigger("click");
    expect(w.emitted("update:modelValue")).toEqual([[undefined]]);
    expect(w.emitted("change")).toHaveLength(1);
  });

  it("allows typing and commits the typed value on change", async () => {
    const w = mountField({ modelValue: "/old" });
    const input = w.get('[data-testid="input-ffmpeg"]');
    expect(input.attributes("readonly")).toBeUndefined();
    await input.setValue("/typed/ffmpeg");
    expect(w.emitted("update:modelValue")).toEqual([["/typed/ffmpeg"]]);
    expect(w.emitted("change")).toHaveLength(1);
    expect(open).not.toHaveBeenCalled();
  });

  it("committing an emptied input resets to undefined", async () => {
    const w = mountField({ modelValue: "/old" });
    const input = w.get('[data-testid="input-ffmpeg"]');
    await input.setValue("  ");
    expect(w.emitted("update:modelValue")).toEqual([[undefined]]);
  });

  it("clicking a filled input does not open the picker", async () => {
    const w = mountField({ modelValue: "/old" });
    await w.get('[data-testid="input-ffmpeg"]').trigger("click");
    expect(open).not.toHaveBeenCalled();
  });

  it("shows no badge until status has loaded, and the path next to System", () => {
    expect(mountField({ status: null }).find('[data-testid="badge-ffmpeg"]').exists()).toBe(false);
    const w = mountField({ status: status({ source: "system", path: "/usr/bin/ffmpeg" }) });
    expect(w.get('[data-testid="badge-ffmpeg"]').text()).toBe("System (/usr/bin/ffmpeg)");
  });

  it("hides reset when no custom path is set", () => {
    expect(mountField().find('[data-testid="reset-ffmpeg"]').exists()).toBe(false);
  });
});
