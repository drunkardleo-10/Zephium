chrome.runtime.sendMessage({op: "snapshot"}).then(result => {
  document.querySelector('#result').textContent = JSON.stringify(result);
});
document.querySelector('#request').addEventListener('click', async () => {
  try {
    const result = await chrome.permissions.request({origins: ['http://localhost/*']});
    document.querySelector('#result').textContent = JSON.stringify({permissionRequest: result});
  } catch (error) {
    document.querySelector('#result').textContent = JSON.stringify({error: String(error)});
  }
});
