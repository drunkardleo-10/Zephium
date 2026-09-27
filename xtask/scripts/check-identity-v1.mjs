import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const asset = new URL(
  "../../crates/zephium-extension-package/assets/macos/webkit-identity-v1.js",
  import.meta.url,
);
const template = await readFile(fileURLToPath(asset), "utf8");
const id = "abcdefghijklmnopabcdefghijklmnop";
assert.equal(template.match(/__ZEPHIUM_CHROMIUM_ID__/g)?.length, 1);
assert.equal(template.match(/app\.zephium\.extension-identity\.v1/g)?.length, 1);
for (const forbidden of ["getAuthToken", "fetch(", "XMLHttpRequest", "WebSocket", "connectNative", "setInterval("]) {
  assert.equal(template.includes(forbidden), false, `identity bridge exposes ${forbidden}`);
}
const source = template.replace("__ZEPHIUM_CHROMIUM_ID__", id);
const script = new vm.Script(source, { filename: fileURLToPath(asset) });
const requests = [];
let nativeResponse = `https://${id}.chromiumapp.org/cb?code=a%2Fb#state=opaque`;
let nativeFailure = false;
const runtime = {
  id: "webkit-runtime-id-is-not-the-chromium-id",
  sendNativeMessage(application, wire, callback) {
    assert.equal(application, "app.zephium.extension-identity.v1");
    const parsed = JSON.parse(wire);
    assert.deepEqual(Object.keys(parsed).sort(), [
      "abortOnLoadForNonInteractive", "interactive", "kind", "timeoutMsForNonInteractive", "url", "v",
    ]);
    requests.push(parsed);
    if (nativeFailure) {
      runtime.lastError = { message: "Interactive identity flow is unavailable" };
      callback(undefined);
      delete runtime.lastError;
    } else {
      callback(nativeResponse);
    }
  },
};
const sandbox = vm.createContext({ chrome: { runtime }, URL, Promise, Symbol });
script.runInContext(sandbox, { timeout: 1_000 });
const identity = sandbox.chrome.identity;
assert.equal(identity.getRedirectURL(), `https://${id}.chromiumapp.org/`);
assert.equal(identity.getRedirectURL("cb"), `https://${id}.chromiumapp.org/cb`);
assert.equal(identity.getRedirectURL("/cb"), `https://${id}.chromiumapp.org/cb`);
assert.equal("getAuthToken" in identity, false);
assert.equal(
  await identity.launchWebAuthFlow({ url: "https://accounts.example.test/login" }),
  nativeResponse,
);
assert.equal(requests[0].interactive, false);
assert.equal(requests[0].abortOnLoadForNonInteractive, true);
assert.equal(requests[0].timeoutMsForNonInteractive, 30_000);

let callbackResult = null;
identity.launchWebAuthFlow(
  { url: "https://accounts.example.test/login", interactive: false },
  (result) => { callbackResult = result; },
);
assert.equal(callbackResult, nativeResponse);

nativeResponse = "https://bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.chromiumapp.org/cb?code=foreign";
await assert.rejects(
  identity.launchWebAuthFlow({ url: "https://accounts.example.test/login" }),
  /foreign redirect/,
);
nativeFailure = true;
await assert.rejects(
  identity.launchWebAuthFlow({ url: "https://accounts.example.test/login", interactive: true }),
  /Interactive identity flow is unavailable/,
);
assert.equal(requests.at(-1).interactive, true);
assert.throws(
  () => identity.launchWebAuthFlow({ url: "https://accounts.example.test/login", timeoutMsForNonInteractive: 0 }, () => {}),
  /details are invalid/,
);
console.log("macOS identity v1 bridge contract passed");
