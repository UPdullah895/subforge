import { describe, it, expect } from "vitest";
import { mount } from "@vue/test-utils";
import { i18n } from "../i18n";
import DependencyList from "./DependencyList.vue";
import type { DepStatus } from "../stores/depsStore";

function item(over: Partial<DepStatus> & Pick<DepStatus, "id">): DepStatus {
  return {
    label: over.id,
    severity: "required",
    state: "ok",
    purpose: "backend purpose",
    download_url: "https://example.com/dl",
    can_auto_install: false,
    ...over,
  };
}

function mountList(items: DepStatus[], extra: Record<string, unknown> = {}) {
  return mount(DependencyList, {
    props: { items, installing: null, logs: {}, errors: {}, ...extra },
    global: { plugins: [i18n] },
  });
}

describe("DependencyList", () => {
  it("shows an installed item without any actions", () => {
    const w = mountList([item({ id: "ffmpeg", version: "ffmpeg 7.1" })]);
    expect(w.get('[data-testid="status-ffmpeg"]').text()).toBe("Installed");
    expect(w.find('[data-testid="install-ffmpeg"]').exists()).toBe(false);
    expect(w.find('[data-testid="download-ffmpeg"]').exists()).toBe(false);
  });

  it("offers Install, Copy and Download for a missing auto-installable item", async () => {
    const w = mountList([
      item({ id: "ffmpeg", state: "missing", can_auto_install: true, install_command: "sudo pacman -S ffmpeg" }),
    ]);
    expect(w.get('[data-testid="status-ffmpeg"]').text()).toBe("Missing");
    expect(w.text()).toContain("sudo pacman -S ffmpeg");
    expect(w.find('[data-testid="copy-ffmpeg"]').exists()).toBe(true);
    expect(w.find('[data-testid="download-ffmpeg"]').exists()).toBe(true);
    await w.get('[data-testid="install-ffmpeg"]').trigger("click");
    expect(w.emitted("install")).toEqual([["ffmpeg"]]);
  });

  it("hides Install and explains the manual step when auto-install is impossible", () => {
    const w = mountList([
      item({ id: "ffmpeg", state: "missing", can_auto_install: false, install_command: "sudo pacman -S ffmpeg" }),
    ]);
    expect(w.find('[data-testid="install-ffmpeg"]').exists()).toBe(false);
    expect(w.text()).toContain("administrator rights");
  });

  it("explains a missing package manager for system tools", () => {
    const w = mountList([item({ id: "ffmpeg", state: "missing", can_auto_install: false })]);
    expect(w.text()).toContain("No supported package manager");
    expect(w.text()).not.toContain("Python");
  });

  it("has no purpose text for removed Python dependencies", () => {
    for (const id of ["python", "pysubs2", "faster_whisper"]) {
      const w = mountList([item({ id: id as DepStatus["id"], state: "missing", purpose: "backend purpose" })]);
      expect(w.text()).toContain("backend purpose");
    }
    const w = mountList([item({ id: "ffmpeg", state: "missing" })]);
    expect(w.text()).not.toContain("Python");
  });

  it("disables buttons while any install runs and labels the active one", () => {
    const items = [
      item({ id: "ffmpeg", state: "missing", can_auto_install: true }),
      item({ id: "mkvmerge", state: "missing", can_auto_install: true }),
    ];
    const w = mountList(items, { installing: "ffmpeg" });
    expect(w.get('[data-testid="install-ffmpeg"]').text()).toContain("Installing");
    expect(w.get('[data-testid="install-ffmpeg"]').attributes("disabled")).toBeDefined();
    expect(w.get('[data-testid="install-mkvmerge"]').attributes("disabled")).toBeDefined();
  });

  it("shows the error and log for a failed install", () => {
    const w = mountList([item({ id: "ffmpeg", state: "missing", can_auto_install: true })], {
      errors: { ffmpeg: "pkexec was dismissed" },
      logs: { ffmpeg: ["$ pkexec pacman -S ffmpeg", "error: target not found"] },
    });
    expect(w.get('[data-testid="error-ffmpeg"]').text()).toContain("pkexec was dismissed");
    expect(w.text()).toContain("error: target not found");
  });

  it("marks optional items as optional", () => {
    const w = mountList([item({ id: "mkvpropedit", severity: "optional", state: "missing" })]);
    expect(w.text()).toContain("Optional");
  });
});
