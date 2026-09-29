// Loaded only in an owned, hidden extension page. Chromium remains the API owner.
(() => {
  const manifest = chrome.runtime.getManifest();
  const action = chrome.action;
  let windowId;
  let running = false;
  let queued = false;
  // Package files are immutable for this manager's lifetime. Retain only the
  // last decoded path, not a growing per-tab cache. setIcon(imageData) remains live.
  let cachedPath, cachedPixels;
  let iconDirty = true, iconTab, actionPixels = null;
  const iconPath = value => typeof value === 'string' ? value : value?.[32] || value?.[48] || value?.[16];
  async function pixels(icon) {
    const imageData = icon?.data && icon.width > 0 && icon.height > 0 && icon.width <= 128 && icon.height <= 128 && icon.data.length === icon.width * icon.height * 4;
    let url;
    if (!imageData) {
      const path = icon?.path || iconPath(manifest.action?.default_icon) || iconPath(manifest.icons);
      if (!path) return null;
      url = new URL(path, chrome.runtime.getURL('/'));
      if (url.protocol !== 'chrome-extension:' || url.host !== chrome.runtime.id) return null;
      if (url.href === cachedPath) return cachedPixels;
    }
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = 32;
    const context = canvas.getContext('2d');
    if (imageData) {
      const source = document.createElement('canvas');
      source.width = icon.width; source.height = icon.height;
      source.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(icon.data), icon.width, icon.height), 0, 0);
      context.drawImage(source, 0, 0, 32, 32);
    } else {
      const response = await fetch(url);
      const blob = await response.blob();
      if (blob.size > 1048576) return null;
      const image = await createImageBitmap(blob, {resizeWidth: 32, resizeHeight: 32});
      try { context.drawImage(image, 0, 0, 32, 32); } finally { image.close(); }
    }
    const result = Array.from(context.getImageData(0, 0, 32, 32).data);
    if (url) { cachedPath = url.href; cachedPixels = result; }
    return result;
  }
  async function refresh() {
    if (!action || !Number.isInteger(windowId)) return;
    if (running) { queued = true; return; }
    running = true;
    // Native queries can finish after the human switches tabs. Tag the whole
    // snapshot with its original binding so the host can reject stale results.
    const requestedWindowId = windowId;
    try {
      const tabs = await chrome.tabs.query({windowId: requestedWindowId});
      if (tabs.length !== 1) throw new Error('Human controller identity unavailable');
      const tabId = tabs[0].id;
      // Native metadata reads need no worker RPC. Reuse the last raster until
      // its tab changes or the observer reports a change, allowing idle workers
      // to sleep between real events. Clear before awaiting so a concurrent
      // notification still invalidates the next queued snapshot.
      const readIcon = iconDirty || iconTab !== tabId;
      iconDirty = false;
      const [title, badge, popup, enabled, report] = await Promise.all([
        action.getTitle({tabId}), action.getBadgeText({tabId}), action.getPopup({tabId}),
        action.isEnabled(tabId), readIcon
          ? chrome.runtime.sendMessage({__zephiumActionSnapshot: true, tabId}).catch(() => { iconDirty = true; return null; })
          : null
      ]);
      if (readIcon) {
        actionPixels = null;
        try { actionPixels = await pixels(report?.icon); } catch { iconDirty = true; }
        iconTab = tabId;
      }
      chrome.webview.postMessage(JSON.stringify({kind: 'action', windowId: requestedWindowId, tabId,
        title: (title || manifest.name || '').slice(0, 256), badge: (badge || '').slice(0, 32),
        popup: (popup || '').slice(0, 2048), enabled, icon: actionPixels}));
    } catch (error) {
      iconDirty = true;
      chrome.webview.postMessage(JSON.stringify({kind: 'action-error', windowId: requestedWindowId, error: String(error).slice(0, 256)}));
    } finally {
      running = false;
      if (queued) { queued = false; void refresh(); }
    }
  }
  window.__zephiumRefresh = id => { if (Number.isInteger(id)) { windowId = id; void refresh(); } };
  window.__zephiumClick = tabId => {
    if (!Number.isInteger(tabId)) return;
    chrome.runtime.sendMessage({__zephiumActionClick: true, tabId})
      .then(() => refresh(), () => refresh());
  };
  chrome.runtime.onMessage.addListener((message, sender) => {
    if (sender.id === chrome.runtime.id && message?.__zephiumActionChanged === true) {
      iconDirty = true;
      void refresh();
    }
  });
  chrome.webview.postMessage(JSON.stringify({kind: 'ready'}));
})();
