// Zephium installs from its own reviewed button, so the store's "Add to Chrome"
// only confuses: WebKit shows it disabled, WebView2 leaves it live. Hidden by
// the install controller rather than its translated label, which also covers
// other languages and navigation between listings.
// Requalify when the store changes its markup.
(() => {
  if (window !== top || location.protocol !== 'https:' ||
      location.hostname !== 'chromewebstore.google.com' || location.port) return;
  const apply = () => {
    const style = document.createElement('style');
    style.textContent = '[jscontroller="ri2s0b"] { display: none !important; }';
    (document.head || document.documentElement).appendChild(style);
  };
  if (document.documentElement) apply();
  else document.addEventListener('DOMContentLoaded', apply, {once: true});
})();
