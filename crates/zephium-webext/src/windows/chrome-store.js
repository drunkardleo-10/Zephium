// The store's native install control does not install into Zephium's registry.
// Keep our sidebar's reviewed install flow as the single entry point. This is
// presentation only: it exposes no host command and grants no page authority.
(() => {
  if (window !== top || location.protocol !== 'https:' ||
      location.hostname !== 'chromewebstore.google.com' || location.port) return;
  const apply = () => {
    const style = document.createElement('style');
    // Store install-controller markup verified in the native lab. Unlike its
    // translated button text, this also covers non-English store listings and
    // SPA navigation between listings. Requalify if the store changes markup.
    style.textContent = '[jscontroller="ri2s0b"] { display: none !important; }';
    (document.head || document.documentElement).appendChild(style);
  };
  if (document.documentElement) apply();
  else document.addEventListener('DOMContentLoaded', apply, {once: true});
})();
