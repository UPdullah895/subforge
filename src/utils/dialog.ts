import { ref } from "vue";
import { isTauri } from "../api/client";

export interface DialogFilter {
  name: string;
  extensions: string[];
}

export interface OpenOptions {
  multiple?: boolean;
  directory?: boolean;
  filters?: DialogFilter[];
  title?: string;
  defaultPath?: string;
}

export interface SaveOptions {
  filters?: DialogFilter[];
  title?: string;
  defaultPath?: string;
}

export interface PickerRequest {
  mode: "open" | "save";
  multiple: boolean;
  directory: boolean;
  filters: DialogFilter[];
  title?: string;
  defaultPath?: string;
  resolve: (value: string | string[] | null) => void;
}

// The request the <WebFilePicker> component is currently showing, if any.
// Only used outside Tauri, where there is no native dialog and the files
// live on the machine running the server.
export const pickerRequest = ref<PickerRequest | null>(null);

function requestWebPicker(
  req: Omit<PickerRequest, "resolve">
): Promise<string | string[] | null> {
  // A second dialog while one is open replaces it; the first caller gets null.
  pickerRequest.value?.resolve(null);
  return new Promise((resolve) => {
    pickerRequest.value = { ...req, resolve };
  });
}

/** Drop-in for `@tauri-apps/plugin-dialog`'s `open`, with a web fallback. */
export async function open(options: OpenOptions = {}): Promise<string | string[] | null> {
  if (isTauri()) {
    const dialog = await import("@tauri-apps/plugin-dialog");
    return dialog.open(options) as Promise<string | string[] | null>;
  }
  return requestWebPicker({
    mode: "open",
    multiple: !!options.multiple,
    directory: !!options.directory,
    filters: options.filters ?? [],
    title: options.title,
    defaultPath: options.defaultPath,
  });
}

/** Drop-in for `@tauri-apps/plugin-dialog`'s `save`, with a web fallback. */
export async function save(options: SaveOptions = {}): Promise<string | null> {
  if (isTauri()) {
    const dialog = await import("@tauri-apps/plugin-dialog");
    return dialog.save(options);
  }
  const result = await requestWebPicker({
    mode: "save",
    multiple: false,
    directory: false,
    filters: options.filters ?? [],
    title: options.title,
    defaultPath: options.defaultPath,
  });
  return typeof result === "string" ? result : null;
}
