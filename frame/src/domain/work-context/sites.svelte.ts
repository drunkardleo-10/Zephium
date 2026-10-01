import { commands } from "$shared/ipc/bindings";
import type {
  WorkSiteAccessResponseV1,
  WorkSiteAccessV1,
  WorkSiteRowV1,
} from "$shared/ipc/bindings";
import { observe } from "$shared/lib/observe";

const TIMEOUT = 9000;

/**
 * The sites the agent may work on as the person, as Rust keeps them. A change
 * shows only once Rust answers with the list it now holds.
 */
export class WorkSitesSession {
  sites = $state.raw<WorkSiteRowV1[]>([]);
  loaded = $state(false);
  unavailable = $state(false);
  /** The site whose change is in flight. */
  busy = $state<string | null>(null);
  private generation = 0;
  private lifetime = new AbortController();
  constructor(readonly profile: string) {}

  async start() {
    const generation = ++this.generation;
    this.lifetime = new AbortController();
    await this.settle(generation, () => commands.workSites(this.profile));
  }

  /** Always, Ask or Never for a site; `null` forgets it, so the agent asks each run. */
  async set(site: string, access: WorkSiteAccessV1 | null) {
    if (this.busy) return false;
    this.busy = site;
    const generation = this.generation;
    const ok = await this.settle(generation, () =>
      commands.workSetSite(this.profile, { site, access }),
    );
    if (generation === this.generation) this.busy = null;
    return ok;
  }

  private async settle(generation: number, request: () => Promise<WorkSiteAccessResponseV1>) {
    const response = await observe(Promise.resolve().then(request), TIMEOUT, this.lifetime.signal);
    if (generation !== this.generation) return false;
    if (
      response.state !== "received" ||
      response.value.version !== 1 ||
      response.value.profile !== this.profile ||
      response.value.error
    ) {
      this.unavailable = true;
      return false;
    }
    this.unavailable = false;
    this.loaded = true;
    this.sites = response.value.sites;
    return true;
  }

  dispose() {
    this.generation++;
    this.lifetime.abort();
    this.busy = null;
  }
}

const NESTED = new Set(["co", "com", "org", "net", "gov", "ac", "edu", "ne", "or"]);

/** The registrable site a person typed, as Rust keys it: `https://app.slack.com/x` → `slack.com`. */
export function siteOf(text: string): string | null {
  const value = text.trim().toLowerCase();
  if (!value) return null;
  const address = /^[a-z][a-z0-9+.-]*:\/\//u.test(value) ? value : `https://${value}`;
  const host = URL.canParse(address) ? URL.parse(address)!.hostname : null;
  if (!host) return null;
  const labels = host.replace(/\.$/u, "").split(".").filter(Boolean);
  if (labels.length < 2 || !labels.every((label) => /^[a-z0-9-]+$/u.test(label))) return null;
  if (/^\d+$/u.test(labels.at(-1)!)) return labels.join(".");
  const nested = labels.length > 2 && labels.at(-1)!.length === 2 && NESTED.has(labels.at(-2)!);
  return labels.slice(nested ? -3 : -2).join(".");
}
