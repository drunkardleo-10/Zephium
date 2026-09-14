<script lang="ts">
  import type { WorkSession } from "$domain/work";
  import { preferences } from "$domain/preferences";
  import { preparationFailure } from "../lib/preparation-failure";
  import Button from "$shared/ui/Button";
  import WorkExecutionReview from "./WorkExecutionReview.svelte";
  import { currentActivity } from "$domain/work";
  import * as m from "$shared/i18n/messages";
  let {
    session,
    ondetails,
    onopenpage,
  }: {
    session: WorkSession;
    ondetails: (execution?: string) => void;
    /** Presents the signed-in page for sign-in handoff or takeover. */
    onopenpage?: (tab: string) => void;
  } = $props();
  const id = $props.id();
  const state = $derived(session.projection);
  const work = $derived(state?.work);
  const planning = $derived(work ? session.operations.latest(work.id, "plan") : undefined);
  const publicRead = $derived(work ? session.operations.latest(work.id, "read_public") : undefined);
  const execution = $derived(state?.executions.at(-1));
  const interrupted = $derived(!!execution && !!state?.interrupted.includes(execution.id));
  const operating = $derived(work ? session.operations.busy(work.id) : false);
  const blocked = $derived(
    !!session.pending || operating || !["ready", "rejected"].includes(session.delivery),
  );
  const activity = $derived(
    state ? currentActivity(state, session.activity).at(-1)?.activity : undefined,
  );
  const activityLabels = {
    planning: m.work_activity_planning,
    delegating: m.work_activity_delegating,
    reading: m.work_activity_reading,
    comparing: m.work_activity_comparing,
    producing_artifact: m.work_activity_producing,
    waiting_for_approval: m.work_activity_approval,
    waiting_for_human: m.work_activity_human,
    cancelling: m.work_activity_cancelling,
    finishing: m.work_activity_finishing,
  };
  const failure = $derived(
    preparationFailure(publicRead?.state) ??
      (planning?.state.kind === "refused"
        ? planning.state.error
        : planning?.state.kind === "planned" && planning.state.response.outcome.kind === "refused"
          ? planning.state.response.outcome.reason.kind
          : planning?.state.kind === "planned" &&
              planning.state.response.outcome.kind === "settled" &&
              planning.state.response.outcome.response.reply.kind === "error"
            ? planning.state.response.outcome.response.reply.error
            : null),
  );
  const preparation = $derived(
    work ? session.operations.latest(work.id, "prepare_plan") : undefined,
  );
  const prepared = $derived(
    preparation?.state.kind === "planned" && preparation.state.response.outcome.kind === "settled"
      ? preparation.state.response.outcome.response.reply
      : null,
  );
  const currentApproval = $derived(
    prepared?.kind === "approval_draft" && prepared.expected_revision === work?.revision,
  );
  const settled = $derived(
    !!execution &&
      (interrupted ||
        ["completed", "failed", "cancelled", "interrupted", "needs_review"].includes(
          execution.status,
        )) &&
      (!work?.plan || work.plan.revision === execution.spec.plan_revision) &&
      !work?.questions.some((question) => question.state === "active") &&
      !blocked &&
      !session.hasDrafts &&
      !session.failure &&
      !failure &&
      !currentApproval &&
      !preparationFailure(preparation?.state),
  );
  const accountScope = $derived.by(() => {
    for (const node of execution?.spec.nodes ?? [])
      if (node.capability.kind === "account_read" || node.capability.kind === "account_update")
        return node.capability.scope;
    return null;
  });
  const intervention = $derived(execution?.intervention ?? null);
  const interventionLabel = $derived.by(() => {
    if (!intervention) return "";
    const origin = intervention.origin ?? accountScope?.origin ?? "";
    switch (intervention.kind) {
      case "sign_in":
        return m.work_intervention_sign_in({ origin });
      case "challenge":
        return m.work_intervention_challenge({ origin });
      case "permission":
        return m.work_intervention_permission();
      case "unsupported_interaction":
        return m.work_intervention_unsupported_interaction();
      case "review":
        return m.work_intervention_review();
      case "human_takeover":
        return m.work_intervention_human_takeover();
    }
  });
  const rerunnable = $derived(
    !!execution &&
      !!work &&
      !!intervention &&
      !["running", "cancel_requested", "approved"].includes(execution.status) &&
      work.status === "plan_ready" &&
      work.plan?.revision === execution.spec.plan_revision,
  );
  async function takeOver() {
    if (!execution || !accountScope || session.pending) return;
    const stopped = await session.execute({
      kind: "cancel",
      execution: execution.id,
      intervention: { kind: "human_takeover", origin: accountScope.origin },
    });
    if (stopped) onopenpage?.(accountScope.tab);
  }
  const executionLabel = $derived(
    interrupted
      ? m.work_interrupted()
      : execution?.status === "failed"
        ? m.work_env_status_failed()
        : execution?.status === "completed"
          ? m.work_env_status_completed()
          : execution?.status === "interrupted"
            ? m.work_interrupted()
            : execution?.status === "cancelled"
              ? m.work_env_status_cancelled()
              : execution?.status === "cancel_requested"
                ? m.work_env_status_stopping()
                : execution?.status === "needs_review"
                  ? m.work_review_required()
                  : execution?.status === "running"
                    ? m.work_env_status_running()
                    : execution?.authorization === "user_directed_public_read"
                      ? m.work_env_public_unstarted_status()
                      : m.work_env_status_approved(),
  );
  async function plan() {
    if (!work || blocked || session.hasDrafts || work.status === "needs_input") return;
    await session.operations.begin({
      kind: "plan",
      request: { version: 1, work: work.id, expected_revision: work.revision },
    });
  }
  async function answer(question: string) {
    const text = session.draft(`question:${question}`)?.trim();
    if (!text || blocked) return;
    if (await session.saveDraft(`question:${question}`, text)) await plan();
  }
