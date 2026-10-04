<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import PreferenceSwitch from "../PreferenceSwitch.svelte";
  import PreferenceSelect from "../PreferenceSelect.svelte";
  import PerformanceExceptions from "../PerformanceExceptions.svelte";
</script>

<SettingsGroup title={m.settings_sleeping_tabs()}
  ><PreferenceSwitch id="performance.sleep" preference="performance.sleep" /><PreferenceSelect
    id="performance.after"
    preference="performance.after"
  /></SettingsGroup
><SettingsGroup title={m.settings_resource_policy()}
  ><PreferenceSelect id="performance.memory" preference="performance.memory" /></SettingsGroup
>
<SettingsGroup
  title={m.pref_performance_exceptions()}
  description={m.pref_performance_exceptions_help()}
>
  <PerformanceExceptions />
</SettingsGroup>
