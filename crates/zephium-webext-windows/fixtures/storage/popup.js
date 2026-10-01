async function show() {
  const data = await chrome.storage.local.get('qaValue');
  document.querySelector('#stored').textContent = data.qaValue ?? '(empty)';
}
document.querySelector('#save').addEventListener('click', async () => {
  await chrome.storage.local.set({qaValue: document.querySelector('#value').value});
  await show();
});
void show();
