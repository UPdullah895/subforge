import { describe, expect, it } from "vitest";
import {
  externalSubFilename,
  parseSubtitleFilename,
  planSubtitleFilenames,
  trackFlavor,
} from "./subtitleNaming";

describe("parseSubtitleFilename", () => {
  it("reads 3-letter language, flavor and extension", () => {
    expect(parseSubtitleFilename("Show - S01E01.eng.sdh.srt")).toEqual({
      stem: "Show - S01E01",
      lang: "en",
      flavor: "sdh",
      ext: "srt",
    });
  });

  it("reads a 2-letter language and ignores a trailing disambiguator", () => {
    const parsed = parseSubtitleFilename("Show.S01E01.ar.2.srt");
    expect(parsed.stem).toBe("Show.S01E01");
    expect(parsed.lang).toBe("ar");
  });

  it("leaves dots in the show name alone when there is no language tag", () => {
    expect(parseSubtitleFilename("Some.Movie.2019.1080p.srt")).toMatchObject({
      stem: "Some.Movie.2019.1080p",
      lang: undefined,
    });
  });

  it("round-trips with externalSubFilename", () => {
    const name = externalSubFilename("Show - S01E01", "ara", "srt", "forced", 2);
    expect(name).toBe("Show - S01E01.ara.forced.2.srt");
    expect(parseSubtitleFilename(name)).toMatchObject({ stem: "Show - S01E01", lang: "ar", flavor: "forced" });
  });
});

describe("planSubtitleFilenames", () => {
  it("adds .N only when language and flavor collide", () => {
    const plan = planSubtitleFilenames("Ep", [
      { id: 2, language: "eng", track_name: "English" },
      { id: 3, language: "spa", track_name: "Spanish" },
      { id: 4, language: "eng", track_name: "English SDH", hearing_impaired: true },
      { id: 5, language: "spa", track_name: "Spanish" },
    ]);
    expect(plan.get(2)).toBe("Ep.eng.srt");
    expect(plan.get(4)).toBe("Ep.eng.sdh.srt");
    expect(plan.get(3)).toBe("Ep.spa.1.srt");
    expect(plan.get(5)).toBe("Ep.spa.2.srt");
  });

  it("falls back to the track name when the flag is missing", () => {
    expect(trackFlavor({ id: 1, track_name: "English (SDH)" })).toBe("sdh");
    expect(trackFlavor({ id: 1, track_name: "Signs & Songs", forced_track: true })).toBe("forced");
    expect(trackFlavor({ id: 1, track_name: "English" })).toBeUndefined();
  });
});
