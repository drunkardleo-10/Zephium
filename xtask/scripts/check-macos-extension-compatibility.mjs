import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const assetUrl = new URL(
  "../../crates/zephium-extension-package/assets/macos/webkit-api-v1.js",
  import.meta.url,
);
const source = await readFile(fileURLToPath(assetUrl), "utf8");
const script = new vm.Script(source, { filename: fileURLToPath(assetUrl) });

for (const forbidden of ["document", "window", "fetch", "XMLHttpRequest", "WebSocket"]) {
  assert.equal(source.includes(forbidden), false, `compatibility asset exposes ${forbidden}`);
}

function nativeNamespace(id) {
  const runtime = {
    id,
    getURL(path) {
      assert.equal(this, runtime, "runtime receiver was not preserved");
      return `native-extension://${id}/${path}`;
    },
  };
  Object.defineProperty(runtime, "fixed", {
    value: Object.freeze({ value: 7 }),
    writable: false,
    enumerable: true,
    configurable: false,
  });
  Object.defineProperty(runtime, "missingAccessor", {
    get: undefined,
    set: undefined,
    enumerable: true,
    configurable: false,
  });
  const local = {
    get(key) {
      assert.equal(this, local, "nested namespace receiver was not preserved");
      return key;
    },
  };
  return { runtime, storage: { local } };
}

function run(context) {
  const sandbox = vm.createContext(context);
  script.runInContext(sandbox, { timeout: 1_000 });
  return sandbox;
}

const nativeChrome = nativeNamespace("chrome-fixture");
const chromeOnly = run({ chrome: nativeChrome });
assert.equal(chromeOnly.chrome.runtime.id, "chrome-fixture");
assert.equal(chromeOnly.browser.runtime.id, "chrome-fixture");
assert.notEqual(chromeOnly.chrome, nativeChrome);
assert.equal(chromeOnly.chrome.runtime.fixed, nativeChrome.runtime.fixed);
assert.equal(chromeOnly.chrome.runtime.missingAccessor, undefined);
const getURL = chromeOnly.chrome.runtime.getURL;
assert.equal(getURL, chromeOnly.chrome.runtime.getURL, "bound method identity drifted");
assert.equal(getURL("asset.js"), "native-extension://chrome-fixture/asset.js");
chromeOnly.chrome.runtime.getURL = function replacement(path) {
  assert.equal(this, nativeChrome.runtime, "replacement receiver was not preserved");
  return `replacement://${path}`;
};
assert.equal(chromeOnly.chrome.runtime.getURL("asset.js"), "replacement://asset.js");
const replacementGetURL = chromeOnly.chrome.runtime.getURL;
const nestedGet = chromeOnly.chrome.storage.local.get;
assert.equal(nestedGet("key"), "key");
assert.equal(typeof chromeOnly.chrome.runtime.onUpdateAvailable.addListener, "function");
assert.equal(chromeOnly.chrome.runtime.onUpdateAvailable.hasListeners(), false);
assert.equal(Object.isFrozen(chromeOnly.chrome.runtime.onUpdateAvailable), true);
script.runInContext(chromeOnly, { timeout: 1_000 });
assert.equal(
  chromeOnly.chrome.runtime.getURL,
  replacementGetURL,
  "idempotent run replaced the API facade",
);

const nativeBrowser = nativeNamespace("browser-fixture");
const browserOnly = run({ browser: nativeBrowser });
assert.equal(browserOnly.chrome.runtime.id, "browser-fixture");
assert.equal(browserOnly.browser.runtime.id, "browser-fixture");
assert.equal(browserOnly.browser.runtime.getURL("x"), "native-extension://browser-fixture/x");

assert.throws(
  () => run({}),
  /Zephium WebKit extension API surface is unavailable/,
  "missing native authority did not fail closed",
);

const lockedNative = nativeNamespace("locked-fixture");
const locked = vm.createContext({});
Object.defineProperty(locked, "chrome", {
  get: () => lockedNative,
  set() {},
  configurable: false,
});
assert.throws(
  () => script.runInContext(locked, { timeout: 1_000 }),
  /Zephium WebKit extension API surface is unavailable/,
  "a non-replaceable native namespace was mistaken for an installed facade",
);

console.log("macOS extension compatibility asset contract passed");
