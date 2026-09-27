<script lang="ts">
  import { extensions } from "$domain/extensions";
  import { AddressField } from "$features/address";
  import { UtilityTray } from "$features/sidebar";
  import { ExtensionActions, ExtensionManager } from "$features/extensions";
  import { surface } from "$domain/surface";
</script>

<AddressField />
<UtilityTray openRequest={extensions.storeReviewRequest()}>
  <ExtensionActions />
</UtilityTray>
{#if surface.currentPage() === "extensions"}<ExtensionManager embedded />{/if}
