<script lang="ts">
  import { tick, untrack } from "svelte";
  import type { WorkSession } from "$domain/work";
  import type { WorkHumanReasonV1 } from "$shared/ipc/bindings";
  import { currentActivity } from "$domain/work";
  import { agentLine, endingNote, isLive } from "../lib/agent-steps";
  import {
    accountRefusal,
    accountRefusalSentence,
    cardCountdown,
    reasonSentence,
  } from "../lib/work-human";
  import { preparationFailure } from "../lib/preparation-failure";
  import { fileName } from "../lib/work-files";
  import type { CanvasItem } from "../lib/canvas-model";
  import AgentOrb from "./cards/AgentOrb.svelte";
  import Icon from "$shared/ui/Icon";
  import {
    ArrowDown01Icon,
    ArrowRight02Icon,
    ArrowUp02Icon,
    NoteAddIcon,
    StopIcon,
  } from "../lib/icons";
  import * as m from "$shared/i18n/messages";
  let {
    session,
    agents = [],
    draft = "",
    waiting = null,
    problem = null,
    ondismissproblem,
    writeup,
    onfocusagent,
    onwaitingpage,
    onopenpage,
    onreview,
    onsteered,
  }: {
    session: WorkSession;
    /** The run's agent presences, as the canvas already projects them. */
    agents?: readonly CanvasItem[];
    /** What the person is typing while the agent runs. */
    draft?: string;
    /** The page card this run is held on, while a person is needed there. */
    waiting?: {
      card: string;
      host: string;
      reason: WorkHumanReasonV1;
      remaining: number;
    } | null;
    /** One line about something the person asked for that did not land. */
    problem?: string | null;
    ondismissproblem?: () => void;
    /** A document result can be written up as the person's note: an action, not a follow-up. */
    writeup?: () => void;
    onfocusagent?: (id: string) => void;
    /** Pans to the waiting page card and focuses it. */
    onwaitingpage?: (card: string) => void;
    /** Presents the signed-in page for sign-in handoff or takeover. */
    onopenpage?: (tab: string) => void;
    /** Opens the change the run is proposing, so the person can read it whole. */
    onreview?: (step: string) => void;
    /** The draft was handed to the agent or queued; clear the composer. */
    onsteered?: () => void;
  } = $props();
  const id = $props.id();
  const runtime = $derived(session.projection);
  const work = $derived(runtime?.work);
  const run = $derived(work ? session.operations.latest(work.id, "run") : undefined);
  const execution = $derived(runtime?.executions.at(-1));
  const interrupted = $derived(!!execution && !!runtime?.interrupted.includes(execution.id));
  const live = $derived(!!runtime && !!execution && isLive(runtime, execution));
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
  /** What the agent is doing in a granted folder, when it is doing that. */
  const fileState = $derived.by(() => {
    for (const step of execution?.steps ?? []) {
      if (step.status !== "running") continue;
      switch (step.kind.kind) {
        case "list":
          return m.work_line_listing_files({ name: fileName(step.kind.path) });
        case "read_file":
          return m.work_line_reading_file({ name: fileName(step.kind.path) });
        case "search_files":
          return m.work_line_searching_files();
        case "write_file":
        case "edit_file":
          return step.kind.decision === undefined || step.kind.decision === null
            ? m.work_line_change_waiting({ name: fileName(step.kind.path) })
            : m.work_line_writing_file({ name: fileName(step.kind.path) });
      }
    }
    return "";
  });
  /** A proposed change to a file, waiting on the person's word. */
  const proposal = $derived.by(() => {
    if (!live) return undefined;
    for (const step of execution?.steps ?? []) {
      if (step.status !== "running") continue;
      const kind = step.kind;
      if (kind.kind !== "write_file" && kind.kind !== "edit_file") continue;
      if (kind.decision === undefined || kind.decision === null) return { id: step.id };
    }
    return undefined;
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
    paused: m.work_line_paused,
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
  /**
   * A run that stopped on its question still asks it: the answer, or any
   * next message, resumes the work as its next request.
   */
  const stoppedQuestion = $derived.by(() => {
    if (live || !execution || !["cancelled", "failed", "interrupted"].includes(execution.status))
      return undefined;
    const step = (execution.steps ?? []).at(-1);
    if (step?.kind.kind !== "ask" || step.kind.answer || step.status === "succeeded")
      return undefined;
    return { id: step.id, prompt: step.kind.prompt, options: step.kind.options };
  });
  const question = $derived(questions.at(-1) ?? stoppedQuestion);
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
  /** Why the agent stopped for a person, in the person's words. */
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
  /** A signed-in page the person may need: to finish a challenge, or to take over. */
  const pageHandoff = $derived(!!accountScope && !!onopenpage && (live || !!intervention));
  const closing = $derived(execution ? agentLine(execution) : null);
  /** Why the run ended early, in Rust's words: the note its last unfinished step left. */
  const ending = $derived(execution ? endingNote(execution) : null);
  /** A signed-in limit the run reached, said plainly where nothing more specific stands. */
  const refusal = $derived.by(() => {
    const found = execution ? accountRefusal(execution) : null;
    return found ? accountRefusalSentence(found) : null;
  });
  /** A request refused before it ran says why, never an older run's words. */
  const refusals: Record<string, () => string> = {
    capacity: m.work_line_full,
    conflict: m.work_line_busy,
    unavailable: m.work_line_unavailable,
    shutdown: m.work_line_unavailable,
    profile_unavailable: m.work_line_unavailable,
    outcome_unknown: m.work_line_unknown,
  };
  /** Two or three words while it works; one quiet sentence once it stops. */
  const headline = $derived.by(() => {
    if (waiting) return reasonSentence(waiting.reason, waiting.host);
    if (intervention) return interventionLabel;
    const refused = failure ?? session.failure;
    if (refused) return refusals[refused]?.() ?? m.work_line_failed();
    if (interrupted) return m.work_line_interrupted();
    if (stoppedQuestion) return m.work_line_waiting_for_you();
    if (live) {
      if (fileState) return fileState;
      if (activity === "reading" && readingHost) return m.work_line_reading({ host: readingHost });
      // Past a signed-in limit, the plain sentence stands in for "Thinking".
      const thinking = !activity || activity === "planning" || activity === "delegating";
      if (refusal && thinking && execution?.status !== "cancel_requested") return refusal;
      if (activity) return activityStates[activity]?.() ?? m.work_line_thinking();
      return execution?.status === "cancel_requested"
        ? m.work_line_stopping()
        : m.work_line_thinking();
    }
    switch (execution?.status) {
      case "completed":
      case "needs_review":
        return closing ?? refusal ?? m.work_env_status_done();
      case "cancelled":
        return ending ?? refusal ?? m.work_line_stopped();
      case "interrupted":
        return ending ?? refusal ?? m.work_line_interrupted();
      case "failed":
        return ending ?? refusal ?? m.work_line_failed();
      default:
        return run?.state.kind === "pending" ? m.work_line_thinking() : m.work_line_ready();
    }
  });
  const settled = $derived(!live && !question && !proposal);
  const followups = $derived(settled ? session.followups.slice(0, 3) : []);
  const writeupOffer = $derived(settled ? writeup : undefined);
  const nextRows = $derived(followups.length > 0 || !!writeupOffer);
  const preview = $derived(draft.trim().slice(0, 60));

  /** What the person opened the capsule for; a question takes it whenever it is open. */
  let want = $state<"agents" | "answer" | "next" | "full" | null>(null);
  let answer = $state("");
  let host = $state<HTMLElement>();
  /** The headline is one line; when it is clipped its words open the whole of it. */
  let words = $state<HTMLElement>();
  let clipped = $state(false);
  const panel = $derived(
    want === null
      ? null
      : want === "agents"
        ? "agents"
        : question
          ? "question"
          : want === "next" && nextRows
            ? "next"
            : want === "full" && clipped
              ? "full"
              : null,
  );
  const expanded = $derived(panel !== null);
  /** What the capsule holds while it closes, so it shrinks around its rows, not around nothing. */
  let held = $state<NonNullable<typeof panel> | null>(null);
  $effect(() => {
    if (panel) held = panel;
  });
  $effect(() => {
    if (want && !panel) want = null;
  });
  $effect(() => {
    void headline;
    const element = words;
    if (!element) return;
    const measure = () => (clipped = element.scrollWidth > element.clientWidth);
    void tick().then(measure);
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  });
  $effect(() => {
    if (!live && want === "agents") untrack(() => (want = null));
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
    const sent = stoppedQuestion
      ? await session.continueWith(text)
      : await session.answerStep(execution.id, question.id, text);
    if (sent) {
      answer = "";
      want = null;
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
    if (expanded && event.target instanceof Node && host && !host.contains(event.target))
      want = null;
  }}
  onkeydown={(event) => {
    if (event.key !== "Escape" || !expanded) return;
    event.stopPropagation();
    want = null;
  }}
/>

{#if work}
  <section class="agent-line" class:settled bind:this={host} aria-label={m.work_agent_line()}>
    <!-- One capsule: it grows upward into its list and folds back into the line. -->
    <div class="capsule" class:open={expanded}>
      <div class="expand" class:shown={expanded} aria-hidden={!expanded} inert={!expanded}>
        <div class="expand-inner">
          {#if held === "agents"}
            <ul class="rows" aria-label={m.work_line_agents()}>
              {#each agents as agent (agent.id)}
                <li>
                  <button
                    type="button"
                    class="row"
                    onclick={() => {
                      want = null;
                      onfocusagent?.(agent.id);
                    }}
                  >
                    <AgentOrb seed={agent.agent?.seed ?? 0} size={18} />
                    <span class="who">{agent.title}</span>
                    <span class="doing">{agent.status}</span>
                  </button>
                </li>
              {:else}<li class="none">{m.work_line_no_agents()}</li>{/each}
            </ul>
          {:else if held === "next"}
            <ul class="rows" aria-label={m.work_line_next()}>
              {#each followups as followup, index (index)}
                <li>
                  <button
                    type="button"
                    class="row"
                    disabled={blocked}
                    onclick={() => {
                      want = null;
                      void session.continueWith(followup);
                    }}><span>{followup}</span><Icon icon={ArrowRight02Icon} size={13} /></button
                  >
                </li>
              {/each}
              {#if writeupOffer}{#if followups.length}<li class="rule" aria-hidden="true"></li>{/if}
                <li>
                  <button
                    type="button"
                    class="row writeup"
                    onclick={() => {
                      want = null;
                      writeupOffer?.();
                    }}
                    ><Icon icon={NoteAddIcon} size={14} /><span>{m.work_line_write_note()}</span
                    ></button
                  >
                </li>{/if}
            </ul>
          {:else if held === "full"}
            <p class="full">{headline}</p>
          {:else if held === "question" && question}
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
          aria-expanded={panel === "agents"}
          aria-label={m.work_line_agents()}
          onclick={() => (want = want === "agents" ? null : "agents")}
        >
          <AgentOrb seed={agents[0]?.agent?.seed ?? 0} size={22} ring={live} />
        </button>
        <div class="state">
          {#if waiting}
            <button
              type="button"
              class="words waiting"
              onclick={() => onwaitingpage?.(waiting.card)}
            >
              {headline}{#if cardCountdown(waiting.remaining)}<span class="left"
                  >{cardCountdown(waiting.remaining)}</span
                >{/if}
            </button>
          {:else if problem}
            <span class="problem" role="alert"
              ><button
                type="button"
                title={m.work_line_dismiss()}
                onclick={() => ondismissproblem?.()}>{problem}</button
              ></span
            >
          {:else}
            {#key headline}
              {#if clipped}
                <button
                  type="button"
                  class="words more"
                  aria-expanded={panel === "full"}
                  onclick={() => (want = want === "full" ? null : "full")}
                  ><span class="text" bind:this={words}>{headline}</span><Icon
                    icon={ArrowDown01Icon}
                    size={12}
                  /></button
                >
              {:else}
                <span class="words"><span class="text" bind:this={words}>{headline}</span></span>
              {/if}
            {/key}
          {/if}
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
          {:else if proposal}
            <button
              type="button"
              class="action"
              disabled={blocked}
              onclick={() => onreview?.(proposal.id)}>{m.work_line_review()}</button
            >
          {:else if question && !expanded}
            <button type="button" class="action" onclick={() => (want = "answer")}
              >{m.work_line_answer()}</button
            >
          {:else if nextRows}
            <!-- The disclosure of the capsule's growth, not a second surface. -->
            <button
              type="button"
              class="action disclosure"
              aria-expanded={panel === "next"}
              onclick={() => (want = want === "next" ? null : "next")}
              >{m.work_line_next()}<Icon icon={ArrowDown01Icon} size={12} /></button
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
    </div>
  </section>
{/if}

<style>
  .agent-line {
    display: flex;
    flex-direction: column;
    min-inline-size: 0;
  }

  /* The same element at rest and grown: a pill while it is one line (the panel
     radius clamps to half its height), a sheet once it holds rows. */
  .capsule {
    display: flex;
    flex-direction: column;
    min-inline-size: 0;
    border-radius: var(--radius-panel);
    background: var(--color-float);
    box-shadow: var(--shadow-popover);
  }

  /* Grows on the arrival curve, folds on the exit curve; the rows fade with it. */
  .expand {
    display: grid;
    grid-template-rows: 0fr;
    transition: grid-template-rows var(--motion-base) var(--ease-exit);
  }

  .expand.shown {
    grid-template-rows: 1fr;
    transition-timing-function: var(--ease-emphasized);
  }

  .expand-inner {
    min-block-size: 0;
    overflow: hidden;
    opacity: 0;
    transition: opacity var(--motion-fast) var(--ease-exit);
  }

  .shown .expand-inner {
    opacity: 1;
    transition: opacity var(--motion-base) var(--ease-out);
  }

  .line {
    display: flex;
    align-items: center;
    gap: 10px;
    box-sizing: border-box;
    block-size: 36px;
    padding: 0 6px 0 7px;
  }

  .open .line {
    border-block-start: 1px solid var(--color-border);
  }

  .avatar {
    display: grid;
    place-items: center;
    flex: none;
    inline-size: 24px;
    block-size: 24px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-capsule);
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
    display: flex;
    align-items: baseline;
    min-inline-size: 0;
    max-inline-size: 100%;
    animation: line-in var(--motion-base) var(--ease-out);
  }

  .words .text {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  button.more {
    align-items: center;
    gap: 4px;
    padding: 0;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: var(--text-label);
    cursor: default;
  }

  button.more:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  button.more :global(svg),
  .disclosure :global(svg) {
    flex: none;
    transition: rotate var(--motion-base) var(--ease-emphasized);
  }

  button.more :global(svg) {
    color: var(--color-muted);
  }

  button.more[aria-expanded="true"] :global(svg) {
    rotate: 180deg;
  }

  .settled .words {
    color: var(--color-muted);
  }

  /* A line that points somewhere reads as a link, not as another button. */
  button.waiting {
    display: inline-flex;
    align-items: baseline;
    gap: 6px;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-label);
    text-decoration: underline;
    text-decoration-color: var(--color-border-strong);
    text-underline-offset: 3px;
    cursor: default;
  }

  button.waiting:hover {
    text-decoration-color: var(--color-lit);
  }

  /* What the person asked for and did not land: said once, gone when read. */
  .problem {
    display: flex;
    min-inline-size: 0;
  }

  .problem button {
    min-inline-size: 0;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-label);
    text-align: start;
    cursor: default;
  }

  .problem button:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  button.waiting .left {
    color: var(--color-muted);
    font-variant-numeric: tabular-nums;
    text-decoration: none;
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
    display: inline-flex;
    align-items: center;
    gap: 4px;
    block-size: 26px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-lit);
    color: var(--color-on-lit);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 550;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .action:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .action:disabled {
    background: var(--color-control);
    color: var(--color-faint);
  }

  .action:hover:not(:disabled) {
    background: var(--color-lit-hover);
  }

  /* Next opens the capsule upward: its chevron points where the rows will be. */
  .action.disclosure {
    padding-inline-end: 10px;
    background: var(--color-control);
    color: var(--color-text);
  }

  .action.disclosure:hover:not(:disabled) {
    background: var(--color-control-hover);
  }

  .disclosure :global(svg) {
    rotate: 180deg;
  }

  .disclosure[aria-expanded="true"] :global(svg) {
    rotate: 0deg;
  }

  .quiet {
    display: grid;
    place-items: center;
    inline-size: 26px;
    block-size: 26px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: transparent;
    color: var(--color-muted);
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out);
  }

  .quiet:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .quiet:hover:not(:disabled) {
    background: var(--color-control-hover);
    color: var(--color-text);
  }

  .full {
    margin: 0;
    padding: 12px 14px;
    font-size: var(--text-label);
    line-height: 16px;
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-block-size: 220px;
    margin: 0;
    padding: 6px;
    overflow: auto;
    list-style: none;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    inline-size: 100%;
    box-sizing: border-box;
    min-block-size: 32px;
    padding: 6px 10px;
    border: 0;
    border-radius: var(--radius-row);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-label);
    text-align: start;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .row :global(svg) {
    flex: none;
    color: var(--color-muted);
  }

  .row:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }

  .row:disabled {
    color: var(--color-faint);
  }

  .rule {
    block-size: 1px;
    margin: 3px 10px;
    background: var(--color-border);
  }

  /* An action of the frontend's own, not words for the agent: its glyph leads. */
  .writeup {
    justify-content: flex-start;
  }

  .writeup span {
    flex: 1;
    text-align: start;
  }

  .row:hover:not(:disabled) {
    background: var(--row-hover);
  }

  .row:active:not(:disabled) {
    background: var(--row-pressed);
  }

  .who {
    flex: none;
  }

  .doing,
  .none {
    flex: 1;
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-muted);
  }

  .none {
    padding: 6px 10px;
    font-size: var(--text-label);
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 0;
    padding: 12px 14px;
  }

  label {
    font-size: var(--text-label);
  }

  .options {
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
    background: var(--color-control);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-label);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .chip:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .chip:disabled {
    color: var(--color-faint);
  }

  .chip:hover:not(:disabled) {
    background: var(--color-control-hover);
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
    border-radius: var(--radius-control);
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
    box-shadow: var(--shadow-field-focus);
  }

  .send {
    display: grid;
    place-items: center;
    flex: none;
    inline-size: 28px;
    block-size: 28px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-lit);
    color: var(--color-on-lit);
    cursor: default;
  }

  .send:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .send:disabled {
    background: var(--color-control);
    color: var(--color-faint);
  }

  @keyframes line-in {
    from {
      opacity: 0;
    }
  }
</style>
