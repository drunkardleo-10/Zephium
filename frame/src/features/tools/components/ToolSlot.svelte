<script lang="ts">
  import { untrack, type Component } from "svelte";
  import type { ToolKind } from "$shared/ipc/bindings";
  import { tools } from "../components/tool-views";
  import {
    toolSession,
    editTool,
    type ToolViewState,
    type ToolHostProps,
  } from "$session/tool-drafts.svelte";
  import * as m from "$shared/i18n/messages";
  let {
    tool,
    profile,
    host,
    profileName,
    onclose,
    onback,
    ondrag,
  }: {
    tool: ToolKind;
    profile: string;
    host: "sidebar" | "floating";
    profileName?: string;
    onclose: () => void;
    onback?: () => void;
    ondrag?: () => void;
  } = $props();
  let view = $state<Component<ToolHostProps> | null>(null);
  let session = $state<ToolViewState | null>(null);
  let failed = $state(false);
  let attempt = $state(0);
  $effect(() => {
    const kind = tool;
    const owner = profile;
    const surface = host;
    void attempt;
    let live = true;
    view = null;
    failed = false;
    session = untrack(() => toolSession(surface, owner, kind));
    void tools[kind]
      .load()
      .then((module) => {
        if (live) view = module.default;
      })
      .catch(() => {
        if (live) failed = true;
      });
    return () => {
      live = false;
    };
  });
</script>

{#if view && session}{@const View = view}<View
    {tool}
    state={session}
    {host}
    {profileName}
    {onclose}
    {onback}
    {ondrag}
    edit={(patch: Partial<ToolViewState>) => {
      if (session) editTool(session, patch);
    }}
  />
{:else}<div class="tool-load-state" role="status">
    {#if failed}<p>{m.panel_load_failed()}</p>
      <button type="button" onclick={() => attempt++}>{m.panel_retry()}</button><button
        type="button"
        onclick={onback ?? onclose}>{m.panel_back()}</button
      >{:else}<span>{m.panel_loading()}</span>{/if}
  </div>{/if}

<style>
  .tool-load-state {
    display: grid;
    place-content: center;
    gap: 12px;
    width: 100%;
    height: 100%;
    text-align: center;
    color: var(--color-label-secondary);
    font-size: 13px;
  }

  .tool-load-state button {
    border: 0;
    border-radius: 7px;
    background: var(--color-fill);
    color: var(--color-text);
    padding: 7px 12px;
    cursor: default;
  }
</style>
