const api = globalThis.browser ?? globalThis.chrome;
const storageKey = "zephiumProductProbeState";
const optionalAccess = {
  permissions: ["tabs"],
  origins: ["https://optional.example/*"],
};

function boundedDetail(error) {
  return String(error?.message ?? error ?? "unknown")
    .replace(/[^A-Za-z0-9 .:_/-]/g, "?")
    .slice(0, 72);
}

async function report(tabId, state, count) {
  try {
    await api.tabs.sendMessage(tabId, {
      kind: "zephium-product-probe-result",
      state,
      count,
    });
  } catch (error) {
    await api.action.setBadgeText({
      tabId,
      text: `ERR:${boundedDetail(error)}`.slice(0, 32),
    });
  }
}

async function runProductProbe(tab) {
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
  const [active] = tabs;
  if (active.id !== tab.id || active.active !== true) {
    return { state: "tabs-query-active-failed" };
  }
  if (active.pinned !== false) {
    return { state: "tabs-query-pinned-failed" };
  }
  if (active.title !== tab.title) {
    return { state: "tabs-query-title-failed" };
  }
  if (active.url !== tab.url) {
    return { state: "tabs-query-url-failed" };
  }

  const created = await api.tabs.create({ active: true });
  if (typeof created?.id !== "number" || created.active !== true) {
    return { state: "tabs-create-failed" };
  }
  const activated = await api.tabs.update(tab.id, {
    active: true,
    highlighted: true,
  });
  if (activated?.id !== tab.id || activated.active !== true) {
    return { state: "tabs-activate-failed" };
  }
  const updated = await api.tabs.update(created.id, { url: "about:blank" });
  if (updated?.id !== created.id) {
    return { state: "tabs-update-url-failed" };
  }
  await api.tabs.remove(created.id);

  const finalTabs = await api.tabs.query({ active: true, currentWindow: true });
  if (finalTabs.length !== 1 || finalTabs[0]?.id !== tab.id) {
    return { state: "tabs-remove-failed" };
  }
  return { count: value.count, state: "ready" };
}

api.action.onClicked.addListener(async (tab) => {
  if (typeof tab?.id !== "number") {
    return;
  }
  try {
    const granted = await api.permissions.request(optionalAccess);
    const contains = await api.permissions.contains(optionalAccess);
    if (granted !== true || contains !== true) {
      await report(tab.id, "optional-permission-denied");
      return;
    }
    const result = await runProductProbe(tab);
    if (result.state === "ready") {
      await api.action.setPopup({ popup: "popup.html" });
    }
    await report(tab.id, result.state, result.count);
  } catch (error) {
    await report(tab.id, `runtime-error:${boundedDetail(error)}`);
  }
});
