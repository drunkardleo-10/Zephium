<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import PreviewNotice from "../PreviewNotice.svelte";
  import PreviewToggle from "../PreviewToggle.svelte";
  import SettingsGroup from "$shared/ui/SettingsGroup";
</script>

<PreviewNotice />
<SettingsGroup title={m.developer_inspect()}><PreviewToggle id="developer.menu" /></SettingsGroup>
<SettingsGroup title={m.developer_integrations()}
  ><PreviewToggle id="developer.local" /><PreviewToggle id="developer.confirm" /></SettingsGroup
>
