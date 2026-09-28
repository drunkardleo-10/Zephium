(() => {
  const binding = __ZEPHIUM_BINDING__;
  if (location.protocol !== 'chrome-extension:' || location.host !== binding.extensionId ||
      !Number.isInteger(binding.tabId) || !Number.isInteger(binding.windowId)) return;
  const original = chrome.tabs.query;
  chrome.tabs.query = function (query, ...rest) {
    if (query?.active === true && query.windowId === undefined) {
      query = {...query, windowId: binding.windowId};
      delete query.currentWindow;
      delete query.lastFocusedWindow;
    }
    return Reflect.apply(original, this, [query, ...rest]);
  };
})();
