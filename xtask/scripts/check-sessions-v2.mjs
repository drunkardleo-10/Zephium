import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const source = await readFile(
  new URL("../../crates/zephium-extension-package/assets/macos/webkit-sessions-v2.js", import.meta.url),
  "utf8",
);
const id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const requests = [];
let malformed = false;
let denied = false;
const runtime = {
  id: "fixture",
  sendNativeMessage(app, wire, callback) {
    requests.push({ app, wire });
    if (denied) {
      this.lastError = { message: "stale session" };
      callback(null);
      delete this.lastError;
      return;
    }
    if (malformed) {
      callback('{"v":2,"items":[{"window":{}}]}');
      return;
    }
    callback(
      JSON.stringify(
        wire.startsWith("v2/sessions.recent/")
          ? {
              v: 2,
              items: [
                {
                  sessionId: id,
                  url: "https://example.test/closed",
                  title: "Closed tab",
                  lastModified: 1_700_000_000,
                },
              ],
            }
          : {
              v: 2,
              restored: {
                url: "https://example.test/closed",
                title: "Closed tab",
                lastModified: 1_700_000_000,
              },
            },
      ),
    );
  },
};
const namespace = { runtime };
const context = vm.createContext({ chrome: namespace, browser: namespace });
vm.runInContext(source, context, { timeout: 1_000 });
const sessions = namespace.sessions;
assert.equal(sessions.MAX_SESSION_RESULTS, 25);
assert.equal(context[Symbol.for("zephium.webkit-sessions-compatibility.mode.v2")], "local-current-profile-space-closed-tabs-only");

const listed = await sessions.getRecentlyClosed();
assert.equal(requests[0].app, "app.zephium.extension-broker.v1");
assert.equal(requests[0].wire, "v2/sessions.recent/25");
assert.equal(listed.length, 1);
assert.equal(listed[0].tab.sessionId, id);
assert.equal(listed[0].lastModified, 1_700_000_000);
assert.equal("window" in listed[0], false);
assert.equal("id" in listed[0].tab, false);
assert.equal("favIconUrl" in listed[0].tab, false);

const callbackList = await new Promise((resolve) => {
  assert.equal(sessions.getRecentlyClosed({ maxResults: 2 }, resolve), undefined);
});
assert.equal(callbackList.length, 1);
assert.equal(requests[1].wire, "v2/sessions.recent/2");

const restored = await sessions.restore(id);
assert.equal(requests[2].wire, `v2/sessions.restore/${id}`);
assert.equal(restored.tab.url, "https://example.test/closed");
assert.equal("sessionId" in restored.tab, false);
assert.equal("windowId" in restored.tab, false);
const callbackRestore = await new Promise((resolve) => {
  assert.equal(sessions.restore(resolve), undefined);
});
assert.equal(requests[3].wire, "v2/sessions.restore/recent");
assert.equal(callbackRestore.lastModified, 1_700_000_000);

assert.throws(() => sessions.getRecentlyClosed({ maxResults: 26 }), /maxResults/);
assert.throws(() => sessions.getRecentlyClosed({ maxResults: 0 }), /maxResults/);
assert.throws(() => sessions.restore("not-a-session"), /canonical session ID/);
assert.equal(requests.length, 4, "invalid arguments cannot reach the native broker");
malformed = true;
await assert.rejects(sessions.getRecentlyClosed(), /invalid list contract/);
malformed = false;
denied = true;
await assert.rejects(sessions.restore(id), /stale session/);
const deniedCallback = await new Promise((resolve) => {
  sessions.restore(id, (value) => resolve({ value, error: runtime.lastError?.message }));
});
assert.equal(deniedCallback.value, null);
assert.equal(deniedCallback.error, "stale session");
denied = false;
vm.runInContext(source, context, { timeout: 1_000 });
assert.equal(namespace.sessions, sessions, "the adapter installs only once");

const blocked = vm.createContext({ chrome: Object.freeze({ runtime, sessions: {} }) });
assert.throws(() => vm.runInContext(source, blocked, { timeout: 1_000 }), /cannot install safely/);
console.log("Sessions v2 wire, scope and callback contract passed");
