document.documentElement.dataset.zephiumExtension = chrome.runtime.id;
chrome.runtime.sendMessage({op: "ping"}).then(result => {
  document.documentElement.dataset.zephiumWorker = JSON.stringify(result);
}).catch(error => {
  document.documentElement.dataset.zephiumWorkerError = String(error);
});
