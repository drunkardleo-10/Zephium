(() => {
  "use strict";

  // This adapter is injected only into a reviewed MV3 background worker. The
  // fixed native application ID is not exposed to publisher code as a general
  // native-messaging grant; every request is re-authorized by the host.
  if (typeof document !== "undefined") {
    throw new Error("Offscreen compatibility requires a service worker");
  }
  const runtime = globalThis.chrome?.runtime ?? globalThis.browser?.runtime;
  if (typeof runtime?.id !== "string" || typeof runtime.connectNative !== "function" ||
      globalThis.chrome?.offscreen != null || globalThis.browser?.offscreen != null) {
    throw new Error("Offscreen compatibility runtime is unavailable");
  }

  const nativeApplication = "app.zephium.extension-offscreen.v1";
  const relaySymbol = Symbol.for("zephium.webkit-offscreen-relay.v1");
  const backgroundDispatchSymbol = Symbol.for("zephium.webkit-runtime-message-dispatch.v2");
  const maxWireBytes = 1024 * 1024;
  const maxPending = 64;
  const timeoutMs = 30_000;
  const encoder = new TextEncoder();
  const pending = new Map();
  let port = null;
  let nextId = 0;
  let documentActive = false;

  const encode = (value) => {
    const text = JSON.stringify(value);
    if (typeof text !== "string" || encoder.encode(text).byteLength > maxWireBytes) {
      throw new RangeError("Offscreen message exceeds the byte limit");
    }
    return text;
  };
  const disconnect = () => {
    const old = port;
    port = null;
    documentActive = false;
    try { old?.disconnect(); } catch (_) {}
  };
  const failPending = () => {
    for (const [, request] of pending) {
      clearTimeout(request.timer);
      request.reject(new Error("Offscreen host disconnected"));
    }
    pending.clear();
  };
  const receive = (wire) => {
    let frame;
    try {
      if (typeof wire !== "string" || encoder.encode(wire).byteLength > maxWireBytes) {
        throw new TypeError("Invalid offscreen host message");
      }
      frame = JSON.parse(wire);
      if (frame?.v !== 1 || !Number.isSafeInteger(frame.id) || frame.id <= 0) {
        throw new TypeError("Invalid offscreen host envelope");
      }
    } catch (_) {
      disconnect();
      failPending();
      return;
    }
    if (frame.operation === "fromDocument") {
      const dispatch = globalThis[backgroundDispatchSymbol];
      Promise.resolve().then(() => {
        if (typeof dispatch !== "function" || !documentActive) {
          return { handled: false };
        }
        return dispatch(frame.value, frame.sender);
      }).then(
        (result) => {
          if (port != null) port.postMessage(encode({
            v: 1, id: frame.id, operation: "fromDocumentResult", result,
          }));
        },
        () => {
          if (port != null) port.postMessage(encode({
            v: 1, id: frame.id, operation: "fromDocumentResult", result: { handled: false },
          }));
        },
      );
      return;
    }
    const request = pending.get(frame.id);
    if (request == null || frame.operation !== "result") return;
    pending.delete(frame.id);
    clearTimeout(request.timer);
    if (frame.ok === true) request.resolve(frame.value);
    else request.reject(new Error(typeof frame.error === "string" ? frame.error : "Offscreen host rejected request"));
  };
  const connect = () => {
    if (port != null) return port;
    const opened = runtime.connectNative(nativeApplication);
    opened.onMessage.addListener(receive);
    opened.onDisconnect.addListener(() => {
      if (port !== opened) return;
      disconnect();
      failPending();
    });
    port = opened;
    return opened;
  };
  const invoke = (operation, value) => {
    if (pending.size >= maxPending) {
      return Promise.reject(new Error("Offscreen request capacity exceeded"));
    }
    let activePort;
    try { activePort = connect(); } catch (error) { return Promise.reject(error); }
    nextId = (nextId + 1) % Number.MAX_SAFE_INTEGER;
    if (nextId === 0) nextId = 1;
    const id = nextId;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        if (!pending.delete(id)) return;
        reject(new Error("Offscreen host timed out"));
        disconnect();
        failPending();
      }, timeoutMs);
      pending.set(id, { resolve, reject, timer });
      try { activePort.postMessage(encode({ v: 1, id, operation, value })); }
      catch (error) {
        clearTimeout(timer);
        pending.delete(id);
        reject(error);
      }
    });
  };
  const hasDocument = async () => {
    const result = await invoke("has", null);
    if (typeof result !== "boolean") throw new TypeError("Invalid offscreen host state");
    documentActive = result;
    if (!result && pending.size === 0) disconnect();
    return result;
  };
  const createDocument = async (options) => {
    if (options == null || typeof options !== "object" ||
        typeof options.url !== "string" || options.url.length === 0 || options.url.length > 512 ||
        !Array.isArray(options.reasons) || options.reasons.length !== 1 ||
        options.reasons[0] !== "LOCAL_STORAGE" ||
        typeof options.justification !== "string" || options.justification.length > 1024) {
      throw new TypeError("Unsupported offscreen document request");
    }
    const result = await invoke("create", options);
    if (result !== true) throw new Error("Offscreen document did not load");
    documentActive = true;
  };
  const closeDocument = async () => {
    const result = await invoke("close", null);
    if (result !== true) throw new Error("Offscreen document did not close");
    disconnect();
  };
  const relay = Object.freeze({
    async dispatchToDocument(message) {
      if (!documentActive) return { handled: false };
      const result = await invoke("message", message);
      if (result == null || typeof result !== "object" || typeof result.handled !== "boolean") {
        throw new TypeError("Invalid offscreen document response");
      }
      return result;
    },
  });
  const offscreen = Object.freeze({
    Reason: Object.freeze({ LOCAL_STORAGE: "LOCAL_STORAGE" }),
    createDocument,
    closeDocument,
    hasDocument,
  });
  Object.defineProperty(globalThis.chrome, "offscreen", {
    value: offscreen, configurable: false, enumerable: true, writable: false,
  });
  if (globalThis.browser !== globalThis.chrome) {
    Object.defineProperty(globalThis.browser, "offscreen", {
      value: offscreen, configurable: false, enumerable: true, writable: false,
    });
  }
  Object.defineProperty(globalThis, relaySymbol, {
    value: relay, configurable: false, enumerable: false, writable: false,
  });
})();
