"use strict";

// Deterministic synthetic DOM smoke for the immutable production asset. This
// file is eval-only, has no native/browser bridge, and is absent from Cargo.

const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

class NodeList {
  constructor(values = []) { this.values = values; }
  get length() { return this.values.length; }
  item(index) { return this.values[index] || null; }
}

class Node {
  constructor(type) {
    this._type = type;
    this._parent = null;
    this._owner = null;
    this._children = new NodeList();
  }
  get nodeType() { return this._type; }
  get parentNode() { return this._parent; }
  get ownerDocument() { return this._owner; }
  get isConnected() { return this === globalThis.document || this._owner === globalThis.document; }
  get childNodes() { return this._children; }
  append(child) {
    child._parent = this;
    child._owner = this instanceof Document ? this : this._owner;
    this._children.values.push(child);
    setOwner(child, child._owner);
    return child;
  }
}

function setOwner(node, owner) {
  node._owner = owner;
  for (const child of node._children.values) setOwner(child, owner);
  if (node instanceof Element && node._shadow !== null) setOwner(node._shadow, owner);
}

class CharacterData extends Node {
  constructor(data) { super(3); this._data = data; }
  get data() { return this._data; }
}

class Element extends Node {
  constructor(tag, attributes = {}) {
    super(1);
    this._tag = tag.toUpperCase();
    this.attributes = { ...attributes };
    this._shadow = null;
    this.rect = { x: 10, y: 10, width: 160, height: 32 };
  }
  get tagName() { return this._tag; }
  get shadowRoot() { return this._shadow; }
  getAttribute(name) {
    return Object.prototype.hasOwnProperty.call(this.attributes, name) ? this.attributes[name] : null;
  }
  hasAttribute(name) { return Object.prototype.hasOwnProperty.call(this.attributes, name); }
  getBoundingClientRect() { return this.rect; }
}

class ShadowRoot extends Node {
  constructor() { super(11); this._active = null; }
  get activeElement() { return this._active; }
}

class HTMLInputElement extends Element {
  constructor(attributes = {}, value = "") {
    super("input", attributes);
    this._value = value;
    this._checked = false;
  }
  get value() { return this._value; }
  get checked() { return this._checked; }
  get labels() { return new NodeList(); }
}

class HTMLTextAreaElement extends Element {
  get value() { return ""; }
  get labels() { return new NodeList(); }
}

class HTMLSelectElement extends Element {
  get selectedIndex() { return 0; }
  get labels() { return new NodeList(); }
}

class HTMLOptionElement extends Element {
  get index() { return 0; }
  get selected() { return false; }
}

class Document extends Node {
  constructor() {
    super(9);
    this._root = null;
    this._active = null;
  }
  get documentElement() { return this._root; }
  get activeElement() { return this._active; }
  getElementById(id) {
    const stack = this._root === null ? [] : [this._root];
    while (stack.length !== 0) {
      const node = stack.pop();
      if (node instanceof Element && node.getAttribute("id") === id) return node;
      for (const child of node.childNodes.values) stack.push(child);
    }
    return null;
  }
}

Object.assign(globalThis, {
  NodeList,
  Node,
  CharacterData,
  Element,
  ShadowRoot,
  HTMLInputElement,
  HTMLTextAreaElement,
  HTMLSelectElement,
  HTMLOptionElement,
  Document,
  innerWidth: 1280,
  innerHeight: 720,
  getComputedStyle() {
    return { display: "block", visibility: "visible", contentVisibility: "visible" };
  }
});

const document = new Document();
globalThis.document = document;
const html = new Element("html");
const body = new Element("body");
const main = new Element("main", { "aria-label": "Account" });
const heading = new Element("h1");
heading.append(new CharacterData("Dashboard"));
const button = new Element("button", { "aria-label": "Save" });
const password = new HTMLInputElement(
  { type: "password", "aria-label": "Password" },
  "never-cross-bridge"
);
const paragraph = new Element("p");
paragraph.append(new CharacterData("Normal private workspace text"));

const openHost = new Element("div");
openHost._shadow = new ShadowRoot();
openHost._shadow.append(new Element("button", { "aria-label": "Open shadow action" }));
const closedHost = new Element("div");
closedHost._closedInternal = new Element("button", { "aria-label": "Closed shadow secret" });

main.append(heading);
main.append(button);
main.append(password);
main.append(paragraph);
main.append(openHost);
main.append(closedHost);
body.append(main);
html.append(body);
document._root = html;
document.append(html);

// Own-property poisoning must not replace the captured document-start methods.
button.getAttribute = () => "poisoned";
button.getBoundingClientRect = () => ({ x: 0, y: 0, width: 0, height: 0 });

const sourcePath = path.resolve(
  process.argv[2] || "crates/zephium-agentic/assets/semantic-runtime-v1.js"
);
const source = fs.readFileSync(sourcePath, "utf8");
vm.runInThisContext(source, { filename: sourcePath });
globalThis.getComputedStyle = () => { throw new Error("late global poisoning"); };

