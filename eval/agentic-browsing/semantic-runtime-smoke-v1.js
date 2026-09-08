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

class Event {
  constructor(type, init = {}) {
    this.type = type;
    this.bubbles = init.bubbles === true;
    this.cancelable = init.cancelable === true;
    this.composed = init.composed === true;
    this.defaultPrevented = false;
    this.isTrusted = false;
    this.target = null;
    this.currentTarget = null;
  }
  preventDefault() {
    if (this.cancelable) this.defaultPrevented = true;
  }
}

class InputEvent extends Event {
  constructor(type, init = {}) {
    super(type, init);
    this.data = init.data === undefined ? null : init.data;
    this.inputType = init.inputType || "";
    this.isComposing = init.isComposing === true;
  }
}

class EventTarget {
  constructor() { this._listeners = new Map(); }
  addEventListener(type, listener) {
    const listeners = this._listeners.get(type) || [];
    listeners.push(listener);
    this._listeners.set(type, listeners);
  }
  dispatchEvent(event) {
    if (this._nativeDispatchThrows) throw new Error("native dispatch failure");
    event.target = this;
    event.currentTarget = this;
    for (const listener of this._listeners.get(event.type) || []) listener.call(this, event);
    return !event.defaultPrevented;
  }
}

class Node extends EventTarget {
  constructor(type) {
    super();
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
  contains(candidate) {
    const stack = [this];
    while (stack.length !== 0) {
      const current = stack.pop();
      if (current === candidate) return true;
      for (const child of current._children.values) stack.push(child);
    }
    return false;
  }
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
  get tagName() {
    // Web IDL getters reject non-implementing receivers. In particular a
    // retained Document ancestor is a Node, never an Element.
    if (!(this instanceof Element)) throw new TypeError("Element receiver required");
    return this._tag;
  }
  get shadowRoot() { return this._shadow; }
  getAttribute(name) {
    return Object.prototype.hasOwnProperty.call(this.attributes, name) ? this.attributes[name] : null;
  }
  hasAttribute(name) { return Object.prototype.hasOwnProperty.call(this.attributes, name); }
  setAttribute(name, value) {
    this.attributes[name] = String(value);
    if (typeof globalThis.__semanticRelayMutation === "function") {
      globalThis.__semanticRelayMutation(this, name);
    }
  }
  removeAttribute(name) { delete this.attributes[name]; }
  getBoundingClientRect() { return this.rect; }
  click() { this._fixedClickCount = (this._fixedClickCount || 0) + 1; }
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
    this._labels = new NodeList();
  }
  get value() { return this._value; }
  set value(value) { this._value = String(value); }
  get checked() { return this._checked; }
  get labels() { return this._labels; }
}

class HTMLAnchorElement extends Element {
  constructor(attributes = {}) { super("a", attributes); }
  get href() {
    if (!(this instanceof HTMLAnchorElement)) throw new TypeError("Anchor receiver required");
    return this.attributes.href || "";
  }
}

class HTMLTextAreaElement extends Element {
  constructor(attributes = {}, value = "") {
    super("textarea", attributes);
    this._value = value;
  }
  get value() { return this._value; }
  set value(value) { this._value = String(value).replace(/\r\n?/g, "\n"); }
  get labels() { return new NodeList(); }
}

class HTMLSelectElement extends Element {
  constructor(attributes = {}, selectedIndex = 0) {
    super("select", attributes);
    this._selectedIndex = selectedIndex;
    this._labels = new NodeList();
  }
  get selectedIndex() { return this._selectedIndex; }
  set selectedIndex(value) {
    this._selectedIndex = this._rewriteAfterSet ? 0 : Number(value);
  }
  get labels() { return this._labels; }
}

class HTMLOptionElement extends Element {
  constructor(attributes = {}, index = 0, selected = false, label = "") {
    super("option", attributes);
    this._index = index;
    this._selected = selected;
    this._label = label;
  }
  get index() { return this._index; }
  get label() { return this.attributes.label || this._label; }
  get selected() {
    return this._parent instanceof HTMLSelectElement
      ? this._parent._selectedIndex === this._index
      : this._selected;
  }
}

class Document extends Node {
  constructor() {
    super(9);
    this._root = null;
    this._active = null;
    this._ready = "complete";
    this._hit = null;
  }
  get documentElement() { return this._root; }
  get activeElement() { return this._active; }
  get readyState() { return this._ready; }
  getElementById(id) {
    const stack = this._root === null ? [] : [this._root];
    while (stack.length !== 0) {
      const node = stack.pop();
      if (node instanceof Element && node.getAttribute("id") === id) return node;
      for (const child of node.childNodes.values) stack.push(child);
    }
    return null;
  }
  elementFromPoint() { return this._hit; }
}

