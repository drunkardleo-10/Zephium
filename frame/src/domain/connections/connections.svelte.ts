import { commands } from "$shared/ipc/bindings";
import type {
  WorkCliRowV1,
  WorkConnectionsResponseV1,
  WorkServerCheckV1,
  WorkServerDraftV1,
  WorkServerRowV1,
} from "$shared/ipc/bindings";
import { observe } from "$shared/lib/observe";

/** Asking four tools for their status can take a few seconds. */
const LIST_TIMEOUT = 15000;
const SAVE_TIMEOUT = 20000;
/** A first `npx` start downloads the server. */
const CHECK_TIMEOUT = 60000;
/** The person signs in in a tab; Rust gives up after five minutes. */
const SIGN_IN_TIMEOUT = 320000;

/**
 * Settings → Connections: the tools on this Mac and the servers a profile
 * added. Rust owns the list and the Keychain; this mirrors its last answer,
 * and a secret passes through once on its way to the Keychain.
 */
export class ConnectionsSession {
  clis = $state.raw<WorkCliRowV1[] | null>(null);
  servers = $state.raw<WorkServerRowV1[] | null>(null);
  /** What the last test of each server found, by id. */
  checks = $state.raw<Record<string, WorkServerCheckV1>>({});
  /** The action in flight: `list`, `save`, `remove:<id>`, `check:<id>`, `sign-in:<id>`. */
  busy = $state<string | null>(null);
  /** The last action that could not finish. */
  failed = $state<string | null>(null);
  unavailable = $state(false);
  private lifetime = new AbortController();
  private generation = 0;
  private active = false;
  private previewId: string | null = null;
  private flows: Record<string, string> = {};
  private signInEvent = (event: Event) => {
    const flow = (event as CustomEvent<unknown>).detail;
    if (
      !flow ||
      typeof flow !== "object" ||
      !("profile" in flow) ||
      flow.profile !== this.profile ||
      !("id" in flow) ||
      typeof flow.id !== "string" ||
      !("request_id" in flow) ||
      typeof flow.request_id !== "string" ||
      !("phase" in flow)
    )
      return;
    if (flow.phase === "started" || flow.phase === "open_requested")
      this.flows[flow.id] = flow.request_id;
    if (flow.phase === "finished" && this.flows[flow.id] === flow.request_id)
      delete this.flows[flow.id];
  };

  async cancelPreview() {
    if (this.previewId) await this.cancelSignIn(this.previewId);
  }

  async cancelSignIn(id: string) {
    const request = this.flows[id];
    if (request) await commands.workCancelConnectionSignIn(this.profile, id, request);
  }
  private recovering = false;
  private recoveryPending = false;
  private timer: ReturnType<typeof setInterval> | null = null;
  private lastTick = 0;
  private recover = () => {
    if (!this.active || this.recovering) return;
    this.recoveryPending = true;
    void this.reconnect();
  };
  constructor(readonly profile: string) {}

  async start() {
    if (this.active) return;
    this.active = true;
    window.addEventListener("zephium:connection-sign-in", this.signInEvent);
    window.addEventListener("online", this.recover);
    window.addEventListener("zephium:connections-resume", this.recover);
    this.lastTick = Date.now();
    this.timer = setInterval(() => {
      const now = Date.now();
      if (now - this.lastTick > 45000) this.recover();
      this.lastTick = now;
      if (this.recoveryPending && !this.busy) void this.reconnect();
    }, 15000);
    await this.list(true);
    this.recover();
  }

  private async reconnect() {
    if (!this.active || this.recovering || this.busy || !navigator.onLine) return;
    this.recoveryPending = false;
    this.recovering = true;
    try {
      for (const row of this.servers ?? []) {
        if (!this.active) break;
        if (row.server.enabled) await this.check(row.server.id);
      }
    } finally {
      this.recovering = false;
    }
  }

  dispose() {
    this.active = false;
    window.removeEventListener("zephium:connection-sign-in", this.signInEvent);
    this.flows = {};
    window.removeEventListener("online", this.recover);
    window.removeEventListener("zephium:connections-resume", this.recover);
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    this.generation += 1;
    this.lifetime.abort();
  }

