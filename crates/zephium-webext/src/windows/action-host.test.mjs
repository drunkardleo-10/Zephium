import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {test} from 'node:test';
import vm from 'node:vm';

const source = readFileSync(new URL('./action-host.js', import.meta.url), 'utf8');
const settle = () => new Promise(resolve => setImmediate(resolve));

function fixture({defaultIcon} = {}) {
  const queries = [], messages = [], window = {};
  const cost = {fetches: 0, decodes: 0, canvases: 0, closes: 0};
  const state = {icon: null, perTabIcons: false, failFetch: false, snapshots: 0, listener: null, report: null, title: 'Fixture'};
  const chrome = {
    runtime: {id: 'fixture', getURL: path => `chrome-extension://fixture${path}`,
      getManifest: () => ({name: 'Fixture', action: {default_icon: defaultIcon}}),
      sendMessage: async () => { state.snapshots++; return state.report ? await state.report() : {icon: state.icon, perTabIcons: state.perTabIcons}; },
      onMessage: {addListener: listener => { state.listener = listener; }}},
    tabs: {query: query => new Promise((resolve, reject) => queries.push({query, resolve, reject}))},
    action: {getTitle: async () => state.title, getBadgeText: async () => '',
      getPopup: async () => '', isEnabled: async () => true},
    webview: {postMessage: message => messages.push(JSON.parse(message))}
  };
  const document = {createElement: () => {
    cost.canvases++;
    const canvas = {pixel: 0};
    canvas.getContext = () => ({
      drawImage: image => { canvas.pixel = image.pixel; },
      putImageData: image => { canvas.pixel = image.data[0]; },
      getImageData: () => ({data: new Uint8ClampedArray(4096).fill(canvas.pixel)})
    });
    return canvas;
  }};
  const fetch = async url => {
    cost.fetches++;
    if (state.failFetch) throw new Error('Unavailable');
    return {blob: async () => ({size: 64, pixel: url.pathname === '/second.png' ? 2 : 1})};
  };
  const createImageBitmap = async blob => {
    cost.decodes++;
    return {pixel: blob.pixel, close: () => { cost.closes++; }};
  };
  const ImageData = class { constructor(data) { this.data = data; } };
  vm.runInNewContext(source, {chrome, window, document, fetch, createImageBitmap, URL, ImageData, Uint8ClampedArray});
  const refresh = async (id = 10) => {
    window.__zephiumRefresh(id);
    queries.at(-1).resolve([{id: id * 10}]);
    await settle();
    return messages.at(-1);
  };
  const changed = () => state.listener({__zephiumActionChanged: true}, {id: 'fixture'});
  const readChange = async () => {
    changed();
    queries.at(-1).resolve([{id: 100}]);
    await settle();
    return messages.at(-1);
  };
  return {queries, messages, window, cost, state, refresh, changed, readChange};
}

for (const fails of [false, true]) {
  test(`tab switching keeps ${fails ? 'failed' : 'successful'} snapshots bound to their original tab`, async () => {
    const {queries, messages, window} = fixture();
    window.__zephiumRefresh(10);
    window.__zephiumRefresh(20);
    window.__zephiumRefresh(30);
    assert.equal(queries.length, 1);
    assert.equal(queries[0].query.windowId, 10);
    if (fails) queries[0].reject(new Error('Retired controller'));
    else queries[0].resolve([{id: 100}]);
    await settle();
    const first = messages.find(message => message.kind !== 'ready');
    assert.equal(first.windowId, 10);
    assert.equal(first.kind, fails ? 'action-error' : 'action');
    if (!fails) assert.equal(first.tabId, 100);
    assert.equal(queries.length, 2);
    assert.equal(queries[1].query.windowId, 30);
    queries[1].resolve([{id: 300}]);
    await settle();
    assert.equal(messages.at(-1).windowId, 30);
    assert.equal(messages.at(-1).tabId, 300);
    assert.equal(queries.length, 2);
  });
}

