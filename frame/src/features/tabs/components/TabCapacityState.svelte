<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { tabs } from "$domain/tabs";
  import type { TabView } from "$shared/ipc/bindings";
  import Button from "$shared/ui/Button";

  let { tab }: { tab: TabView } = $props();
  let waiting = $derived(tab.availability?.state === "waiting_for_capacity");
</script>

<div class="capacity" role="status">
  <h1>{waiting ? m.tabs_capacity_waiting() : m.tabs_capacity_blocked()}</h1>
  <p>{waiting ? m.tabs_capacity_waiting_help() : m.tabs_capacity_blocked_help()}</p>
  {#if tab.availability?.state === "blocked_by_capacity"}
    <Button
      onclick={() => {
        const state = tab.availability;
        if (state?.state === "blocked_by_capacity") tabs.navigate(tab.id, state.url);
      }}>{m.surface_retry()}</Button
    >
  {/if}
</div>

<style>
  .capacity {
    display: grid;
    justify-items: center;
    align-content: center;
    gap: 12px;
    block-size: 100%;
    padding: 24px;
    text-align: center;
  }

  h1 {
    margin: 0;
    color: var(--color-text);
    font-size: var(--text-title);
    font-weight: 500;
  }

  p {
    margin: 0;
    max-inline-size: 40ch;
    color: var(--color-muted);
    font-size: var(--text-body);
  }
</style>
