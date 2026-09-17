<script lang="ts">
  import { untrack } from "svelte";
  import type { WorkSession } from "$domain/work";
  import { currentActivity } from "$domain/work";
  import { agentLine } from "../lib/agent-steps";
  import { preparationFailure } from "../lib/preparation-failure";
  import type { CanvasItem } from "../lib/canvas-model";
  import AgentAvatar from "./cards/AgentAvatar.svelte";
  import Icon from "$shared/ui/Icon";
  import { ArrowUp02Icon, StopIcon } from "../lib/icons";
  import * as m from "$shared/i18n/messages";
  let {
    session,
    agents = [],
    draft = "",
    onfocusagent,
    onopenpage,
    onsteered,
  }: {
    session: WorkSession;
    /** The run's agent presences, as the canvas already projects them. */
    agents?: readonly CanvasItem[];
    /** What the person is typing while the agent runs. */
    draft?: string;
    onfocusagent?: (id: string) => void;
    /** Presents the signed-in page for sign-in handoff or takeover. */
    onopenpage?: (tab: string) => void;
    /** The draft was handed to the agent or queued; clear the composer. */
    onsteered?: () => void;
  } = $props();
  const id = $props.id();
  const runtime = $derived(session.projection);
  const work = $derived(runtime?.work);
  const run = $derived(work ? session.operations.latest(work.id, "run") : undefined);
  const execution = $derived(runtime?.executions.at(-1));
  const interrupted = $derived(!!execution && !!runtime?.interrupted.includes(execution.id));
  const live = $derived(
    !!execution &&
      !interrupted &&
      ["approved", "running", "cancel_requested"].includes(execution.status),
  );
  const blocked = $derived(!!session.pending || !["ready", "rejected"].includes(session.delivery));
  const activity = $derived(
    runtime ? currentActivity(runtime, session.activity).at(-1)?.activity : undefined,
  );
  const readingHost = $derived.by(() => {
    const step = (execution?.steps ?? []).find(
      (step) =>
        step.status === "running" && (step.kind.kind === "read" || step.kind.kind === "discover"),
    );
    if (step?.kind.kind !== "read") return "";
    try {
      return new URL(step.kind.url).host;
    } catch {
      return "";
    }
  });
  const activityStates: Record<string, () => string> = {
    planning: m.work_line_thinking,
    delegating: m.work_line_thinking,
    searching: m.work_line_searching,
    reading: m.work_line_reading_web,
    interacting: m.work_line_using_page,
    verifying: m.work_line_checking,
    recovering: m.work_line_recovering,
    comparing: m.work_line_comparing,
    producing_artifact: m.work_line_writing,
    waiting_for_approval: m.work_line_waiting,
    waiting_for_human: m.work_line_waiting_for_you,
    cancelling: m.work_line_stopping,
    finishing: m.work_line_finishing,
  };
  const questions = $derived(
    live
      ? (execution?.steps ?? []).flatMap((step) =>
          step.kind.kind === "ask" && step.status === "running"
            ? [{ id: step.id, prompt: step.kind.prompt, options: step.kind.options }]
            : [],
        )
      : [],
  );
  const question = $derived(questions.at(-1));
  const failure = $derived(
    preparationFailure(run?.state) ??
      (run?.state.kind === "settled" && run.state.response.reply.kind === "error"
        ? run.state.response.reply.error
        : null),
  );
  const approval = $derived(
    work ? session.operations.latest(work.id, ["prepare_plan", "prepare_account"]) : undefined,
  );
  const approvalDraft = $derived.by(() => {
    const state = approval?.state;
    const reply =
      state?.kind === "settled"
        ? state.response.reply
        : state?.kind === "planned" && state.response.outcome.kind === "settled"
          ? state.response.outcome.response.reply
          : null;
    return reply?.kind === "approval_draft" && reply.expected_revision === work?.revision
      ? reply
      : null;
  });
  const accountScope = $derived.by(() => {
    for (const node of execution?.spec.nodes ?? [])
      if (node.capability.kind === "account_read" || node.capability.kind === "account_update")
        return node.capability.scope;
    return null;
  });
  const intervention = $derived(execution?.intervention ?? null);
  /** A signed-in page the person may need: to finish a challenge, or to take over. */
  const pageHandoff = $derived(!!accountScope && !!onopenpage && (live || !!intervention));
  const closing = $derived(execution ? agentLine(execution) : null);
  /** Two or three words while it works; one quiet sentence once it stops. */
  const headline = $derived.by(() => {
    if (failure || session.failure) return m.work_line_failed();
    if (interrupted) return m.work_line_stopped();
    if (live) {
      if (activity === "reading" && readingHost) return m.work_line_reading({ host: readingHost });
      if (activity) return activityStates[activity]?.() ?? m.work_line_thinking();
      return execution?.status === "cancel_requested"
        ? m.work_line_stopping()
        : m.work_line_thinking();
    }
    switch (execution?.status) {
      case "completed":
      case "needs_review":
        return closing ?? m.work_env_status_done();
      case "cancelled":
      case "interrupted":
        return m.work_line_stopped();
      case "failed":
        return m.work_line_failed();
      default:
        return run?.state.kind === "pending" ? m.work_line_thinking() : m.work_line_ready();
    }
  });
  const settled = $derived(!live && !question);
  const followups = $derived(settled ? session.followups.slice(0, 3) : []);
  const preview = $derived(draft.trim().slice(0, 60));

  let open = $state(false);
  let expanded = $state(false);
  let answer = $state("");
  let host = $state<HTMLElement>();
  $effect(() => {
    if (!question) expanded = false;
  });
  $effect(() => {
    if (!live) open = false;
  });
  // The oldest queued message rides on as soon as the run in flight closes.
  let sending = false;
  $effect(() => {
    if (live || blocked || !session.queue.length || sending) return;
    sending = true;
    void untrack(() => session.sendQueued()).finally(() => (sending = false));
  });
  async function submitAnswer() {
    const text = answer.trim();
    if (!text || !execution || !question || blocked) return;
    if (await session.answerStep(execution.id, question.id, text)) {
      answer = "";
      expanded = false;
    }
  }
  async function steer() {
    const text = draft.trim();
    if (!text) return;
    if (!(await session.steer(text))) session.enqueue(text);
    onsteered?.();
  }
  /** Handing the page back: a live run is revoked with the reason before it opens. */
  async function openPage() {
    const scope = accountScope;
    if (!scope || blocked) return;
    if (live && execution) {
      const stopped = await session.execute({
        kind: "cancel",
        execution: execution.id,
        intervention: { kind: "human_takeover", origin: scope.origin },
      });
      if (!stopped) return;
    }
    onopenpage?.(scope.tab);
  }
  async function stop() {
    if (!execution || blocked) return;
    await session.execute({ kind: "cancel", execution: execution.id });
  }
