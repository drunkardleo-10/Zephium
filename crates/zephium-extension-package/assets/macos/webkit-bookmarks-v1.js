(() => {
  "use strict";

  const installed = Symbol.for("zephium.webkit-bookmarks-compatibility.v1");
  const modeMarker = Symbol.for("zephium.webkit-bookmarks-compatibility.mode.v1");
  if (globalThis[installed] === true) return;

  const namespaces = [...new Set([globalThis.chrome, globalThis.browser])].filter(
    (namespace) => namespace?.runtime?.id != null,
  );
  if (namespaces.length === 0) {
    throw new Error("Zephium WebKit bookmarks compatibility requires a native namespace");
  }
  if (namespaces.some((namespace) => namespace.bookmarks != null)) {
    if (!namespaces.every((namespace) => namespace.bookmarks != null)) {
      throw new Error("Zephium WebKit bookmarks namespaces disagree");
    }
    Object.defineProperty(globalThis, modeMarker, {
      value: "native-preserved",
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
    return;
  }

  const maxListeners = 128;
  const event = () => {
    const listeners = new Set();
    return Object.freeze({
      addListener(listener) {
        if (typeof listener !== "function") {
          throw new TypeError("bookmarks listener must be a function");
        }
        if (listeners.has(listener)) return;
        if (listeners.size >= maxListeners) {
          throw new RangeError("bookmarks listener capacity exceeded");
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
  const settle = (value, callback) => {
    const copy = JSON.parse(JSON.stringify(value));
    if (typeof callback === "function") {
      queueMicrotask(() => callback(copy));
      return undefined;
    }
    return Promise.resolve(copy);
  };
  const emptyRoot = () => [{ id: "0", title: "", children: [] }];
  const empty = () => [];
  const facade = Object.freeze({
    getTree(callback) {
      return settle(emptyRoot(), callback);
    },
    get(ids, callback) {
      if (typeof ids === "function") return settle(empty(), ids);
      const requested = Array.isArray(ids) ? ids : [ids];
      const value = requested.some((id) => id === "0" || id === 0) ? emptyRoot() : empty();
      return settle(value, callback);
    },
    getChildren(_id, callback) {
      return settle(empty(), callback);
    },
    getRecent(numberOfItems, callback) {
      if (!Number.isInteger(numberOfItems) || numberOfItems < 0 || numberOfItems > 10_000) {
        throw new TypeError("bookmarks.getRecent count is invalid");
      }
      return settle(empty(), callback);
    },
    getSubTree(id, callback) {
      return settle(id === "0" || id === 0 ? emptyRoot() : empty(), callback);
    },
    search(_query, callback) {
      return settle(empty(), callback);
    },
    onChanged: event(),
    onChildrenReordered: event(),
    onCreated: event(),
    onImportBegan: event(),
    onImportEnded: event(),
    onMoved: event(),
    onRemoved: event(),
  });

  for (const namespace of namespaces) {
    const descriptor = Reflect.getOwnPropertyDescriptor(namespace, "bookmarks");
    if (
      (descriptor != null && descriptor.configurable !== true) ||
      (descriptor == null && !Object.isExtensible(namespace))
    ) {
      throw new Error("Zephium WebKit bookmarks compatibility cannot install safely");
    }
    Object.defineProperty(namespace, "bookmarks", {
      value: facade,
      writable: false,
      enumerable: true,
      configurable: false,
    });
    if (namespace.bookmarks !== facade) {
      throw new Error("Zephium WebKit bookmarks compatibility did not settle exactly");
    }
  }

  Object.defineProperty(globalThis, modeMarker, {
    value: "empty-read-only",
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
