(() => {
  "use strict";

  const installed = Symbol.for("zephium.webkit-web-navigation.v1");
  if (globalThis[installed] === true) return;

  const CHANNEL = "zephium.webkit-web-navigation.v1";
  const DOM_EVENT = "zephium-webkit-same-document-navigation-v1";
  const api = globalThis.browser ?? globalThis.chrome;
  const runtime = api?.runtime;
  if (runtime?.id == null) {
    throw new Error("Zephium WebKit navigation bridge has no extension runtime");
  }

  const isExtensionExecutionContext = () => {
    if (typeof document === "undefined") return true;
    if (typeof runtime.getURL !== "function") return false;
    try {
      const extensionRoot = new URL(runtime.getURL("/"));
      return extensionRoot.origin === location.origin;
    } catch (_) {
      return false;
    }
  };

  const installMarker = () => {
    Object.defineProperty(globalThis, installed, {
      value: true,
      writable: false,
      enumerable: false,
      configurable: false,
    });
  };

  if (isExtensionExecutionContext()) {
    const webNavigation = api?.webNavigation;
    if (webNavigation == null || runtime.onMessage == null) {
      throw new Error("Zephium WebKit navigation bridge has no native event surface");
    }

    const createEvent = () => {
      const listeners = new Set();
      const event = Object.freeze({
        addListener(listener) {
          if (typeof listener !== "function") throw new TypeError("listener must be a function");
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
      return {
        event,
        dispatch(details) {
          for (const listener of Array.from(listeners)) {
            try {
              listener(details);
            } catch (error) {
              console.error("Zephium WebKit navigation listener failed", error);
            }
          }
        },
      };
    };

    const inertEvent = Object.freeze({
      addListener(listener) {
        if (typeof listener !== "function") throw new TypeError("listener must be a function");
      },
      removeListener() {},
      hasListener() {
        return false;
      },
      hasListeners() {
        return false;
      },
    });

    const installInertEvent = (name) => {
      if (webNavigation[name] != null) return;
      Object.defineProperty(webNavigation, name, {
        value: inertEvent,
        writable: false,
        enumerable: true,
        configurable: false,
      });
      if (webNavigation[name] !== inertEvent) {
        throw new Error(`Zephium WebKit navigation event ${name} was not installed`);
      }
    };

    const installEvent = (name) => {
      if (webNavigation[name] != null) return null;
      const synthetic = createEvent();
      Object.defineProperty(webNavigation, name, {
        value: synthetic.event,
        writable: false,
        enumerable: true,
        configurable: false,
      });
      if (webNavigation[name] !== synthetic.event) {
        throw new Error(`Zephium WebKit navigation event ${name} was not installed`);
      }
      return synthetic;
    };

    const historyState = installEvent("onHistoryStateUpdated");
    const referenceFragment = installEvent("onReferenceFragmentUpdated");
    installInertEvent("onCreatedNavigationTarget");
    const retainedEvents = Object.freeze([
      ["onHistoryStateUpdated", webNavigation.onHistoryStateUpdated],
      ["onReferenceFragmentUpdated", webNavigation.onReferenceFragmentUpdated],
      ["onCreatedNavigationTarget", webNavigation.onCreatedNavigationTarget],
    ]);
    const reconcileEvents = () => {
      const namespaces = [...new Set([globalThis.chrome, globalThis.browser])].filter(
        (namespace) => namespace?.runtime?.id != null && namespace?.webNavigation != null,
      );
      if (namespaces.length === 0) {
        throw new Error("Zephium WebKit navigation bridge lost its native event surface");
      }
      for (const namespace of namespaces) {
        for (const [name, event] of retainedEvents) {
          if (namespace.webNavigation[name] == null) {
            Object.defineProperty(namespace.webNavigation, name, {
              value: event,
              writable: false,
              enumerable: true,
              configurable: false,
            });
          }
          if (namespace.webNavigation[name] == null) {
            throw new Error(`Zephium WebKit navigation event ${name} was not reconciled`);
          }
        }
      }
    };
    // Rebind at bounded event-loop frontiers after vendor polyfills replace
    // nested namespace objects. Every task is one-shot; no polling remains.
    queueMicrotask(reconcileEvents);
    setTimeout(reconcileEvents, 0);
    if (historyState != null || referenceFragment != null) {
      runtime.onMessage.addListener((message, sender) => {
        if (message == null || typeof message !== "object" || Array.isArray(message)) return;
        if (message.channel !== CHANNEL) return;
        if (message.kind !== "history-state" && message.kind !== "reference-fragment") return;
        if (typeof message.url !== "string" || message.url.length === 0 || message.url.length > 8192) return;
        if (!Number.isSafeInteger(sender?.tab?.id) || sender.tab.id < 0) return;
        if (!Number.isSafeInteger(sender?.frameId) || sender.frameId < 0) return;

        let url;
        try {
          url = new URL(message.url);
        } catch (_) {
          return;
        }
        if (!(url.protocol === "http:" || url.protocol === "https:"
            || (url.protocol === "about:" && (url.href === "about:blank" || url.href === "about:srcdoc")))) {
          return;
        }

        const target = message.kind === "history-state" ? historyState : referenceFragment;
        if (target == null) return;
        const details = {
          tabId: sender.tab.id,
          frameId: sender.frameId,
          url: url.href,
          timeStamp: Date.now(),
          transitionType: "link",
          transitionQualifiers: [],
        };
        if (typeof sender.documentId === "string" && sender.documentId.length <= 128) {
          details.documentId = sender.documentId;
        }
        target.dispatch(Object.freeze(details));
      });
    }
    installMarker();
    return;
  }

  let previousUrl = location.href;
  let scheduled = false;
  const withoutHash = (value) => {
    const parsed = new URL(value);
    parsed.hash = "";
    return parsed.href;
  };
  const report = () => {
    scheduled = false;
    const nextUrl = location.href;
    if (nextUrl === previousUrl) return;
    const kind = withoutHash(previousUrl) === withoutHash(nextUrl)
      ? "reference-fragment"
      : "history-state";
    previousUrl = nextUrl;
    try {
      const result = runtime.sendMessage({ channel: CHANNEL, kind, url: nextUrl });
      result?.catch?.(() => {});
    } catch (_) {}
  };
  const schedule = () => {
    if (scheduled) return;
    scheduled = true;
    queueMicrotask(report);
  };
  addEventListener(DOM_EVENT, schedule, true);
  addEventListener("hashchange", schedule, true);
  addEventListener("popstate", schedule, true);
  installMarker();
})();
