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
    metadataOverflowSnapshot.c === "text_limit" &&
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
  assert(largeProseSnapshot.c === "text_limit" && largeProseSnapshot.n.some((node) => node.r === "paragraph" && Buffer.byteLength(node.t) <= 4096), "inline copies escaped the original per-prose-field ceiling");

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
