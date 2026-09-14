<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { resourceSession } from "$domain/resources";
  import { commands } from "$shared/ipc/bindings";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import { File01Icon, Image01Icon, Pdf01Icon, Upload01Icon } from "../lib/icons";
  import * as m from "$shared/i18n/messages";
  let {
    profile,
    host,
    attachedIds = [],
    pending = false,
    onattach,
  }: {
    profile: string;
    host: string;
    attachedIds?: readonly string[];
    pending?: boolean;
    onattach: (ids: string[]) => void;
  } = $props();
  const session = untrack(() => resourceSession(profile, "media", host));
  onMount(() => {
    void session?.start();
    return () => session?.stopObserving();
  });
  const items = $derived(session?.items ?? []);
  let selected = $state<string[]>([]);
  let importing = $state(false);
  let failure = $state<string | null>(null);
  const eligible = $derived(
    selected.filter((id) => items.some((item) => item.id === id) && !attachedIds.includes(id)),
  );
  function toggle(id: string) {
    selected = selected.includes(id) ? selected.filter((x) => x !== id) : [...selected, id];
  }
  async function importFile() {
    if (importing) return;
    importing = true;
    failure = null;
    try {
      const result = await commands.mediaImport(profile);
      if (result.status !== "ok") {
        failure = m.work_media_import_failed();
        return;
      }
      const outcome = result.data;
      if (outcome.kind === "imported") {
        await session?.reload();
        onattach([outcome.record.id]);
      } else if (outcome.kind === "refused") {
        failure =
          outcome.error === "capacity"
            ? m.work_media_import_too_large()
            : m.work_media_import_failed();
      }
    } catch {
      failure = m.work_media_import_failed();
    } finally {
      importing = false;
    }
  }
</script>

<section class="picker" aria-label={m.work_env_media()} aria-busy={pending || importing}>
  <header><strong>{m.work_env_media()}</strong></header>
  <ul>
    {#each items as item (item.id)}
      <li>
        <label class="row">
          <input
            type="checkbox"
            checked={eligible.includes(item.id)}
            disabled={pending || attachedIds.includes(item.id)}
            onchange={() => toggle(item.id)}
          />
          <span class="glyph"
            ><Icon
              icon={item.title.toLowerCase().endsWith(".pdf")
                ? Pdf01Icon
                : /\.(png|jpe?g|gif|webp)$/i.test(item.title)
                  ? Image01Icon
                  : File01Icon}
              size={14}
            /></span
          >
          <span class="identity"><strong>{item.title}</strong></span>
          {#if attachedIds.includes(item.id)}<span class="attached">{m.work_media_added()}</span
            >{/if}
        </label>
      </li>
    {:else}<li class="empty">{m.work_media_none()}</li>{/each}
  </ul>
  {#if failure}<p role="alert">{failure}</p>{/if}
  <footer>
    <Button size="compact" disabled={pending || importing} onclick={() => void importFile()}
      ><Icon icon={Upload01Icon} size={13} />{importing
        ? m.work_media_importing()
        : m.work_media_import()}</Button
    ><Button
      size="compact"
      variant="primary"
      disabled={pending || eligible.length === 0}
      onclick={() => onattach([...eligible])}>{m.work_media_add({ count: eligible.length })}</Button
    >
  </footer>
</section>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-inline-size: 0;
  }

  header strong {
    font-size: var(--text-body);
    font-weight: 600;
  }

  ul {
    display: grid;
    gap: 2px;
    max-block-size: 320px;
    margin: 0;
    padding: 0;
    overflow: auto;
    list-style: none;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    border-radius: var(--radius-sm);
  }

  .row:hover {
    background: var(--color-fill);
  }

  .glyph {
    display: inline-flex;
    color: var(--color-muted);
  }

  .identity {
    flex: 1;
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-caption);
  }

  .attached,
  .empty {
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  .empty {
    padding: 8px;
  }

  p[role="alert"] {
    margin: 0;
    color: var(--color-danger);
    font-size: var(--text-caption);
  }

  footer {
    display: flex;
    justify-content: space-between;
    gap: 8px;
  }
</style>
