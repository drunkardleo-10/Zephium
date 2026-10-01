import { getCurrentWindow } from "@tauri-apps/api/window";
import { mount } from "svelte";
import App from "$app/onboarding/OnboardingApp.svelte";

// A first run opens this page in the main window instead of the browser;
// native replaces it with the browser once it is finished.
try {
  if (getCurrentWindow().label !== "main") throw new Error("unexpected native surface");
  const target = document.getElementById("root");
  if (!(target instanceof HTMLElement)) throw new Error("trusted UI root is unavailable");
  mount(App, { target });
} catch {
  console.error("trusted onboarding initialization failed");
}
