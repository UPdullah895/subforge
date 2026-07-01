import { defineStore } from "pinia";
import { ref } from "vue";
import { apiInvoke } from "../api/client";

export type RenameProvider = "tmdb" | "tvmaze";
export type RenameMode = "manual" | RenameProvider;

export interface SeriesMatch {
  id: string;
  name: string;
  year?: number;
  poster_url?: string;
  provider: RenameProvider;
}

export interface Episode {
  season: number;
  episode: number;
  title: string;
}

export interface RenamePreviewRow {
  old_path: string;
  new_name?: string;
  matched: boolean;
  season?: number;
  episode?: number;
}

export interface ParsedFilename {
  series_guess: string;
  season?: number;
  episode?: number;
  tags: string[];
}

function fileExt(path: string): string {
  const name = path.split(/[/\\]/).pop() || path;
  const idx = name.lastIndexOf(".");
  return idx > 0 ? name.slice(idx + 1) : "";
}

function fileDir(path: string): string {
  const lastSlash = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return path.substring(0, lastSlash + 1);
}

export const useRenameStore = defineStore("rename", () => {
  const mode = ref<RenameMode>("manual");
  const query = ref("");
  const results = ref<SeriesMatch[]>([]);
  const selectedSeries = ref<SeriesMatch | null>(null);
  const episodes = ref<Episode[]>([]);
  const pattern = ref("{title} - S{s}E{e} - {episodeTitle}");
  const previewRows = ref<RenamePreviewRow[]>([]);
  const manualNames = ref<Record<string, string>>({});
  const manualPattern = ref("{name} - S{s}E{e}");
  const manualPatternName = ref("");
  const loading = ref(false);
  const error = ref<string | null>(null);

  function reset() {
    mode.value = "manual";
    query.value = "";
    results.value = [];
    selectedSeries.value = null;
    episodes.value = [];
    previewRows.value = [];
    manualNames.value = {};
    manualPatternName.value = "";
    error.value = null;
  }

  function fileNameOnly(path: string): string {
    return path.split(/[/\\]/).pop() || path;
  }

  /**
   * Fills manualNames for every file from one pattern + free-text name, using
   * the same season/episode parsing as parser.rs (no title-lookup API
   * involved). Rows stay individually editable afterward — this only sets
   * the initial values, it doesn't lock them.
   */
  async function applyManualPattern(files: string[]) {
    for (const f of files) {
      try {
        const parsed = await apiInvoke<ParsedFilename>("parse_filename", { name: fileNameOnly(f) });
        if (parsed.season == null || parsed.episode == null) continue;
        manualNames.value[f] = manualPattern.value
          .replace("{name}", manualPatternName.value)
          .replace("{s}", String(parsed.season).padStart(2, "0"))
          .replace("{e}", String(parsed.episode).padStart(2, "0"));
      } catch {
        // Skip files that fail to parse — leave their manual name untouched.
      }
    }
  }

  async function guessQueryFrom(firstFile: string) {
    try {
      const name = firstFile.split(/[/\\]/).pop() || firstFile;
      const parsed = await apiInvoke<ParsedFilename>("parse_filename", { name });
      query.value = parsed.series_guess;
    } catch {
      // Non-fatal — leave the search box empty for the user to fill in.
    }
  }

  async function search() {
    if (mode.value === "manual" || !query.value.trim()) return;
    loading.value = true;
    error.value = null;
    try {
      results.value = await apiInvoke<SeriesMatch[]>("search_series", {
        provider: mode.value,
        query: query.value,
      });
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
      results.value = [];
    } finally {
      loading.value = false;
    }
  }

  async function selectSeries(series: SeriesMatch) {
    selectedSeries.value = series;
    loading.value = true;
    error.value = null;
    try {
      episodes.value = await apiInvoke<Episode[]>("get_episodes", {
        provider: series.provider,
        series_id: series.id,
      });
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
      episodes.value = [];
    } finally {
      loading.value = false;
    }
  }

  async function buildPreview(files: string[]) {
    error.value = null;
    if (mode.value === "manual") {
      previewRows.value = files.map((f) => {
        const base = manualNames.value[f];
        const ext = fileExt(f);
        return {
          old_path: f,
          new_name: base ? `${base}${ext ? "." + ext : ""}` : undefined,
          matched: true,
        };
      });
      return;
    }
    if (!selectedSeries.value) return;
    loading.value = true;
    try {
      previewRows.value = await apiInvoke<RenamePreviewRow[]>("preview_rename", {
        files,
        episodes: episodes.value,
        series_title: selectedSeries.value.name,
        pattern: pattern.value,
      });
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    } finally {
      loading.value = false;
    }
  }

  function setRowOverride(oldPath: string, ep: Episode) {
    const row = previewRows.value.find((r) => r.old_path === oldPath);
    if (!row) return;
    row.season = ep.season;
    row.episode = ep.episode;
    row.matched = true;
    const ext = fileExt(oldPath);
    const title = selectedSeries.value?.name ?? "";
    const name = pattern.value
      .replace("{title}", title)
      .replace("{s}", String(ep.season).padStart(2, "0"))
      .replace("{e}", String(ep.episode).padStart(2, "0"))
      .replace("{episodeTitle}", ep.title)
      .replace("{quality}", "");
    row.new_name = `${name}${ext ? "." + ext : ""}`;
  }

  async function apply(): Promise<number> {
    const mapping = previewRows.value
      .filter((r) => r.new_name)
      .map((r) => ({
        old_path: r.old_path,
        new_path: `${fileDir(r.old_path)}${r.new_name}`,
      }));
    if (mapping.length === 0) return 0;
    return apiInvoke<number>("apply_rename", { mapping });
  }

  return {
    mode,
    query,
    results,
    selectedSeries,
    episodes,
    pattern,
    previewRows,
    manualNames,
    manualPattern,
    manualPatternName,
    loading,
    error,
    reset,
    guessQueryFrom,
    search,
    selectSeries,
    buildPreview,
    setRowOverride,
    applyManualPattern,
    apply,
  };
});
