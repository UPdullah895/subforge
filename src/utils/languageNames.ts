// Mirrors crates/subforge-metadata/src/languages.rs — kept in sync manually
// since the frontend needs this purely for display (track-name previews),
// not for anything that talks to mkvmerge directly.
const LANGUAGES: Record<string, { english: string; native: string }> = {
  en: { english: "English", native: "English" },
  ar: { english: "Arabic", native: "عربي" },
  es: { english: "Spanish", native: "Español" },
  fr: { english: "French", native: "Français" },
  de: { english: "German", native: "Deutsch" },
  ja: { english: "Japanese", native: "日本語" },
  ko: { english: "Korean", native: "한국어" },
  zh: { english: "Chinese", native: "中文" },
  ru: { english: "Russian", native: "Русский" },
  pt: { english: "Portuguese", native: "Português" },
  it: { english: "Italian", native: "Italiano" },
  tr: { english: "Turkish", native: "Türkçe" },
  hi: { english: "Hindi", native: "हिन्दी" },
  fa: { english: "Persian", native: "فارسی" },
  ur: { english: "Urdu", native: "اردو" },
  nl: { english: "Dutch", native: "Nederlands" },
  pl: { english: "Polish", native: "Polski" },
  sv: { english: "Swedish", native: "Svenska" },
  th: { english: "Thai", native: "ไทย" },
  vi: { english: "Vietnamese", native: "Tiếng Việt" },
  id: { english: "Indonesian", native: "Bahasa Indonesia" },
  und: { english: "Undetermined", native: "Undetermined" },
};

// mkvmerge reports ISO 639-2 three-letter codes ("eng"); the table above is
// keyed by ISO 639-1 ("en"). Includes both bibliographic and terminology forms.
const ISO3_TO_ISO1: Record<string, string> = {
  eng: "en", ara: "ar", spa: "es", fre: "fr", fra: "fr", ger: "de", deu: "de", jpn: "ja",
  kor: "ko", chi: "zh", zho: "zh", rus: "ru", por: "pt", ita: "it", tur: "tr", hin: "hi",
  per: "fa", fas: "fa", urd: "ur", dut: "nl", nld: "nl", pol: "pl", swe: "sv", tha: "th",
  vie: "vi", ind: "id",
};

/** "eng" / "ENG" / "en" -> "en"; unknown codes come back lower-cased. */
export function normalizeLangCode(code: string): string {
  const c = code.toLowerCase();
  return ISO3_TO_ISO1[c] ?? c;
}

/** True for ISO 639-1 codes SubForge knows ("en") and their 639-2 forms ("eng"). */
export function isKnownLangCode(code: string): boolean {
  const c = code.toLowerCase();
  return c in LANGUAGES || c in ISO3_TO_ISO1;
}

export function englishName(code: string): string | undefined {
  return LANGUAGES[normalizeLangCode(code)]?.english;
}

export function nativeName(code: string): string | undefined {
  return LANGUAGES[normalizeLangCode(code)]?.native;
}

// Builds a track-name preview like "Arabic - عربي", matching what the Rust
// backend actually writes via `--track-name` when merging. Appends
// "(SDH)" for hearing-impaired tracks unless already present.
export function trackDisplayName(code: string, hearingImpaired = false): string {
  const entry = LANGUAGES[normalizeLangCode(code)];
  const english = entry?.english ?? code;
  const native = entry?.native ?? code;

  let name = english.toLowerCase() === native.toLowerCase() ? english : `${english} - ${native}`;

  if (hearingImpaired && !name.toLowerCase().includes("sdh")) {
    name += " (SDH)";
  }

  return name;
}

/// Languages DeepL's v2 API actually supports, matching the backend const.
export const DEEPL_LANGS = new Set(["en", "ar", "es", "fr", "de", "ja", "ko", "zh", "ru", "pt", "it", "tr", "nl", "pl", "sv", "id"]);

export function isDeeplSupported(code: string): boolean {
  return DEEPL_LANGS.has(code.toLowerCase());
}