</script>

<svelte:window
  onpointerdown={(event) => {
    if (open && event.target instanceof Node && host && !host.contains(event.target)) open = false;
  }}
  onkeydown={(event) => {
    if (event.key !== "Escape" || (!open && !expanded)) return;
    event.stopPropagation();
    open = false;
    expanded = false;
  }}
/>

{#if work}
  <section class="agent-line" class:settled bind:this={host} aria-label={m.work_agent_line()}>
    {#if open}
      <ul class="roster" aria-label={m.work_line_agents()}>
        {#each agents as agent (agent.id)}
          <li>
            <button
              type="button"
              onclick={() => {
                open = false;
                onfocusagent?.(agent.id);
              }}
            >
              <AgentAvatar seed={agent.agent?.seed ?? 0} size={18} active={live} />
              <span class="who">{agent.title}</span>
              <span class="doing">{agent.status}</span>
            </button>
          </li>
        {:else}<li class="none">{m.work_line_no_agents()}</li>{/each}
      </ul>
    {/if}
    <div class="expand" class:shown={expanded && !!question} aria-hidden={!expanded}>
      <div class="expand-inner">
        {#if question}
          <form
            onsubmit={(event) => {
              event.preventDefault();
              void submitAnswer();
            }}
          >
            <label for={`${id}-answer`}>{question.prompt}</label>
            <div class="options">
              {#each question.options as option, index (index)}
                <button
                  type="button"
                  class="chip"
                  disabled={blocked}
                  onclick={() => (answer = option)}>{option}</button
                >
              {/each}
            </div>
            <div class="answer">
              <input
                id={`${id}-answer`}
                maxlength="8192"
                placeholder={m.work_line_answer_placeholder()}
                bind:value={answer}
                disabled={blocked}
              />
              <button
                type="submit"
                class="send"
                aria-label={m.work_line_send_answer()}
                disabled={blocked || !answer.trim()}
              >
                <Icon icon={ArrowUp02Icon} size={15} strokeWidth={2} />
              </button>
            </div>
          </form>
        {/if}
      </div>
    </div>
    <div class="line">
      <button
        type="button"
        class="avatar"
        aria-expanded={open}
        aria-label={m.work_line_agents()}
        onclick={() => (open = !open)}
      >
        <AgentAvatar seed={agents[0]?.agent?.seed ?? 0} size={22} active={live} />
      </button>
      <div class="state">
        {#key headline}<span class="words">{headline}</span>{/key}
        {#if live && preview}<span class="draft">{preview}</span>{/if}
      </div>
      <div class="controls">
        {#if live && preview}
          <button
            type="button"
            class="action"
            title={m.work_line_steer_hint()}
            onclick={() => void steer()}>{m.work_line_steer()}</button
          >
        {:else if question && !expanded}
          <button type="button" class="action" onclick={() => (expanded = true)}
            >{m.work_line_answer()}</button
          >
        {:else if pageHandoff}
          <button type="button" class="action" disabled={blocked} onclick={() => void openPage()}
            >{m.work_line_open()}</button
          >
        {:else if approvalDraft && !live}
          <button
            type="button"
            class="action"
            disabled={blocked}
            onclick={() => {
              if (approvalDraft)
                void session.execute(
                  { kind: "approve", spec: approvalDraft.spec },
                  approvalDraft.expected_revision,
                );
            }}>{m.work_line_approve()}</button
          >
        {/if}
        {#if live}
          <button
            type="button"
            class="quiet"
            aria-label={m.work_line_stop()}
            title={m.work_line_stop()}
            disabled={blocked}
            onclick={() => void stop()}
          >
            <Icon icon={StopIcon} size={14} />
          </button>
        {/if}
      </div>
    </div>
    {#if followups.length}
      <div class="followups">
        {#each followups as followup, index (index)}
          <button
            type="button"
            class="chip"
            disabled={blocked}
            onclick={() => void session.continueWith(followup)}>{followup}</button
          >
        {/each}
      </div>
    {/if}
  </section>
{/if}

<style>
  .agent-line {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-inline-size: 0;
  }

  .line {
    display: flex;
    align-items: center;
    gap: 10px;
    box-sizing: border-box;
    block-size: 36px;
    padding: 0 6px 0 7px;
    border-radius: var(--radius-capsule);
    background: var(--color-menu);
    backdrop-filter: blur(12px) saturate(1.2);
    box-shadow: var(--shadow-float);
  }

  .avatar {
    display: grid;
    place-items: center;
    flex: none;
    inline-size: 24px;
    block-size: 24px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: transparent;
    cursor: default;
    transition: scale var(--motion-base) var(--ease-spring);
  }

  .avatar:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .avatar:active {
    scale: 0.94;
  }

  .state {
    display: flex;
    align-items: baseline;
    gap: 8px;
    flex: 1;
    min-inline-size: 0;
    font-size: var(--text-label);
  }

  .words {
    flex: none;
    max-inline-size: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    animation: line-in var(--motion-base) var(--ease-smooth);
  }

  .settled .words {
    color: var(--color-muted);
  }

  .draft {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-faint);
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 4px;
    flex: none;
  }

  .action {
    block-size: 26px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-accent);
    color: var(--color-on-accent);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 550;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-smooth);
  }

  .action:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .action:disabled {
    background: var(--color-fill);
    color: var(--color-faint);
  }

  .action:hover:not(:disabled) {
    background: var(--color-accent-hover);
  }

  .quiet {
    display: grid;
    place-items: center;
    inline-size: 26px;
    block-size: 26px;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--color-muted);
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-smooth),
      color var(--motion-fast) var(--ease-smooth);
  }

  .quiet:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .quiet:hover:not(:disabled) {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .expand {
    display: grid;
    grid-template-rows: 0fr;
    transition: grid-template-rows var(--motion-base) var(--ease-smooth);
  }

  .expand.shown {
    grid-template-rows: 1fr;
  }

  .expand-inner {
    min-block-size: 0;
    overflow: hidden;
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 14px;
    border-radius: var(--radius-control);
    background: var(--color-menu);
    backdrop-filter: blur(12px) saturate(1.2);
    box-shadow: var(--shadow-float);
  }

  label {
    font-size: var(--text-label);
  }

  .options,
  .followups {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    min-inline-size: 0;
  }

  .chip {
    max-inline-size: 100%;
    block-size: 26px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-label);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-smooth);
  }

  .chip:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .chip:disabled {
    color: var(--color-faint);
  }

  .chip:hover:not(:disabled) {
    background: var(--color-fill-hover);
  }

  .answer {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  input {
    flex: 1;
    min-inline-size: 0;
    box-sizing: border-box;
    block-size: 30px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: var(--color-field);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-label);
    outline: none;
  }

  input::placeholder {
    color: var(--color-faint);
  }

  input:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .send {
    display: grid;
    place-items: center;
    flex: none;
    inline-size: 28px;
    block-size: 28px;
    border: 0;
    border-radius: 50%;
    background: var(--color-accent);
    color: var(--color-on-accent);
    cursor: default;
  }

  .send:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .send:disabled {
    background: var(--color-fill);
    color: var(--color-faint);
  }

  .roster {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 6px;
    max-block-size: 220px;
    overflow: auto;
    border-radius: var(--radius-menu);
    background: var(--color-menu);
    backdrop-filter: blur(12px) saturate(1.2);
    box-shadow: var(--shadow-popover);
    animation: line-in var(--motion-base) var(--ease-smooth);
  }

  .roster button {
    display: flex;
    align-items: center;
    gap: 10px;
    inline-size: 100%;
    box-sizing: border-box;
    min-block-size: 32px;
    padding: 4px 8px;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-label);
    text-align: start;
    cursor: default;
    transition: background-color var(--motion-instant) ease;
  }

  .roster button:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .roster button:hover {
    background: var(--color-control-hover);
  }

  .who {
    flex: none;
  }

  .doing,
  .none {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-muted);
  }

  .none {
    padding: 6px 8px;
    font-size: var(--text-label);
  }

  @keyframes line-in {
    from {
      opacity: 0;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .expand {
      transition: none;
    }

    .words,
    .roster {
      animation: none;
    }
  }
</style>
