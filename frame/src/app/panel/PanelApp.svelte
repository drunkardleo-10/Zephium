<script lang="ts">
  import { installCloseService } from "$shared/lib/close";
  import LazyView from "$shared/ui/LazyView";
  import RenderBoundary from "$shared/ui/RenderBoundary";
  import { loadToolSlot } from "$features/tools";
  import "@fontsource-variable/inter";
  import "$styles/panel.css";
  import { onMount, flushSync } from "svelte";
  import type { PanelState, PanelIntent, ToolKind } from "$shared/ipc/bindings";
  import { commands } from "$shared/ipc/bindings";
  import { events } from "$shared/ipc/native-events";
  import { theme } from "$domain/appearance";
  import { acceptPanelState } from "$features/panel";
  import { tools as toolManifest, toolKinds } from "$features/tools";
  import { loadLauncherPanel } from "$features/search";
  import * as m from "$shared/i18n/messages";
  const destinations = toolKinds.map((kind) => ({
    kind,
    label: toolManifest[kind].title(),
    icon: toolManifest[kind].icon,
  }));
  let presentation = $state<PanelState | null>(null);
  let failed = $state(false);
  function apply(next: PanelState) {
    if (acceptPanelState(presentation, next))
      flushSync(() => {
        presentation = next;
        failed = false;
      });
  }
  async function intent(value: PanelIntent) {
    try {
      if (!(await commands.panelIntent(value))) failed = true;
    } catch {
      failed = true;
    }
  }
  async function drag() {
    try {
      if (!(await commands.panelDrag())) failed = true;
    } catch {
      failed = true;
    }
  }
  function keydown(event: KeyboardEvent) {
    if (event.defaultPrevented || event.key !== "Escape") return;
    event.preventDefault();
    void intent({ type: presentation?.route.type === "tool" ? "back" : "dismiss" });
  }
  onMount(installCloseService);
  onMount(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    let stopMotion: (() => void) | undefined;
    let liveMotion = false;
    const motionListener = events.uiCommand.listen(({ payload }) => {
      if (disposed) return;
      if (
        payload === "preference.ui.reduce-motion=true" ||
        payload === "preference.ui.reduce-motion=false"
      ) {
        liveMotion = true;
        theme.setReducedMotion(payload.endsWith("=true"));
      }
    });
    const listener = events.panelState.listen((event) => {
      if (!disposed) apply(event.payload);
    });
    void (async () => {
      try {
        stop = await listener;
        stopMotion = await motionListener;
        await theme.init();
        if (disposed) return;
        const motion = await commands.settingGet("ui.reduce-motion").catch(() => null);
        if (disposed) return;
        if (!liveMotion) theme.setReducedMotion(motion === "true");
        void document.documentElement.getBoundingClientRect();
        const initial = await commands.panelReady();
        if (!disposed) {
          if (initial) apply(initial);
          else failed = true;
        }
      } catch {
        if (!disposed) failed = true;
      }
    })();
    return () => {
      disposed = true;
      stop?.();
      stopMotion?.();
      theme.dispose();
    };
  });
  function tool(kind: ToolKind) {
    void intent({ type: "tool", tool: kind });
  }
</script>

<svelte:window onkeydown={keydown} />
<div
  class="panel-root"
  style:--panel-radius={`${presentation?.corner_radius ?? 20}px`}
  data-visible={presentation?.visible ?? false}
>
  {#if presentation?.visible}
    {#if failed}<div class="panel-error" role="alert">{m.panel_action_failed()}</div>{/if}
    <RenderBoundary title={m.surface_render_failed()} retryLabel={m.surface_retry()}>
      {#if presentation.route.type === "search"}{#key presentation.session_id}{@const owner =
            presentation}<LazyView
            loader={loadLauncherPanel}
            loadingLabel={m.surface_loading()}
            failureLabel={m.surface_render_failed()}
            retryLabel={m.surface_retry()}
            >{#snippet children(Launcher)}<Launcher
                context={owner}
                {destinations}
                onTool={tool}
                onDrag={() => void drag()}
              />{/snippet}</LazyView
          >{/key}
      {:else}{@const tool = presentation.route.tool}{@const owner = presentation}<LazyView
          loader={loadToolSlot}
          loadingLabel={m.surface_loading()}
          failureLabel={m.surface_render_failed()}
          retryLabel={m.surface_retry()}
          >{#snippet children(View)}<View
              {tool}
              profile={owner.profile_id ?? "unbound"}
              profileName={owner.profile_name ?? undefined}
              host="floating"
              onclose={() => void intent({ type: "dismiss" })}
              onback={() => void intent({ type: "back" })}
              ondrag={() => void drag()}
            />{/snippet}</LazyView
        >{/if}
    </RenderBoundary>
  {/if}
</div>
