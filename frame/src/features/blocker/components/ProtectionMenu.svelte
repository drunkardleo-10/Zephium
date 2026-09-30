<script lang="ts">
  import { ArrowRight01Icon, Cancel01Icon, Shield01Icon } from "@hugeicons/core-free-icons";
  import { blocker, blockerSites, hiding, shieldPresentation } from "$domain/blocker";
  import type { BlockerSiteAction } from "$shared/ipc/bindings";
  import { commands } from "$shared/ipc/bindings";
  import Icon from "$shared/ui/Icon";
  import Switch from "$shared/ui/Switch";

  /** `opened` counts menu openings, so the day's count is fresh each time. */
  let { labelled = false, opened = 0 }: { labelled?: boolean; opened?: number } = $props();
  let status = $derived(blocker.status());
  let shield = $derived(shieldPresentation(status));
  let site = $derived(status.site);
  let on = $derived(status.applied_enabled === true);
  let busy = $state(false);
  let managing = $state(false);
  let failure = $state("");
  let contextKey = $derived(
    site ? `${site.context.profile}:${site.context.tab}:${site.context.site}` : "",
  );
  $effect(() => {
    void contextKey;
    failure = "";
    managing = false;
  });

  let blockedToday = $state<number | null>(null);
  $effect(() => {
    void opened;
    const profile = site?.context.profile;
    if (!profile) return;
    let current = true;
    void commands
      .blockerStats(profile)
      .then((result) => {
        if (current) blockedToday = result.status === "ok" ? result.data.today : null;
      })
      .catch(() => {});
    return () => {
      current = false;
    };
  });

  let standing = $derived(
    status.protection === "disabled"
      ? "Protection is off"
      : status.protection === "pending"
        ? "Starting protection…"
        : status.protection === "degraded"
          ? "Protection needs attention"
          : site?.paused
            ? "Paused on this site"
            : blockedToday
              ? `${blockedToday.toLocaleString()} ads and trackers blocked today`
              : "Blocking ads and trackers",
  );
  let active = $derived(status.protection === "active" && !site?.paused);

  function ok(result: blocker.BlockerMutationResult) {
    return (
      result.state === "processed" &&
      (result.disposition.outcome === "applied" || result.disposition.outcome === "no_op")
    );
  }

  // The context's revision moves with every change, so each step reads it fresh.
  async function change(action: BlockerSiteAction): Promise<boolean> {
    const context = blocker.status().site?.context;
    if (!context) return false;
    const done = ok(await blockerSites.changeSite(context, action));
    if (!done) failure = "That didn't go through. Try again.";
    return done;
  }

  async function run(task: () => Promise<void>) {
    if (busy) return;
    busy = true;
    failure = "";
    try {
      await task();
    } finally {
      busy = false;
    }
  }

  // Requests the page already made stay made; reloading is what makes the
  // switch mean what it says, so it happens here rather than as advice.
  const setProtected = (protect: boolean) =>
    run(async () => {
      const tab = site?.context.tab;
      if ((await change({ kind: "pause", paused: !protect })) && tab) void commands.tabsReload(tab);
    });

  const enable = () =>
    run(async () => {
      if (!ok(await blocker.setEnabled(true))) failure = "Protection couldn't start. Try again.";
    });

  const showAgain = (ids: string[]) =>
    run(async () => {
      for (const id of ids) if (!(await change({ kind: "remove_hide", id }))) return;
    });

  async function hide() {
    if (site && !(await hiding.start(site.context)))
      failure = "Elements can't be hidden on this page.";
  }
</script>

