<script lang="ts">
  import type { WorkEnvironmentReference, WorkRuntimeProjection } from "$shared/ipc/bindings";
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  import { mediaUrl } from "$domain/resources";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import HostGlyph from "./cards/HostGlyph.svelte";
  import LiftHeader from "./LiftHeader.svelte";
  import SourcesRail from "./SourcesRail.svelte";
  import { displayHost } from "$shared/ui/data/Artifact/artifact";
  import { Cancel01Icon, Tick02Icon } from "../lib/icons";
  import type { ComparePicture } from "../lib/compare";
  import { subjectDetail } from "../lib/subject-detail";
  import { subjectsOf } from "../lib/subjects";
  import * as m from "$shared/i18n/messages";
  type SubjectReference = Extract<WorkEnvironmentReference, { kind: "subject" }>;
  let {
    reference,
    objectives,
    pictures = [],
    onopen,
    onfile,
  }: {
    reference: SubjectReference;
    objectives: ReadonlyMap<string, WorkRuntimeProjection>;
    /** Admitted pictures of this subject; a remote address is never loaded here. */
    pictures?: readonly (ComparePicture & { name: string })[];
    onopen?: (url: string) => void;
    onfile?: (record: string) => void;
  } = $props();
  const execution = $derived(
    objectives
      .get(reference.objective)
      ?.executions.find((execution) => execution.id === reference.execution),
  );
  const subject = $derived.by(() => {
    const artifact = execution?.artifacts.find((artifact) => artifact.id === reference.artifact);
    return artifact ? subjectsOf(artifact)[reference.index] : undefined;
  });
  const detail = $derived(
    execution && subject
      ? subjectDetail(execution, subject)
      : { facts: [], sources: [], price: undefined },
  );
  let shown = $state(0);
  const picture = $derived(pictures[Math.min(shown, Math.max(0, pictures.length - 1))]);
  const meta = $derived(subject?.homepage ? displayHost(subject.homepage) : "");
  function open(reference: EvidenceReference) {
    if (reference.file) onfile?.(reference.file.record);
    else if (reference.url) onopen?.(reference.url);
  }
</script>

{#if subject}
  <section class="product">
    <LiftHeader kind={m.work_env_subject()} title={subject.name} {meta}>
      {#snippet leading()}<HostGlyph
          host={subject?.homepage ? displayHost(subject.homepage) : ""}
          size={16}
        />{/snippet}
      {#snippet actions()}{#if subject?.homepage}<Button
            size="compact"
            onclick={() => onopen?.(subject!.homepage!)}>{m.work_env_open_page()}</Button
          >{/if}{/snippet}
    </LiftHeader>
    <div class="body">
      <div class="gallery">
        <span class="hero">
          {#if picture}<img src={mediaUrl(picture.profile, picture.digest)} alt={subject.name} />
          {:else}<span class="mark" aria-hidden="true">{subject.name.slice(0, 1)}</span>{/if}
        </span>
        {#if pictures.length > 1}
          <ul class="thumbs">
            {#each pictures as candidate, index (candidate.digest)}
              <li>
                <button
                  type="button"
                  class:on={index === shown}
                  aria-label={candidate.name}
                  aria-pressed={index === shown}
                  onclick={() => (shown = index)}
                >
                  <img src={mediaUrl(candidate.profile, candidate.digest)} alt="" />
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
      <div class="details">
        {#if detail.price}<p class="price">{detail.price}</p>{/if}
        {#if subject.descriptor}<p class="descriptor">{subject.descriptor}</p>{/if}
        {#if detail.facts.length}
          <dl class="facts">
            {#each detail.facts as fact (fact.key)}
              <div class="fact">
                <dt>{fact.label}</dt>
                <dd class:numeric={fact.numeric}>
                  {#if fact.value.kind === "mark"}<span
                      class="glyph"
                      class:yes={fact.value.yes}
                      aria-label={fact.value.yes ? m.work_yes() : m.work_no()}
                      ><Icon icon={fact.value.yes ? Tick02Icon : Cancel01Icon} size={15} /></span
                    >
                  {:else if fact.value.kind !== "unknown"}{fact.value.text}{/if}
                  <span class="chips">
                    {#each fact.evidence as source (source.key)}
                      <button
                        type="button"
                        class="chip"
                        title={source.label}
                        onclick={() => open(source)}
                        ><HostGlyph
                          host={source.origin ?? ""}
                          file={!!source.file}
                          size={13}
                        /><span>{source.origin || source.label}</span></button
                      >
                    {/each}
                  </span>
                </dd>
              </div>
            {/each}
          </dl>
        {/if}
      </div>
    </div>
    <SourcesRail references={detail.sources} onpick={open} />
  </section>
{:else}<p role="status">{m.work_artifact_unavailable()}</p>{/if}

<style>
  .product {
    display: flex;
    flex-direction: column;
    gap: 20px;
    min-inline-size: 0;
  }

  .body {
    display: flex;
    gap: 20px;
    min-inline-size: 0;
  }

  .gallery {
    display: flex;
    flex-direction: column;
    gap: 8px;
    flex: none;
    inline-size: 160px;
  }

  .hero {
    display: block;
    inline-size: 160px;
    block-size: 160px;
    border-radius: var(--radius-lg);
    background: var(--color-fill);
    overflow: hidden;
  }

  .hero img {
    inline-size: 100%;
    block-size: 100%;
    object-fit: contain;
  }

  .mark {
    display: grid;
    place-items: center;
    inline-size: 100%;
    block-size: 100%;
    background: var(--color-accent-soft);
    font-size: 40px;
    font-weight: 700;
    text-transform: uppercase;
  }

  .thumbs {
    list-style: none;
    display: flex;
    gap: 6px;
    margin: 0;
    padding: 0;
  }

  .thumbs button {
    inline-size: 34px;
    block-size: 34px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: var(--color-fill);
    box-shadow: inset 0 0 0 1px var(--color-border);
    overflow: hidden;
    cursor: default;
  }

  .thumbs button.on {
    box-shadow: inset 0 0 0 2px var(--color-accent);
  }

  .thumbs img {
    inline-size: 100%;
    block-size: 100%;
    object-fit: cover;
  }

  .details {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 12px;
    min-inline-size: 0;
  }

  .price {
    margin: 0;
    font-size: var(--text-title);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  .descriptor {
    margin: 0;
    color: var(--color-muted);
  }

  .facts {
    display: flex;
    flex-direction: column;
    gap: 0;
    margin: 0;
  }

  .fact {
    display: grid;
    grid-template-columns: minmax(120px, 30%) 1fr;
    gap: 12px;
    padding-block: 9px;
    border-block-end: 1px solid var(--color-border);
    font-size: var(--text-label);
  }

  dt {
    color: var(--color-muted);
  }

  dd {
    margin: 0;
    min-inline-size: 0;
  }

  dd.numeric {
    font-variant-numeric: tabular-nums;
  }

  .glyph {
    display: inline-grid;
    place-items: center;
    color: var(--color-faint);
  }

  .glyph.yes {
    color: var(--color-success);
  }

  .chips {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-inline-start: 8px;
    opacity: 0;
    transition: opacity var(--motion-fast) var(--ease-smooth);
  }

  .fact:hover .chips,
  .fact:focus-within .chips {
    opacity: 1;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    max-inline-size: 140px;
    padding: 1px 7px 1px 2px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-caption);
    cursor: default;
  }

  .chip span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chip:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .thumbs button:focus-visible,
  .chip:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }
</style>
