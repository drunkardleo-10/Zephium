import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";

const source = fs.readFileSync(
  new URL("../../crates/zephium-extension-package/assets/macos/webkit-runtime-messaging-v2.js", import.meta.url),
  "utf8",
);
const event = () => {
  const listeners = [];
  return {
    listeners,
    addListener(listener) { listeners.push(listener); },
    removeListener(listener) {
      const index = listeners.indexOf(listener);
      if (index >= 0) listeners.splice(index, 1);
    },
    hasListener(listener) { return listeners.includes(listener); },
  };
};
const onMessage = event();
const onConnect = event();
const values = new Map();
let writes = 0;
const runtime = {
  id: "a".repeat(32),
  onMessage,
  onConnect,
  connect() {},
  sendMessage() {},
};
const chrome = {
  runtime,
  storage: {
    session: {
      async get(key) { return values.has(key) ? { [key]: values.get(key) } : {}; },
      async set(entries) {
        writes += 1;
        for (const [key, value] of Object.entries(entries)) values.set(key, value);
      },
      async remove(key) { values.delete(key); },
    },
  },
};
const context = vm.createContext({
  chrome, browser: chrome, Symbol, Promise, TextEncoder, setTimeout, clearTimeout,
});
vm.runInContext(source, context, { filename: "webkit-runtime-messaging-v2.js" });
const dispatch = context[Symbol.for("zephium.webkit-runtime-message-dispatch.v2")];
const sender = { id: runtime.id, url: `webkit-extension://${runtime.id}/offscreen.html` };

let invoked = 0;
const slower = (_, __, respond) => {
  invoked += 1;
  setTimeout(() => respond("slow"), 20);
  return true;
};
const faster = () => {
  invoked += 1;
  return Promise.resolve("fast");
};
runtime.onMessage.addListener(slower);
runtime.onMessage.addListener(faster);
assert.equal((await dispatch({ request: 1 }, sender)).value, "fast");
assert.equal(invoked, 2);

const port = {
  name: "__zephium_runtime_message_v2__",
  sender,
  onMessage: event(),
  disconnect() {},
};
for (const listener of onConnect.listeners) listener(port);
assert.equal(port.onMessage.listeners.length, 1);
port.onMessage.listeners[0]({ v: 1, id: "response-1", message: { request: 2 } });
await new Promise((resolve) => setTimeout(resolve, 5));
assert.equal(values.get("__zephium_runtime_message_response_v2__response-1")?.value, "fast");
await new Promise((resolve) => setTimeout(resolve, 25));
assert.equal(writes, 1);

runtime.onMessage.removeListener(slower);
runtime.onMessage.removeListener(faster);
let secondInvoked = false;
runtime.onMessage.addListener((_, __, respond) => { respond("first"); });
runtime.onMessage.addListener(() => { secondInvoked = true; return Promise.resolve("later"); });
assert.equal((await dispatch({ request: 3 }, sender)).value, "first");
assert.equal(secondInvoked, true);
assert.throws(() => dispatch({}, { id: "b".repeat(32), url: sender.url }), /Invalid offscreen sender/);
console.log("runtime messaging v2: first response wins across callback and Promise listeners");
