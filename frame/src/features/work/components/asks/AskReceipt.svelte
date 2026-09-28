<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import { Alert02Icon, MinusSignIcon, Tick02Icon } from "./icons";

  /** A decided question, kept where it stood as one quiet line of history. */
  let {
    tone,
    status,
    text,
    detail = null,
  }: {
    tone: "done" | "declined" | "failed" | "working";
    /** "Sent", "Not now", "Allowed". */
    status: string;
    /** What it was about: "Send to #design". */
    text: string;
    /** Rust's note when it did not go as asked. */
    detail?: string | null;
  } = $props();
</script>

<p class="receipt {tone}" role="status">
  <span class="glyph" aria-hidden="true">
    {#if tone === "working"}<span class="dot"></span>
    {:else}<Icon
        icon={tone === "done" ? Tick02Icon : tone === "failed" ? Alert02Icon : MinusSignIcon}
        size={13}
        strokeWidth={2}
      />{/if}
  </span>
  <span class="words"><strong>{status}</strong><span class="text">{text}</span></span>
  {#if detail}<span class="detail">{detail}</span>{/if}
</p>

<style>
  .receipt {
    display: grid;
    grid-template-columns: 16px minmax(0, 1fr);
    column-gap: 6px;
    align-items: baseline;
    box-sizing: border-box;
    max-inline-size: 360px;
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 16px;
  }

  .glyph {
    display: grid;
    place-items: center;
    align-self: center;
    block-size: 16px;
  }

  .done .glyph {
    color: var(--color-success);
  }

  .failed .glyph {
    color: var(--color-warning);
  }

  .dot {
    inline-size: 6px;
    block-size: 6px;
    border-radius: var(--radius-capsule);
    background: var(--color-muted);
    animation: breathe 1.6s var(--ease-in-out) infinite;
  }

  .words {
    display: flex;
    gap: 6px;
    min-inline-size: 0;
  }

  strong {
    flex: none;
    color: var(--color-text);
    font-weight: 550;
  }

  .text {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .detail {
    grid-column: 2;
    color: var(--color-faint);
    font-size: var(--text-caption);
    line-height: 14px;
  }

  @keyframes breathe {
    50% {
      opacity: 0.35;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .dot {
      animation: none;
    }
  }
</style>
