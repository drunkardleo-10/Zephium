(() => {
  "use strict";

  const installed = Symbol.for("zephium.webkit-managed-storage.v1");
  if (globalThis[installed] === true) return;

  const inertEvent = Object.freeze({
    addListener(listener) {
      if (typeof listener !== "function") throw new TypeError("listener must be a function");
    },
    removeListener() {},
    hasListener() {
      return false;
    },
    hasListeners() {
      return false;
    },
  });
  const settle = (args, value) => {
    const callback = args[args.length - 1];
    if (typeof callback === "function") {
      queueMicrotask(() => {
        try {
          callback(value);
        } catch (_) {}
      });
    }
    return Promise.resolve(value);
  };
  const empty = Object.freeze({});
  const facade = Object.freeze({
    onChanged: inertEvent,
    get(...args) {
      return settle(args, empty);
    },
  });

  const install = () => {
    const namespaces = [...new Set([globalThis.chrome, globalThis.browser])].filter(
      (namespace) => namespace?.runtime?.id != null && namespace?.storage != null,
    );
    if (namespaces.length === 0) {
      throw new Error("Zephium managed-storage compatibility has no native storage namespace");
    }
    const native = namespaces.find((namespace) => namespace.storage.managed != null)?.storage
      .managed;
    const managed = native ?? facade;
    for (const namespace of namespaces) {
      const missing = namespace.storage.managed == null;
      if (missing) {
        try {
          Object.defineProperty(namespace.storage, "managed", {
            value: managed,
            writable: false,
            enumerable: false,
            configurable: false,
          });
        } catch (_) {
          throw new Error("Zephium managed-storage compatibility could not install safely");
        }
      }
      if (missing && namespace.storage.managed !== managed) {
        throw new Error(
          "Zephium managed-storage compatibility found divergent native namespaces",
        );
      }
    }
  };

  install();
  // Some extension polyfills replace nested namespace objects during their
  // own module setup. Reconcile at bounded event-loop frontiers; every task is
  // one-shot and no polling loop or proxy remains resident.
  queueMicrotask(install);
  setTimeout(install, 0);
  if (typeof document !== "undefined" && document.readyState === "loading") {
    addEventListener("DOMContentLoaded", install, { once: true });
    addEventListener("load", () => setTimeout(install, 0), { once: true });
  }

  Object.defineProperty(globalThis, installed, {
    value: true,
    writable: false,
    enumerable: false,
    configurable: false,
  });
})();
