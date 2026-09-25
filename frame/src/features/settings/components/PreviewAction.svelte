<script lang="ts">
  import type { Snippet } from "svelte";
  import { fields, type FieldId } from "../lib/catalog";
  import * as m from "$shared/i18n/messages";
  import SettingsRow from "$shared/ui/SettingsRow";
  import Button from "$shared/ui/Button";
  import PreviewDialog from "./PreviewDialog.svelte";
  let {
    id,
    children,
    onapply,
    valid = true,
    actionLabel = m.action_choose(),
    value,
    onopen,
  }: {
    id: FieldId;
    children: Snippet;
    onapply?: () => void;
    valid?: boolean;
    actionLabel?: string;
    value?: string;
    onopen?: () => void;
  } = $props();
  let field = $derived(fields[id]);
  let open = $state(false);
</script>

<div data-setting={id}>
  <SettingsRow title={field.label()} description={field.description()}
    ><div class="settings-action-control">
      {#if value}<span class="settings-value">{value}</span>{/if}<Button
        onclick={() => {
          onopen?.();
          open = true;
        }}>{actionLabel}</Button
      >
    </div></SettingsRow
  >
</div>
<PreviewDialog bind:open title={field.label()} {onapply} {valid}>{@render children()}</PreviewDialog
>
