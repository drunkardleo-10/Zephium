export { default } from "./BrandMark.svelte";

/** A brand's mark, with the version drawn for a dark ground when the brand's
 *  own is a dark glyph that would vanish there. */
export type BrandArt = { light: string; dark?: string };
