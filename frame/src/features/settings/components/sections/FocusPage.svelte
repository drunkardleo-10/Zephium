<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { loadShutSites } from "$features/time";
  import LazyView from "$shared/ui/LazyView";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import PreferenceSelect from "../PreferenceSelect.svelte";
  import PreferenceSwitch from "../PreferenceSwitch.svelte";
</script>

<SettingsGroup title={m.time_title()} description={m.time_local_only()}
  ><PreferenceSwitch id="time.track" preference="time.track" /><PreferenceSelect
    id="time.retention"
    preference="time.retention"
  /></SettingsGroup
>
<SettingsGroup title={m.focus_title()}
  ><PreferenceSelect id="focus.minutes" preference="focus.minutes" /><PreferenceSwitch
    id="focus.breaks"
    preference="focus.breaks"
  /><PreferenceSelect id="focus.goal" preference="focus.goal" /></SettingsGroup
>
<SettingsGroup title={m.focus_shut_title()} description={m.focus_shut_help()}>
  <div class="shut">
    <LazyView
      loader={loadShutSites}
      loadingLabel={m.surface_loading()}
      failureLabel={m.surface_render_failed()}
      retryLabel={m.surface_retry()}>{#snippet children(View)}<View />{/snippet}</LazyView
    >
  </div>
</SettingsGroup>

<style>
  .shut {
    padding: 14px 16px;
  }
</style>
