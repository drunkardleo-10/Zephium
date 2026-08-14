(() => {
  "use strict";
  const installed = Symbol.for("zephium.webkit-api-compatibility.v1");
  if (globalThis[installed] === true) return;

  const inertCatalogUpdateEvent = Object.freeze({
    addListener() {},
    removeListener() {},
    hasListener() {
      return false;
    },
    hasListeners() {
      return false;
    },
  });
  const objectProxies = new WeakMap();
  const runtimeProxies = new WeakMap();
  const boundFunctions = new WeakMap();

  const bindFunction = (target, property, value) => {
    let properties = boundFunctions.get(target);
    if (properties === undefined) {
      properties = new Map();
      boundFunctions.set(target, properties);
    }
    const cached = properties.get(property);
    if (cached?.source !== value) {
      properties.set(property, { source: value, bound: value.bind(target) });
    }
    return properties.get(property).bound;
  };

  const wrap = (target, runtimeNamespace = false) => {
    if ((typeof target !== "object" && typeof target !== "function") || target === null) {
      return target;
    }
    const cache = runtimeNamespace ? runtimeProxies : objectProxies;
    const cached = cache.get(target);
    if (cached !== undefined) return cached;
    const proxy = new Proxy(target, {
      get(nativeTarget, property) {
        const fixed = Reflect.getOwnPropertyDescriptor(nativeTarget, property);
        if (fixed && !fixed.configurable && "value" in fixed && !fixed.writable) {
          return fixed.value;
        }
        if (fixed && !fixed.configurable && !("value" in fixed) && fixed.get === undefined) {
          return undefined;
        }
        if (
          runtimeNamespace &&
          property === "onUpdateAvailable" &&
          Reflect.get(nativeTarget, property, nativeTarget) == null
        ) {
          return inertCatalogUpdateEvent;
        }
        const value = Reflect.get(nativeTarget, property, nativeTarget);
        if (typeof value === "function") return bindFunction(nativeTarget, property, value);
        return wrap(value, property === "runtime");
      },
      set(nativeTarget, property, value) {
        const settled = Reflect.set(nativeTarget, property, value, nativeTarget);
        if (settled) boundFunctions.get(nativeTarget)?.delete(property);
        return settled;
      },
    });
    cache.set(target, proxy);
    return proxy;
  };

  const install = (name, nativeNamespace) => {
    if (nativeNamespace?.runtime?.id == null) return false;
    const compatible = wrap(nativeNamespace);
    const descriptor = Reflect.getOwnPropertyDescriptor(globalThis, name);
    try {
      if (descriptor === undefined || descriptor.configurable) {
        Object.defineProperty(globalThis, name, {
          value: compatible,
          writable: false,
          enumerable: descriptor?.enumerable ?? false,
          configurable: false,
        });
      } else {
        globalThis[name] = compatible;
      }
    } catch (_) {
      return false;
    }
    return globalThis[name] === compatible;
  };

  const nativeChrome = globalThis.chrome;
  const nativeBrowser = globalThis.browser;
  const chromeInstalled = install("chrome", nativeChrome ?? nativeBrowser);
  const browserInstalled = install("browser", nativeBrowser ?? nativeChrome);
  if (!chromeInstalled || !browserInstalled) {
    throw new Error("Zephium WebKit extension API surface is unavailable");
  }
  Object.defineProperty(globalThis, installed, {
    value: true,
    writable: false,
    enumerable: false,
    configurable: false,
  });
})();
