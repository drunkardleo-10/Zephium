// Runs the compatibility layer against stub extension contexts and checks
// that each fix is actually applied.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

const source = readFileSync(new URL("../src/compat/compat.js", import.meta.url), "utf8");
const event = () => ({ addListener() {}, removeListener() {}, hasListener: () => false });

function context(kind) {
  const invalid = (name) => new Error(`Invalid call. The 'permissions' value is invalid, because '${name}' is not a valid permission.`);
  const chrome = {
    runtime: {
      id: "abcdefghijklmnopabcdefghijklmnop",
      sendNativeMessage: async (_app, message) => (message.api === "trace" ? false : null),
      connectNative: () => ({ onMessage: event(), onDisconnect: event(), postMessage() {}, disconnect() {} }),
      onMessage: event(),
      onConnect: event(),
    },
    permissions: {
      contains(request) {
        const unknown = (request.permissions || []).find((name) => name === "privacy");
        if (unknown) throw invalid(unknown);
        return Promise.resolve(true);
      },
      request: () => Promise.resolve(true),
      remove: () => Promise.resolve(true),
    },
    storage: { local: {}, onChanged: event() },
    scripting: {
      registerContentScripts: () => Promise.reject(new Error("Duplicate ID 'a'.")),
      updateContentScripts: () => Promise.resolve(),
    },
    webNavigation: { onCommitted: event() },
    tabs: { onUpdated: event(), onRemoved: event(), sendMessage() {} },
  };
  const g = {
    chrome,
    browser: { ...chrome },
    // As in WebKit, the user agent lives on Navigator.prototype.
    navigator: Object.create({ userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15" }),
    addEventListener() {},
    setTimeout: () => 0,
    setInterval: () => 0,
    clearInterval() {},
    console: { error() {}, warn() {} },
    WebAssembly: { instantiateStreaming() {}, compileStreaming() {} },
    Symbol,
    Promise,
    Object,
    Array,
    String,
    Boolean,
    Error,
    Set,
    WeakMap,
    WeakSet,
    Map,
    JSON,
    Date,
    Math,
    RegExp,
  };
  if (kind === "worker") {
    g.ServiceWorkerGlobalScope = class {
      static [Symbol.hasInstance]() {
        return true;
      }
    };
    g.location = { protocol: "chrome-extension:", href: "chrome-extension://x/sw.js" };
    g.EventTarget = class {};
  } else if (kind === "page") {
    g.location = { protocol: "chrome-extension:", pathname: "/popup.html", href: "chrome-extension://x/popup.html" };
  } else {
    g.location = { protocol: "https:", pathname: "/", href: "https://example.com/" };
  }
  g.globalThis = g;
  vm.createContext(g);
  vm.runInContext(source, g);
  return g;
}

test("pages and workers get every API fix", async () => {
  for (const kind of ["page", "worker"]) {
    const { chrome, browser } = context(kind);
    assert.equal(browser, chrome, `${kind}: browser aliases chrome`);
    assert.equal(await chrome.permissions.contains({ permissions: ["privacy"] }), false, kind);
    assert.equal(await chrome.permissions.request({ permissions: ["privacy"] }), false, kind);
    assert.equal(Object.keys(await chrome.storage.managed.get()).length, 0, kind);
    assert.equal(typeof chrome.storage.managed.onChanged.addListener, "function", kind);
    assert.equal(chrome.scripting.ExecutionWorld.ISOLATED, "ISOLATED", kind);
    await chrome.scripting.registerContentScripts([{ id: "a" }]);
    assert.equal(typeof chrome.webNavigation.onHistoryStateUpdated.addListener, "function", kind);
    assert.equal(typeof chrome.notifications.create, "function", kind);
    assert.match(chrome.identity.getRedirectURL("cb"), /^https:\/\/abcdefghijklmnopabcdefghijklmnop\.chromiumapp\.org\/cb$/);
    const saving = await chrome.privacy.services.passwordSavingEnabled.get({});
    assert.equal(saving.value, true, kind);
    await chrome.privacy.services.passwordSavingEnabled.set({ value: false });
    assert.equal((await chrome.privacy.services.passwordSavingEnabled.get({})).value, false, kind);
  }
});

test("workers present a Chrome identity and bridge WebSockets", () => {
  const g = context("worker");
  assert.match(g.navigator.userAgent, / Chrome\/\d+/);
  assert.equal(typeof g.WebSocket, "function");
});

test("content scripts keep the page's identity", () => {
  const g = context("content");
  assert.doesNotMatch(g.navigator.userAgent, / Chrome\//);
  assert.equal(typeof g.chrome.storage.managed, "object");
});
