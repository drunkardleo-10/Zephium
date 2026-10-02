/**
 * The interface language. Paraglide reads the locale through `getLocale`;
 * backing it with state makes every message re-render where it is shown when
 * the language changes, with no reload.
 */
import { baseLocale, isLocale, locales, overwriteGetLocale } from "$shared/i18n/runtime";

type Locale = (typeof locales)[number];

let current = $state<Locale>(baseLocale);
overwriteGetLocale(() => current);

/** The first of the system's languages this build is translated into. */
function systemLocale(): Locale {
  for (const tag of navigator.languages ?? [navigator.language]) {
    if (isLocale(tag)) return tag;
    const base = tag.split("-")[0];
    if (base && isLocale(base)) return base;
  }
  return baseLocale;
}

/** Applies the `ui.language` preference: a locale id, or "system". */
export function applyLanguage(preference: string | null | undefined) {
  const wanted = !preference || preference === "system" ? systemLocale() : preference;
  current = isLocale(wanted) ? wanted : baseLocale;
  document.documentElement.lang = current;
  // Direction follows the language actually shown, not the one asked for.
  document.documentElement.dir = RIGHT_TO_LEFT.has(current.split("-")[0]!) ? "rtl" : "ltr";
}

/** Every language the interface offers, translated or not yet. Mirrors
 *  `zephium_core::preferences::LANGUAGES`. */
const INTERFACE_LANGUAGES = [
  "en",
  "ar",
  "bg",
  "bn",
  "ca",
  "cs",
  "da",
  "de",
  "el",
  "es",
  "es-419",
  "et",
  "fa",
  "fi",
  "fil",
  "fr",
  "he",
  "hi",
  "hr",
  "hu",
  "id",
  "it",
  "ja",
  "ko",
  "lt",
  "lv",
  "ms",
  "nb",
  "nl",
  "pl",
  "pt-BR",
  "pt-PT",
  "ro",
  "ru",
  "sk",
  "sl",
  "sr",
  "sv",
  "sw",
  "ta",
  "te",
  "th",
  "tr",
  "uk",
  "vi",
  "zh-CN",
  "zh-TW",
] as const;

const RIGHT_TO_LEFT = new Set(["ar", "fa", "he"]);

export const languages = (): readonly string[] => INTERFACE_LANGUAGES;
