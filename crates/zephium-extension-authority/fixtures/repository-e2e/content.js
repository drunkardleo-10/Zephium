const marker = "data-zephium-extension-product-probe";
const message = {
  kind: "zephium-product-probe",
  pageTitle: document.title,
  pageUrl: globalThis.location.href,
};

function settle(response) {
  const value =
    response?.state === "ready" && response?.count === 1
      ? "ready:1"
      : `invalid-response:${response?.state ?? "missing"}`;
  document.documentElement.setAttribute(marker, value);
}

function fail(channel, error) {
  const detail = String(error?.message ?? error ?? "unknown")
    .replace(/[^A-Za-z0-9 .:_/-]/g, "?")
    .slice(0, 96);
  document.documentElement.setAttribute(marker, `${channel}:${detail}`);
}

if (globalThis.browser?.runtime) {
  globalThis.browser.runtime.sendMessage(message).then(settle, (error) => {
    fail("promise-message-failed", error);
  });
} else {
  globalThis.chrome.runtime.sendMessage(message, (response) => {
    if (globalThis.chrome.runtime.lastError) {
      fail("callback-message-failed", globalThis.chrome.runtime.lastError);
      return;
    }
    settle(response);
  });
}
