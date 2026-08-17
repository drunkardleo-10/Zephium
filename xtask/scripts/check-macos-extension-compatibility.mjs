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
const historyAssetUrl = new URL(
  "../../crates/zephium-extension-package/assets/macos/webkit-history-v1.js",
  import.meta.url,
);
const historySource = await readFile(fileURLToPath(historyAssetUrl), "utf8");
const historyScript = new vm.Script(historySource, {
  filename: fileURLToPath(historyAssetUrl),
});

for (const forbidden of ["document", "window", "fetch", "XMLHttpRequest", "WebSocket"]) {
  assert.equal(source.includes(forbidden), false, `compatibility asset exposes ${forbidden}`);
}
for (const forbidden of [
  "document",
  "window",
  "fetch",
  "XMLHttpRequest",
  "WebSocket",
  "connectNative",
]) {
  assert.equal(historySource.includes(forbidden), false, `history asset exposes ${forbidden}`);
}
assert.equal(
  historySource.match(/app\.zephium\.extension-broker\.v1/g)?.length,
  1,
  "history asset must bind one fixed native application identifier",
);
assert.equal(
  historySource.match(/v1\/history\.recent\//g)?.length,
  1,
  "history asset must bind one fixed broker operation",
);

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

function historyNamespace(responder) {
  const namespace = nativeNamespace("history-fixture");
  namespace.runtime.sendNativeMessage = (application, request, callback) => {
    responder({ application, request, callback, runtime: namespace.runtime });
  };
  return namespace;
}

function runHistory(namespace) {
  const sandbox = vm.createContext({ chrome: namespace });
  script.runInContext(sandbox, { timeout: 1_000 });
  historyScript.runInContext(sandbox, { timeout: 1_000 });
  return sandbox;
}

const requests = [];
const historyNative = historyNamespace(({ application, request, callback }) => {
  requests.push({ application, request });
  callback(
    JSON.stringify({
      v: 1,
      items: [
        {
          url: "https://older.example/path",
          title: "Older result",
          lastVisit: 1_000,
        },
        {
          url: "https://matching.example/path",
          title: "Matching result",
          lastVisit: 2_000,
        },
      ],
    }),
  );
});
const history = runHistory(historyNative);
const historyInstalled = Symbol.for("zephium.webkit-history-compatibility.v1");
const historyMode = Symbol.for("zephium.webkit-history-compatibility.mode.v1");
assert.equal(history[historyInstalled], true);
assert.equal(history[historyMode], "bounded-recent-search");
assert.equal(history.chrome.history, history.browser.history);
assert.equal(Object.isFrozen(history.chrome.history), true);
assert.equal(typeof history.chrome.history.addUrl, "undefined");
assert.equal(typeof history.chrome.history.deleteAll, "undefined");
const searchResults = await history.chrome.history.search({
  text: "matching",
  maxResults: 20_000,
  startTime: 0,
  endTime: 3_000,
});
assert.deepEqual(JSON.parse(JSON.stringify(searchResults)), [
  {
    id: "https://matching.example/path",
    url: "https://matching.example/path",
    title: "Matching result",
    lastVisitTime: 2_000,
  },
]);
assert.deepEqual(requests, [
  {
    application: "app.zephium.extension-broker.v1",
    request: "v1/history.recent/100",
  },
]);

let callbackResults;
const callbackReturn = history.chrome.history.search(
  { text: "", maxResults: 2, startTime: 0 },
  (results) => {
    callbackResults = JSON.parse(JSON.stringify(results));
  },
);
assert.equal(callbackReturn, undefined);
assert.equal(callbackResults.length, 2);
assert.equal(requests.at(-1)?.request, "v1/history.recent/2");
assert.deepEqual(
  JSON.parse(
    JSON.stringify(await history.chrome.history.search({ text: "", maxResults: 0 })),
  ),
  [],
);
assert.equal(requests.length, 2, "zero-result query reached the native broker");
assert.throws(() => history.chrome.history.search({}), /query\.text/);
assert.throws(
  () => history.chrome.history.search({ text: "", maxResults: -1 }),
  /maxResults/,
);

const onVisited = history.chrome.history.onVisited;
const listener = () => {};
onVisited.addListener(listener);
assert.equal(onVisited.hasListener(listener), true);
assert.equal(onVisited.hasListeners(), true);
onVisited.removeListener(listener);
assert.equal(onVisited.hasListener(listener), false);
assert.equal(onVisited.hasListeners(), false);
assert.throws(() => onVisited.addListener("not-a-function"), /must be a function/);

historyScript.runInContext(history, { timeout: 1_000 });
assert.equal(history.chrome.history, history.browser.history, "idempotent run replaced history");

const nativeHistory = Object.freeze({ search() {} });
const preservedNamespace = historyNamespace(() => {
  throw new Error("native history preservation called the Zephium broker");
});
preservedNamespace.history = nativeHistory;
const preserved = runHistory(preservedNamespace);
assert.equal(preserved.chrome.history, nativeHistory);
assert.equal(preserved.browser.history, nativeHistory);
assert.equal(preserved[historyMode], "native-preserved");

const missingBroker = nativeNamespace("missing-broker");
const missingBrokerContext = vm.createContext({ chrome: missingBroker });
script.runInContext(missingBrokerContext, { timeout: 1_000 });
assert.throws(
  () => historyScript.runInContext(missingBrokerContext, { timeout: 1_000 }),
  /history compatibility broker is unavailable/,
);

const malformed = runHistory(
  historyNamespace(({ callback }) => callback('{"v":1,"items":[{"url":"duplicate"}]}')),
);
await assert.rejects(
  malformed.chrome.history.search({ text: "", startTime: 0 }),
  /invalid history item/,
);

console.log("macOS extension compatibility asset contract passed");
