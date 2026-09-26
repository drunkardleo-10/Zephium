<script lang="ts">
  import type { ArtifactContent, EvidenceReference } from "$shared/ui/data/Artifact";
  import HostGlyph from "./cards/HostGlyph.svelte";
  import * as m from "$shared/i18n/messages";
  type Findings = Extract<ArtifactContent, { kind: "findings" }>;
  let {
    content,
    onevidence,
  }: {
    content: Findings;
    onevidence?: (reference: EvidenceReference) => void;
  } = $props();
  const confidence = {
    supported: m.work_confidence_supported,
    inferred: m.work_confidence_inferred,
    unverified: m.work_confidence_unverified,
    contradicted: m.work_confidence_contradicted,
  };
  /** Claims about each subject together, in the order the subjects were named. */
  type Group = {
    key: string;
    subject: (typeof content.subjects)[number] | undefined;
    items: { item: (typeof content.items)[number]; index: number }[];
  };
  const groups = $derived.by((): Group[] => {
    const items = content.items.map((item, index) => ({ item, index }));
    const named = (subject: number | undefined) =>
      subject !== undefined && !!content.subjects[subject];
    if (!items.some((entry) => named(entry.item.subject)))
      return [{ key: "all", subject: undefined, items }];
    const out = content.subjects.flatMap((subject, index): Group[] => {
      const about = items.filter((entry) => entry.item.subject === index);
      return about.length ? [{ key: `subject:${index}`, subject, items: about }] : [];
    });
    const rest = items.filter((entry) => !named(entry.item.subject));
    if (rest.length) out.push({ key: "rest", subject: undefined, items: rest });
    return out;
  });
  const grouped = $derived(groups.some((group) => group.subject));
</script>

<!-- A reading list: what was found, where each claim came from, beside it. -->
<div class="findings">
  {#each groups as group (group.key)}
    <section aria-label={group.subject?.name ?? m.work_findings_other()}>
      {#if group.subject}<h3>
          <span class="glyph" aria-hidden="true"
            ><HostGlyph url={group.subject.homepage ?? ""} size={16} initial={false} /></span
          >{group.subject.name}
        </h3>{:else if grouped}<h3>{m.work_findings_other()}</h3>{/if}
      <ol>
        {#each group.items as { item, index } (index)}{@const subject =
            item.subject === undefined ? undefined : content.subjects[item.subject]}
          <li class={item.confidence}>
            <span
              class="marker"
              role="img"
              aria-label={confidence[item.confidence]()}
              title={confidence[item.confidence]()}
            ></span>
            <div class="body">
              <p class="claim">{item.claim}</p>
              {#if item.detail}<p class="detail">{item.detail}</p>{/if}
              {#if (subject && !grouped) || item.evidence.length}<p class="cites">
                  {#if subject && !grouped}<span class="subject"
                      ><HostGlyph url={subject.homepage ?? ""} size={12} initial={false} /><span
                        >{subject.name}</span
                      ></span
                    >{/if}
                  {#each item.evidence as source (source.key)}<button
                      type="button"
                      class="cite"
                      title={source.url ?? source.label}
                      onclick={() => onevidence?.(source)}
                      ><HostGlyph
                        host={source.origin ?? ""}
                        url={source.url}
                        file={!!source.file}
                        size={12}
                        initial={false}
                      /><span>{source.label || source.origin}</span></button
                    >{/each}
                </p>{/if}
            </div>
          </li>
        {/each}
      </ol>
    </section>
  {/each}
</div>

<style>
  .findings {
    display: flex;
    flex-direction: column;
    gap: 28px;
    inline-size: 100%;
    max-inline-size: 640px;
    margin-inline: auto;
  }

  section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    font-size: var(--text-body);
    font-weight: 600;
  }

  .glyph {
    display: grid;
    place-items: center;
    inline-size: 24px;
    block-size: 24px;
    border-radius: var(--radius-inset);
    background: var(--color-fill);
  }

  ol {
    display: flex;
    flex-direction: column;
    gap: 16px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    gap: 12px;
    align-items: flex-start;
  }

  .marker {
    flex: none;
    inline-size: 6px;
    block-size: 6px;
    margin-block-start: 9px;
    border-radius: 50%;
    background: var(--color-faint);
  }

  .supported .marker {
    background: var(--color-success);
  }

  .contradicted .marker {
    background: var(--color-danger);
  }

  .inferred .marker {
    background: var(--color-info);
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-inline-size: 0;
  }

  .claim {
    margin: 0;
    font-size: var(--text-page-body);
    line-height: 1.55;
    text-wrap: pretty;
    overflow-wrap: anywhere;
  }

  .detail {
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-body);
    line-height: 1.5;
  }

  .cites {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 12px;
    margin: 2px 0 0;
  }

  .subject,
  .cite {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    max-inline-size: 280px;
    font-size: var(--text-caption);
    line-height: 16px;
  }

  .subject span,
  .cite span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .subject {
    padding: 1px 8px 1px 4px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-label-secondary);
  }

  .cite {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-caption);
    cursor: default;
    transition: color var(--motion-fast) var(--ease-out);
  }

  .cite:hover {
    color: var(--color-text);
  }

  .cite:hover span {
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .cite:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
    border-radius: var(--radius-inset);
  }
</style>
