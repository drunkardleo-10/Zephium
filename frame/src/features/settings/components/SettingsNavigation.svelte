<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { surface as browser } from "$domain/surface";
  import * as state from "../lib/settings-state.svelte";
  import { groups, sections } from "../lib/settings-model";
  import Icon from "$shared/ui/Icon";
  import { ArrowLeft02Icon, Search01Icon } from "@hugeicons/core-free-icons";
</script>

<nav class="settings-sidebar-navigation" aria-label={m.settings_title()}>
  <button type="button" class="settings-back" onclick={() => void browser.open(null)}
    ><Icon icon={ArrowLeft02Icon} size={15} />{m.settings_back()}</button
  >
  <div class="settings-sidebar-search">
    <Icon icon={Search01Icon} size={15} /><input
      type="search"
      aria-label={m.settings_search()}
      placeholder={m.settings_search()}
      value={state.query()}
      oninput={(e) => state.setQuery(e.currentTarget.value)}
    />
  </div>
  <div class="settings-navigation-list">
    {#each groups as group (group.id)}<div class="settings-nav-group">
        <h2>{group.label()}</h2>
        {#each sections.filter((item) => item.group === group.id) as item (item.id)}<button
            type="button"
            class="settings-nav-item"
            aria-current={state.section() === item.id && !state.query() ? "page" : undefined}
            onclick={() => state.select(item.id)}
            ><Icon icon={item.icon} size={16} /><span>{item.title()}</span></button
          >{/each}
      </div>{/each}
  </div>
</nav>
