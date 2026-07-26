<script lang="ts">
  import { Search01Icon } from "@hugeicons/core-free-icons";
  import { onMount } from "svelte";
  import * as tabs from "../../state/tabs.svelte";
  import Icon from "../../ui/Icon.svelte";
  import { greetingFor } from "./greeting";

  let input: HTMLInputElement;
  let now = $state(new Date());

  let time = $derived(
    new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).format(now),
  );
  let date = $derived(
    new Intl.DateTimeFormat(undefined, {
      weekday: "long",
      month: "long",
      day: "numeric",
    }).format(now),
  );

  // Chosen once per page, not per render, so the line does not flicker while
  // the clock advances or the user types.
  const greeting = greetingFor(new Date());

  function go(value: string): void {
    const id = tabs.activeId();
    const destination = value.trim();
    if (id != null && destination) tabs.navigate(id, destination);
  }

  onMount(() => {
    input.focus();

    // Align the first tick to the next minute so the clock never sits a whole
    // minute behind, then settle into a plain minute interval.
    let interval: ReturnType<typeof setInterval> | undefined;
    const align = setTimeout(
      () => {
        now = new Date();
        interval = setInterval(() => (now = new Date()), 60_000);
      },
      60_000 - (Date.now() % 60_000),
    );

    return () => {
      clearTimeout(align);
      if (interval !== undefined) clearInterval(interval);
    };
  });
</script>

<section
  class="relative flex h-full flex-col items-center justify-center gap-10 px-6 pb-16"
  aria-label="New tab"
>
  <header class="text-center">
    <p
      class="text-[52px] leading-none font-light tracking-[-0.03em] text-text tabular-nums"
      aria-label={`${time}, ${date}`}
    >
      {time}
    </p>
    <p class="mt-3 text-[13px] text-muted">{date}</p>
    <p class="mt-1.5 text-[13px] text-faint">{greeting}</p>
  </header>

  <form
    class="w-full max-w-[560px]"
    role="search"
    onsubmit={(event) => {
      event.preventDefault();
      go(input.value);
    }}
  >
    <label class="sr-only" for="new-tab-search">Search the web or enter an address</label>
    <div
      class="flex h-12 items-center gap-3 rounded-xl bg-fill px-4 shadow-field transition-[background-color,box-shadow] duration-[var(--motion-base)] ease-[var(--ease-out-quiet)] focus-within:bg-fill-hover focus-within:shadow-focus hover:bg-fill-hover"
    >
      <Icon icon={Search01Icon} size={17} class="shrink-0 text-faint" />
      <input
        id="new-tab-search"
        bind:this={input}
        class="min-w-0 flex-1 bg-transparent text-[14px] text-text outline-none placeholder:text-faint"
        placeholder="Search the web or enter an address"
        autocomplete="off"
        autocapitalize="off"
        spellcheck={false}
      />
    </div>
  </form>
</section>
