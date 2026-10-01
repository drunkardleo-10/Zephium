(() => {
  if (window.top !== window) return;
  let loaded = 0;
  let failed = 0;
  const stylesheet = (target) => target instanceof HTMLLinkElement && target.relList.contains('stylesheet');
  document.addEventListener('load', (event) => {
    if (stylesheet(event.target)) loaded = Math.min(loaded + 1, 255);
  }, true);
  document.addEventListener('error', (event) => {
    if (stylesheet(event.target)) failed = Math.min(failed + 1, 255);
  }, true);
  Object.defineProperty(window, '__zephiumStartupStyles', { value: () => {
    const links = [...document.querySelectorAll('link[rel~="stylesheet"]')];
    const body = document.body;
    const shell = document.querySelector('.shell');
    const bodyStyle = body ? getComputedStyle(body) : null;
    const shellStyle = shell ? getComputedStyle(shell) : null;
    return JSON.stringify({
      loaded, failed, expected: Math.min(links.length, 255),
      sheets: links.slice(0, 64).map(link => {
        let rules = false;
        let inaccessible = false;
        try { rules = !!link.sheet?.cssRules.length; } catch { inaccessible = true; }
        return { ready: !!link.sheet, disabled: !!link.disabled, rules, inaccessible };
      }),
      token: !!bodyStyle?.getPropertyValue('--color-text').trim(),
      flex: shellStyle?.display === 'flex',
      overflow_hidden: bodyStyle?.overflow === 'hidden',
      sized: !!shell && shell.getBoundingClientRect().height >= window.innerHeight - 1,
      visible: document.visibilityState === 'visible',
      complete: document.readyState === 'complete',
    });
  }});
})();
