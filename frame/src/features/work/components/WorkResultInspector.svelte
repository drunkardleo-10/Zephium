<script lang="ts">
  import type { WorkSession } from "$domain/work";
  import type { ResultReference } from "../lib/project-environment-results";
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  import Artifact, { DocumentView } from "$shared/ui/data/Artifact";
  import AnswerView from "$shared/ui/data/Artifact/AnswerView.svelte";
  import Evidence, { type EvidenceView } from "$shared/ui/data/Evidence";
  import Compare from "./compare/Compare.svelte";
  import FindingsLift from "./FindingsLift.svelte";
  import Button from "$shared/ui/Button";
  import LiftHeader, { type LiftAction } from "./LiftHeader.svelte";
  import SourcesRail from "./SourcesRail.svelte";
  import LazyView from "$shared/ui/LazyView";
  import {
    CheckListIcon,
    Doc01Icon,
    GitCompareIcon,
    Table01Icon,
    ChartColumnIcon,
    CubeIcon,
    SourceCodeIcon,
  } from "../lib/icons";
  import { artifactView } from "../lib/project-work";
  import { compareModel, type CompareCell, type ComparePicture } from "../lib/compare";
  import { correctedMatrix } from "../lib/correct";
  import { untrack } from "svelte";
  import * as m from "$shared/i18n/messages";
  /** The diagram's picture and lists load only when a diagram is lifted. */
  const loadDiagram = () => import("./DiagramLift.svelte");
  let {
    session,
    reference,
    source,
    pictures,
    onopen,
    onfile,
    primary,
    secondary,
  }: {
    session: WorkSession;
    reference: ResultReference;
    source: EvidenceReference | null;
    /** The admitted picture of each subject, so a column looks like its card. */
    pictures?: ReadonlyMap<string, ComparePicture>;
    onopen?: (url: string) => void;
    /** A cited file opens where the run recorded it, not in the pane. */
    onfile?: (record: string) => void;
    /** Make tasks for a plan, Save as note for a document. */
    primary?: LiftAction;
    /** Save as note beside a plan's Make tasks. */
    secondary?: LiftAction;
  } = $props();
  const execution = $derived(
    session.projection?.work.id === reference.objective
      ? session.projection.executions.find((value) => value.id === reference.execution)
      : undefined,
  );
  const artifact = $derived(execution?.artifacts.find((value) => value.id === reference.artifact));
  const view = $derived(artifact && execution ? artifactView(artifact, execution) : undefined);
  const compare = $derived(
    view?.content.kind === "matrix" ? compareModel(view.content, pictures) : null,
  );
  const user = $derived(
    execution?.user_artifacts?.find((value) => value.artifact === reference.artifact),
  );
  const links = $derived(user?.edited_data ? user.evidence : (artifact?.evidence ?? []));
  /** A settled result can still be corrected; a running one cannot. */
  const settled = $derived(
    !!execution && ["needs_review", "completed"].includes(execution.status) && !session.pending,
  );
  /** Rust keeps the run in review until the person accepts what it published. */
  const accepting = $derived(
    execution?.status === "needs_review" && user?.decision !== "accepted" && !session.pending,
  );
  async function correct(cell: CompareCell, text: string) {
    const current = execution;
    const original = artifact;
    if (!current || !original || !settled) return;
    const next = correctedMatrix(
      user?.edited_data ?? original.data,
      cell.subject,
      cell.criterion,
      text,
    );
    if (!next) return;
    session.editArtifact(current.id, original.id, next);
    if (!(await session.saveArtifact(original.id))) session.discardArtifact(original.id);
  }
  function accept() {
    const current = execution;
    const original = artifact;
    if (!current || !original || !accepting) return;
    void session.execute({
      kind: "review_artifact",
      execution: current.id,
      artifact: original.id,
      decision: "accepted",
    });
  }
  /** Everything the result cites, once each, for the rail. */
  const cited = $derived.by(() => {
    const content = view?.content;
    if (!view || !content || content.kind === "sources" || view.knowledge) return [];
    const all: EvidenceReference[] = [...view.evidence];
    if (content.kind === "matrix")
      for (const row of content.cells) for (const cell of row) all.push(...cell.evidence);
    if (content.kind === "findings") for (const item of content.items) all.push(...item.evidence);
    return [...new Map(all.map((entry) => [entry.key, entry])).values()];
  });
  const answer = $derived(view?.content.kind === "answer" ? view.content : undefined);
  /** An answer's header is its kind and its question, nothing more. */
  const icon = $derived(
    answer
      ? undefined
      : compare || view?.content.kind === "comparison"
        ? GitCompareIcon
        : view?.content.kind === "findings"
          ? CheckListIcon
          : view?.content.kind === "table"
            ? Table01Icon
            : view?.content.kind === "chart"
              ? ChartColumnIcon
              : view?.content.kind === "diagram"
                ? CubeIcon
                : view?.content.kind === "code"
                  ? SourceCodeIcon
                  : Doc01Icon,
  );
  const kind = $derived(
    answer
      ? m.work_card_kind_answer()
      : compare || view?.content.kind === "comparison"
        ? m.work_lift_comparison()
        : view?.content.kind === "findings"
          ? m.work_env_findings()
          : view?.content.kind === "diagram"
            ? m.work_card_kind_diagram()
            : view?.content.kind === "code"
              ? `${m.work_card_kind_code()} · ${view.content.language}`
              : m.work_env_result(),
  );
  const meta = $derived.by(() => {
    const content = view?.content;
    if (answer) return "";
    if (compare)
      return m.work_lift_compare_meta({
        subjects: compare.columns.length,
        rows: compare.rows.length,
      });
    if (content?.kind === "findings")
      return content.items.length === 1
        ? m.work_lift_claims_one()
        : m.work_lift_claims({ count: content.items.length });
    return session.projection?.work.objective ?? "";
  });
  let copied = $state<"idle" | "copied" | "failed">("idle");
  /** Code's primary action copies the whole text; an answer's copies its Markdown. */
  const copy = $derived.by((): LiftAction | undefined => {
    const content = view?.content;
    if (content?.kind !== "code" && content?.kind !== "answer") return undefined;
    const text = content.kind === "code" ? content.text : content.markdown;
    return {
      label:
        content.kind === "answer"
          ? copied === "copied"
            ? m.work_answer_copied()
            : m.work_answer_copy()
          : copied === "copied"
            ? m.work_code_copied()
            : m.work_code_copy(),
      onclick: () =>
        void navigator.clipboard.writeText(text).then(
          () => (copied = "copied"),
          () => (copied = "failed"),
        ),
    };
  });
  let evidence = $state.raw<EvidenceView | null>(null);
  let selectedSource = $derived(source);
  /** One click, one destination: a page opens in the pane, a file in the lift,
   * and retained evidence with no address opens here. */
  function pick(entry: EvidenceReference) {
    if (entry.file) {
      onfile?.(entry.file.record);
      return;
    }
    if (entry.url) {
      onopen?.(entry.url);
      return;
    }
    selectedSource = entry;
  }
  $effect(() => {
    const selected = selectedSource;
    const link = links.find((link) => `${link.extraction_id}:${link.source_id}` === selected?.key);
    let live = true;
    evidence = selected ? { state: "loading" } : null;
    if (selected && link)
      void untrack(() => session.evidence(link)).then((value) => {
        if (!live) return;
        evidence = value
          ? {
              state: "ready",
              title: selected.label,
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
                      title: value.source.title,
                      url: value.source.url,
                    },
                  }
                : {}),
            }
          : { state: "unavailable", reason: m.work_evidence_unavailable() };
      });
    else if (selected) evidence = { state: "unavailable", reason: m.work_evidence_unavailable() };
    return () => {
      live = false;
    };
  });
