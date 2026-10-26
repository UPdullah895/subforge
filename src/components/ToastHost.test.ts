import { describe, it, expect } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { i18n } from "../i18n";
import ToastHost from "./ToastHost.vue";
import ErrorBanner from "./ErrorBanner.vue";
import { useToastStore } from "../stores/toastStore";

describe("error details expander", () => {
  it("toast shows raw tool output in a collapsed Details section", async () => {
    const pinia = createPinia();
    setActivePinia(pinia);
    const toast = useToastStore();
    const w = mount(ToastHost, { global: { plugins: [pinia, i18n] } });
    toast.error("Extraction failed", "mkvmerge failed: bad file", "mkvmerge v102.0\nError: bad file");
    await w.vm.$nextTick();
    const details = w.get('[data-testid="error-details"]');
    expect(details.element.tagName).toBe("DETAILS");
    expect((details.element as HTMLDetailsElement).open).toBe(false);
    expect(details.text()).toContain(i18n.global.t("common.details"));
    expect(details.get("pre").text()).toBe("mkvmerge v102.0\nError: bad file");
  });

  it("toast without details has no expander", async () => {
    const pinia = createPinia();
    setActivePinia(pinia);
    const toast = useToastStore();
    const w = mount(ToastHost, { global: { plugins: [pinia, i18n] } });
    toast.error("Extraction failed", "short");
    await w.vm.$nextTick();
    expect(w.find('[data-testid="error-details"]').exists()).toBe(false);
  });

  it("error banner shows details when given", () => {
    const w = mount(ErrorBanner, {
      props: { message: "Failed to download model", details: "GET x -> 404" },
      global: { plugins: [i18n] },
    });
    expect(w.get('[data-testid="error-details"] pre').text()).toBe("GET x -> 404");
  });
});
