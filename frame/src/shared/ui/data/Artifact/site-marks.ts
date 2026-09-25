import { getContext, setContext } from "svelte";

/** A site's decoded mark, as its owner's favicon cache holds it. Display-only. */
export type SiteMark = { image: ImageData; tone: "dark" | "light" | "mid" };
/** The mark already held for an address or a bare host, or null; it fetches nothing. */
export type SiteMarks = (address: string) => SiteMark | null;

const key = Symbol("site-marks");

/** The owner supplies its cache; a renderer without one draws the neutral glyph. */
export function provideSiteMarks(resolve: SiteMarks) {
  setContext(key, resolve);
}

export function siteMarks(): SiteMarks | undefined {
  return getContext<SiteMarks | undefined>(key);
}
