// Loaded only in an owned, hidden extension page. Chromium remains the API owner.
(() => {
  const manifest = chrome.runtime.getManifest();
  const action = chrome.action;
  let windowId;
  let running = false;
  let queued = false;
  const iconPath = value => typeof value === 'string' ? value : value?.[32] || value?.[48] || value?.[16];
  async function pixels(icon) {
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = 32;
    const context = canvas.getContext('2d');
    if (icon?.data && icon.width > 0 && icon.height > 0 && icon.width <= 128 && icon.height <= 128 && icon.data.length === icon.width * icon.height * 4) {
      const source = document.createElement('canvas');
      source.width = icon.width; source.height = icon.height;
      source.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(icon.data), icon.width, icon.height), 0, 0);
      context.drawImage(source, 0, 0, 32, 32);
    } else {
      const path = icon?.path || iconPath(manifest.action?.default_icon) || iconPath(manifest.icons);
      if (!path) return null;
      const url = new URL(path, chrome.runtime.getURL('/'));
      if (url.protocol !== 'chrome-extension:' || url.host !== chrome.runtime.id) return null;
      const response = await fetch(url);
      const blob = await response.blob();
      if (blob.size > 1048576) return null;
      const image = await createImageBitmap(blob, {resizeWidth: 32, resizeHeight: 32});
      context.drawImage(image, 0, 0, 32, 32); image.close();
    }
    return Array.from(context.getImageData(0, 0, 32, 32).data);
  }
  async function refresh() {
    if (!action || !Number.isInteger(windowId)) return;
    if (running) { queued = true; return; }
    running = true;
    try {
      const tabs = await chrome.tabs.query({windowId});
      if (tabs.length !== 1) throw new Error('Human controller identity unavailable');
      const tabId = tabs[0].id;
      const [title, badge, popup, enabled, report] = await Promise.all([
        action.getTitle({tabId}), action.getBadgeText({tabId}), action.getPopup({tabId}),
        action.isEnabled(tabId), chrome.runtime.sendMessage({__zephiumActionSnapshot: true, tabId}).catch(() => null)
      ]);
      let icon = null;
      try { icon = await pixels(report?.icon); } catch { /* Keep the native action without an icon. */ }
      chrome.webview.postMessage(JSON.stringify({kind: 'action', windowId, tabId,
        title: (title || manifest.name || '').slice(0, 256), badge: (badge || '').slice(0, 32),
        popup: (popup || '').slice(0, 2048), enabled, icon}));
    } catch (error) {
      chrome.webview.postMessage(JSON.stringify({kind: 'action-error', windowId, error: String(error).slice(0, 256)}));
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
    if (sender.id === chrome.runtime.id && message?.__zephiumActionChanged === true) void refresh();
  });
  chrome.webview.postMessage(JSON.stringify({kind: 'ready'}));
})();
