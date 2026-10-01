<script lang="ts">
  import type {
    WorkEnvironmentReference,
    WorkPageV1,
    WorkRuntimeProjection,
  } from "$shared/ipc/bindings";
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  import { mediaUrl, pageFrameUrl } from "$domain/resources";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import HostGlyph from "./cards/HostGlyph.svelte";
  import LiftHeader, { type LiftAction } from "./LiftHeader.svelte";
  import { displayHost } from "$shared/ui/data/Artifact/artifact";
  import { Cancel01Icon, Tick02Icon } from "../lib/icons";
  import { cellText, type ComparePicture } from "../lib/compare";
  import { subjectDetail, subjectMarkdown, subjectPage } from "../lib/subject-detail";
  import { subjectsOf } from "../lib/subjects";
  import * as m from "$shared/i18n/messages";
  type SubjectReference = Extract<WorkEnvironmentReference, { kind: "subject" }>;
  let {
    reference,
    objectives,
    pictures = [],
    pages = [],
    note,
    onopen,
    onfile,
    onask,
  }: {
    reference: SubjectReference;
    objectives: ReadonlyMap<string, WorkRuntimeProjection>;
    /** Admitted pictures of this subject; a remote address is never loaded here. */
    pictures?: readonly (ComparePicture & { name: string })[];
    /** The pages the run read, with the frames it captured of them. */
    pages?: readonly WorkPageV1[];
    /** Save as note for this subject's Markdown, until the note exists. */
    note?: (markdown: string) => LiftAction | undefined;
    onopen?: (url: string) => void;
    onfile?: (record: string) => void;
    /** Ask about this: the composer takes the subject's name. */
    onask?: (name: string) => void;
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
      : { facts: [], sources: [], quotes: {}, price: undefined },
  );
  const homepage = $derived(subject?.homepage ?? undefined);
  const host = $derived(homepage ? displayHost(homepage) : "");
  let shown = $state(0);
  const picture = $derived(pictures[Math.min(shown, Math.max(0, pictures.length - 1))]);
  const pictureSource = $derived(picture ? mediaUrl(picture.profile, picture.digest, 1600) : null);
  const page = $derived(subjectPage(pages, homepage, detail.sources));
  const frameSource = $derived(
    page?.frame ? pageFrameUrl(page.attempt, page.step, page.frame.generation) : null,
  );
  // A picture or a frame that will not load gives way to the next thing, never a broken glyph.
  let failed = $state<readonly string[]>([]);
  const hero = $derived(
    pictureSource && !failed.includes(pictureSource)
      ? { kind: "picture" as const, src: pictureSource }
      : frameSource && !failed.includes(frameSource)
        ? { kind: "frame" as const, src: frameSource }
        : { kind: "site" as const },
  );
  const markdown = $derived(
    subject
      ? subjectMarkdown(
          subject,
          detail,
          (value) => cellText(value, m.work_yes(), m.work_no()),
          m.work_sources(),
        )
      : "",
  );
  const save = $derived(subject ? note?.(markdown) : undefined);
  /** A fact under the pointer lights the sources behind it. */
  let pointed = $state<readonly string[] | null>(null);
  function open(reference: EvidenceReference) {
    if (reference.file) onfile?.(reference.file.record);
    else if (reference.url) onopen?.(reference.url);
  }
  const wide = (text: string) => text.length > 44;
</script>