Object.assign(globalThis, {
  Event,
  InputEvent,
  EventTarget,
  NodeList,
  Node,
  CharacterData,
  Element,
  HTMLElement: Element,
  ShadowRoot,
  HTMLInputElement,
  HTMLAnchorElement,
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
Object.defineProperty(globalThis, "crypto", {
  value: Object.freeze({}),
  configurable: true,
  enumerable: false,
  writable: false
});

const document = new Document();
globalThis.document = document;
const html = new Element("html");
const body = new Element("body");
const main = new Element("main", { "aria-label": "Account" });
const heading = new Element("h1");
heading.append(new CharacterData("Dashboard"));
const button = new Element("button", { "aria-label": "Save" });
const textInput = new HTMLInputElement(
  { type: "text", "aria-label": "Account name" },
  "fixture text"
);
const searchInput = new HTMLInputElement(
  { type: "search", "aria-label": "Account search" },
  "fixture search"
);
const textarea = new HTMLTextAreaElement(
  { "aria-label": "Account notes" },
  "fixture notes"
);
const contentEditable = new Element("div", {
  contenteditable: "true",
  role: "textbox",
  "aria-label": "Rich account notes"
});
const password = new HTMLInputElement(
  { type: "password", "aria-label": "Password" },
  "never-cross-bridge"
);
const lateSecretInput = new HTMLInputElement(
  { type: "text", "aria-label": "Large ordinary field" },
  `${"x".repeat(3900)} sk-super-secret-value`
);
const oversizedValueInput = new HTMLInputElement(
  { type: "text", "aria-label": "Oversized ordinary field" },
  "x".repeat(65537)
);
const invalidValueInputs = [
  ["NUL value field", "safe\u0000suffix"],
  ["CR value field", "safe\rsuffix"],
  ["Bidi value field", "safe\u202esuffix"],
  ["Surrogate value field", "safe\ud800suffix"]
].map(([name, value]) => new HTMLInputElement(
  { type: "text", "aria-label": name },
  value
));
const metadataOverflowValues = [
  "attribute-overflow-value-must-not-cross",
  "aria-overflow-value-must-not-cross",
  "label-overflow-value-must-not-cross"
];
const attributeOverflowInput = new HTMLInputElement(
  {
    type: "text",
    "aria-label": "Attribute overflow field",
    name: `${"x".repeat(250)}api key`
  },
  metadataOverflowValues[0]
);
const ariaOverflowInput = new HTMLInputElement(
  { type: "text", "aria-label": `${"x".repeat(506)}api key` },
  metadataOverflowValues[1]
);
const labelOverflowInput = new HTMLInputElement(
  { type: "text", "aria-label": "Associated label overflow field" },
  metadataOverflowValues[2]
);
const overflowingLabel = new Element("label");
overflowingLabel.append(new CharacterData(`${"x".repeat(506)}api key`));
labelOverflowInput._labels = new NodeList([overflowingLabel]);
const paragraph = new Element("p");
paragraph.append(new CharacterData("Normal private workspace text"));
const languageSelect = new HTMLSelectElement({ "aria-label": "Language" });
const englishOption = new HTMLOptionElement({}, 0, true, "English");
const germanOption = new HTMLOptionElement({}, 1, false, "Deutsch");
englishOption.rect = { x: 0, y: 0, width: 0, height: 0 };
germanOption.rect = { x: 0, y: 0, width: 0, height: 0 };
languageSelect.append(englishOption);
languageSelect.append(germanOption);
const offscreenSelect = new HTMLSelectElement({ "aria-label": "Offscreen language" });
offscreenSelect.rect = { x: 10, y: 900, width: 160, height: 32 };
const offscreenOption = new HTMLOptionElement({}, 0, false, "Must stay filtered");
offscreenOption.rect = { x: 0, y: 0, width: 0, height: 0 };
offscreenSelect.append(offscreenOption);
const documentParentSelect = new HTMLSelectElement({ "aria-label": "Document-parent select" });
documentParentSelect.rect = { x: 10, y: 900, width: 160, height: 32 };
const documentParentOption = new HTMLOptionElement({}, 0, false, "Document-parent option must stay filtered");
documentParentOption.rect = { x: 0, y: 0, width: 0, height: 0 };
documentParentSelect.append(documentParentOption);

const openHost = new Element("div");
openHost._shadow = new ShadowRoot();
openHost._shadow.append(new Element("button", { "aria-label": "Open shadow action" }));
const closedHost = new Element("div");
closedHost._closedInternal = new Element("button", { "aria-label": "Closed shadow secret" });

const fillEvents = new Map();
for (const target of [textInput, searchInput, textarea]) {
  const events = [];
  fillEvents.set(target, events);
  target.addEventListener("beforeinput", (event) => {
    events.push(event);
    if (target._cancelBeforeInput) event.preventDefault();
    if (target._repurposeAfterBeforeInput) target.attributes.type = "password";
  });
  target.addEventListener("input", (event) => {
    events.push(event);
    if (target._rewriteAfterInput) target._value = "page rewrite";
    if (target._repurposeAfterInput) target.attributes["aria-label"] = "Password replacement";
  });
  target.addEventListener("change", (event) => events.push(event));
}

const relayReady = "data-zephium-fill-relay-ready-v1";
const relayCommand = "data-zephium-fill-relay-command-v1";
const relayTerminal = "data-zephium-fill-relay-terminal-v1";
let relayMode = "normal";
html.attributes[relayReady] = "1";
const relayDispatch = EventTarget.prototype.dispatchEvent;
const relayInputValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value");
const relayTextareaValue = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value");
globalThis.__semanticRelayMutation = (target, name) => {
  if (name !== relayCommand || relayMode === "absent") return;
  const raw = target.getAttribute(relayCommand);
  if (raw === null) return;
  target.removeAttribute(relayCommand);
  if (relayMode === "duplicate") {
    target.setAttribute(relayTerminal, "1|0|duplicate");
    return;
  }
  if (relayMode === "flood") return;
  let command;
  try { command = JSON.parse(raw); } catch (_) {
    target.setAttribute(relayTerminal, "1|0|invalid");
    return;
  }
  const input = Object.getPrototypeOf(target) === HTMLInputElement.prototype;
  const textarea = Object.getPrototypeOf(target) === HTMLTextAreaElement.prototype;
  if ((!input && !textarea) ||
      (input && target.getAttribute("type") !== "text" && target.getAttribute("type") !== "search")) {
    target.setAttribute(relayTerminal, `1|${command.a}|refused`);
    return;
  }
  try {
    const before = new InputEvent("beforeinput", {
      bubbles: true, cancelable: true, composed: true, data: command.z,
      inputType: "insertReplacementText", isComposing: false
    });
    if (!Reflect.apply(relayDispatch, target, [before])) {
      target.setAttribute(relayTerminal, `1|${command.a}|indeterminate`);
      return;
    }
    if (
      Object.getPrototypeOf(target) !== (input ? HTMLInputElement.prototype : HTMLTextAreaElement.prototype) ||
      (input && target.getAttribute("type") !== "text" && target.getAttribute("type") !== "search") ||
      target.hasAttribute("disabled") || target.hasAttribute("readonly")
    ) {
      target.setAttribute(relayTerminal, `1|${command.a}|indeterminate`);
      return;
    }
    const valueDescriptor = input ? relayInputValue : relayTextareaValue;
    Reflect.apply(valueDescriptor.set, target, [command.z]);
    Reflect.apply(relayDispatch, target, [new InputEvent("input", {
      bubbles: true, cancelable: false, composed: true, data: command.z,
      inputType: "insertReplacementText", isComposing: false
    })]);
    target.setAttribute(
      relayTerminal,
      `1|${command.a}|${Reflect.apply(valueDescriptor.get, target, []) === command.z ? "ok" : "indeterminate"}`
    );
  } catch (_) {
    target.setAttribute(relayTerminal, `1|${command.a}|indeterminate`);
  }
};

const nativeTransportResults = [];
let nativeTransportPulls = 0;
let stopNativeTransport = null;
globalThis.webkit = {
  messageHandlers: {
    zephiumSemanticRuntimeV1: {
      postMessage(message) {
        if (message === "P1") {
          nativeTransportPulls += 1;
          if (nativeTransportPulls === 1) {
            return Promise.resolve(JSON.stringify({
              v: 1,
              i: 101,
              g: 101,
              s: { k: "initial" },
              b: { n: 128, t: 16384, w: 65536, x: 512, geo: false }
            }));
          }
          return new Promise((resolve) => { stopNativeTransport = resolve; });
        }
        if (typeof message === "string" && message.startsWith("R1:")) {
          nativeTransportResults.push(message.slice(3));
          return Promise.resolve("A1");
        }
        return Promise.reject(new Error("unexpected semantic transport message"));
      }
    }
  }
};

main.append(heading);
main.append(button);
main.append(textInput);
main.append(searchInput);
main.append(textarea);
main.append(contentEditable);
main.append(password);
main.append(lateSecretInput);
main.append(oversizedValueInput);
for (const invalidValueInput of invalidValueInputs) main.append(invalidValueInput);
main.append(attributeOverflowInput);
main.append(labelOverflowInput);
main.append(overflowingLabel);
main.append(paragraph);
main.append(languageSelect);
main.append(offscreenSelect);
main.append(openHost);
main.append(closedHost);
body.append(main);
// A filtered select need not have a retained Element ancestor. Its nearest
// semantic parent here is the Document record, which cannot receive tagName.
body.append(documentParentSelect);
html.append(body);
document._root = html;
document.append(html);
document._hit = button;

// Own-property poisoning must not replace the captured document-start methods.
button.getAttribute = () => "poisoned";
button.getBoundingClientRect = () => ({ x: 0, y: 0, width: 0, height: 0 });

const sourcePath = path.resolve(
  process.argv[2] || "crates/zephium-agentic/assets/semantic-runtime-v1.js"
);
const negativeControl = process.argv[3] === "--without-document-parent-brand";
assert(process.argv.length <= 4 && (process.argv[3] === undefined || negativeControl), "unknown smoke mode");
let source = fs.readFileSync(sourcePath, "utf8");
if (negativeControl) {
  // Mutate only this process's program string. Exactly one known guard must
  // match; refactors fail visibly rather than silently skipping the control.
  const guard = /nodeType\(\s*records\[parent\]\.element\s*\)\s*===\s*1\s*&&/g;
  assert((source.match(guard) || []).length === 1, "negative-control guard is not unique");
  source = source.replace(guard, "true &&");
}
vm.runInThisContext(source, { filename: sourcePath });
Object.defineProperty(document, "nodeType", { value: 1 });
Object.defineProperty(document, "tagName", { value: "SELECT" });
globalThis.getComputedStyle = () => { throw new Error("late global poisoning"); };
Object.defineProperty(textInput, "value", {
  get() { throw new Error("own input getter poison"); },
  set() { throw new Error("own input setter poison"); },
  configurable: true
});
Object.defineProperty(HTMLTextAreaElement.prototype, "value", {
  get() { throw new Error("prototype textarea getter poison"); },
  set() { throw new Error("prototype textarea setter poison"); },
  configurable: true
});
EventTarget.prototype.dispatchEvent = function poisonedDispatch() {
  throw new Error("prototype dispatch poison");
};
globalThis.InputEvent = function PoisonedInputEvent() {
  throw new Error("global InputEvent poison");
};

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
if (negativeControl) {
  assert(initialWire === "E1:internal", "missing parent guard did not reproduce the closed fault");
  process.stdout.write("semantic-runtime negative control: missing parent brand guard reproduces E1:internal\n");
  process.exit(0);
}
assert(!initialWire.startsWith("E1:"), initialWire);
const initial = JSON.parse(initialWire);
assert(initial.v === 1 && initial.i === 7 && initial.g === 1, "authority mismatch");
assert(initial.c === "complete", `unexpected completeness ${initial.c}`);
assert(!initialWire.includes("never-cross-bridge"), "password value crossed bridge");
assert(!initialWire.includes("Closed shadow secret"), "closed shadow root was bypassed");
assert(initial.n.some((node) => node.n === "Open shadow action"), "open shadow root missing");
assert(initial.n.some((node) => node.n === "Save"), "captured element methods were poisoned");
const languageNode = initial.n.find((node) => node.n === "Language");
assert(languageNode && languageNode.r === "combobox", "visible native select missing");
const languageNodeIndex = initial.n.indexOf(languageNode);
const englishNode = initial.n.find((node) => node.n === "English");
const germanNode = initial.n.find((node) => node.n === "Deutsch");
assert(
  englishNode && germanNode &&
    englishNode.r === "option" && germanNode.r === "option" &&
    englishNode.p === languageNodeIndex && germanNode.p === languageNodeIndex &&
    englishNode.b === undefined && germanNode.b === undefined,
  "non-geometric options of an admitted native select were not retained"
);
assert(
  !initial.n.some((node) => node.n === "Must stay filtered"),
  "option of an offscreen native select escaped initial filtering"
);
assert(
  !initial.n.some((node) => node.n === "Document-parent option must stay filtered"),
  "a Document ancestor was mistaken for an admitted native select"
);
const passwordNode = initial.n.find((node) => node.r === "password");
assert(passwordNode && passwordNode.v.k === "redacted", "password not redacted");
const lateSecretNode = initial.n.find((node) => node.n === "Large ordinary field");
assert(
  lateSecretNode && lateSecretNode.v.k === "redacted" && lateSecretNode.q === "secret",
  "secret after the model-preview ceiling was exposed"
);
assert(!initialWire.includes("sk-super-secret-value"), "late secret crossed the wire");
const oversizedValueNode = initial.n.find((node) => node.n === "Oversized ordinary field");
assert(
  oversizedValueNode && oversizedValueNode.v.k === "redacted" && oversizedValueNode.q === "secret",
  "value beyond the exact-value ceiling was not conservatively redacted"
);
for (const [name] of [
  ["NUL value field"],
  ["CR value field"],
  ["Bidi value field"],
  ["Surrogate value field"]
]) {
  const invalidValueNode = initial.n.find((node) => node.n === name);
  assert(
    invalidValueNode && invalidValueNode.v.k === "redacted" && invalidValueNode.q === "secret",
    `${name} crossed the exact-value boundary`
  );
}
for (const privateValue of metadataOverflowValues) {
  assert(!initialWire.includes(privateValue), "credential-metadata overflow exposed its value");
}
assert(
  initial.n.filter((node) =>
    node.r === "textbox" && node.v && node.v.k === "redacted" && node.q === "secret"
  ).length >= 8,
  "overflow or late-marker credential metadata was not marked secret"
);
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
for (const primitive of ["null", "true", "1", '"text"', "[]"]) {
  assert(runtime.invoke(primitive) === "E1:invalid_request", `primitive request accepted: ${primitive}`);
}
assert(
  runtime.invoke('{"o":"action_execute"}') === "E1:invalid_request",
  "malformed action request accepted"
);
assert(runtime.invoke("{") === "E1:invalid_request", "malformed JSON request accepted");
document._ready = "loading";
assert(invoke(12, 6, { k: "initial" }) === "E1:document_loading", "loading document accepted");
document._ready = "complete";
assert(
  runtime.invoke('{"v":1,"i":1,"g":1,"s":{"k":"initial"},"b":{"n":1,"t":1,"w":1024,"x":1,"geo":false},"selector":"*"}') ===
    "E1:invalid_request",
  "unknown request field accepted"
);

main._owner = null;
assert(invoke(13, 7, { k: "region", a: mainNode.k }) === "E1:anchor_missing", "detached anchor accepted");
setOwner(main, document);
const reattached = JSON.parse(invoke(14, 8, { k: "initial" }));
assert(
  reattached.n.some((node) => node.r === "landmark" && node.k === mainNode.k),
  "reattached stable identity changed"
);

const globalDescriptor = Object.getOwnPropertyDescriptor(globalThis, "__zephiumSemanticRuntimeV1");
assert(!globalDescriptor.writable && !globalDescriptor.configurable && !globalDescriptor.enumerable, "global mutable");
assert(Object.isFrozen(runtime) && Object.isFrozen(runtime.invoke), "runtime mutable");
assert(Object.keys(runtime).join(",") === "invoke", "unexpected runtime API");

finish().catch((error) => {
  process.stderr.write(`${error.stack || error}\n`);
  process.exitCode = 1;
});

async function finish() {
  await new Promise((resolve) => setImmediate(resolve));
  assert(nativeTransportResults.length === 1, "native transport did not settle exactly once");
  const transported = JSON.parse(nativeTransportResults[0]);
  assert(
    transported.i === 101 && transported.g === 101 && transported.n.length > 0,
    "native transport lost exact invocation authority"
  );
  assert(nativeTransportPulls === 2, "native transport did not hold one dormant pull");
  assert(typeof stopNativeTransport === "function", "native transport did not retain its pull");

  const saveNode = transported.n.find((node) => node.r === "button" && node.n === "Save");
  assert(saveNode && saveNode.o === 9, "action target missing");
  const saveDescriptor = Object.freeze({
    r: 7, o: 9, q: 1, s: 0, n: "Save", vk: 0, vt: null, vo: 0, vb: false
  });
  assert(
    JSON.stringify(saveDescriptor) ===
      '{"r":7,"o":9,"q":1,"s":0,"n":"Save","vk":0,"vt":null,"vo":0,"vb":false}',
    "cross-language action descriptor golden drifted"
  );
  const actionRequest = (attempt) => JSON.stringify({
    v: 1,
    o: "action_execute",
    a: attempt,
    i: 101,
    g: 101,
    t: saveNode.k,
    r: "button",
    k: "click",
    e: { x: 10, y: 10, w: 160, h: 32 },
    p: 0,
    z: null,
    f: saveDescriptor,
    of: null
  });

  const actionSuccess = JSON.parse(runtime.invoke(actionRequest(1)));
  assert(
    actionSuccess.a === 1 && actionSuccess.i === 101 && actionSuccess.g === 101 &&
      actionSuccess.r === "visible" && actionSuccess.px === 90 && actionSuccess.py === 26 &&
      actionSuccess.b === "fixed_semantic_recipe" && button._fixedClickCount === 1,
    "fixed action execution evidence mismatch"
  );

  const unrelated = new Element("p");
  unrelated.append(new CharacterData("Unrelated live region mutation"));
  main.append(unrelated);
  setOwner(unrelated, document);
  const unrelatedMutation = JSON.parse(runtime.invoke(actionRequest(2)));
  assert(
    unrelatedMutation.a === 2 && unrelatedMutation.r === "visible" &&
      unrelatedMutation.b === "fixed_semantic_recipe" && button._fixedClickCount === 2,
    "unrelated mutation blocked fixed action"
  );

  button.attributes["aria-label"] = "Delete";
  assert(
    runtime.invoke(actionRequest(3)) === "E2:target_changed",
    "same-node semantic repurposing was accepted"
  );
  button.attributes["aria-label"] = "Save";

  document._hit = paragraph;
  assert(
    runtime.invoke(actionRequest(4)) === "E2:target_occluded",
    "occluded action target was accepted"
  );
  document._hit = button;

  button.rect = { x: 240, y: 10, width: 160, height: 32 };
  assert(
    runtime.invoke(actionRequest(5)) === "E2:target_changed",
    "incompatible target geometry was accepted"
  );
  button.rect = { x: 10, y: 10, width: 160, height: 32 };

  const escapedDescriptor = JSON.parse(actionRequest(6));
  escapedDescriptor.f = { ...saveDescriptor, n: 'Save"\\\u2028Ω' };
  assert(
    runtime.invoke(JSON.stringify(escapedDescriptor)) === "E2:target_changed",
    "escaped private descriptor bypassed exact comparison"
  );
  const savedButtonChildren = button._children;
  button._children = new NodeList();
  for (let index = 0; index < 2050; index += 1) button.append(new Element("span"));
  assert(
    runtime.invoke(actionRequest(7)) === "E2:target_changed",
    "oversized action target subtree escaped descriptor budget"
  );
  button._children = savedButtonChildren;
  assert(runtime.invoke(`{"padding":"${"x".repeat(17800)}"}`) === "E1:invalid_request", "oversize action request accepted");

  const roleCodes = Object.freeze({textbox: 8, searchbox: 10, combobox: 13, option: 15});
  const descriptorFor = (node) => ({
    r: roleCodes[node.r],
    o: node.o || 0,
    q: node.q === "sensitive" ? 2 : node.q === "secret" ? 3 : 1,
    s: node.s || 0,
    n: node.n === undefined ? null : node.n,
    vk: node.v && node.v.k === "text" ? 1 :
      node.v && node.v.k === "redacted" ? 2 : node.v && node.v.k === "ordinal" ? 4 : 0,
    vt: node.v && node.v.k === "text" ? node.v.value : null,
    vo: node.v && node.v.k === "ordinal" ? node.v.value : 0,
    vb: false
  });
  const actionNode = (name) => transported.n.find((node) => node.n === name);
  const fillRequest = (targetNode, value, attempt) => JSON.stringify({
    v: 1,
    o: "action_execute",
    a: attempt,
    i: 101,
    g: 101,
    t: targetNode.k,
    r: targetNode.r,
    k: "fill",
    e: { x: 10, y: 10, w: 160, h: 32 },
    p: 0,
    z: value,
    f: descriptorFor(targetNode),
    of: null
  });
  const selectRequest = (attempt) => JSON.stringify({
    v: 1,
    o: "action_execute",
    a: attempt,
    i: 101,
    g: 101,
    t: languageNode.k,
    r: languageNode.r,
    k: "select",
    e: { x: 10, y: 10, w: 160, h: 32 },
    p: germanNode.k,
    z: null,
    f: descriptorFor(languageNode),
    of: descriptorFor(germanNode)
  });
  document._hit = languageSelect;
  const selectSuccess = JSON.parse(runtime.invoke(selectRequest(29)));
  assert(
    selectSuccess.a === 29 && selectSuccess.d === 1 &&
      selectSuccess.b === "fixed_semantic_recipe" && languageSelect._selectedIndex === 1 &&
      germanOption.selected,
    "fixed native-select execution evidence mismatch"
  );
  languageSelect._selectedIndex = 0;
  languageSelect._rewriteAfterSet = true;
  assert(
    runtime.invoke(selectRequest(30)) === "E2:applied_unverified" &&
      languageSelect._selectedIndex === 0,
    "page-rewritten native selection remained retryable"
  );
  languageSelect._rewriteAfterSet = false;
  document._hit = button;
  const assertInputEvent = (target, expected) => {
    const events = fillEvents.get(target);
    assert(
      events.length === 2 && events[0].type === "beforeinput" && events[1].type === "input",
      "fill emitted an unexpected event sequence"
    );
    assert(
      events[0].isTrusted === false && events[0].cancelable === true &&
        events[1].isTrusted === false && events[1].bubbles === true &&
        events[1].composed === true && events[1].cancelable === false &&
        events[1].data === expected && events[1].inputType === "insertReplacementText" &&
        events[1].isComposing === false,
      "fill input event contract drifted"
    );
  };

  const textNode = actionNode("Account name");
  const searchNode = actionNode("Account search");
  const textareaNode = actionNode("Account notes");
  const editableNode = actionNode("Rich account notes");
  assert(
    textNode && searchNode && textareaNode && editableNode,
    `fill targets missing: ${transported.n.map((node) => `${node.r}:${node.n || ""}`).join("|")}`
  );

  document._hit = textInput;
  const textFill = JSON.parse(await runtime.invoke(fillRequest(textNode, "Zephium fixed text", 8)));
  assert(
    textFill.a === 8 && textFill.b === "page_world_compatibility_fill" && textFill.r === "form" &&
      textInput._value === "Zephium fixed text",
    "captured native text-input fill failed"
  );
  assertInputEvent(textInput, "Zephium fixed text");

  document._hit = searchInput;
  const searchFill = JSON.parse(await runtime.invoke(fillRequest(searchNode, "", 9)));
  assert(
    searchFill.a === 9 && searchFill.b === "page_world_compatibility_fill" &&
      searchFill.r === "form" && searchInput._value === "",
    "empty search-input fill failed"
  );
  assertInputEvent(searchInput, "");

  document._hit = textarea;
  const textareaValue = "  Zephium  fixed textarea\nline two  ";
  const textareaFill = JSON.parse(await runtime.invoke(fillRequest(textareaNode, textareaValue, 10)));
  assert(
    textareaFill.a === 10 && textareaFill.b === "page_world_compatibility_fill" &&
      textareaFill.r === "form" && textarea._value === textareaValue,
    "captured native textarea fill failed"
  );
  assertInputEvent(textarea, textareaValue);

  document._hit = contentEditable;
  assert(
    runtime.invoke(fillRequest(editableNode, "unsupported rich edit", 11)) ===
      "E2:unsupported_interaction",
    "contenteditable entered the fixed fill route"
  );

  textInput._value = "fixture text";
  fillEvents.get(textInput).length = 0;
  document._hit = textInput;
  textInput.attributes.type = "password";
  assert(
    runtime.invoke(fillRequest(textNode, "credential refusal", 12)) === "E2:credential_boundary",
    "credential transition entered the fixed fill route"
  );
  assert(textInput._value === "fixture text" && fillEvents.get(textInput).length === 0, "credential refusal mutated target");
  textInput.attributes.type = "text";

  textInput.attributes.readonly = "";
  assert(
    runtime.invoke(fillRequest(textNode, "readonly refusal", 13)) === "E2:target_disabled",
    "readonly target entered the fixed fill route"
  );
  assert(textInput._value === "fixture text" && fillEvents.get(textInput).length === 0, "readonly refusal mutated target");
  delete textInput.attributes.readonly;

  textInput._value = "descriptor drift";
  assert(
    runtime.invoke(fillRequest(textNode, "drift refusal", 14)) === "E2:target_changed",
    "pre-fill value drift escaped exact descriptor revalidation"
  );
  textInput._value = "fixture text";

  textInput._nativeDispatchThrows = true;
  assert(
    await runtime.invoke(fillRequest(textNode, "dispatch refusal", 15)) === "E2:applied_unverified",
    "possibly page-observed dispatch exception remained retryable"
  );
  assert(textInput._value === "fixture text", "failed event dispatch did not restore native value");
  textInput._nativeDispatchThrows = false;

  textInput._rewriteAfterInput = true;
  assert(
    await runtime.invoke(fillRequest(textNode, "synchronous rewrite", 16)) === "E2:applied_unverified",
    "synchronous page rewrite escaped the native getter check"
  );
  assert(textInput._value === "page rewrite", "rewrite fixture did not execute");
  textInput._rewriteAfterInput = false;
  textInput._value = "fixture text";

  textInput._repurposeAfterInput = true;
  assert(
    await runtime.invoke(fillRequest(textNode, "semantic repurpose", 17)) === "E2:applied_unverified",
    "post-event credential repurposing was accepted"
  );
  textInput._repurposeAfterInput = false;
  textInput.attributes["aria-label"] = "Account name";
  textInput._value = "fixture text";

  assert(
    runtime.invoke(fillRequest(textNode, "x".repeat(4097), 18)) === "E1:invalid_request",
    "unverifiable fill value exceeded the semantic projection ceiling"
  );
  assert(textInput._value === "fixture text", "oversized fill mutated target");
  assert(
    runtime.invoke(fillRequest(textNode, "line one\nline two", 19)) === "E2:unsupported_interaction",
    "single-line input accepted a line feed"
  );
  assert(textInput._value === "fixture text", "line-feed refusal mutated input");
  const invalidFill = JSON.parse(fillRequest(textNode, "valid", 20));
  invalidFill.z = "forbidden\u0000control";
  assert(runtime.invoke(JSON.stringify(invalidFill)) === "E1:invalid_request", "forbidden fill text was accepted");
  const invalidClickText = JSON.parse(actionRequest(21));
  invalidClickText.z = "unexpected";
  assert(runtime.invoke(JSON.stringify(invalidClickText)) === "E1:invalid_request", "click accepted fill text");

  textInput._value = "fixture text";
  fillEvents.get(textInput).length = 0;
  document._hit = textInput;
  delete html.attributes[relayReady];
  assert(
    runtime.invoke(fillRequest(textNode, "relay absent", 22)) === "E2:page_relay_not_ready",
    "fill proceeded without a READY page-world relay"
  );
  assert(textInput._value === "fixture text", "relay-absent refusal mutated target");
  html.attributes[relayReady] = "1";
  relayMode = "duplicate";
  assert(
    await runtime.invoke(fillRequest(textNode, "duplicate marker", 23)) === "E2:target_occluded",
    "duplicate relay marker was accepted"
  );
  assert(textInput._value === "fixture text", "duplicate relay refusal mutated target");
  assert(
    !textInput.hasAttribute(relayCommand) && !textInput.hasAttribute(relayTerminal),
    "duplicate relay refusal retained transport markers"
  );
  relayMode = "normal";

  textInput.setAttribute(relayTerminal, "1|24|ok");
  fillEvents.get(textInput).length = 0;
  const forgedTerminalFill = JSON.parse(
    await runtime.invoke(fillRequest(textNode, "forged terminal replaced", 24))
  );
  assert(
    forgedTerminalFill.a === 24 && textInput._value === "forged terminal replaced",
    "preexisting forged terminal substituted correlated relay evidence"
  );
  assertInputEvent(textInput, "forged terminal replaced");
  assert(
    !textInput.hasAttribute(relayCommand) && !textInput.hasAttribute(relayTerminal),
    "successful relay retained transport markers"
  );
  textInput._value = "fixture text";

  fillEvents.get(textInput).length = 0;
  textInput._cancelBeforeInput = true;
  assert(
    await runtime.invoke(fillRequest(textNode, "cancelled edit", 25)) === "E2:applied_unverified",
    "page-observed beforeinput cancellation remained retryable"
  );
  assert(
    textInput._value === "fixture text" && fillEvents.get(textInput).length === 1 &&
      fillEvents.get(textInput)[0].type === "beforeinput",
    "beforeinput cancellation evidence drifted"
  );
  textInput._cancelBeforeInput = false;
  assert(
    !textInput.hasAttribute(relayCommand) && !textInput.hasAttribute(relayTerminal),
    "cancelled relay retained transport markers"
  );

  fillEvents.get(textInput).length = 0;
  textInput._repurposeAfterBeforeInput = true;
  assert(
    await runtime.invoke(fillRequest(textNode, "repurposed edit", 26)) === "E2:applied_unverified",
    "post-beforeinput target repurposing remained retryable"
  );
  assert(
    textInput._value === "fixture text" && textInput.attributes.type === "password" &&
      fillEvents.get(textInput).length === 1,
    "post-beforeinput repurposing evidence drifted"
  );
  textInput._repurposeAfterBeforeInput = false;
  textInput.attributes.type = "text";
  assert(
    !textInput.hasAttribute(relayCommand) && !textInput.hasAttribute(relayTerminal),
    "repurposed relay retained transport markers"
  );

  fillEvents.get(textInput).length = 0;
  relayMode = "flood";
  assert(
    await runtime.invoke(fillRequest(textNode, "flood refusal", 27)) === "E2:applied_unverified",
    "missing terminal after bounded record refusal remained retryable"
  );
  assert(
    textInput._value === "fixture text" && fillEvents.get(textInput).length === 0,
    "bounded record refusal mutated target"
  );
  assert(
    !textInput.hasAttribute(relayCommand) && !textInput.hasAttribute(relayTerminal),
    "bounded record refusal retained transport markers"
  );
  relayMode = "normal";

  const carriageReturnFill = JSON.parse(fillRequest(textNode, "valid", 28));
  carriageReturnFill.z = "carriage\rreturn";
  assert(
    runtime.invoke(JSON.stringify(carriageReturnFill)) === "E1:invalid_request",
    "carriage-return fill text crossed the runtime boundary"
  );

  main.append(ariaOverflowInput);
  setOwner(main, document);
  const metadataOverflowWire = invoke(102, 102, { k: "initial" });
  const metadataOverflowSnapshot = JSON.parse(metadataOverflowWire);
  assert(
    metadataOverflowSnapshot.c === "field_limit" &&
      metadataOverflowSnapshot.n.filter((node) =>
        node.r === "textbox" && node.v && node.v.k === "redacted" && node.q === "secret"
      ).length >= 9 &&
      !metadataOverflowWire.includes(metadataOverflowValues[1]),
    "513-byte accessible-name overflow did not fail closed"
  );

  stopNativeTransport("S1");
  await new Promise((resolve) => setImmediate(resolve));

  // The production commerce shape: named semantic children inside an unnamed
  // actionable link. Preserve both children and the composed link name without
  // another DOM walk or exposing hidden/credential content through ancestors.
  const catalog = new Element("main");
  const productLink = catalog.append(new HTMLAnchorElement({ href: "https://example.test/product" }));
  productLink.append(new Element("h3")).append(new CharacterData("Fixture Cup"));
  productLink.append(new Element("p")).append(new CharacterData("$15.00 USD"));
  productLink.append(new Element("span", { hidden: "" })).append(new CharacterData("hidden-catalog-secret"));
  const explicitLink = catalog.append(new Element("a", { href: "https://example.test/exact", "aria-label": "Explicit product" }));
  explicitLink.append(new Element("h3")).append(new CharacterData("Unselected child label"));
  const privateLink = catalog.append(new Element("a", { href: "https://example.test/private" }));
  privateLink.append(new Element("div", { contenteditable: "true", "aria-label": "Private key" }))
    .append(new Element("p")).append(new CharacterData("nested-credential-never-name"));
  const secretLink = catalog.append(new Element("a", { href: "https://example.test/secret" }));
  secretLink.append(new Element("h3")).append(new CharacterData("sk-name-secret-fixture-value"));
  document._root = catalog;
  setOwner(catalog, document);
  const anchorGetter = Object.getOwnPropertyDescriptor(HTMLAnchorElement.prototype, "href");
  Object.defineProperty(productLink, "href", { get() { throw new Error("page-owned href getter"); } });
  Object.defineProperty(HTMLAnchorElement.prototype, "href", { configurable: true, get() { throw new Error("replaced prototype getter"); } });
  const catalogWire = invoke(103, 103, { k: "initial" });
  Object.defineProperty(HTMLAnchorElement.prototype, "href", anchorGetter);
  const catalogSnapshot = JSON.parse(catalogWire);
  const catalogLink = catalogSnapshot.n.find((node) => node.r === "link" && node.n === "Fixture Cup $15.00 USD");
  assert(catalogLink && (catalogLink.o & 1) === 1, "nested product text lost actionable link identity");
  assert(catalogLink.u === "https://example.test/product", "link destination did not use the captured exact native getter");
  assert(catalogSnapshot.n.some((node) => node.r === "heading" && node.n === "Fixture Cup"), "nested heading was discarded");
  assert(catalogSnapshot.n.some((node) => node.r === "paragraph" && node.t === "$15.00 USD"), "nested price was discarded");
  assert(catalogSnapshot.n.some((node) => node.r === "link" && node.n === "Explicit product"), "explicit accessible name was overwritten");
  assert(!catalogWire.includes("hidden-catalog-secret"), "hidden descendant entered parent name");
  assert(!catalogSnapshot.n.some((node) => node.r === "link" && node.n && node.n.includes("nested-credential")), "credential descendant entered parent name");
  assert(!catalogWire.includes("sk-name-secret-fixture-value"), "nested secret entered ancestor name");
  const catalogLimited = JSON.parse(invoke(104, 104, { k: "initial" }, { t: 16 }));
  assert(catalogLimited.c === "text_limit", "ancestor copies escaped aggregate text accounting");
  const catalogInspected = JSON.parse(invoke(105, 105, { k: "initial" }, { n: 4, x: 4 }));
  assert(catalogInspected.c === "inspection_limit", "nested names escaped the DOM inspection ceiling");

  // A source-backed prose unit must not lose the labels of its inline links.
  // Keep the child refs, but also preserve their visible words in DOM order in
  // the nearest prose record; do not flatten unrelated nested block prose.
  const prose = new Element("main");
  const observable = prose.append(new Element("li"));
  observable.append(new CharacterData("Observable :"));
  const features = ["recording", "streaming", "debugging", "profiling", "diffing"];
  for (let index = 0; index < features.length; index += 1) {
    if (index !== 0) observable.append(new CharacterData(index === 4 ? ", and" : ","));
    const attributes = { href: `https://example.test/${features[index]}` };
    if (index === 3) attributes["aria-label"] = "Profile tooling";
    observable.append(new HTMLAnchorElement(attributes))
      .append(new CharacterData(features[index]));
  }
  observable.append(new CharacterData("tools are built in."));
  observable.append(new Element("span", { hidden: "" })).append(new CharacterData("hidden-prose-label"));
  observable.append(new Element("div", { contenteditable: "true" }))
    .append(new Element("p")).append(new CharacterData("editable-prose-label"));
  observable.append(new HTMLAnchorElement({ href: "https://example.test/secret" }))
    .append(new CharacterData("sk-prose-secret-fixture-value"));
  observable.append(new HTMLAnchorElement({ href: "https://example.test/private-label", "aria-label": "sk-private-label-fixture-value" }))
    .append(new CharacterData("private-link-body"));
  const outer = prose.append(new Element("li"));
  outer.append(new CharacterData("Outer only"));
  const inner = outer.append(new Element("p"));
  inner.append(new CharacterData("Separate paragraph"));
  inner.append(new HTMLAnchorElement({ href: "https://example.test/detail" }))
    .append(new CharacterData("detail"));
  document._root = prose;
  setOwner(prose, document);
  const proseWire = invoke(106, 106, { k: "initial" });
  const proseSnapshot = JSON.parse(proseWire);
  const observableNode = proseSnapshot.n.find((node) => node.r === "list_item");
  assert(observableNode.t === "Observable : recording , streaming , debugging , profiling , and diffing tools are built in.", "inline link labels were lost from their prose source");
  for (const feature of features) {
    const link = proseSnapshot.n.find((node) => node.u === `https://example.test/${feature}`);
    assert(link && link.r === "link" && link.p === proseSnapshot.n.indexOf(observableNode), "prose composition lost exact child link ancestry");
    assert(link.n === (feature === "profiling" ? "Profile tooling" : feature), "prose composition changed a child accessible name");
  }
  assert(proseSnapshot.n.some((node) => node.r === "list_item" && node.t === "Outer only"), "nested block prose was flattened into its ancestor");
  assert(proseSnapshot.n.some((node) => node.r === "paragraph" && node.t === "Separate paragraph detail"), "nearest paragraph lost its inline label");
  assert(!proseWire.includes("hidden-prose-label") && !proseWire.includes("sk-prose-secret-fixture-value"), "hidden or secret inline text escaped");
  assert(!observableNode.t.includes("editable-prose-label"), "editable text entered an ancestor prose source");
  assert(!observableNode.t.includes("private-link-body"), "a secret-classified explicit link entered an ancestor prose source");
  assert(JSON.parse(invoke(107, 107, { k: "initial" }, { t: 16 })).c === "text_limit", "prose copies escaped aggregate text accounting");
  assert(JSON.parse(invoke(108, 108, { k: "initial" }, { n: 4, x: 4 })).c === "inspection_limit", "prose composition escaped the DOM inspection ceiling");
  const largeProse = new Element("p");
  for (let index = 0; index < 11; index += 1) {
    largeProse.append(new HTMLAnchorElement({ href: `https://example.test/bounded/${index}` }))
      .append(new CharacterData("x".repeat(400)));
  }
  document._root = largeProse;
  setOwner(largeProse, document);
  const largeProseSnapshot = JSON.parse(invoke(109, 109, { k: "initial" }));
  assert(largeProseSnapshot.c === "field_limit" && largeProseSnapshot.n.some((node) => node.r === "paragraph" && Buffer.byteLength(node.t) <= 4096), "inline copies escaped the original per-prose-field ceiling");

  // Generic below-viewport evidence: TOC links are not section bodies. The
  // initial inventory may expose actual rendered headings at spare capacity;
  // exact surrounding-text capture reaches their following sibling prose.
  const article = new Element("main");
  article.append(new HTMLAnchorElement({ href: "https://example.test/article#details" }))
    .append(new CharacterData("Details in this page"));
  const sectionHeading = article.append(new Element("h2"));
  sectionHeading.rect.y = 1800;
  sectionHeading.append(new CharacterData("Detailed behavior"));
  const editableHeading = sectionHeading.append(new Element("span", { contenteditable: "true", "aria-label": "Private key" }));
  editableHeading.rect.y = 1800;
  editableHeading.append(new CharacterData("editable-heading-secret"));
  const sectionBody = article.append(new Element("p"));
  sectionBody.rect.y = 1840;
  sectionBody.append(new CharacterData("New scoped evidence from a following sibling."));
  article.append(new Element("p", { hidden: "" })).append(new CharacterData("hidden-section-body"));
  document._root = article;
  setOwner(article, document);
  const inventoryWire = invoke(110, 110, { k: "initial" });
  const inventory = JSON.parse(inventoryWire);
  const sectionRef = inventory.n.find((node) => node.r === "heading" && node.n === "Detailed behavior");
  assert(sectionRef && !inventoryWire.includes("New scoped evidence") && !inventoryWire.includes("editable-heading-secret"), "offscreen anchor leaked its body/credential descendants or was undiscoverable");
  const sectionWire = invoke(111, 111, { k: "surrounding_text", a: sectionRef.k, p: 0, n: 1024 });
  assert(sectionWire.includes("New scoped evidence from a following sibling.") && !sectionWire.includes("hidden-section-body"), "exact heading expansion omitted the body or included hidden text");
  const subtreeWire = invoke(112, 112, { k: "subtree", a: sectionRef.k });
  assert(!subtreeWire.includes("New scoped evidence"), "a heading subtree silently widened into sibling content");
  const crowd = new Element("main");
  for (let index = 0; index < 200; index += 1) {
    const h = crowd.append(new Element("h2")); h.rect.y = 2000 + index * 40;
    h.append(new CharacterData(`Offscreen heading ${index}`));
  }
  crowd.append(new Element("button")).append(new CharacterData("Visible final action"));
  crowd.append(new Element("p")).append(new CharacterData("Visible final evidence"));
  document._root = crowd; setOwner(crowd, document);
  const crowded = JSON.parse(invoke(113, 113, { k: "initial" }, { n: 8, t: 1024 }));
  assert(crowded.n.some((node) => node.n === "Visible final action") && crowded.n.some((node) => node.t === "Visible final evidence"), "offscreen heading inventory starved later viewport content");
  assert(crowded.n.length <= 8 && crowded.c === "node_limit", "structural inventory escaped its bound or hid omission");
  const tight = JSON.parse(invoke(114, 114, { k: "initial" }, { t: 64 }));
  assert(tight.n.some((node) => node.n === "Visible final action"), "offscreen headings spent visible content's text budget");
  const inventoryExtraBytes = Buffer.byteLength(inventoryWire) - Buffer.byteLength(JSON.stringify({ ...inventory, n: inventory.n.filter((node) => node !== sectionRef) }));

  // A broad parent region with a long earlier nested navigation must expose
  // its own prose and nested anchors, not exhaust its budget in that sidebar.
  const page = new Element("main", { "aria-label": "Workspace" });
  const navigation = page.append(new Element("nav", { "aria-label": "Reference index" }));
  const items = navigation.append(new Element("ul"));
  for (let index = 0; index < 180; index += 1) {
    const item = items.append(new Element("li"));
    item.rect.y = 2000 + index * 30;
    const link = item.append(new Element("a", { href: `/guide/item-${index}` }));
    link.rect.y = item.rect.y;
    link.append(new CharacterData(`Index item ${index}`));
  }
  page.append(new Element("h2")).append(new CharacterData("Actual topic"));
  page.append(new Element("p")).append(new CharacterData("Useful parent-region evidence beyond the index."));
  const child = page.append(new Element("article", { "aria-label": "Independent article" }));
  child.append(new Element("p")).append(new CharacterData("Nested article evidence."));
  document._root = page; setOwner(page, document);
  const beforeRegion = JSON.parse(invoke(115, 115, { k: "initial" }));
  const parentKey = beforeRegion.n.find(node => node.n === "Workspace").k;
  const regionWire = invoke(116, 116, { k: "region", a: parentKey }, { n: 128 });
  const region = JSON.parse(regionWire);
  assert(regionWire.includes("Useful parent-region evidence") && !regionWire.includes("Index item") && !regionWire.includes("Nested article evidence"), "nested regions starved or silently widened parent-region capture");
  assert(region.c === "scope_boundary" && region.n.some(node => node.n === "Reference index") && region.n.some(node => node.n === "Independent article"), "region omission lost truthful boundaries or expandable anchors");
  const nestedKey = region.n.find(node => node.n === "Independent article").k;
  assert(invoke(117, 117, { k: "region", a: nestedKey }).includes("Nested article evidence"), "nested region anchor cannot be independently inspected");
  const restored = JSON.parse(invoke(118, 118, { k: "initial" }));
  const restoredKey = restored.n.find(node => node.n === "Workspace").k;
  const recursive = JSON.parse(invoke(119, 119, { k: "subtree", a: restoredKey }, { n: 128 }));
  assert(recursive.c === "node_limit" && !JSON.stringify(recursive).includes("Useful parent-region evidence"), "subtree no longer has explicit recursive semantics");
  const limitedRegion = JSON.parse(invoke(120, 120, { k: "region", a: restoredKey }, { n: 3 }));
  assert(limitedRegion.c === "node_limit", "intentional region boundaries hid actual node-budget exhaustion");

  // A field-local ceiling must not consume the traversal's remaining capacity.
  const product = new Element("main");
  const productHeading = product.append(new Element("h1"));
  productHeading.append(new CharacterData("Display model"));
  const longDescription = product.append(new Element("p"));
  longDescription.append(new CharacterData("Building detail ".repeat(300)));
  const dimensionsHeading = product.append(new Element("h2"));
  dimensionsHeading.append(new CharacterData("Dimensions"));
  const dimensions = product.append(new Element("p"));
  dimensions.append(new CharacterData("Width 28 cm; depth 18 cm; height 32 cm."));
  product.append(new Element("p")).append(new CharacterData("Price USD 129.99. In stock."));
  const oversizedButton = product.append(new Element("button"));
  oversizedButton.append(new CharacterData("b".repeat(700)));
  product.append(new Element("p")).append(new CharacterData("Later evidence remains visible."));
  const privateWindow = product.append(new Element("div", { contenteditable: "true" }));
  privateWindow.append(new Element("p")).append(new CharacterData("private-window-value"));
  product.append(new Element("p", { hidden: "" })).append(new CharacterData("hidden-window-value"));
  product.append(new Element("p")).append(new CharacterData("sk-window-private-token-value"));
  document._root = product; setOwner(product, document);
  const productInitial = JSON.parse(invoke(121, 121, { k: "initial" }));
  assert(productInitial.c === "field_limit", "local field saturation was reported as aggregate exhaustion");
  assert(productInitial.n.some(node => node.t === "Later evidence remains visible.") &&
    productInitial.n.some(node => node.t === "Width 28 cm; depth 18 cm; height 32 cm."),
  "a long earlier field starved independent later evidence");
  const productKey = productInitial.n.find(node => node.n === "Display model").k;
  const surroundingWire = invoke(122, 122, { k: "surrounding_text", a: productKey, p: 0, n: 8192 });
  const surrounding = JSON.parse(surroundingWire);
  const dimensionSource = surrounding.n.find(node => node.t === "Width 28 cm; depth 18 cm; height 32 cm.");
  assert(surrounding.c === "field_limit" && surrounding.n.length > 2 && dimensionSource,
    "surrounding evidence collapsed into its anchor or lost evidence after a 4-KiB source");
  assert(surrounding.n[0].k === productKey && surrounding.n[0].t === undefined &&
    surrounding.n.every(node => node.p === undefined && !node.o && !node.u),
  "a read window invented ancestry, anchor text, actions, or navigation authority");
  assert(!surroundingWire.includes("private-window-value") && !surroundingWire.includes("hidden-window-value") &&
    !surroundingWire.includes("sk-window-private-token-value"), "window projection exposed private or hidden descendants");
  const dimensionsSubtree = invoke(123, 123, { k: "subtree", a: dimensionSource.k });
  assert(dimensionsSubtree.includes("Width 28 cm; depth 18 cm; height 32 cm.") &&
    !dimensionsSubtree.includes("Price USD"), "fresh window source cannot be inspected without widening its subtree");

  const productAgain = JSON.parse(invoke(124, 124, { k: "initial" }));
  const productAgainKey = productAgain.n.find(node => node.n === "Display model").k;
  const shortWindow = JSON.parse(invoke(125, 125, { k: "surrounding_text", a: productAgainKey, p: 0, n: 1024 }));
  const paragraphAnchor = shortWindow.n.find(node => node.r === "paragraph" && node.t);
  assert(shortWindow.c === "scope_boundary" && paragraphAnchor && !JSON.stringify(shortWindow).includes("Width 28"),
    "short window hid its boundary or escaped its requested text ceiling");
  const followingWindow = invoke(126, 126, { k: "surrounding_text", a: paragraphAnchor.k, p: 0, n: 1024 });
  assert(followingWindow.includes("Width 28 cm"), "a bounded window retained no fresh anchor for following evidence");
  dimensions._owner = null;
  assert(invoke(127, 127, { k: "subtree", a: dimensionSource.k }) === "E1:anchor_missing",
    "a detached surrounding source remained inspectable");
  setOwner(dimensions, document);

  const textExhausted = JSON.parse(invoke(128, 128, { k: "initial" }, { t: 64 }));
  assert(textExhausted.c === "text_limit" && !JSON.stringify(textExhausted).includes("Later evidence"),
    "aggregate text exhaustion no longer stopped bounded traversal");
  const windowLimited = JSON.parse(invoke(129, 129, { k: "surrounding_text", a: productAgainKey, p: 0, n: 8192 }, { n: 2 }));
  assert(windowLimited.n.length <= 2 && windowLimited.c === "node_limit", "window sources escaped the node bound");
  const inspectionWindow = JSON.parse(invoke(130, 130, { k: "surrounding_text", a: productAgainKey, p: 0, n: 8192 }, { n: 2, x: 2 }));
  assert(inspectionWindow.c === "inspection_limit", "window collection hid its inspection ceiling");
  const windowWireLimited = invoke(131, 131, { k: "surrounding_text", a: productAgainKey, p: 0, n: 8192 }, { w: 1024 });
  assert(Buffer.byteLength(windowWireLimited) <= 1024 && JSON.parse(windowWireLimited).c === "wire_limit",
    "structured window escaped the original wire ceiling");
  const clippedActionSnapshot = JSON.parse(invoke(132, 132, { k: "initial" }));
  const clippedButton = clippedActionSnapshot.n.find(node => node.r === "button");
  document._hit = oversizedButton;
  const clippedAction = JSON.parse(actionRequest(200));
  clippedAction.i = 132; clippedAction.g = 132; clippedAction.t = clippedButton.k;
  clippedAction.f.n = clippedButton.n;
  const clippedActionResult = runtime.invoke(JSON.stringify(clippedAction));
  assert(clippedActionResult === "E2:target_changed" && !oversizedButton._fixedClickCount,
    `a field-clipped descriptor became actionable during live revalidation: ${clippedActionResult}`);
  const unicodePage = new Element("main");
  unicodePage.append(new Element("p")).append(new CharacterData("Earlier words then nearest"));
  const unicodeHeading = unicodePage.append(new Element("h2"));
  unicodeHeading.append(new CharacterData("Unicode sources"));
  unicodePage.append(new Element("p")).append(new CharacterData("é".repeat(3000)));
  unicodePage.append(new Element("p")).append(new CharacterData("Following UTF-8 evidence"));
  document._root = unicodePage; setOwner(unicodePage, document);
  const unicodeInitial = JSON.parse(invoke(133, 133, { k: "initial" }));
  const unicodeKey = unicodeInitial.n.find(node => node.n === "Unicode sources").k;
  const unicodeWindow = JSON.parse(invoke(134, 134, { k: "surrounding_text", a: unicodeKey, p: 7, n: 8185 }));
  assert(unicodeWindow.n.some(node => node.t === "nearest") &&
    unicodeWindow.n.some(node => node.t === "Following UTF-8 evidence") &&
    unicodeWindow.n.every(node => !node.t || Buffer.byteLength(node.t) <= 4096),
  "window suffix, UTF-8 field clipping, or later source discovery regressed");

  // Inline semantic words must stay in order in their actual prose source.
  // Otherwise dropping the link "not" can reverse an extracted claim.
  const fidelityPage = new Element("main");
  const fidelityHeading = fidelityPage.append(new Element("h2"));
  fidelityHeading.append(new CharacterData("Availability evidence"));
  const negativeProse = fidelityPage.append(new Element("p"));
  negativeProse.append(new CharacterData("This model is "));
  const inlineNegation = negativeProse.append(new HTMLAnchorElement({ href: "https://example.test/terms", "aria-label": "Terms" }));
  inlineNegation.append(new CharacterData("not"));
  negativeProse.append(new CharacterData(" available."));
  fidelityPage.append(new Element("p")).append(new CharacterData("Another model is available."));
  const inlineHeading = fidelityPage.append(new Element("h3"));
  inlineHeading.append(new CharacterData("Do "));
  inlineHeading.append(new HTMLAnchorElement({ href: "https://example.test/shipping" })).append(new CharacterData("not"));
  inlineHeading.append(new CharacterData(" ship."));
  const interruptedProse = fidelityPage.append(new Element("li"));
  interruptedProse.append(new CharacterData("Shipping is "));
  interruptedProse.append(new Element("p")).append(new CharacterData("not"));
  interruptedProse.append(new CharacterData(" available."));
  const fencedProse = fidelityPage.append(new Element("p"));
  fencedProse.append(new CharacterData("Private result is "));
  fencedProse.append(new Element("span", { contenteditable: "true" })).append(new CharacterData("private-negation"));
  fencedProse.append(new CharacterData(" approved."));
  document._root = fidelityPage; setOwner(fidelityPage, document);
  const fidelityInitial = JSON.parse(invoke(135, 135, { k: "initial" }));
  const fidelityKey = fidelityInitial.n.find(node => node.n === "Availability evidence").k;
  const negativeProseKey = fidelityInitial.n.find(node => node.t === "This model is not available.").k;
  const fidelityWindowWire = invoke(136, 136, { k: "surrounding_text", a: fidelityKey, p: 0, n: 2048 });
  const fidelityWindow = JSON.parse(fidelityWindowWire);
  assert(fidelityWindow.n.find(node => node.k === negativeProseKey)?.t === "This model is not available.",
    "inline negation was omitted from its surrounding source quote");
  assert(JSON.stringify(fidelityWindow.n.slice(1).map(node => node.t)) === JSON.stringify([
    "This model is not available.", "Another model is available.", "Do not ship.",
    "Shipping is", "not", "Private result is"
  ]), "surrounding source order or contiguous quote fidelity changed");
  assert(fidelityWindow.c === "scope_boundary" && !fidelityWindowWire.includes("Shipping is available.") &&
    !fidelityWindowWire.includes("Private result is approved.") && !fidelityWindowWire.includes("private-negation") &&
    fidelityWindow.n.every(node => !node.o && !node.u && node.p === undefined),
  "discontiguous/private prose became a false quote or gained authority");
  const fidelityAgain = JSON.parse(invoke(137, 137, { k: "initial" }));
  const omittedNegationKey = fidelityAgain.n.find(node => node.n === "Terms").k;
  const aroundInline = JSON.parse(invoke(138, 138, { k: "surrounding_text", a: omittedNegationKey, p: 14, n: 11 }));
  assert(aroundInline.c === "scope_boundary" && aroundInline.n.find(node => node.k === negativeProseKey)?.t === "This model is" &&
    !JSON.stringify(aroundInline).includes("This model is available."),
  "before/after context joined across the omitted anchor into a false quote");

  // DOM fragmentation is not semantic-source cardinality. Both sides retain
  // all 160 inline fragments and the following independent evidence in four
  // roots, including when the output budget is much smaller than 160.
  const fragmentedPage = new Element("main");
  const fragmentedBefore = fragmentedPage.append(new Element("p"));
  for (let index = 0; index < 160; index += 1) {
    fragmentedBefore.append(new Element("span")).append(new CharacterData(`b${index}`));
  }
  const fragmentedHeading = fragmentedPage.append(new Element("h2"));
  fragmentedHeading.append(new CharacterData("Fragmented evidence"));
  const fragmentedAfter = fragmentedPage.append(new Element("p"));
  for (let index = 0; index < 160; index += 1) {
    fragmentedAfter.append(new Element("span")).append(new CharacterData(`a${index}`));
  }
  fragmentedPage.append(new Element("p")).append(new CharacterData("Following independent evidence"));
  document._root = fragmentedPage; setOwner(fragmentedPage, document);
  const fragmentedInitial = JSON.parse(invoke(139, 139, { k: "initial" }));
  const fragmentedKey = fragmentedInitial.n.find(node => node.n === "Fragmented evidence").k;
  const expectedBefore = Array.from({ length: 160 }, (_, index) => `b${index}`).join(" ");
  const expectedAfter = Array.from({ length: 160 }, (_, index) => `a${index}`).join(" ");
  const fragmentedWindow = JSON.parse(invoke(140, 140, { k: "surrounding_text", a: fragmentedKey, p: 2048, n: 2048 }, { n: 4 }));
  assert(fragmentedWindow.c === "complete" && fragmentedWindow.n.length === 4 &&
    JSON.stringify(fragmentedWindow.n.slice(1).map(node => node.t)) ===
      JSON.stringify([expectedBefore, expectedAfter, "Following independent evidence"]),
  "raw inline fragment count exhausted semantic slots or lost contiguous evidence");

  // Output pressure selects nearby sources from both directions. The initial
  // capture establishes the anchor before preceding page furniture is inserted.
  const balancedPage = new Element("main");
  const balancedHeading = balancedPage.append(new Element("h2"));
  balancedHeading.append(new CharacterData("Balanced evidence"));
  balancedPage.append(new Element("p")).append(new CharacterData("Immediate following evidence"));
  for (let index = 1; index < 140; index += 1) {
    balancedPage.append(new Element("p")).append(new CharacterData(`After ${index}`));
  }
  document._root = balancedPage; setOwner(balancedPage, document);
  const balancedInitial = JSON.parse(invoke(141, 141, { k: "initial" }));
  const balancedKey = balancedInitial.n.find(node => node.n === "Balanced evidence").k;
  for (let index = 0; index < 140; index += 1) {
    const preceding = new Element("p");
    preceding.append(new CharacterData(`Before ${index}`));
    preceding._parent = balancedPage;
    setOwner(preceding, document);
    balancedPage._children.values.splice(index, 0, preceding);
  }
  const balancedWindow = JSON.parse(invoke(142, 142, { k: "surrounding_text", a: balancedKey, p: 4096, n: 4096 }));
  assert(balancedWindow.c === "node_limit" && balancedWindow.n.length === 128 &&
    balancedWindow.n.some(node => node.t === "Before 139") &&
    balancedWindow.n.some(node => node.t === "Immediate following evidence") &&
    !balancedWindow.n.some(node => node.t === "Before 0"),
  "distant preceding sources starved immediately adjacent evidence");
  const tinyBalancedWindow = JSON.parse(invoke(143, 143, { k: "surrounding_text", a: balancedKey, p: 4096, n: 4096 }, { n: 3 }));
  assert(tinyBalancedWindow.c === "node_limit" && JSON.stringify(tinyBalancedWindow.n.slice(1).map(node => node.t)) ===
    JSON.stringify(["Before 139", "Immediate following evidence"]),
  "three-slot window did not retain one immediate source on each side");
  const repeatedBalancedWindow = JSON.parse(invoke(144, 144, { k: "surrounding_text", a: balancedKey, p: 4096, n: 4096 }, { n: 3 }));
  assert(JSON.stringify(tinyBalancedWindow.n) === JSON.stringify(repeatedBalancedWindow.n) &&
    repeatedBalancedWindow.n.every(node => !node.o && !node.u && node.p === undefined),
  "fair source selection changed across identical captures or widened authority");
  const rollingWindow = JSON.parse(invoke(145, 145, { k: "surrounding_text", a: balancedKey, p: 32, n: 0 }, { n: 4 }));
  assert(rollingWindow.c === "scope_boundary" && JSON.stringify(rollingWindow.n.slice(1).map(node => node.t)) ===
    JSON.stringify(["Before 137", "Before 138", "Before 139"]),
  "rolling source eviction lost the nearest byte-bounded suffix");

  // Query-directed discovery must reach visible sources after an initially
  // saturated inventory, while staying inside its exact region and refusing
  // hidden, editable, credential and frame descendants.
  body._children.values = [];
  const searchable = body.append(new Element("main"));
  for (let index = 0; index < 180; index += 1) {
    searchable.append(new Element("button")).append(new CharacterData(`Furniture ${index}`));
  }
  const dimensionParagraph = searchable.append(new Element("p"));
  dimensionParagraph.rect = { x: 0, y: 6000, width: 600, height: 30 };
  dimensionParagraph.append(new CharacterData("Dimensions: width "));
  dimensionParagraph.append(new Element("span")).append(new CharacterData("89 cm"));
  dimensionParagraph.append(new CharacterData("; depth 19 cm. Not suitable for a 30 cm shelf."));
  searchable.append(new Element("p"))
    .append(new CharacterData("Available now. Includes 3745 pieces."));
  searchable.append(new Element("p")).append(new CharacterData("$349.99 / €319.99; 15% discount."));
  searchable.append(new Element("p", { hidden: "" })).append(new CharacterData("$ Dimensions hidden-answer"));
  searchable.append(new Element("div", { contenteditable: "true" })).append(new CharacterData("$ Dimensions editable-answer"));
  searchable.append(new HTMLInputElement({ type: "password", value: "$ Dimensions secret-answer" }));
  searchable.append(new Element("iframe")).append(new CharacterData("$ Dimensions frame-answer"));
  body.append(new Element("p")).append(new CharacterData("$ Dimensions outside-answer"));
  document._root = body; setOwner(body, document);
  const searchInitial = JSON.parse(invoke(146, 146, { k: "initial" }));
  assert(!JSON.stringify(searchInitial).includes("89 cm"), "search fixture did not saturate initial inventory");
  const searchAnchor = searchInitial.n.find(node => node.r === "landmark").k;
  const searchedWire = invoke(147, 147, { k: "text_search", a: searchAnchor, q: "dimensions width depth available pieces" });
  const searched = JSON.parse(searchedWire);
  assert(searched.n.some(node => node.t === "Dimensions: width 89 cm ; depth 19 cm. Not suitable for a 30 cm shelf.") &&
    searched.n.some(node => node.t === "Available now. Includes 3745 pieces."), "keyword discovery missed omitted visible evidence");
  assert(searched.n[0].k === searchAnchor && searched.n.every(node => !node.o && !node.u && node.p === undefined),
    "text search widened action authority or attributed evidence to the region");
  assert(!/hidden-answer|editable-answer|secret-answer|frame-answer|outside-answer/.test(searchedWire),
    "text search escaped its exact region or privacy boundaries");
  const searchAbsent = JSON.parse(invoke(148, 148, { k: "text_search", a: searchAnchor, q: "unmentioned" }));
  assert(searchAbsent.n.length === 1 && searchAbsent.c === "complete", "complete search miss invented evidence");
  const abstractFieldSearch = JSON.parse(invoke(148, 148, { k: "text_search", a: searchAnchor, q: "availability" }));
  assert(abstractFieldSearch.n.length === 1 && abstractFieldSearch.c === "complete",
    "literal keyword discovery silently interpreted an abstract field name");
  const visibleWordingSearch = JSON.parse(invoke(148, 148, { k: "text_search", a: searchAnchor, q: "AVAILABLE" }));
  assert(visibleWordingSearch.n.some(node => node.t === "Available now. Includes 3745 pieces."),
    "page wording failed to recover evidence after an abstract-field search miss");
  const searchLimited = JSON.parse(invoke(149, 149, { k: "text_search", a: searchAnchor, q: "dimensions" }, { x: 128 }));
  assert(searchLimited.c === "inspection_limit" && searchLimited.n.length === 1, "search scan ceiling became a false complete miss");
  for (const query of ["$", " € ", "%"]) {
    const symbolSearch = JSON.parse(invoke(149, 149, { k: "text_search", a: searchAnchor, q: query }));
    assert(symbolSearch.n.length === 2 && symbolSearch.n[1].t === "$349.99 / €319.99; 15% discount.",
      "literal symbol query failed to recover unlabeled numeric evidence");
    assert(symbolSearch.n.every(node => !node.o && !node.u && node.p === undefined),
      "symbol search widened action authority");
  }
  const metacharacterSearch = JSON.parse(invoke(149, 149, { k: "text_search", a: searchAnchor, q: "$[]" }));
  assert(metacharacterSearch.n.length === 1 && metacharacterSearch.c === "complete",
    "symbol query was treated as a regex or split into broad punctuation alternatives");
  const punctuatedWordSearch = JSON.parse(invoke(149, 149, { k: "text_search", a: searchAnchor, q: "Available?" }));
  assert(punctuatedWordSearch.n.length === 2 && punctuatedWordSearch.n[1].t === "Available now. Includes 3745 pieces.",
    "sentence punctuation changed exact-word matching");
  for (const query of ["", "   ", "x".repeat(257), "dimensions\nwidth", "width\u202edepth", "width\u200bdepth", "sk-private-search-query-value"]) {
    assert(invoke(150, 150, { k: "text_search", a: searchAnchor, q: query }).includes("invalid_request"), "invalid search query admitted");
  }
  const searchSecret = searchable.append(new Element("p"));
  searchSecret.append(new CharacterData("dimensions sk-private-search-source-value"));
  const interruptedQuote = searchable.append(new Element("p"));
  interruptedQuote.append(new CharacterData("width"));
  interruptedQuote.append(new Element("span", { contenteditable: "true" })).append(new CharacterData("not"));
  interruptedQuote.append(new CharacterData("supported"));
  setOwner(body, document);
  invoke(150, 150, { k: "initial" });
  const sourcePrivacyWire = invoke(151, 151, { k: "text_search", a: searchAnchor, q: "dimensions width supported" });
  assert(!sourcePrivacyWire.includes("sk-private-search-source-value") && !sourcePrivacyWire.includes("width supported"),
    "search disclosed a credential or joined a quote across an excluded editable boundary");
  searchable._children.values = [];
  for (let index = 0; index < 20; index += 1) {
    searchable.append(new Element("p")).append(new CharacterData(`dimensions ${index} ${"界".repeat(700)}`));
  }
  setOwner(body, document);
  const searchOutput = JSON.parse(invoke(152, 152, { k: "text_search", a: searchAnchor, q: "dimensions" }));
  assert(searchOutput.n.length <= 17 && searchOutput.n.reduce((sum, node) => sum + Buffer.byteLength(node.t || ""), 0) <= 8192 &&
    searchOutput.c !== "complete", "keyword search exceeded source/text budget or hid omissions");
  searchable._children.values = [];
  for (let index = 0; index < 40; index += 1) {
    searchable.append(new Element("p")).append(new CharacterData(" ".repeat(4000)));
  }
  searchable.append(new Element("p")).append(new CharacterData("dimensions beyond scan budget"));
  setOwner(body, document);
  const searchWhitespace = JSON.parse(invoke(153, 153, { k: "text_search", a: searchAnchor, q: "dimensions" }));
  assert(searchWhitespace.c === "inspection_limit" && searchWhitespace.n.length === 1,
    "raw whitespace evaded the native search byte-scan ceiling");
  body.attributes.contenteditable = "true";
  assert(invoke(154, 154, { k: "text_search", a: searchAnchor, q: "dimensions" }).includes("anchor_missing"),
    "an acknowledged region bypassed its newly editable ancestor");
  delete body.attributes.contenteditable;

  // A bounded raw-fragment scan is an omitted-content boundary, even when
  // normalization produced only a short prefix. Never join the next sibling
  // onto that prefix and silently remove a skipped negation from the quote.
  searchable._children.values = [];
  const scanInterrupted = searchable.append(new Element("p"));
  scanInterrupted.append(new CharacterData(`width ${" ".repeat(40000)}not `));
  scanInterrupted.append(new CharacterData("supported"));
  setOwner(body, document);
  const scanInitial = JSON.parse(invoke(155, 155, { k: "initial" }));
  const scanAnchor = scanInitial.n.find(node => node.r === "landmark").k;
  const scanInterruptedResult = JSON.parse(invoke(156, 156, { k: "text_search", a: scanAnchor, q: "width supported" }));
  assert(scanInterruptedResult.c === "scope_boundary" &&
    scanInterruptedResult.n.some(node => node.t === "width") &&
    !scanInterruptedResult.n.some(node => node.t === "width supported"),
    "raw scan truncation stitched a false quote across an omitted negation");
  scanInterrupted._children.values = [];
  scanInterrupted.append(new CharacterData("width"));
  scanInterrupted.append(new CharacterData(`${" ".repeat(40000)}not `));
  scanInterrupted.append(new CharacterData("supported"));
  setOwner(body, document);
  const emptyScanPrefix = JSON.parse(invoke(157, 157, { k: "text_search", a: scanAnchor, q: "width supported" }));
  assert(emptyScanPrefix.c === "scope_boundary" && !emptyScanPrefix.n.some(node => node.t === "width supported"),
    "an empty scan-truncated prefix failed to break the pending quote");
  scanInterrupted._children.values = [];
  for (const fragment of ["width", "not", "supported"]) scanInterrupted.append(new CharacterData(fragment));
  setOwner(body, document);
  const intactSearchQuote = JSON.parse(invoke(158, 158, { k: "text_search", a: scanAnchor, q: "width supported" }));
  assert(intactSearchQuote.n.some(node => node.t === "width not supported"),
    "untruncated sibling fragments lost contiguous quote coalescing");
  scanInterrupted._children.values = [];
  scanInterrupted.append(new CharacterData("width"));
  const boundedChildren = scanInterrupted.append(new Element("span"));
  for (let index = 0; index < 200; index += 1) boundedChildren.append(new Element("span"));
  boundedChildren.append(new CharacterData("not"));
  scanInterrupted.append(new CharacterData("supported"));
  setOwner(body, document);
  const childScanBoundary = JSON.parse(invoke(159, 159, { k: "text_search", a: scanAnchor, q: "width supported" }, { x: 128 }));
  assert(childScanBoundary.c === "inspection_limit" && childScanBoundary.n.some(node => node.t === "width") &&
    !childScanBoundary.n.some(node => node.t === "width supported"),
    "bounded child enumeration stitched a quote across an omitted negation");
  searchable._children.values = [];
  searchable.append(new Element("h2")).append(new CharacterData("Clipped source region"));
  searchable.append(scanInterrupted);
  setOwner(body, document);
  const clippedInitial = JSON.parse(invoke(160, 160, { k: "initial" }));
  const clippedHeading = clippedInitial.n.find(node => node.n === "Clipped source region").k;
  const clippedWindow = JSON.parse(invoke(161, 161, { k: "surrounding_text", a: clippedHeading, p: 0, n: 1024 }, { x: 128 }));
  assert(!clippedWindow.n.some(node => node.t === "width supported"),
    "surrounding text stitched a quote across clipped child enumeration");

  process.stdout.write(`${JSON.stringify({
    bounded_visible_text_search: true,
    offscreen_anchor_wire_bytes: inventoryExtraBytes,
    parent_region_nodes: region.n.length,
    parent_region_wire_bytes: Buffer.byteLength(regionWire),
    surrounding_source_nodes: surrounding.n.length,
    field_local_discovery: true,
    surrounding_fresh_source_continuation: true,
    surrounding_source_quote_fidelity: true,
    surrounding_fragment_coalescing: true,
    surrounding_bidirectional_admission: true,
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
    native_transport_settled: true,
    native_transport_dormant_pull: true,
    action_descriptor_golden: true,
    no_webcrypto_required: true,
    fixed_action_execution: true,
    fixed_fill_execution: true,
    fixed_select_execution: true,
    document_parent_option_brand_guard: true,
    select_rewrite_nonretryable: true,
    fill_native_primitives_captured: true,
    fill_input_event_contract: true,
    fill_contenteditable_excluded: true,
    fill_hostile_transitions_rejected: true,
    fill_observed_refusals_nonretryable: true,
    fill_forged_and_flood_terminals_rejected: true,
    same_node_repurpose_rejected: true,
    unrelated_mutation_allowed: true,
    occlusion_rejected: true,
    geometry_change_rejected: true,
    malformed_request_rejected: true,
    oversized_action_subtree_rejected: true,
    nested_control_names_bounded: true,
    inline_prose_sources_bounded: true,
    captured_link_destination_getter: true,
    immutable: true
  })}\n`);
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}
