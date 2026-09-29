import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {test} from 'node:test';
import vm from 'node:vm';

const source = readFileSync(new URL('./action-host.js', import.meta.url), 'utf8');
const settle = () => new Promise(resolve => setImmediate(resolve));

function fixture() {
  const queries = [], messages = [], window = {};
  const chrome = {
    runtime: {getManifest: () => ({name: 'Fixture'}), sendMessage: async () => null,
      onMessage: {addListener() {}}},
    tabs: {query: query => new Promise((resolve, reject) => queries.push({query, resolve, reject}))},
    action: {getTitle: async () => 'Fixture', getBadgeText: async () => '',
      getPopup: async () => '', isEnabled: async () => true},
    webview: {postMessage: message => messages.push(JSON.parse(message))}
  };
  // The fixture declares no icon. A minimal canvas suffices for this path.
  const document = {createElement: () => ({getContext: () => ({})})};
  vm.runInNewContext(source, {chrome, window, document});
  return {queries, messages, window};
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
