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

  const namespaces = [...new Set([globalThis.chrome, globalThis.browser])].filter(
    (namespace) => namespace?.runtime?.id != null,
  );
  if (namespaces.length === 0) {
    throw new Error("Zephium notifications compatibility has no native extension namespace");
  }
  const native = namespaces.find((namespace) => namespace.notifications != null)?.notifications;
  const notifications = native ?? facade;
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
    }
    if (missing && namespace.notifications !== notifications) {
      throw new Error("Zephium notifications compatibility found divergent native namespaces");
    }
  }
  Object.defineProperty(globalThis, installed, {
    value: true,
    writable: false,
    enumerable: false,
    configurable: false,
  });
})();
