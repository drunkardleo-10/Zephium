<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { StarIcon } from "@hugeicons/core-free-icons";
  import Icon from "$shared/ui/Icon";
  let { dropping = false }: { dropping?: boolean } = $props();
</script>

<div class="essentials-hint" data-dropping={dropping}>
  <Icon icon={StarIcon} size={13} /><span>{m.settings_essentials_hint()}</span>
</div>
