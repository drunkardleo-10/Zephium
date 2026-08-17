(() => {
  "use strict";

  const installed = Symbol.for("zephium.webkit-history-compatibility.v1");
  const modeMarker = Symbol.for("zephium.webkit-history-compatibility.mode.v1");
  if (globalThis[installed] === true) return;

  const applicationIdentifier = "app.zephium.extension-broker.v1";
  const requestPrefix = "v1/history.recent/";
  const maxResults = 100;
  const maxResponseCodeUnits = 64 * 1024;
  const maxQueryCodeUnits = 4096;
  const maxListenersPerEvent = 64;
  const oneDayMilliseconds = 24 * 60 * 60 * 1000;

  const namespaces = [...new Set([globalThis.chrome, globalThis.browser])].filter(
    (namespace) => namespace?.runtime?.id != null,
  );
  if (namespaces.length === 0) {
    throw new Error("Zephium WebKit history compatibility requires a native extension namespace");
  }

  const runtime = namespaces
    .map((namespace) => namespace.runtime)
    .find((candidate) => typeof candidate?.sendNativeMessage === "function");
  if (runtime == null) {
    throw new Error("Zephium WebKit history compatibility broker is unavailable");
  }

  const inertEvent = () => {
    const listeners = new Set();
    return Object.freeze({
      addListener(listener) {
        if (typeof listener !== "function") {
          throw new TypeError("history event listener must be a function");
        }
        if (!listeners.has(listener) && listeners.size >= maxListenersPerEvent) {
          throw new RangeError("history event listener capacity exceeded");
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
  };

  const finiteTimestamp = (value, field) => {
    if (typeof value !== "number" || !Number.isFinite(value) || value < 0) {
      throw new TypeError(`history.search ${field} must be a non-negative finite number`);
    }
    return value;
  };

  const normalizeQuery = (query) => {
    if (query == null || typeof query !== "object" || Array.isArray(query)) {
      throw new TypeError("history.search query must be an object");
    }
    if (typeof query.text !== "string") {
      throw new TypeError("history.search query.text must be a string");
    }
    if (query.text.length > maxQueryCodeUnits) {
      throw new RangeError("history.search query.text exceeds the compatibility ceiling");
    }
    const requested = query.maxResults ?? maxResults;
    if (!Number.isSafeInteger(requested) || requested < 0) {
      throw new TypeError("history.search query.maxResults must be a non-negative safe integer");
    }
    const now = Date.now();
    const startTime =
      query.startTime === undefined
        ? Math.max(0, now - oneDayMilliseconds)
        : finiteTimestamp(query.startTime, "query.startTime");
    const endTime =
      query.endTime === undefined
        ? Number.MAX_SAFE_INTEGER
        : finiteTimestamp(query.endTime, "query.endTime");
    return Object.freeze({
      text: query.text.toLowerCase(),
      startTime,
      endTime,
      requested: Math.min(requested, maxResults),
    });
  };

  const exactKeys = (value, expected) => {
    const keys = Object.keys(value).sort();
    return keys.length === expected.length && keys.every((key, index) => key === expected[index]);
  };

  const decode = (encoded, query) => {
    if (typeof encoded !== "string" || encoded.length > maxResponseCodeUnits) {
      throw new Error("Zephium history broker returned an invalid response envelope");
    }
    const response = JSON.parse(encoded);
    if (
      response == null ||
      typeof response !== "object" ||
      Array.isArray(response) ||
      !exactKeys(response, ["items", "v"]) ||
      response.v !== 1 ||
      !Array.isArray(response.items) ||
      response.items.length > query.requested
    ) {
      throw new Error("Zephium history broker returned an invalid response contract");
    }

    const seen = new Set();
    const results = [];
    for (const item of response.items) {
      if (
        item == null ||
        typeof item !== "object" ||
        Array.isArray(item) ||
        !exactKeys(item, ["lastVisit", "title", "url"]) ||
        typeof item.url !== "string" ||
        item.url.length === 0 ||
        typeof item.title !== "string" ||
        !Number.isSafeInteger(item.lastVisit) ||
        item.lastVisit < 0 ||
        seen.has(item.url)
      ) {
        throw new Error("Zephium history broker returned an invalid history item");
      }
      seen.add(item.url);
      if (item.lastVisit < query.startTime || item.lastVisit > query.endTime) continue;
      if (
        query.text !== "" &&
        !item.url.toLowerCase().includes(query.text) &&
        !item.title.toLowerCase().includes(query.text)
      ) {
        continue;
      }
      results.push({
        id: item.url,
        url: item.url,
        title: item.title,
        lastVisitTime: item.lastVisit,
      });
    }
    return results;
  };

  const request = (query, settle) => {
    if (query.requested === 0 || query.endTime < query.startTime) {
      settle(null, []);
      return;
    }
    runtime.sendNativeMessage(
      applicationIdentifier,
      `${requestPrefix}${query.requested}`,
      (encoded) => {
        const nativeError = runtime.lastError;
        if (nativeError != null) {
          settle(new Error(String(nativeError.message ?? "Zephium history broker rejected request")));
          return;
        }
        try {
          settle(null, decode(encoded, query));
        } catch (error) {
          settle(error instanceof Error ? error : new Error("Zephium history response failed"));
        }
      },
    );
  };

  const search = (query, callback) => {
    const normalized = normalizeQuery(query);
    if (callback !== undefined) {
      if (typeof callback !== "function") {
        throw new TypeError("history.search callback must be a function");
      }
      request(normalized, (error, results) => callback(error == null ? results : []));
      return undefined;
    }
    return new Promise((resolve, reject) => {
      request(normalized, (error, results) => {
        if (error == null) resolve(results);
        else reject(error);
      });
    });
  };

  const facade = Object.freeze({
    search,
    onVisited: inertEvent(),
    onVisitRemoved: inertEvent(),
  });
  for (const namespace of namespaces) {
    const descriptor = Reflect.getOwnPropertyDescriptor(namespace, "history");
    if (
      (descriptor != null && descriptor.configurable !== true) ||
      (descriptor == null && !Object.isExtensible(namespace))
    ) {
      throw new Error("Zephium WebKit history compatibility cannot install safely");
    }
  }
  for (const namespace of namespaces) {
    // A service-worker realm can expose WebKit's native history object while
    // extension-page realms do not. That object is bound to WebKit's private
    // data store, not Zephium's product history authority. Shadow it with one
    // consistent read-only surface, but never mutate the native object.
    Object.defineProperty(namespace, "history", {
      value: facade,
      writable: false,
      enumerable: true,
      configurable: false,
    });
  }
  for (const namespace of namespaces) {
    const installedSurface = namespace.history;
    if (
      installedSurface?.search !== search ||
      installedSurface?.onVisited == null ||
      installedSurface?.onVisitRemoved == null ||
      installedSurface?.addUrl !== undefined ||
      installedSurface?.deleteAll !== undefined ||
      installedSurface?.deleteRange !== undefined ||
      installedSurface?.deleteUrl !== undefined ||
      installedSurface?.getVisits !== undefined
    ) {
      throw new Error("Zephium WebKit history compatibility did not settle exactly");
    }
  }
  Object.defineProperty(globalThis, modeMarker, {
    value: "bounded-recent-search",
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
