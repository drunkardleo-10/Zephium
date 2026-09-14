import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { commands } from "$shared/ipc/bindings";
import type {
  WorkOperationV1,
  WorkOperationResponseV1,
  WorkOperationStateV1,
} from "$shared/ipc/bindings";
import { observe } from "$shared/lib/observe";
import { commandId } from "./work-model";

export type OperationObservation = {
  id: string;
  input: WorkOperationV1;
  state: WorkOperationStateV1;
};
function operationBasis(input: WorkOperationV1) {
  return input.kind === "read_public" || input.kind === "run" ? input.command : input.request;
}

/** Transient observations of native jobs. Disposal stops reads, never workers. */
export class WorkOperations {
  readonly jobs = new SvelteMap<string, OperationObservation>();
  private live = false;
  private generation = 0;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private polling: Promise<void> | null = null;
  private submitting = new SvelteSet<string>();
  constructor(
    private profile: string,
    private changed: (work: string) => Promise<void>,
  ) {}

  start() {
    this.live = true;
    ++this.generation;
    void this.poll();
  }
  stop() {
    this.live = false;
    ++this.generation;
    clearTimeout(this.timer);
  }
  latest(work: string, kind: WorkOperationV1["kind"] | readonly WorkOperationV1["kind"][]) {
    const kinds = typeof kind === "string" ? [kind] : kind;
    return [...this.jobs.values()].findLast(
      (job) => operationBasis(job.input).work === work && kinds.includes(job.input.kind),
    );
  }
  busy(work: string) {
    return [...this.jobs.values()].some(
      (job) =>
        operationBasis(job.input).work === work && ["pending", "unknown"].includes(job.state.kind),
    );
  }
  async begin(input: WorkOperationV1): Promise<void> {
    if (this.busy(operationBasis(input).work)) return;
    // Completed UI observations are disposable; native acknowledgement retains
    // a tombstone so reordered admission cannot restart a finished provider call.
    if (this.jobs.size >= 16) {
      const finished = [...this.jobs].find(
        ([, job]) => job.state.kind !== "pending" && job.state.kind !== "unknown",
      );
      if (finished) this.jobs.delete(finished[0]);
      else return;
    }
    const id = commandId();
    const job: OperationObservation = {
      id,
      input,
      state: { kind: "pending", work: operationBasis(input).work },
    };
    this.jobs.set(id, job);
    this.submitting.add(id);
    const result = await this.received(() => commands.workOperation(this.profile, id, input), id);
    await this.accept(job, result);
    this.submitting.delete(id);
    if (this.live) void this.poll();
  }
  private async received(
    call: () => Promise<WorkOperationResponseV1>,
    id: string,
  ): Promise<WorkOperationStateV1> {
    try {
      const result = await observe(Promise.resolve().then(call), 9_000);
      if (
        result.state === "received" &&
        result.value.version === 1 &&
        result.value.profile === this.profile &&
        result.value.operation === id
      )
        return result.value.state;
    } catch {
      /* A missing response never permits automatic admission replay. */
    }
    return { kind: "unknown" };
  }
  private async accept(job: OperationObservation, state: WorkOperationStateV1) {
    if (this.jobs.get(job.id) !== job) return;
    const work = operationBasis(job.input).work;
    const matching =
      state.kind === "pending"
        ? state.work === work
        : state.kind === "planned"
          ? ["plan", "prepare_plan"].includes(job.input.kind) &&
            state.response.version === 1 &&
            state.response.profile === this.profile &&
            state.response.work === work &&
            state.response.basis_revision === operationBasis(job.input).expected_revision
          : state.kind === "settled"
            ? state.response.version === 1 &&
              state.response.profile === this.profile &&
              (state.response.reply.kind === "error" ||
                (["prepare", "prepare_account"].includes(job.input.kind) &&
                  state.response.reply.kind === "approval_draft" &&
                  state.response.reply.work === work &&
                  state.response.reply.expected_revision ===
                    operationBasis(job.input).expected_revision) ||
                (["start", "read_public", "run"].includes(job.input.kind) &&
                  state.response.reply.kind === "projection" &&
                  state.response.reply.projection.work.id === work &&
                  state.response.reply.projection.work.profile === this.profile))
            : true;
    const admitted = matching ? state : { kind: "unknown" as const };
    this.jobs.set(job.id, { ...job, state: admitted });
    if (admitted.kind !== "pending") {
      await this.changed(work);
      if (admitted.kind !== "unknown") {
        // Lost acknowledgement only retains a bounded native observation.
        void this.received(
          () => commands.workOperationStatus(this.profile, work, job.id, true),
          job.id,
        );
      }
    }
  }
  private poll(): Promise<void> {
    clearTimeout(this.timer);
    if (this.polling) return this.polling;
    const generation = this.generation;
    const pending = this.readJobs().finally(() => {
      this.polling = null;
      if (
        this.live &&
        (generation !== this.generation ||
          [...this.jobs.values()].some((job) => job.state.kind === "pending"))
      ) {
        this.timer = setTimeout(() => {
          void this.poll();
        }, 1_000);
      }
    });
    this.polling = pending;
    return pending;
  }
  private async readJobs() {
    const generation = this.generation;
    if (!this.live) return;
    for (const job of this.jobs.values()) {
      if (this.submitting.has(job.id) || !["pending", "unknown"].includes(job.state.kind)) continue;
      const state = await this.received(
        () =>
          commands.workOperationStatus(this.profile, operationBasis(job.input).work, job.id, false),
        job.id,
      );
      if (!this.live || generation !== this.generation) return;
      await this.accept(job, state);
    }
  }
  async reconcile() {
    if (this.live) await this.poll();
  }
}
