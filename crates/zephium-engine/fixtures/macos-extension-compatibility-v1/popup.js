const api = globalThis.browser ?? globalThis.chrome;
const state = document.querySelector("#state");
const token = "zephium-popup-background-lifecycle-v1";

try {
  const response = await api.runtime.sendMessage({
    kind: "popup-background-lifecycle",
    token,
  });
  const passed = response?.kind === "popup-background-ready"
    && response?.token === token
    && response?.async === true;
  state.textContent = passed ? "passed" : "invalid-response";
  document.title = passed
    ? "ZEPHIUM_COMPAT_POPUP:passed"
    : "ZEPHIUM_COMPAT_POPUP:invalid-response";
} catch (_) {
  state.textContent = "rejected";
  document.title = "ZEPHIUM_COMPAT_POPUP:rejected";
}
