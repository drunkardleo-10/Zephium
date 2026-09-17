<script lang="ts">
  import type { WorkEnvironmentReference, WorkRuntimeProjection } from "$shared/ipc/bindings";
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  import { mediaUrl } from "$domain/resources";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import HostGlyph from "./cards/HostGlyph.svelte";
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
  function open(reference: EvidenceReference) {
    if (reference.file) onfile?.(reference.file.record);
    else if (reference.url) onopen?.(reference.url);
  }
</script>

{#if subject}
  <section class="product">
    <header>
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
      <div class="identity">
        <p class="kind">{m.work_env_subject()}</p>
        <h2>{subject.name}</h2>
        {#if detail.price}<p class="price">{detail.price}</p>{/if}
        {#if subject.descriptor}<p class="descriptor">{subject.descriptor}</p>{/if}
        {#if subject.homepage}
          <Button size="compact" onclick={() => onopen?.(subject!.homepage!)}
            >{m.work_env_open_page()}</Button
          >
        {/if}
      </div>
    </header>
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
                    ><HostGlyph host={source.origin ?? ""} file={!!source.file} size={13} /><span
                      >{source.origin || source.label}</span
                    ></button
                  >
                {/each}
              </span>
            </dd>
          </div>
        {/each}
      </dl>
    {/if}
    {#if detail.sources.length}
      <section class="sources" aria-label={m.work_sources()}>
        <h3>{m.work_sources()}</h3>
        <ul>
          {#each detail.sources as source (source.key)}
            <li>
              <button type="button" onclick={() => open(source)}>
                <HostGlyph host={source.origin ?? ""} file={!!source.file} size={20} />
                <span class="source">
                  <strong>{source.label}</strong>
                  <span>{source.origin}</span>
                </span>
              </button>
            </li>
          {/each}
        </ul>
      </section>
    {/if}
  </section>
{:else}<p role="status">{m.work_artifact_unavailable()}</p>{/if}

<style>
  .product {
    display: flex;
    flex-direction: column;
    gap: 20px;
    min-inline-size: 0;
  }

  header {
    display: flex;
    gap: 20px;
    padding-inline-end: 28px;
  }

  .gallery {
    display: flex;
    flex-direction: column;
    gap: 8px;
    flex: none;
    inline-size: 220px;
  }

  .hero {
    display: block;
    inline-size: 220px;
    block-size: 220px;
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
    font-size: 48px;
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
    inline-size: 46px;
    block-size: 46px;
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

  .identity {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
    min-inline-size: 0;
  }

  .kind {
    margin: 0;
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  h2 {
    margin: 0;
    font-size: var(--text-title);
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  h3 {
    margin: 0 0 6px;
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-weight: 500;
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

  .chip span,
  .source strong,
  .source span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chip:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .sources ul {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
  }

  .sources button {
    display: flex;
    align-items: center;
    gap: 10px;
    inline-size: 100%;
    box-sizing: border-box;
    padding: 7px 10px;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    text-align: start;
    cursor: default;
  }

  .sources button:hover {
    background: var(--color-fill-hover);
  }

  .source {
    display: flex;
    flex-direction: column;
    min-inline-size: 0;
  }

  .source strong {
    font-size: var(--text-label);
    font-weight: 550;
  }

  .source span {
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .thumbs button:focus-visible,
  .chip:focus-visible,
  .sources button:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }
</style>
