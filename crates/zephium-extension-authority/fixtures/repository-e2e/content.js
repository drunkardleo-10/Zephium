const api = globalThis.browser ?? globalThis.chrome;
const marker = "data-zephium-extension-product-probe";
const popupMarker = "data-zephium-extension-popup-probe";
let popupExecutions = 0;

function settle(message) {
  const value =
    message?.state === "ready" && message?.count === 1
      ? "ready:1"
      : `invalid-response:${message?.state ?? "missing"}`;
  document.documentElement.setAttribute(marker, value);
}

api.runtime.onMessage.addListener((message) => {
  if (message?.kind === "zephium-product-probe-result") {
    settle(message);
  } else if (message?.kind === "zephium-product-probe-popup-ready") {
    popupExecutions += 1;
    document.documentElement.setAttribute(popupMarker, `ready:${popupExecutions}`);
  }
  return undefined;
});

// The product probe invokes the native action only after this listener is
// installed, so a fast service-worker response cannot race page readiness.
document.documentElement.setAttribute(marker, "armed");
