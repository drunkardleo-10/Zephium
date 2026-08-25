(() => {
  "use strict";

  const installed = Symbol.for("zephium.webkit-background-document.v1");
  if (globalThis[installed] === true) return;

  const api = globalThis.browser ?? globalThis.chrome;
  const runtime = api?.runtime;
  if (runtime?.id == null || typeof runtime.getURL !== "function") {
    throw new Error("Zephium WebKit background-document bridge has no extension runtime");
  }
  if (typeof document === "undefined") {
    Object.defineProperty(globalThis, installed, {
      value: true,
      writable: false,
      enumerable: false,
      configurable: false,
    });
    return;
  }

  let extensionOrigin;
  try {
    extensionOrigin = new URL(runtime.getURL("/")).origin;
  } catch (_) {
    throw new Error("Zephium WebKit background-document bridge has no extension origin");
  }
  if (extensionOrigin !== location.origin) {
    throw new Error("Zephium WebKit background-document bridge refused a page context");
  }

  if (globalThis.clients == null) {
    const clients = Object.freeze({
      matchAll(options = undefined) {
        if (options !== undefined && (options === null || typeof options !== "object")) {
          return Promise.reject(new TypeError("clients.matchAll options must be an object"));
        }
        // A document background has no ServiceWorker WindowClient inventory.
        // Zephium owns popup admission and enforces one process-wide popup
        // lease, so an empty immutable result cannot mint another surface.
        return Promise.resolve(Object.freeze([]));
      },
    });
    Object.defineProperty(globalThis, "clients", {
      value: clients,
      writable: false,
      enumerable: true,
      configurable: false,
    });
  }
  if (typeof globalThis.clients?.matchAll !== "function") {
    throw new Error("Zephium WebKit background-document clients facade was not installed");
  }

  Object.defineProperty(globalThis, installed, {
    value: true,
    writable: false,
    enumerable: false,
    configurable: false,
  });
})();
