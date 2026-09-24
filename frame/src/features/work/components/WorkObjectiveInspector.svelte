<script lang="ts">
  import type { WorkSession, WorkPlanRevision } from "$domain/work";
  import { untrack } from "svelte";
  import Artifact, { type EvidenceReference } from "$shared/ui/data/Artifact";
  import Evidence, { type EvidenceView } from "$shared/ui/data/Evidence";
  import { projectWork } from "../lib/project-work";
  import { preferences } from "$domain/preferences";
  import type { WorkEnvironmentReference } from "$shared/ipc/bindings";
  import Button from "$shared/ui/Button";
  import WorkPlanEditor from "./WorkPlanEditor.svelte";
  import WorkExecutionReview from "./WorkExecutionReview.svelte";
  import ContextManifestList from "./composer/ContextManifestList.svelte";
  import LiftHeader from "./LiftHeader.svelte";
  import Icon from "$shared/ui/Icon";
  import HostGlyph from "./cards/HostGlyph.svelte";
  import {
    CheckListIcon,
    ComputerTerminal01Icon,
    File01Icon,
    Note01Icon,
    Search01Icon,
    Target01Icon,
    Tick02Icon,
  } from "../lib/icons";
  import { runTimeline, type TimelineGlyph } from "../lib/run-timeline";
  import { isAgentExecution, isLive } from "../lib/agent-steps";
  import * as m from "$shared/i18n/messages";
  let {
    session,
    attached,
    onattach,
    onopencitation,
  }: {
    session: WorkSession;
    attached: readonly WorkEnvironmentReference[];
    onattach: (reference: WorkEnvironmentReference) => void;
    onopencitation?: (url: string) => void;
  } = $props();
  const id = $props.id();
  const projection = $derived(session.projection);
  const work = $derived(projection?.work);
  let executionId = $state<string | null>(null);
  let artifactId = $state<string | null>(null);
  let historicalPlan = $state.raw<WorkPlanRevision | null>(null);
  let evidence = $state.raw<EvidenceView | null>(null);
  let evidenceRead = 0;
  const execution = $derived(
    projection?.executions.find((item) => item.id === executionId) ?? projection?.executions.at(-1),
  );
  const revision = $derived(execution?.spec.plan_revision);
  const plan = $derived(
    !revision || revision === work?.plan?.revision
      ? (work?.plan ?? null)
      : historicalPlan?.revision === revision
        ? historicalPlan
        : null,
  );
  $effect(() => {
    const requested = revision;
    const current = work?.plan?.revision;
    let live = true;
    if (requested && requested !== current)
      void untrack(() => session.plan(requested)).then((value) => {
        if (live) historicalPlan = value;
      });
    return () => {
      live = false;
    };
  });
  const view = $derived(
    projection ? projectWork(projection, plan, execution?.id ?? null, session.activity) : null,
  );
  const artifact = $derived(view?.artifacts.find((artifact) => artifact.key === artifactId));
  async function inspectEvidence(reference: EvidenceReference) {
    const read = ++evidenceRead;
    const link = execution?.artifacts
      .flatMap((artifact) => {
        const user = execution?.user_artifacts?.find((value) => value.artifact === artifact.id);
        return user?.edited_data ? user.evidence : artifact.evidence;
      })
      .find((link) => `${link.extraction_id}:${link.source_id}` === reference.key);
    if (!link) return;
    evidence = { state: "loading" };
    const value = await session.evidence(link);
    if (read !== evidenceRead) return;
    evidence = value
      ? {
          state: "ready",
          title: reference.label,
          origin: value.origin,
          role: value.role,
          text: value.text,
          truncated: value.truncated,
          sourceBytes: value.source_bytes,
          ...(value.source?.kind === "provider_search"
            ? {
                citation: {
                  provider: "OpenAI",
                  model: value.source.model,
                  url: value.source.url,
                  title: value.source.title,
                },
              }
            : {}),
        }
      : { state: "unavailable", reason: m.work_evidence_unavailable() };
  }

  const timeline = $derived(execution ? runTimeline(execution) : []);
  const GLYPHS = {
    said: Note01Icon,
    search: Search01Icon,
    file: File01Icon,
    command: ComputerTerminal01Icon,
    ask: CheckListIcon,
    done: Tick02Icon,
  } as const satisfies Record<Exclude<TimelineGlyph, "page">, unknown>;
  const meta = $derived.by(() => {
    if (!projection || !execution) return "";
    const state = isLive(projection, execution)
      ? m.work_lift_run_live()
      : execution.status === "completed" || execution.status === "needs_review"
        ? m.work_lift_run_done()
        : projection.interrupted.includes(execution.id) || execution.status === "cancelled"
          ? m.work_lift_run_stopped()
          : m.work_lift_run_failed();
    if (!timeline.length) return state;
    const steps =
      timeline.length === 1
        ? m.work_lift_run_steps_one()
        : m.work_lift_run_steps({ count: timeline.length });
    return `${state} · ${steps}`;
  });
  /** Agent runs need no planning; reviewed work keeps its plan controls open. */
  const planning = $derived(
    !execution ||
      !isAgentExecution(execution) ||
      !!work?.questions.some((question) => question.state === "active"),
  );
  const blocked = $derived(
    !!session.pending || session.hasDrafts || !["ready", "rejected"].includes(session.delivery),
  );
  const operating = $derived(work ? session.operations.busy(work.id) : false);
  const latest = $derived(work ? session.operations.latest(work.id, "plan") : undefined);
