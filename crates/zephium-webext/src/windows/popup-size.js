// Presentation only, in this owned popup's main frame. Never alters extension APIs.
(() => {
  if (window !== top) return;
  let last = '', queued = false;
  const measure = () => {
    queued = false;
    const root = document.documentElement, body = document.body;
    if (!root || !body) return;
    const rect = body.getBoundingClientRect(), style = getComputedStyle(body);
    const marginX = (parseFloat(style.marginLeft) || 0) + (parseFloat(style.marginRight) || 0);
    const marginY = (parseFloat(style.marginTop) || 0) + (parseFloat(style.marginBottom) || 0);
    // Root scroll extents include the viewport. Use them only for real overflow,
    // so a shorter/fixed-width body can shrink after asynchronous content changes.
    const width = Math.max(body.scrollWidth, rect.width) + marginX;
    const height = Math.max(body.scrollHeight, rect.height) + marginY;
    const size = [Math.ceil(Math.min(800, Math.max(25, width,
      root.scrollWidth > innerWidth ? root.scrollWidth : 0))),
      Math.ceil(Math.min(600, Math.max(25, height,
        root.scrollHeight > innerHeight ? root.scrollHeight : 0)))];
    const key = size.join(',');
    if (key === last) return;
    last = key;
    chrome.webview.postMessage(JSON.stringify({kind: 'popup-size', width: size[0], height: size[1]}));
  };
  const schedule = () => {
    if (!queued) { queued = true; requestAnimationFrame(measure); }
  };
  const start = () => {
    // The first read must not wait for rAF in a still-hidden native window.
    measure();
    const observer = new ResizeObserver(schedule);
    observer.observe(document.documentElement);
    if (document.body) observer.observe(document.body);
    addEventListener('pagehide', () => observer.disconnect(), {once: true});
    document.fonts?.ready.then(schedule);
  };
  window.__zephiumPopupShown = () => {
    if (!matchMedia('(prefers-reduced-motion: reduce)').matches) {
      document.documentElement.animate([{opacity: 0}, {opacity: 1}],
        {duration: 100, easing: 'ease-out'});
    }
  };
  if (document.readyState !== 'complete') addEventListener('load', start, {once: true});
  else start();
})();
