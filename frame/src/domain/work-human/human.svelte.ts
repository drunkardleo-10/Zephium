import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { commands } from "$shared/ipc/bindings";
import type {
  WorkFailureV1,
  WorkHumanAccountV1,
  WorkHumanPageIdV1,
  WorkHumanPageV1,
  WorkHumanRegionV1,
  WorkHumanResponseV1,
} from "$shared/ipc/bindings";
import { events } from "$shared/ipc/native-events";
import { observe } from "$shared/lib/observe";

export type WorkHumanFailure = WorkFailureV1 | "transport";
/** A page still held for a person; `released` and `reading` are settled states. */
const held = (page: WorkHumanPageV1) => page.phase !== "reading" && page.phase !== "released";
const BEAT = 1000;

/**
 * The pages each attached work is holding open for a person. Rust computes the
 * remaining wait on every read, so this refetches on invalidation and on a beat
 * while a page is held instead of ticking a stale number down.
 */
export class WorkHumanSession {
  readonly pages = new SvelteMap<string, WorkHumanPageV1[]>();
  private works: string[] = [];
  private active = false;
  private generation = 0;
  private stop: (() => void) | null = null;
  private lifetime = new AbortController();
  private reading = new SvelteSet<string>();
  private again = new SvelteSet<string>();
  private beat: ReturnType<typeof setInterval> | undefined;
  constructor(readonly profile: string) {}
  /** Subscribe before the first read: an invalidation in between is never lost. */
  async start() {
    if (this.active) return;
    this.active = true;
    const generation = ++this.generation;
    this.lifetime = new AbortController();
    const stop = await events.workHumanChanged.listen(({ payload }) => {
      if (payload.profile === this.profile && this.works.includes(payload.work))
        this.refresh(payload.work);
    });
    if (!this.active || generation !== this.generation) {
      stop();
      return;
    }
    this.stop = stop;
    this.beat = setInterval(() => {
      for (const work of this.works)
        if ((this.pages.get(work) ?? []).some(held)) this.refresh(work);
    }, BEAT);
    for (const work of this.works) this.refresh(work);
  }
  /** The works on this canvas; pages of a work that left are dropped with it. */
  update(works: readonly string[]) {
    if (works.join(":") === this.works.join(":")) return;
    const wanted = [...works];
    const added = wanted.filter((work) => !this.works.includes(work));
    this.works = wanted;
    for (const work of [...this.pages.keys()]) if (!wanted.includes(work)) this.pages.delete(work);
    if (this.active) for (const work of added) this.refresh(work);
  }
  private refresh(work: string) {
    if (!this.active) return;
    if (this.reading.has(work)) {
      this.again.add(work);
      return;
    }
    this.reading.add(work);
    const generation = this.generation;
    void this.read(work, generation).finally(() => {
      this.reading.delete(work);
      if (this.again.delete(work) && this.works.includes(work)) this.refresh(work);
    });
  }
  private async read(work: string, generation: number) {
    const response = await observe(
      Promise.resolve().then(() => commands.workHumanPages(this.profile, work)),
      9000,
      this.lifetime.signal,
    );
    if (response.state !== "received") return;
    this.adopt(work, generation, response.value);
  }
  private adopt(work: string, generation: number, response: WorkHumanResponseV1) {
    if (!this.active || generation !== this.generation || !this.works.includes(work)) return;
    if (
      response.version !== 1 ||
      response.profile !== this.profile ||
      response.work !== work ||
      response.error
    )
      return;
    this.pages.set(work, response.pages ?? []);
  }
  /** Asks Rust to put the agent's own view inside `region`; acceptance is not presentation. */
  present(work: string, page: WorkHumanPageIdV1, region: WorkHumanRegionV1) {
    return this.act(work, commands.workHumanPresent(this.profile, work, page, region));
  }
  /** The person's explicit account attestation; the read resumes under it. */
  continue(work: string, page: WorkHumanPageIdV1, account: WorkHumanAccountV1) {
    return this.act(work, commands.workHumanContinue(this.profile, work, page, account));
  }
  release(work: string, page: WorkHumanPageIdV1) {
    return this.act(work, commands.workHumanRelease(this.profile, work, page));
  }
  private async act(
    work: string,
    request: Promise<WorkHumanResponseV1>,
  ): Promise<WorkHumanFailure | null> {
    const generation = this.generation;
    const response = await observe(request, 9000, this.lifetime.signal);
    if (response.state !== "received") return "transport";
    this.adopt(work, generation, response.value);
    return response.value.error ?? (response.value.accepted ? null : "conflict");
  }
  dispose() {
    this.active = false;
    this.generation++;
    clearInterval(this.beat);
    this.beat = undefined;
    this.stop?.();
    this.stop = null;
    this.lifetime.abort();
    this.reading.clear();
    this.again.clear();
    this.pages.clear();
    this.works = [];
  }
}
