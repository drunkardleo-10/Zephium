<script lang="ts" module>
  export type AttachKind = "tabs" | "document" | "image" | "link" | "folder";
</script>

<script lang="ts">
  import type { Snippet } from "svelte";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import { BrowserIcon, File01Icon, FolderAddIcon, Image01Icon, Link04Icon } from "../../lib/icons";
  import * as m from "$shared/i18n/messages";

  let {
    kind = $bindable("tabs"),
    busy = false,
    folderPending = false,
    picker,
    media,
    onaddlink,
    onaddfolder,
    onchoosefolder,
  }: {
    kind?: AttachKind;
    busy?: boolean;
    folderPending?: boolean;
    picker: Snippet;
    media: Snippet<["document" | "image"]>;
    /** Resolves false when the address could not become a card. */
    onaddlink: (raw: string) => Promise<boolean>;
    onaddfolder: (path: string) => Promise<boolean>;
    onchoosefolder: () => void;
  } = $props();

  const segments = [
    { key: "tabs" as const, icon: BrowserIcon, label: m.work_env_tabs },
    { key: "document" as const, icon: File01Icon, label: m.work_env_documents },
    { key: "image" as const, icon: Image01Icon, label: m.work_env_images },
    { key: "link" as const, icon: Link04Icon, label: m.work_env_links },
    { key: "folder" as const, icon: FolderAddIcon, label: m.work_env_folders },
  ];
  let link = $state("");
  let linkPending = $state(false);
  let linkFailed = $state(false);
  let folder = $state("");

  async function addLink() {
    if (!link.trim() || linkPending || busy) return;
    linkPending = true;
    linkFailed = false;
    try {
      if (await onaddlink(link)) link = "";
      else linkFailed = true;
    } finally {
      linkPending = false;
    }
  }

  async function addFolder() {
    if (!folder.trim() || folderPending || busy) return;
    if (await onaddfolder(folder.trim())) folder = "";
  }
</script>

<!-- Everything that can join the canvas from outside it, one kind at a time. -->
<div class="attach">
  <div class="segments" role="group" aria-label={m.work_tool_attach()}>
    {#each segments as segment (segment.key)}
      <button
        type="button"
        class="segment"
        aria-pressed={kind === segment.key}
        onclick={() => (kind = segment.key)}
      >
        <Icon icon={segment.icon} size={14} /><span>{segment.label()}</span>
      </button>
    {/each}
  </div>
  {#if kind === "tabs"}
    {@render picker()}
  {:else if kind === "folder"}
    <form
      class="create"
      onsubmit={(event) => {
        event.preventDefault();
        void addFolder();
      }}
    >
      <span class="glyph"><Icon icon={FolderAddIcon} /></span>
      <input
        aria-label={m.work_env_folder_placeholder()}
        placeholder={m.work_env_folder_placeholder()}
        bind:value={folder}
        maxlength="1024"
        disabled={busy || folderPending}
      /><Button type="submit" size="compact" disabled={busy || folderPending || !folder.trim()}
        >{m.work_env_folder_add()}</Button
      >
    </form>
    <div class="footer">
      <Button size="compact" disabled={busy || folderPending} onclick={onchoosefolder}
        >{m.work_env_folder_choose()}</Button
      >
    </div>
    <p class="note">{m.work_env_folder_hint()}</p>
  {:else if kind === "link"}
    <form
      class="create"
      onsubmit={(event) => {
        event.preventDefault();
        void addLink();
      }}
    >
      <span class="glyph"><Icon icon={Link04Icon} /></span>
      <input
        type="url"
        aria-label={m.work_env_link_placeholder()}
        placeholder={m.work_env_link_placeholder()}
        bind:value={link}
        maxlength="2048"
        disabled={busy || linkPending}
      /><Button type="submit" size="compact" disabled={busy || linkPending || !link.trim()}
        >{m.work_env_link_add()}</Button
      >
    </form>
    {#if linkFailed}<p class="note" role="alert">{m.work_env_link_failed()}</p>{/if}
  {:else}
    {@render media(kind)}
  {/if}
</div>

<style>
  .attach {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-inline-size: 0;
  }

  .segments {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: var(--radius-control-compact);
    background: var(--color-fill);
  }

  .segment {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    flex: 1;
    min-inline-size: 0;
    block-size: 28px;
    padding-inline: 6px;
    overflow: hidden;
    white-space: nowrap;
    border: 0;
    border-radius: var(--radius-inset);
    background: transparent;
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-label);
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out);
  }

  .segment span {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .segment:hover {
    color: var(--color-text);
  }

  .segment:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }

  .segment[aria-pressed="true"] {
    background: var(--row-active);
    box-shadow: var(--row-rim);
    color: var(--color-text);
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

  .footer {
    display: flex;
    justify-content: flex-end;
    padding-inline: 4px;
  }

  .note {
    margin: 0;
    padding: 0 10px 4px;
    color: var(--color-muted);
    font-size: var(--text-caption);
  }
</style>
