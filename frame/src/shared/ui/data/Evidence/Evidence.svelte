<script lang="ts">
  import type { EvidenceView } from "./evidence";
  import * as m from "$shared/i18n/messages";
  let { evidence }: { evidence: EvidenceView } = $props();
  let valid = $derived(
    evidence.state !== "ready" ||
      (evidence.text.length <= 8192 &&
        new TextEncoder().encode(evidence.text).length <= 8192 &&
        evidence.title.length <= 512 &&
        evidence.origin.length <= 2048 &&
        evidence.role.length <= 256 &&
        /^\d{1,20}$/u.test(evidence.sourceBytes)),
  );
</script>

<section aria-label={m.work_sources()} class="evidence">
  {#if !valid}<p role="alert">{m.work_artifact_unavailable()}</p>
  {:else if evidence.state === "loading"}<p role="status">{m.surface_loading()}</p>
  {:else if evidence.state === "unavailable"}<p role="status">{evidence.reason}</p>
  {:else}<header>
      <h2>{evidence.title}</h2>
      <p>{evidence.origin} · {evidence.role}</p>
    </header>
    <p class="historical">{m.work_historical_source()}</p>
    <blockquote>{evidence.text}</blockquote>
    <p class="extent">
      {evidence.truncated ? m.work_source_truncated() : m.work_source_complete()} · {m.work_source_bytes(
        { bytes: evidence.sourceBytes },
      )}
    </p>{/if}
</section>

<style>
  .evidence {
    color: var(--color-text);
    overflow-wrap: anywhere;
  }

  h2 {
    font-size: var(--text-title);
    margin-block: 0 8px;
  }

  p {
    color: var(--color-muted);
    font-size: var(--text-caption);
    line-height: 1.5;
  }

  blockquote {
    max-height: 360px;
    overflow: auto;
    margin: 16px 0;
    padding: 16px;
    white-space: pre-wrap;
    border-inline-start: 2px solid var(--color-border-strong);
    background: var(--color-fill);
    line-height: 1.65;
  }
</style>
