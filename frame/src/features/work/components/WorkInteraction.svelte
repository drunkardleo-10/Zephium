<script lang="ts">
  import type { WorkSession } from "$domain/work";
  import { preferences } from "$domain/preferences";
  import { preparationFailure } from "../lib/preparation-failure";
  import { agentLine, stepLabel } from "../lib/agent-steps";
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
    /** Opens the objective inspector for an interrupted execution. */
    ondetails: (execution?: string) => void;
    /** Presents the signed-in page for sign-in handoff or takeover. */
    onopenpage?: (tab: string) => void;
  } = $props();
  const id = $props.id();
  const runtime = $derived(session.projection);
  const work = $derived(runtime?.work);
  const run = $derived(work ? session.operations.latest(work.id, "run") : undefined);
  const execution = $derived(runtime?.executions.at(-1));
  const interrupted = $derived(!!execution && !!runtime?.interrupted.includes(execution.id));
  const operating = $derived(work ? session.operations.busy(work.id) : false);
  const blocked = $derived(
    !!session.pending || operating || !["ready", "rejected"].includes(session.delivery),
  );
  // A run is one long operation; its questions are answered while it is busy.
  const answerBlocked = $derived(
    !!session.pending || !["ready", "rejected"].includes(session.delivery),
  );
  const live = $derived(
    !!execution &&
      !interrupted &&
      ["approved", "running", "cancel_requested"].includes(execution.status),
  );
  const activity = $derived(
    runtime ? currentActivity(runtime, session.activity).at(-1)?.activity : undefined,
  );
  const activityLabels = {
    planning: m.work_activity_planning,
    delegating: m.work_activity_delegating,
    searching: m.work_activity_searching,
    reading: m.work_activity_reading,
    interacting: m.work_activity_interacting,
    verifying: m.work_activity_verifying,
    recovering: m.work_activity_recovering,
    comparing: m.work_activity_comparing,
    producing_artifact: m.work_activity_producing,
    waiting_for_approval: m.work_activity_approval,
    waiting_for_human: m.work_activity_human,
    cancelling: m.work_activity_cancelling,
    finishing: m.work_activity_finishing,
  };
  const line = $derived(execution ? agentLine(execution) : null);
  const recent = $derived(
    (execution?.steps ?? [])
      .filter((step) => step.kind.kind !== "turn" && step.kind.kind !== "ask")
      .slice(-4)
      .map((step) => ({ id: step.id, label: stepLabel(step) })),
  );
  const questions = $derived(
    live
      ? (execution?.steps ?? []).flatMap((step) =>
          step.kind.kind === "ask" && step.status === "running"
            ? [{ id: step.id, prompt: step.kind.prompt, options: step.kind.options }]
            : [],
        )
      : [],
  );
  const failure = $derived(
    preparationFailure(run?.state) ??
      (run?.state.kind === "settled" && run.state.response.reply.kind === "error"
        ? run.state.response.reply.error
        : null),
  );
  const approval = $derived(
    work ? session.operations.latest(work.id, ["prepare_plan", "prepare_account"]) : undefined,
  );
  const awaitingApproval = $derived(
    (approval?.state.kind === "settled" &&
      approval.state.response.reply.kind === "approval_draft" &&
      approval.state.response.reply.expected_revision === work?.revision) ||
      (approval?.state.kind === "planned" &&
        approval.state.response.outcome.kind === "settled" &&
        approval.state.response.outcome.response.reply.kind === "approval_draft" &&
        approval.state.response.outcome.response.reply.expected_revision === work?.revision) ||
      approval?.state.kind === "pending",
  );
  const settled = $derived(
    !live &&
      questions.length === 0 &&
      !blocked &&
      !session.hasDrafts &&
      !session.failure &&
      !failure &&
      !awaitingApproval &&
      run?.state.kind !== "pending",
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
      !!accountScope &&
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
        : execution?.status === "completed" || execution?.status === "needs_review"
          ? m.work_env_status_done()
          : execution?.status === "interrupted"
            ? m.work_interrupted()
            : execution?.status === "cancelled"
              ? m.work_env_status_cancelled()
              : execution?.status === "cancel_requested"
                ? m.work_env_status_stopping()
                : execution?.status === "running"
                  ? m.work_env_status_running()
                  : execution?.authorization === "user_directed_public_read"
                    ? m.work_env_public_unstarted_status()
                    : m.work_env_status_approved(),
  );
  let answers = $state<Record<string, string>>({});
  async function answer(step: string) {
    const text = answers[step]?.trim();
    if (!text || !execution || answerBlocked) return;
    if (await session.answerStep(execution.id, step, text)) delete answers[step];
  }
</script>

{#if work}<section class="interaction" class:settled aria-label={m.work_env_current_work()}>
    <header>
      <strong role={settled ? "status" : undefined}
        >{settled ? (line ?? executionLabel) : work.objective.slice(0, 96)}</strong
      >{#if interrupted}<Button
          class="plan-details"
          size="compact"
          onclick={() => ondetails(execution?.id)}>{m.work_env_review_interruption()}</Button
        >{/if}
    </header>
    {#if !settled}
      {#if run?.state.kind === "unknown" || session.delivery === "unknown"}<p role="status">
          {m.work_operation_unknown()}
        </p>
      {:else if line && live}<p class="line" role="status">{line}</p>
      {:else if activity}<p role="status">{activityLabels[activity]()}</p>
      {:else if run?.state.kind === "pending"}<p role="status">{m.work_activity_planning()}</p>
      {:else if execution}<p role="status">{executionLabel}</p>{/if}
      {#if live && activity && line}<p class="activity">{activityLabels[activity]()}</p>{/if}
      {#if live && recent.length}<ol class="steps">
          {#each recent as step (step.id)}<li>{step.label}</li>{/each}
        </ol>{/if}
    {/if}
    {#each questions as question (question.id)}<form
        onsubmit={(event) => {
          event.preventDefault();
          void answer(question.id);
        }}
      >
        <label for={`${id}-${question.id}`}>{question.prompt}</label>
        <div class="options">
          {#each question.options as option, index (index)}<Button
              size="compact"
              disabled={answerBlocked}
              onclick={() => (answers[question.id] = option)}>{option}</Button
            >{/each}
        </div>
        <input
          id={`${id}-${question.id}`}
          maxlength="8192"
          value={answers[question.id] ?? ""}
          oninput={(event) => (answers[question.id] = event.currentTarget.value)}
          disabled={answerBlocked}
        />
        <Button type="submit" disabled={answerBlocked || !answers[question.id]?.trim()}
          >{m.work_env_continue()}</Button
        >
      </form>{/each}
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
    {#if accountScope && execution && !interrupted && execution.status === "running" && onopenpage}<div
        class="options"
      >
        <Button size="compact" disabled={!!session.pending} onclick={() => void takeOver()}
          >{m.work_take_over()}</Button
        >
      </div>{/if}
    {#if failure || session.failure}<p role="status">
        {m.work_request_failed({ reason: session.failure ?? failure ?? "" })}
      </p>{/if}
    {#if session.pending || run?.state.kind === "unknown" || !["ready", "rejected"].includes(session.delivery)}<Button
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

  .line {
    color: var(--color-text);
  }

  .activity {
    font-size: var(--text-label);
  }

  .steps {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 2px;
    color: var(--color-faint);
    font-size: var(--text-label);
  }

  .steps li {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