test('unchanged package icons decode once across repeated refreshes and tab switches', async () => {
  const {refresh, cost} = fixture({defaultIcon: 'first.png'});
  for (let i = 0; i < 50; i++) {
    const snapshot = await refresh(10 + i % 2);
    assert.equal(snapshot.icon[0], 1);
    assert.equal(snapshot.tabId, (10 + i % 2) * 10);
  }
  assert.deepEqual(cost, {fetches: 1, decodes: 1, canvases: 1, closes: 1});
});

test('path changes replace the single entry and imageData remains live', async () => {
  const {refresh, state, cost, readChange} = fixture({defaultIcon: 'first.png'});
  assert.equal((await refresh()).icon[0], 1);
  state.icon = {path: 'second.png'};
  assert.equal((await readChange()).icon[0], 2);
  state.icon = {width: 1, height: 1, data: [3, 3, 3, 255]};
  assert.equal((await readChange()).icon[0], 3);
  state.icon.data[0] = 4;
  assert.equal((await readChange()).icon[0], 4);
  state.icon = null;
  assert.equal((await readChange()).icon[0], 1);
  assert.equal(cost.fetches, 3); // The old path was evicted, not retained per tab.
  assert.equal(cost.closes, cost.decodes);
});

test('failed icon reads retry and foreign URLs cannot reuse the cached icon', async () => {
  const {refresh, state, cost, readChange} = fixture({defaultIcon: 'first.png'});
  state.failFetch = true;
  assert.equal((await refresh()).icon, null);
  state.failFetch = false;
  assert.equal((await refresh()).icon[0], 1);
  for (const path of ['https://example.test/first.png', 'chrome-extension://other/first.png']) {
    state.icon = {path};
    assert.equal((await readChange()).icon, null);
  }
  assert.equal(cost.fetches, 2);
  assert.equal(cost.decodes, 1);
});

test('metadata and global-icon tab switches do not wake a worker; actual changes do', async () => {
  const {refresh, state, readChange} = fixture({defaultIcon: 'first.png'});
  await refresh();
  state.title = 'Native metadata remains live';
  for (let i = 0; i < 20; i++) assert.equal((await refresh()).title, state.title);
  assert.equal(state.snapshots, 1);
  await readChange();
  assert.equal(state.snapshots, 2);
  await refresh(20);
  assert.equal(state.snapshots, 2);
  state.perTabIcons = true;
  await readChange();
  await refresh(20);
  assert.equal(state.snapshots, 4);
});

test('a change during the worker response cannot leave the next icon stale', async () => {
  const {window, queries, state, messages, changed} = fixture({defaultIcon: 'first.png'});
  let finish;
  state.report = () => new Promise(resolve => { finish = resolve; });
  window.__zephiumRefresh(10);
  queries[0].resolve([{id: 100}]);
  await settle();
  changed();
  finish({icon: {path: 'first.png'}});
  state.report = null;
  state.icon = {path: 'second.png'};
  await settle();
  queries[1].resolve([{id: 100}]);
  await settle();
  assert.equal(state.snapshots, 2);
  assert.equal(messages.at(-1).icon[0], 2);
});

test('a failed worker snapshot retries and another extension cannot invalidate the cache', async () => {
  const {refresh, state, queries} = fixture({defaultIcon: 'first.png'});
  state.report = async () => { throw new Error('Worker restarting'); };
  await refresh();
  state.report = null;
  await refresh();
  assert.equal(state.snapshots, 2);
  const count = queries.length;
  state.listener({__zephiumActionChanged: true}, {id: 'other'});
  assert.equal(queries.length, count);
  await refresh();
  assert.equal(state.snapshots, 2);
});

test('a worker that keeps failing or a broken icon is not woken on every refresh', async () => {
  const {refresh, state} = fixture({defaultIcon: 'missing.png'});
  state.report = async () => { throw new Error('Worker gone'); };
  for (let i = 0; i < 10; i++) await refresh();
  assert.equal(state.snapshots, 2);
});

test('missing and broken package icons stay quiet across tab switches after bounded retries', async () => {
  for (const defaultIcon of [undefined, 'missing.png']) {
    const {refresh, state} = fixture({defaultIcon});
    state.failFetch = true;
    for (let i = 0; i < 50; i++) await refresh(10 + i % 2);
    assert.equal(state.snapshots, defaultIcon ? 2 : 1);
  }
});
