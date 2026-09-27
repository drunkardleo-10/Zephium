// Evaluated only in Zephium's inert, controller-free page at one exact
// webkit-extension origin. The page loads no publisher bytes or scripts.
const fail = (reason) => { throw new Error(reason); };
localStorage.clear();
sessionStorage.clear();
if (localStorage.length || sessionStorage.length) fail('dom-storage-remains');

const cacheNames = await caches.keys();
if (cacheNames.length > 512) fail('too-many-caches');
for (const name of cacheNames) {
  if (!await caches.delete(name)) fail('cache-delete-failed');
}
if ((await caches.keys()).length) fail('cache-remains');

if (typeof indexedDB.databases !== 'function') fail('database-list-unavailable');
const databases = await indexedDB.databases();
if (databases.length > 512) fail('too-many-databases');
for (const {name} of databases) {
  if (typeof name !== 'string') fail('unnamed-database');
  await new Promise((resolve, reject) => {
    const request = indexedDB.deleteDatabase(name);
    request.onsuccess = resolve;
    request.onerror = () => reject(request.error || Error('database-delete-failed'));
    request.onblocked = () => reject(Error('database-delete-blocked'));
  });
}
if ((await indexedDB.databases()).length) fail('database-remains');

if (typeof navigator.storage?.getDirectory === 'function') {
  const root = await navigator.storage.getDirectory();
  let count = 0;
  for await (const [name] of root.entries()) {
    if (++count > 512) fail('too-many-opfs-entries');
    await root.removeEntry(name, {recursive: true});
  }
  for await (const _ of root.entries()) fail('opfs-remains');
}

if (navigator.serviceWorker?.getRegistrations) {
  const registrations = await navigator.serviceWorker.getRegistrations();
  if (registrations.length > 512) fail('too-many-service-workers');
  for (const registration of registrations) {
    if (!await registration.unregister()) fail('service-worker-unregister-failed');
  }
  if ((await navigator.serviceWorker.getRegistrations()).length) fail('service-worker-remains');
}

// Cookies are checked separately through WKHTTPCookieStore. A custom-scheme
// document cannot see HttpOnly cookies, so document.cookie is not evidence of
// complete cookie erasure.
return 'erased';
