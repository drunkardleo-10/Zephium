<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { Globe02Icon } from "@hugeicons/core-free-icons";
  import { greetingFor } from "../lib/greeting";
  import { onMount } from "svelte";
  import { commands } from "$shared/ipc/bindings";
  import { preferences } from "$domain/preferences";
  import { Settings01Icon } from "@hugeicons/core-free-icons";
  import IconButton from "$shared/ui/IconButton";
  import { tabs } from "$domain/tabs";
  import type { TabView } from "$shared/ipc/bindings";
  import FavIcon from "$shared/ui/FavIcon";
  import type { Snippet } from "svelte";
  import SearchField from "$shared/ui/SearchField";

  let {
    search,
    clockFormat = "System",
    showGreeting = true,
    personalize = false,
    showClock = true,
  }: {
    search?: Snippet;
    clockFormat?: string;
    showGreeting?: boolean;
    personalize?: boolean;
    showClock?: boolean;
  } = $props();
  let input = $state<HTMLInputElement>();
  let now = $state(new Date());
  let firstName = $derived((tabs.profile()?.name ?? "").trim().split(/\s+/u)[0] ?? "");
  let greeting = $derived(greetingFor(now));
  let clock = $derived(
    new Intl.DateTimeFormat(undefined, {
      hour: "numeric",
      minute: "2-digit",
      ...(clockFormat === "System" ? {} : { hour12: clockFormat === "12-hour" }),
    }).format(now),
  );
  let date = $derived(
    new Intl.DateTimeFormat(undefined, { weekday: "long", month: "long", day: "numeric" }).format(
      now,
    ),
  );

  // Top-level favorites only; folders and nesting stay in the sidebar.
  let essentials = $derived.by(() => {
    const byId = new Map(tabs.tabs().map((tab) => [tab.id, tab] as const));
    const found: TabView[] = [];
    for (const node of tabs.sidebarNodes()) {
      if (node.section !== "favorites" || node.parent_id !== null || node.kind.type !== "tab")
        continue;
      const tab = byId.get(node.kind.tab_id);
      if (tab !== undefined) found.push(tab);
    }
    return found;
  });

  function go(value: string): void {
    const id = tabs.activeId();
    const destination = value.trim();
    if (id != null && destination) tabs.navigate(id, destination);
  }

  onMount(() => {
    input?.focus();
    let timer: ReturnType<typeof setTimeout>;
    function update() {
      now = new Date();
      if (document.visibilityState === "visible")
        timer = setTimeout(update, 60000 - (Date.now() % 60000));
    }
    function visibility() {
      clearTimeout(timer);
      if (document.visibilityState === "visible") update();
    }
    update();
    document.addEventListener("visibilitychange", visibility);
    return () => {
      clearTimeout(timer);
      document.removeEventListener("visibilitychange", visibility);
    };
  });
</script>

<!--
  Renders in the chrome, so it costs no WebView and shows no paint flash. The
  wordmark is drawn through a mask in the text color; the file's own fill is
  never shown, so it follows the theme like every other glyph.
-->
<section class="newtab-page" aria-label={m.new_tab()}>
  {#if preferences.value("ui.newtab-logo") === "true"}<span
      role="img"
      aria-label="Zephium"
      class="zephium-wordmark"
    ></span>{/if}

  {#if showGreeting}<p class="newtab-greeting">
      {personalize && firstName ? m.ntp_greeting_named({ greeting, name: firstName }) : greeting}
    </p>{/if}
  {#if showClock}<div class="newtab-clock">
      <time datetime={now.toISOString()}>{clock}</time><span>{date}</span>
    </div>{/if}
  <div class="newtab-search">
    {#if search}{@render search()}{:else}<SearchField
        size="page"
        label={m.search_web()}
        placeholder={m.search_web()}
        bind:ref={input}
        onsubmit={go}
      />{/if}
  </div>

  {#if preferences.value("ui.newtab-shortcuts") === "true" && essentials.length > 0}
    <ul class="flex flex-wrap justify-center gap-2" role="list" aria-label={m.ntp_essentials()}>
      {#each essentials as tab (tab.id)}
        <li>
          <button
            type="button"
            title={tab.title || m.untitled_tab()}
            aria-label={tab.title || m.untitled_tab()}
            class="press flex h-11 w-11 cursor-default items-center justify-center rounded-lg bg-fill outline-none hover:bg-fill-hover"
            onclick={() => tabs.activate(tab.id)}
          >
            <FavIcon favicon={tab.favicon} size={20} lit fallback={Globe02Icon} />
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  <div class="newtab-customize">
    <IconButton
      icon={Settings01Icon}
      label={m.settings_customize_newtab()}
      onclick={() => void commands.runCommand("settings.newtab")}
    />
  </div>
</section>
