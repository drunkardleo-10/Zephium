// This bootstrap runs only in Zephium's isolated named world. It carries no
// page data, commands or selectors; native frame attribution owns the lookup.
(() => {
  "use strict";
  if (window === top) return;
  const bytes = new Uint32Array(4);
  crypto.getRandomValues(bytes);
  const token = Array.from(bytes, n => n.toString(16).padStart(8, "0")).join("");
  const handler = window.webkit?.messageHandlers?.zephiumFrameStyleV1;
  if (!handler) return;
  const send = kind => { try { handler.postMessage(`${kind}:${token}`).catch(() => {}); } catch (_) {} };
  send("hello");
  document.addEventListener("DOMContentLoaded", () => send("hello"), { once: true });
  window.addEventListener("pagehide", event => { if (event.isTrusted) send("bye"); });
  window.addEventListener("pageshow", event => { if (event.isTrusted && event.persisted) send("hello"); });
  window.addEventListener("hashchange", event => { if (event.isTrusted) send("hello"); });
  window.addEventListener("popstate", event => { if (event.isTrusted) send("hello"); });
})();
