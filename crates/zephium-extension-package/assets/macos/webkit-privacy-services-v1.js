(() => {
  "use strict";

  const installed = Symbol.for("zephium.webkit-privacy-services.v1");
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
  const validateDetails = (details, operation) => {
    if (details === null || typeof details !== "object" || Array.isArray(details)) {
      throw new TypeError(`privacy setting ${operation} details must be an object`);
    }
  };
  const settle = (args, value) => {
    const callback = args[args.length - 1];
    if (typeof callback === "function") {
      queueMicrotask(() => {
        try {
          callback(value);
        } catch (_) {}
      });
      return undefined;
    }
    return Promise.resolve(value);
  };
  const createDisabledSetting = () => {
    let controlled = false;
    return Object.freeze({
      onChange: inertEvent,
      get(...args) {
        validateDetails(args[0] ?? {}, "get");
        return settle(
          args,
          Object.freeze({
            value: false,
            levelOfControl: controlled
              ? "controlled_by_this_extension"
              : "controllable_by_this_extension",
          }),
        );
      },
      set(...args) {
        const details = args[0];
        validateDetails(details, "set");
        if (details.value !== false) {
          throw new DOMException(
            "Zephium has no browser password or address autofill service to enable",
            "NotSupportedError",
          );
        }
        controlled = true;
        return settle(args, undefined);
      },
      clear(...args) {
        validateDetails(args[0] ?? {}, "clear");
        controlled = false;
        return settle(args, undefined);
      },
    });
  };

  const fallbackServices = Object.freeze({
    autofillEnabled: createDisabledSetting(),
    autofillAddressEnabled: createDisabledSetting(),
    autofillCreditCardEnabled: createDisabledSetting(),
    passwordSavingEnabled: createDisabledSetting(),
  });
  const fallbackPrivacy = Object.freeze({ services: fallbackServices });
  const requiredSettings = Object.keys(fallbackServices);
  const validSetting = (setting) =>
    setting != null &&
    typeof setting.get === "function" &&
    typeof setting.set === "function" &&
    typeof setting.clear === "function";

  const installMissingSettings = (services) => {
    for (const name of requiredSettings) {
      const current = services[name];
      if (current != null) {
        if (!validSetting(current)) {
          throw new Error(`Zephium privacy compatibility found invalid native ${name}`);
        }
        continue;
      }
      const fallback = fallbackServices[name];
      try {
        Object.defineProperty(services, name, {
          value: fallback,
          writable: false,
          enumerable: false,
          configurable: false,
        });
      } catch (_) {
        throw new Error(`Zephium privacy compatibility could not install ${name}`);
      }
      if (services[name] !== fallback) {
        throw new Error(`Zephium privacy compatibility could not verify ${name}`);
      }
    }
  };

  const installServices = (privacy) => {
    if (privacy.services == null) {
      try {
        Object.defineProperty(privacy, "services", {
          value: fallbackServices,
          writable: false,
          enumerable: false,
          configurable: false,
        });
      } catch (_) {
        throw new Error("Zephium privacy compatibility could not install services");
      }
    } else {
      installMissingSettings(privacy.services);
    }
    if (privacy.services == null) {
      throw new Error("Zephium privacy compatibility could not verify services");
    }
  };

  const install = () => {
    const namespaces = [...new Set([globalThis.chrome, globalThis.browser])].filter(
      (namespace) => namespace?.runtime?.id != null,
    );
    if (namespaces.length === 0) {
      throw new Error("Zephium privacy compatibility has no native extension namespace");
    }
    const nativePrivacy = namespaces.find((namespace) => namespace.privacy != null)?.privacy;
    const privacy = nativePrivacy ?? fallbackPrivacy;
    if (privacy !== fallbackPrivacy) installServices(privacy);
    for (const namespace of namespaces) {
      if (namespace.privacy == null) {
        try {
          Object.defineProperty(namespace, "privacy", {
            value: privacy,
            writable: false,
            enumerable: false,
            configurable: false,
          });
        } catch (_) {
          throw new Error("Zephium privacy compatibility could not install safely");
        }
      } else if (namespace.privacy !== privacy) {
        installServices(namespace.privacy);
      }
      if (namespace.privacy?.services == null) {
        throw new Error("Zephium privacy compatibility found divergent native namespaces");
      }
    }
  };

  install();
  // Extension polyfills can replace configurable nested namespaces during
  // startup. Reconcile at bounded initial event-loop frontiers only.
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
