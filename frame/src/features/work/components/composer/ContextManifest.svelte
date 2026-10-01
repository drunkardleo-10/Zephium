<script lang="ts">
  import type {
    WorkContextDisclosureV1,
    WorkContextPurpose,
    WorkContextSelectionV1,
    WorkFailureV1,
  } from "$shared/ipc/bindings";
  import { commands } from "$shared/ipc/bindings";
  import { observe } from "$shared/lib/observe";
  import Icon from "$shared/ui/Icon";
  import { ArrowDown01Icon, Link04Icon } from "../../lib/icons";
  import ContextManifestList from "./ContextManifestList.svelte";
  import * as m from "$shared/i18n/messages";
  let {
    profile,
    selection,
    purpose,
    onreview,
  }: {
    profile: string;
    selection: WorkContextSelectionV1;
    purpose: WorkContextPurpose;
    /** Whether the admitted selection needs the reviewed-plan route. */
    onreview?: (required: boolean) => void;
  } = $props();
  let preview = $state.raw<
    | { kind: "pending" }
    | { kind: "admitted"; disclosure: WorkContextDisclosureV1 }
    | { kind: "refused"; error: WorkFailureV1 | "transport" }
  >({ kind: "pending" });
  let open = $state(false);
  let generation = 0;
  const key = $derived(JSON.stringify([purpose, selection]));
  $effect(() => {
    void key;
    const request = ++generation;
    const current = selection;
    const currentPurpose = purpose;
    preview = { kind: "pending" };
    void (async () => {
      const result = await observe(
        Promise.resolve().then(() => commands.workContextPreview(profile, currentPurpose, current)),
        6000,
      );
      if (request !== generation) return;
      if (result.state !== "received") {
        preview = { kind: "refused", error: "transport" };
        onreview?.(false);
        return;
      }
      const value = result.value;
      preview = value;
      onreview?.(
        value.kind === "admitted" &&
          value.disclosure.items.some((item) => item.visibility === "private"),
      );
    })();
  });
  const requiresReview = $derived(
    preview.kind === "admitted" &&
      purpose === "public_read" &&
      preview.disclosure.items.some((item) => item.visibility === "private"),
  );
  const count = $derived(
    preview.kind === "admitted"
      ? preview.disclosure.items.filter((item) => !item.implicit).length
      : selection.items.length,
  );
  /** What the selection is, said in full to assistive technology and on hover. */
  const said = $derived.by(() => {
    if (failure) return failure;
    if (preview.kind === "pending") return m.work_context_preparing();
    return count === 1 ? m.work_context_using_one() : m.work_context_using({ count });
  });
  const failure = $derived.by(() => {
    if (preview.kind !== "refused") return null;
    switch (preview.error) {
      case "conflict":
        return m.work_context_stale();
      case "capacity":
        return m.work_context_too_large();
      default:
        return m.work_context_unavailable();
    }
  });
</script>

<!-- A compact chip in the field: what goes with the ask, opened above it, never a line of its own. -->
<div class="context-manifest" class:review={requiresReview} class:failed={!!failure}>
  <button
    type="button"
    class="summary"
    aria-expanded={open}
    aria-label={said}
    title={requiresReview ? `${said}. ${m.work_context_review_note()}` : said}
    onclick={() => (open = !open)}
    disabled={preview.kind !== "admitted"}
  >
    <Icon icon={Link04Icon} size={13} />
    <span class="count"
      >{failure
        ? failure
        : count === 1
          ? m.work_context_chip_one()
          : m.work_context_chip({ count })}</span
    >
    {#if preview.kind === "admitted"}<span class="chevron" class:open
        ><Icon icon={ArrowDown01Icon} size={12} /></span
      >{/if}
  </button>
  {#if open && preview.kind === "admitted"}
    <div class="details">
      {#if requiresReview}<p class="note">{m.work_context_review_note()}</p>{/if}
      <ContextManifestList disclosure={preview.disclosure} compact />
    </div>
  {/if}
</div>

<style>
  .context-manifest {
    position: relative;
    display: inline-flex;
    flex: none;
  }

  .count {
    max-inline-size: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .summary {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    block-size: 26px;
    padding: 0 10px 0 8px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-caption);
    cursor: default;
    justify-self: start;
  }

  .review .summary {
    color: var(--color-warning);
  }

  .failed .summary {
    color: var(--color-danger);
  }

  .summary:hover:not(:disabled) {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .chevron {
    display: inline-flex;
    transition: rotate var(--motion-fast) var(--ease-smooth);
  }

  .chevron.open {
    rotate: 180deg;
  }

  .note {
    margin: 0 0 6px;
    color: var(--color-warning);
    font-size: 11.5px;
  }

  /* Opened above the field, over the canvas: the bar never grows for it. */
  .details {
    position: absolute;
    inset-block-end: calc(100% + 10px);
    inset-inline-start: 0;
    z-index: 10;
    inline-size: 320px;
    max-block-size: 280px;
    overflow: auto;
    padding: 8px 10px;
    border-radius: var(--radius-row);
    background: var(--color-float);
    box-shadow: var(--shadow-menu);
  }
</style>
