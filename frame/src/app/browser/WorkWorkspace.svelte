<script lang="ts">
  import { loadWorkCanvas } from "$features/work";
  import LazyView from "$shared/ui/LazyView";
  import * as m from "$shared/i18n/messages";
  // Native presentation host only. The runtime integration owns which entities,
  // actions and product controls belong here; this host loads no resource stores.
</script>

<section class="workspace" aria-label={m.mode_work()}>
  <LazyView
    loader={loadWorkCanvas}
    loadingLabel={m.surface_loading()}
    failureLabel={m.surface_render_failed()}
    retryLabel={m.surface_retry()}
  >
    {#snippet children(Canvas)}<Canvas items={[]} links={[]} oninspect={() => {}} />{/snippet}
  </LazyView>
</section>

<style>
  .workspace {
    height: 100%;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    border-radius: var(--content-radius);
    background: var(--color-canvas);
  }
</style>
