<script lang="ts">
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import { SquareDashedTopSolidIcon } from "../../lib/icons";
  import * as m from "$shared/i18n/messages";

  let {
    areas,
    selected,
    busy = false,
    oncreate,
  }: {
    areas: readonly { id: string; title: string }[];
    /** Selected objects the new Area gathers. */
    selected: number;
    busy?: boolean;
    /** Resolves true once the Area exists. */
    oncreate: (title: string) => Promise<boolean>;
  } = $props();
  let title = $state("");
</script>

<div class="area-panel">
  <p class="heading">{m.work_env_new_area_hint()}</p>
  <form
    class="create"
    onsubmit={(event) => {
      event.preventDefault();
      const name = title.trim();
      if (name) void oncreate(name).then((made) => made && (title = ""));
    }}
  >
    <span class="glyph"><Icon icon={SquareDashedTopSolidIcon} /></span>
    <input
      aria-label={m.work_env_new_area()}
      bind:value={title}
      maxlength="128"
      placeholder={m.work_env_area()}
      disabled={busy}
    /><Button type="submit" size="compact" disabled={busy || !title.trim()}
      >{selected > 0
        ? m.work_env_group_selection({ count: selected })
        : m.work_env_create()}</Button
    >
  </form>
  {#if areas.length}<p class="heading">{m.work_env_area()}</p>
    <ul>
      {#each areas as area (area.id)}<li>{area.title}</li>{/each}
    </ul>{/if}
</div>

<style>
  .area-panel {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .heading {
    margin: 0;
    padding: 8px 10px 4px;
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  .create {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 4px 4px 10px;
    border-radius: var(--radius-control-compact);
    background: var(--color-field);
  }

  .create input {
    flex: 1;
    min-inline-size: 0;
    border: 0;
    background: transparent;
    color: var(--color-text);
    font: inherit;
    outline: none;
  }

  .glyph {
    display: grid;
    place-items: center;
    flex: none;
    inline-size: 16px;
    color: var(--color-muted);
  }

  ul {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-block-size: 220px;
    margin: 0;
    padding: 0;
    overflow: auto;
    list-style: none;
  }

  li {
    padding: 7px 10px;
    border-radius: var(--radius-control-compact);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