</script>

{#if view}
  <section class="result">
    <LiftHeader {kind} title={view.title} {meta} {icon} primary={primary ?? copy}>
      {#snippet actions()}{#if answer && primary && copy}<Button
            size="compact"
            onclick={copy.onclick}>{copy.label}</Button
          >{/if}{#if secondary}<Button
            size="compact"
            disabled={secondary.disabled}
            title={secondary.title}
            onclick={secondary.onclick}>{secondary.label}</Button
          >{/if}{/snippet}
    </LiftHeader>
    {#if compare}<Compare
        model={compare}
        correctable={settled}
        onevidence={pick}
        oncorrect={(cell, text) => void correct(cell, text)}
      />
    {:else if answer}<div class="reading">
        <AnswerView blocks={answer.blocks} label={view.title} page />
      </div>
    {:else if view.content.kind === "document"}<div class="reading">
        {#if view.content.formatted}<DocumentView
            document={view.content.formatted}
            onlink={onopen}
          />
        {:else}{#each view.content.paragraphs as paragraph, index (index)}<p>
              {paragraph}
            </p>{/each}{/if}
      </div>
    {:else if view.content.kind === "findings"}<FindingsLift
        content={view.content}
        onevidence={pick}
      />
    {:else if view.content.kind === "code"}<div class="reading code">
        <Artifact artifact={view} embedded onevidence={pick} onlink={onopen} />
      </div>
    {:else if view.content.kind === "diagram"}{@const content = view.content}<LazyView
        loader={loadDiagram}
        loadingLabel={m.surface_loading()}
        failureLabel={m.work_artifact_unavailable()}
        retryLabel={m.surface_retry()}
        >{#snippet children(DiagramLift)}<DiagramLift {content} />{/snippet}</LazyView
      >
    {:else}<Artifact artifact={view} embedded onevidence={pick} onlink={onopen} />{/if}
    {#if copied === "failed"}<p role="alert">{m.work_code_copy_failed()}</p>{/if}
    {#if evidence}<Evidence {evidence} {onopen} />{/if}
    <SourcesRail references={cited} onpick={pick} />
    {#if accepting}<footer>
        <Button size="compact" onclick={accept}>{m.work_env_accept_result()}</Button>
      </footer>{/if}
  </section>
{:else}<p role="status">{m.work_artifact_unavailable()}</p>{/if}

<style>
  .result {
    display: flex;
    flex-direction: column;
    gap: 16px;
    min-inline-size: 0;
  }

  footer {
    display: flex;
    justify-content: flex-end;
  }

  /* Reading mode: a page, not a card. */
  .reading {
    inline-size: 100%;
    max-inline-size: 640px;
    margin-inline: auto;
    font-size: var(--text-page-body);
    line-height: 1.55;
    overflow-wrap: anywhere;
  }

  .reading p {
    margin-block: 0 12px;
    white-space: pre-wrap;
  }

  /* A document reads as the answer's page does: one measure, one scale. */
  .reading:not(.code) :global(p),
  .reading:not(.code) :global(ul),
  .reading:not(.code) :global(ol),
  .reading:not(.code) :global(blockquote),
  .reading:not(.code) :global(pre) {
    margin-block-end: 12px;
    line-height: 1.6;
  }

  .reading:not(.code) :global(h3) {
    margin-block: 24px 8px;
    font-size: var(--text-page-title);
  }

  .reading:not(.code) :global(h4) {
    margin-block: 16px 6px;
    font-size: var(--text-body);
  }

  .reading:not(.code) > :global(:first-child > :first-child) {
    margin-block-start: 0;
  }

  /* Code keeps its rail and its own size; it only shares the measure. */
  .reading.code {
    font-size: inherit;
    line-height: inherit;
    overflow-wrap: normal;
  }

  .reading.code:has(:global(.rail)) {
    max-inline-size: calc(640px + 16px + 240px);
  }
</style>
