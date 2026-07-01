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

export function englishName(code: string): string | undefined {
  return LANGUAGES[code.toLowerCase()]?.english;
}

export function nativeName(code: string): string | undefined {
  return LANGUAGES[code.toLowerCase()]?.native;
}

// Builds a track-name preview like "Arabic - عربي", matching what the Rust
// backend actually writes via `--track-name` when merging. Appends
// "(SDH)" for hearing-impaired tracks unless already present.
export function trackDisplayName(code: string, hearingImpaired = false): string {
  const entry = LANGUAGES[code.toLowerCase()];
  const english = entry?.english ?? code;
  const native = entry?.native ?? code;

  let name = english.toLowerCase() === native.toLowerCase() ? english : `${english} - ${native}`;

  if (hearingImpaired && !name.toLowerCase().includes("sdh")) {
    name += " (SDH)";
  }

  return name;
}
