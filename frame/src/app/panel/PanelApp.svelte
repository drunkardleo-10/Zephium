<script lang="ts">
  import { installCloseService } from "$shared/lib/close";
  import RenderBoundary from "$shared/ui/RenderBoundary";
  import "$styles/panel.css";
  import { onMount, flushSync } from "svelte";
  import type { PanelState, PanelIntent, ToolKind } from "$shared/ipc/bindings";
  import { commands } from "$shared/ipc/bindings";
  import { events } from "$shared/ipc/native-events";
  import { theme } from "$domain/appearance";
  import { favicons } from "$domain/favicons";
  import { acceptPanelState } from "$features/panel";
  import { loadLauncherPanel } from "$features/search";
  import { loadCapture } from "$features/tasks";
  import * as m from "$shared/i18n/messages";
  import { applyLanguage } from "$shared/lib/locale.svelte";

  let presentation = $state<PanelState | null>(null);
  let Launcher = $state<Awaited<ReturnType<typeof loadLauncherPanel>>["default"] | null>(null);
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
  onMount(installCloseService);
  onMount(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    let stopMotion: (() => void) | undefined;
    let liveMotion = false;
    let liveLanguage = false;
    const motionListener = events.uiCommand.listen(({ payload }) => {
      if (disposed) return;
      if (payload.startsWith("preference.ui.language=")) {
        liveLanguage = true;
        applyLanguage(payload.slice("preference.ui.language=".length));
      }
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

    // Result rasters arrive just ahead of the results that reference them.
    const faviconsReady = favicons.init();
    void (async () => {
      try {
        stop = await listener;
        stopMotion = await motionListener;
        // The launcher is loaded while the window is still hidden, and native
        // shows nothing before `panelReady`, so the first frame is never empty.
        const [launcher] = await Promise.all([loadLauncherPanel(), theme.init(), faviconsReady]);
        if (disposed) return;
        Launcher = launcher.default;
        const [motion, language] = await Promise.all([
          commands.settingGet("ui.reduce-motion").catch(() => null),
          commands.settingGet("ui.language").catch(() => null),
        ]);
        if (disposed) return;
        if (!liveMotion) theme.setReducedMotion(motion === "true");
        if (!liveLanguage) applyLanguage(language);
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
      favicons.dispose();
      theme.dispose();
    };
  });
  function open(kind: ToolKind) {
    void intent({ type: "open", tool: kind });
  }
  /** Long enough to read that it landed, short enough not to wait on. */
  const CAPTURED_MS = 700;
  async function capture(text: string): Promise<string | null> {
    const profile = presentation?.profile_id;
    if (!profile) return null;
    const { captureTask } = await loadCapture();
    const title = await captureTask(profile, text);
    if (title) setTimeout(() => void intent({ type: "dismiss" }), CAPTURED_MS);
    return title;
  }
</script>

<div
  class="panel-root"
  style:--panel-radius={`${presentation?.corner_radius ?? 20}px`}
  data-visible={presentation?.visible ?? false}
>
  {#if failed}<div class="panel-error" role="alert">{m.panel_action_failed()}</div>{/if}
  <RenderBoundary title={m.surface_render_failed()} retryLabel={m.surface_retry()}>
    {#if Launcher}<Launcher
        context={presentation}
        onTool={open}
        onCapture={capture}
        onDismiss={() => void intent({ type: "dismiss" })}
      />{/if}
  </RenderBoundary>
</div>