</script>

{#if work}<section class="interaction" class:settled aria-label={m.work_env_current_work()}>
    <header>
      <strong role={settled ? "status" : undefined}
        >{settled ? executionLabel : work.objective.slice(0, 96)}</strong
      >{#if settled && !interrupted && work.status === "plan_ready" && (execution?.status === "failed" || execution?.status === "interrupted")}<Button
          size="compact"
          disabled={preferences.value("ai.enabled") === "false" || blocked}
          onclick={() => {
            if (work && !blocked && preferences.value("ai.enabled") !== "false")
              void session.operations.begin({
                kind: "prepare_plan",
                request: { version: 1, work: work.id, expected_revision: work.revision },
              });
          }}>{m.work_prepare_execution()}</Button
        >{/if}<Button
        class="plan-details"
        size="compact"
        onclick={() => ondetails(interrupted ? execution?.id : undefined)}
        >{interrupted ? m.work_env_review_interruption() : m.work_env_plan_details()}</Button
      >
    </header>
    {#if !settled}
      {#if publicRead?.state.kind === "unknown"}<p role="status">{m.work_operation_unknown()}</p>
      {:else if publicRead?.state.kind === "pending" && !activity}<p role="status">
          {m.work_env_public_pending()}
        </p>
      {:else if activity}<p role="status">{activityLabels[activity]()}</p>
      {:else if planning?.state.kind === "pending"}<p role="status">{m.work_activity_planning()}</p>
      {:else if planning?.state.kind === "unknown"}<p role="status">{m.work_operation_unknown()}</p>
      {:else if execution}<p role="status">{executionLabel}</p>{/if}
    {/if}
    {#each work.questions.filter((question) => question.state === "active") as question (question.id)}<form
        onsubmit={(event) => {
          event.preventDefault();
          void answer(question.id);
        }}
      >
        <label for={`${id}-${question.id}`}>{question.prompt}</label>
        <div class="options">
          {#each question.options as option, index (index)}<Button
              size="compact"
              disabled={blocked}
              onclick={() => session.setDraft(`question:${question.id}`, option)}>{option}</Button
            >{/each}
        </div>
        <input
          id={`${id}-${question.id}`}
          maxlength="8192"
          value={session.draft(`question:${question.id}`) ?? ""}
          oninput={(event) =>
            session.setDraft(`question:${question.id}`, event.currentTarget.value)}
          disabled={blocked}
        />
        <Button
          type="submit"
          disabled={blocked || !session.draft(`question:${question.id}`)?.trim()}
          >{m.work_env_continue()}</Button
        >
      </form>{/each}
    {#if work.status === "draft" && !operating && !publicRead}<Button
        size="compact"
        disabled={blocked || session.hasDrafts}
        onclick={() => void plan()}>{m.work_env_continue()}</Button
      >{/if}
    {#if intervention && execution && !interrupted}<div class="intervention" role="status">
        <strong>{m.work_intervention_needs_you()}</strong>
        <p>{interventionLabel}</p>
        {#if intervention.kind !== "human_takeover"}<p>
            {m.work_intervention_continue_hint()}
          </p>{/if}
        <div class="options">
          {#if accountScope && onopenpage}<Button
              size="compact"
              onclick={() => onopenpage?.(accountScope.tab)}
              >{m.work_intervention_open_page()}</Button
            >{/if}
          {#if rerunnable}<Button
              size="compact"
              disabled={blocked || preferences.value("ai.enabled") === "false"}
              onclick={() => void session.execute({ kind: "approve", spec: execution.spec })}
              >{m.work_intervention_run_again()}</Button
            >{/if}
        </div>
      </div>{/if}
    {#if !settled}<WorkExecutionReview {session} compact />{/if}
    {#if execution && !interrupted && ["approved", "running"].includes(execution.status)}<div
        class="options"
      >
        {#if accountScope && execution.status === "running" && onopenpage}<Button
            size="compact"
            disabled={!!session.pending}
            onclick={() => void takeOver()}>{m.work_take_over()}</Button
          >{/if}
        <Button
          size="compact"
          disabled={!!session.pending}
          onclick={() => void session.execute({ kind: "cancel", execution: execution.id })}
          >{m.work_cancel()}</Button
        >
      </div>{/if}
    {#if failure || session.failure}<p role="status">
        {m.work_request_failed({ reason: session.failure ?? failure ?? "" })}
      </p>{/if}
    {#if session.pending || publicRead?.state.kind === "unknown" || !["ready", "rejected"].includes(session.delivery)}<Button
        size="compact"
        onclick={() => void session.reconcile()}>{m.work_reconcile()}</Button
      >{/if}
  </section>{/if}

<style>
  .interaction {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 10px;
    padding: 12px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-surface);
    box-shadow: var(--shadow-float);
  }

  .interaction.settled {
    padding: 0 4px;
    border: 0;
    background: transparent;
    box-shadow: none;
  }

  header,
  .options {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  header {
    justify-content: space-between;
  }

  header :global(.plan-details) {
    flex-shrink: 0;
  }

  strong {
    min-inline-size: 0;
    font-weight: 550;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .settled strong {
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  p {
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  form {
    display: grid;
    gap: 8px;
  }

  .intervention {
    display: grid;
    gap: 6px;
    padding: 10px 12px;
    border-radius: var(--radius-control);
    background: var(--color-fill);
  }

  .intervention strong {
    font-size: var(--text-caption);
    color: var(--color-warning);
  }

  .options {
    flex-wrap: wrap;
  }

  input {
    inline-size: 100%;
    box-sizing: border-box;
    padding: 8px;
    font: inherit;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-field);
    color: var(--color-text);
  }

  input:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }
</style>
