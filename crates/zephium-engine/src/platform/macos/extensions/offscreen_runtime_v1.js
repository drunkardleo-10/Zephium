(() => {
  "use strict";

  // The native host substitutes this token only after binding one authenticated
  // package, profile and live extension owner. This script is injected into a
  // plain WKWebView; WebKit's extension controller is never attached to it.
  const extensionId = "__ZEPHIUM_OFFSCREEN_EXTENSION_ID__";
  const base = "__ZEPHIUM_OFFSCREEN_BASE_URL__";
  const bridge = globalThis.webkit?.messageHandlers?.zephiumOffscreenRuntimeV1;
  if (bridge == null || globalThis.chrome !== undefined || globalThis.browser !== undefined) {
    throw new Error("Isolated offscreen runtime construction failed");
  }

  const encoder = new TextEncoder();
  const maxMessageBytes = 1024 * 1024;
  const maxListeners = 128;
  const messageTimeoutMs = 30_000;
  const listeners = new Set();
  let closed = false;

  const serialize = (value) => {
    const text = JSON.stringify(value);
    if (typeof text !== "string" || encoder.encode(text).byteLength > maxMessageBytes) {
      throw new RangeError("Offscreen runtime message exceeds the byte limit");
    }
    return text;
  };

  const post = async (operation, value) => {
    if (closed) throw new Error("Offscreen document is closed");
    const response = await bridge.postMessage(serialize({ v: 1, operation, value }));
    if (typeof response !== "string" || encoder.encode(response).byteLength > maxMessageBytes) {
      throw new Error("Offscreen native response is invalid");
    }
    const decoded = JSON.parse(response);
    if (decoded?.v !== 1 || decoded?.ok !== true) {
      throw new Error("Offscreen native request failed");
    }
    return decoded.value;
  };

  const onMessage = Object.freeze({
    addListener(listener) {
      if (typeof listener !== "function") throw new TypeError("Listener must be a function");
      if (listeners.size >= maxListeners && !listeners.has(listener)) {
        throw new RangeError("Too many offscreen message listeners");
      }
      listeners.add(listener);
    },
    removeListener(listener) { listeners.delete(listener); },
    hasListener(listener) { return listeners.has(listener); },
  });

  const runtime = Object.freeze({
    id: extensionId,
    onMessage,
    getURL(path = "") {
      if (typeof path !== "string" || path.length > 512 || path.startsWith("/") ||
          path.split("/").some((component) => component === "." || component === "..")) {
        throw new TypeError("Invalid extension resource path");
      }
      const url = new URL(path, base);
      if (url.origin !== new URL(base).origin || url.search || url.hash ||
          url.pathname.includes("//") || path.includes("\\") || path.includes("%")) {
        throw new TypeError("Invalid extension resource path");
      }
      return url.href;
    },
    sendMessage(message, callback) {
      const promise = post("runtime.sendMessage", message).then((result) =>
        result?.handled === true ? result.value : undefined,
      );
      if (typeof callback === "function") {
        void promise.then((value) => callback(value), () => callback(undefined));
        return undefined;
      }
      return promise;
    },
  });

  const api = Object.freeze({ runtime });
  Object.defineProperty(globalThis, "chrome", { value: api, configurable: false, writable: false });
  Object.defineProperty(globalThis, "browser", { value: api, configurable: false, writable: false });

  // A native callAsyncJavaScript invocation awaits this result. The sender is
  // always the same bound extension; no page-provided sender identity is used.
  const dispatch = async (message) => {
    if (closed) throw new Error("Offscreen document is closed");
    let settled = false;
    let waiting = false;
    let firstError;
    let timer;
    let resolveReply;
    let rejectReply;
    const response = new Promise((resolve, reject) => {
      resolveReply = resolve;
      rejectReply = reject;
    });
    const respond = (value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolveReply({ handled: true, value });
    };
    const fail = (error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      rejectReply(error);
    };
    for (const listener of [...listeners]) {
      let callbackActive = true;
      let result;
      try {
        result = listener(message, Object.freeze({ id: extensionId }), (value) => {
          if (callbackActive) respond(value);
        });
      } catch (error) {
        callbackActive = false;
        firstError ??= error;
        continue;
      }
      if (result === true) {
        waiting = true;
      } else {
        callbackActive = false;
        if (result != null && typeof result.then === "function") {
          waiting = true;
          Promise.resolve(result).then(respond, fail);
        } else if (result !== undefined && result !== false) {
          respond(result);
        }
      }
    }
    if (!settled && !waiting) {
      if (firstError !== undefined) fail(firstError);
      else resolveReply({ handled: false });
    } else if (!settled) {
      timer = setTimeout(() => fail(new Error("Offscreen response timed out")), messageTimeoutMs);
    }
    return serialize(await response);
  };
  Object.defineProperty(globalThis, "__zephiumOffscreenRuntimeV1", {
    value: Object.freeze({ dispatch, close() { closed = true; listeners.clear(); } }),
    configurable: false, writable: false,
  });

  // Playback activity is event-driven. The native host owns the 30-second
  // one-shot inactivity timer and independently checks WK media state before
  // retiring an AUDIO_PLAYBACK document. Buffer sources cover Google Translate.
  let activePlayback = 0;
  const activity = (active) => {
    activePlayback = Math.max(0, activePlayback + (active ? 1 : -1));
    void post("audio.activity", { active: activePlayback > 0 }).catch(() => {});
  };
  addEventListener("play", () => activity(true), true);
  addEventListener("pause", () => activity(false), true);
  addEventListener("ended", () => activity(false), true);
  const OriginalAudioContext = globalThis.AudioContext;
  if (typeof OriginalAudioContext === "function") {
    class ObservedAudioContext extends OriginalAudioContext {
      createBufferSource(...args) {
        const source = super.createBufferSource(...args);
        const start = source.start.bind(source);
        let started = false;
        let ended = false;
        source.start = (...startArgs) => {
          const result = start(...startArgs);
          if (!started) { started = true; activity(true); }
          return result;
        };
        source.addEventListener("ended", () => {
          if (started && !ended) { ended = true; activity(false); }
        }, { once: true });
        return source;
      }
    }
    Object.defineProperty(globalThis, "AudioContext", {
      value: ObservedAudioContext, configurable: false, writable: false,
    });
  }
})();
