import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { commands } from "$shared/ipc/bindings";
import type {
  WorkEnvironmentSnapshot,
  WorkRuntimeProjection,
  WorkPlanRevision,
  ResourceSummary,
  WorkPageV1,
  MediaAssetV1_Deserialize as MediaAssetV1,
} from "$shared/ipc/bindings";
import { events } from "$shared/ipc/native-events";
import { observe } from "$shared/lib/observe";
import { listenAll } from "$shared/lib/lifecycle";
import { validRevision } from "$domain/work";

/** Read-only joins for visible Work resources. No model or operation supervisor is started. */
export class WorkEnvironmentContext {
  readonly objectives = new SvelteMap<string, WorkRuntimeProjection>();
  readonly plans = new SvelteMap<string, WorkPlanRevision>();
  /**
   * The pages each attached work opened, with the last frame recorded for
   * every one. A page card is drawn from the run's steps, which arrive with
   * the projection; without this its frame would wait for whichever work the
   * Work session happens to be on.
   */
  readonly pages = new SvelteMap<string, WorkPageV1[]>();
  notes = $state.raw<ResourceSummary[]>([]);
  /** Admitted media assets for resource elements that are not notes. */
  readonly media = new SvelteMap<string, MediaAssetV1>();
  readonly unavailable = new SvelteMap<string, boolean>();
  private wanted: string[] = [];
  private noteIds: string[] = [];
  private pending = new SvelteSet<string>();
  private workers = 0;
  private inFlight = new SvelteSet<string>();
  private notesTask: Promise<void> | null = null;
  private notesAgain = false;
  private active = false;
  private generation = 0;
  private noteRead = 0;
  private stop: (() => void) | null = null;
  private lifetime = new AbortController();
  constructor(readonly profile: string) {}
  async start() {
    if (this.active) return;
    this.active = true;
    const generation = ++this.generation;
    this.lifetime = new AbortController();
    const stops = await listenAll([
      events.workChanged.listen(({ payload }) => {
        if (payload.profile === this.profile && this.wanted.includes(payload.work)) {
          this.pending.add(payload.work);
          this.pump();
        }
      }),
      events.resourceChanged.listen(({ payload }) => {
        if (payload.profile === this.profile && this.noteIds.includes(payload.id))
          void this.readNotes();
      }),
    ]);
    if (!this.active || generation !== this.generation) {
      stops.forEach((stop) => stop());
      return;
    }
    this.stop = () => stops.forEach((stop) => stop());
    this.pump();
    if (this.noteIds.length) void this.readNotes();
  }
  update(snapshot: WorkEnvironmentSnapshot, focused?: string) {
    const ordered = [...snapshot.elements].sort(
      (a, b) => Number(b.id === focused) - Number(a.id === focused),
    );
    const wanted = [
      ...new SvelteSet(
        ordered.flatMap((element) =>
          "objective" in element.reference ? [element.reference.objective] : [],
        ),
      ),
    ].slice(0, 32);
    this.wanted = wanted;
    for (const id of [...this.objectives.keys()])
      if (!wanted.includes(id)) {
        this.objectives.delete(id);
        this.plans.delete(id);
        this.pages.delete(id);
      }
    for (const id of [...this.unavailable.keys()])
      if (!wanted.includes(id)) this.unavailable.delete(id);
    for (const id of [...this.pending]) if (!wanted.includes(id)) this.pending.delete(id);
    for (const id of wanted)
      if (!this.objectives.has(id) && !this.unavailable.has(id) && !this.inFlight.has(id))
        this.pending.add(id);
    const noteIds = snapshot.elements.flatMap((element) =>
      element.reference.kind === "resource" ? [element.reference.resource] : [],
    );
    if (noteIds.join(":") !== this.noteIds.join(":")) {
      this.noteIds = noteIds;
      if (this.active) void this.readNotes();
    }
    this.pump();
  }
  private pump() {
    if (!this.active) return;
    while (this.workers < 2 && this.pending.size) {
      const id = this.pending.values().next().value;
      if (!id) break;
      this.pending.delete(id);
      this.workers++;
      this.inFlight.add(id);
      const generation = this.generation;
      void this.readObjective(id, generation).finally(() => {
        this.workers--;
        this.inFlight.delete(id);
        this.pump();
      });
    }
  }
  private async readObjective(id: string, generation: number) {
    const response = await observe(
      Promise.resolve().then(() =>
        commands.workCall(this.profile, {
          kind: "query",
          request: { version: 1, query: { kind: "projection", work: id } },
        }),
      ),
      9000,
      this.lifetime.signal,
    );
    if (!this.active || generation !== this.generation || !this.wanted.includes(id)) return;
    if (
      response.state === "received" &&
      response.value.version === 1 &&
      response.value.profile === this.profile &&
      response.value.reply.kind === "projection"
    ) {
      const next = response.value.reply.projection;
      const current = this.objectives.get(id);
      if (
        next.version === 1 &&
        next.work.id === id &&
        next.work.profile === this.profile &&
        validRevision(next.work.revision) &&
        next.executions.length <= 16 &&
        (!current || BigInt(next.work.revision) >= BigInt(current.work.revision))
      ) {
        this.objectives.set(id, next);
        this.unavailable.delete(id);
        if (next.executions.length) await this.readPages(id, generation);
        if (!this.active || generation !== this.generation || !this.wanted.includes(id)) return;
        const revision = next.executions.at(-1)?.spec.plan_revision ?? next.work.plan?.revision;
        if (revision && next.work.plan?.revision === revision) this.plans.set(id, next.work.plan);
        else if (revision && this.plans.get(id)?.revision !== revision) {
          const historical = await observe(
            Promise.resolve().then(() =>
              commands.workCall(this.profile, {
                kind: "query",
                request: { version: 1, query: { kind: "plan", work: id, revision } },
              }),
            ),
            9000,
            this.lifetime.signal,
          );
          if (!this.active || generation !== this.generation || !this.wanted.includes(id)) return;
          if (
            historical.state === "received" &&
            historical.value.version === 1 &&
            historical.value.profile === this.profile &&
            historical.value.reply.kind === "plan" &&
            historical.value.reply.plan.revision === revision
          )
            this.plans.set(id, historical.value.reply.plan);
        }
        return;
      }
    }
    this.unavailable.set(id, true);
  }
  /** The frames one work recorded, so its page cards are pictures on arrival. */
  private async readPages(id: string, generation: number) {
    const response = await observe(
      Promise.resolve().then(() => commands.workActivity(this.profile, id)),
      9000,
      this.lifetime.signal,
    );
    if (!this.active || generation !== this.generation || !this.wanted.includes(id)) return;
    if (
      response.state === "received" &&
      response.value.version === 1 &&
      response.value.profile === this.profile &&
      response.value.work === id &&
      !response.value.error
    )
      this.pages.set(id, response.value.pages ?? []);
  }
  private readNotes(): Promise<void> {
    if (this.notesTask) {
      this.notesAgain = true;
      ++this.noteRead;
      return this.notesTask;
    }
    const task = this.fetchNotes();
    this.notesTask = task;
    void task.finally(() => {
      if (this.notesTask === task) this.notesTask = null;
      if (this.active && this.notesAgain) {
        this.notesAgain = false;
        void this.readNotes();
      }
    });
    return task;
  }
  private async fetchNotes() {
    const read = ++this.noteRead;
    const ids = [...this.noteIds];
    const rows: ResourceSummary[] = [];
    for (let offset = 0; offset < ids.length; offset += 64) {
      const response = await observe(
        Promise.resolve().then(() =>
          commands.resourceCall(this.profile, {
            kind: "resolve_notes",
            ids: ids.slice(offset, offset + 64),
          }),
        ),
        9000,
        this.lifetime.signal,
      );
      if (!this.active || read !== this.noteRead) return;
      if (
        response.state === "received" &&
        response.value.profile === this.profile &&
        response.value.response.kind === "page"
      )
        rows.push(...response.value.response.items.filter((item) => ids.includes(item.id)));
    }
    if (!this.active || read !== this.noteRead) return;
    this.notes = rows;
    for (const id of [...this.media.keys()]) if (!ids.includes(id)) this.media.delete(id);
    const others = ids.filter((id) => !rows.some((row) => row.id === id)).slice(0, 32);
    for (const id of others) {
      if (this.media.has(id)) continue;
      const response = await observe(
        Promise.resolve().then(() => commands.resourceCall(this.profile, { kind: "get", id })),
        9000,
        this.lifetime.signal,
      );
      if (!this.active || read !== this.noteRead) return;
      if (
        response.state === "received" &&
        response.value.profile === this.profile &&
        response.value.response.kind === "record" &&
        response.value.response.record.draft.content.kind === "media" &&
        !response.value.response.record.trashed
      )
        this.media.set(id, response.value.response.record.draft.content.asset);
    }
  }
  dispose() {
    this.active = false;
    this.generation++;
    this.noteRead++;
    this.stop?.();
    this.stop = null;
    this.lifetime.abort();
    this.pending.clear();
    this.objectives.clear();
    this.media.clear();
    this.plans.clear();
    this.pages.clear();
    this.unavailable.clear();
    this.notes = [];
  }
}
