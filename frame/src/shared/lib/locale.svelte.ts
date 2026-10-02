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
}

export const languages = () => locales;
