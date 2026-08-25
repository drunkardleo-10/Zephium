import assert from "node:assert/strict";
import { webcrypto } from "node:crypto";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const assetUrl = new URL(
  "../../crates/zephium-extension-package/assets/macos/webkit-api-v1.js",
  import.meta.url,
);
const source = await readFile(fileURLToPath(assetUrl), "utf8");
const script = new vm.Script(source, { filename: fileURLToPath(assetUrl) });
const notificationsAssetUrl = new URL(
  "../../crates/zephium-extension-package/assets/macos/webkit-notifications-v1.js",
  import.meta.url,
);
const notificationsSource = await readFile(fileURLToPath(notificationsAssetUrl), "utf8");
const notificationsScript = new vm.Script(notificationsSource, {
  filename: fileURLToPath(notificationsAssetUrl),
});
const managedStorageAssetUrl = new URL(
  "../../crates/zephium-extension-package/assets/macos/webkit-managed-storage-v1.js",
  import.meta.url,
);
const managedStorageSource = await readFile(fileURLToPath(managedStorageAssetUrl), "utf8");
const managedStorageScript = new vm.Script(managedStorageSource, {
  filename: fileURLToPath(managedStorageAssetUrl),
});
const backgroundDocumentAssetUrl = new URL(
  "../../crates/zephium-extension-package/assets/macos/webkit-background-document-v1.js",
  import.meta.url,
);
const backgroundDocumentSource = await readFile(
  fileURLToPath(backgroundDocumentAssetUrl),
  "utf8",
);
const backgroundDocumentScript = new vm.Script(backgroundDocumentSource, {
  filename: fileURLToPath(backgroundDocumentAssetUrl),
});
const nativeMessagingDenyAssetUrl = new URL(
  "../../crates/zephium-extension-package/assets/macos/webkit-native-messaging-deny-v1.js",
  import.meta.url,
);
const nativeMessagingDenySource = await readFile(
  fileURLToPath(nativeMessagingDenyAssetUrl),
  "utf8",
);
const nativeMessagingDenyScript = new vm.Script(nativeMessagingDenySource, {
  filename: fileURLToPath(nativeMessagingDenyAssetUrl),
});
const historyAssetUrl = new URL(
  "../../crates/zephium-extension-package/assets/macos/webkit-history-v1.js",
  import.meta.url,
);
const historySource = await readFile(fileURLToPath(historyAssetUrl), "utf8");
const historyScript = new vm.Script(historySource, {
  filename: fileURLToPath(historyAssetUrl),
});
const runtimeMessagingAssetUrl = new URL(
  "../../crates/zephium-extension-package/assets/macos/webkit-runtime-messaging-v1.js",
  import.meta.url,
);
const runtimeMessagingSource = await readFile(fileURLToPath(runtimeMessagingAssetUrl), "utf8");
const runtimeMessagingScript = new vm.Script(runtimeMessagingSource, {
  filename: fileURLToPath(runtimeMessagingAssetUrl),
});
const bookmarksAssetUrl = new URL(
  "../../crates/zephium-extension-package/assets/macos/webkit-bookmarks-v1.js",
  import.meta.url,
);
const bookmarksSource = await readFile(fileURLToPath(bookmarksAssetUrl), "utf8");
const bookmarksScript = new vm.Script(bookmarksSource, {
  filename: fileURLToPath(bookmarksAssetUrl),
});
const faviconAssetUrl = new URL(
  "../../crates/zephium-extension-package/assets/macos/webkit-favicon-v1.js",
  import.meta.url,
);
const faviconSource = await readFile(fileURLToPath(faviconAssetUrl), "utf8");
const faviconScript = new vm.Script(faviconSource, {
  filename: fileURLToPath(faviconAssetUrl),
});
const optionsAssetUrl = new URL(
  "../../crates/zephium-extension-package/assets/macos/webkit-options-page-v1.js",
  import.meta.url,
);
const optionsSource = await readFile(fileURLToPath(optionsAssetUrl), "utf8");
const optionsScript = new vm.Script(optionsSource, {
  filename: fileURLToPath(optionsAssetUrl),
});

