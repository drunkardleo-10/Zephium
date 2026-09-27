(() => {
  "use strict";

  const installed = Symbol.for("zephium.webkit-identity-compatibility.v1");
  if (globalThis[installed] === true) return;

  // Replaced by the compiler from the exact admitted manifest key. The native
  // broker derives the same ID from the published runtime's package key.
  const extensionId = "__ZEPHIUM_CHROMIUM_ID__";
  const applicationIdentifier = "app.zephium.extension-identity.v1";
  const maxUrlBytes = 8192;
  const defaultTimeoutMs = 30_000;
  const maxTimeoutMs = 60_000;
  const namespaces = [...new Set([globalThis.chrome, globalThis.browser])].filter(
    (namespace) => namespace?.runtime?.id != null,
  );
  const runtime = namespaces
    .map((namespace) => namespace.runtime)
    .find((candidate) => typeof candidate?.sendNativeMessage === "function");
  if (namespaces.length === 0 || runtime == null || !/^[a-p]{32}$/.test(extensionId)) {
    throw new Error("Zephium identity flow requires an authenticated native extension runtime");
  }

  const redirectHost = `${extensionId}.chromiumapp.org`;
  const getRedirectURL = (path) => {
    const suffix = path == null ? "/" : String(path);
    const url = `https://${redirectHost}${suffix.startsWith("/") ? suffix : `/${suffix}`}`;
    if (url.length > maxUrlBytes || /[\u0000-\u001f\u007f]/.test(url)) {
      throw new TypeError("identity.getRedirectURL path is invalid");
    }
    return url;
  };

  const decodeResponse = (value) => {
    if (typeof value !== "string" || value.length > maxUrlBytes) {
      throw new Error("Zephium identity flow returned an invalid redirect");
    }
    const url = new URL(value);
    if (
      url.protocol !== "https:" ||
      url.hostname !== redirectHost ||
      url.port !== "" ||
      url.username !== "" ||
      url.password !== ""
    ) {
      throw new Error("Zephium identity flow returned a foreign redirect");
    }
    return value;
  };

  const launchWebAuthFlow = (details, callback) => {
    if (callback !== undefined && typeof callback !== "function") {
      throw new TypeError("identity.launchWebAuthFlow callback must be a function");
    }
    if (details == null || typeof details !== "object" || Array.isArray(details)) {
      throw new TypeError("identity.launchWebAuthFlow details must be an object");
    }
    const keys = Object.keys(details);
    if (keys.some((key) => ![
      "url", "interactive", "abortOnLoadForNonInteractive", "timeoutMsForNonInteractive",
    ].includes(key))) {
      throw new TypeError("identity.launchWebAuthFlow has an unsupported option");
    }
    const {
      url,
      interactive = false,
      abortOnLoadForNonInteractive = true,
      timeoutMsForNonInteractive = defaultTimeoutMs,
    } = details;
    if (
      typeof url !== "string" ||
      url.length === 0 ||
      url.length > maxUrlBytes ||
      typeof interactive !== "boolean" ||
      typeof abortOnLoadForNonInteractive !== "boolean" ||
      !Number.isSafeInteger(timeoutMsForNonInteractive) ||
      timeoutMsForNonInteractive < 1 ||
      timeoutMsForNonInteractive > maxTimeoutMs
    ) {
      throw new TypeError("identity.launchWebAuthFlow details are invalid");
    }
    const request = JSON.stringify({
      v: 1,
      kind: "identity.launch",
      url,
      interactive,
      abortOnLoadForNonInteractive,
      timeoutMsForNonInteractive,
    });
    if (callback !== undefined) {
      runtime.sendNativeMessage(applicationIdentifier, request, (response) => {
        if (runtime.lastError != null) {
          callback(undefined);
          return;
        }
        try {
          callback(decodeResponse(response));
        } catch {
          callback(undefined);
        }
      });
      return undefined;
    }
    return new Promise((resolve, reject) => {
      runtime.sendNativeMessage(applicationIdentifier, request, (response) => {
        const nativeError = runtime.lastError;
        if (nativeError != null) {
          reject(new Error(String(nativeError.message ?? "Zephium identity flow was rejected")));
          return;
        }
        try {
          resolve(decodeResponse(response));
        } catch (error) {
          reject(error);
        }
      });
    });
  };

  const facade = Object.freeze({ getRedirectURL, launchWebAuthFlow });
  for (const namespace of namespaces) {
    const descriptor = Reflect.getOwnPropertyDescriptor(namespace, "identity");
    if (
      (descriptor != null && descriptor.configurable !== true) ||
      (descriptor == null && !Object.isExtensible(namespace))
    ) {
      throw new Error("Zephium identity flow cannot install safely");
    }
  }
  for (const namespace of namespaces) {
    Object.defineProperty(namespace, "identity", {
      value: facade,
      writable: false,
      enumerable: true,
      configurable: false,
    });
  }
  Object.defineProperty(globalThis, installed, {
    value: true,
    writable: false,
    enumerable: false,
    configurable: false,
  });
})();
