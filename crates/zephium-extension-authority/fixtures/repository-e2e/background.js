const api = globalThis.browser ?? globalThis.chrome;
const storageKey = "zephiumProductProbeState";

api.runtime.onMessage.addListener(async (message) => {
  if (message?.kind !== "zephium-product-probe") {
    return undefined;
  }

  const before = await api.storage.local.get(null);
  const beforeKeys = Object.keys(before).sort();
  const previous = before[storageKey];
  if (
    beforeKeys.length > 1 ||
    (beforeKeys.length === 1 && beforeKeys[0] !== storageKey)
  ) {
    return { state: "unexpected-existing-storage" };
  }

  const value = { count: 1, schema: 1 };
  if (
    previous !== undefined &&
    (previous?.count !== value.count || previous?.schema !== value.schema)
  ) {
    return { state: "invalid-existing-storage" };
  }
  await api.storage.local.set({ [storageKey]: value });
  const after = await api.storage.local.get(null);
  const keys = Object.keys(after).sort();
  const stored = after[storageKey];
  if (
    keys.length !== 1 ||
    keys[0] !== storageKey ||
    stored?.count !== value.count ||
    stored?.schema !== value.schema
  ) {
    return { state: "storage-readback-failed" };
  }

  const tabs = await api.tabs.query({ active: true, currentWindow: true });
  if (tabs.length !== 1) {
    return { state: "tabs-query-count-failed" };
  }
  const [tab] = tabs;
  if (tab.active !== true) {
    return { state: "tabs-query-active-failed" };
  }
  if (tab.pinned !== false) {
    return { state: "tabs-query-pinned-failed" };
  }
  if (tab.title !== message.pageTitle) {
    return { state: "tabs-query-title-failed" };
  }
  if (tab.url !== message.pageUrl) {
    return { state: "tabs-query-url-failed" };
  }

  return { count: value.count, state: "ready" };
});