for (const forbidden of ["document", "window", "fetch", "XMLHttpRequest", "WebSocket"]) {
  assert.equal(source.includes(forbidden), false, `compatibility asset exposes ${forbidden}`);
}
for (const forbidden of ["window", "fetch", "XMLHttpRequest", "WebSocket"]) {
  assert.equal(
    notificationsSource.includes(forbidden),
    false,
    `notifications asset exposes ${forbidden}`,
  );
  assert.equal(
    managedStorageSource.includes(forbidden),
    false,
    `managed-storage asset exposes ${forbidden}`,
  );
}
for (const forbidden of ["fetch", "XMLHttpRequest", "WebSocket"]) {
  assert.equal(
    nativeMessagingDenySource.includes(forbidden),
    false,
    `native-messaging denial asset exposes ${forbidden}`,
  );
}
for (const forbidden of ["fetch", "XMLHttpRequest", "WebSocket", "connectNative"]) {
  assert.equal(
    backgroundDocumentSource.includes(forbidden),
    false,
    `background-document asset exposes ${forbidden}`,
  );
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
for (const forbidden of ["document", "window", "fetch(", "XMLHttpRequest", "WebSocket"]) {
  assert.equal(bookmarksSource.includes(forbidden), false, `bookmarks asset exposes ${forbidden}`);
  assert.equal(faviconSource.includes(forbidden), false, `favicon asset exposes ${forbidden}`);
}
for (const forbidden of ["fetch(", "XMLHttpRequest", "WebSocket", "connectNative"]) {
  assert.equal(optionsSource.includes(forbidden), false, `options asset exposes ${forbidden}`);
}
for (const forbidden of [
  "fetch(",
  "XMLHttpRequest",
  "WebSocket",
  "sendNativeMessage",
  "connectNative",
]) {
  assert.equal(
    runtimeMessagingSource.includes(forbidden),
    false,
    `runtime messaging asset exposes ${forbidden}`,
  );
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

let notificationCallback;
const notificationsNative = nativeNamespace("notifications-fixture");
const notificationsContext = run({
  chrome: notificationsNative,
  queueMicrotask,
  setTimeout: (callback) => {
    callback();
    return 1;
  },
});
notificationsScript.runInContext(notificationsContext, { timeout: 1_000 });
const notifications = notificationsContext.chrome.notifications;
assert.equal(notificationsContext.browser.notifications, notifications);
assert.equal(Object.isFrozen(notifications), true);
assert.equal(notifications.onClicked.hasListeners(), false);
assert.equal(notifications.onClosed.hasListener(() => {}), false);
assert.equal(await notifications.getPermissionLevel(), "denied");
assert.equal(Object.keys(await notifications.getAll()).length, 0);
assert.equal(
  await notifications.create("requested-id", { type: "basic" }, (value) => {
    notificationCallback = value;
  }),
  "requested-id",
);
await Promise.resolve();
assert.equal(notificationCallback, "requested-id");
assert.equal(await notifications.update("requested-id", {}), false);
assert.equal(await notifications.clear("requested-id"), false);
notificationsScript.runInContext(notificationsContext, { timeout: 1_000 });
assert.equal(notificationsContext.chrome.notifications, notifications);

const nativeNotificationEvent = {
  addListener() {},
  removeListener() {},
};
const nativeNotifications = {
  native: true,
  onClicked: nativeNotificationEvent,
  create() {
    return Promise.resolve("native");
  },
};
const preexistingNotifications = nativeNamespace("native-notifications-fixture");
preexistingNotifications.notifications = nativeNotifications;
const preexistingContext = run({
  chrome: preexistingNotifications,
  queueMicrotask,
  setTimeout: (callback) => {
    callback();
    return 1;
  },
});
notificationsScript.runInContext(preexistingContext, { timeout: 1_000 });
assert.equal(preexistingContext.chrome.notifications, nativeNotifications);
assert.equal(preexistingContext.browser.notifications, nativeNotifications);
assert.equal(preexistingContext.chrome.notifications.onClicked, nativeNotificationEvent);
assert.equal(await preexistingContext.chrome.notifications.create(), "native");
assert.equal(typeof preexistingContext.chrome.notifications.onClosed.addListener, "function");

const managedTasks = [];
const managedNative = nativeNamespace("managed-storage-fixture");
const managedContext = run({
  chrome: managedNative,
  queueMicrotask: (callback) => managedTasks.push(callback),
  setTimeout: (callback) => {
    managedTasks.push(callback);
    return 1;
  },
});
managedStorageScript.runInContext(managedContext, { timeout: 1_000 });
const managed = managedContext.chrome.storage.managed;
assert.equal(managedContext.browser.storage.managed, managed);
assert.equal(Object.isFrozen(managed), true);
assert.equal(managed.onChanged.hasListeners(), false);
assert.equal(Object.keys(await managed.get(null)).length, 0);
managedNative.storage = { local: managedNative.storage.local };
for (const task of managedTasks.splice(0)) task();
assert.equal(
  managedContext.chrome.storage.managed,
  managed,
  "bounded reconciliation did not restore managed storage after namespace replacement",
);

const backgroundNative = nativeNamespace("background-document-fixture");
backgroundNative.runtime.getURL = function (path) {
  assert.equal(this, backgroundNative.runtime);
  return `https://background-document-fixture.example${path}`;
};
const backgroundDocumentContext = run({
  chrome: backgroundNative,
  document: {},
  location: { origin: "https://background-document-fixture.example" },
  URL,
});
backgroundDocumentScript.runInContext(backgroundDocumentContext, { timeout: 1_000 });
assert.equal(Object.isFrozen(backgroundDocumentContext.clients), true);
assert.deepEqual(
  JSON.parse(JSON.stringify(await backgroundDocumentContext.clients.matchAll({ type: "window" }))),
  [],
);
await assert.rejects(
  backgroundDocumentContext.clients.matchAll("window"),
  /options must be an object/,
);
assert.throws(
  () =>
    backgroundDocumentScript.runInContext(
      run({
        chrome: nativeNamespace("page-fixture"),
        document: {},
        location: { origin: "https://page.example" },
        URL,
      }),
      { timeout: 1_000 },
    ),
  /refused a page context/,
);

const nativeMessagingTasks = [];
const nativeMessagingNative = nativeNamespace("native-messaging-denied-fixture");
const nativeMessagingContext = run({
  chrome: nativeMessagingNative,
  queueMicrotask: (callback) => nativeMessagingTasks.push(callback),
});
nativeMessagingDenyScript.runInContext(nativeMessagingContext, { timeout: 1_000 });
assert.equal(
  nativeMessagingContext.chrome.runtime.connectNative,
  nativeMessagingContext.browser.runtime.connectNative,
);
assert.equal(
  nativeMessagingContext.chrome.runtime.sendNativeMessage,
  nativeMessagingContext.browser.runtime.sendNativeMessage,
);
const deniedPort = nativeMessagingContext.browser.runtime.connectNative("com.example.host");
assert.equal(Object.isFrozen(deniedPort), true);
assert.equal(deniedPort.name, "com.example.host");
let disconnects = 0;
deniedPort.onDisconnect.addListener((port) => {
  assert.equal(port, deniedPort);
  disconnects += 1;
});
deniedPort.postMessage({ ignored: true });
for (const task of nativeMessagingTasks.splice(0)) task();
assert.equal(disconnects, 1);
assert.throws(() => deniedPort.postMessage({}), /port is disconnected/);
deniedPort.disconnect();
assert.equal(disconnects, 1, "manual disconnect repeated the terminal event");
await assert.rejects(
  nativeMessagingContext.browser.runtime.sendNativeMessage("com.example.host", { ping: true }),
  /host is unavailable/,
);
let deniedCallbackCalls = 0;
assert.equal(
  nativeMessagingContext.chrome.runtime.sendNativeMessage("", { ping: true }, (response) => {
    assert.equal(response, undefined);
    deniedCallbackCalls += 1;
  }),
  undefined,
);
for (const task of nativeMessagingTasks.splice(0)) task();
assert.equal(deniedCallbackCalls, 1);
for (const invalid of ["", "space separated", "../host", "x".repeat(257)]) {
  assert.throws(
    () => nativeMessagingContext.chrome.runtime.connectNative(invalid),
    /identifier is invalid/,
  );
}
nativeMessagingDenyScript.runInContext(nativeMessagingContext, { timeout: 1_000 });
assert.equal(
  nativeMessagingContext.chrome.runtime.connectNative,
  nativeMessagingContext.browser.runtime.connectNative,
  "idempotent denial setup replaced connectNative",
);

const unsafeNativeMessaging = nativeNamespace("unsafe-native-messaging-fixture");
unsafeNativeMessaging.runtime.connectNative = () => ({ native: true });
unsafeNativeMessaging.runtime.sendNativeMessage = () => Promise.resolve({ native: true });
const unsafeNativeMessagingContext = run({ chrome: unsafeNativeMessaging, queueMicrotask });
assert.throws(
  () => nativeMessagingDenyScript.runInContext(unsafeNativeMessagingContext, { timeout: 1_000 }),
  /denial facade was not installed/,
  "a preexisting native-messaging capability was preserved after permission omission",
);

assert.throws(
  () => run({}),
  /Zephium WebKit extension API surface is unavailable/,
  "missing native authority did not fail closed",
);

let optionsClick;
let optionsTargetClick;
let optionsTargetKeydown;
let optionsContentLoaded;
let optionsMutation;
let optionsObserverDisconnected = 0;
let optionsOpened = 0;
class OptionsElement {
  closest() {
    return this;
  }
}
class OptionsAnchor extends OptionsElement {
  constructor(href) {
    super();
    this.href = href;
  }
  setAttribute(name, value) {
    assert.fail(`unexpected options-anchor attribute mutation: ${name}=${value}`);
  }
  removeAttribute(name) {
    assert.equal(name, "target");
  }
  addEventListener(type, listener, capture) {
    assert.equal(capture, true);
    if (type === "click") optionsTargetClick = listener;
    else {
      assert.equal(type, "keydown");
      optionsTargetKeydown = listener;
    }
  }
}
const optionsNative = nativeNamespace("options-fixture");
optionsNative.runtime.getURL = function (path) {
  assert.equal(this, optionsNative.runtime, "options runtime receiver was not preserved");
  return `webkit-extension://options-fixture/${path}`;
};
optionsNative.runtime.sendNativeMessage = (application, operation, callback) => {
  assert.equal(application, "app.zephium.extension-broker.v1");
  assert.equal(operation, "v1/options.open");
  optionsOpened += 1;
  callback('{"v":1,"opened":true}');
};
const optionsDocument = {
  readyState: "loading",
  documentElement: {},
  querySelectorAll(selector) {
    if (selector === 'meta[name="zephium-extension-options-page"]') {
      return [{ getAttribute: (name) => (name === "content" ? "pages/options.html" : null) }];
    }
    assert.equal(selector, "a[href]");
    return [optionsAnchor];
  },
  addEventListener(type, listener, options) {
    if (type === "click") {
      assert.equal(options, true);
      optionsClick = listener;
      return;
    }
    assert.equal(type, "DOMContentLoaded");
    assert.equal(options?.once, true);
    assert.deepEqual(Object.keys(options), ["once"]);
    optionsContentLoaded = listener;
  },
};
const optionsAnchor = new OptionsAnchor(
  "",
);
class OptionsMutationObserver {
  constructor(callback) {
    optionsMutation = callback;
  }
  observe(target, options) {
    assert.equal(target, optionsDocument.documentElement);
    assert.equal(options.attributes, true);
    assert.equal(options.subtree, true);
    assert.deepEqual(Array.from(options.attributeFilter), ["href"]);
  }
  disconnect() {
    optionsObserverDisconnected += 1;
  }
}
const optionsContext = vm.createContext({
  chrome: optionsNative,
  document: optionsDocument,
  Element: OptionsElement,
  HTMLAnchorElement: OptionsAnchor,
  MutationObserver: OptionsMutationObserver,
  setTimeout: () => 1,
  clearTimeout: () => {},
});
script.runInContext(optionsContext, { timeout: 1_000 });
optionsScript.runInContext(optionsContext, { timeout: 1_000 });
optionsContentLoaded();
assert.equal(typeof optionsMutation, "function");
assert.equal(optionsTargetClick, undefined);
optionsAnchor.href = "webkit-extension://options-fixture/pages/options.html";
optionsMutation([{ type: "attributes", target: optionsAnchor }]);
assert.equal(typeof optionsTargetClick, "function");
assert.equal(typeof optionsTargetKeydown, "function");
assert.equal(optionsAnchor.role, undefined);
assert.equal(optionsAnchor.tabindex, undefined);
assert.equal(optionsAnchor.href, "webkit-extension://options-fixture/pages/options.html");
assert.equal(optionsObserverDisconnected, 1);
let optionsPrevented = 0;
let optionsPropagationStopped = 0;
optionsTargetClick({
  isTrusted: true,
  defaultPrevented: true,
  button: 0,
  target: optionsAnchor,
  preventDefault() {
    optionsPrevented += 1;
  },
  stopImmediatePropagation() {
    optionsPropagationStopped += 1;
  },
});
await Promise.resolve();
assert.equal(optionsPrevented, 1);
assert.equal(optionsPropagationStopped, 1);
assert.equal(optionsOpened, 1);
optionsTargetKeydown({
  key: "Enter",
  currentTarget: optionsAnchor,
  preventDefault() {
    optionsPrevented += 1;
  },
  stopImmediatePropagation() {
    optionsPropagationStopped += 1;
  },
});
await Promise.resolve();
assert.equal(optionsPrevented, 2);
assert.equal(optionsPropagationStopped, 2);
assert.equal(optionsOpened, 2);
optionsClick({
  isTrusted: false,
  defaultPrevented: false,
  button: 0,
  target: new OptionsAnchor("webkit-extension://options-fixture/pages/other.html"),
  preventDefault() {
    optionsPrevented += 1;
  },
  stopImmediatePropagation() {
    optionsPropagationStopped += 1;
  },
});
assert.equal(optionsPrevented, 2, "another extension link reached the options bridge");
assert.equal(optionsPropagationStopped, 2, "another extension link was intercepted");
assert.equal(optionsOpened, 2, "another extension link opened the options page");
assert.equal(
  optionsContext[Symbol.for("zephium.webkit-options-page-compatibility.mode.v1")],
  "runtime-open-options-page-window",
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

let nativeHistoryCalls = 0;
const nativeHistory = Object.freeze({
  search() {
    nativeHistoryCalls += 1;
  },
});
const shadowRequests = [];
const shadowedNamespace = historyNamespace(({ application, request, callback }) => {
  shadowRequests.push({ application, request });
  callback('{"v":1,"items":[]}');
});
shadowedNamespace.history = nativeHistory;
const shadowed = runHistory(shadowedNamespace);
assert.notEqual(shadowed.chrome.history, nativeHistory);
assert.equal(shadowed.chrome.history, shadowed.browser.history);
assert.equal(shadowed[historyMode], "bounded-recent-search");
await shadowed.chrome.history.search({ text: "", maxResults: 1, startTime: 0 });
assert.equal(nativeHistoryCalls, 0, "brokered search reached WebKit's private history object");
assert.deepEqual(shadowRequests, [
  {
    application: "app.zephium.extension-broker.v1",
    request: "v1/history.recent/1",
  },
]);

const unshadowableNamespace = historyNamespace(() => {});
Object.defineProperty(unshadowableNamespace, "history", {
  value: nativeHistory,
  configurable: false,
  writable: false,
});
assert.throws(() => runHistory(unshadowableNamespace), /cannot install safely/);

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

const bookmarksNamespace = nativeNamespace("bookmarks-fixture");
const bookmarksContext = vm.createContext({
  chrome: bookmarksNamespace,
  queueMicrotask,
});
script.runInContext(bookmarksContext, { timeout: 1_000 });
bookmarksScript.runInContext(bookmarksContext, { timeout: 1_000 });
const bookmarksInstalled = Symbol.for("zephium.webkit-bookmarks-compatibility.v1");
const bookmarksMode = Symbol.for("zephium.webkit-bookmarks-compatibility.mode.v1");
assert.equal(bookmarksContext[bookmarksInstalled], true);
assert.equal(bookmarksContext[bookmarksMode], "empty-read-only");
assert.equal(bookmarksContext.chrome.bookmarks, bookmarksContext.browser.bookmarks);
assert.deepEqual(
  JSON.parse(JSON.stringify(await bookmarksContext.chrome.bookmarks.getTree())),
  [{ id: "0", title: "", children: [] }],
);
assert.deepEqual(
  JSON.parse(JSON.stringify(await bookmarksContext.chrome.bookmarks.search("anything"))),
  [],
);
assert.deepEqual(
  JSON.parse(JSON.stringify(await bookmarksContext.chrome.bookmarks.getSubTree("missing"))),
  [],
);
let bookmarkCallback;
assert.equal(
  bookmarksContext.chrome.bookmarks.getTree((tree) => {
    bookmarkCallback = JSON.parse(JSON.stringify(tree));
  }),
  undefined,
);
await new Promise((resolve) => queueMicrotask(resolve));
assert.deepEqual(bookmarkCallback, [{ id: "0", title: "", children: [] }]);
assert.equal(typeof bookmarksContext.chrome.bookmarks.create, "undefined");
const bookmarkListener = () => {};
bookmarksContext.chrome.bookmarks.onCreated.addListener(bookmarkListener);
assert.equal(bookmarksContext.chrome.bookmarks.onCreated.hasListener(bookmarkListener), true);
bookmarksContext.chrome.bookmarks.onCreated.removeListener(bookmarkListener);
assert.equal(bookmarksContext.chrome.bookmarks.onCreated.hasListeners(), false);
assert.throws(
  () => bookmarksContext.chrome.bookmarks.getRecent(-1),
  /count is invalid/,
);
bookmarksScript.runInContext(bookmarksContext, { timeout: 1_000 });
assert.equal(bookmarksContext.chrome.bookmarks, bookmarksContext.browser.bookmarks);

const preservedBookmarks = Object.freeze({ getTree() {} });
const preservedBookmarksNamespace = nativeNamespace("native-bookmarks");
preservedBookmarksNamespace.bookmarks = preservedBookmarks;
const preservedBookmarksContext = vm.createContext({
  chrome: preservedBookmarksNamespace,
  queueMicrotask,
});
script.runInContext(preservedBookmarksContext, { timeout: 1_000 });
bookmarksScript.runInContext(preservedBookmarksContext, { timeout: 1_000 });
assert.equal(preservedBookmarksContext.chrome.bookmarks, preservedBookmarks);
assert.equal(preservedBookmarksContext[bookmarksMode], "native-preserved");

const faviconNamespace = nativeNamespace("favicon-fixture");
const faviconContext = vm.createContext({ chrome: faviconNamespace });
script.runInContext(faviconContext, { timeout: 1_000 });
faviconScript.runInContext(faviconContext, { timeout: 1_000 });
const faviconInstalled = Symbol.for("zephium.webkit-favicon-compatibility.v1");
const faviconMode = Symbol.for("zephium.webkit-favicon-compatibility.mode.v1");
assert.equal(faviconContext[faviconInstalled], true);
assert.equal(faviconContext[faviconMode], "transparent-fallback");
assert.equal(
  faviconContext.chrome.runtime.getURL("_favicon/?pageUrl=https%3A%2F%2Fexample.com"),
  "native-extension://favicon-fixture/__zephium__/favicon-empty-v1.svg",
);
assert.equal(
  faviconContext.chrome.runtime.getURL("/favicon/icon.svg"),
  "native-extension://favicon-fixture//favicon/icon.svg",
);
faviconScript.runInContext(faviconContext, { timeout: 1_000 });
assert.equal(
  faviconContext.chrome.runtime.getURL("_favicon/"),
  "native-extension://favicon-fixture/__zephium__/favicon-empty-v1.svg",
);

function nativeEvent() {
  const listeners = new Set();
  return {
    addListener(listener) {
      listeners.add(listener);
    },
    removeListener(listener) {
      listeners.delete(listener);
    },
    hasListener(listener) {
      return listeners.has(listener);
    },
    hasListeners() {
      return listeners.size !== 0;
    },
    emit(...args) {
      return [...listeners].map((listener) => listener(...args));
    },
  };
}

function linkedPorts(name, sender) {
  const pageMessages = nativeEvent();
  const backgroundMessages = nativeEvent();
  const pageDisconnect = nativeEvent();
  const backgroundDisconnect = nativeEvent();
  let disconnected = false;
  const disconnect = () => {
    if (disconnected) return;
    disconnected = true;
    pageDisconnect.emit();
    backgroundDisconnect.emit();
  };
  const page = {
    name,
    onMessage: pageMessages,
    onDisconnect: pageDisconnect,
    postMessage(message) {
      if (disconnected) throw new Error("port disconnected");
      backgroundMessages.emit(message);
    },
    disconnect,
  };
  const background = {
    name,
    sender,
    onMessage: backgroundMessages,
    onDisconnect: backgroundDisconnect,
    postMessage(message) {
      if (disconnected) throw new Error("port disconnected");
      pageMessages.emit(message);
    },
    disconnect,
  };
  return { page, background };
}

function sessionStorageFixture() {
  const values = new Map();
  const onChanged = nativeEvent();
  return {
    onChanged,
    session: {
      async get(key) {
        return values.has(key) ? { [key]: values.get(key) } : {};
      },
      async set(entries) {
        const changes = {};
        for (const [key, value] of Object.entries(entries)) {
          changes[key] = { oldValue: values.get(key), newValue: value };
          values.set(key, value);
        }
        onChanged.emit(changes, "session");
      },
      async remove(key) {
        if (!values.has(key)) return;
        const oldValue = values.get(key);
        values.delete(key);
        onChanged.emit({ [key]: { oldValue } }, "session");
      },
    },
  };
}

function runtimeMessagingFixture() {
  const backgroundOnMessage = nativeEvent();
  const backgroundOnConnect = nativeEvent();
  const pageOnMessage = nativeEvent();
  const pageOnConnect = nativeEvent();
  const sender = Object.freeze({
    frameId: 7,
    url: "webkit-extension://fixture/pages/vomnibar.html",
    tab: Object.freeze({ id: 41, url: "https://page.example/path" }),
  });
  let externalCalls = 0;
  const storage = sessionStorageFixture();
  const backgroundRuntime = {
    id: "runtime-messaging-fixture",
    onMessage: backgroundOnMessage,
    onConnect: backgroundOnConnect,
    connect() {
      throw new Error("background connect must not be used");
    },
    sendMessage() {
      throw new Error("background sendMessage must not be used");
    },
  };
  const pageRuntime = {
    id: "runtime-messaging-fixture",
    onMessage: pageOnMessage,
    onConnect: pageOnConnect,
    connect(options) {
      const ports = linkedPorts(options?.name ?? "", sender);
      backgroundOnConnect.emit(ports.background);
      return ports.page;
    },
    sendMessage(...args) {
      if (typeof args[0] === "string" && args.length >= 2) {
        externalCalls += 1;
        return "native-external-result";
      }
      const callback = typeof args.at(-1) === "function" ? args.at(-1) : undefined;
      backgroundOnMessage.emit(args[0], sender, () => {});
      callback?.(undefined);
      return undefined;
    },
  };
  return {
    backgroundRuntime,
    pageRuntime,
    storage,
    sender,
    externalCalls: () => externalCalls,
  };
}

const messaging = runtimeMessagingFixture();
const messagingGlobals = {
  setTimeout,
  clearTimeout,
  queueMicrotask,
  crypto: webcrypto,
  TextEncoder,
};
const backgroundMessagingContext = vm.createContext({
  ...messagingGlobals,
  chrome: { runtime: messaging.backgroundRuntime, storage: messaging.storage },
});
runtimeMessagingScript.runInContext(backgroundMessagingContext, { timeout: 1_000 });
const pageMessagingContext = vm.createContext({
  ...messagingGlobals,
  chrome: { runtime: messaging.pageRuntime, storage: messaging.storage },
  document: {},
  location: { protocol: "webkit-extension:" },
});
runtimeMessagingScript.runInContext(pageMessagingContext, { timeout: 1_000 });

const asyncListener = (message, sender, sendResponse) => {
  assert.deepEqual(JSON.parse(JSON.stringify(sender)), messaging.sender);
  if (message?.kind !== "async") return false;
  queueMicrotask(() => sendResponse({ value: message.value + 1 }));
  return true;
};
messaging.backgroundRuntime.onMessage.addListener(asyncListener);
assert.equal(messaging.backgroundRuntime.onMessage.hasListener(asyncListener), true);
assert.equal(messaging.backgroundRuntime.onMessage.hasListeners(), true);
assert.deepEqual(
  JSON.parse(
    JSON.stringify(await messaging.pageRuntime.sendMessage({ kind: "async", value: 6 })),
  ),
  { value: 7 },
);

let callbackValue;
assert.equal(
  messaging.pageRuntime.sendMessage({ kind: "async", value: 8 }, (value) => {
    callbackValue = JSON.parse(JSON.stringify(value));
  }),
  undefined,
);
await new Promise((resolve) => setTimeout(resolve, 0));
assert.deepEqual(callbackValue, { value: 9 });

messaging.backgroundRuntime.onMessage.removeListener(asyncListener);
assert.equal(messaging.backgroundRuntime.onMessage.hasListener(asyncListener), false);
const promiseListener = (message) =>
  message?.kind === "promise" ? Promise.resolve({ promised: true }) : false;
messaging.backgroundRuntime.onMessage.addListener(promiseListener);
assert.deepEqual(
  JSON.parse(
    JSON.stringify(await messaging.pageRuntime.sendMessage({ kind: "promise" })),
  ),
  { promised: true },
);
assert.equal(await messaging.pageRuntime.sendMessage({ kind: "unhandled" }), undefined);

let publicConnects = 0;
const publicConnectListener = () => {
  publicConnects += 1;
};
messaging.backgroundRuntime.onConnect.addListener(publicConnectListener);
await messaging.pageRuntime.sendMessage({ kind: "promise" });
assert.equal(publicConnects, 0, "reserved compatibility port reached extension listeners");
const publicPort = messaging.pageRuntime.connect({ name: "public-extension-port" });
assert.equal(publicConnects, 1);
publicPort.disconnect();
messaging.backgroundRuntime.onConnect.removeListener(publicConnectListener);
assert.equal(messaging.backgroundRuntime.onConnect.hasListener(publicConnectListener), false);

assert.equal(
  messaging.pageRuntime.sendMessage("other-extension", { kind: "external" }),
  "native-external-result",
);
assert.equal(messaging.externalCalls(), 1);
runtimeMessagingScript.runInContext(backgroundMessagingContext, { timeout: 1_000 });
runtimeMessagingScript.runInContext(pageMessagingContext, { timeout: 1_000 });
assert.equal(
  backgroundMessagingContext[
    Symbol.for("zephium.webkit-runtime-messaging-compatibility.v1")
  ],
  true,
);
assert.equal(
  pageMessagingContext[Symbol.for("zephium.webkit-runtime-messaging-compatibility.v1")],
  true,
);

const missingRuntimeMessaging = vm.createContext({
  ...messagingGlobals,
  chrome: { runtime: { id: "missing-events" } },
  document: {},
  location: { protocol: "webkit-extension:" },
});
assert.throws(
  () => runtimeMessagingScript.runInContext(missingRuntimeMessaging, { timeout: 1_000 }),
  /surface is unavailable/,
);

console.log("macOS extension compatibility asset contract passed");
