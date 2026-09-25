import { getCurrentWindow } from "@tauri-apps/api/window";
import { mount, flushSync } from "svelte";
import App from "$app/browser/BrowserApp.svelte";

try {
  if (getCurrentWindow().label !== "main") throw new Error("unexpected native surface");
  const target = document.getElementById("root");
  if (!(target instanceof HTMLElement)) throw new Error("trusted UI root is unavailable");
  mount(App, { target });
  // The native presentation barrier observes this DOM synchronously.
  flushSync();
} catch {
  console.error("trusted browser initialization failed");
}
