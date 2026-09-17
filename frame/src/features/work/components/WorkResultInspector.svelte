<script lang="ts">
  import type { WorkSession } from "$domain/work";
  import type { ResultReference } from "../lib/project-environment-results";
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  import Artifact from "$shared/ui/data/Artifact";
  import Evidence, { type EvidenceView } from "$shared/ui/data/Evidence";
  import Compare from "./compare/Compare.svelte";
  import { artifactView } from "../lib/project-work";
  import { compareModel, type ComparePicture } from "../lib/compare";
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
    <h2>{view.title}</h2>
    {#if compare}<Compare model={compare} onevidence={pick} />
    {:else}<Artifact artifact={view} embedded onevidence={pick} onlink={onopen} />{/if}
    {#if evidence}<Evidence {evidence} {onopen} />{/if}
  </section>
{:else}<p role="status">{m.work_artifact_unavailable()}</p>{/if}

<style>
  .result {
    display: flex;
    flex-direction: column;
    gap: 16px;
    min-inline-size: 0;
  }

  h2 {
    margin: 0 28px 0 0;
    font-size: var(--text-title);
    font-weight: 600;
    letter-spacing: -0.01em;
  }
</style>
