<script lang="ts">
  import {
    BrowserIcon,
    Clock01Icon,
    CommandLineIcon,
    Link01Icon,
    Search01Icon,
  } from "@hugeicons/core-free-icons";
  import { onMount } from "svelte";
  import type { SearchResult } from "../../shared/ipc/bindings";
  import { commands } from "../../shared/ipc/bindings";
  import { events } from "../../shared/ipc/native-events";
  import FavIcon from "../../shared/ui/FavIcon.svelte";
  import Icon from "../../shared/ui/Icon.svelte";

  const KIND_LABEL: Record<string, string> = {
    tab: "Open tab",
    url: "Open",
    search: "Search",
    command: "Command",
    history: "History",
  };

  const KIND_ICON = {
    tab: BrowserIcon,
    url: Link01Icon,
    search: Search01Icon,
    command: CommandLineIcon,
    history: Clock01Icon,
  } as const;

  let input: HTMLInputElement;
  let query = $state("");
  let results = $state<SearchResult[]>([]);
  let selected = $state(0);

  function search(value: string): void {
    query = value;
    void commands.launcherSearch(value);
  }

  function accelerator(detail: string): string {
    if (!detail) return "";
    return detail
      .replace("CmdOrCtrl+", "⌘")
      .replace("Ctrl+", "⌃")
      .replace("Shift+", "⇧")
      .replace("Space", "␣");
  }

  function run(result: SearchResult | undefined): void {
    if (result) void commands.launcherRun(result.action);
  }

  function handleKeydown(event: KeyboardEvent): void {
    const length = results.length;

    switch (event.key) {
      case "Escape":
        event.preventDefault();
        void commands.panelHide();
        break;
      case "ArrowDown":
        event.preventDefault();
        if (length > 0) selected = (selected + 1) % length;
        break;
      case "ArrowUp":
        event.preventDefault();
        if (length > 0) selected = (selected + length - 1) % length;
        break;
      case "Enter":
        event.preventDefault();
        run(results[selected]);
        break;
    }
  }

  onMount(() => {
    input.focus();

    const unlistenResults = events.searchChanged.listen((event) => {
      if (event.payload.query !== query) return;
      results = event.payload.results;
      selected = 0;
    });

    const handleFocus = (): void => {
      input.focus();
      input.select();
      search(input.value);
    };

    window.addEventListener("focus", handleFocus);
    search("");

    return () => {
      void unlistenResults.then((unlisten) => unlisten());
      window.removeEventListener("focus", handleFocus);
    };
  });
</script>

<div
  class="launcher-shell flex h-screen w-screen flex-col overflow-hidden rounded-xl text-text shadow-overlay"
>
  <div class="relative shrink-0 border-b border-border">
    <Icon
      icon={Search01Icon}
      size={17}
      class="pointer-events-none absolute top-1/2 left-5 -translate-y-1/2 text-faint"
    />
    <input
      bind:this={input}
      value={query}
      oninput={(event) => search(event.currentTarget.value)}
      onkeydown={handleKeydown}
      role="combobox"
      aria-label="Search tabs, history, commands, or the web"
      aria-controls="launcher-results"
      aria-expanded={results.length > 0}
      aria-autocomplete="list"
      aria-activedescendant={results.length > 0 ? `launcher-result-${selected}` : undefined}
      placeholder="Search or enter address"
      autocomplete="off"
      autocapitalize="off"
      spellcheck={false}
      class="h-[58px] w-full bg-transparent pr-5 pl-[46px] text-[15px] tracking-[-0.01em] text-text outline-none placeholder:text-faint"
    />
  </div>

  <div
    id="launcher-results"
    role="listbox"
    aria-label="Search results"
    class="flex-1 overflow-y-auto p-2"
  >
    {#each results as result, index (`${result.kind}:${result.title}:${result.detail}:${index}`)}
      {@const kindIcon = KIND_ICON[result.kind as keyof typeof KIND_ICON]}
      <button
        id={`launcher-result-${index}`}
        type="button"
        role="option"
        aria-selected={index === selected}
        onpointermove={() => (selected = index)}
        onclick={() => run(result)}
        class:bg-fill-hover={index === selected}
        class="group flex h-11 w-full items-center gap-3 rounded-md px-2.5 text-start outline-none"
      >
        <span
          class="flex h-7 w-7 shrink-0 items-center justify-center rounded-sm bg-fill text-faint"
        >
          {#if result.favicon}
            <FavIcon favicon={result.favicon} lit={index === selected} />
          {:else if kindIcon}
            <Icon icon={kindIcon} size={15} />
          {/if}
        </span>
        <span class="min-w-0 flex-1">
          <span class="block truncate text-[13.5px] text-text">{result.title}</span>
          <span class="mt-0.5 block truncate text-[11.5px] text-faint">
            {KIND_LABEL[result.kind] ?? result.kind}
            {#if result.kind !== "command" && result.detail}
              <span aria-hidden="true"> · </span>{result.detail}
            {/if}
          </span>
        </span>
        <span class="max-w-44 shrink-0 truncate text-[11px] text-faint">
          {result.kind === "command" ? accelerator(result.detail) : ""}
        </span>
      </button>
    {/each}

    {#if results.length === 0 && query.trim() !== ""}
      <p class="px-3 py-2 text-[12px] text-faint" role="status">No results</p>
    {/if}
  </div>

  <footer
    class="flex h-9 shrink-0 items-center justify-end gap-3 border-t border-border px-3 text-[10.5px] text-faint"
    aria-hidden="true"
  >
    <span class="flex items-center gap-1.5"><kbd>↑↓</kbd> Navigate</span>
    <span class="flex items-center gap-1.5"><kbd>↵</kbd> Open</span>
    <span class="flex items-center gap-1.5"><kbd>esc</kbd> Close</span>
  </footer>
</div>
