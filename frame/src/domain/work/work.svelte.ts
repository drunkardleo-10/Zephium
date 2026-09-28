import { commands } from "$shared/ipc/bindings";
import type {
  WorkCallV1,
  WorkReplyV1,
  WorkRuntimeProjection,
  WorkSummary,
  WorkUserEdit,
  WorkRuntimeIntent,
  WorkPlanRevision,
  WorkEvidenceLink,
  WorkEvidencePreviewV1,
  WorkPlanProposal,
  WorkSignalV1,
  WorkPageV1,
  WorkArtifactDataV1,
  WorkContextSelectionV1,
} from "$shared/ipc/bindings";
import { events } from "$shared/ipc/native-events";
import { observe } from "$shared/lib/observe";
import { registerCloseTask } from "$shared/lib/close";
import { SvelteMap } from "svelte/reactivity";
import {
  admitProjection,
  commandId,
  validRevision,
  planProposal,
  AGENT_GRANT,
  AGENT_LIMITS,
} from "./work-model";
import { currentActivity } from "./work-activity";
import { WorkOperations } from "./work-operations.svelte";

export type WorkDelivery =
  "ready" | "pending" | "rejected" | "conflict" | "unknown" | "resynchronizing";
type Mutation = Extract<WorkCallV1, { kind: "author" | "execute" }>;
type DraftField = "objective" | `question:${string}`;
type ArtifactDraft = {
  basis: string;
  execution: string;
  data: WorkArtifactDataV1;
  evidence: WorkEvidenceLink[];
};
type TextDraft = { basis: string; text: string };

/** Profile-scoped Rust projections. Pending operands survive view unmount. */
export class WorkSession {
  readonly operations: WorkOperations;
  readonly profile: string;
  works = $state.raw<WorkSummary[]>([]);
  next = $state<string | null>(null);
  selected = $state<string | null>(null);
  projection = $state.raw<WorkRuntimeProjection | null>(null);
  delivery = $state<WorkDelivery>("ready");
  failure = $state<string | null>(null);
  loading = $state(false);
  pending = $state.raw<Mutation | null>(null);
  composer = $state("");
  activity = $state.raw<WorkSignalV1[]>([]);
  /** Pages the latest runs opened, with their newest frames; kept after a run ends. */
  pages = $state.raw<WorkPageV1[]>([]);
  /** Folders the canvas grants to the runs it starts; empty until a person grants one. */
  folders = $state.raw<string[]>([]);
  /** Messages typed while a run was live; each is sent on as the run before it ends. */
  queue = $state.raw<string[]>([]);
  /** Requests waiting on an origin grant, by work: the context rides with the run once allowed. */
  private activityRefresh: ReturnType<typeof setTimeout> | undefined;
  private readonly artifacts = new SvelteMap<string, ArtifactDraft>();
  private readonly drafts = new SvelteMap<string, TextDraft>();
  private readonly plans = new SvelteMap<string, { basis: string; proposal: WorkPlanProposal }>();
  private active = false;
  private generation = 0;
  private reads = 0;
  private listing = 0;
  private stop: (() => void) | null = null;
  private refresh: ReturnType<typeof setTimeout> | undefined;
  private writing: Promise<boolean> | null = null;
  private lifetime = new AbortController();
  private readonly removeCloseTask: () => void;

