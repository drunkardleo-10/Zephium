<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { Cancel01Icon, IncognitoIcon } from "@hugeicons/core-free-icons";
  import { commands } from "$shared/ipc/bindings";
  import Icon from "$shared/ui/Icon";
  import IconButton from "$shared/ui/IconButton";
</script>

<!--
  Heads the column while it shows private tabs, in the band the Browse/Work
  switch holds otherwise: it names the scope and is the way back out of it.
-->
<div class="private-bar" role="group" aria-label={m.private_title()}>
  <Icon icon={IncognitoIcon} size={16} />
  <span class="name">{m.private_title()}</span>
  <button type="button" class="back" onclick={() => void commands.runCommand("window.leavePrivate")}
    >{m.private_back()}</button
  >
  <IconButton
    icon={Cancel01Icon}
    label={m.private_close()}
    size={14}
    buttonSize={24}
    onclick={() => void commands.runCommand("window.closePrivate")}
  />
</div>

<style>
  .private-bar {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 6px;
    min-width: 0;
    height: var(--control-regular);
    padding-inline: 10px 3px;
    border-radius: var(--radius-row);
    background: var(--color-fill);
    color: var(--color-text);
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    font-size: 13px;
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .back {
    flex: none;
    padding: 2px 6px;
    border-radius: 6px;
    font-size: 12px;
    color: var(--color-muted);
  }

  .back:hover,
  .back:focus-visible {
    color: var(--color-text);
    background: var(--color-fill);
  }
</style>