  /** Asks the tools again, as when the person installed or signed in to one. */
  refresh() {
    return this.list(true);
  }

  private async list(tools: boolean) {
    if (this.busy) return;
    const generation = this.generation;
    this.busy = "list";
    const response = await observe(
      Promise.resolve().then(() => commands.workConnections(this.profile)),
      LIST_TIMEOUT,
      this.lifetime.signal,
    );
    if (generation !== this.generation) return;
    this.busy = null;
    if (response.state !== "received" || !this.adopt(response.value, tools)) {
      if (!this.servers) this.unavailable = true;
    }
  }

  private adopt(response: WorkConnectionsResponseV1, tools: boolean): boolean {
    if (response.version !== 1 || response.profile !== this.profile || response.error) return false;
    this.unavailable = false;
    if (tools) this.clis = response.clis;
    this.servers = response.servers;
    return true;
  }

  private async act(
    action: string,
    request: () => Promise<WorkConnectionsResponseV1>,
  ): Promise<boolean> {
    if (this.busy) return false;
    const generation = this.generation;
    this.busy = action;
    this.failed = null;
    const response = await observe(
      Promise.resolve().then(request),
      SAVE_TIMEOUT,
      this.lifetime.signal,
    );
    if (generation !== this.generation) return false;
    this.busy = null;
    const done = response.state === "received" && this.adopt(response.value, false);
    if (!done) this.failed = action;
    return done;
  }

  async preview(draft: WorkServerDraftV1): Promise<WorkServerCheckV1 | null> {
    if (this.busy) return null;
    const generation = this.generation;
    this.busy = "preview";
    this.previewId = draft.server.id;
    this.failed = null;
    const response = await observe(
      Promise.resolve().then(() => commands.workPreviewConnection(this.profile, draft)),
      SIGN_IN_TIMEOUT,
      this.lifetime.signal,
    );
    if (generation !== this.generation) return null;
    this.busy = null;
    this.previewId = null;
    if (
      response.state !== "received" ||
      response.value.profile !== this.profile ||
      response.value.id !== draft.server.id ||
      response.value.version !== 1 ||
      response.value.error
    ) {
      this.failed = "preview";
      return null;
    }
    return response.value;
  }

  save(draft: WorkServerDraftV1) {
    return this.act("save", () => commands.workSaveConnection(this.profile, draft)).then(
      (saved) => {
        if (saved) {
          const { [draft.previous ?? draft.server.id]: _old, ...rest } = this.checks;
          this.checks = rest;
          void this.check(draft.server.id);
        }
        return saved;
      },
    );
  }

  remove(id: string) {
    return this.act(`remove:${id}`, () => commands.workRemoveConnection(this.profile, id));
  }

  /** Turns a server on or off for runs without touching its secrets. */
  enable(row: WorkServerRowV1, enabled: boolean) {
    return this.save({
      server: { ...row.server, enabled },
      secrets: [],
      previous: null,
    });
  }

  private async probe(action: string, id: string, timeout: number, sign: boolean) {
    if (this.busy) return;
    const generation = this.generation;
    this.busy = action;
    this.failed = null;
    const response = await observe(
      Promise.resolve().then(() =>
        sign
          ? commands.workSignInConnection(this.profile, id)
          : commands.workCheckConnection(this.profile, id),
      ),
      timeout,
      this.lifetime.signal,
    );
    if (generation !== this.generation) return;
    this.busy = null;
    if (
      response.state !== "received" ||
      response.value.version !== 1 ||
      response.value.profile !== this.profile ||
      response.value.id !== id ||
      response.value.error
    ) {
      this.failed = action;
      this.checks = {
        ...this.checks,
        [id]: {
          version: 1,
          profile: this.profile,
          id,
          outcome: "failed",
          tools: [],
          server_name: null,
          error: null,
        },
      };
      return;
    }
    this.checks = { ...this.checks, [id]: response.value };
    if (sign) await this.list(false);
  }

  check(id: string) {
    return this.probe(`check:${id}`, id, CHECK_TIMEOUT, false);
  }

  signIn(id: string) {
    return this.probe(`sign-in:${id}`, id, SIGN_IN_TIMEOUT, true);
  }
}
