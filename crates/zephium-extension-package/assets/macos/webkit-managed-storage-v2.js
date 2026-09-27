(() => {
  "use strict";
  const marker = Symbol.for("zephium.webkit-managed-storage.v2");
  if (globalThis[marker]) return;
  const namespaces = () => [...new Set([globalThis.chrome, globalThis.browser])]
    .filter(api => api?.runtime?.id != null && api.storage != null);
  if (!namespaces().length) throw new Error("Managed storage requires an extension namespace");

  // This browser has no administrator policy provider. Keep this area separate
  // from local/sync/session storage: reads contain no policy keys, writes fail.
  const listeners = new Set();
  const onChanged = Object.freeze({
    addListener(listener) {
      if (typeof listener !== "function") throw new TypeError("listener must be a function");
      if (!listeners.has(listener) && listeners.size >= 64) throw new RangeError("managed listener limit");
      listeners.add(listener);
    },
    removeListener(listener) { listeners.delete(listener); },
    hasListener(listener) { return listeners.has(listener); },
    hasListeners() { return listeners.size !== 0; },
  });
  function callbackOrPromise(value, callback) {
    if (callback !== undefined && typeof callback !== "function") throw new TypeError("invalid callback");
    if (callback) { queueMicrotask(() => callback(value)); return undefined; }
    return Promise.resolve(value);
  }
  function validateKeys(keys, defaultsAllowed) {
    if (keys == null || typeof keys === "string") return;
    if (Array.isArray(keys) && keys.every(key => typeof key === "string")) return;
    if (defaultsAllowed && typeof keys === "object" && !Array.isArray(keys)) return;
    throw new TypeError("invalid managed storage keys");
  }
  function denied(callback, message) {
    if (callback === undefined) return Promise.reject(new Error(message));
    if (typeof callback !== "function") throw new TypeError("invalid callback");
    // Chrome callback callers observe lastError only while their callback runs.
    // Refuse synchronously if the native runtime cannot represent that error.
    const runtimes = [...new Set(namespaces().map(api => api.runtime))];
    const descriptors = runtimes.map(runtime => Object.getOwnPropertyDescriptor(runtime, "lastError"));
    if (descriptors.some(descriptor => descriptor && !descriptor.configurable)) throw new Error(message);
    queueMicrotask(() => {
      const restore = [];
      try {
        for (const runtime of runtimes) {
          const previous = Object.getOwnPropertyDescriptor(runtime, "lastError");
          Object.defineProperty(runtime, "lastError", {configurable: true, value: {message}});
          restore.push([runtime, previous]);
        }
        callback();
      } finally {
        for (const [runtime, previous] of restore) {
          if (previous) Object.defineProperty(runtime, "lastError", previous);
          else delete runtime.lastError;
        }
      }
    });
    return undefined;
  }
  const facade = Object.freeze({
    onChanged,
    get(keys, callback) {
      if (typeof keys === "function" && callback === undefined) { callback = keys; keys = undefined; }
      validateKeys(keys, true);
      // JSON serialization matches the value model of extension storage and
      // returns a fresh object; caller defaults never become stored policies.
      const result = {};
      if (keys != null && typeof keys === "object" && !Array.isArray(keys)) {
        for (const [key, value] of Object.entries(keys)) {
          const encoded = JSON.stringify(value);
          if (encoded !== undefined) Object.defineProperty(result, key, {
            value: JSON.parse(encoded), enumerable: true, configurable: true, writable: true,
          });
        }
      }
      return callbackOrPromise(result, callback);
    },
    getKeys(callback) { return callbackOrPromise([], callback); },
    getBytesInUse(keys, callback) {
      if (typeof keys === "function" && callback === undefined) { callback = keys; keys = undefined; }
      validateKeys(keys, false);
      return callbackOrPromise(0, callback);
    },
    set(_items, callback) { return denied(callback, "Managed storage is read-only"); },
    remove(_keys, callback) { return denied(callback, "Managed storage is read-only"); },
    clear(callback) { return denied(callback, "Managed storage is read-only"); },
    setAccessLevel(_options, callback) { return denied(callback, "Managed storage access configuration is unavailable"); },
  });
  function install() {
    // Never overwrite real native managed storage or route managed reads to a
    // writable area. Polyfills may replace the enclosing storage namespace.
    for (const api of namespaces()) {
      if (api.storage.managed == null) {
        Object.defineProperty(api.storage, "managed", {value: facade, configurable: false, writable: false});
      }
      if (typeof api.storage.managed.get !== "function") throw new Error("Invalid native managed storage");
    }
  }
  install();
  // Same bounded startup reconciliation as v1, with no recurring timer.
  const reconcile = (schedule, count) => {
    if (count) schedule(() => { install(); reconcile(schedule, count - 1); });
  };
  reconcile(queueMicrotask, 4);
  reconcile(callback => setTimeout(callback, 0), 4);
  if (typeof document !== "undefined" && document.readyState === "loading") {
    addEventListener("DOMContentLoaded", install, {once: true});
  }
  Object.defineProperty(globalThis, marker, {value: true});
})();
