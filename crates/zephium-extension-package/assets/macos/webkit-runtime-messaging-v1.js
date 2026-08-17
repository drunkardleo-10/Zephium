(() => {
  "use strict";

  const installed = Symbol.for("zephium.webkit-runtime-messaging-compatibility.v1");
  if (globalThis[installed] === true) return;

  const runtime = globalThis.chrome?.runtime ?? globalThis.browser?.runtime;
  if (runtime?.id == null) {
    throw new Error("Zephium WebKit runtime messaging requires a native extension namespace");
  }

  const isBackground = typeof globalThis.document === "undefined";
  const isExtensionPage =
    !isBackground && globalThis.location?.protocol === "webkit-extension:";
  if (!isBackground && !isExtensionPage) {
    throw new Error("Zephium WebKit runtime messaging loaded in an unsupported realm");
  }

  const channel = "__zephium_runtime_message_v1__";
  const responseKeyPrefix = "__zephium_runtime_message_response_v1__";
  const wakeHandler = "__zephium_runtime_message_wake_v1__";
  const maxListeners = 128;
  const maxPendingRequests = 256;
  const maxResponseBytes = 1024 * 1024;
  const requestTimeoutMs = 30_000;

  // WKWebExtension preserves sender identity on extension ports, but reverse
  // port messages are not observable from an embedded extension page on the
  // supported WebKit floor. Requests therefore use the native port; responses
  // use per-extension, in-memory session storage and are removed by the sender.

  const nativeMessageEvent = runtime.onMessage;
  const nativeConnectEvent = runtime.onConnect;
  if (
    nativeMessageEvent == null ||
    nativeConnectEvent == null ||
    typeof nativeMessageEvent.addListener !== "function" ||
    typeof nativeMessageEvent.removeListener !== "function" ||
    typeof nativeMessageEvent.hasListener !== "function" ||
    typeof nativeConnectEvent.addListener !== "function" ||
    typeof nativeConnectEvent.removeListener !== "function" ||
    typeof nativeConnectEvent.hasListener !== "function" ||
    typeof runtime.connect !== "function" ||
    typeof runtime.sendMessage !== "function" ||
    typeof globalThis.TextEncoder !== "function"
  ) {
    throw new Error("Zephium WebKit runtime messaging surface is unavailable");
  }

  const nativeSendMessage = runtime.sendMessage.bind(runtime);
  const nativeConnect = runtime.connect.bind(runtime);
  const storage = globalThis.chrome?.storage ?? globalThis.browser?.storage;
  const session = storage?.session;
  if (
    typeof session?.get !== "function" ||
    typeof session?.set !== "function" ||
    typeof session?.remove !== "function"
  ) {
    throw new Error("Zephium WebKit runtime messaging requires extension session storage");
  }
  const callSession = (method, ...args) => {
    let returned;
    try {
      returned = method.call(session, ...args);
    } catch (error) {
      return Promise.reject(error);
    }
    if (returned == null || typeof returned.then !== "function") {
      return Promise.reject(new Error("Extension session storage did not return a Promise"));
    }
    return Promise.resolve(returned);
  };
  const sessionGet = (key) => callSession(session.get, key);
  const sessionSet = (entries) => callSession(session.set, entries);
  const sessionRemove = (key) => callSession(session.remove, key);

  if (isBackground) {
    const nativeAddMessageListener = nativeMessageEvent.addListener.bind(nativeMessageEvent);
    const nativeRemoveMessageListener = nativeMessageEvent.removeListener.bind(nativeMessageEvent);
    const nativeAddConnectListener = nativeConnectEvent.addListener.bind(nativeConnectEvent);
    const nativeRemoveConnectListener = nativeConnectEvent.removeListener.bind(nativeConnectEvent);
    const messageListeners = new Map();
    const connectListeners = new Map();

    const isWake = (message) =>
      message?.handler === wakeHandler && message?.v === 1 && message?.wake === true;

    const postResponse = (port, payload) => {
      if (typeof payload?.id !== "string" || payload.id.length === 0) {
        try {
          port.disconnect();
        } catch (_) {}
        return;
      }
      const key = `${responseKeyPrefix}${payload.id}`;
      let normalized;
      try {
        // Match extension-message structured-clone behavior and keep class
        // instances or functions from stranding a storage write.
        const encoded = JSON.stringify(payload);
        if (new TextEncoder().encode(encoded).byteLength > maxResponseBytes) {
          throw new RangeError("Runtime response exceeded the relay byte ceiling");
        }
        normalized = JSON.parse(encoded);
      } catch (error) {
        normalized = {
          v: 1,
          id: payload.id,
          kind: "error",
          message: String(error?.message ?? error),
        };
      }
      sessionSet({ [key]: normalized }).then(
        () => {
          try {
            port.disconnect();
          } catch (_) {}
        },
        () => {
          try {
            port.disconnect();
          } catch (_) {}
        },
      );
    };

    const dispatch = (port, request) => {
      if (
        request == null ||
        request.v !== 1 ||
        typeof request.id !== "string" ||
        request.id.length === 0 ||
        request.id.length > 48 ||
        !Object.hasOwn(request, "message")
      ) {
        postResponse(port, { v: 1, id: "", kind: "invalid" });
        return;
      }

      const records = [...messageListeners.values()];
      let index = 0;
      while (index < records.length) {
        const listener = records[index++].original;
        let callbackActive = true;
        let callbackSettled = false;
        const sendResponse = (value) => {
          if (!callbackActive || callbackSettled) return;
          callbackSettled = true;
          postResponse(port, {
            v: 1,
            id: request.id,
            kind: "response",
            hasValue: value !== undefined,
            ...(value === undefined ? {} : { value }),
          });
        };
        let returned;
        try {
          returned = listener(request.message, port.sender, sendResponse);
        } catch (error) {
          callbackActive = false;
          postResponse(port, {
            v: 1,
            id: request.id,
            kind: "error",
            message: String(error?.message ?? error),
          });
          return;
        }
        if (callbackSettled) return;
        if (returned === true) return;
        if (returned != null && typeof returned.then === "function") {
          callbackActive = false;
          Promise.resolve(returned).then(
            (value) =>
              postResponse(port, {
                v: 1,
                id: request.id,
                kind: "response",
                hasValue: value !== undefined,
                ...(value === undefined ? {} : { value }),
              }),
            (error) =>
              postResponse(port, {
                v: 1,
                id: request.id,
                kind: "error",
                message: String(error?.message ?? error),
              }),
          );
          return;
        }
        callbackActive = false;
      }
      postResponse(port, { v: 1, id: request.id, kind: "unhandled" });
    };

    nativeAddMessageListener((message) => (isWake(message) ? false : undefined));
    nativeAddConnectListener((port) => {
      if (port?.name !== channel) return;
      let received = false;
      port.onMessage.addListener((request) => {
        if (received) {
          postResponse(port, { v: 1, id: "", kind: "invalid" });
          return;
        }
        received = true;
        dispatch(port, request);
      });
    });

    const addMessageListener = (listener) => {
      if (typeof listener !== "function") {
        throw new TypeError("runtime.onMessage listener must be a function");
      }
      if (messageListeners.has(listener)) return;
      if (messageListeners.size >= maxListeners) {
        throw new RangeError("runtime.onMessage listener capacity exceeded");
      }
      const nativeWrapper = (message, sender, sendResponse) =>
        isWake(message) ? false : listener(message, sender, sendResponse);
      messageListeners.set(listener, { original: listener, nativeWrapper });
      nativeAddMessageListener(nativeWrapper);
    };
    const removeMessageListener = (listener) => {
      const record = messageListeners.get(listener);
      if (record == null) return;
      messageListeners.delete(listener);
      nativeRemoveMessageListener(record.nativeWrapper);
    };
    const addConnectListener = (listener) => {
      if (typeof listener !== "function") {
        throw new TypeError("runtime.onConnect listener must be a function");
      }
      if (connectListeners.has(listener)) return;
      if (connectListeners.size >= maxListeners) {
        throw new RangeError("runtime.onConnect listener capacity exceeded");
      }
      const nativeWrapper = (port) => {
        if (port?.name !== channel) listener(port);
      };
      connectListeners.set(listener, nativeWrapper);
      nativeAddConnectListener(nativeWrapper);
    };
    const removeConnectListener = (listener) => {
      const wrapped = connectListeners.get(listener);
      if (wrapped == null) return;
      connectListeners.delete(listener);
      nativeRemoveConnectListener(wrapped);
    };

    try {
      Object.defineProperties(nativeMessageEvent, {
        addListener: {
          value: addMessageListener,
          writable: false,
          enumerable: false,
          configurable: false,
        },
        removeListener: {
          value: removeMessageListener,
          writable: false,
          enumerable: false,
          configurable: false,
        },
        hasListener: {
          value: (listener) => messageListeners.has(listener),
          writable: false,
          enumerable: false,
          configurable: false,
        },
        hasListeners: {
          value: () => messageListeners.size !== 0,
          writable: false,
          enumerable: false,
          configurable: false,
        },
      });
      Object.defineProperties(nativeConnectEvent, {
        addListener: {
          value: addConnectListener,
          writable: false,
          enumerable: false,
          configurable: false,
        },
        removeListener: {
          value: removeConnectListener,
          writable: false,
          enumerable: false,
          configurable: false,
        },
        hasListener: {
          value: (listener) => connectListeners.has(listener),
          writable: false,
          enumerable: false,
          configurable: false,
        },
        hasListeners: {
          value: () => connectListeners.size !== 0,
          writable: false,
          enumerable: false,
          configurable: false,
        },
      });
    } catch (error) {
      throw new Error(
        `Zephium WebKit background messaging cannot be adapted safely: ${String(
          error?.message ?? error,
        )}`,
      );
    }
  } else {
    const pending = new Map();
    let nextRequestId = 0;
    if (typeof globalThis.crypto?.getRandomValues !== "function") {
      throw new Error("Zephium WebKit runtime messaging requires secure request identifiers");
    }
    if (typeof storage?.onChanged?.addListener === "function") {
      storage.onChanged.addListener((changes, areaName) => {
        if (areaName !== "session" || changes == null) return;
        for (const [key, change] of Object.entries(changes)) {
          if (!key.startsWith(responseKeyPrefix)) continue;
          const id = key.slice(responseKeyPrefix.length);
          pending.get(id)?.(change?.newValue);
        }
      });
    }

    const sendMessage = (...input) => {
      const args = [...input];
      const callback = typeof args.at(-1) === "function" ? args.pop() : null;
      if (typeof args[0] === "string" && args.length >= 2) {
        // Cross-extension messaging is outside this same-principal bridge.
        return nativeSendMessage(...input);
      }
      if (args.length === 0 || args.length > 2) {
        return nativeSendMessage(...input);
      }
      if (pending.size >= maxPendingRequests) {
        const error = new Error("Zephium runtime message capacity exceeded");
        if (callback != null) {
          queueMicrotask(() => callback(undefined));
          return undefined;
        }
        return Promise.reject(error);
      }

      nextRequestId = (nextRequestId + 1) % Number.MAX_SAFE_INTEGER;
      const entropy = new Uint32Array(3);
      globalThis.crypto.getRandomValues(entropy);
      const id = `${[...entropy].map((value) => value.toString(16).padStart(8, "0")).join("")}-${nextRequestId.toString(36)}`;
      const responseKey = `${responseKeyPrefix}${id}`;
      const promise = new Promise((resolve, reject) => {
        let activePort;
        let settled = false;
        const settle = (operation, value) => {
          if (settled) return;
          settled = true;
          clearTimeout(timeout);
          pending.delete(id);
          void sessionRemove(responseKey).catch(() => {});
          try {
            activePort?.disconnect();
          } catch (_) {}
          operation(value);
        };
        const timeout = setTimeout(
          () => settle(reject, new Error("Zephium runtime message timed out")),
          requestTimeoutMs,
        );
        const consume = (response) => {
          if (response?.v !== 1 || response.id !== id) return false;
          if (response.kind === "response") {
            settle(resolve, response.hasValue === true ? response.value : undefined);
          } else if (response.kind === "unhandled") {
            settle(resolve, undefined);
          } else {
            settle(
              reject,
              new Error(
                response.kind === "error" && typeof response.message === "string"
                  ? response.message
                  : "Zephium runtime message protocol failed",
                ),
            );
          }
          return true;
        };
        pending.set(id, consume);

        const pollDelaysMs = [10, 25, 50, 100, 250, 500];
        let pollIndex = 0;
        const poll = () => {
          if (settled) return;
          sessionGet(responseKey).then(
            (stored) => {
              if (settled) return;
              if (stored != null && Object.hasOwn(stored, responseKey)) {
                consume(stored[responseKey]);
                return;
              }
              const delay = pollDelaysMs[Math.min(pollIndex++, pollDelaysMs.length - 1)];
              setTimeout(poll, delay);
            },
            () => setTimeout(poll, pollDelaysMs.at(-1)),
          );
        };
        poll();

        const open = () => {
          if (settled) return;
          let port;
          try {
            port = nativeConnect({ name: channel });
            activePort = port;
            port.onDisconnect.addListener(() => {
              if (!settled && port === activePort) activePort = undefined;
            });
            port.postMessage({ v: 1, id, message: args[0] });
          } catch (error) {
            settle(reject, error);
          }
        };

        let started = false;
        const start = () => {
          if (started || settled) return;
          started = true;
          open();
        };
        sessionRemove(responseKey).finally(() => {
          // The one-way native message wakes a suspended MV3 worker. Its
          // reserved shape is filtered before original extension listeners.
          try {
            nativeSendMessage({ handler: wakeHandler, v: 1, wake: true }, start);
          } catch (_) {
            start();
          }
          setTimeout(start, 25);
        });
      });

      if (callback != null) {
        promise.then(callback, () => callback(undefined));
        return undefined;
      }
      return promise;
    };

    try {
      Object.defineProperty(runtime, "sendMessage", {
        value: sendMessage,
        writable: false,
        enumerable: false,
        configurable: false,
      });
    } catch (error) {
      throw new Error(
        `Zephium WebKit extension-page messaging cannot be adapted safely: ${String(
          error?.message ?? error,
        )}`,
      );
    }
    if (runtime.sendMessage !== sendMessage) {
      throw new Error("Zephium WebKit extension-page messaging did not settle exactly");
    }
  }

  Object.defineProperty(globalThis, installed, {
    value: true,
    writable: false,
    enumerable: false,
    configurable: false,
  });
})();