  constructor(profile: string) {
    this.profile = profile;
    this.operations = new WorkOperations(profile, async (work) => {
      if (!this.active) return;
      await this.reload();
      if (this.selected === work) await this.open(work);
    });
    this.removeCloseTask = registerCloseTask(async () => {
      if (this.writing) await this.writing;
      const settled = this.pending ? await this.retry() : true;
      return (
        settled &&
        !this.composer &&
        this.drafts.size === 0 &&
        this.plans.size === 0 &&
        this.artifacts.size === 0
      );
    });
  }
  async start() {
    if (this.active) return;
    this.active = true;
    const generation = ++this.generation;
    this.lifetime = new AbortController();
    const stop = await events.workChanged.listen(({ payload }) => {
      if (generation !== this.generation || payload.profile !== this.profile) return;
      clearTimeout(this.refresh);
      this.refresh = setTimeout(() => {
        void this.reload();
        if (this.selected) void this.open(this.selected);
      }, 100);
    });
    if (!this.active || generation !== this.generation) {
      stop();
      return;
    }
    this.stop = stop;
    await this.reload();
    if (this.selected) await this.open(this.selected);
    this.operations.start();
  }
  stopObserving() {
    this.operations.stop();
    this.active = false;
    ++this.generation;
    ++this.reads;
    ++this.listing;
    this.lifetime.abort();
    this.stop?.();
    this.stop = null;
    clearTimeout(this.refresh);
    clearTimeout(this.activityRefresh);
    // Pages survive: the workspace remounts on every return to Work.
    this.activity = [];
    this.works = [];
    this.projection = null;
    this.loading = false;
    if (this.pending) this.delivery = "unknown";
  }
  dispose() {
    this.stopObserving();
    this.removeCloseTask();
  }

