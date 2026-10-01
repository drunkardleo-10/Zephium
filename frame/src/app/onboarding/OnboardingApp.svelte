<script lang="ts">
  import { installCloseService } from "$shared/lib/close";
  import "$styles/global.css";
  import { onMount, tick } from "svelte";
  import { commands } from "$shared/ipc/bindings";
  import { IS_MAC } from "$shared/platform";
  import { theme } from "$domain/appearance";
  import { operations } from "$domain/operations";
  import { preferences } from "$domain/preferences";
  import { tabs } from "$domain/tabs";
  import * as motion from "$session/motion.svelte";
  import { Onboarding } from "$features/onboarding";
  import WindowControls from "$shared/ui/WindowControls";

  $effect(() => {
    theme.applyUiPreferences(
      preferences.value("ui.accent"),
      preferences.value("ui.reduce-motion") === "true",
    );
  });

  // Drawn once what it shows has arrived, and before native reveals the
  // window, so the first frame is already the welcome.
  let ready = $state(false);
  // Once native has taken the window for the browser, nothing of onboarding
  // stays running while its page is replaced.
  let finished = $state(false);

  async function finish(): Promise<boolean> {
    const opened = await commands.onboardingFinish().catch(() => false);
    if (opened) {
      finished = true;
      await tick();
    }
    return opened;
  }

  onMount(installCloseService);
  onMount(() => {
    let disposed = false;
    const themeReady = theme.init();
    const preferencesReady = preferences.init();
    void operations.init();
    // Bootstraps the shell behind this page, so a kept site or a name lands
    // in the session the browser opens with.
    const tabsReady = tabs.init();

    void (async () => {
      try {
        await Promise.all([themeReady, preferencesReady, tabsReady]);
        if (disposed) return;
        ready = true;
        await tick();
        // A hidden native window can suspend animation frames indefinitely.
        // Resolve style/layout synchronously before admitting native reveal.
        void document.documentElement.getBoundingClientRect();
        void getComputedStyle(document.body).backgroundColor;
        if (!(await commands.uiReady())) {
          throw new Error("native startup gate rejected UI");
        }
        motion.reveal();
      } catch {
        console.error("trusted onboarding initialization failed");
      }
    })();

    return () => {
      disposed = true;
      theme.dispose();
      motion.dispose();
      preferences.dispose();
      operations.dispose();
      tabs.dispose();
    };
  });
</script>

{#if ready && !finished}
  <Onboarding onfinish={finish}
    >{#snippet windowControls()}{#if !IS_MAC}<WindowControls />{/if}{/snippet}</Onboarding
  >
{/if}
