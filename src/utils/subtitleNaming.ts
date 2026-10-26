// Mirrors `subforge_metadata::filenaming` (crates/subforge-metadata) so that
// subtitles extracted from the single-file view get the same
// `{video_stem}.{lang}[.{flavor}][.{n}].{ext}` names the batch extractor
// writes and the external-subs scan recognises (and that Plex/Jellyfin
// pick up). Keep the two in step.

export type SubFlavor = "forced" | "sdh";

export interface NamedTrack {
  id: number;
  language?: string | null;
  track_name?: string | null;
  forced_track?: boolean;
  hearing_impaired?: boolean;
}

import { isKnownLangCode, normalizeLangCode } from "./languageNames";

export interface ParsedSubtitleName {
  /** File name without the trailing `.{lang}[.{flavor}][.{n}].{ext}` tags. */
  stem: string;
  /** ISO 639-1 code, when the name carried a recognisable language tag. */
  lang?: string;
  flavor?: SubFlavor;
  ext: string;
}

/**
 * Splits `Show - S01E01.eng.sdh.srt` into stem / language / flavor / ext
 * (accepts 2- and 3-letter codes, a trailing `.N` disambiguator, and the
 * `sdh`/`forced`/`cc` tags). The inverse of `externalSubFilename`.
 */
export function parseSubtitleFilename(fileName: string): ParsedSubtitleName {
  const extMatch = fileName.match(/\.(srt|ass|ssa|vtt|sub)$/i);
  const ext = extMatch ? extMatch[1].toLowerCase() : "srt";
  const parts = (extMatch ? fileName.slice(0, -extMatch[0].length) : fileName).split(".");

  if (parts.length > 1 && /^\d+$/.test(parts[parts.length - 1])) parts.pop();
  let flavor: SubFlavor | undefined;
  const tail = parts[parts.length - 1]?.toLowerCase();
  if (parts.length > 1 && (tail === "sdh" || tail === "cc" || tail === "forced")) {
    flavor = tail === "forced" ? "forced" : "sdh";
    parts.pop();
  }
  let lang: string | undefined;
  const maybeLang = parts[parts.length - 1];
  if (parts.length > 1 && maybeLang && isKnownLangCode(maybeLang)) {
    lang = normalizeLangCode(maybeLang);
    parts.pop();
  }
  return { stem: parts.join("."), lang, flavor, ext };
}

export function dirOf(path: string): string {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return i >= 0 ? path.slice(0, i) : "";
}

export function baseName(path: string): string {
  return path.slice(Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\")) + 1);
}

export function fileStem(path: string): string {
  return baseName(path).replace(/\.[^.]+$/, "");
}

export function joinPath(dir: string, name: string): string {
  if (!dir) return name;
  const sep = dir.includes("\\") && !dir.includes("/") ? "\\" : "/";
  return dir.endsWith(sep) ? `${dir}${name}` : `${dir}${sep}${name}`;
}

export function trackFlavor(track: NamedTrack): SubFlavor | undefined {
  if (track.hearing_impaired || /\b(sdh|cc|hearing[ -]impaired)\b/i.test(track.track_name ?? "")) {
    return "sdh";
  }
  if (track.forced_track || /\bforced\b/i.test(track.track_name ?? "")) return "forced";
  return undefined;
}

export function externalSubFilename(
  videoStem: string,
  lang: string,
  ext: string,
  flavor?: SubFlavor,
  disambiguator?: number
): string {
  let name = `${videoStem}.${lang}`;
  if (flavor) name += `.${flavor}`;
  if (disambiguator !== undefined) name += `.${disambiguator}`;
  return `${name}.${ext}`;
}

/** One filename per track, adding `.N` only when language+flavor collide. */
export function planSubtitleFilenames(
  videoStem: string,
  tracks: NamedTrack[],
  ext = "srt"
): Map<number, string> {
  const keyOf = (t: NamedTrack) => `${t.language || "und"}|${trackFlavor(t) ?? ""}`;
  const totals = new Map<string, number>();
  for (const t of tracks) totals.set(keyOf(t), (totals.get(keyOf(t)) ?? 0) + 1);
  const seen = new Map<string, number>();
  const plan = new Map<number, string>();
  for (const t of tracks) {
    const key = keyOf(t);
    let n: number | undefined;
    if ((totals.get(key) ?? 0) > 1) {
      n = (seen.get(key) ?? 0) + 1;
      seen.set(key, n);
    }
    plan.set(t.id, externalSubFilename(videoStem, t.language || "und", ext, trackFlavor(t), n));
  }
  return plan;
}
