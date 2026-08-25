(() => {
  "use strict";
  const installed = Symbol.for("zephium.webkit-notifications-compatibility.v1");
  if (globalThis[installed] === true) return;

  const inertEvent = () =>
    Object.freeze({
      addListener() {},
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
  const facade = Object.freeze({
    onButtonClicked: inertEvent(),
    onClicked: inertEvent(),
    onClosed: inertEvent(),
    onPermissionLevelChanged: inertEvent(),
    onShown: inertEvent(),
    create(...args) {
      const requestedId = typeof args[0] === "string" ? args[0] : "";
      return settle(args, requestedId);
    },
    update(...args) {
      return settle(args, false);
    },
    clear(...args) {
      return settle(args, false);
    },
    getAll(...args) {
      return settle(args, Object.freeze({}));
    },
    getPermissionLevel(...args) {
      return settle(args, "denied");
    },
  });

  const eventMembers = new Set([
    "onButtonClicked",
    "onClicked",
    "onClosed",
    "onPermissionLevelChanged",
    "onShown",
  ]);
  const installMissingMembers = (notifications) => {
    for (const [name, fallback] of Object.entries(facade)) {
      const current = notifications[name];
      if (current != null) {
        const valid = eventMembers.has(name)
          ? typeof current.addListener === "function" &&
            typeof current.removeListener === "function"
          : typeof current === "function";
        if (!valid) {
          throw new Error(`Zephium notifications compatibility found invalid native ${name}`);
        }
        continue;
      }
      try {
        Object.defineProperty(notifications, name, {
          value: fallback,
          writable: false,
          enumerable: false,
          configurable: false,
        });
      } catch (_) {
        throw new Error(`Zephium notifications compatibility could not install ${name}`);
      }
      if (notifications[name] !== fallback) {
        throw new Error(`Zephium notifications compatibility could not verify ${name}`);
      }
    }
  };
  const install = () => {
    const namespaces = [...new Set([globalThis.chrome, globalThis.browser])].filter(
      (namespace) => namespace?.runtime?.id != null,
    );
    if (namespaces.length === 0) {
      throw new Error("Zephium notifications compatibility has no native extension namespace");
    }
    const native = namespaces.find((namespace) => namespace.notifications != null)?.notifications;
    const notifications = native ?? facade;
    installMissingMembers(notifications);
    for (const namespace of namespaces) {
      const missing = namespace.notifications == null;
      if (missing) {
        try {
          Object.defineProperty(namespace, "notifications", {
            value: notifications,
            writable: false,
            enumerable: false,
            configurable: false,
          });
        } catch (_) {
          throw new Error("Zephium notifications compatibility could not install safely");
        }
      } else if (namespace.notifications !== notifications) {
        installMissingMembers(namespace.notifications);
      }
      if (namespace.notifications == null) {
        throw new Error("Zephium notifications compatibility could not reconcile namespaces");
      }
    }
  };

  install();
  // Extension polyfills can replace a configurable native namespace during
  // startup. Reconcile only at bounded event-loop frontiers; no timer or
  // observer remains resident after initial document/worker setup.
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
