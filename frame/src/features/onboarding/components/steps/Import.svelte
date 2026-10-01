<script lang="ts">
  import { Tick02Icon } from "@hugeicons/core-free-icons";
  import * as m from "$shared/i18n/messages";
  import { browserImport, type ImportKind } from "$domain/browser-import";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import { BROWSER_MARKS, BROWSERS } from "../../lib/catalog";
  import Mark from "../Mark.svelte";

  // What native found, or, until it can look, the browsers people come from.
  let sources = $derived(
    browserImport.found() ??
      BROWSERS.map(([id, name]) => ({
        id,
        browser: id,
        name,
        profiles: [],
        kinds: ["bookmarks", "history"] as ImportKind[],
        needsPermission: false,
        running: false,
      })),
  );
  let available = $derived(browserImport.available());
  let job = $derived(browserImport.current());
  let chosen = $state<string | null>(null);
  let kinds = $state<ImportKind[]>(["bookmarks", "history"]);
  let source = $derived(sources.find((candidate) => candidate.id === chosen) ?? null);
  const number = new Intl.NumberFormat();
  let total = $derived(job ? job.kinds.reduce((sum, entry) => sum + entry.done, 0) : 0);
  let share = $derived.by(() => {
    if (!job) return 0;
    const known = job.kinds.filter((entry) => entry.total);
    if (known.length === 0) return 0;
    return (
      known.reduce((sum, entry) => sum + entry.done, 0) /
      known.reduce((sum, entry) => sum + (entry.total ?? 0), 0)
    );
  });

  function toggle(kind: ImportKind) {
    kinds = kinds.includes(kind)
      ? kinds.filter((candidate) => candidate !== kind)
      : [...kinds, kind];
  }
</script>

<div class="import">
  <div class="browsers" role="radiogroup" aria-label={m.onb_import_from()}>
    {#each sources as candidate, index (candidate.id)}
      <button
        type="button"
        class="browser"
        role="radio"
        aria-checked={chosen === candidate.id}
        disabled={browserImport.busy()}
        style:--i={index}
        onclick={() => (chosen = candidate.id)}
      >
        <span class="plate"
          ><Mark mark={BROWSER_MARKS[candidate.browser] ?? BROWSER_MARKS.chrome!} size={30} /></span
        >
        <span class="name">{candidate.name}</span>
      </button>
    {/each}
  </div>

  <div class="next" aria-live="polite">
    {#if job?.finished}
      <span class="done"
        ><Icon icon={Tick02Icon} size={14} />{m.onb_import_done({
          count: number.format(total),
        })}</span
      >
    {:else if job}
      <span class="meter"><i style:transform={`scaleX(${share})`}></i></span>
      <span class="reading">{m.onb_import_reading({ count: number.format(total) })}</span>
      <button type="button" class="stop" onclick={() => void browserImport.cancel()}
        >{m.onb_import_stop()}</button
      >
    {:else if source}
      <div class="kinds">
        {#each ["bookmarks", "history"] as const as kind (kind)}
          <button
            type="button"
            class="kind"
            aria-pressed={kinds.includes(kind)}
            onclick={() => toggle(kind)}
            >{kind === "bookmarks" ? m.onb_import_bookmarks() : m.onb_import_history()}</button
          >
        {/each}
      </div>
      {#if available && source.needsPermission}
        <Button
          variant="primary"
          size="compact"
          shape="capsule"
          onclick={() => void browserImport.openPermissionSettings(source.id)}
          >{m.onb_import_allow()}</Button
        >
      {:else}
        <Button
          variant="primary"
          size="compact"
          shape="capsule"
          disabled={!available || kinds.length === 0 || source.running}
          onclick={() => void browserImport.start(source.id, source.profiles[0]?.id ?? "", kinds)}
          >{m.onb_import_action()}</Button
        >
      {/if}
    {/if}
  </div>
  {#if source && !job}
    <p class="note">
      {#if !available}{m.onb_import_unavailable()}{:else if source.running}{m.onb_import_quit({
          browser: source.name,
        })}{:else if source.needsPermission}{m.onb_import_permission({
          browser: source.name,
        })}{/if}
    </p>
  {/if}
</div>

<style>
  .import {
    display: grid;
    justify-items: start;
    gap: 26px;
    inline-size: 300px;
  }

  .browsers {
    display: grid;
    grid-template-columns: repeat(3, 64px);
    gap: 30px 22px;
  }

  .browser {
    display: grid;
    justify-items: center;
    gap: 9px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-faint);
    font: inherit;
    font-size: 11.5px;
    font-weight: 500;
    cursor: default;
    animation: lift 640ms var(--ease-emphasized) backwards;
    animation-delay: calc(240ms + var(--i) * 50ms);
  }

  .plate {
    display: grid;
    place-items: center;
    inline-size: 64px;
    block-size: 64px;
    border-radius: var(--radius-card);
    background: var(--color-card);
    box-shadow: var(--shadow-raise);
    transition:
      background-color var(--motion-fast) var(--ease-out),
      box-shadow var(--motion-fast) var(--ease-out),
      scale var(--motion-slow) var(--ease-spring);
  }

  .browser:hover .plate {
    background: var(--color-fill-hover);
  }

  .browser:active .plate {
    scale: 0.94;
  }

  .browser:focus-visible {
    outline: none;
  }

  .browser:focus-visible .plate {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .browser[aria-checked="true"] {
    color: var(--color-text);
  }

  .browser[aria-checked="true"] .plate {
    background: var(--color-fill-active);
    box-shadow: inset 0 0 0 1.5px color-mix(in srgb, var(--color-lit) 75%, transparent);
  }

  .next {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
    min-block-size: 32px;
  }

  .kinds {
    display: flex;
    gap: 6px;
  }

  .kind {
    block-size: 28px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: transparent;
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
    color: var(--color-faint);
    font: inherit;
    font-size: 12px;
    font-weight: 500;
    cursor: default;
    transition:
      background-color var(--motion-instant) var(--ease-smooth),
      color var(--motion-instant) var(--ease-smooth);
  }

  .kind[aria-pressed="true"] {
    background: var(--color-control);
    box-shadow: none;
    color: var(--color-text);
  }

  .meter {
    inline-size: 200px;
    block-size: 2px;
    overflow: hidden;
    border-radius: var(--radius-capsule);
    background: var(--color-fill-strong);
  }

  .meter i {
    display: block;
    block-size: 100%;
    background: var(--color-text);
    transform-origin: left center;
    transition: transform var(--motion-page) var(--ease-out);
  }

  .stop {
    padding: 4px 6px;
    border: 0;
    background: none;
    color: var(--color-faint);
    font: inherit;
    font-size: 12px;
    cursor: default;
  }

  .stop:hover {
    color: var(--color-muted);
  }

  .reading,
  .done,
  .note {
    color: var(--color-muted);
    font-size: 12.5px;
    font-variant-numeric: tabular-nums;
  }

  .done {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    color: var(--color-text);
  }

  .note {
    max-inline-size: 30ch;
    margin: -12px 0 0;
    color: var(--color-faint);
    line-height: 1.45;
  }

  @keyframes lift {
    from {
      opacity: 0;
      transform: translateY(10px);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .browser {
      animation: none;
    }
  }
</style>
