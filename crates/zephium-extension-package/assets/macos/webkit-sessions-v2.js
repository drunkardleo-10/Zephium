(() => {
  "use strict";

  const installed = Symbol.for("zephium.webkit-sessions-compatibility.v2");
  const modeMarker = Symbol.for("zephium.webkit-sessions-compatibility.mode.v2");
  if (globalThis[installed] === true) return;

  const applicationIdentifier = "app.zephium.extension-broker.v1";
  const maxResults = 25;
  const sessionIdPattern = /^[0-7][0-9A-HJKMNP-TV-Z]{25}$/;
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

  const parseResponse = (encoded, maxBytes) => {
    if (typeof encoded !== "string" || encoded.length > maxBytes) {
      throw new Error("Zephium sessions broker returned an invalid response");
    }
    const response = JSON.parse(encoded);
    if (response == null || typeof response !== "object" || Array.isArray(response)) {
      throw new Error("Zephium sessions broker returned an invalid response");
    }
    return response;
  };
  const validTab = (value, listed) => {
    if (value == null || typeof value !== "object" || Array.isArray(value)) return false;
    const keys = Object.keys(value).sort();
    const expected = listed
      ? ["lastModified", "sessionId", "title", "url"]
      : ["lastModified", "title", "url"];
    return (
      keys.length === expected.length &&
      keys.every((key, index) => key === expected[index]) &&
      (!listed ||
        (typeof value.sessionId === "string" && sessionIdPattern.test(value.sessionId))) &&
      typeof value.url === "string" &&
      value.url.length <= 8192 &&
      typeof value.title === "string" &&
      value.title.length <= 1024 &&
      Number.isSafeInteger(value.lastModified) &&
      value.lastModified > 0
    );
  };
  const decodeList = (encoded) => {
    const response = parseResponse(encoded, 64 * 1024);
    const keys = Object.keys(response).sort();
    if (
      keys.length !== 2 ||
      keys[0] !== "items" ||
      keys[1] !== "v" ||
      response.v !== 2 ||
      !Array.isArray(response.items) ||
      response.items.length > maxResults ||
      !response.items.every((item) => validTab(item, true))
    ) {
      throw new Error("Zephium sessions broker returned an invalid list contract");
    }
    return response.items.map((item) => ({
      lastModified: item.lastModified,
      tab: { sessionId: item.sessionId, url: item.url, title: item.title },
    }));
  };
  const decodeRestore = (encoded) => {
    const response = parseResponse(encoded, 64 * 1024);
    const keys = Object.keys(response).sort();
    if (
      keys.length !== 2 ||
      keys[0] !== "restored" ||
      keys[1] !== "v" ||
      response.v !== 2 ||
      (response.restored !== null && !validTab(response.restored, false))
    ) {
      throw new Error("Zephium sessions broker returned an invalid restore contract");
    }
    const tab = response.restored;
    if (tab === null) throw new Error("No recently closed tab is available");
    return { lastModified: tab.lastModified, tab: { url: tab.url, title: tab.title } };
  };
  const dispatch = (operation, decode, callback) => {
    if (callback !== undefined) {
      runtime.sendNativeMessage(applicationIdentifier, operation, (encoded) => {
        if (runtime.lastError != null) {
          callback(null);
          return;
        }
        let value;
        try {
          value = decode(encoded);
        } catch {
          callback(null);
          return;
        }
        callback(value);
      });
      return undefined;
    }
    const promise = new Promise((resolve, reject) => {
      runtime.sendNativeMessage(applicationIdentifier, operation, (encoded) => {
        const nativeError = runtime.lastError;
        if (nativeError != null) {
          reject(new Error(String(nativeError.message ?? "Zephium sessions broker rejected request")));
          return;
        }
        try {
          resolve(decode(encoded));
        } catch (error) {
          reject(error instanceof Error ? error : new Error("Zephium sessions response failed"));
        }
      });
    });
    return promise;
  };
  const getRecentlyClosed = (filter, callback) => {
    if (typeof filter === "function" && callback === undefined) {
      callback = filter;
      filter = undefined;
    }
    if (callback !== undefined && typeof callback !== "function") {
      throw new TypeError("sessions.getRecentlyClosed callback must be a function");
    }
    if (filter !== undefined && (filter === null || typeof filter !== "object" || Array.isArray(filter))) {
      throw new TypeError("sessions.getRecentlyClosed filter must be an object");
    }
    if (filter !== undefined && Object.keys(filter).some((key) => key !== "maxResults")) {
      throw new TypeError("sessions.getRecentlyClosed filter is unsupported");
    }
    const limit = filter?.maxResults ?? maxResults;
    if (!Number.isInteger(limit) || limit < 1 || limit > maxResults) {
      throw new RangeError("sessions.getRecentlyClosed maxResults must be from 1 to 25");
    }
    return dispatch(`v2/sessions.recent/${limit}`, decodeList, callback);
  };
  const restore = (sessionId, callback) => {
    if (typeof sessionId === "function" && callback === undefined) {
      callback = sessionId;
      sessionId = undefined;
    }
    if (callback !== undefined && typeof callback !== "function") {
      throw new TypeError("sessions.restore callback must be a function");
    }
    if (sessionId !== undefined && sessionId !== null) {
      if (typeof sessionId !== "string" || !sessionIdPattern.test(sessionId)) {
        throw new TypeError("sessions.restore requires a canonical session ID");
      }
    }
    return dispatch(
      `v2/sessions.restore/${sessionId ?? "recent"}`,
      decodeRestore,
      callback,
    );
  };
  const facade = Object.freeze({ MAX_SESSION_RESULTS: maxResults, getRecentlyClosed, restore });
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
    value: "local-current-profile-space-closed-tabs-only",
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
