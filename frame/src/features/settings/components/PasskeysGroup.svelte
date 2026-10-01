<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { browserCredentials, browserPasskeyStatus } from "$domain/credentials";
  import Button from "$shared/ui/Button";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import SettingsRow from "$shared/ui/SettingsRow";
  import * as m from "$shared/i18n/messages";

  let credentials = $derived(browserCredentials.current());

  onMount(() => void browserCredentials.activate());
  onDestroy(() => browserCredentials.deactivate());
</script>

{#if credentials !== null && credentials.passkey_authorization !== "unsupported"}
  <SettingsGroup title={m.settings_passkeys()}>
    <SettingsRow
      title={m.settings_passkeys()}
      description={browserPasskeyStatus(credentials.passkey_authorization)}
    >
      {#if credentials.can_request_passkey_authorization}
        <Button
          size="compact"
          variant="secondary"
          pending={browserCredentials.busy()}
          onclick={() => void browserCredentials.requestPasskeyAuthorization()}
          >{m.settings_enable_passkeys()}</Button
        >
      {/if}
    </SettingsRow>
  </SettingsGroup>
{/if}
