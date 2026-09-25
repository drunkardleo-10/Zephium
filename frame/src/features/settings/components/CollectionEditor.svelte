<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import * as preview from "../lib/preview.svelte";
  import { fields, type FieldId } from "../lib/catalog";
  import Button from "$shared/ui/Button";
  import SearchField from "$shared/ui/SearchField";
  import Field from "$shared/ui/Field";
  import PreviewDialog from "./PreviewDialog.svelte";
  let { id, kind = "text" }: { id: FieldId; kind?: "text" | "url" | "shortcut" | "credential" } =
    $props();
  let field = $derived(fields[id]);
  let query = $state("");
  let filtered = $derived(
    preview
      .entries(id)
      .filter((item) =>
        (item.name + " " + item.value).toLocaleLowerCase().includes(query.toLocaleLowerCase()),
      ),
  );
  let open = $state(false);
  let removing = $state<string | null>(null);
  let entryId = $state("");
  let name = $state("");
  let value = $state("");
  let secret = $state("");
  let duplicate = $derived(
    preview
      .entries(id)
      .some(
        (item) =>
          item.id !== entryId && item.name.toLocaleLowerCase() === name.trim().toLocaleLowerCase(),
      ),
  );
  let urlValid = $derived(
    kind !== "url" || /^(https?:\/\/|[a-z0-9][a-z0-9.-]+\.[a-z]{2,})/iu.test(value),
  );
  let shortcutConflict = $derived(
    kind === "shortcut" &&
      preview.entries(id).some((item) => item.id !== entryId && item.value === value),
  );
  let valid = $derived(
    Boolean(name.trim() && value.trim()) && !duplicate && !shortcutConflict && urlValid,
  );
  function edit(item?: { id: string; name: string; value: string }) {
    entryId = item?.id ?? crypto.randomUUID();
    name = item?.name ?? "";
    value = item?.value ?? "";
    secret = "";
    open = true;
  }
</script>

<section class="settings-collection" data-setting={id}>
  <header>
    <div>
      <h2>{field.label()}</h2>
      <p>{field.description()}</p>
    </div>
    <Button onclick={() => edit()} disabled={preview.entries(id).length >= 100}
      >{m.action_add()}</Button
    >
  </header>
  {#if preview.entries(id).length > 0}<SearchField
      label={m.settings_search()}
      placeholder={m.settings_search()}
      bind:value={query}
    />{/if}
  {#if preview.entries(id).length === 0}<div class="collection-empty">
      {m.preview_empty()}
    </div>{:else}<ul>
      {#each filtered as item (item.id)}<li>
          <button type="button" class="collection-entry" onclick={() => edit(item)}
            ><strong>{item.name}</strong><span>{item.value}</span></button
          ><Button variant="ghost" onclick={() => (removing = item.id)}>{m.action_remove()}</Button>
        </li>{/each}
    </ul>{/if}
</section>
<PreviewDialog
  bind:open
  title={field.label()}
  description={m.preview_collection_help()}
  {valid}
  onapply={() => {
    preview.saveEntry(id, { id: entryId, name: name.trim(), value: value.trim() });
    secret = "";
  }}
>
  <Field
    label={m.field_name()}
    bind:value={name}
    required
    maxlength={100}
    error={duplicate ? m.field_duplicate() : undefined}
  />
  <Field
    label={kind === "url" ? m.field_url() : m.field_value()}
    bind:value
    required
    maxlength={500}
    error={shortcutConflict
      ? m.field_shortcut_conflict()
      : value && !urlValid
        ? m.field_invalid_url()
        : undefined}
    onkeydown={kind === "shortcut"
      ? (event) => {
          event.preventDefault();
          value = [
            event.metaKey ? "⌘" : "",
            event.ctrlKey ? "Ctrl" : "",
            event.altKey ? "Alt" : "",
            event.shiftKey ? "Shift" : "",
            event.key.length === 1 ? event.key.toUpperCase() : event.key,
          ]
            .filter(Boolean)
            .join(" + ");
        }
      : undefined}
  />
  {#if kind === "credential"}<Field
      label={m.preview_passwords()}
      type="password"
      bind:value={secret}
      autocomplete="off"
    />{/if}
</PreviewDialog>
<PreviewDialog
  open={removing !== null}
  onclose={() => (removing = null)}
  title={m.preview_delete_title()}
  description={m.preview_delete_body()}
  onapply={() => {
    if (removing) preview.removeEntry(id, removing);
    removing = null;
  }}>{m.preview_delete_body()}</PreviewDialog
>
