(() => {
  'use strict';
  const routes = __ZEPHIUM_GLOB_ROUTES__;
  function matchesGlob(pattern, value, questionWildcard = true) {
    let p = 0, v = 0, star = -1, retry = 0;
    while (v < value.length) {
      if (p < pattern.length && (pattern[p] === value[v] || (questionWildcard && pattern[p] === '?'))) {
        p++; v++; continue;
      }
      if (p < pattern.length && pattern[p] === '*') {
        star = p++; retry = v; continue;
      }
      if (star >= 0) {
        p = star + 1; v = ++retry; continue;
      }
      return false;
    }
    while (pattern[p] === '*') p++;
    return p === pattern.length;
  }
  function matchesPattern(pattern, url, path) {
    if (pattern.all) return true;
    const scheme = url.protocol.slice(0, -1);
    if (pattern.scheme !== scheme && !(pattern.scheme === 'web' && (scheme === 'http' || scheme === 'https'))) return false;
    const host = url.hostname;
    if (pattern.hostKind === 'none') return false;
    if (pattern.hostKind === 'exact' && host !== pattern.host) return false;
    if (pattern.hostKind === 'subdomains' && host !== pattern.host && !host.endsWith(`.${pattern.host}`)) return false;
    if (pattern.port !== null && Number(url.port || (scheme === 'https' ? 443 : 80)) !== pattern.port) return false;
    if (pattern.path.endsWith('/*') && path === pattern.path.slice(0, -2)) return true;
    return matchesGlob(pattern.path, path, false);
  }
  function matchesPrimary(route, url, path) {
    return route.primary.some(pattern => matchesPattern(pattern, url, path)) &&
      !route.primaryExcludes.some(pattern => matchesPattern(pattern, url, path));
  }
  function matchesRoute(route, url, rawUrl, path) {
    return matchesPrimary(route, url, path) &&
      (!route.include.length || route.include.some(pattern => matchesGlob(pattern, rawUrl))) &&
      !route.exclude.some(pattern => matchesGlob(pattern, rawUrl));
  }
  chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    if (message?.kind !== 'zephium-main-document-glob-v1') return false;
    const route = Number.isInteger(message.route) ? routes.find(row => row.index === message.route) : null;
    const tabId = sender?.tab?.id;
    const documentId = sender?.documentId;
    const rawUrl = sender?.url;
    let url;
    try { url = new URL(rawUrl); } catch { sendResponse({ok:false}); return false; }
    // URL.search drops an explicitly empty query, while the compiled native
    // matcher includes the '?' separator. URL path/query is ASCII-encoded.
    const path = url.pathname + (url.search || (url.href.includes('?') ? '?' : ''));
    if (!route || sender.frameId !== 0 || !Number.isInteger(tabId) ||
        typeof documentId !== 'string' || documentId.length < 16 || documentId.length > 128 ||
        typeof rawUrl !== 'string' || rawUrl !== message.url ||
        !['http:', 'https:'].includes(url.protocol) || url.username || url.password ||
        url.hash || path.length > 32768 || !matchesPrimary(route, url, path) ||
        (sender.origin && sender.origin !== url.origin) ||
        !routes.some(row => matchesRoute(row, url, rawUrl, path))) {
      sendResponse({ok:false});
      return false;
    }
    const selected = routes.filter(row => matchesRoute(row, url, rawUrl, path));
    (async () => {
      for (const row of selected) {
        const results = await chrome.scripting.executeScript({
          target:{tabId, documentIds:[documentId]}, files:row.files, world:'ISOLATED'
        });
        if (!Array.isArray(results) || results.length !== row.files.length ||
            !results.every(result => result.documentId === documentId && result.frameId === 0)) {
          return false;
        }
      }
      return true;
    })().then(ok => sendResponse({ok}), () => sendResponse({ok:false}));
    return true;
  });
})();
