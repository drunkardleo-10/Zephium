<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import * as m from "$shared/i18n/messages";
  import type { FolderView, ObjectActions } from "../../lib/board/types";
  import { File01Icon, Folder01Icon } from "./icons";
  /** A folder as a small stack: its name, how many it holds, and the first few. */
  let {
    object,
    actions = {},
    centre = false,
  }: {
    object: FolderView;
    actions?: ObjectActions;
    /** Opened in the centre: all of it, at reading size. */
    centre?: boolean;
  } = $props();
  const shown = $derived(centre ? object.entries : object.entries.slice(0, 4));
</script>

<figure class="folder" aria-label={object.name}>
  <span class="behind two" aria-hidden="true"></span>
  <span class="behind one" aria-hidden="true"></span>
  <button type="button" class="front nodrag nopan" onclick={() => actions.open?.(object.id)}>
    <span class="head">
      <Icon icon={Folder01Icon} size={18} />
      <span class="name">{object.name}</span>
      <span class="count">{m.work_folder_count({ count: object.count })}</span>
    </span>
    {#if shown.length}
      <ul>
        {#each shown as entry (entry.name)}
          <li>
            {#if entry.picture}<img
                src={entry.picture.src}
                alt=""
                width="18"
                height="18"
                decoding="async"
              />{:else}<Icon icon={entry.folder ? Folder01Icon : File01Icon} size={15} />{/if}<span
              >{entry.name}</span
            >
          </li>
        {/each}
      </ul>
    {/if}
  </button>
</figure>

<style>
  .folder {
    position: relative;
    margin: 0;
    padding-block-start: 12px;
    inline-size: 100%;
    color: var(--color-text);
  }

  .behind {
    position: absolute;
    inset-inline: 12px;
    block-size: 24px;
    border-radius: var(--radius-card) var(--radius-card) 0 0;
    background: var(--color-surface);
    box-shadow: var(--shadow-raised);
  }

  .behind.two {
    inset-block-start: 0;
    inset-inline: 24px;
    opacity: 0.55;
  }

  .behind.one {
    inset-block-start: 6px;
    opacity: 0.8;
  }

  .front {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 12px;
    box-sizing: border-box;
    inline-size: 100%;
    padding: 14px 16px 16px;
    border: 0;
    border-radius: var(--radius-card);
    background: var(--color-surface);
    box-shadow: var(--shadow-float);
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: default;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 9px;
    color: var(--color-muted);
  }

  .name {
    flex: 1;
    min-inline-size: 0;
    color: var(--color-text);
    font-size: var(--text-body);
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .count {
    flex: none;
    font-size: var(--text-label);
    font-variant-numeric: tabular-nums;
  }

  ul {
    display: flex;
    flex-direction: column;
    gap: 7px;
    margin: 0;
    padding: 10px 0 0;
    border-block-start: 1px solid var(--color-border);
    list-style: none;
    color: var(--color-label-secondary);
    font-size: var(--text-label);
  }

  li {
    display: flex;
    align-items: center;
    gap: 8px;
    min-inline-size: 0;
    color: var(--color-muted);
  }

  li span {
    min-inline-size: 0;
    color: var(--color-label-secondary);
    overflow-wrap: anywhere;
  }

  li img {
    border-radius: var(--radius-inset);
    object-fit: cover;
  }
</style>
