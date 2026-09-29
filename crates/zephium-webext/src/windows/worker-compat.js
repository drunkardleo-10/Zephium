// WebView2 creates native tabs but may omit the tabs.create result. Keep native
// ownership and events; pair pending creates FIFO within this worker instance.
(() => {
  const tabs = chrome.tabs;
  if (!tabs?.create || !tabs.onCreated) return;
  const create = tabs.create;
  const pending = [];
  const onCreated = tab => {
    const request = pending.find(request => !request.tab);
    if (request && Number.isInteger(tab?.id)) {
      request.tab = tab;
      request.finish();
    }
  };
  tabs.create = function (properties, callback) {
    const promise = new Promise((resolve, reject) => {
      const request = {tab: null, nativeDone: false, finish: null};
      const remove = () => {
        clearTimeout(timer);
        const index = pending.indexOf(request);
        if (index !== -1) pending.splice(index, 1);
        if (!pending.length) tabs.onCreated.removeListener(onCreated);
      };
      request.finish = () => {
        if (!request.nativeDone || !request.tab) return;
        remove(); resolve(request.tab);
      };
      const timer = setTimeout(() => { remove(); reject(new Error('Native tab creation timed out')); }, 15000);
      if (!pending.length) tabs.onCreated.addListener(onCreated);
      pending.push(request);
      try {
        Reflect.apply(create, this, [properties, tab => {
          const error = chrome.runtime.lastError;
          if (error) { remove(); reject(new Error(error.message)); return; }
          request.nativeDone = true;
          if (Number.isInteger(tab?.id)) request.tab = tab;
          request.finish();
        }]);
      } catch (error) { remove(); reject(error); }
    });
    if (typeof callback !== 'function') return promise;
    promise.then(callback, error => {
      // Match the callback API's synchronous lastError lifetime.
      const descriptor = Object.getOwnPropertyDescriptor(chrome.runtime, 'lastError');
      try {
        Object.defineProperty(chrome.runtime, 'lastError', {configurable: true, value: {message: error.message}});
        callback();
      } finally {
        if (descriptor) Object.defineProperty(chrome.runtime, 'lastError', descriptor);
        else delete chrome.runtime.lastError;
      }
    });
  };
})();

// The host relays an explicit toolbar click through our own extension page.
// Native listeners stay registered so any future native action route still works.
(() => {
  const event = chrome.action?.onClicked;
  if (!event) return;
  const listeners = new Set();
  const add = event.addListener, remove = event.removeListener;
  event.addListener = function (listener) {
    Reflect.apply(add, this, [listener]); listeners.add(listener);
  };
  event.removeListener = function (listener) {
    Reflect.apply(remove, this, [listener]); listeners.delete(listener);
  };
  chrome.runtime.onMessage.addListener((message, sender, reply) => {
    if (sender.id !== chrome.runtime.id ||
        sender.url !== chrome.runtime.getURL('zephium-windows-host/host.html') ||
        message?.__zephiumActionClick !== true || !Number.isInteger(message.tabId)) return;
    (async () => {
      const tab = await chrome.tabs.get(message.tabId);
      if (!await chrome.action.isEnabled(tab.id) || await chrome.action.getPopup({tabId: tab.id}))
        throw new Error('Action changed before click');
      for (const listener of [...listeners]) {
        try { Promise.resolve(listener(tab)).catch(() => {}); } catch { /* Other listeners still run. */ }
      }
      return {dispatched: true};
    })().then(reply, error => reply({error: String(error)}));
    return true;
  });
})();
