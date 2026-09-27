/** Presentation-only store URL recognition. Rust revalidates the foreground
 * tab, origin and package before preparing an installation. */
export function isChromeStoreListing(value: string): boolean {
  try {
    const url = new URL(value);
    if (url.protocol !== "https:" || url.username !== "" || url.password !== "" || url.port !== "")
      return false;
    const path =
      url.hostname === "chrome.google.com"
        ? url.pathname.replace(/^\/webstore\//, "/")
        : url.pathname;
    return (
      ["chromewebstore.google.com", "chrome.google.com"].includes(url.hostname) &&
      /^\/detail\/(?:[^/]+\/)?[a-p]{32}$/.test(path)
    );
  } catch {
    return false;
  }
}
