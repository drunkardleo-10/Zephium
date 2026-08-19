(() => {
  "use strict";

  const installed = Symbol.for("zephium.webkit-options-page-compatibility.v1");
  const modeMarker = Symbol.for("zephium.webkit-options-page-compatibility.mode.v1");
  const applicationIdentifier = "app.zephium.extension-broker.v1";
  const operation = "v1/options.open";
  if (globalThis[installed] === true) return;

  const namespaces = [...new Set([globalThis.chrome, globalThis.browser])].filter(
    (namespace) => namespace?.runtime?.id != null,
  );
  const runtime = namespaces
    .map((namespace) => namespace.runtime)
    .find(
      (candidate) =>
        typeof candidate?.getURL === "function" &&
        typeof candidate?.sendNativeMessage === "function",
    );
  const descriptors = document.querySelectorAll('meta[name="zephium-extension-options-page"]');
  const path = descriptors.length === 1 ? descriptors[0].getAttribute("content") : null;
  if (runtime == null || path == null || path.length === 0 || path.length > 2048) {
    throw new Error("Zephium WebKit options-page compatibility is unavailable");
  }
  const expected = runtime.getURL(path);
  if (typeof expected !== "string" || !expected.startsWith("webkit-extension://")) {
    throw new Error("Zephium WebKit options-page identity is invalid");
  }
  const boundOptionsLinks = new WeakSet();

  const requestOptions = () => {
    runtime.sendNativeMessage(applicationIdentifier, operation, (encoded) => {
      if (typeof encoded !== "string" || encoded.length > 64) return;
      try {
        const response = JSON.parse(encoded);
        if (
          Object.keys(response).sort().join(",") !== "opened,v" ||
          response.v !== 1 ||
          typeof response.opened !== "boolean"
        ) {
          return;
        }
      } catch {
        // A malformed native settlement is already fail-closed.
      }
    });
  };
  const consumeOptionsActivation = (event, element) => {
    if (!(element instanceof HTMLAnchorElement) || !boundOptionsLinks.has(element)) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    // Preserve the exact authenticated href as an accessibility fallback.
    // WebKit can service AXPress as native link activation without delivering
    // the page's DOM listener. The controller delegate admits only this exact
    // context.optionsPageURL(), so a physical/DOM activation uses the narrow
    // broker while an accessibility-only activation reaches the same native
    // options surface through the URL path.
    if (element.href !== expected) return;
    try {
      requestOptions();
    } catch {
      // The page owns its own user-facing retry behavior.
    }
  };
  const openOptions = (event) => {
    if (event.button !== 0) return;
    const element = event.target instanceof Element ? event.target.closest("a") : null;
    consumeOptionsActivation(event, element);
  };
  const openOptionsFromKeyboard = (event) => {
    if (event.key !== "Enter" && event.key !== " ") return;
    consumeOptionsActivation(event, event.currentTarget);
  };
  document.addEventListener("click", openOptions, true);

  // WebKit's accessibility action for an extension link may target the
  // element without traversing the document listener. Bind the same exact,
  // URL-checked handler directly once the static extension page is parsed.
  let lateLinkObserver = null;
  let lateLinkExpiry = null;
  let observedMutations = 0;
  const stopLateLinkObservation = () => {
    lateLinkObserver?.disconnect();
    lateLinkObserver = null;
    if (lateLinkExpiry != null) clearTimeout(lateLinkExpiry);
    lateLinkExpiry = null;
  };
  const bindOptionsLink = (link) => {
    if (
      !(link instanceof HTMLAnchorElement) ||
      link.href !== expected ||
      boundOptionsLinks.has(link)
    ) {
      return false;
    }
    boundOptionsLinks.add(link);
    link.setAttribute("role", "button");
    link.setAttribute("tabindex", "0");
    link.removeAttribute("target");
    link.addEventListener("click", openOptions, true);
    link.addEventListener("keydown", openOptionsFromKeyboard, true);
    stopLateLinkObservation();
    return true;
  };
  const bindOptionsLinks = () => {
    const links = document.querySelectorAll("a[href]");
    if (links.length > 512) return false;
    for (const link of links) {
      if (bindOptionsLink(link)) return true;
    }
    return false;
  };
  const watchLateOptionsLink = () => {
    if (bindOptionsLinks() || typeof MutationObserver !== "function") return;
    lateLinkObserver = new MutationObserver((records) => {
      observedMutations += records.length;
      if (observedMutations > 512) {
        stopLateLinkObservation();
        return;
      }
      for (const record of records) {
        if (record.type === "attributes" && bindOptionsLink(record.target)) return;
      }
    });
    lateLinkObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["href"],
      subtree: true,
    });
    lateLinkExpiry = setTimeout(stopLateLinkObservation, 10_000);
  };
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", watchLateOptionsLink, { once: true });
  } else {
    watchLateOptionsLink();
  }

  Object.defineProperty(globalThis, modeMarker, {
    value: "runtime-open-options-page-window",
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
