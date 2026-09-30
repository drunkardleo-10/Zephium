<script lang="ts">
  import { getContext } from "svelte";
  import type { WorkRuntimeProjection } from "$shared/ipc/bindings";
  import { serviceKey, serviceMark } from "$domain/connections";
  import Orb from "$shared/ui/presence/Orb.svelte";
  import Shimmer from "$shared/ui/presence/Shimmer.svelte";
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

  let { part, objective, steps }: PartContentProps = $props();

  const work = getContext<((objective: string) => WorkRuntimeProjection | undefined) | undefined>(
    canvasWork,
  );
  const view = $derived(connectionView(work?.(objective), steps, part));
  const mark = $derived(serviceMark(view.service));
  const ROWS = 4;
  const shown = $derived(view.calls.slice(-ROWS));
  const hidden = $derived(view.calls.length - shown.length);
  /** Calls through several servers stand under each server's mark and name, in the order first used. */
  const groups = $derived.by(() => {
    const servers = [...new Set(shown.map((call) => call.server ?? ""))];
    return servers.map((server) => ({
      server,
      calls: shown.filter((call) => (call.server ?? "") === server),
    }));
  });
  const headed = $derived(groups.length > 1);
  const nameOf = (server: string) => server.charAt(0).toUpperCase() + server.slice(1);
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
</script>

<div class="connection" aria-label={part.title}>
  <ul class="rows">
    {#if hidden}<li class="earlier">
        {hidden === 1
          ? m.work_connection_earlier_one()
          : m.work_connection_earlier({ count: String(hidden) })}
      </li>{/if}
    {#each groups as group (group.server)}
      {#if headed && group.server}<li class="server">
          <span class="glyph"
            ><Icon icon={serviceMark(serviceKey(group.server))} size={13} strokeWidth={1.6} /></span
          ><span class="text">{nameOf(group.server)}</span>
        </li>{/if}
      {#each group.calls as call (call.key)}
        <li
          class="call"
          class:nested={headed}
          data-state={call.state}
          title={call.url ?? call.text}
        >
          <span class="glyph"
            >{#if call.state === "running"}<Orb
                size={13}
              />{:else if call.state === "declined" || call.state === "failed"}<Icon
                icon={Cancel01Icon}
                size={11}
                strokeWidth={2}
              />{:else}<Icon icon={glyph(call.text)} size={13} strokeWidth={1.6} />{/if}</span
          >
          <span class="text"
            >{#if call.state === "running"}<span class="what"><Shimmer text={call.text} /></span
              >{:else}<span class="what">{call.text}</span>{/if}{#if call.detail}<span
                class="detail"
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
    {/each}
    {#if !view.calls.length}<li class="empty">
        <span class="glyph"><Icon icon={mark} size={13} strokeWidth={1.6} /></span>
        <span class="text">{part.state === "running" ? m.work_connection_starting() : ""}</span>
      </li>{/if}
  </ul>
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
    align-items: flex-start;
    gap: 8px;
    min-inline-size: 0;
    padding-block: 4px;
    font-size: var(--text-label);
    line-height: 16px;
  }

  .glyph {
    display: grid;
    flex: none;
    inline-size: 16px;
    block-size: 16px;
    color: var(--color-muted);
    place-items: center;
  }

  .call[data-state="declined"] .glyph,
  .call[data-state="failed"] .glyph {
    color: var(--color-danger);
  }

  .text {
    flex: 1;
    min-inline-size: 0;
    overflow-wrap: anywhere;
  }

  .what {
    font-weight: 500;
  }

  .detail {
    color: var(--color-muted);
  }

  .detail::before {
    content: "·";
    margin-inline: 6px;
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

  .server {
    color: var(--color-muted);
    font-weight: 600;
  }

  .nested {
    padding-inline-start: 12px;
  }

  .earlier {
    padding-inline-start: 24px;
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  .empty .text {
    color: var(--color-muted);
  }
</style>
