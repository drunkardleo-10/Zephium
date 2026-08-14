const api = globalThis.browser ?? globalThis.chrome;
const status = document.getElementById("status");

async function provePopupExecution() {
  const tabs = await api.tabs.query({ active: true, currentWindow: true });
  if (tabs.length !== 1 || typeof tabs[0]?.id !== "number") {
    throw new Error("active product tab unavailable");
  }
  await api.tabs.sendMessage(tabs[0].id, {
    kind: "zephium-product-probe-popup-ready",
  });
  status.textContent = "Authenticated extension popup ready";
  document.documentElement.setAttribute("data-zephium-popup-ready", "true");
}

void provePopupExecution().catch((error) => {
  const detail = String(error?.message ?? error ?? "unknown")
    .replace(/[^A-Za-z0-9 .:_/-]/g, "?")
    .slice(0, 72);
  status.textContent = "Extension popup unavailable";
  document.documentElement.setAttribute("data-zephium-popup-error", detail);
});
