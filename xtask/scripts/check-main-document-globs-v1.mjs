import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const asset = new URL(
  "../../crates/zephium-extension-package/assets/macos/webkit-glob-worker-v1.js",
  import.meta.url,
);
const template = await readFile(fileURLToPath(asset), "utf8");
const routes = [
  {
    index: 0,
    primary: [{ all: true }],
    primaryExcludes: [
      { scheme: "https", hostKind: "exact", host: "mail.example.test", port: null, path: "/*" },
      { scheme: "https", hostKind: "exact", host: "example.test", port: null, path: "/path?" },
    ],
    include: [],
    exclude: ["*docs.example.test*", "*blocked.example.test*"],
    files: ["original-styles.js", "original-script.js"],
  },
  {
    index: 1,
    primary: [{ scheme: "https", hostKind: "exact", host: "mail.example.test", port: null, path: "/*" }],
    primaryExcludes: [],
    include: [],
    exclude: [],
    files: ["special.js"],
  },
  {
    index: 2,
    primary: [{ scheme: "https", hostKind: "exact", host: "overlap.example.test", port: null, path: "/*" }],
    primaryExcludes: [],
    include: [],
    exclude: [],
    files: ["later.js"],
  },
  {
    index: 3,
    primary: [{ scheme: "https", hostKind: "exact", host: "blocked.example.test", port: null, path: "/*" }],
    primaryExcludes: [],
    include: [],
    exclude: [],
    files: ["allowed-after-first-glob-denial.js"],
  },
];
let listener;
const injections = [];
let resultDocument = "document-1234567890";
const context = vm.createContext({
  URL,
  Promise,
  chrome: {
    runtime: { onMessage: { addListener(callback) { listener = callback; } } },
    scripting: { executeScript(args) {
      injections.push(args);
      return Promise.resolve(args.files.map(() => ({ documentId: resultDocument, frameId: 0 })));
    } },
  },
});
new vm.Script(template.replace("__ZEPHIUM_GLOB_ROUTES__", JSON.stringify(routes)), {
  filename: fileURLToPath(asset),
}).runInContext(context, { timeout: 1_000 });

function send(route, url, frameId = 0) {
  return new Promise((resolve) => listener(
    { kind: "zephium-main-document-glob-v1", route, url },
    { frameId, tab: { id: 7 }, documentId: "document-1234567890", url, origin: new URL(url).origin },
    resolve,
  ));
}

assert.equal((await send(0, "https://example.test/editor")).ok, true);
assert.deepEqual(Array.from(injections[0].files), ["original-styles.js", "original-script.js"]);
assert.deepEqual(Array.from(injections[0].target.documentIds), ["document-1234567890"]);
assert.equal((await send(0, "https://mail.example.test/inbox")).ok, false,
  "a same-extension sender from another route bypassed primary exclude_matches");
assert.equal((await send(1, "https://mail.example.test/inbox")).ok, true);
assert.equal((await send(0, "https://example.test/?next=docs.example.test")).ok, false);
assert.equal((await send(0, "https://example.test/path?")).ok, false,
  "an explicitly empty query bypassed exclude_matches");
assert.equal((await send(0, "https://example.test/editor", 1)).ok, false);
assert.equal((await send(0, `https://example.test/${"x".repeat(32769)}`)).ok, false);
assert.equal((await send(0, "https://example.test/editor#fragment")).ok, false);
resultDocument = "different-document-1234567890";
assert.equal((await send(0, "https://example.test/editor")).ok, false,
  "a result from a different document was accepted");
assert.equal(injections.length, 3);
resultDocument = "document-1234567890";
assert.equal((await send(2, "https://overlap.example.test/editor")).ok, true);
assert.deepEqual(Array.from(injections[3].files), ["original-styles.js", "original-script.js"]);
assert.deepEqual(Array.from(injections[4].files), ["later.js"]);
assert.equal(injections.length, 5);
assert.equal((await send(0, "https://blocked.example.test/editor")).ok, true,
  "the first primary match suppressed a later route after its glob denied");
assert.deepEqual(Array.from(injections[5].files), ["allowed-after-first-glob-denial.js"]);
assert.equal(injections.length, 6);

const bootstrapAsset = new URL(
  "../../crates/zephium-extension-package/assets/macos/webkit-glob-bootstrap-v1.js",
  import.meta.url,
);
const bootstrap = await readFile(fileURLToPath(bootstrapAsset), "utf8");
let sends = 0;
const documentWorld = vm.createContext({
  Symbol,
  location: { href: "https://overlap.example.test/editor" },
  chrome: { runtime: { sendMessage() { sends++; return Promise.resolve(); } } },
});
new vm.Script(bootstrap.replace("__ZEPHIUM_ROUTE__", "2")).runInContext(documentWorld);
new vm.Script(bootstrap.replace("__ZEPHIUM_ROUTE__", "0")).runInContext(documentWorld);
assert.equal(sends, 1, "overlapping bootstraps sent more than one worker request");
console.log("main-document glob worker exact-scope contract passed");