</script>

{#if work}<div class="objective-inspector">
    <LiftHeader
      kind={m.work_env_objective()}
      title={work.objective}
      {meta}
      icon={Target01Icon}
      whole
    />
    {#if timeline.length}<ol class="timeline" aria-label={m.work_lift_run_label()}>
        {#each timeline as row (row.key)}<li class:said={row.glyph === "said"}>
            <span class="glyph" class:live={row.live} aria-hidden="true"
              >{#if row.glyph === "page"}<HostGlyph host={row.where} size={14} />{:else}<Icon
                  icon={GLYPHS[row.glyph]}
                  size={14}
                />{/if}</span
            >
            <span class="what">{row.what}</span>
            {#if row.where}<span class="where" class:mono={row.glyph === "command"}
                >{row.where}</span
              >{/if}
            {#if row.elapsed}<span class="elapsed">{row.elapsed}</span>{/if}
            {#if row.failure}<span class="note">{row.failure}</span>{/if}
          </li>{/each}
      </ol>{/if}
    <details class="planning" open={planning}>
      <summary>{m.work_lift_request_details()}</summary>
      <form
        onsubmit={(event) => {
          event.preventDefault();
          const text = session.draft("objective");
          if (text) void session.saveDraft("objective", text.trim());
        }}
      >
        <label for={`${id}-objective`}>{m.work_env_objective()}</label><textarea
          id={`${id}-objective`}
          rows="3"
          value={session.draft("objective") ?? work.objective}
          oninput={(event) => session.setDraft("objective", event.currentTarget.value)}
          disabled={!!session.pending}
          maxlength="8192"></textarea><Button
          type="submit"
          disabled={!!session.pending || !session.draft("objective")}
          >{m.work_submit_objective()}</Button
        >
      </form>
      {#each work.questions.filter((question) => question.state === "active") as question (question.id)}<form
          onsubmit={(event) => {
            event.preventDefault();
            const text = session.draft(`question:${question.id}`);
            if (text) void session.saveDraft(`question:${question.id}`, text.trim());
          }}
        >
          <label for={`${id}-${question.id}`}>{question.prompt}</label>
          <div class="actions">
            {#each question.options as option, index (index)}<Button
                size="compact"
                disabled={!!session.pending}
                onclick={() => session.setDraft(`question:${question.id}`, option)}>{option}</Button
              >{/each}
          </div>
          <textarea
            id={`${id}-${question.id}`}
            rows="2"
            maxlength="8192"
            value={session.draft(`question:${question.id}`) ?? ""}
            oninput={(event) =>
              session.setDraft(`question:${question.id}`, event.currentTarget.value)}
            disabled={!!session.pending}></textarea><Button
            type="submit"
            disabled={!!session.pending || !session.draft(`question:${question.id}`)}
            >{m.work_submit_answer()}</Button
          >
        </form>{/each}
      <div class="actions">
        <Button
          disabled={blocked || operating || preferences.value("ai.enabled") === "false"}
          onclick={() => {
            if (work)
              void session.operations.begin({
                kind: "plan",
                request: { version: 1, work: work.id, expected_revision: work.revision },
              });
          }}>{m.work_generate_plan()}</Button
        ><Button
          disabled={!!session.pending || !!session.planDraft}
          onclick={() => session.editPlan()}>{m.work_edit_plan()}</Button
        >
      </div>
      <p>{m.work_planning_disclosure()}</p>
      {#if latest && ["pending", "unknown"].includes(latest.state.kind)}<p role="status">
          {latest.state.kind === "pending"
            ? m.work_operation_pending()
            : m.work_operation_unknown()}
        </p>{/if}
      {#if plan && !session.planDraft}<ol>
          {#each plan.draft.nodes as node (node.id)}<li>{node.objective}</li>{/each}
        </ol>
        {#if plan.context}<details class="context">
            <summary>{m.work_context_disclosed()}</summary>
            <ContextManifestList disclosure={plan.context} />
          </details>{/if}{/if}
      <WorkPlanEditor {session} />
      <WorkExecutionReview {session} />
    </details>
    {#if projection && execution && projection.executions.length > 1}<label
        >{m.work_execution()}<select
          value={execution?.id}
          onchange={(event) => {
            executionId = event.currentTarget.value;
            artifactId = null;
            evidence = null;
            evidenceRead++;
          }}
          >{#each projection.executions as item, index (item.id)}<option value={item.id}
              >{projection.interrupted.includes(item.id)
                ? m.work_interrupted()
                : m.work_env_run_number({ number: index + 1 })}</option
            >{/each}</select
        ></label
      >{/if}
    {#if execution}<section aria-label={m.work_env_results()}>
        <h3>{m.work_env_results()}</h3>
        {#if !projection?.interrupted.includes(execution.id) && ["approved", "running"].includes(execution.status)}<Button
            disabled={!!session.pending}
            onclick={() => void session.execute({ kind: "cancel", execution: execution.id })}
            >{m.work_cancel()}</Button
          >{/if}
        <ul>
          {#each execution.artifacts as result (result.id)}<li>
              <Button size="compact" onclick={() => (artifactId = result.id)}>{result.title}</Button
              ><Button
                size="compact"
                disabled={attached.some(
                  (reference) =>
                    reference.kind === "artifact" &&
                    reference.objective === work.id &&
                    reference.execution === execution.id &&
                    reference.artifact === result.id,
                )}
                onclick={() =>
                  onattach({
                    kind: "artifact",
                    objective: work.id,
                    execution: execution.id,
                    artifact: result.id,
                  })}>{m.work_env_add_result()}</Button
              >
            </li>{/each}
        </ul>
      </section>{/if}
    {#if artifact && execution}<Artifact
        {artifact}
        onevidence={(reference) => void inspectEvidence(reference)}
        onlink={onopencitation}
      />{/if}
    {#if evidence}<aside>
        <Button
          size="compact"
          onclick={() => {
            evidence = null;
            evidenceRead++;
          }}>{m.resource_close()}</Button
        ><Evidence {evidence} onopen={onopencitation} />
      </aside>{/if}
    {#if session.hasDrafts}<Button
        disabled={!!session.pending}
        onclick={() => session.discardDrafts()}>{m.work_discard_drafts()}</Button
      >{/if}
    {#if view}{#each view.actions.filter( (action) => action.key.startsWith("acknowledge:") ) as action (action.key)}<details
        >
          <summary>{action.label}</summary>
          <p>{action.scope}</p>
          <p>{action.consequence}</p>
          <Button
            disabled={!!session.pending || !!action.disabledReason}
            onclick={() =>
              void session.execute({
                kind: "acknowledge_interruption",
                execution: action.key.slice(12),
              })}>{action.label}</Button
          >
        </details>{/each}{/if}
    <Button
      disabled={!!session.pending}
      onclick={() =>
        void session.edit({ kind: work.lifecycle === "active" ? "archive" : "restore" })}
      >{work.lifecycle === "active"
        ? m.work_env_archive_objective()
        : m.work_env_restore_objective()}</Button
    >
    {#if session.failure}<p role="status">{m.work_request_failed()}</p>
      <Button onclick={() => void session.reconcile()}>{m.work_reconcile()}</Button>{/if}
  </div>{:else}<p>{m.surface_loading()}</p>{/if}

<style>
  .objective-inspector,
  form,
  .planning {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  p {
    margin: 0;
    color: var(--color-muted);
  }

  h3 {
    margin: 0;
    font-size: var(--text-body);
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  summary {
    color: var(--color-muted);
    font-size: var(--text-label);
    cursor: default;
  }

  summary:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  li {
    margin-block: 8px;
  }

  ul li {
    display: flex;
    gap: 12px;
    justify-content: space-between;
    align-items: center;
  }

  .timeline {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .timeline li {
    display: grid;
    grid-template-columns: 20px auto minmax(0, 1fr) auto;
    align-items: baseline;
    column-gap: 8px;
    margin: 0;
    padding-block: 5px;
    font-size: var(--text-label);
  }

  .timeline .glyph {
    display: inline-grid;
    place-items: center;
    align-self: center;
    color: var(--color-faint);
  }

  .timeline .glyph.live {
    color: var(--color-accent);
  }

  .timeline .what {
    font-weight: 550;
  }

  .timeline .said .what {
    grid-column: 2 / 4;
    color: var(--color-muted);
    font-weight: 400;
  }

  .timeline .where {
    min-inline-size: 0;
    color: var(--color-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .timeline .mono {
    font-family: var(--font-mono);
    font-size: var(--text-caption);
  }

  .timeline .elapsed {
    grid-column: 4;
    color: var(--color-faint);
    font-size: var(--text-caption);
    font-variant-numeric: tabular-nums;
  }

  .timeline .note {
    grid-column: 2 / -1;
    color: var(--color-danger);
    font-size: var(--text-caption);
  }

  select,
  textarea {
    box-sizing: border-box;
    inline-size: 100%;
    color: var(--color-text);
    background: var(--color-field);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    padding: 8px;
    font: inherit;
  }

  select:focus-visible,
  textarea:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  ul {
    list-style: none;
    padding: 0;
  }
</style>
