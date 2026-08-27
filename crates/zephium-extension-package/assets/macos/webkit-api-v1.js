(() => {
  "use strict";
  const installed = Symbol.for("zephium.webkit-api-compatibility.v1");
  const modeMarker = Symbol.for("zephium.webkit-api-compatibility.mode.v1");
  const schedulerYieldMarker = Symbol.for("zephium.webkit-scheduler-yield.v1");
  const maxPendingYields = 128;
  if (globalThis[installed] === true) return;

  const installSchedulerYield = () => {
    if (typeof globalThis.scheduler?.yield === "function") return "native-preserved";
    let queue = null;
    const closeQueue = () => {
      if (queue == null) return;
      queue.channel.port1.onmessage = null;
      queue.channel.port1.close();
      queue.channel.port2.close();
      queue = null;
    };
    const yieldToBrowser = () => {
      if (queue?.count === maxPendingYields) {
        return Promise.reject(
          new DOMException("Scheduler continuation capacity exceeded", "QuotaExceededError"),
        );
      }
      return new Promise((resolve, reject) => {
        if (queue == null) {
          const channel = new MessageChannel();
          queue = {
            channel,
            resolvers: new Array(maxPendingYields),
            head: 0,
            tail: 0,
            count: 0,
          };
          channel.port1.onmessage = () => {
            const current = queue;
            if (current == null || current.count === 0) return;
            const continuation = current.resolvers[current.head];
            current.resolvers[current.head] = undefined;
            current.head = (current.head + 1) % maxPendingYields;
            current.count -= 1;
            if (current.count === 0) closeQueue();
            continuation();
          };
          channel.port1.start();
        }
        const current = queue;
        current.resolvers[current.tail] = resolve;
        current.tail = (current.tail + 1) % maxPendingYields;
        current.count += 1;
        try {
          current.channel.port2.postMessage(null);
        } catch (error) {
          current.tail = (current.tail + maxPendingYields - 1) % maxPendingYields;
          current.resolvers[current.tail] = undefined;
          current.count -= 1;
          if (current.count === 0) closeQueue();
          reject(error);
        }
      });
    };
    let scheduler = globalThis.scheduler;
    if (scheduler == null) {
      scheduler = {};
      try {
        Object.defineProperty(globalThis, "scheduler", {
          value: scheduler,
          writable: false,
          enumerable: false,
          configurable: false,
        });
      } catch (_) {
        return "unavailable";
      }
    }
    try {
      Object.defineProperty(scheduler, "yield", {
        value: yieldToBrowser,
        writable: false,
        enumerable: false,
        configurable: false,
      });
      Object.defineProperty(globalThis, schedulerYieldMarker, {
        value: "message-channel-bounded",
        writable: false,
        enumerable: false,
        configurable: false,
      });
    } catch (_) {
      return "unavailable";
    }
    return scheduler.yield === yieldToBrowser ? "message-channel-bounded" : "unavailable";
  };

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

  const ensureCatalogUpdateEvent = (runtime) => {
    if (runtime?.onUpdateAvailable != null) return true;
    try {
      Object.defineProperty(runtime, "onUpdateAvailable", {
        value: inertCatalogUpdateEvent,
        writable: false,
        enumerable: false,
        configurable: false,
      });
    } catch (_) {
      return false;
    }
    return runtime.onUpdateAvailable === inertCatalogUpdateEvent;
  };

  const nativeChrome = globalThis.chrome;
  const nativeBrowser = globalThis.browser;
  const install = (name, nativeNamespace) => {
    if (nativeNamespace?.runtime?.id == null) return "unavailable";
    let aliased = false;
    if (globalThis[name] == null) {
      try {
        Object.defineProperty(globalThis, name, {
          value: nativeNamespace,
          writable: false,
          enumerable: false,
          configurable: false,
        });
      } catch (_) {
        return "unavailable";
      }
      aliased = true;
    }
    if (globalThis[name] !== nativeNamespace) return "unavailable";
    const descriptor = Reflect.getOwnPropertyDescriptor(globalThis, name);
    try {
      if (descriptor?.configurable || ("value" in descriptor && descriptor.writable)) {
        Object.defineProperty(globalThis, name, {
          value: nativeNamespace,
          writable: false,
          enumerable: descriptor.enumerable,
          configurable: false,
        });
      }
    } catch (_) {
      return "unavailable";
    }
    const settled = Reflect.getOwnPropertyDescriptor(globalThis, name);
    const stable =
      settled != null &&
      !settled.configurable &&
      (("value" in settled && !settled.writable && settled.value === nativeNamespace) ||
        (!("value" in settled) && settled.set === undefined && globalThis[name] === nativeNamespace));
    if (!stable) return "unavailable";
    if (!ensureCatalogUpdateEvent(nativeNamespace.runtime)) return "unavailable";
    return aliased ? "native-aliased" : "native-preserved";
  };
  const chromeMode = install("chrome", nativeChrome ?? nativeBrowser);
  const browserMode = install("browser", nativeBrowser ?? nativeChrome);
  if (chromeMode === "unavailable" || browserMode === "unavailable") {
    throw new Error("Zephium WebKit extension API surface is unavailable");
  }
  const mode =
    chromeMode === "native-preserved" && browserMode === "native-preserved"
      ? "native-preserved"
      : "native-aliased";
  const schedulerYieldMode = installSchedulerYield();
  if (schedulerYieldMode === "unavailable") {
    throw new Error("Zephium WebKit scheduler continuation surface is unavailable");
  }
  Object.defineProperty(globalThis, modeMarker, {
    value: mode,
    writable: false,
    enumerable: false,
    configurable: false,
  });
  Object.defineProperty(globalThis, installed, {
    value: true,
    writable: false,
    enumerable: false,
    configurable: false,
  });
})();