{#if shield.visible && labelled}
  <div class="protection">
    <div class="head" data-keep-open>
      <span class="text">
        <span class="site" title={site?.context.site}
          >{site ? site.context.site : "Ad and tracker protection"}{site?.private_session
            ? " · private"
            : ""}</span
        >
        <span class="standing" class:active class:warning={shield.tone === "warning"}>
          <Icon icon={Shield01Icon} size={12} />
          <span>{standing}</span>
        </span>
      </span>
      {#if on && site?.ready}
        <Switch
          label="Protection on this site"
          labelHidden
          checked={!site.paused}
          disabled={busy || site.busy}
          onchange={(value) => void setProtected(value)}
        />
      {/if}
    </div>

    {#if status.desired_enabled !== true}
      <button
        type="button"
        role="menuitem"
        class="ui-menu-item row"
        data-keep-open
        disabled={busy || !status.can_enable || status.preference !== "authoritative"}
        onclick={() => void enable()}>Turn on protection</button
      >
    {/if}

    {#if site && on}
      {#if site.ready}
        <button
          type="button"
          role="menuitem"
          class="ui-menu-item row"
          disabled={busy || site.busy}
          onclick={() => void hide()}
        >
          <span>Hide elements</span>
        </button>
      {:else}
        <button
          type="button"
          role="menuitem"
          class="ui-menu-item row"
          data-keep-open
          disabled={busy || site.busy}
          onclick={() => void run(async () => void (await change({ kind: "retry" })))}
          >Retry site controls</button
        >
      {/if}
      {#if site.hides.length > 0}
        <button
          type="button"
          role="menuitem"
          class="ui-menu-item row"
          data-keep-open
          aria-expanded={managing}
          onclick={() => (managing = !managing)}
        >
          <span>Hidden on this site</span>
          <span class="count">{site.hides.length}</span>
          <span class="chevron" class:open={managing}
            ><Icon icon={ArrowRight01Icon} size={14} /></span
          >
        </button>
        {#if managing}
          <ul class="hides" data-keep-open>
            {#each site.hides as hide (hide.id)}
              <li>
                <span class="label" title={hide.label}>{hide.label}</span>
                <button
                  type="button"
                  class="restore"
                  title="Show again"
                  aria-label={`Show ${hide.label} again`}
                  disabled={busy || site.busy}
                  onclick={() => void showAgain([hide.id])}
                  ><Icon icon={Cancel01Icon} size={12} /></button
                >
              </li>
            {/each}
            {#if site.hides.length > 1}
              <li>
                <button
                  type="button"
                  class="all"
                  disabled={busy || site.busy}
                  onclick={() => void showAgain(site?.hides.map((hide) => hide.id) ?? [])}
                  >Show all</button
                >
              </li>
            {/if}
          </ul>
        {/if}
      {/if}
    {:else if !site && on}
      <p class="hint">Site controls work on web pages.</p>
    {/if}

    {#if failure}<p class="hint failure" role="status">{failure}</p>{/if}
  </div>
{:else if shield.visible}
  <span
    class="compact"
    class:warning={shield.tone === "warning"}
    title={shield.label}
    role="img"
    aria-label={shield.label}><Icon icon={Shield01Icon} size={14} /></span
  >
{/if}

<style>
  .protection {
    display: flex;
    flex-direction: column;
    min-inline-size: 0;
    padding-block-end: 4px;
    border-block-end: 0.5px solid var(--color-border);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px var(--menu-item-inset) 10px;
  }

  .text {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 2px;
    min-inline-size: 0;
  }

  .site {
    overflow: hidden;
    color: var(--color-text);
    font-size: var(--text-body);
    font-weight: 500;
    line-height: 18px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .standing {
    display: flex;
    align-items: center;
    gap: 5px;
    min-inline-size: 0;
    color: var(--color-muted);
    font-size: var(--text-caption);
    line-height: 16px;
  }

  .standing > span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .standing :global(svg) {
    flex: none;
  }

  .standing.active :global(svg) {
    color: var(--color-success);
  }

  .standing.warning {
    color: var(--color-warning);
  }

  .row {
    inline-size: 100%;
    border: 0;
    background: transparent;
    font: inherit;
    text-align: start;
  }

  .row:disabled {
    opacity: 0.45;
  }

  .row:focus-visible,
  .row:hover:not(:disabled) {
    background: var(--row-active);
  }

  .count {
    margin-inline-start: auto;
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-variant-numeric: tabular-nums;
  }

  .chevron {
    display: grid;
    color: var(--color-faint);
    transition: rotate var(--motion-fast) var(--ease-out);
  }

  .chevron.open {
    rotate: 90deg;
  }

  .hides {
    max-block-size: 184px;
    margin: 0 0 2px;
    padding: 0 0 0 calc(var(--menu-item-inset) + 8px);
    overflow: auto;
    list-style: none;
  }

  .hides li {
    display: flex;
    align-items: center;
    gap: 6px;
    min-block-size: 28px;
    padding-inline-end: 4px;
  }

  .label {
    flex: 1;
    min-inline-size: 0;
    overflow: hidden;
    color: var(--color-text);
    font-size: var(--text-caption);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .restore {
    display: grid;
    flex: none;
    place-items: center;
    inline-size: 22px;
    block-size: 22px;
    border: 0;
    border-radius: var(--radius-inset);
    background: transparent;
    color: var(--color-faint);
    cursor: default;
  }

  .restore:hover:not(:disabled) {
    background: var(--row-hover);
    color: var(--color-text);
  }

  .all {
    padding: 2px 0;
    border: 0;
    background: transparent;
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-caption);
    cursor: default;
  }

  .all:hover:not(:disabled) {
    color: var(--color-text);
  }

  .hint {
    max-inline-size: 240px;
    margin: 2px var(--menu-item-inset) 4px;
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .failure {
    color: var(--color-warning);
  }

  .compact {
    display: grid;
    place-items: center;
    inline-size: 20px;
    block-size: 20px;
    color: var(--color-faint);
  }

  .compact.warning {
    color: var(--color-warning);
  }
</style>
