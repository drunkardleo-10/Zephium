importScripts('compat.js');
const results = {created: [], clicks: []};
chrome.tabs.onCreated.addListener(tab => results.created.push(tab));
chrome.action.onClicked.addListener(tab => results.clicks.push(tab.id));
chrome.runtime.onMessage.addListener((message, sender, reply) => {
  if (message.op === 'results') { reply(results); return; }
  if (message.op !== 'create') return;
  if (message.callback) {
    chrome.tabs.create({url: message.url}, tab => reply({tab: tab || null, error: chrome.runtime.lastError?.message}));
  } else {
    chrome.tabs.create({url: message.url}).then(tab => reply({tab}), error => reply({error: String(error)}));
  }
  return true;
});
