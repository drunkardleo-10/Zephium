<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import { UserIcon } from "../../lib/icons";
  import * as m from "$shared/i18n/messages";
  let { host, size = 16 }: { host: string; size?: number } = $props();
</script>

<!-- Read with the person's own session: a person on the lit rung, beside the site's mark. -->
<span
  class="account-badge"
  role="img"
  title={m.work_account_badge({ host })}
  aria-label={m.work_account_badge({ host })}
  style:--badge-size={`${size}px`}><Icon icon={UserIcon} size={Math.round(size * 0.7)} /></span
>

<style>
  .account-badge {
    display: inline-grid;
    flex: none;
    place-items: center;
    inline-size: var(--badge-size);
    block-size: var(--badge-size);
    border-radius: 50%;
    background: var(--color-lit-soft);
    color: var(--color-text);
  }
</style>
