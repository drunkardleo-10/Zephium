importScripts('action-observer.js');
const started = Date.now();
const attempt = async fn => { try { return await fn(); } catch (error) { return {error: String(error)}; } };
chrome.runtime.onMessage.addListener((message, sender, reply) => {
  (async () => {
    if (message.op === 'ping') return {started, senderTab: sender.tab, id: chrome.runtime.id};
    if (message.op === 'actionReports') return globalThis.zephiumLabActionReports;
    if (message.op === 'snapshot') return {
      started,
      tabs: await attempt(() => chrome.tabs.query({})),
      activeTabs: await attempt(() => chrome.tabs.query({active: true, currentWindow: true})),
      windows: await attempt(() => chrome.windows.getAll({populate: true})),
      permissions: await attempt(() => chrome.permissions.getAll()),
      title: await attempt(() => chrome.action.getTitle({})),
      badge: await attempt(() => chrome.action.getBadgeText({})),
      popup: await attempt(() => chrome.action.getPopup({})),
      clicks: await attempt(() => chrome.storage.local.get('clicks'))
    };
    if (message.op === 'action') {
      const result = {};
      result.title = await attempt(() => chrome.action.setTitle({title: 'Changed by worker'}));
      result.badge = await attempt(() => chrome.action.setBadgeText({text: '7'}));
      result.icon = await attempt(() => {
        const imageData = new ImageData(16, 16);
        imageData.data.fill(255);
        return chrome.action.setIcon({imageData});
      });
      return result;
    }
    if (message.op === 'inject') return attempt(() => chrome.scripting.executeScript({
      target: {tabId: message.tabId}, func: () => ({url: location.href, title: document.title})
    }));
    if (message.op === 'fetch') return attempt(async () => ({status: (await fetch(message.url)).status}));
    return {error: 'unknown operation'};
  })().then(reply, error => reply({error: String(error)}));
  return true;
});
chrome.action.onClicked.addListener(async tab => {
  const {clicks = []} = await chrome.storage.local.get('clicks');
  await chrome.storage.local.set({clicks: [...clicks, {tab, time: Date.now()}]});
});
