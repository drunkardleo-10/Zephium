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
  const inertGet = (...args) => settle(args, empty);
  const facade = Object.freeze({
    onChanged: inertEvent,
    get: inertGet,
  });

  const validEvent = (event) =>
    event != null &&
    typeof event.addListener === "function" &&
    typeof event.removeListener === "function";
  const completeManaged = (candidate) => {
    const managed = candidate ?? facade;
    if ((typeof managed !== "object" && typeof managed !== "function") || managed == null) {
      throw new Error("Zephium managed-storage compatibility found an invalid native namespace");
    }
    if (managed.onChanged == null) {
      try {
        Object.defineProperty(managed, "onChanged", {
          value: inertEvent,
          writable: false,
          enumerable: false,
          configurable: false,
        });
      } catch (_) {
        throw new Error(
          "Zephium managed-storage compatibility could not complete the native event surface",
        );
      }
    }
    if (managed.get == null) {
      try {
        Object.defineProperty(managed, "get", {
          value: inertGet,
          writable: false,
          enumerable: false,
          configurable: false,
        });
      } catch (_) {
        throw new Error(
          "Zephium managed-storage compatibility could not complete the native read surface",
        );
      }
    }
    if (!validEvent(managed.onChanged) || typeof managed.get !== "function") {
      throw new Error("Zephium managed-storage compatibility found an invalid managed surface");
    }
    return managed;
  };

  const install = () => {
    const namespaces = [...new Set([globalThis.chrome, globalThis.browser])].filter(
      (namespace) => namespace?.runtime?.id != null && namespace?.storage != null,
    );
    if (namespaces.length === 0) {
      throw new Error("Zephium managed-storage compatibility has no native storage namespace");
    }
    const native = namespaces
      .map((namespace) => namespace.storage.managed)
      .filter((managed) => managed != null);
    if (native.some((managed) => managed !== native[0])) {
      throw new Error("Zephium managed-storage compatibility found divergent native namespaces");
    }
    const managed = completeManaged(native[0]);
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
      if (namespace.storage.managed !== managed) {
        throw new Error(
          "Zephium managed-storage compatibility found divergent native namespaces",
        );
      }
    }
  };

  // Extension polyfills can replace a nested namespace while their module
  // graph is evaluating. Reconcile across a small fixed cohort of microtask
  // and task frontiers so a replacement made after this prelude cannot erase
  // the facade. This is startup-only bounded work, not a poller or keepalive.
  const reconciliationFrontiers = 4;
  const scheduleReconciliation = (schedule, remaining) => {
    if (remaining === 0) return;
    schedule(() => {
      install();
      scheduleReconciliation(schedule, remaining - 1);
    });
  };

  install();
  scheduleReconciliation(queueMicrotask, reconciliationFrontiers);
  scheduleReconciliation((callback) => setTimeout(callback, 0), reconciliationFrontiers);
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
