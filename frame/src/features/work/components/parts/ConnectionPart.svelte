<script lang="ts">
  import { getContext } from "svelte";
  import type { WorkRuntimeProjection } from "$shared/ipc/bindings";
  import { serviceMark } from "$domain/connections";
  import Icon from "$shared/ui/Icon";
  import Cancel01Icon from "@hugeicons/core-free-icons/Cancel01Icon";
  import CheckListIcon from "@hugeicons/core-free-icons/CheckListIcon";
  import CircleDotIcon from "@hugeicons/core-free-icons/CircleDotIcon";
  import Comment01Icon from "@hugeicons/core-free-icons/Comment01Icon";
  import GitMergeIcon from "@hugeicons/core-free-icons/GitMergeIcon";
  import GitPullRequestIcon from "@hugeicons/core-free-icons/GitPullRequestIcon";
  import type { IconSvgElement } from "@hugeicons/svelte";
  import * as m from "$shared/i18n/messages";
  import { canvasWork } from "../../lib/canvas-context";
  import { connectionView } from "../../lib/parts/connection";
  import type { PartContentProps } from "../run/slots";

  let { part, detail, objective, steps }: PartContentProps = $props();

  const work = getContext<((objective: string) => WorkRuntimeProjection | undefined) | undefined>(
    canvasWork,
  );
  const view = $derived(connectionView(work?.(objective), steps, part));
  const mark = $derived(serviceMark(view.service));
  const ROWS = 4;
  const shown = $derived(view.calls.slice(-ROWS));
  const hidden = $derived(view.calls.length - shown.length);
  /** What a row is about, from its words: an issue, a pull request, checks, a comment. */
  function glyph(text: string): IconSvgElement {
    if (/^(Commented|Comment)\b/u.test(text)) return Comment01Icon;
    if (/^(Merged|Merge)\b/u.test(text)) return GitMergeIcon;
    if (/\bchecks\b/u.test(text)) return CheckListIcon;
    if (/pull request/u.test(text)) return GitPullRequestIcon;
    if (/\bissues?\b/u.test(text)) return CircleDotIcon;
    return mark;
  }
  /** Titles keep their `code` spans as code, not as backticks. */
  function spans(text: string): { code: boolean; text: string }[] {
    return text
      .split(/(`[^`]+`)/u)
      .filter(Boolean)
      .map((piece) =>
        piece.startsWith("`") && piece.endsWith("`") && piece.length > 2
          ? { code: true, text: piece.slice(1, -1) }
          : { code: false, text: piece },
      );
  }
  /** Surveyed, the call that says what the part is about: one waiting on you, else the first. */
  const lead = $derived(view.calls.find((call) => call.state === "waiting") ?? view.calls[0]);
</script>

<div class="connection {detail}" aria-label={part.title}>
  {#if detail === "tile"}
    <span class="tile-mark"><Icon icon={mark} size={44} strokeWidth={1.3} /></span>
  {:else if detail === "overview"}
    {#if lead}<p class="big" data-state={lead.state}>
        <Icon icon={mark} size={24} strokeWidth={1.5} /><span>{lead.text}</span>
      </p>
      {#if lead.state === "waiting"}<p class="more-big waiting">
          {m.work_computer_waiting()}
        </p>{:else if lead.detail}<p class="more-big">
          {lead.detail.replaceAll("`", "")}
        </p>{/if}{/if}
  {:else}
    <ul class="rows">
      {#if hidden}<li class="earlier">
          {hidden === 1
            ? m.work_connection_earlier_one()
            : m.work_connection_earlier({ count: String(hidden) })}
        </li>{/if}
      {#each shown as call (call.key)}
        <li class="call" data-state={call.state} title={call.url ?? call.text}>
          <span class="glyph"
            >{#if call.state === "declined" || call.state === "failed"}<Icon
                icon={Cancel01Icon}
                size={11}
                strokeWidth={2}
              />{:else}<Icon icon={glyph(call.text)} size={13} strokeWidth={1.6} />{/if}</span
          >
          <span class="text"
            ><span class="what">{call.text}</span>{#if call.detail}<span class="detail"
                >{#each spans(call.detail) as piece, index (index)}{#if piece.code}<code
                      >{piece.text}</code
                    >{:else}{piece.text}{/if}{/each}</span
              >{/if}</span
          >
          {#if call.state === "waiting"}<span class="pill">{m.work_computer_waiting()}</span
            >{:else if call.state === "declined"}<span class="note"
              >{m.work_computer_declined()}</span
            >{/if}
        </li>
      {/each}
      {#if !view.calls.length}<li class="empty">
          <span class="glyph"><Icon icon={mark} size={13} strokeWidth={1.6} /></span>
          <span class="text">{part.state === "running" ? m.work_connection_starting() : ""}</span>
        </li>{/if}
    </ul>
  {/if}
</div>

<style>
  .connection {
    box-sizing: border-box;
    inline-size: 100%;
    color: var(--color-text);
  }

  .rows {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    align-items: center;
    gap: 8px;
    min-inline-size: 0;
    block-size: 24px;
    font-size: var(--text-label);
    line-height: 16px;
  }

  .glyph {
    display: grid;
    flex: none;
    inline-size: 16px;
    color: var(--color-muted);
    place-items: center;
  }

  .call[data-state="declined"] .glyph,
  .call[data-state="failed"] .glyph {
    color: var(--color-danger);
  }

  .text {
    display: flex;
    flex: 1;
    gap: 6px;
    overflow: hidden;
    min-inline-size: 0;
    white-space: nowrap;
  }

  .what {
    flex: none;
    font-weight: 500;
  }

  .detail {
    overflow: hidden;
    color: var(--color-muted);
    text-overflow: ellipsis;
  }

  .detail::before {
    content: "·";
    margin-inline-end: 6px;
    color: var(--color-faint);
  }

  .detail code {
    font-family: var(--font-mono);
    font-size: var(--text-caption);
  }

  .call[data-state="declined"] .what,
  .call[data-state="failed"] .what {
    color: var(--color-muted);
  }

  .pill {
    flex: none;
    padding: 1px 7px;
    border-radius: var(--radius-capsule);
    background: var(--color-lit-soft);
    font-size: var(--text-caption);
    font-weight: 600;
  }

  .note {
    flex: none;
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .earlier {
    block-size: 20px;
    padding-inline-start: 24px;
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  .empty .text {
    color: var(--color-muted);
  }

  .big {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0;
    font-size: var(--text-overview-title);
    font-weight: 600;
    line-height: 1.15;
    letter-spacing: -0.01em;
  }

  .big span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .more-big {
    display: -webkit-box;
    margin: 6px 0 0 34px;
    overflow: hidden;
    color: var(--color-muted);
    font-size: var(--text-overview-label);
    line-height: 1.25;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }

  .more-big.waiting {
    color: var(--color-text);
    font-weight: 600;
  }

  .tile-mark {
    display: grid;
    color: var(--color-text);
  }
</style>
