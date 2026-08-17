(() => {
  "use strict";

  const installed = Symbol.for("zephium.webkit-favicon-compatibility.v1");
  const modeMarker = Symbol.for("zephium.webkit-favicon-compatibility.mode.v1");
  if (globalThis[installed] === true) return;

  const resource = "__zephium__/favicon-empty-v1.svg";
  const runtimes = [
    ...new Set([globalThis.chrome?.runtime, globalThis.browser?.runtime]),
  ].filter((runtime) => runtime?.id != null);
  if (runtimes.length === 0) {
    throw new Error("Zephium WebKit favicon compatibility requires a native runtime");
  }

  for (const runtime of runtimes) {
    if (typeof runtime.getURL !== "function") {
      throw new Error("Zephium WebKit favicon compatibility requires runtime.getURL");
    }
    const nativeGetURL = runtime.getURL.bind(runtime);
    const getURL = (path) => {
      if (typeof path !== "string") {
        return nativeGetURL(path);
      }
      const normalized = path.startsWith("/") ? path.slice(1) : path;
      return nativeGetURL(
        normalized === "_favicon" || normalized.startsWith("_favicon/") ? resource : path,
      );
    };
    try {
      Object.defineProperty(runtime, "getURL", {
        value: getURL,
        writable: false,
        enumerable: false,
        configurable: false,
      });
    } catch (error) {
      throw new Error(
        `Zephium WebKit favicon compatibility cannot install safely: ${String(
          error?.message ?? error,
        )}`,
      );
    }
    if (runtime.getURL !== getURL) {
      throw new Error("Zephium WebKit favicon compatibility did not settle exactly");
    }
  }

  Object.defineProperty(globalThis, modeMarker, {
    value: "transparent-fallback",
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
