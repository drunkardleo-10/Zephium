<script lang="ts">
  import type { WorkSession } from "$domain/work";
  import type { ResultReference } from "../lib/project-environment-results";
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  import Evidence, { type EvidenceView } from "$shared/ui/data/Evidence";
  import Button from "$shared/ui/Button";
  import { untrack } from "svelte";
  import * as m from "$shared/i18n/messages";
  let {
    session,
    reference,
    source,
    onopen,
  }: {
    session: WorkSession;
    reference: ResultReference;
    source: EvidenceReference | null;
    onopen?: (url: string) => void;
  } = $props();
  const execution = $derived(
    session.projection?.work.id === reference.objective
      ? session.projection.executions.find((value) => value.id === reference.execution)
      : undefined,
  );
  const artifact = $derived(execution?.artifacts.find((value) => value.id === reference.artifact));
  const user = $derived(
    execution?.user_artifacts?.find((value) => value.artifact === reference.artifact),
  );
  const links = $derived(user?.edited_data ? user.evidence : (artifact?.evidence ?? []));
  let evidence = $state.raw<EvidenceView | null>(null);
  let selectedSource = $derived(source);
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

{#if artifact}<h2>{artifact.title}</h2>
  {#each links as link, index (`${link.extraction_id}:${link.source_id}`)}<Button
      size="compact"
      onclick={() =>
        (selectedSource = {
          key: `${link.extraction_id}:${link.source_id}`,
          label: m.work_source_number({ number: index + 1 }),
        })}>{m.work_source_number({ number: index + 1 })}</Button
    >{/each}
  {#if evidence}<Evidence {evidence} {onopen} />{/if}
{:else}<p role="status">{m.work_artifact_unavailable()}</p>{/if}
