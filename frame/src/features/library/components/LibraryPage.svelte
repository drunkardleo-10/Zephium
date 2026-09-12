<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { surface as browser } from "$domain/surface";
  import IconButton from "$shared/ui/IconButton";
  import Icon from "$shared/ui/Icon";
  import { ArrowLeft02Icon, Clock01Icon, Download01Icon } from "@hugeicons/core-free-icons";
  let { kind }: { kind: "history" | "downloads" } = $props();
  let history = $derived(kind === "history");
</script>

<section class="library-shell">
  <header class="internal-toolbar">
    <IconButton
      icon={ArrowLeft02Icon}
      label={m.settings_back()}
      onclick={() => void browser.open(null)}
    /><span>{history ? m.browser_history_title() : m.browser_downloads_title()}</span>
  </header>
  <div class="library-content">
    <header class="settings-page-heading">
      <h1>{history ? m.browser_history_title() : m.browser_downloads_title()}</h1>
      <p>{history ? m.browser_history_description() : m.browser_downloads_description()}</p>
    </header>
    <div class="library-empty">
      <span class="library-icon"
        ><Icon icon={history ? Clock01Icon : Download01Icon} size={28} /></span
      >
      <h2>{history ? m.browser_history_empty() : m.browser_downloads_empty()}</h2>
      <p>{history ? m.browser_history_unavailable() : m.browser_downloads_unavailable()}</p>
    </div>
  </div>
</section>
