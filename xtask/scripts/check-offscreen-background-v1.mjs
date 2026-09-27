import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";

const source = fs.readFileSync(new URL("../../crates/zephium-extension-package/assets/macos/webkit-offscreen-background-v1.js", import.meta.url), "utf8");
const ports = [];
const chrome = { runtime: { id: "a".repeat(32), connectNative(name) {
  assert.equal(name, "app.zephium.extension-offscreen.v1");
  const messageListeners = [];
  const disconnectListeners = [];
  const port = {
    closed: false,
    sent: [],
    onMessage: { addListener(callback) { messageListeners.push(callback); } },
    onDisconnect: { addListener(callback) { disconnectListeners.push(callback); } },
    postMessage(wire) { this.sent.push(JSON.parse(wire)); },
    disconnect() { this.closed = true; for (const callback of disconnectListeners) callback(); },
    receive(frame) { for (const callback of messageListeners) callback(JSON.stringify(frame)); },
  };
  ports.push(port);
  return port;
} } };
const context = vm.createContext({ chrome, browser: chrome, TextEncoder, Symbol, Promise, setTimeout, clearTimeout });
vm.runInContext(source, context, { filename: "webkit-offscreen-background-v1.js" });
assert.equal(ports.length, 0);
assert.equal(chrome.offscreen.Reason.LOCAL_STORAGE, "LOCAL_STORAGE");
await assert.rejects(chrome.offscreen.createDocument({ url: "a.html", reasons: ["AUDIO_PLAYBACK"], justification: "x" }), /Unsupported/);
assert.equal(ports.length, 0);

const has = chrome.offscreen.hasDocument();
assert.equal(ports.length, 1);
assert.equal(ports[0].sent[0].operation, "has");
ports[0].receive({ v: 1, id: ports[0].sent[0].id, operation: "result", ok: true, value: false });
assert.equal(await has, false);
assert.equal(ports[0].closed, true);

const create = chrome.offscreen.createDocument({ url: "offscreen-document/index.html", reasons: ["LOCAL_STORAGE"], justification: "backup" });
assert.equal(ports.length, 2);
assert.equal(ports[1].sent[0].operation, "create");
ports[1].receive({ v: 1, id: ports[1].sent[0].id, operation: "result", ok: true, value: true });
await create;
const relay = context[Symbol.for("zephium.webkit-offscreen-relay.v1")];
const message = relay.dispatchToDocument({ command: "localStorageGet", key: "x" });
assert.equal(ports[1].sent[1].operation, "message");
ports[1].receive({ v: 1, id: ports[1].sent[1].id, operation: "result", ok: true, value: { handled: true, value: "x" } });
assert.equal((await message).value, "x");

const close = chrome.offscreen.closeDocument();
assert.equal(ports[1].sent[2].operation, "close");
ports[1].receive({ v: 1, id: ports[1].sent[2].id, operation: "result", ok: true, value: true });
await close;
assert.equal(ports[1].closed, true);
assert.equal((await relay.dispatchToDocument({ command: "x" })).handled, false);
console.log("offscreen background v1: on-demand port, storage reason, message, close passed");
