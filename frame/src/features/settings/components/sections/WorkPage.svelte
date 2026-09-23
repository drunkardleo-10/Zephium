<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { tabs } from "$domain/tabs";
  import WorkDecisions from "../WorkDecisions.svelte";
  let profile = $derived(tabs.profile());
</script>

{#if profile && profile.kind !== "incognito"}<WorkDecisions profile={profile.id} />{:else}<p
    class="work-note"
  >
    {m.work_regular_profile()}
  </p>{/if}

<style>
  .work-note {
    margin: 0 16px;
    font-size: var(--text-label);
    line-height: 1.5;
    color: var(--color-muted);
  }
</style>
