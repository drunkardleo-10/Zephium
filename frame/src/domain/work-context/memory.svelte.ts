import { commands } from "$shared/ipc/bindings";
import type {
  WorkMemoryChangeV1,
  WorkMemoryQueryV1,
  WorkMemoryRefusalV1,
  WorkMemoryResponseV1,
  WorkMemoryV1,
} from "$shared/ipc/bindings";
import { observe } from "$shared/lib/observe";

const TIMEOUT = 9000;

/**
 * What the agent remembers about the person, as Rust keeps it: all of it for
 * Settings, or one work's for the canvas. A change shows once Rust answers.
 */
export class WorkMemorySession {
  memories = $state.raw<WorkMemoryV1[]>([]);
  loaded = $state(false);
  unavailable = $state(false);
  busy = $state(false);
  /** Why the last fact written was refused. */
  refused = $state<WorkMemoryRefusalV1 | null>(null);
  private query: WorkMemoryQueryV1;
  private generation = 0;
  private lifetime = new AbortController();
  constructor(
    readonly profile: string,
    query: Partial<WorkMemoryQueryV1> = {},
  ) {
    this.query = { query: query.query ?? null, work: query.work ?? null };
  }

  async start() {
    const generation = ++this.generation;
    this.lifetime = new AbortController();
    await this.settle(generation, () => commands.workMemories(this.profile, this.query));
  }

  /** Reads again, keeping what is shown until the answer comes. */
  refresh() {
    return this.settle(this.generation, () => commands.workMemories(this.profile, this.query));
  }

  search(text: string) {
    this.query = { ...this.query, query: text.trim() || null };
    return this.refresh();
  }

  async change(change: WorkMemoryChangeV1) {
    if (this.busy) return false;
    this.busy = true;
    const generation = this.generation;
    const ok = await this.settle(generation, () =>
      commands.workChangeMemory(this.profile, this.query, change),
    );
    if (generation === this.generation) this.busy = false;
    return ok && !this.refused;
  }

  private async settle(generation: number, request: () => Promise<WorkMemoryResponseV1>) {
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
    this.refused = response.value.refused;
    this.memories = response.value.memories;
    return true;
  }

  dispose() {
    this.generation++;
    this.lifetime.abort();
    this.busy = false;
  }
}