  private async call(call: WorkCallV1, signal?: AbortSignal): Promise<WorkReplyV1> {
    try {
      const response = await observe(
        Promise.resolve().then(() => commands.workCall(this.profile, call)),
        9_000,
        signal,
      );
      if (response.state !== "received") return { kind: "error", error: "outcome_unknown" };
      if (response.value.version !== 1 || response.value.profile !== this.profile)
        return { kind: "error", error: "outcome_unknown" };
      return response.value.reply;
    } catch {
      return { kind: "error", error: "outcome_unknown" };
    }
  }
  async reload(more = false) {
    const generation = ++this.listing;
    const result = await this.call(
      {
        kind: "query",
        request: { version: 1, query: { kind: "list", after: more ? this.next : null, limit: 32 } },
      },
      this.lifetime.signal,
    );
    if (generation !== this.listing || !this.active) return;
    if (result.kind === "page") {
      this.works = [
        ...new SvelteMap(
          [...(more ? this.works : []), ...result.works].map((w) => [w.id, w]),
        ).values(),
      ];
      this.next = result.next;
    } else if (result.kind === "error") this.failure = result.error;
  }
  async open(work: string): Promise<boolean> {
    const generation = ++this.reads;
    clearTimeout(this.activityRefresh);
    if (work !== this.selected) {
      this.selected = work;
      this.projection = null;
      this.pages = [];
      if (!this.pending) {
        this.delivery = "ready";
        this.failure = null;
      }
    }
    this.loading = true;
    const result = await this.call(
      { kind: "query", request: { version: 1, query: { kind: "projection", work } } },
      this.lifetime.signal,
    );
    if (generation !== this.reads || !this.active || this.selected !== work) return false;
    this.loading = false;
    if (
      result.kind === "projection" &&
      admitProjection(result.projection, this.profile, work, this.projection)
    ) {
      this.projection = result.projection;
      this.activity = currentActivity(result.projection, this.activity);
      // Pages and their frames outlive the run that opened them, and the
      // workspace remounts whenever Work is left: read them on every open,
      // not only while an execution is live.
      if (result.projection.executions.length) {
        void this.readActivity(result.projection, generation);
        if (
          result.projection.executions.some(
            (entry) =>
              ["running", "cancel_requested"].includes(entry.status) &&
              !result.projection.interrupted.includes(entry.id),
          )
        )
          this.activityRefresh = setTimeout(() => {
            if (this.active && this.selected === work) void this.open(work);
          }, 1_000);
      }
      if (this.delivery === "ready") this.failure = null;
      return true;
    }
    if (result.kind === "error") this.failure = result.error;
    else this.failure = "unsupported_projection";
    return false;
  }
  private async readActivity(state: WorkRuntimeProjection, generation: number) {
    try {
      const result = await observe(
        Promise.resolve().then(() => commands.workActivity(this.profile, state.work.id)),
        9_000,
        this.lifetime.signal,
      );
      if (!this.active || this.selected !== state.work.id) return;
      const valid =
        result.state === "received" &&
        result.value.version === 1 &&
        result.value.profile === this.profile &&
        result.value.work === state.work.id &&
        !result.value.error;
      // Pages answer for the work, not for one read of it: an open that
      // overtook this one must not throw away frames it did not ask for.
      if (valid) this.pages = result.value.pages ?? [];
      if (generation !== this.reads) return;
      this.activity = valid ? currentActivity(state, result.value.signals) : [];
    } catch {
      if (generation === this.reads) this.activity = [];
    }
  }
  back() {
    ++this.reads;
    this.selected = null;
    this.projection = null;
    this.loading = false;
  }
  async reconcile() {
    await this.operations.reconcile();
    this.delivery = "resynchronizing";
    if (this.pending) return this.retry();
    await this.reload();
    const okay = this.selected ? await this.open(this.selected) : true;
    this.delivery = okay ? "ready" : "unknown";
    return okay;
  }
  create(objective: string, command = commandId()) {
    return this.mutate({
      kind: "author",
      command: { version: 1, command, intent: { kind: "create", objective } },
    });
  }
  draft(field: DraftField): string | undefined {
    return this.drafts.get(`${this.selected}:${field}`)?.text;
  }
  get canRelease() {
    return (
      !this.pending &&
      !this.composer &&
      !this.selected &&
      this.drafts.size === 0 &&
      this.plans.size === 0 &&
      this.artifacts.size === 0
    );
  }
  get hasDrafts() {
    return (
      !!this.selected &&
      (this.plans.has(this.selected) ||
        [...this.artifacts.keys()].some((key) => key.startsWith(`${this.selected}:`)) ||
        [...this.drafts.keys()].some((key) => key.startsWith(`${this.selected}:`)))
    );
  }
  get planDraft() {
    return this.selected ? this.plans.get(this.selected) : undefined;
  }
  editPlan(proposal?: WorkPlanProposal) {
    const work = this.projection?.work;
    if (!work) return;
    const original =
      proposal ??
      (work.plan
        ? planProposal(work.plan)
        : {
            nodes: [
              {
                key: 0,
                objective: work.objective,
                dependencies: [],
                outputs: [
                  {
                    name: "Result",
                    description: work.objective,
                    review: "source_mapped_needs_review" as const,
                  },
                ],
              },
            ],
          });
    this.plans.set(work.id, {
      basis: this.plans.get(work.id)?.basis ?? work.revision,
      proposal: original,
    });
  }
  savePlan() {
    const draft = this.planDraft;
    return draft
      ? this.edit({ kind: "replace_draft", proposal: draft.proposal }, draft.basis)
      : Promise.resolve(false);
  }
  discardDrafts() {
    if (this.pending) return;
    const work = this.selected;
    if (!work) return;
    this.plans.delete(work);
    for (const key of this.artifacts.keys())
      if (key.startsWith(`${work}:`)) this.artifacts.delete(key);
    for (const key of this.drafts.keys()) {
      if (key.startsWith(`${work}:`)) this.drafts.delete(key);
    }
    if (this.delivery === "conflict" || this.delivery === "rejected") {
      this.delivery = "ready";
      this.failure = null;
    }
  }
  artifactDraft(artifact: string) {
    return this.artifacts.get(`${this.selected}:${artifact}`);
  }
  editArtifact(execution: string, artifact: string, data?: WorkArtifactDataV1) {
    const work = this.projection?.work;
    const fact = this.projection?.executions.find((item) => item.id === execution);
    const original = fact?.artifacts.find((item) => item.id === artifact);
    if (!work || !fact || !original || !["needs_review", "completed"].includes(fact.status)) return;
    const key = `${work.id}:${artifact}`;
    const previous = this.artifacts.get(key);
    const user = fact.user_artifacts?.find((item) => item.artifact === artifact);
    this.artifacts.set(key, {
      basis: previous?.basis ?? work.revision,
      execution,
      data: structuredClone(data ?? previous?.data ?? user?.edited_data ?? original.data),
      evidence: previous?.evidence ?? (user?.edited_data ? user.evidence : original.evidence),
    });
  }
  discardArtifact(artifact: string) {
    if (!this.pending) this.artifacts.delete(`${this.selected}:${artifact}`);
  }
  saveArtifact(artifact: string) {
    const draft = this.artifactDraft(artifact);
    return draft
      ? this.execute(
          {
            kind: "edit_artifact",
            execution: draft.execution,
            artifact,
            data: draft.data,
            evidence: draft.evidence,
          },
          draft.basis,
        )
      : Promise.resolve(false);
  }
  setDraft(field: DraftField, text: string) {
    const work = this.projection?.work;
    if (!work) return;
    const key = `${work.id}:${field}`;
    const previous = this.drafts.get(key);
    const original = field === "objective" ? work.objective : "";
    if (text === original) this.drafts.delete(key);
    else this.drafts.set(key, { basis: previous?.basis ?? work.revision, text });
  }
  async saveDraft(field: DraftField, text: string) {
    const work = this.projection?.work;
    if (!work) return false;
    const key = `${work.id}:${field}`;
    const draft = this.drafts.get(key);
    if (!draft || draft.text.trim() !== text) return false;
    const saved = await this.edit(
      field === "objective"
        ? { kind: "set_objective", objective: text }
        : { kind: "answer_question", id: field.slice(9), answer: text },
      draft.basis,
    );
    if (saved && this.drafts.get(key) === draft) this.drafts.delete(key);
    return saved;
  }
  edit(edit: WorkUserEdit, expected?: string) {
    const work = this.projection?.work;
    if (!work) return Promise.resolve(false);
    return this.mutate({
      kind: "author",
      command: {
        version: 1,
        command: commandId(),
        intent: { kind: "edit", work: work.id, expected_revision: expected ?? work.revision, edit },
      },
    });
  }
  async readPublic(context: WorkContextSelectionV1 | null = null) {
    const work = this.projection?.work;
    if (!work || this.pending || this.operations.busy(work.id)) return;
    await this.operations.begin({
      kind: "read_public",
      ...(context ? { context } : {}),
      command: {
        version: 1,
        work: work.id,
        expected_revision: work.revision,
        command: commandId(),
        intent: {
          kind: "read_public",
          scope: { provider: "open_ai", model: "gpt-6-luna", query: work.objective },
          limits: {
            model_tokens: 147456,
            cost_micro_usd: 100000,
            operations: 1,
            timeout_seconds: 180,
            max_workers: 1,
          },
        },
      },
    });
  }
  /**
   * The routine loop: sending the objective runs the agent in the person's
   * own sessions; a site it has not worked on yet asks first, on the canvas.
   */
  async run(context: WorkContextSelectionV1 | null = null, options: { private?: boolean } = {}) {
    if (!this.projection || this.pending || this.operations.busy(this.projection.work.id)) return;
    // What an earlier launch left running is acknowledged first: sending the
    // next request moves on from it, and the work cannot run while it stands.
    for (const execution of [...this.projection.interrupted])
      if (!(await this.execute({ kind: "acknowledge_interruption", execution }))) return;
    const work = this.projection?.work;
    if (!work || this.pending || this.operations.busy(work.id)) return;
    await this.operations.begin({
      kind: "run",
      ...(context ? { context } : {}),
      command: {
        version: 1,
        work: work.id,
        expected_revision: work.revision,
        command: commandId(),
        intent: {
          kind: "begin_agent",
          grant: {
            ...AGENT_GRANT,
            ...(this.folders.length ? { folders: this.folders } : {}),
            ...(options.private ? { private: true } : {}),
          },
          limits: AGENT_LIMITS,
        },
      },
    });
  }
  /**
   * The person's next message on the same work: it becomes the live request and
   * starts a fresh run over everything the work already established.
   */
  async continueWith(
    text: string,
    context: WorkContextSelectionV1 | null = null,
    options: { private?: boolean } = {},
  ): Promise<boolean> {
    const message = text.trim();
    if (!message || !this.projection?.work) return false;
    if (!(await this.edit({ kind: "set_objective", objective: message }))) return false;
    await this.run(context, options);
    return true;
  }
  /** Up to three next requests the finished run offers; empty while it works. */
  get followups(): readonly string[] {
    const steps = this.projection?.executions.at(-1)?.steps ?? [];
    for (let index = steps.length - 1; index >= 0; index--) {
      const kind = steps[index]!.kind;
      if (kind.kind === "finish") return kind.followups ?? [];
    }
    return [];
  }
  /** The live execution a person can still speak to. */
  private get running() {
    return this.projection?.executions.find(
      (entry) =>
        ["running", "approved"].includes(entry.status) &&
        !this.projection?.interrupted.includes(entry.id),
    );
  }
  /** Hands a message to the agent at its next move; refused when no run is live. */
  async steer(text: string): Promise<boolean> {
    const message = text.trim();
    const execution = this.running;
    if (!message || !execution) return false;
    return this.execute({ kind: "steer", execution: execution.id, text: message });
  }
  /** Holds a message until the run in flight finishes. */
  enqueue(text: string) {
    const message = text.trim();
    if (message && this.queue.length < 16) this.queue = [...this.queue, message];
  }
  /** Sends the oldest queued message on as a continuation; the rest wait their turn. */
  async sendQueued(): Promise<boolean> {
    const [next, ...rest] = this.queue;
    if (!next) return false;
    this.queue = rest;
    return this.continueWith(next);
  }
  /** Accepts or refuses one proposed file change the running agent is waiting on. */
  approveStep(step: string, approve: boolean): Promise<boolean> {
    const execution = this.running;
    if (!execution) return Promise.resolve(false);
    return this.execute({ kind: "approve_step", execution: execution.id, step, approve });
  }
  answerStep(execution: string, step: string, answer: string) {
    return this.execute({ kind: "answer_step", execution, step, answer });
  }
  execute(
    intent: Exclude<WorkRuntimeIntent, { kind: "read_public" | "begin_agent" }>,
    expected?: string,
  ) {
    const work = this.projection?.work;
    if (!work) return Promise.resolve(false);
    return this.mutate({
      kind: "execute",
      command: {
        version: 1,
        work: work.id,
        expected_revision: expected ?? work.revision,
        command: commandId(),
        intent,
      },
    });
  }
  private mutate(call: Mutation) {
    if (this.pending || this.writing) return Promise.resolve(false);
    this.pending = call;
    return this.retry();
  }
  retry(): Promise<boolean> {
    if (this.writing) return this.writing;
    const call = this.pending;
    if (!call) return Promise.resolve(true);
    this.delivery = "pending";
    const write = this.settle(call);
    this.writing = write;
    void write.then(() => {
      if (this.writing === write) this.writing = null;
    });
    return write;
  }
  private async settle(call: Mutation): Promise<boolean> {
    // Observation disposal cannot cancel a submitted durable mutation.
    const reply = await this.call(call);
    if (this.pending !== call) return false;
    const receipt =
      reply.kind === "authoring_applied" || reply.kind === "execution_applied"
        ? reply.receipt
        : null;
    const matching =
      receipt &&
      receipt.command === call.command.command &&
      validRevision(receipt.applied_revision) &&
      (call.kind === "author" && reply.kind === "authoring_applied"
        ? (call.command.intent.kind === "create" ||
            reply.receipt.work === call.command.intent.work) &&
          reply.receipt.deleted === (call.command.intent.kind === "delete")
        : call.kind === "execute" &&
          reply.kind === "execution_applied" &&
          reply.projection.work.id === call.command.work &&
          (call.command.intent.kind === "approve" ||
            ("execution" in call.command.intent &&
              reply.receipt.execution === call.command.intent.execution)));
    if (!matching) {
      const failure = reply.kind === "error" ? reply.error : "outcome_unknown";
      this.failure = failure;
      this.delivery =
        failure === "conflict"
          ? "conflict"
          : failure === "outcome_unknown"
            ? "unknown"
            : "rejected";
      if (failure !== "outcome_unknown") this.pending = null;
      return false;
    }
    this.pending = null;
    this.delivery = "ready";
    this.failure = null;
    if (call.kind === "execute" && call.command.intent.kind === "edit_artifact") {
      const key = `${call.command.work}:${call.command.intent.artifact}`;
      const draft = this.artifacts.get(key);
      if (
        draft?.basis === call.command.expected_revision &&
        JSON.stringify(draft.data) === JSON.stringify(call.command.intent.data)
      )
        this.artifacts.delete(key);
    }
    if (reply.kind === "authoring_applied") {
      if (call.kind === "author" && call.command.intent.kind === "create") {
        this.composer = "";
        this.selected = reply.receipt.work;
      }
      if (call.kind === "author" && call.command.intent.kind === "edit") {
        const { work, expected_revision, edit } = call.command.intent;
        if (edit.kind === "replace_draft") {
          const draft = this.plans.get(work);
          if (
            draft?.basis === expected_revision &&
            JSON.stringify(draft.proposal) === JSON.stringify(edit.proposal)
          )
            this.plans.delete(work);
        }
        const field =
          edit.kind === "set_objective"
            ? "objective"
            : edit.kind === "answer_question"
              ? `question:${edit.id}`
              : null;
        if (field) {
          const key = `${work}:${field}`;
          const draft = this.drafts.get(key);
          const text =
            edit.kind === "set_objective"
              ? edit.objective
              : edit.kind === "answer_question"
                ? edit.answer
                : null;
          if (draft?.basis === expected_revision && draft.text.trim() === text)
            this.drafts.delete(key);
        }
      }
      if (reply.receipt.deleted && this.selected === reply.receipt.work) this.back();
    }
    if (
      reply.kind === "execution_applied" &&
      this.selected &&
      this.selected === reply.projection.work.id &&
      admitProjection(reply.projection, this.profile, this.selected, this.projection)
    )
      this.projection = reply.projection;
    if (this.active) {
      await this.reload();
      if (this.selected) await this.open(this.selected);
    }
    return true;
  }
  async plan(revision: string): Promise<WorkPlanRevision | null> {
    const work = this.selected;
    if (!work) return null;
    const result = await this.call(
      { kind: "query", request: { version: 1, query: { kind: "plan", work, revision } } },
      this.lifetime.signal,
    );
    return this.selected === work && result.kind === "plan" && result.plan.revision === revision
      ? result.plan
      : null;
  }
  async evidence(link: WorkEvidenceLink): Promise<WorkEvidencePreviewV1 | null> {
    const work = this.selected;
    if (!work) return null;
    const result = await this.call(
      { kind: "query", request: { version: 1, query: { kind: "evidence", work, link } } },
      this.lifetime.signal,
    );
    return this.selected === work &&
      result.kind === "evidence" &&
      result.evidence.link.extraction_id === link.extraction_id &&
      result.evidence.link.source_id === link.source_id
      ? result.evidence
      : null;
  }
}

// Bounded document-lifetime ownership, retained across Browse/Work transitions.
const sessions = new SvelteMap<string, WorkSession>();
export function workSession(profile: string): WorkSession {
  let session = sessions.get(profile);
  if (!session) {
    if (sessions.size >= 8) {
      const idle = [...sessions].find(([, value]) => value.canRelease);
      if (idle) {
        idle[1].dispose();
        sessions.delete(idle[0]);
      } else throw new Error("Work session capacity reached");
    }
    session = new WorkSession(profile);
    sessions.set(profile, session);
  }
  return session;
}
