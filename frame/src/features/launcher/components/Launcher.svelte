<script lang="ts">
  import { onMount, tick } from "svelte";
  import {
    Search01Icon,
    Globe02Icon,
    CommandLineIcon,
    Clock01Icon,
    ArrowRight01Icon,
    Cancel01Icon,
  } from "@hugeicons/core-free-icons";
  import type { PanelState, SearchResult, ToolKind } from "$shared/ipc/bindings";
  import { commands } from "$shared/ipc/bindings";
  import { events } from "$shared/ipc/native-events";
  import { resultIdentity } from "../lib/search-model";
  import { createSearchController, type SearchSnapshot } from "../lib/search-controller";
  import { IS_MAC } from "$shared/platform";
  import Icon from "$shared/ui/Icon";
  import IconButton from "$shared/ui/IconButton";
  import FavIcon from "$shared/ui/FavIcon";
  import * as m from "$shared/i18n/messages";
  import type { IconSvgElement } from "@hugeicons/svelte";
  let {
    context,
    onTool,
    onDrag,
    destinations,
  }: {
    context: PanelState;
    onTool: (tool: ToolKind) => void;
    onDrag: () => void;
    destinations: { kind: ToolKind; label: string; icon: IconSvgElement }[];
  } = $props();
  let input: HTMLInputElement;
  let list: HTMLElement;
  let query = $state("");
  let results = $state<SearchResult[]>([]);
  let selected = $state<string | null>(null);
  let pending = $state(false);
  let failed = $state(false);
  let running = $state(false);
  let composing = false;
  let disposed = false;
  let controller: ReturnType<typeof createSearchController> | undefined;
  let searchError: SearchSnapshot["error"] = $state("none");
  $effect(() => {
    if (context.error) running = false;
  });
  let matchingTools = $derived(
    query.trim()
      ? destinations
          .filter((destination) =>
            destination.label.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()),
          )
          .map((destination) => destination.kind)
      : [],
  );
  let rows = $derived([
    ...matchingTools.map((kind) => ({ id: `tool:${kind}`, tool: kind, result: null })),
    ...results.map((result) => ({ id: resultIdentity(result), tool: null, result })),
  ]);
  let selectedIndex = $derived(rows.findIndex((row) => row.id === selected));
  function changed(value: string) {
    query = value;
    running = false;
    failed = false;
    controller?.change(value);
  }
  async function runSelected(result?: SearchResult, tool?: ToolKind) {
    if (running) return;
    if (tool) {
      onTool(tool);
      return;
    }
    const expected = controller?.context();
    if (!result || !expected) return;
    running = true;
    failed = false;
    try {
      const admission = await commands.launcherRun(result.action, expected);
      if (!disposed && !admission.accepted) {
        running = false;
        failed = true;
      }
    } catch {
      if (!disposed) {
        running = false;
        failed = true;
      }
    }
  }
  async function selectBy(offset: number) {
    if (!rows.length) return;
    const index =
      ((selectedIndex < 0 ? (offset > 0 ? -1 : 0) : selectedIndex) + offset + rows.length) %
      rows.length;
    selected = rows[index]!.id;
    await tick();
    list
      .querySelector<HTMLElement>(`[data-result-index="${index}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }
  function keydown(event: KeyboardEvent) {
    if (composing || event.isComposing) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      void selectBy(event.key === "ArrowDown" ? 1 : -1);
    } else if (event.key === "Enter") {
      event.preventDefault();
      const row = rows[Math.max(0, selectedIndex)];
      void runSelected(row?.result ?? undefined, row?.tool ?? undefined);
    }
  }
  function drag(event: PointerEvent) {
    if (event.button === 0 && !(event.target as HTMLElement).closest("button,input,a")) onDrag();
  }
  function shortcut(value: string) {
    return value
      .replace("CmdOrCtrl+", IS_MAC ? "⌘" : "Ctrl+")
      .replace("Shift+", IS_MAC ? "⇧" : "Shift+");
  }
  onMount(() => {
    input.focus();
    controller = createSearchController({
      owner:
        context.window_id && context.profile_id && context.space_id
          ? {
              window_id: context.window_id,
              profile_id: context.profile_id,
              space_id: context.space_id,
              session_id: context.session_id,
            }
          : null,
      send: commands.launcherSearch,
      update: (snapshot) => {
        results = snapshot.results;
        pending = snapshot.pending;
        searchError = snapshot.error;
        const identities = [
          ...matchingTools.map((kind) => `tool:${kind}`),
          ...results.map(resultIdentity),
        ];
        if (!selected || !identities.includes(selected)) selected = identities[0] ?? null;
      },
    });
    const listener = events.searchChanged.listen((event) => controller?.receive(event.payload));
    void listener
      .then(() => {
        if (!disposed) controller?.start();
      })
      .catch(() => {
        if (!disposed) failed = true;
      });
    return () => {
      disposed = true;
      controller?.dispose();
      void listener.then((stop) => stop()).catch(() => {});
    };
  });
</script>

<div class="launcher-view">
  <header
    role="group"
    aria-label={m.panel_search_mode()}
    class="panel-dragbar"
    onpointerdown={drag}
  >
    <span class="panel-context">{context.profile_name ?? m.panel_context_unavailable()}</span><span
      class="panel-drag-grip"
      aria-hidden="true"
    ></span><span class="panel-context">{m.panel_search_mode()}</span>
  </header>
  <div class="panel-search-field">
    <Icon icon={Search01Icon} size={20} /><input
      bind:this={input}
      value={query}
      oninput={(event) => changed(event.currentTarget.value)}
      oncompositionstart={() => {
        composing = true;
        controller?.compositionStart();
      }}
      oncompositionend={(event) => {
        composing = false;
        query = event.currentTarget.value;
        controller?.compositionEnd(query);
      }}
      onkeydown={keydown}
      maxlength={2048}
      autocomplete="off"
      autocapitalize="off"
      spellcheck={false}
      placeholder={m.panel_search_placeholder()}
      aria-label={m.panel_search_placeholder()}
      role="combobox"
      aria-controls="panel-results"
      aria-expanded={rows.length > 0}
      aria-autocomplete="list"
      aria-activedescendant={selectedIndex >= 0 ? `panel-result-${selectedIndex}` : undefined}
    />{#if query}<IconButton
        icon={Cancel01Icon}
        label={m.panel_clear_search()}
        onclick={() => {
          changed("");
          input.focus();
        }}
      />{/if}
  </div>
  <div class="panel-scroll" bind:this={list}>
    {#if !query.trim()}<section class="panel-tools" aria-label={m.panel_tools()}>
        <h2>{m.panel_tools()}</h2>
        <div class="panel-tool-grid">
          {#each destinations as destination (destination.kind)}
            {@const kind = destination.kind}<button type="button" onclick={() => onTool(kind)}
              ><span><Icon icon={destination.icon} size={18} /></span>{destination.label}<Icon
                icon={ArrowRight01Icon}
                size={13}
              /></button
            >{/each}
        </div>
      </section>
      <h2 class="panel-group-title">{m.panel_recent()}</h2>{/if}
    {#if failed || context.error || searchError !== "none"}<div
        class="panel-inline-error"
        role="alert"
      >
        {searchError === "too_long" ? m.panel_query_too_long() : m.panel_action_failed()}<button
          type="button"
          onclick={() => controller?.change(query)}>{m.panel_retry()}</button
        >
      </div>{/if}
    <div id="panel-results" role="listbox" aria-label={m.panel_results()} aria-busy={pending}>
      {#each rows as row, index (row.id)}{@const result = row.result}<button
          type="button"
          id={`panel-result-${index}`}
          data-result-index={index}
          class="panel-result"
          tabindex="-1"
          class:panel-result-selected={selected === row.id}
          role="option"
          aria-selected={selected === row.id}
          onpointermove={() => (selected = row.id)}
          onclick={() => void runSelected(result ?? undefined, row.tool ?? undefined)}
          ><span class="panel-result-icon"
            >{#if row.tool}<Icon
                icon={destinations.find((destination) => destination.kind === row.tool)!.icon}
                size={18}
              />{:else if result?.favicon}<FavIcon favicon={result.favicon} lit />{:else}<Icon
                icon={result?.kind === "command"
                  ? CommandLineIcon
                  : result?.kind === "history"
                    ? Clock01Icon
                    : Globe02Icon}
                size={18}
              />{/if}</span
          ><span class="panel-result-copy"
            ><strong
              >{row.tool
                ? destinations.find((destination) => destination.kind === row.tool)!.label
                : result?.title}</strong
            ><small
              >{row.tool
                ? m.panel_tool_view()
                : result?.kind === "command"
                  ? m.panel_command()
                  : result?.detail}</small
            ></span
          >{#if result?.kind === "command" && result.detail}<kbd>{shortcut(result.detail)}</kbd
            >{/if}</button
        >{/each}
    </div>
    {#if !rows.length && !pending && query.trim() && !failed}<p class="panel-empty">
        {m.panel_no_results()}
      </p>{/if}
  </div>
  <footer class="panel-footer">
    <span
      >{pending ? m.panel_searching() : running ? m.panel_opening() : m.panel_search_hint()}</span
    ><span><kbd>↑↓</kbd> {m.panel_navigate()}</span><span><kbd>↵</kbd> {m.panel_open()}</span><span
      ><kbd>esc</kbd> {m.panel_close()}</span
    >
  </footer>
</div>
