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
  const onMessage = {
    addListener() {},
    removeListener() {},
    hasListener() {
      return false;
    },
  };
  const runtime = {
    id,
    onMessage,
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
const installed = Symbol.for("zephium.webkit-api-compatibility.v1");
const mode = Symbol.for("zephium.webkit-api-compatibility.mode.v1");
assert.equal(chromeOnly.chrome.runtime.id, "chrome-fixture");
assert.equal(chromeOnly.browser.runtime.id, "chrome-fixture");
assert.equal(chromeOnly.chrome, nativeChrome, "native chrome identity changed");
assert.equal(chromeOnly.browser, nativeChrome, "missing browser alias did not reuse chrome");
for (const name of ["chrome", "browser"]) {
  const descriptor = Object.getOwnPropertyDescriptor(chromeOnly, name);
  assert.equal(descriptor?.value, nativeChrome);
  assert.equal(descriptor?.writable, false);
  assert.equal(descriptor?.configurable, false);
}
assert.equal(chromeOnly.chrome.runtime.fixed, nativeChrome.runtime.fixed);
assert.equal(chromeOnly.chrome.runtime.missingAccessor, undefined);
assert.equal(
  chromeOnly.chrome.runtime.onMessage,
  nativeChrome.runtime.onMessage,
  "native event identity changed",
);
assert.equal(
  chromeOnly.chrome.runtime.getURL("asset.js"),
  "native-extension://chrome-fixture/asset.js",
);
assert.equal(chromeOnly.chrome.storage.local.get("key"), "key");
assert.equal(typeof chromeOnly.chrome.runtime.onUpdateAvailable.addListener, "function");
assert.equal(chromeOnly.chrome.runtime.onUpdateAvailable.hasListeners(), false);
assert.equal(Object.isFrozen(chromeOnly.chrome.runtime.onUpdateAvailable), true);
assert.equal(chromeOnly[installed], true);
assert.equal(chromeOnly[mode], "native-aliased");
script.runInContext(chromeOnly, { timeout: 1_000 });
assert.equal(chromeOnly.chrome, nativeChrome, "idempotent run replaced native chrome");
assert.equal(chromeOnly.browser, nativeChrome, "idempotent run replaced browser alias");

const nativeBrowser = nativeNamespace("browser-fixture");
const browserOnly = run({ browser: nativeBrowser });
assert.equal(browserOnly.chrome.runtime.id, "browser-fixture");
assert.equal(browserOnly.browser.runtime.id, "browser-fixture");
assert.equal(browserOnly.chrome, nativeBrowser);
assert.equal(browserOnly.browser, nativeBrowser);
assert.equal(browserOnly.browser.runtime.getURL("x"), "native-extension://browser-fixture/x");
assert.equal(browserOnly[mode], "native-aliased");

assert.throws(
  () => run({}),
  /Zephium WebKit extension API surface is unavailable/,
  "missing native authority did not fail closed",
);

const lockedNative = nativeNamespace("locked-fixture");
const locked = vm.createContext({});
Object.defineProperty(locked, "chrome", {
  get: () => lockedNative,
  configurable: false,
});
script.runInContext(locked, { timeout: 1_000 });
assert.equal(locked.chrome, lockedNative);
assert.equal(locked.browser.runtime.id, "locked-fixture");
assert.equal(locked.chrome.runtime.onUpdateAvailable.hasListeners(), false);
assert.equal(locked[installed], true);
assert.equal(locked[mode], "native-aliased");

const workerChrome = nativeNamespace("worker-chrome-fixture");
const workerBrowser = nativeNamespace("worker-browser-fixture");
const worker = run({
  chrome: workerChrome,
  browser: workerBrowser,
  registration: {},
  clients: {},
});
assert.equal(worker.chrome, workerChrome, "service-worker chrome identity changed");
assert.equal(worker.browser, workerBrowser, "service-worker browser identity changed");
assert.equal(worker.chrome.runtime.onUpdateAvailable.hasListeners(), false);
assert.equal(worker.browser.runtime.onUpdateAvailable.hasListeners(), false);
assert.equal(worker[installed], true);
assert.equal(worker[mode], "native-preserved");

const sealedNative = nativeNamespace("sealed-fixture");
Object.preventExtensions(sealedNative.runtime);
assert.throws(
  () => run({ chrome: sealedNative }),
  /Zephium WebKit extension API surface is unavailable/,
  "unadaptable native runtime did not fail closed",
);

console.log("macOS extension compatibility asset contract passed");
