/** Second-level suffixes a registrable name sits under: `airbnb.co.uk` is Airbnb's, not `co.uk`'s. */
const SUFFIXES = new Set([
  "co.uk",
  "org.uk",
  "ac.uk",
  "gov.uk",
  "me.uk",
  "com.au",
  "net.au",
  "org.au",
  "co.nz",
  "co.jp",
  "ne.jp",
  "or.jp",
  "co.kr",
  "com.br",
  "com.mx",
  "com.ar",
  "com.tr",
  "com.cn",
  "com.hk",
  "com.sg",
  "com.tw",
  "co.in",
  "co.za",
  "co.il",
  "com.pl",
  "com.ua",
  "github.io",
  "gitlab.io",
  "vercel.app",
  "netlify.app",
  "pages.dev",
  "workers.dev",
  "herokuapp.com",
  "blogspot.com",
]);

/** The host without `www.`, or empty for an address that is not one. */
export function hostOf(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./u, "").toLowerCase();
  } catch {
    return "";
  }
}

/** A host's registrable site: `jobs.paessler.com` → `paessler.com`, `www.airbnb.co.uk` → `airbnb.co.uk`. */
export function registrableSite(host: string): string {
  const labels = host
    .replace(/^www\./u, "")
    .toLowerCase()
    .split(".")
    .filter(Boolean);
  if (labels.length <= 2) return labels.join(".");
  if (/^\d+$/u.test(labels.at(-1)!)) return labels.join(".");
  const two = labels.slice(-2).join(".");
  return SUFFIXES.has(two) ? labels.slice(-3).join(".") : two;
}

/** The name a site goes by across its countries: `airbnb.com` and `airbnb.co.uk` are both `airbnb`. */
export function siteKey(host: string): string {
  const site = registrableSite(host);
  return site.split(".")[0] ?? site;
}

const KNOWN: Record<string, string> = {
  ycombinator: "Y Combinator",
  linkedin: "LinkedIn",
  github: "GitHub",
  gitlab: "GitLab",
  youtube: "YouTube",
  openai: "OpenAI",
  paypal: "PayPal",
  tripadvisor: "Tripadvisor",
  booking: "Booking.com",
  airbnb: "Airbnb",
  stackoverflow: "Stack Overflow",
  duckduckgo: "DuckDuckGo",
  wikipedia: "Wikipedia",
  greenhouse: "Greenhouse",
  ashbyhq: "Ashby",
  lever: "Lever",
  google: "Google",
  reddit: "Reddit",
};

const squash = (text: string) => text.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "");

/**
 * The name a person calls a site by. A page title usually carries it with its
 * own casing ("Apply to YC | Y Combinator", "Remote Work With EPAM"); a few
 * well-known names are known; otherwise the site's name, capitalised.
 */
export function siteName(host: string, titles: readonly string[] = []): string {
  const key = siteKey(host);
  if (!key) return host;
  if (KNOWN[key]) return KNOWN[key];
  for (const title of titles)
    for (const segment of title.split(/\s+[|·–—-]\s+|\s*[|·]\s*/u)) {
      const part = segment.trim();
      if (part && part.length <= 32 && squash(part) === key) return part;
    }
  const word = new RegExp(
    `(?:^|[^\\p{L}\\p{N}])(${key.replace(/[^a-z0-9]/gu, "")})(?![\\p{L}\\p{N}])`,
    "iu",
  );
  for (const title of titles) {
    const found = word.exec(title)?.[1];
    if (found && found !== found.toLowerCase()) return found;
  }
  // A three-letter site is an initialism: CMU, MIT, LOT.
  if (key.length <= 3) return key.toUpperCase();
  return key.charAt(0).toUpperCase() + key.slice(1);
}
