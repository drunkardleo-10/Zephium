(() => {
  const key = Symbol.for('zephium.main-document-globs.v1');
  if (globalThis[key]) return;
  globalThis[key] = true;
  chrome.runtime.sendMessage({
    kind: 'zephium-main-document-glob-v1',
    route: __ZEPHIUM_ROUTE__,
    url: location.href,
  }).catch(() => undefined);
})();
