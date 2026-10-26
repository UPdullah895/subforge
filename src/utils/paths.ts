import { apiInvoke, isTauri } from "../api/client";

/**
 * Whether a dropped path is a folder. Drag and drop hands over bare paths with
 * no type information, and a folder usually has no extension, so it would
 * otherwise be treated as a media file (`mkvmerge: could not be opened`).
 *
 * Outside Tauri there are no local paths, so nothing is a folder.
 */
export async function isDirectory(path: string): Promise<boolean> {
  if (!isTauri()) return false;
  try {
    return await apiInvoke<boolean>("path_is_dir", { path });
  } catch {
    return false;
  }
}

/** Splits dropped paths into files and folders. */
export async function splitFilesAndFolders(paths: string[]): Promise<{
  files: string[];
  folders: string[];
}> {
  const kinds = await Promise.all(paths.map(isDirectory));
  return {
    files: paths.filter((_, i) => !kinds[i]),
    folders: paths.filter((_, i) => kinds[i]),
  };
}
