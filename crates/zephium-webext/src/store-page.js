// Zephium installs from its own reviewed button, so the store's "Add to Chrome"
// only confuses: WebKit shows it disabled, WebView2 leaves it live. The store
// also asks other browsers to switch to Chrome, in a banner over listings and a
// dialog under the header. All three are hidden by their controllers rather
// than translated labels, which covers other languages and navigation between
// listings.
// Requalify when the store changes its markup.
(() => {
  if (window !== top || location.protocol !== 'https:' ||
      location.hostname !== 'chromewebstore.google.com' || location.port) return;
  const apply = () => {
    const style = document.createElement('style');
    style.textContent = '[jscontroller="ri2s0b"], [jscontroller="o2G9me"], [jscontroller="h4ilFc"] { display: none !important; }';
    (document.head || document.documentElement).appendChild(style);
  };
  if (document.documentElement) apply();
  else document.addEventListener('DOMContentLoaded', apply, {once: true});
})();
