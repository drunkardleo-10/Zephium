import { commands } from "$shared/ipc/bindings";
import type {
  WorkAccountEffectV1,
  WorkAccountModeV1,
  WorkContextSelectionV1,
  WorkEnvironmentCall,
  WorkEnvironmentIntent,
  WorkEnvironmentEdit,
  WorkEnvironmentSnapshot,
  WorkEnvironmentSummary,
  WorkEnvironmentView,
  WorkReplyV1,
} from "$shared/ipc/bindings";
import { commandId, validRevision } from "$domain/work";
import { events } from "$shared/ipc/native-events";
import { observe } from "$shared/lib/observe";
import { registerCloseTask } from "$shared/lib/close";
import { SvelteMap } from "svelte/reactivity";

type Pending = Extract<WorkEnvironmentCall, { kind: "command" | "checkpoint" }>;
/** Profile/Space-scoped native projections plus document-lifetime unresolved operands. */
export class WorkEnvironmentSession {
  readonly profile: string;
  readonly space: string;
  snapshot = $state.raw<WorkEnvironmentSnapshot | null>(null);
  works = $state.raw<WorkEnvironmentSummary[]>([]);
  selected = $state<string | null>(null);
  next = $state<string | null>(null);
  pending = $state.raw<Pending | null>(null);
  delivery = $state<"ready" | "pending" | "unknown" | "conflict" | "rejected">("ready");
  failure = $state<string | null>(null);
  loading = $state(false);
  tabsIntroduced = false;
  remoteView = $state.raw<{ sequence: number; view: WorkEnvironmentView } | null>(null);
  private remoteSequence = 0;
  composer = $state("");
  objectiveSubmission = $state.raw<{
    objective: string;
    command: string;
    attached: boolean;
    /** Selected context is connected to the goal card once; retries skip it. */
    related?: boolean;
    context: WorkContextSelectionV1 | null;
    /** The attached tab whose signed-in session the request should use. */
    account?: { element: string; effect: WorkAccountEffectV1; mode?: WorkAccountModeV1 } | null;
  } | null>(null);
  /** A tab chosen for signed-in work, one page or its origin; cleared when the request is sent. */
  accountScope = $state.raw<{
    element: string;
    title: string;
    origin: string;
    mode: WorkAccountModeV1;
  } | null>(null);
  objectiveToAttach = $state<string | null>(null);
  viewDraft = $state.raw<{ id: string; expected: string; view: WorkEnvironmentView } | null>(null);
  private active = false;
  private generation = 0;
  private reads = 0;
  private listing = 0;
  private stop: (() => void) | null = null;
  private invalidation: ReturnType<typeof setTimeout> | undefined;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private writing: Promise<boolean> | null = null;
  private readonly removeCloseTask: () => void;
  constructor(profile: string, space: string) {
    this.profile = profile;
    this.space = space;
    this.removeCloseTask = registerCloseTask(async () => {
      if (this.writing) await this.writing;
      if (this.pending && !(await this.retry())) return false;
      return (await this.flushView()) && !this.composer && !this.objectiveToAttach;
    });
  }
  get disposable() {
    return (
      !this.active &&
      !this.pending &&
      !this.viewDraft &&
      !this.writing &&
      !this.composer &&
      !this.objectiveToAttach
    );
  }
  async start(defaultTitle: string) {
    if (this.active) return;
    this.active = true;
    const generation = ++this.generation;
    const refresh = () => {
      clearTimeout(this.invalidation);
      this.invalidation = setTimeout(() => {
        if (this.active && generation === this.generation) {
          void this.reload();
          void this.refresh();
        }
      }, 80);
    };
    const stop = await events.workEnvironmentChanged.listen(({ payload }) => {
      if (payload.profile === this.profile) refresh();
    });
    if (!this.active || generation !== this.generation) {
      stop();
      return;
    }
    if (typeof window !== "undefined") window.addEventListener("focus", refresh);
    this.stop = () => {
      stop();
      if (typeof window !== "undefined") window.removeEventListener("focus", refresh);
    };
    if (this.pending) await this.retry();
    if (!this.active || generation !== this.generation || this.pending) return;
    const listed = await this.reload();
    if (!this.active || generation !== this.generation || !listed) return;
    if (this.selected) await this.open(this.selected);
    else {
      const work = this.works.find((work) => work.lifecycle === "active");
      if (work) await this.open(work.id);
      else await this.create(defaultTitle);
    }
  }
  stopObserving() {
    this.active = false;
    ++this.generation;
    ++this.reads;
    ++this.listing;
    clearTimeout(this.timer);
    clearTimeout(this.invalidation);
    this.stop?.();
    this.stop = null;
    void this.flushView();
    this.loading = false;
  }
  dispose() {
    this.stopObserving();
    this.removeCloseTask();
  }
  private async call(request: WorkEnvironmentCall): Promise<WorkReplyV1> {
    try {
      const result = await observe(
        Promise.resolve().then(() =>
          commands.workCall(this.profile, { kind: "environment", version: 1, request }),
        ),
        9000,
      );
      if (
        result.state !== "received" ||
        result.value.version !== 1 ||
        result.value.profile !== this.profile
      )
        return { kind: "error", error: "outcome_unknown" };
      return result.value.reply;
    } catch {
      return { kind: "error", error: "outcome_unknown" };
    }
  }
  private admit(snapshot: WorkEnvironmentSnapshot, id?: string): boolean {
    if (
      snapshot.version !== 1 ||
      snapshot.profile !== this.profile ||
      snapshot.space !== this.space ||
      (id && snapshot.id !== id) ||
      !validRevision(snapshot.revision) ||
      !validRevision(snapshot.view.revision) ||
      snapshot.elements.length > 500 ||
      snapshot.areas.length > 64
    )
      return false;
    const current = this.snapshot;
    if (
      current?.id === snapshot.id &&
      (BigInt(snapshot.revision) < BigInt(current.revision) ||
        BigInt(snapshot.view.revision) < BigInt(current.view.revision))
    )
      return false;
    if (
      current?.id === snapshot.id &&
      current.view.revision !== snapshot.view.revision &&
      !this.viewDraft &&
      !this.pending
    )
      this.publishRemoteView(snapshot.view);
    this.snapshot = snapshot;
    this.selected = snapshot.id;
    this.works = [
      {
        id: snapshot.id,
        space: snapshot.space,
        title: snapshot.title,
        lifecycle: snapshot.lifecycle,
        revision: snapshot.revision,
      },
      ...this.works.filter((work) => work.id !== snapshot.id),
    ].slice(0, 256);
    return true;
  }
  async reload(more = false): Promise<boolean> {
    const request = ++this.listing;
    const reply = await this.call({
      kind: "list",
      space: this.space,
      after: more ? this.next : null,
      limit: 32,
    });
    if (!this.active || request !== this.listing) return false;
    if (
      reply.kind === "environment" &&
      reply.reply.kind === "page" &&
      reply.reply.works.every((work) => work.space === this.space)
    ) {
      this.works = [
        ...new SvelteMap(
          [...(more ? this.works : []), ...reply.reply.works].map((work) => [work.id, work]),
        ).values(),
      ].slice(0, 256);
      this.next = reply.reply.next;
      if (!this.selected) this.selected = reply.reply.selected;
      return true;
    }
    this.failure = reply.kind === "error" ? reply.error : "outcome_unknown";
    return false;
  }
  async open(id: string): Promise<boolean> {
    if (this.pending || !(await this.flushView())) return false;
    const request = ++this.reads;
    this.loading = true;
    const reply = await this.call({ kind: "open", id });
    if (!this.active || request !== this.reads) return false;
    this.loading = false;
    if (
      reply.kind === "environment" &&
      reply.reply.kind === "snapshot" &&
      this.admit(reply.reply.snapshot, id)
    ) {
      this.failure = null;
      this.delivery = "ready";
      return true;
    }
    this.failure = reply.kind === "error" ? reply.error : "outcome_unknown";
    return false;
  }
  async refresh(): Promise<boolean> {
    const id = this.selected;
    if (!id || this.pending) return false;
    const request = ++this.reads;
    const reply = await this.call({ kind: "read", id });
    if (!this.active || request !== this.reads) return false;
    if (
      reply.kind === "environment" &&
      reply.reply.kind === "snapshot" &&
      this.admit(reply.reply.snapshot, id)
    ) {
      if (!this.viewDraft) {
        this.delivery = "ready";
        this.failure = null;
      }
      return true;
    }
    this.failure = reply.kind === "error" ? reply.error : "outcome_unknown";
    return false;
  }
  async discardView() {
    if (this.pending || this.writing) return false;
    this.viewDraft = null;
    this.delivery = "ready";
    const okay = await this.refresh();
    if (okay && this.snapshot) this.publishRemoteView(this.snapshot.view);
    return okay;
  }
  private publishRemoteView(view: WorkEnvironmentView) {
    this.remoteView = { sequence: ++this.remoteSequence, view };
  }
  async create(title: string) {
    if (!(await this.flushView())) return false;
    return this.mutate({ kind: "create", space: this.space, title });
  }
  async edit(edit: WorkEnvironmentEdit) {
    if (!(await this.flushView()) || !this.snapshot) return false;
    return this.mutate({
      kind: "edit",
      id: this.snapshot.id,
      expected: this.snapshot.revision,
      edit,
    });
  }
  private mutate(intent: WorkEnvironmentIntent) {
    if (this.pending || this.writing) return Promise.resolve(false);
    this.pending = { kind: "command", command: commandId(), intent };
    return this.retry();
  }
  retry(): Promise<boolean> {
    if (this.writing) return this.writing;
    const pending = this.pending;
    if (!pending) return Promise.resolve(true);
    this.delivery = "pending";
    const task = this.settle(pending);
    this.writing = task;
    void task.finally(() => {
      if (this.writing === task) this.writing = null;
    });
    return task;
  }
  private async settle(pending: Pending): Promise<boolean> {
    const reply = await this.call(pending);
    if (this.pending !== pending) return false;
    if (
      pending.kind === "checkpoint" &&
      reply.kind === "environment" &&
      reply.reply.kind === "checkpointed"
    ) {
      const receipt = reply.reply;
      if (
        receipt.expected === pending.expected &&
        validRevision(receipt.expected) &&
        validRevision(receipt.applied_view_revision) &&
        validRevision(receipt.snapshot.view.revision) &&
        BigInt(receipt.applied_view_revision) === BigInt(receipt.expected) + 1n &&
        BigInt(receipt.snapshot.view.revision) >= BigInt(receipt.applied_view_revision) &&
        this.admit(receipt.snapshot, pending.id)
      ) {
        const draft = this.viewDraft;
        const advanced = receipt.snapshot.view.revision !== receipt.applied_view_revision;
        this.pending = null;
        if (draft?.view === pending.view) {
          this.viewDraft = null;
          if (advanced) this.publishRemoteView(receipt.snapshot.view);
        } else if (draft?.id === pending.id) {
          // New local gestures were based on this checkpoint. A later remote view
          // cannot be silently replaced merely because its earlier receipt replayed.
          this.viewDraft = {
            ...draft,
            expected: receipt.applied_view_revision,
            view: { ...draft.view, revision: receipt.applied_view_revision },
          };
          if (advanced) {
            this.delivery = "conflict";
            this.failure = "conflict";
            return true;
          }
        }
        this.delivery = "ready";
        this.failure = null;
        this.scheduleView();
        return true;
      }
    }
    if (
      pending.kind === "command" &&
      reply.kind === "environment" &&
      reply.reply.kind === "applied" &&
      reply.reply.command === pending.command &&
      validRevision(reply.reply.applied_revision) &&
      validRevision(reply.reply.applied_view_revision) &&
      validRevision(reply.reply.snapshot.revision) &&
      validRevision(reply.reply.snapshot.view.revision) &&
      BigInt(reply.reply.applied_revision) <= BigInt(reply.reply.snapshot.revision) &&
      BigInt(reply.reply.applied_view_revision) <= BigInt(reply.reply.snapshot.view.revision) &&
      this.admit(
        reply.reply.snapshot,
        pending.intent.kind === "create" ? undefined : pending.intent.id,
      )
    ) {
      this.pending = null;
      if (this.viewDraft && this.viewDraft.expected !== reply.reply.snapshot.view.revision) {
        this.delivery = "conflict";
        this.failure = "conflict";
      } else {
        this.delivery = "ready";
        this.failure = null;
        this.scheduleView();
      }
      return true;
    }
    const error = reply.kind === "error" ? reply.error : "outcome_unknown";
    this.failure = error;
    this.delivery =
      error === "conflict" ? "conflict" : error === "outcome_unknown" ? "unknown" : "rejected";
    if (this.delivery !== "unknown") this.pending = null;
    return false;
  }
  private scheduleView() {
    clearTimeout(this.timer);
    if (this.active && this.delivery === "ready" && this.viewDraft)
      this.timer = setTimeout(() => {
        void this.flushView();
      }, 750);
  }
  checkpoint(view: WorkEnvironmentView) {
    const snapshot = this.snapshot;
    if (!snapshot || snapshot.lifecycle !== "active") return;
    const expected =
      this.viewDraft?.id === snapshot.id ? this.viewDraft.expected : snapshot.view.revision;
    this.viewDraft = { id: snapshot.id, expected, view: { ...view, revision: expected } };
    this.scheduleView();
  }
  async flushView(): Promise<boolean> {
    clearTimeout(this.timer);
    if (this.writing && !(await this.writing)) return false;
    if (this.pending) return false;
    const draft = this.viewDraft;
    if (!draft) return true;
    if (this.delivery === "conflict" || this.delivery === "unknown") return false;
    this.pending = { kind: "checkpoint", id: draft.id, expected: draft.expected, view: draft.view };
    const okay = await this.retry();
    if (okay && this.viewDraft && this.viewDraft !== draft && this.delivery === "ready")
      return this.flushView();
    return okay && !this.viewDraft;
  }
}
const sessions = new SvelteMap<string, WorkEnvironmentSession>();
export function environmentSession(profile: string, space: string): WorkEnvironmentSession | null {
  const key = `${profile}:${space}`;
  const existing = sessions.get(key);
  if (existing) return existing;
  if (sessions.size >= 16) {
    const candidate = [...sessions].find(([, session]) => session.disposable);
    if (!candidate) return null;
    candidate[1].dispose();
    sessions.delete(candidate[0]);
  }
  const session = new WorkEnvironmentSession(profile, space);
  sessions.set(key, session);
  return session;
}
