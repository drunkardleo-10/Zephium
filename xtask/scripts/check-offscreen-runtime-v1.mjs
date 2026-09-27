import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';

const source = readFileSync(resolve('crates/zephium-engine/src/platform/macos/extensions/offscreen_runtime_v1.js'), 'utf8')
  .replace('__ZEPHIUM_OFFSCREEN_EXTENSION_ID__', 'cccccccccccccccccccccccccccccccc')
  .replace('__ZEPHIUM_OFFSCREEN_BASE_URL__', 'webkit-extension://cccccccccccccccccccccccccccccccc/');
const sent = [];
const events = new Map();
class AudioContext {
  createBufferSource() {
    const listeners = new Map();
    return {
      start() {},
      addEventListener(name, callback) { listeners.set(name, callback); },
      finish() { listeners.get('ended')?.(); },
    };
  }
}
const page = {
  URL,
  TextEncoder,
  setTimeout,
  clearTimeout,
  AudioContext,
  addEventListener(name, callback) { events.set(name, callback); },
  webkit: { messageHandlers: { zephiumOffscreenRuntimeV1: {
    async postMessage(wire) {
      const message = JSON.parse(wire);
      sent.push(message);
      return JSON.stringify({ v: 1, ok: true, value: message.operation === 'runtime.sendMessage'
        ? (message.value?.unhandled ? { handled: false } : { handled: true, value: message.value })
        : message.value });
    },
  } } },
};
page.globalThis = page;
vm.runInNewContext(source, page, { timeout: 1000 });

assert.equal(page.chrome.runtime.id, 'cccccccccccccccccccccccccccccccc');
assert.equal(page.chrome, page.browser);
assert.equal(page.chrome.storage, undefined);
assert.equal(page.chrome.tabs, undefined);
assert.equal(page.chrome.offscreen, undefined);
assert.equal(page.chrome.runtime.getURL('document.html'),
  'webkit-extension://cccccccccccccccccccccccccccccccc/document.html');
for (const path of ['../foreign.html', '/foreign.html', '//foreign.invalid/x', 'a%2fb', 'a\\b']) {
  assert.throws(() => page.chrome.runtime.getURL(path), `accepted ${path}`);
}

const reply = await page.chrome.runtime.sendMessage({ operation: 'probe' });
assert.equal(reply.operation, 'probe');
assert.equal(await page.chrome.runtime.sendMessage({ unhandled: true }), undefined);
assert.equal(sent.at(-1).operation, 'runtime.sendMessage');
let observed;
const echo = (message, sender) => {
  observed = sender.id;
  return { echoed: message.value };
};
page.chrome.runtime.onMessage.addListener(echo);
const incoming = await page.__zephiumOffscreenRuntimeV1.dispatch({ value: 42 });
assert.deepEqual(JSON.parse(incoming), { handled: true, value: { echoed: 42 } });
assert.equal(observed, page.chrome.runtime.id);
page.chrome.runtime.onMessage.removeListener(echo);
const slow = (_, __, respond) => { setTimeout(() => respond('slow'), 20); return true; };
const fast = () => Promise.resolve('fast');
page.chrome.runtime.onMessage.addListener(slow);
page.chrome.runtime.onMessage.addListener(fast);
const first = await page.__zephiumOffscreenRuntimeV1.dispatch({ value: 43 });
assert.deepEqual(JSON.parse(first), { handled: true, value: 'fast' });
page.chrome.runtime.onMessage.removeListener(slow);
page.chrome.runtime.onMessage.removeListener(fast);

const context = new page.AudioContext();
const sourceNode = context.createBufferSource();
sourceNode.start();
await Promise.resolve();
assert.equal(sent.at(-1).operation, 'audio.activity');
assert.equal(sent.at(-1).value.active, true);
sourceNode.finish();
await Promise.resolve();
assert.equal(sent.at(-1).value.active, false);

page.__zephiumOffscreenRuntimeV1.close();
await assert.rejects(page.chrome.runtime.sendMessage({ afterClose: true }));
console.log('offscreen runtime facade: bounded runtime messaging, isolation and audio events passed');
