<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import * as preview from "../../lib/preview.svelte";
  import PreviewNotice from "../PreviewNotice.svelte";
  import PreviewDialog from "../PreviewDialog.svelte";
  import Button from "$shared/ui/Button";
  import Field from "$shared/ui/Field";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import SettingsRow from "$shared/ui/SettingsRow";
  import Icon from "$shared/ui/Icon";
  import { UserCircleIcon } from "@hugeicons/core-free-icons";
  let open = $state(false);
  let email = $state(preview.get("account.email", ""));
</script>

<PreviewNotice />
{#if !preview.get("account.signedin", false)}<div class="settings-intro">
    <Icon icon={UserCircleIcon} size={34} />
    <h2>{m.settings_account_heading()}</h2>
    <p>{m.settings_account_body()}</p>
    <Button onclick={() => (open = true)}>{m.preview_account_signin()}</Button>
  </div>{:else}<div class="identity-card">
    <div class="identity-avatar"><Icon icon={UserCircleIcon} size={28} /></div>
    <div>
      <h2>{preview.get("account.email", "")}</h2>
      <p>{m.preview_account_signedin()}</p>
    </div>
  </div>
  <SettingsGroup title={m.preview_account_usage()}
    ><SettingsRow title={m.preview_plan()}>{m.preview_plan_local()}</SettingsRow><SettingsRow
      title={m.preview_usage()}><span>—</span></SettingsRow
    ></SettingsGroup
  ><Button onclick={() => preview.set("account.signedin", false)}>{m.preview_signout()}</Button
  >{/if}
<PreviewDialog
  bind:open
  title={m.preview_account_signin()}
  description={m.preview_account_note()}
  valid={/^[^\s@]+@[^\s@]+\.[^\s@]+$/u.test(email)}
  onapply={() => {
    preview.set("account.email", email);
    preview.set("account.signedin", true);
  }}
  ><Field
    label={m.preview_email()}
    type="email"
    bind:value={email}
    required
    autocomplete="off"
  /></PreviewDialog
>
