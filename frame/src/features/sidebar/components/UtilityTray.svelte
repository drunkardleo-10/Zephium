<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import type { Snippet } from "svelte";
  import { Settings05Icon } from "@hugeicons/core-free-icons";
  import Icon from "$shared/ui/Icon";
  import Disclosure from "$shared/ui/Disclosure";

  let { children, onopen }: { children: Snippet; onopen?: () => void } = $props();
</script>

<!--
  Everything that acts on the page without being the page: extensions, the
  protection standing, whatever else earns a place later. One glyph holds
  them, because none of them is worth a permanent seat in the chrome.
-->
<div class="utilities">
  <Disclosure
    {onopen}
    label={m.utilities()}
    menu
    triggerClass="utilities-trigger"
    panelClass="utilities-panel"
    align="end"
  >
    {#snippet trigger()}<Icon icon={Settings05Icon} size={15} />{/snippet}
    {@render children()}
  </Disclosure>
</div>

<style>
  /*
    Present but unpainted until the field it rides is in use. It acts on the
    page that field names, so it has no business drawing the eye the rest of
    the time; hovering the field, focusing into it, or its own panel being
    open all count as in use. The hook belongs to the field, which is the
    only element that knows when it is being addressed.
  */
  .utilities {
    opacity: 0;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .utilities:has(:global([data-state="open"])),
  :global(.address-field:hover) .utilities,
  :global(.address-field:focus-within) .utilities {
    opacity: 1;
  }

  .utilities :global(.utilities-trigger) {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border: 0;
    border-radius: var(--radius-inset);
    background: transparent;
    color: var(--color-faint);
    cursor: default;
    outline: none;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out);
  }

  .utilities :global(.utilities-panel) {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 212px;
  }

  .utilities :global(.utilities-trigger:hover),
  .utilities :global(.utilities-trigger[data-state="open"]) {
    background: var(--row-hover);
    color: var(--color-text);
  }
</style>
