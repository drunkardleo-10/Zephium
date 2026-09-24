<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { surface as browser } from "$domain/surface";
  import { tabs } from "$domain/tabs";
  import { DownloadsList } from "$features/downloads";
  import IconButton from "$shared/ui/IconButton";
  import { ArrowLeft02Icon } from "@hugeicons/core-free-icons";
  let { kind }: { kind: "downloads" } = $props();
</script>

<section class="library-shell" data-library={kind}>
  <header class="internal-toolbar">
    <IconButton
      icon={ArrowLeft02Icon}
      label={m.settings_back()}
      onclick={() => void browser.open(null)}
    /><span>{m.browser_downloads_title()}</span>
  </header>
  <div class="library-content">
    <header class="settings-page-heading">
      <h1>{m.browser_downloads_title()}</h1>
      <p>{m.browser_downloads_description()}</p>
    </header>
    {#if tabs.profile()?.id}{#key tabs.profile()?.id}<DownloadsList
          profile={tabs.profile()!.id}
        />{/key}{/if}
  </div>
</section>
