/** Presentation-only store URL recognition. Rust revalidates the foreground
 * tab, origin and package before preparing an installation. */
export function chromeStoreListingId(value: string): string | null {
  try {
    const url = new URL(value);
    if (url.protocol !== "https:" || url.username !== "" || url.password !== "" || url.port !== "")
      return null;
    if (!["chromewebstore.google.com", "chrome.google.com"].includes(url.hostname)) return null;
    const path =
      url.hostname === "chrome.google.com"
        ? url.pathname.replace(/^\/webstore\//, "/")
        : url.pathname;
    return /^\/detail\/(?:[^/]+\/)?([a-p]{32})$/.exec(path)?.[1] ?? null;
  } catch {
    return null;
  }
}

export const isChromeStoreListing = (value: string): boolean =>
  chromeStoreListingId(value) !== null;
