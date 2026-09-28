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

type More = { entries: WorkModelEntry[]; fault: WorkModelsFaultV1 | null; loading: boolean };

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
  more = $state.raw<Partial<Record<WorkModelProvider, More>>>({});
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
    const generation = this.generation;
    const response = await observe(
      Promise.resolve().then(() => commands.workModels(this.profile)),
      TIMEOUT,
      this.lifetime.signal,
    );
    if (generation !== this.generation) return;
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
    this.models = response;
  }

  private async act(action: string, request: () => Promise<WorkModelsV1>, timeout = TIMEOUT) {
    if (this.busy) return false;
    this.busy = action;
    this.fault = null;
    const generation = this.generation;
    const response = await observe(Promise.resolve().then(request), timeout, this.lifetime.signal);
    if (generation !== this.generation) return false;
    this.busy = null;
    if (response.state !== "received") {
      this.fault = { action, fault: "unreachable" };
      return false;
    }
    this.adopt(response.value);
    const fault = response.value.fault;
    if (fault && fault !== "unavailable") this.fault = { action, fault };
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

  /** The provider's own list, fetched once per session. */
  async loadMore(provider: WorkModelProvider) {
    const known = this.more[provider];
    if (known && (known.loading || !known.fault)) return;
    this.more = { ...this.more, [provider]: { entries: [], fault: null, loading: true } };
    const generation = this.generation;
    const response = await observe(
      Promise.resolve().then(() => commands.workMoreModels(this.profile, provider)),
      KEY_TIMEOUT,
      this.lifetime.signal,
    );
    if (generation !== this.generation) return;
    const next: More =
      response.state === "received"
        ? { entries: response.value.entries, fault: response.value.fault, loading: false }
        : { entries: [], fault: "unreachable", loading: false };
    this.more = { ...this.more, [provider]: next };
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
    this.more = {};
  }
}
