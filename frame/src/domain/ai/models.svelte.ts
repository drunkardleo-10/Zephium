import { commands } from "$shared/ipc/bindings";
import type {
  WorkModelEntry,
  WorkModelProvider,
  WorkModelRole,
  WorkModelsFaultV1,
  WorkModelsV1,
} from "$shared/ipc/bindings";
import { events } from "$shared/ipc/native-events";
import { observe } from "$shared/lib/observe";

const TIMEOUT = 9000;
/** A key check asks the provider over the network. */
const KEY_TIMEOUT = 20000;

/**
 * The picker's and Settings → AI's view of models and keys. Rust owns the
 * catalog, the choices and the Keychain; this mirrors its last answer. A key
 * passes through once on its way to the Keychain and is never kept here.
 */
export class ModelsSession {
  models = $state.raw<WorkModelsV1 | null>(null);
  unavailable = $state(false);
  /** The action in flight, such as `key:anthropic` or `choose:lead`. */
  busy = $state<string | null>(null);
  /** What the last action could not do, with the action it belongs to. */
  fault = $state.raw<{ action: string; fault: WorkModelsFaultV1 } | null>(null);
  /** The models the person's own server serves, once asked. */
  endpoint = $state.raw<WorkModelEntry[]>([]);
  more = $state.raw<WorkModelEntry[]>([]);
  listingMore = $state(false);
  private listing = false;
  private readSequence = 0;
  private readPending = false;
  private active = false;
  private generation = 0;
  private stop: (() => void) | null = null;
  private lifetime = new AbortController();
  constructor(readonly profile: string) {}

  /** Subscribe before the first read: a change in between is never lost. */
  async start() {
    if (this.active) return;
    this.active = true;
    const generation = ++this.generation;
    this.lifetime = new AbortController();
    const stop = await events.workModelsChanged.listen(() => void this.read());
    if (!this.active || generation !== this.generation) {
      stop();
      return;
    }
    this.stop = stop;
    await this.read();
  }

  private async read() {
    if (this.busy) {
      this.readPending = true;
      return;
    }
    const sequence = ++this.readSequence;
    const generation = this.generation;
    const response = await observe(
      Promise.resolve().then(() => commands.workModels(this.profile)),
      TIMEOUT,
      this.lifetime.signal,
    );
    if (generation !== this.generation || sequence !== this.readSequence) return;
    if (response.state !== "received") {
      this.unavailable = true;
      return;
    }
    this.adopt(response.value);
  }

  private adopt(response: WorkModelsV1) {
    if (response.version !== 1 || response.profile !== this.profile) {
      this.unavailable = true;
      return;
    }
    if (response.fault === "unavailable") {
      this.unavailable = true;
      return;
    }
    this.unavailable = false;
    const previousBase = this.models?.providers.find((row) => row.provider === "compatible")?.base;
    const nextBase = response.providers.find((row) => row.provider === "compatible")?.base;
    if (previousBase !== nextBase) this.endpoint = [];
    this.models = response;
  }

  private async act(action: string, request: () => Promise<WorkModelsV1>, timeout = TIMEOUT) {
    if (this.busy) return false;
    this.busy = action;
    this.readSequence++;
    this.fault = null;
    const generation = this.generation;
    const response = await observe(Promise.resolve().then(request), timeout, this.lifetime.signal);
    if (generation !== this.generation) return false;
    this.busy = null;
    if (response.state !== "received") {
      this.fault = { action, fault: "unreachable" };
      if (this.readPending) {
        this.readPending = false;
        void this.read();
      }
      return false;
    }
    this.adopt(response.value);
    const fault = response.value.fault;
    if (fault && fault !== "unavailable") this.fault = { action, fault };
    if (this.readPending) {
      this.readPending = false;
      void this.read();
    }
    return !fault;
  }

  choose(role: WorkModelRole, id: string | null) {
    return this.act(`choose:${role}`, () => commands.workChooseModel(this.profile, role, id));
  }

  /** The key goes straight to Rust, which checks it with the provider first. */
  setKey(provider: WorkModelProvider, key: string) {
    return this.act(
      `key:${provider}`,
      () => commands.workSetProviderKey(this.profile, provider, key),
      KEY_TIMEOUT,
    );
  }

  testKey(provider: WorkModelProvider) {
    return this.act(
      `test:${provider}`,
      () => commands.workTestProviderKey(this.profile, provider),
      KEY_TIMEOUT,
    );
  }

  clearKey(provider: WorkModelProvider) {
    return this.act(`clear:${provider}`, () =>
      commands.workClearProviderKey(this.profile, provider),
    );
  }

  setEndpoint(base: string | null) {
    return this.act("endpoint", () => commands.workSetModelEndpoint(this.profile, base));
  }

  /** The person's own server's models, asked once per session. */
  async listEndpoint() {
    if (this.listing || this.endpoint.length) return;
    this.listing = true;
    const generation = this.generation;
    const response = await observe(
      Promise.resolve().then(() => commands.workMoreModels(this.profile, "compatible")),
      KEY_TIMEOUT,
      this.lifetime.signal,
    );
    if (generation !== this.generation) return;
    this.listing = false;
    if (response.state === "received" && !response.value.fault)
      this.endpoint = response.value.entries;
  }

  async listMore(providers: readonly WorkModelProvider[]) {
    if (this.listingMore) return;
    this.listingMore = true;
    const generation = this.generation;
    const results = await Promise.all(
      providers.map((provider) =>
        observe(
          Promise.resolve().then(() => commands.workMoreModels(this.profile, provider)),
          KEY_TIMEOUT,
          this.lifetime.signal,
        ),
      ),
    );
    if (generation !== this.generation) return;
    this.listingMore = false;
    const entries = results.flatMap((result) =>
      result.state === "received" && !result.value.fault ? result.value.entries : [],
    );
    this.more = entries;
    const failed = results.find((result) => result.state !== "received" || result.value.fault);
    if (failed)
      this.fault = {
        action: "more",
        fault: failed.state === "received" ? (failed.value.fault ?? "unreachable") : "unreachable",
      };
  }

  dispose() {
    this.active = false;
    this.generation++;
    this.stop?.();
    this.stop = null;
    this.lifetime.abort();
    this.models = null;
    this.busy = null;
    this.fault = null;
    this.endpoint = [];
    this.more = [];
    this.listingMore = false;
    this.listing = false;
    this.readPending = false;
    this.readSequence++;
  }
}