const runtime = globalThis.__zephiumSemanticRuntimeV1;
assert(runtime && typeof runtime.invoke === "function", "runtime missing");

const invoke = (invocation, generation, scope, budget = {}) => runtime.invoke(JSON.stringify({
  v: 1,
  i: invocation,
  g: generation,
  s: scope,
  b: {
    n: budget.n || 128,
    t: budget.t || 16384,
    w: budget.w || 65536,
    x: budget.x || 16384,
    geo: budget.geo !== false
  }
}));

const initialWire = invoke(7, 1, { k: "initial" });
assert(!initialWire.startsWith("E1:"), initialWire);
const initial = JSON.parse(initialWire);
assert(initial.v === 1 && initial.i === 7 && initial.g === 1, "authority mismatch");
assert(initial.c === "complete", `unexpected completeness ${initial.c}`);
assert(!initialWire.includes("never-cross-bridge"), "password value crossed bridge");
assert(!initialWire.includes("Closed shadow secret"), "closed shadow root was bypassed");
assert(initial.n.some((node) => node.n === "Open shadow action"), "open shadow root missing");
assert(initial.n.some((node) => node.n === "Save"), "captured element methods were poisoned");
const passwordNode = initial.n.find((node) => node.r === "password");
assert(passwordNode && passwordNode.v.k === "redacted", "password not redacted");
const mainNode = initial.n.find((node) => node.r === "landmark");
assert(mainNode, "landmark missing");

const expansionWire = invoke(8, 2, { k: "region", a: mainNode.k }, { n: 64, t: 8192, w: 32768, x: 4096, geo: false });
assert(!expansionWire.startsWith("E1:"), expansionWire);
const expansion = JSON.parse(expansionWire);
assert(expansion.n[0].k === mainNode.k && expansion.n[0].p === undefined, "anchor identity changed");

const nodeLimited = JSON.parse(invoke(9, 3, { k: "initial" }, { n: 2, t: 1024, w: 4096, x: 64 }));
assert(nodeLimited.c === "node_limit" && nodeLimited.n.length === 2, "node limit not truthful");
const inspectionLimited = JSON.parse(invoke(10, 4, { k: "initial" }, { n: 1, t: 1, w: 1024, x: 1 }));
assert(inspectionLimited.c === "inspection_limit", "inspection limit not truthful");

for (let index = 0; index < 40; index += 1) {
  const extra = new Element("p");
  extra.append(new CharacterData(`row-${index}-${"x".repeat(180)}`));
  main.append(extra);
}
setOwner(main, document);
const wireLimited = invoke(11, 5, { k: "initial" }, { n: 128, t: 16384, w: 1024, x: 4096 });
assert(Buffer.byteLength(wireLimited) <= 1024, "wire ceiling exceeded");
assert(JSON.parse(wireLimited).c === "wire_limit", "wire limit not truthful");

assert(runtime.invoke("{}") === "E1:invalid_request", "invalid request accepted");
assert(
  runtime.invoke('{"v":1,"i":1,"g":1,"s":{"k":"initial"},"b":{"n":1,"t":1,"w":1024,"x":1,"geo":false},"selector":"*"}') ===
    "E1:invalid_request",
  "unknown request field accepted"
);

main._owner = null;
assert(invoke(12, 6, { k: "region", a: mainNode.k }) === "E1:anchor_missing", "detached anchor accepted");
setOwner(main, document);
const reattached = JSON.parse(invoke(13, 7, { k: "initial" }));
assert(
  reattached.n.some((node) => node.r === "landmark" && node.k === mainNode.k),
  "reattached stable identity changed"
);

const globalDescriptor = Object.getOwnPropertyDescriptor(globalThis, "__zephiumSemanticRuntimeV1");
assert(!globalDescriptor.writable && !globalDescriptor.configurable && !globalDescriptor.enumerable, "global mutable");
assert(Object.isFrozen(runtime) && Object.isFrozen(runtime.invoke), "runtime mutable");
assert(Object.keys(runtime).join(",") === "invoke", "unexpected runtime API");

process.stdout.write(`${JSON.stringify({
  schema: "zephium.agentic.semantic-runtime-smoke.v1",
  initial_nodes: initial.n.length,
  expanded_nodes: expansion.n.length,
  initial_bytes: Buffer.byteLength(initialWire),
  expanded_bytes: Buffer.byteLength(expansionWire),
  wire_limited_bytes: Buffer.byteLength(wireLimited),
  password_redacted: true,
  open_shadow_observed: true,
  closed_shadow_excluded: true,
  reattached_identity_preserved: true,
  immutable: true
})}\n`);

function assert(condition, message) {
  if (!condition) throw new Error(message);
}
