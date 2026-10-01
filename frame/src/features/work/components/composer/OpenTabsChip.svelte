<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import { BrowserIcon, Cancel01Icon } from "../../lib/icons";
  import * as m from "$shared/i18n/messages";
  let { disabled = false, onremove }: { disabled?: boolean; onremove: () => void } = $props();
</script>

<!-- Consent for this request only: the tabs' titles and addresses, never their pages. -->
<span class="open-tabs" title={m.work_env_all_tabs_note()}>
  <Icon icon={BrowserIcon} size={13} />
  <span>{m.work_context_open_tabs()}</span>
  <button type="button" aria-label={m.work_context_open_tabs_remove()} {disabled} onclick={onremove}
    ><Icon icon={Cancel01Icon} size={12} /></button
  >
</span>

<style>
  .open-tabs {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 2px 2px 2px 8px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill-active);
    color: var(--color-text);
    font-size: var(--text-caption);
  }

  button {
    display: grid;
    place-items: center;
    inline-size: 20px;
    block-size: 20px;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--color-muted);
    cursor: default;
  }

  button:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 1px;
  }

  button:hover:not(:disabled) {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }
</style>
