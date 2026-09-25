<script lang="ts">
  import { Dialog } from "bits-ui";
  import { untrack, type Snippet } from "svelte";
  import * as preview from "../lib/preview.svelte";
  import * as m from "$shared/i18n/messages";
  import Button from "$shared/ui/Button";
  let {
    open = $bindable(false),
    title,
    description = m.preview_description(),
    children,
    onapply,
    valid = true,
    onclose,
    applyLabel = m.preview_apply(),
  }: {
    open?: boolean;
    title: string;
    description?: string;
    children: Snippet;
    onapply?: () => void;
    valid?: boolean;
    onclose?: () => void;
    applyLabel?: string;
  } = $props();
  let checkpoint: ReturnType<typeof preview.capture> | null = null;
  let committed = false;
  let opener: HTMLElement | null = null;
  $effect(() => {
    if (open)
      untrack(() => {
        opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
        checkpoint = preview.capture();
        committed = false;
      });
  });
  function cancel() {
    if (checkpoint && !committed) preview.restore(checkpoint);
    checkpoint = null;
    open = false;
    onclose?.();
  }
</script>

<Dialog.Root
  bind:open
  onOpenChange={(value) => {
    if (!value) cancel();
  }}
  ><Dialog.Portal
    ><Dialog.Overlay class="settings-dialog-overlay" /><Dialog.Content
      class="settings-dialog"
      onCloseAutoFocus={(event) => {
        if (opener?.isConnected) {
          event.preventDefault();
          opener.focus();
        }
      }}
      ><Dialog.Title class="settings-dialog-title">{title}</Dialog.Title><Dialog.Description
        class="settings-dialog-description">{description}</Dialog.Description
      >
      <form
        onsubmit={(event) => {
          event.preventDefault();
          if (valid) {
            committed = true;
            onapply?.();
            open = false;
          }
        }}
      >
        <div class="settings-dialog-body">{@render children()}</div>
        <footer>
          <Button onclick={cancel}>{m.action_cancel()}</Button><Button
            type="submit"
            variant="primary"
            disabled={!valid}>{applyLabel}</Button
          >
        </footer>
      </form></Dialog.Content
    ></Dialog.Portal
  ></Dialog.Root
>
