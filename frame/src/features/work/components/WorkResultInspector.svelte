<script lang="ts">
  import type { WorkSession } from "$domain/work";
  import type { ResultReference } from "../lib/project-environment-results";
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  import Artifact, { DocumentView } from "$shared/ui/data/Artifact";
  import Evidence, { type EvidenceView } from "$shared/ui/data/Evidence";
  import Compare from "./compare/Compare.svelte";
  import Button from "$shared/ui/Button";
  import LiftHeader from "./LiftHeader.svelte";
  import SourcesRail from "./SourcesRail.svelte";
  import {
    CheckListIcon,
    Doc01Icon,
    GitCompareIcon,
    Table01Icon,
    ChartColumnIcon,
  } from "../lib/icons";
  import { artifactView } from "../lib/project-work";
  import { compareModel, type CompareCell, type ComparePicture } from "../lib/compare";
  import { correctedMatrix } from "../lib/correct";
  import { untrack } from "svelte";
  import * as m from "$shared/i18n/messages";
  let {
    session,
    reference,
    source,
    pictures,
    onopen,
    onfile,
  }: {
    session: WorkSession;
    reference: ResultReference;
    source: EvidenceReference | null;
    /** The admitted picture of each subject, so a column looks like its card. */
    pictures?: ReadonlyMap<string, ComparePicture>;
    onopen?: (url: string) => void;
    /** A cited file opens where the run recorded it, not in the pane. */
    onfile?: (record: string) => void;
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
    if (!view || !content || content.kind === "sources") return [];
    const all: EvidenceReference[] = [...view.evidence];
    if (content.kind === "matrix")
      for (const row of content.cells) for (const cell of row) all.push(...cell.evidence);
    if (content.kind === "findings") for (const item of content.items) all.push(...item.evidence);
    return [...new Map(all.map((entry) => [entry.key, entry])).values()];
  });
  const icon = $derived(
    compare || view?.content.kind === "comparison"
      ? GitCompareIcon
      : view?.content.kind === "findings"
        ? CheckListIcon
        : view?.content.kind === "table"
          ? Table01Icon
          : view?.content.kind === "chart"
            ? ChartColumnIcon
            : Doc01Icon,
  );
  const kind = $derived(
    compare || view?.content.kind === "comparison"
      ? m.work_lift_comparison()
      : view?.content.kind === "findings"
        ? m.work_env_findings()
        : m.work_env_result(),
  );
  const meta = $derived.by(() => {
    const content = view?.content;
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
    <LiftHeader {kind} title={view.title} {meta} {icon} />
    {#if compare}<Compare
        model={compare}
        correctable={settled}
        onevidence={pick}
        oncorrect={(cell, text) => void correct(cell, text)}
      />
    {:else if view.content.kind === "document"}<div class="reading">
        {#if view.content.formatted}<DocumentView
            document={view.content.formatted}
            onlink={onopen}
          />
        {:else}{#each view.content.paragraphs as paragraph, index (index)}<p>
              {paragraph}
            </p>{/each}{/if}
      </div>
    {:else}<Artifact artifact={view} embedded onevidence={pick} onlink={onopen} />{/if}
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
</style>
