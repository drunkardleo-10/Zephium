// Zephium's compatibility layer for Chrome extensions on WebKit. It runs first
// in every extension context and defines only what WebKit lacks or gets wrong,
// so anything WebKit later implements properly takes precedence.
(() => {
  "use strict";
  const g = globalThis;
  const KEY = Symbol.for("zephium.compat");
  if (g[KEY]) return;
  const chromeApi = g.chrome;
  const runtime = chromeApi && chromeApi.runtime;
  // Absent in page-world scripts and sandboxed pages: nothing to adapt.
  if (!runtime || !runtime.id) return;

  const CHROME_VERSION = "152.0.0.0";
  const isWorker =
    typeof ServiceWorkerGlobalScope !== "undefined" && g instanceof ServiceWorkerGlobalScope;
  const isPage =
    !isWorker &&
    typeof location !== "undefined" &&
    (location.protocol === "chrome-extension:" || location.protocol === "webkit-extension:");
  const isContent = !isWorker && !isPage;
  const config = typeof __zephiumConfig === "object" && __zephiumConfig ? __zephiumConfig : {};
  const Z = { isWorker, isPage, isContent, config };
  Object.defineProperty(g, KEY, { value: Z });

  const native = (api, fields) =>
    runtime.sendNativeMessage("app.zephium.webext", Object.assign({ api }, fields));

  // ---- Diagnostics ---------------------------------------------------------
  // Errors inside workers and extension pages are otherwise invisible to the
  // browser; report them, bounded, so failures have a cause on record.
  if (!isContent && typeof runtime.sendNativeMessage === "function") {
    let budget = 60;
    setInterval(() => (budget = 60), 10000);
    const describe = (value) => {
      if (value instanceof Error) return value.stack ? `${value}\n${value.stack}` : String(value);
      if (typeof value === "object" && value !== null) {
        try {
          return JSON.stringify(value).slice(0, 500);
        } catch {
          return Object.prototype.toString.call(value);
        }
      }
      return String(value);
    };
    const where = isWorker ? "worker" : location.pathname;
    const report = (level, text) => {
      if (budget-- <= 0) return;
      try {
        native("log", { level, text: `[${where}] ${text}` }).catch(() => {});
      } catch {}
    };
    Z.report = report;
    g.addEventListener("error", (event) => {
      const origin = event.filename ? ` @ ${event.filename}:${event.lineno}:${event.colno}` : "";
      report("error", `${event.message}${origin}${event.error && event.error.stack ? "\n" + event.error.stack : ""}`);
    });
    g.addEventListener("unhandledrejection", (event) => {
      report("error", `unhandled rejection: ${describe(event.reason)}`);
    });
    const consoleError = console.error;
    console.error = function (...args) {
      report("error", args.map(describe).join(" "));
      return consoleError.apply(this, args);
    };
    const consoleWarn = console.warn;
    console.warn = function (...args) {
      report("warning", args.map(describe).join(" "));
      return consoleWarn.apply(this, args);
    };
  }

  // ---- Language gaps -------------------------------------------------------
  if (typeof Symbol.dispose !== "symbol") {
    Object.defineProperty(Symbol, "dispose", { value: Symbol.for("Symbol.dispose") });
  }
  if (typeof Symbol.asyncDispose !== "symbol") {
    Object.defineProperty(Symbol, "asyncDispose", { value: Symbol.for("Symbol.asyncDispose") });
  }

  // ---- Browser identity ----------------------------------------------------
  // The native user agent must stay identical to web tabs' (see the runtime),
  // so extensions are told they run in Chrome here. Content scripts share the
  // page's navigator and keep the real value.
  if (!isContent && !/ Chrome\//.test(navigator.userAgent)) {
    const base = navigator.userAgent.replace(/ Version\/\S+/, "").replace(/ Safari\/\S+$/, "");
    const chromeAgent = `${base} Chrome/${CHROME_VERSION} Safari/537.36`;
    const major = CHROME_VERSION.split(".")[0];
    const brands = [
      { brand: "Chromium", version: major },
      { brand: "Google Chrome", version: major },
      { brand: "Not=A?Brand", version: "24" },
    ];
    const platformVersion = (/Mac OS X (\d+)[_.](\d+)/.exec(navigator.userAgent) || []).slice(1).join(".") || "15.0";
    const agentData = {
      brands,
      mobile: false,
      platform: "macOS",
      getHighEntropyValues: async (hints) => {
        const values = {
          brands,
          mobile: false,
          platform: "macOS",
          architecture: "arm",
          bitness: "64",
          model: "",
          platformVersion,
          uaFullVersion: CHROME_VERSION,
          fullVersionList: brands.map((b) => ({ brand: b.brand, version: b.brand === "Not=A?Brand" ? "24.0.0.0" : CHROME_VERSION })),
        };
        const result = { brands, mobile: false, platform: "macOS" };
        for (const hint of hints || []) if (hint in values) result[hint] = values[hint];
        return result;
      },
      toJSON() {
        return { brands, mobile: false, platform: "macOS" };
      },
    };
    const proto = Object.getPrototypeOf(navigator);
    const define = (name, value) => {
      try {
        Object.defineProperty(proto, name, { configurable: true, get: () => value });
      } catch {}
    };
    define("userAgent", chromeAgent);
    define("appVersion", chromeAgent.replace(/^Mozilla\//, ""));
    define("vendor", "Google Inc.");
    define("userAgentData", agentData);
  }

  // ---- Worker WebSockets ---------------------------------------------------
  // A WebSocket opened in an extension worker deadlocks it in WebKit; connect
  // through the browser instead.
  if (isWorker && typeof runtime.connectNative === "function" && !config.nativeWebSockets) {
    const encode = (buffer) => {
      const bytes = new Uint8Array(buffer);
      let text = "";
      for (let i = 0; i < bytes.length; i += 0x8000) {
        text += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
      }
      return btoa(text);
    };
    const decode = (text) => {
      const binary = atob(text);
      const bytes = new Uint8Array(binary.length);
      for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
      return bytes.buffer;
    };
    const states = { CONNECTING: 0, OPEN: 1, CLOSING: 2, CLOSED: 3 };
    class WebSocket extends EventTarget {
      #port;
      #ready = false;
      #queue = [];
      #hello;
      #sending = Promise.resolve();
      #binaryType = "blob";
      constructor(url, protocols) {
        super();
        let parsed;
        try {
          parsed = new URL(url, location.href);
        } catch {
          throw new DOMException(`Invalid URL '${url}'`, "SyntaxError");
        }
        if (parsed.protocol === "http:") parsed.protocol = "ws:";
        if (parsed.protocol === "https:") parsed.protocol = "wss:";
        if (parsed.protocol !== "ws:" && parsed.protocol !== "wss:") {
          throw new DOMException(`Invalid URL scheme '${parsed.protocol}'`, "SyntaxError");
        }
        this.url = parsed.href;
        this.readyState = states.CONNECTING;
        this.protocol = "";
        this.extensions = "";
        this.bufferedAmount = 0;
        this.onopen = this.onmessage = this.onerror = this.onclose = null;
        const port = runtime.connectNative("app.zephium.socket");
        this.#port = port;
        port.onMessage.addListener((message) => this.#receive(message));
        port.onDisconnect.addListener(() => this.#finish(1006, ""));
        const hello = () => {
          try {
            port.postMessage({ op: "hello" });
          } catch {}
        };
        hello();
        this.#hello = setInterval(hello, 50);
        const list = protocols === undefined ? [] : [].concat(protocols).map(String);
        this.#post({ op: "open", url: this.url, protocols: list, userAgent: navigator.userAgent });
      }
      get binaryType() {
        return this.#binaryType;
      }
      set binaryType(value) {
        if (value === "blob" || value === "arraybuffer") this.#binaryType = value;
      }
      send(data) {
        if (this.readyState === states.CONNECTING) {
          throw new DOMException("Still in CONNECTING state.", "InvalidStateError");
        }
        if (this.readyState !== states.OPEN) return;
        const post = (message) => this.#post(message);
        if (typeof data === "string") {
          this.#sending = this.#sending.then(() => post({ op: "send", text: data }));
        } else if (data instanceof Blob) {
          this.#sending = this.#sending.then(() =>
            data.arrayBuffer().then((buffer) => post({ op: "send", base64: encode(buffer) })),
          );
        } else if (data instanceof ArrayBuffer || ArrayBuffer.isView(data)) {
          const view = data instanceof ArrayBuffer ? data : data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength);
          this.#sending = this.#sending.then(() => post({ op: "send", base64: encode(view) }));
        } else {
          this.#sending = this.#sending.then(() => post({ op: "send", text: String(data) }));
        }
      }
      close(code, reason) {
        if (this.readyState >= states.CLOSING) return;
        this.readyState = states.CLOSING;
        this.#post({ op: "close", code: code === undefined ? 1000 : code, reason: reason || "" });
      }
      #post(message) {
        if (this.#ready) this.#port.postMessage(message);
        else this.#queue.push(message);
      }
      #receive(message) {
        switch (message && message.op) {
          case "hello":
            if (!this.#ready) {
              this.#ready = true;
              clearInterval(this.#hello);
              for (const queued of this.#queue.splice(0)) this.#port.postMessage(queued);
            }
            break;
          case "open":
            this.readyState = states.OPEN;
            this.protocol = message.protocol || "";
            this.#fire(new Event("open"));
            break;
          case "message": {
            let data = message.text;
            if (data === undefined) {
              const buffer = decode(message.base64 || "");
              data = this.#binaryType === "arraybuffer" ? buffer : new Blob([buffer]);
            }
            this.#fire(new MessageEvent("message", { data, origin: new URL(this.url).origin }));
            break;
          }
          case "error":
            this.#fire(new Event("error"));
            break;
          case "close":
            this.#finish(message.code, message.reason);
            break;
        }
      }
      #fire(event) {
        const handler = this["on" + event.type];
        if (typeof handler === "function") {
          try {
            handler.call(this, event);
          } catch (error) {
            setTimeout(() => {
              throw error;
            });
          }
        }
        this.dispatchEvent(event);
      }
      #finish(code, reason) {
        if (this.readyState === states.CLOSED) return;
        clearInterval(this.#hello);
        this.readyState = states.CLOSED;
        this.#fire(new CloseEvent("close", { code: code || 1006, reason: reason || "", wasClean: code === 1000 }));
        try {
          this.#port.disconnect();
        } catch {}
      }
    }
    Object.assign(WebSocket, states);
    Object.assign(WebSocket.prototype, states);
    g.WebSocket = WebSocket;
  }
})();
