(() => {
  "use strict";

  const installed = Symbol.for("zephium.webkit-sessions-compatibility.v1");
  const modeMarker = Symbol.for("zephium.webkit-sessions-compatibility.mode.v1");
  if (globalThis[installed] === true) return;

  const applicationIdentifier = "app.zephium.extension-broker.v1";
  const operation = "v1/sessions.restore/recent";
  const namespaces = [...new Set([globalThis.chrome, globalThis.browser])].filter(
    (namespace) => namespace?.runtime?.id != null,
  );
  if (namespaces.length === 0) {
    throw new Error("Zephium WebKit sessions compatibility requires a native extension namespace");
  }
  const runtime = namespaces
    .map((namespace) => namespace.runtime)
    .find((candidate) => typeof candidate?.sendNativeMessage === "function");
  if (runtime == null) {
    throw new Error("Zephium WebKit sessions compatibility broker is unavailable");
  }
  const decode = (encoded) => {
    if (typeof encoded !== "string" || encoded.length > 64) {
      throw new Error("Zephium sessions broker returned an invalid response");
    }
    const response = JSON.parse(encoded);
    const keys = Object.keys(response).sort();
    if (
      keys.length !== 2 ||
      keys[0] !== "restored" ||
      keys[1] !== "v" ||
      response.v !== 1 ||
      typeof response.restored !== "boolean"
    ) {
      throw new Error("Zephium sessions broker returned an invalid contract");
    }
    return response.restored;
  };
  const restore = (sessionId, callback) => {
    if (sessionId !== undefined && sessionId !== null) {
      throw new TypeError("sessions.restore supports only the most recent tab");
    }
    if (callback !== undefined && typeof callback !== "function") {
      throw new TypeError("sessions.restore callback must be a function");
    }
    const promise = new Promise((resolve, reject) => {
      runtime.sendNativeMessage(applicationIdentifier, operation, (encoded) => {
        const nativeError = runtime.lastError;
        if (nativeError != null) {
          reject(
            new Error(String(nativeError.message ?? "Zephium sessions broker rejected request")),
          );
          return;
        }
        try {
          resolve(decode(encoded) ? undefined : null);
        } catch (error) {
          reject(error instanceof Error ? error : new Error("Zephium sessions response failed"));
        }
      });
    });
    if (callback !== undefined) {
      promise.then(
        (value) => callback(value),
        () => callback(null),
      );
      return undefined;
    }
    return promise;
  };
  const facade = Object.freeze({ MAX_SESSION_RESULTS: 1, restore });
  for (const namespace of namespaces) {
    const descriptor = Reflect.getOwnPropertyDescriptor(namespace, "sessions");
    if (
      (descriptor != null && descriptor.configurable !== true) ||
      (descriptor == null && !Object.isExtensible(namespace))
    ) {
      throw new Error("Zephium WebKit sessions compatibility cannot install safely");
    }
  }
  for (const namespace of namespaces) {
    Object.defineProperty(namespace, "sessions", {
      value: facade,
      writable: false,
      enumerable: true,
      configurable: false,
    });
  }
  Object.defineProperty(globalThis, modeMarker, {
    value: "recent-current-space-tab-only",
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
