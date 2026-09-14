<script lang="ts">
  import type { EvidenceReference, FindingView, SubjectView } from "./artifact";
  import EvidenceChips from "./EvidenceChips.svelte";
  let {
    subjects,
    items,
    labels,
    onevidence,
  }: {
    subjects: readonly SubjectView[];
    items: readonly FindingView[];
    labels: {
      confidence: Record<FindingView["confidence"], string>;
      generalKnowledge: string;
    };
    onevidence?: (reference: EvidenceReference) => void;
  } = $props();
</script>

<ul class="findings">
  {#each items as finding, index (index)}
    <li class={`finding ${finding.confidence}`}>
      <span class="marker" aria-hidden="true"></span>
      <div class="body">
        <p class="claim">{finding.claim}</p>
        {#if finding.detail}<p class="detail">{finding.detail}</p>{/if}
        <div class="meta">
          <span class="confidence">{labels.confidence[finding.confidence]}</span>
          {#if finding.subject !== undefined && subjects[finding.subject]}<span class="subject"
              >{subjects[finding.subject]!.name}</span
            >{/if}
          {#if finding.generalKnowledge}<span class="subject">{labels.generalKnowledge}</span>{/if}
          <EvidenceChips references={finding.evidence} compact {onevidence} />
        </div>
      </div>
    </li>
  {/each}
</ul>

<style>
  .findings {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .finding {
    display: flex;
    gap: 10px;
    align-items: flex-start;
  }

  .marker {
    flex: none;
    inline-size: 8px;
    block-size: 8px;
    margin-block-start: 6px;
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
    min-inline-size: 0;
  }

  .claim {
    margin: 0;
    font-size: var(--text-body);
    line-height: 19px;
  }

  .detail {
    margin: 4px 0 0;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 17px;
  }

  .meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin-block-start: 6px;
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  .confidence {
    text-transform: capitalize;
  }

  .subject {
    padding: 1px 6px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-label-secondary);
  }
</style>
