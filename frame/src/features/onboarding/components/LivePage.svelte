<script lang="ts">
  import type { Mark as MarkType } from "../lib/catalog";
  import Mark from "$shared/ui/BrandMark";

  let {
    kind,
    site,
    mark,
  }: {
    /** Which kind of page is open: each moves the way that page does. */
    kind: "issues" | "review" | "chat";
    site: string;
    mark: MarkType;
  } = $props();
</script>

<!-- A page Work has open, still running: what an agent reads is the live
     site, not a snapshot of it. Drawn, not captured, so no one's page is
     shown, and each keeps moving the way its kind of page does. -->
<div class="page" data-kind={kind}>
  <div class="bar">
    <Mark {mark} size={9} /><span>{site}</span><i class="live"></i>
  </div>
  <div class="body">
    {#if kind === "issues"}
      {#each [0, 1, 2, 3] as row (row)}
        <div class="issue" style:--row={row}>
          <i class="state"></i><i class="text" style:inline-size={`${[64, 52, 70, 46][row]}%`}></i>
        </div>
      {/each}
    {:else if kind === "review"}
      <div class="pr"><i class="pr-title"></i><b class="pr-state"></b></div>
      {#each [0, 1, -1, -1, 1, 0] as change, row (row)}
        <div class="diff" data-change={change}>
          <i class="line"></i><i style:inline-size={`${[58, 72, 48, 54, 80, 40][row]}%`}></i>
        </div>
      {/each}
      <!-- A reviewer's note landing on the changed line, then the merge. -->
      <div class="comment"><i class="who"></i><span><i></i><i></i></span></div>
    {:else}
      {#each [0, 1, 2] as message (message)}
        <div class="message" style:--row={message}>
          <i class="who"></i><span
            ><i style:inline-size={`${[70, 54, 62][message]}%`}></i><i
              style:inline-size={`${[44, 66, 38][message]}%`}
            ></i></span
          >
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    inline-size: 100%;
    block-size: 100%;
    overflow: hidden;
    border-radius: var(--radius-inset);
    background: var(--color-surface);
    box-shadow:
      var(--shadow-raised),
      inset 0 0 0 0.5px var(--color-border);
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 5px;
    flex: none;
    block-size: 16px;
    padding-inline: 6px;
    background: var(--color-fill);
    color: var(--color-muted);
    font-size: 7.5px;
    white-space: nowrap;
  }

  /* Live: a slow pulse, the one sign the page is still being read. */
  .live {
    inline-size: 5px;
    block-size: 5px;
    margin-inline-start: auto;
    border-radius: var(--radius-capsule);
    background: var(--color-success);
    animation: pulse 1.8s ease-in-out infinite;
  }

  .body {
    position: relative;
    display: grid;
    align-content: start;
    gap: 5px;
    padding: 7px 8px;
  }

  .body i {
    display: block;
    block-size: 3px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill-strong);
  }

  /* Issues close one after another. */
  .issue {
    display: flex;
    align-items: center;
    gap: 5px;
  }

  .issue .state {
    flex: none;
    inline-size: 6px;
    block-size: 6px;
    background: transparent;
    box-shadow: inset 0 0 0 1px var(--color-faint);
    animation: close 5.6s var(--ease-out) infinite;
    animation-delay: calc(var(--row) * 1.4s);
  }

  /* A pull request under review: a note lands on a changed line, and the
     request turns from open to merged. */
  .pr {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-block-end: 2px;
  }

  .body .pr-title {
    flex: 1;
    block-size: 4px;
    max-inline-size: 62%;
    background: var(--color-muted);
  }

  .pr-state {
    inline-size: 16px;
    block-size: 6px;
    margin-inline-start: auto;
    border-radius: var(--radius-capsule);
    box-shadow: inset 0 0 0 1px var(--color-success);
    animation: merge 7.2s var(--ease-out) infinite;
  }

  .diff {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 1px 3px;
    border-radius: 2px;
  }

  .body .diff .line {
    flex: none;
    inline-size: 6px;
    background: var(--color-fill);
  }

  .diff[data-change="1"] {
    background: color-mix(in srgb, var(--color-success) 14%, transparent);
  }

  .diff[data-change="-1"] {
    background: color-mix(in srgb, var(--color-danger) 14%, transparent);
  }

  .comment {
    position: absolute;
    inset-inline: 12px 8px;
    inset-block-start: 41px;
    display: flex;
    gap: 5px;
    padding: 5px 6px;
    border-radius: 3px;
    background: var(--color-surface);
    box-shadow:
      var(--shadow-raised),
      inset 0 0 0 0.5px var(--color-border-strong);
    transform-origin: 10% 0;
    animation: note 7.2s var(--ease-emphasized) infinite;
  }

  .comment .who {
    flex: none;
    inline-size: 8px;
    block-size: 8px;
    background: var(--color-fill-strong);
  }

  .comment span {
    display: grid;
    flex: 1;
    gap: 3px;
  }

  .comment span i:last-child {
    inline-size: 60%;
  }

  /* A thread, with the newest message arriving. */
  .message {
    display: flex;
    gap: 5px;
    animation: arrive 5.4s var(--ease-out) infinite backwards;
    animation-delay: calc(var(--row) * 1.8s);
  }

  .message .who {
    flex: none;
    inline-size: 8px;
    block-size: 8px;
    background: var(--color-fill-strong);
  }

  .message span {
    display: grid;
    flex: 1;
    gap: 3px;
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  @keyframes close {
    0%,
    12% {
      background: transparent;
      box-shadow: inset 0 0 0 1px var(--color-faint);
    }

    20%,
    100% {
      background: var(--color-success);
      box-shadow: inset 0 0 0 1px var(--color-success);
    }
  }

  @keyframes note {
    0%,
    18% {
      opacity: 0;
      transform: translateY(-3px) scale(0.96);
    }

    26%,
    84% {
      opacity: 1;
      transform: none;
    }

    92%,
    100% {
      opacity: 0;
      transform: none;
    }
  }

  @keyframes merge {
    0%,
    58% {
      background: transparent;
    }

    66%,
    92% {
      background: var(--color-success);
    }

    100% {
      background: transparent;
    }
  }

  @keyframes arrive {
    0% {
      opacity: 0;
      transform: translateY(4px);
    }

    10%,
    100% {
      opacity: 1;
      transform: none;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .live,
    .issue .state,
    .pr-state,
    .comment,
    .message {
      animation: none;
    }
  }
</style>
