import { describe, it, expect, vi, beforeEach } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { i18n } from "../i18n";

vi.mock("../api/client", () => ({ apiInvoke: vi.fn(), isTauri: () => false }));
vi.mock("../api/events", () => ({ apiListen: vi.fn() }));

import DependencyDialog from "./DependencyDialog.vue";
import DependencyBanner from "./DependencyBanner.vue";
import { useDepsStore } from "../stores/depsStore";

const missingFfmpeg = {
  os: "linux",
  has_missing_required: true,
  items: [
    {
      id: "ffmpeg", label: "FFmpeg", severity: "required", state: "missing", purpose: "p",
      download_url: "https://ffmpeg.org/download.html", install_command: "sudo pacman -S ffmpeg",
      can_auto_install: true,
    },
  ],
};

function setup() {
  const pinia = createPinia();
  setActivePinia(pinia);
  const store = useDepsStore();
  return { pinia, store };
}

describe("DependencyDialog", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("is hidden when nothing is missing", () => {
    const { pinia } = setup();
    mount(DependencyDialog, { global: { plugins: [pinia, i18n] }, attachTo: document.body });
    expect(document.body.querySelector('[data-testid="deps-dialog"]')).toBeNull();
  });

  it("opens when a required dependency is missing and Continue anyway dismisses it", async () => {
    const { pinia, store } = setup();
    store.report = missingFfmpeg as never;
    const w = mount(DependencyDialog, { global: { plugins: [pinia, i18n] }, attachTo: document.body });
    expect(document.body.querySelector('[data-testid="deps-dialog"]')).not.toBeNull();
    (document.body.querySelector('[data-testid="deps-later"]') as HTMLElement).click();
    await w.vm.$nextTick();
    expect(document.body.querySelector('[data-testid="deps-dialog"]')).toBeNull();
    expect(store.dialogDismissed).toBe(true);
  });
});

describe("DependencyBanner", () => {
  it("only shows after the dialog was dismissed, and Fix now reopens it", async () => {
    const { pinia, store } = setup();
    store.report = missingFfmpeg as never;
    const w = mount(DependencyBanner, { global: { plugins: [pinia, i18n] } });
    expect(w.find('[data-testid="deps-banner"]').exists()).toBe(false);
    store.dismiss();
    await w.vm.$nextTick();
    expect(w.find('[data-testid="deps-banner"]').exists()).toBe(true);
    await w.get('[data-testid="deps-fix"]').trigger("click");
    expect(store.dialogDismissed).toBe(false);
  });
});
