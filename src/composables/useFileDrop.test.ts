import { describe, it, expect, vi, beforeEach } from "vitest";
import { defineComponent, h, nextTick } from "vue";
import { flushPromises, mount } from "@vue/test-utils";

const state = vi.hoisted(() => ({
  handler: null as null | ((event: { payload: { type: string; paths?: string[] } }) => void),
  unlisten: vi.fn(),
  tauri: true,
}));

vi.mock("../api/client", () => ({ isTauri: () => state.tauri }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: async (handler: typeof state.handler) => {
      state.handler = handler;
      return state.unlisten;
    },
  }),
}));

import { useFileDrop } from "./useFileDrop";

function mountDrop(extensions?: string[], onDrop = vi.fn()) {
  const Comp = defineComponent({
    setup() {
      const { dragging } = useFileDrop({ extensions, onDrop });
      return { dragging };
    },
    render() {
      return h("div", this.dragging ? "dragging" : "idle");
    },
  });
  const wrapper = mount(Comp);
  return { wrapper, onDrop };
}

function drop(paths: string[]) {
  state.handler?.({ payload: { type: "drop", paths } });
}

describe("useFileDrop", () => {
  beforeEach(() => {
    state.handler = null;
    state.tauri = true;
    state.unlisten.mockClear();
  });

  it("shows the overlay on enter and clears it on drop", async () => {
    const { wrapper } = mountDrop();
    await flushPromises();
    state.handler?.({ payload: { type: "enter", paths: [] } });
    await nextTick();
    expect(wrapper.text()).toBe("dragging");
    drop(["/v/episode.mkv"]);
    await nextTick();
    expect(wrapper.text()).toBe("idle");
  });

  it("filters by extension and accepts extension-less paths as folders", async () => {
    const { wrapper, onDrop } = mountDrop(["mkv", "srt"]);
    await flushPromises();
    drop(["/v/episode.MKV", "/v/notes.txt", "/v/subtitles"]);
    expect(onDrop).toHaveBeenCalledWith(["/v/episode.MKV", "/v/subtitles"]);
    expect(wrapper.text()).toBe("idle");
  });

  it("does nothing outside Tauri", async () => {
    state.tauri = false;
    const { onDrop } = mountDrop(["mkv"]);
    await flushPromises();
    expect(state.handler).toBeNull();
    expect(onDrop).not.toHaveBeenCalled();
  });

  it("unsubscribes when the view unmounts", async () => {
    const { wrapper } = mountDrop();
    await flushPromises();
    wrapper.unmount();
    expect(state.unlisten).toHaveBeenCalled();
  });
});
