<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import type { Detail } from "../../lib/board/types";
  import AgentOrb from "../cards/AgentOrb.svelte";
  import HostGlyph from "../cards/HostGlyph.svelte";
  import { siteName, type Ask } from "./asks";

  /**
   * An open ask as the canvas shows it from afar: what it needs, set large
   * enough to read at the zoom it is seen at. Pressing it brings the card up.
   */
  let {
    ask,
    detail,
    seed = 0,
    onopen,
  }: { ask: Ask; detail: Exclude<Detail, "full">; seed?: number; onopen?: () => void } = $props();

  const host = $derived(
    ask.kind === "confirm"
      ? ask.site
      : ask.kind === "sign_in"
        ? ask.host
        : ask.kind === "entry" || ask.kind === "connection"
          ? (ask.host ?? "")
          : "",
  );
  const title = $derived.by(() => {
    switch (ask.kind) {
      case "confirm":
        return ask.headline;
      case "entry":
        return m.work_ask_entry_title({ name: ask.name });
      case "sign_in":
        return m.work_ask_sign_in_title({ host: siteName(ask.host) });
      case "context":
        return {
          history: m.work_ask_history_title,
          notes: m.work_ask_notes_title,
          tabs: m.work_ask_tabs_title,
        }[ask.source]();
      case "connection":
        return ask.tool
          ? m.work_ask_connection_tool({ service: ask.service, tool: ask.tool })
          : m.work_ask_connection({ service: ask.service });
      case "question":
        return ask.prompt;
    }
  });
</script>

<button type="button" class="pill {detail}" onclick={() => onopen?.()} aria-label={title}>
  <span class="mark" aria-hidden="true"
    >{#if host}<HostGlyph
        {host}
        size={detail === "tile" ? 40 : 22}
        initial={false}
      />{:else}<AgentOrb {seed} size={detail === "tile" ? 40 : 22} />{/if}</span
  >
  {#if detail === "overview"}<span class="words"
      ><span class="needs">{m.work_ask_needs_you()}</span><span class="title">{title}</span></span
    >{/if}
</button>

<style>
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 14px;
    max-inline-size: 720px;
    padding: 14px 24px 14px 16px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-raised);
    box-shadow: var(--shadow-float);
    color: var(--color-text);
    font: inherit;
    text-align: start;
    cursor: default;
  }

  .pill:focus-visible {
    outline: 4px solid var(--color-ring);
    outline-offset: 4px;
  }

  .tile {
    padding: 16px;
  }

  .mark {
    display: grid;
    flex: none;
    place-items: center;
  }

  .words {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-inline-size: 0;
  }

  .needs {
    color: var(--color-muted);
    font-size: var(--text-overview-label);
    line-height: 1.2;
  }

  .title {
    overflow: hidden;
    font-size: var(--text-overview-title);
    font-weight: 600;
    line-height: 1.2;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
