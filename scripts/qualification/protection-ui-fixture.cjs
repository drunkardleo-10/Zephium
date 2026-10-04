// Local-only UI fixture. Run with Node; no CDP or browser automation bridge.
const http = require('node:http');
const strict = '<!doctype html><title>Protection UI strict CSP</title><h1>Protection UI fixture</h1><p>Useful content must remain visible.</p><div id="first"><h2>First target</h2><p>Click this box to hide it.</p></div><div id="second"><h2>Second target</h2><p>Hide this too, then Undo.</p></div><div id="third"><h2>Third target</h2><p>Hide and restart to verify persistence.</p></div>';
const server = http.createServer((request, response) => {
  const path = new URL(request.url, 'http://localhost').pathname;
  console.log(JSON.stringify({ at: Date.now(), path }));
  response.setHeader('Cache-Control', 'no-store');
  if (path === '/ads/cbr.js') {
    response.setHeader('Content-Type', 'application/javascript');
    response.end('document.getElementById("network").textContent="Ad script delivered";');
    return;
  }
  response.setHeader('Content-Type', 'text/html');
  if (path.startsWith('/clean')) {
    response.end('<!doctype html><meta charset="utf-8"><title>Cosmetics clean ' + path + '</title><h1>Clean measurement page</h1><p>No ad requests, no generic targets, no script activity.</p>');
    return;
  }
  if (path === '/cosmetic-pull') {
    response.end(`<!doctype html><meta charset="utf-8"><title>Cosmetic pull QA</title>
      <style>body{font:18px system-ui;padding:32px}button{font:inherit;padding:8px;margin:8px}</style>
      <h1>Cosmetic pull QA</h1><p id="useful">Useful content remains visible.</p>
      <div class="ad-slot" id="initial">Initial generic ad</div><p id="initial-status">Checking initial ad</p>
      <button id="late">Insert a late ad</button><button id="check">Check results</button>
      <p id="late-status">Late ad not inserted</p><p id="diagnostic"></p>
      <script>
        function check(){const api=globalThis.__zephium_content_style_v1__;document.getElementById('diagnostic').textContent='Token pull API: '+typeof api?.pullGeneric+'; subscription: '+(api?JSON.parse(api.inspectEncoded()).subscription:'missing')+'; sheets: '+document.adoptedStyleSheets.length;document.getElementById('initial-status').textContent='Initial ad: '+getComputedStyle(document.getElementById('initial')).display;
          const late=document.getElementById('late-ad'); if(late) document.getElementById('late-status').textContent='Late ad: '+getComputedStyle(late).display}
        document.getElementById('late').onclick=()=>{if(!document.getElementById('late-ad')){const ad=document.createElement('div');ad.className='google-ad';ad.id='late-ad';ad.textContent='Late generic ad';document.body.append(ad)}setTimeout(check,500)};
        document.getElementById('check').onclick=check;
        setTimeout(check,1500);
      </script>`);
    return;
  }

  if (path === '/strict') {
    response.setHeader('Content-Security-Policy', "script-src 'none'; style-src 'none'; require-trusted-types-for 'script'");
    response.end(strict);
    return;
  }
  response.end(strict.replace('strict CSP', 'network') + '<p id="network">Ad script blocked (or pending)</p><script src="/ads/cbr.js"></script>');
});
const host = process.env.HOST || '127.0.0.1';
if (!['127.0.0.1', '::1'].includes(host)) throw new Error('Fixture must stay on loopback');
server.listen(Number(process.env.PORT || 0), host, () => console.log(`FIXTURE http://${host === '::1' ? '[::1]' : host}:${server.address().port}`));
