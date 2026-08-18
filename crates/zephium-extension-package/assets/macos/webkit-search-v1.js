(() => {
  "use strict";

  const installed = Symbol.for("zephium.webkit-search-compatibility.v1");
  const modeMarker = Symbol.for("zephium.webkit-search-compatibility.mode.v1");
  if (globalThis[installed] === true) return;

  const applicationIdentifier = "app.zephium.extension-broker.v1";
  const maxQueryBytes = 1024;
  const namespaces = [...new Set([globalThis.chrome, globalThis.browser])].filter(
    (namespace) => namespace?.runtime?.id != null,
  );
  if (namespaces.length === 0) {
    throw new Error("Zephium WebKit search compatibility requires a native extension namespace");
  }
  const runtime = namespaces
    .map((namespace) => namespace.runtime)
    .find((candidate) => typeof candidate?.sendNativeMessage === "function");
  if (runtime == null || typeof globalThis.TextEncoder !== "function") {
    throw new Error("Zephium WebKit search compatibility broker is unavailable");
  }
  const exactKeys = (value, admitted) => {
    const keys = Object.keys(value).sort();
    return keys.every((key) => admitted.includes(key));
  };
  const encode = (value) => {
    const bytes = new TextEncoder().encode(value);
    if (bytes.length === 0 || bytes.length > maxQueryBytes) {
      throw new RangeError("search.query text exceeds the compatibility ceiling");
    }
    let binary = "";
    for (const byte of bytes) binary += String.fromCharCode(byte);
    return btoa(binary).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/u, "");
  };
  const normalize = (details) => {
    if (
      details == null ||
      typeof details !== "object" ||
      Array.isArray(details) ||
      !exactKeys(details, ["disposition", "tabId", "text"]) ||
      typeof details.text !== "string"
    ) {
      throw new TypeError("search.query details are invalid");
    }
    if (
      details.tabId !== undefined &&
      (!Number.isSafeInteger(details.tabId) || details.tabId < 0)
    ) {
      throw new TypeError("search.query tabId is invalid");
    }
    const disposition = details.tabId !== undefined ? "CURRENT_TAB" : details.disposition;
    if (disposition !== undefined && disposition !== "CURRENT_TAB" && disposition !== "NEW_TAB") {
      throw new TypeError("search.query disposition is unsupported");
    }
    return Object.freeze({
      encoded: encode(details.text),
      disposition: disposition === "NEW_TAB" ? "new" : "current",
    });
  };
  const decode = (encoded) => {
    if (typeof encoded !== "string" || encoded.length > 64) {
      throw new Error("Zephium search broker returned an invalid response");
    }
    const response = JSON.parse(encoded);
    const keys = Object.keys(response).sort();
    if (
      keys.length !== 2 ||
      keys[0] !== "opened" ||
      keys[1] !== "v" ||
      response.v !== 1 ||
      typeof response.opened !== "boolean"
    ) {
      throw new Error("Zephium search broker returned an invalid contract");
    }
    return response.opened;
  };
  const query = (details, callback) => {
    if (callback !== undefined && typeof callback !== "function") {
      throw new TypeError("search.query callback must be a function");
    }
    const normalized = normalize(details);
    const operation = `v1/search.default/${normalized.disposition}/${normalized.encoded}`;
    const promise = new Promise((resolve, reject) => {
      runtime.sendNativeMessage(applicationIdentifier, operation, (encoded) => {
        const nativeError = runtime.lastError;
        if (nativeError != null) {
          reject(new Error(String(nativeError.message ?? "Zephium search broker rejected request")));
          return;
        }
        try {
          if (!decode(encoded)) {
            reject(new Error("Zephium could not open the browser search"));
            return;
          }
          resolve(undefined);
        } catch (error) {
          reject(error instanceof Error ? error : new Error("Zephium search response failed"));
        }
      });
    });
    if (callback !== undefined) {
      promise.then(
        () => callback(),
        () => callback(),
      );
      return undefined;
    }
    return promise;
  };
  const facade = Object.freeze({ query });
  for (const namespace of namespaces) {
    const descriptor = Reflect.getOwnPropertyDescriptor(namespace, "search");
    if (
      (descriptor != null && descriptor.configurable !== true) ||
      (descriptor == null && !Object.isExtensible(namespace))
    ) {
      throw new Error("Zephium WebKit search compatibility cannot install safely");
    }
  }
  for (const namespace of namespaces) {
    Object.defineProperty(namespace, "search", {
      value: facade,
      writable: false,
      enumerable: true,
      configurable: false,
    });
  }
  Object.defineProperty(globalThis, modeMarker, {
    value: "browser-default-current-or-new-tab",
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
