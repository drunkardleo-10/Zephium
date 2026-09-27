import assert from 'node:assert/strict';
import vm from 'node:vm';
import { readFile } from 'node:fs/promises';
const source = await readFile(new URL('../../crates/zephium-extension-package/assets/macos/webkit-managed-storage-v2.js', import.meta.url), 'utf8');
const tasks = [];
const api = {runtime: {id: 'fixture'}, storage: {local: {sentinel: true}}};
const context = vm.createContext({chrome: api, queueMicrotask, setTimeout: callback => tasks.push(callback)});
vm.runInContext(source, context, {timeout: 1000});
const managed = api.storage.managed;
const plain = value => JSON.parse(JSON.stringify(value));
assert.deepEqual(plain(await managed.get()), {});
assert.deepEqual(plain(await managed.get(null)), {});
assert.deepEqual(plain(await managed.get(['missing'])), {});
const defaults = JSON.parse('{"enabled":true,"nested":{"items":[1,2]},"__proto__":{"safe":true}}');
const result = await managed.get(defaults);
assert.deepEqual(plain(result), defaults);
result.nested.items.push(3);
assert.equal(defaults.nested.items.length, 2);
assert.equal(Object.hasOwn(result, '__proto__'), true);
assert.equal(Object.getPrototypeOf(result).safe, undefined);
assert.deepEqual(plain(await managed.get()), {}, 'defaults must not become stored policy');
assert.deepEqual(plain(await managed.getKeys()), []);
assert.equal(await managed.getBytesInUse(['enabled']), 0);
assert.equal(api.storage.local.sentinel, true);
let synchronous = true;
await new Promise(resolve => {
  assert.equal(managed.get({enabled:false}, value => {
    assert.equal(synchronous, false);
    assert.deepEqual(plain(value), {enabled:false});
    resolve();
  }), undefined);
  synchronous = false;
});
for (const run of [() => managed.set({enabled:true}), () => managed.remove('enabled'), () => managed.clear()]) {
  await assert.rejects(run(), /read-only/);
}
await new Promise(resolve => managed.set({enabled:true}, () => {
  assert.match(api.runtime.lastError.message, /read-only/);
  resolve();
}));
assert.equal(api.runtime.lastError, undefined);
await assert.rejects(managed.setAccessLevel({accessLevel:'TRUSTED_CONTEXTS'}), /unavailable/);
assert.throws(() => managed.get(3), /invalid/);
assert.throws(() => managed.get(['a', 1]), /invalid/);
assert.throws(() => managed.getBytesInUse({a:1}), /invalid/);
const listener = () => assert.fail('no configured policy can change');
managed.onChanged.addListener(listener);
assert.equal(managed.onChanged.hasListener(listener), true);
managed.onChanged.removeListener(listener);
assert.equal(managed.onChanged.hasListeners(), false);
for (let i = 0; i < 64; i++) managed.onChanged.addListener(() => {});
assert.throws(() => managed.onChanged.addListener(() => {}), /limit/);
// Replacing a namespace during extension-polyfill startup stays bounded.
context.browser = {runtime: api.runtime, storage: {}};
while (tasks.length) tasks.shift()();
await new Promise(resolve => queueMicrotask(resolve));
assert.equal(context.browser.storage.managed, managed);
assert.deepEqual(plain(await context.browser.storage.managed.get()), {});
vm.runInContext(source, context, {timeout: 1000});
assert.equal(api.storage.managed, managed);
assert.equal(tasks.length, 0, 'idempotence must not schedule more reconciliation');
const native = {get: () => Promise.resolve({native:true})};
const nativeApi = {runtime:{id:'native-fixture'},storage:{managed:native}};
vm.runInNewContext(source, {chrome:nativeApi,queueMicrotask,setTimeout:()=>{}}, {timeout:1000});
assert.equal(nativeApi.storage.managed, native, 'real native storage must not be overwritten');
console.log('Managed-storage v2 behavior passed');
