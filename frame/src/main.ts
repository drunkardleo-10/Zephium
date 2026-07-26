import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import { flushSync, mount } from "svelte";
import App from "./app/App.svelte";
import "./styles/global.css";

const target = document.getElementById("root");
if (!(target instanceof HTMLElement)) {
  throw new Error("trusted UI root is unavailable");
}

mount(App, { target });

// Native presentation and startup checks are synchronous. Svelte 5 mount and
// onMount work are scheduled, so explicitly drain them before bootstrap exits.
flushSync();
