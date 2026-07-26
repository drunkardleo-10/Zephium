/**
 * Returns the quiet, native-chrome representation of one authoritative URL.
 *
 * Rust's synchronous presentation barrier performs the same `URL.host`
 * projection before revealing page content. Keep this helper strict: an
 * invalid authoritative value should render empty instead of echoing
 * untrusted text into privileged chrome.
 */
export function restingAddress(url: string | null | undefined): string {
  if (!url) return "";
  try {
    return new URL(url).host;
  } catch {
    return "";
  }
}

export function editingAddress(url: string | null | undefined): string {
  return url ?? "";
}

export type AddressSecurity = "none" | "secure" | "insecure";

/**
 * Transport standing of one authoritative URL. Anything that is not an exact
 * known web scheme reports `none` rather than guessing, so an internal page or
 * a malformed authoritative value never renders a reassuring lock.
 */
export function addressSecurity(url: string | null | undefined): AddressSecurity {
  if (!url) return "none";
  try {
    switch (new URL(url).protocol) {
      case "https:":
      case "wss:":
        return "secure";
      case "http:":
      case "ws:":
        return "insecure";
      default:
        return "none";
    }
  } catch {
    return "none";
  }
}
