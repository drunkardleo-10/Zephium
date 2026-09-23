<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import { Note01Icon } from "@hugeicons/core-free-icons";
  import * as m from "$shared/i18n/messages";
  let {
    title,
    pinned,
    selected = false,
    onopen,
  }: { title: string; pinned: boolean; selected?: boolean; onopen: () => void } = $props();
</script>

<button type="button" class:selected onclick={onopen}
  ><span class="symbol"><Icon icon={Note01Icon} size={18} /></span><strong>{title}</strong
  >{#if pinned}<span aria-label={m.resource_pinned()}>•</span>{/if}</button
>

<style>
  button {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    text-align: start;
    padding: 12px;
    font: inherit;
    border: 1px solid transparent;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--color-text);
    cursor: pointer;
  }

  strong {
    min-width: 0;
    flex: 1;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .symbol {
    color: var(--color-muted);
  }

  button:hover {
    background: var(--color-fill-hover);
  }

  button.selected {
    background: var(--color-fill-active);
    border-color: var(--color-border);
  }

  button:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }
</style>
