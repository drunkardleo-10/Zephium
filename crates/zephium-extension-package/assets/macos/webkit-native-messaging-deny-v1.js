(() => {
  "use strict";

  const installed = Symbol.for("zephium.webkit-native-messaging-deny.v1");
  if (globalThis[installed] === true) return;

  const namespaces = [...new Set([globalThis.chrome, globalThis.browser])].filter(
    (namespace) => namespace?.runtime?.id != null,
  );
  if (namespaces.length === 0) {
    throw new Error("Zephium native-messaging denial facade has no extension runtime");
  }

  const MAX_LISTENERS = 64;
  const createEvent = () => {
    const listeners = new Set();
    const event = Object.freeze({
      addListener(listener) {
        if (typeof listener !== "function") throw new TypeError("listener must be a function");
        if (!listeners.has(listener) && listeners.size >= MAX_LISTENERS) {
          throw new Error("native-messaging denial listener capacity exceeded");
        }
        listeners.add(listener);
      },
      removeListener(listener) {
        listeners.delete(listener);
      },
      hasListener(listener) {
        return listeners.has(listener);
      },
      hasListeners() {
        return listeners.size !== 0;
      },
    });
    return {
      event,
      dispatch(port) {
        for (const listener of Array.from(listeners)) {
          try {
            listener(port);
          } catch (error) {
            console.error("Zephium native-messaging disconnect listener failed", error);
          }
        }
        listeners.clear();
      },
    };
  };

  const connectNative = (application) => {
    if (
      typeof application !== "string" ||
      application.length === 0 ||
      application.length > 256 ||
      !/^[A-Za-z0-9._-]+$/.test(application)
    ) {
      throw new TypeError("native messaging application identifier is invalid");
    }
    const onMessage = createEvent();
    const onDisconnect = createEvent();
    let disconnected = false;
    let port;
    const finishDisconnect = () => {
      if (disconnected) return;
      disconnected = true;
      onDisconnect.dispatch(port);
    };
    port = Object.freeze({
      name: application,
      onMessage: onMessage.event,
      onDisconnect: onDisconnect.event,
      postMessage() {
        if (disconnected) throw new Error("native messaging port is disconnected");
        // Match a native host lookup that has not settled yet. No message is
        // retained, serialized, or sent anywhere before the bounded denial.
      },
      disconnect: finishDisconnect,
    });
    queueMicrotask(finishDisconnect);
    return port;
  };

  const sendNativeMessage = (application, _message, callback = undefined) => {
    if (
      typeof application !== "string" ||
      application.length > 256 ||
      (application.length !== 0 && !/^[A-Za-z0-9._-]+$/.test(application))
    ) {
      throw new TypeError("native messaging application identifier is invalid");
    }
    const error = new Error("native messaging host is unavailable");
    if (callback !== undefined) {
      if (typeof callback !== "function") throw new TypeError("callback must be a function");
      queueMicrotask(() => callback(undefined));
      return undefined;
    }
    return Promise.reject(error);
  };

  for (const namespace of namespaces) {
    const runtime = namespace.runtime;
    if (runtime.connectNative == null) {
      Object.defineProperty(runtime, "connectNative", {
        value: connectNative,
        writable: false,
        enumerable: true,
        configurable: false,
      });
    }
    if (runtime.connectNative !== connectNative) {
      throw new Error("Zephium native-messaging denial facade was not installed");
    }
    if (runtime.sendNativeMessage == null) {
      Object.defineProperty(runtime, "sendNativeMessage", {
        value: sendNativeMessage,
        writable: false,
        enumerable: true,
        configurable: false,
      });
    }
    if (runtime.sendNativeMessage !== sendNativeMessage) {
      throw new Error("Zephium native-messaging denial facade was not installed");
    }
  }

  Object.defineProperty(globalThis, installed, {
    value: true,
    writable: false,
    enumerable: false,
    configurable: false,
  });
})();
