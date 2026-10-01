<script lang="ts">
  import { Cancel01Icon, Maximize01Icon, MinusSignIcon } from "@hugeicons/core-free-icons";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Icon from "$shared/ui/Icon";
</script>

<div class="flex gap-0.5 pr-2">
  <button
    type="button"
    class="window-button"
    aria-label="Minimize"
    title="Minimize"
    onclick={() => void getCurrentWindow().minimize()}
  >
    <Icon icon={MinusSignIcon} size={13} />
  </button>
  <button
    type="button"
    class="window-button"
    aria-label="Maximize or restore"
    title="Maximize or restore"
    onclick={() => void getCurrentWindow().toggleMaximize()}
  >
    <Icon icon={Maximize01Icon} size={11} />
  </button>
  <button
    type="button"
    class="window-button window-button-close"
    aria-label="Close"
    title="Close"
    onclick={() => void getCurrentWindow().close()}
  >
    <Icon icon={Cancel01Icon} size={13} />
  </button>
</div>