{#if subject}
  <section class="subject">
    <LiftHeader
      kind={m.work_env_subject()}
      title={subject.name}
      {host}
      url={homepage}
      onhost={homepage ? () => onopen?.(homepage) : undefined}
      primary={homepage
        ? { label: m.work_env_open_page(), onclick: () => onopen?.(homepage) }
        : undefined}
    >
      {#snippet leading()}<HostGlyph
          url={homepage ?? ""}
          {host}
          size={16}
          initial={false}
        />{/snippet}
      {#snippet actions()}{#if save}<Button
            size="compact"
            disabled={save.disabled}
            title={save.title}
            onclick={save.onclick}>{save.label}</Button
          >{/if}{#if onask}<Button size="compact" onclick={() => onask?.(subject!.name)}
            >{m.work_lift_ask()}</Button
          >{/if}{/snippet}
    </LiftHeader>
    <div class="body">
      <div class="main">
        <figure class="hero {hero.kind}">
          {#if hero.kind === "site"}<span class="site" aria-hidden="true"
              ><HostGlyph url={homepage ?? ""} {host} size={64} initial={false} /></span
            >{:else}<img
              src={hero.src}
              alt={hero.kind === "picture" ? subject.name : ""}
              decoding="async"
              draggable="false"
              onerror={(event) =>
                (failed = [...failed, event.currentTarget.getAttribute("src") ?? ""])}
            />{/if}
        </figure>
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
                  <img src={mediaUrl(candidate.profile, candidate.digest, 160)} alt="" />
                </button>
              </li>
            {/each}
          </ul>
        {/if}
        {#if detail.price || subject.descriptor}<div class="lead">
            {#if detail.price}<p class="price">{detail.price}</p>{/if}
            {#if subject.descriptor}<p class="descriptor">{subject.descriptor}</p>{/if}
          </div>{/if}
        {#if detail.facts.length}
          <dl class="facts">
            {#each detail.facts as fact (fact.key)}{@const text =
                fact.value.kind === "text" || fact.value.kind === "number" ? fact.value.text : ""}
              <div
                class="fact"
                class:wide={wide(text)}
                role="group"
                aria-label={fact.label}
                onpointerenter={() => (pointed = fact.evidence.map((source) => source.key))}
                onpointerleave={() => (pointed = null)}
              >
                <dt>{fact.label}</dt>
                <dd class:numeric={fact.numeric}>
                  {#if fact.value.kind === "mark"}<span
                      class="glyph"
                      class:yes={fact.value.yes}
                      aria-label={fact.value.yes ? m.work_yes() : m.work_no()}
                      ><Icon icon={fact.value.yes ? Tick02Icon : Cancel01Icon} size={15} /></span
                    >{:else}{text}{/if}
                </dd>
              </div>
            {/each}
          </dl>
        {/if}
      </div>
      {#if detail.sources.length}<aside class="sources" aria-label={m.work_sources()}>
          <h3>{m.work_sources()}<span class="count">{detail.sources.length}</span></h3>
          <ul>
            {#each detail.sources as source (source.key)}{@const quote = detail.quotes[source.key]}
              <li>
                <button
                  type="button"
                  class="source"
                  class:lit={pointed?.includes(source.key)}
                  class:quiet={pointed && !pointed.includes(source.key)}
                  onclick={() => open(source)}
                >
                  <span class="mark" aria-hidden="true"
                    ><HostGlyph
                      host={source.origin ?? ""}
                      url={source.url}
                      file={!!source.file}
                      size={16}
                      initial={false}
                    /></span
                  >
                  <span class="words">
                    <span class="title">{source.label}</span>
                    {#if source.origin}<span class="where">{source.origin}</span>{/if}
                    {#if quote}<span class="quote">{quote}</span>{/if}
                  </span>
                </button>
              </li>
            {/each}
          </ul>
        </aside>{/if}
    </div>
  </section>
{:else}<p role="status">{m.work_artifact_unavailable()}</p>{/if}

<style>
  .subject {
    display: flex;
    flex-direction: column;
    gap: 20px;
    min-inline-size: 0;
    container-type: inline-size;
  }

  /* The object and what is known of it on the left; where it was learned on the right. */
  .body {
    display: grid;
    grid-template-columns: minmax(0, 640px) minmax(240px, 320px);
    gap: 32px;
    align-items: start;
  }

  @container (width < 760px) {
    .body {
      grid-template-columns: minmax(0, 1fr);
    }
  }

  .main {
    display: flex;
    flex-direction: column;
    gap: 20px;
    min-inline-size: 0;
  }

  .hero {
    display: grid;
    place-items: center;
    margin: 0;
    aspect-ratio: 248 / 168;
    border-radius: var(--radius-card);
    background: var(--color-fill);
    box-shadow: inset 0 0 0 1px var(--color-border);
    overflow: hidden;
  }

  .thumbs img {
    inline-size: 100%;
    block-size: 100%;
    object-fit: cover;
  }

  .hero img {
    display: block;
    inline-size: 100%;
    block-size: 100%;
  }

  .hero.picture img {
    object-fit: contain;
  }

  /* The page as the run saw it, from its top, as the page card shows it. */
  .hero.frame img {
    object-fit: cover;
    object-position: top;
  }

  .hero.site {
    aspect-ratio: auto;
    block-size: 200px;
  }

  .site {
    display: grid;
    place-items: center;
  }

  .thumbs {
    list-style: none;
    display: flex;
    gap: 6px;
    margin: -12px 0 0;
    padding: 0;
  }

  .thumbs button {
    inline-size: 40px;
    block-size: 40px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-inset);
    background: var(--color-fill);
    box-shadow: inset 0 0 0 1px var(--color-border);
    overflow: hidden;
    cursor: default;
  }

  .thumbs button.on {
    box-shadow: inset 0 0 0 2px var(--color-accent);
  }

  .lead {
    display: flex;
    flex-direction: column;
    gap: 4px;
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
    font-size: var(--text-body);
    line-height: 1.5;
  }

  /* Two columns of facts: the label quiet above, the value in the body size. */
  .facts {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 0 24px;
    margin: 0;
  }

  @container (width < 480px) {
    .facts {
      grid-template-columns: minmax(0, 1fr);
    }
  }

  .fact {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding-block: 10px;
    border-block-start: 1px solid var(--color-border);
  }

  .fact.wide {
    grid-column: 1 / -1;
  }

  dt {
    color: var(--color-muted);
    font-size: var(--text-caption);
    line-height: 14px;
  }

  dd {
    margin: 0;
    min-inline-size: 0;
    font-size: var(--text-body);
    line-height: 19px;
    overflow-wrap: anywhere;
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

  .sources {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-inline-size: 0;
  }

  h3 {
    display: flex;
    align-items: baseline;
    gap: 6px;
    margin: 0 0 2px;
    padding-inline: 8px;
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-weight: 500;
  }

  .count {
    color: var(--color-faint);
    font-variant-numeric: tabular-nums;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .source {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    inline-size: 100%;
    padding: 8px;
    border: 0;
    border-radius: var(--radius-row);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    text-align: start;
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      opacity var(--motion-fast) var(--ease-out);
  }

  .source:hover,
  .source.lit {
    background: var(--row-hover);
  }

  .source.quiet {
    opacity: 0.5;
  }

  .source:focus-visible,
  .thumbs button:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }

  .mark {
    display: grid;
    flex: none;
    place-items: center;
    block-size: 18px;
  }

  .words {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-inline-size: 0;
  }

  .title {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    font-size: var(--text-label);
    font-weight: 500;
    line-height: 18px;
    overflow-wrap: anywhere;
  }

  .where {
    overflow: hidden;
    color: var(--color-muted);
    font-size: var(--text-caption);
    line-height: 14px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .quote {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    margin-block-start: 4px;
    padding-inline-start: 8px;
    overflow: hidden;
    border-inline-start: 2px solid var(--color-border-strong);
    color: var(--color-label-secondary);
    font-size: var(--text-label);
    line-height: 17px;
  }
</style>
