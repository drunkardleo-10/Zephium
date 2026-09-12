<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import * as preview from "../lib/preview.svelte";
  import PreviewDialog from "./PreviewDialog.svelte";
  import Button from "$shared/ui/Button";
  import Field from "$shared/ui/Field";
  import Select from "$shared/ui/Select";
  import SearchField from "$shared/ui/SearchField";
  const kinds = [
    { value: "camera", label: m.pref_privacy_camera },
    { value: "microphone", label: m.pref_privacy_microphone },
    { value: "location", label: m.pref_privacy_location },
    { value: "notifications", label: m.pref_privacy_notifications },
    { value: "popups", label: m.permission_popups },
    { value: "clipboard", label: m.permission_clipboard },
  ];
  const policies = [
    { value: "ask", label: m.permission_ask },
    { value: "allow", label: m.permission_allow },
    { value: "block", label: m.permission_block },
  ];
  let open = $state(false);
  let remove = $state<string | null>(null);
  let id = $state("");
  let site = $state("");
  let kind = $state("camera");
  let policy = $state("ask");
  let query = $state("");
  let origin = $derived.by(() => {
    try {
      const url = new URL(site);
      return ["https:", "http:"].includes(url.protocol) ? url.origin : null;
    } catch {
      return null;
    }
  });
  let duplicate = $derived(
    preview
      .entries("privacy.site-exceptions")
      .some(
        (entry) => entry.id !== id && entry.name === origin && entry.value.split(":")[0] === kind,
      ),
  );
  let entries = $derived(
    preview
      .entries("privacy.site-exceptions")
      .filter((entry) =>
        (entry.name + " " + entry.value).toLowerCase().includes(query.toLowerCase()),
      ),
  );
  function edit(entry?: { id: string; name: string; value: string }) {
    id = entry?.id ?? crypto.randomUUID();
    site = entry?.name ?? "";
    kind = entry?.value.split(":")[0] ?? "camera";
    policy = entry?.value.split(":")[1] ?? "ask";
    open = true;
  }
</script>

<section class="settings-collection" data-setting="privacy.exceptions">
  <header>
    <div>
      <h2>{m.pref_privacy_exceptions()}</h2>
      <p>{m.permission_context()}</p>
    </div>
    <Button
      onclick={() => edit()}
      disabled={preview.entries("privacy.site-exceptions").length >= 100}>{m.action_add()}</Button
    >
  </header>
  {#if preview.entries("privacy.site-exceptions").length}<SearchField
      label={m.settings_search()}
      placeholder={m.settings_search()}
      bind:value={query}
    />{/if}
  {#if entries.length === 0}<div class="collection-empty">{m.permission_empty()}</div>{:else}<ul>
      {#each entries as entry (entry.id)}{@const parts = entry.value.split(":")}
        <li>
          <button type="button" class="collection-entry" onclick={() => edit(entry)}
            ><strong>{entry.name}</strong><span
              >{kinds.find((item) => item.value === parts[0])?.label()} · {policies
                .find((item) => item.value === parts[1])
                ?.label()}</span
            ></button
          ><Button variant="ghost" onclick={() => (remove = entry.id)}>{m.action_remove()}</Button>
        </li>{/each}
    </ul>{/if}
</section>
<PreviewDialog
  bind:open
  title={m.permission_edit()}
  description={m.preview_description()}
  valid={origin !== null && !duplicate}
  onapply={() =>
    preview.saveEntry("privacy.site-exceptions", { id, name: origin!, value: `${kind}:${policy}` })}
  ><Field
    label={m.permission_site()}
    bind:value={site}
    type="url"
    placeholder="https://example.com"
    required
    maxlength={500}
    error={site && !origin
      ? m.permission_invalid()
      : duplicate
        ? m.permission_duplicate()
        : undefined}
  /><Select
    label={m.permission_kind()}
    bind:value={kind}
    options={kinds.map((item) => ({ value: item.value, label: item.label() }))}
  /><Select
    label={m.permission_behavior()}
    bind:value={policy}
    options={policies.map((item) => ({ value: item.value, label: item.label() }))}
  /></PreviewDialog
>
<PreviewDialog
  open={remove !== null}
  title={m.preview_delete_title()}
  description={m.preview_delete_body()}
  onclose={() => (remove = null)}
  onapply={() => {
    if (remove) preview.removeEntry("privacy.site-exceptions", remove);
    remove = null;
  }}>{m.preview_delete_body()}</PreviewDialog
>
