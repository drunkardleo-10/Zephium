(() => {
  "use strict";

  const GLOBAL_NAME = "__zephiumSemanticRuntimeV1";
  const PROTOCOL_VERSION = 1;
  const WIRE_VERSION = 1;
  const MAX_REQUEST_BYTES = 2048;
  const MAX_SAFE_INTEGER = 9007199254740991;
  const MAX_NODES = 512;
  const MAX_TEXT_BYTES = 131072;
  const MIN_WIRE_BYTES = 1024;
  const MAX_WIRE_BYTES = 262144;
  const MAX_VISITED_NODES = 32768;
  const MAX_TREE_DEPTH = 32;
  const MAX_NAME_BYTES = 512;
  const MAX_NODE_TEXT_BYTES = 4096;
  const MAX_VALUE_BYTES = 1024;
  const MAX_SURROUNDING_BYTES = 8192;
  const MAX_TRACKED_IDENTITIES = 2048;

  const objectDefineProperty = Object.defineProperty;
  const objectFreeze = Object.freeze;
  const objectGetOwnPropertyDescriptor = Object.getOwnPropertyDescriptor;
  const objectKeys = Object.keys;
  const objectHasOwn = Function.call.bind(Object.prototype.hasOwnProperty);
  const arrayIsArray = Array.isArray;
  const jsonParse = JSON.parse;
  const jsonStringify = JSON.stringify;
  const reflectApply = Reflect.apply;
  const numberIsFinite = Number.isFinite;
  const numberIsSafeInteger = Number.isSafeInteger;
  const mathRound = Math.round;
  const mathMin = Math.min;
  const mathMax = Math.max;
  const stringToLowerCase = String.prototype.toLowerCase;
  const stringSlice = String.prototype.slice;
  const stringSplit = String.prototype.split;
  const stringTrim = String.prototype.trim;

  if (objectHasOwn(globalThis, GLOBAL_NAME)) {
    return;
  }

  const apply = (callable, receiver, argumentsList) =>
    reflectApply(callable, receiver, argumentsList);
  const getter = (prototype, name) => {
    if (typeof prototype !== "object" || prototype === null) return null;
    const descriptor = objectGetOwnPropertyDescriptor(prototype, name);
    return descriptor && typeof descriptor.get === "function" ? descriptor.get : null;
  };
  const read = (accessor, receiver) =>
    accessor === null ? undefined : apply(accessor, receiver, []);

  const nodeTypeGetter = getter(Node.prototype, "nodeType");
  const nodeParentGetter = getter(Node.prototype, "parentNode");
  const nodeOwnerDocumentGetter = getter(Node.prototype, "ownerDocument");
  const nodeConnectedGetter = getter(Node.prototype, "isConnected");
  const characterDataGetter = getter(CharacterData.prototype, "data");
  const elementTagGetter = getter(Element.prototype, "tagName");
  const elementShadowGetter = getter(Element.prototype, "shadowRoot");
  const documentElementGetter = getter(Document.prototype, "documentElement");
  const documentActiveGetter = getter(Document.prototype, "activeElement");
  const shadowActiveGetter =
    typeof ShadowRoot === "function" ? getter(ShadowRoot.prototype, "activeElement") : null;
  const nodeListLengthGetter = getter(NodeList.prototype, "length");
  const nodeListItem = NodeList.prototype.item;
  const getAttribute = Element.prototype.getAttribute;
  const hasAttribute = Element.prototype.hasAttribute;
  const getBoundingClientRect = Element.prototype.getBoundingClientRect;
  const documentGetElementById = Document.prototype.getElementById;
  const getComputedStyleFixed = globalThis.getComputedStyle;
  const weakMapGet = WeakMap.prototype.get;
  const weakMapSet = WeakMap.prototype.set;

  const inputValueGetter =
    typeof HTMLInputElement === "function" ? getter(HTMLInputElement.prototype, "value") : null;
  const inputCheckedGetter =
    typeof HTMLInputElement === "function" ? getter(HTMLInputElement.prototype, "checked") : null;
  const inputLabelsGetter =
    typeof HTMLInputElement === "function" ? getter(HTMLInputElement.prototype, "labels") : null;
  const textareaValueGetter =
    typeof HTMLTextAreaElement === "function"
      ? getter(HTMLTextAreaElement.prototype, "value")
      : null;
  const textareaLabelsGetter =
    typeof HTMLTextAreaElement === "function"
      ? getter(HTMLTextAreaElement.prototype, "labels")
      : null;
  const selectIndexGetter =
    typeof HTMLSelectElement === "function"
      ? getter(HTMLSelectElement.prototype, "selectedIndex")
      : null;
  const selectLabelsGetter =
    typeof HTMLSelectElement === "function" ? getter(HTMLSelectElement.prototype, "labels") : null;
  const optionIndexGetter =
    typeof HTMLOptionElement === "function" ? getter(HTMLOptionElement.prototype, "index") : null;
  const optionSelectedGetter =
    typeof HTMLOptionElement === "function"
      ? getter(HTMLOptionElement.prototype, "selected")
      : null;

  const nodeKeys = new WeakMap();
  const keyNodes = new Map();
  let nextNodeKey = 1;
  let busy = false;

  const IDENTITY_EXHAUSTED = objectFreeze({});

  const fault = (code) => `E1:${code}`;

  function isPlainObject(value) {
    return typeof value === "object" && value !== null && !arrayIsArray(value);
  }

  function hasExactKeys(value, expected) {
    if (!isPlainObject(value)) return false;
    const actual = objectKeys(value);
    if (actual.length !== expected.length) return false;
    for (let index = 0; index < expected.length; index += 1) {
      if (!objectHasOwn(value, expected[index])) return false;
    }
    return true;
  }

  function isPositiveSafeInteger(value) {
    return numberIsSafeInteger(value) && value > 0 && value <= MAX_SAFE_INTEGER;
  }

  function parseRequest(encoded) {
    if (typeof encoded !== "string" || encoded.length === 0 || encoded.length > MAX_REQUEST_BYTES) {
      return null;
    }
    for (let index = 0; index < encoded.length; index += 1) {
      if (encoded.charCodeAt(index) > 127) return null;
    }

    let request;
    try {
      request = apply(jsonParse, JSON, [encoded]);
    } catch (_) {
      return null;
    }
    if (!hasExactKeys(request, ["v", "i", "g", "s", "b"])) return null;
    if (
      request.v !== PROTOCOL_VERSION ||
      !isPositiveSafeInteger(request.i) ||
      !isPositiveSafeInteger(request.g)
    ) {
      return null;
    }

    const budget = request.b;
    if (!hasExactKeys(budget, ["n", "t", "w", "x", "geo"])) return null;
    if (
      !numberIsSafeInteger(budget.n) ||
      budget.n < 1 ||
      budget.n > MAX_NODES ||
      !numberIsSafeInteger(budget.t) ||
      budget.t < 1 ||
      budget.t > MAX_TEXT_BYTES ||
      !numberIsSafeInteger(budget.w) ||
      budget.w < MIN_WIRE_BYTES ||
      budget.w > MAX_WIRE_BYTES ||
      !numberIsSafeInteger(budget.x) ||
      budget.x < budget.n ||
      budget.x > MAX_VISITED_NODES ||
      typeof budget.geo !== "boolean"
    ) {
      return null;
    }

    const scope = request.s;
    if (!isPlainObject(scope) || typeof scope.k !== "string") return null;
    if (scope.k === "initial") {
      if (!hasExactKeys(scope, ["k"])) return null;
    } else if (
      scope.k === "region" ||
      scope.k === "subtree" ||
      scope.k === "table" ||
      scope.k === "frame"
    ) {
      if (!hasExactKeys(scope, ["k", "a"]) || !isPositiveSafeInteger(scope.a)) return null;
    } else if (scope.k === "surrounding_text") {
      if (
        !hasExactKeys(scope, ["k", "a", "p", "n"]) ||
        !isPositiveSafeInteger(scope.a) ||
        !numberIsSafeInteger(scope.p) ||
        scope.p < 0 ||
        scope.p > MAX_SURROUNDING_BYTES ||
        !numberIsSafeInteger(scope.n) ||
        scope.n < 0 ||
        scope.n > MAX_SURROUNDING_BYTES ||
        scope.p + scope.n < 1 ||
        scope.p + scope.n > MAX_SURROUNDING_BYTES
      ) {
        return null;
      }
    } else {
      return null;
    }
    return request;
  }

  function utf8Length(value, ceiling) {
    let bytes = 0;
    for (const character of value) {
      const point = character.codePointAt(0);
      bytes += point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4;
      if (bytes > ceiling) return bytes;
    }
    return bytes;
  }

  function isWhitespace(point) {
    return (
      point === 0x09 ||
      point === 0x0a ||
      point === 0x0b ||
      point === 0x0c ||
      point === 0x0d ||
      point === 0x20 ||
      point === 0x85 ||
      point === 0xa0 ||
      point === 0x1680 ||
      (point >= 0x2000 && point <= 0x200a) ||
      point === 0x2028 ||
      point === 0x2029 ||
      point === 0x202f ||
      point === 0x205f ||
      point === 0x3000
    );
  }

  function isForbiddenTextPoint(point) {
    return (
      point < 0x20 ||
      (point >= 0x7f && point <= 0x9f) ||
      point === 0xad ||
      point === 0x61c ||
      point === 0x180e ||
      (point >= 0x200b && point <= 0x200f) ||
      (point >= 0x202a && point <= 0x202e) ||
      (point >= 0x2060 && point <= 0x2064) ||
      (point >= 0x2066 && point <= 0x206f) ||
      point === 0xfeff ||
      (point >= 0xfff9 && point <= 0xfffb) ||
      point === 0xe0001 ||
      (point >= 0xe0020 && point <= 0xe007f)
    );
  }

  function normalizeText(raw, byteLimit) {
    if (typeof raw !== "string" || byteLimit <= 0) {
      return { value: "", bytes: 0, truncated: typeof raw === "string" && raw.length > 0 };
    }
    let value = "";
    let bytes = 0;
    let pendingSpace = false;
    let inspected = 0;
    let truncated = false;
    const scanLimit = byteLimit * 8 + 256;
    for (const character of raw) {
      inspected += 1;
      if (inspected > scanLimit) {
        truncated = true;
        break;
      }
      const point = character.codePointAt(0);
      if (isWhitespace(point)) {
        if (value.length !== 0) pendingSpace = true;
        continue;
      }
      if (isForbiddenTextPoint(point)) continue;
      const characterBytes = point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4;
      const separatorBytes = pendingSpace && value.length !== 0 ? 1 : 0;
      if (bytes + separatorBytes + characterBytes > byteLimit) {
        truncated = true;
        break;
      }
      if (separatorBytes !== 0) {
        value += " ";
        bytes += 1;
      }
      value += character;
      bytes += characterBytes;
      pendingSpace = false;
    }
    return { value, bytes, truncated };
  }

  function looksLikeSecret(value) {
    const trimmed = apply(stringTrim, value, []);
    if (trimmed.length < 8) return false;
    const lower = apply(stringToLowerCase, trimmed, []);
    if (
      lower.includes("-----begin private key-----") ||
      lower.includes("-----begin rsa private key-----") ||
      lower.includes("-----begin ec private key-----")
    ) {
      return true;
    }
    const words = apply(stringSplit, lower, [/\s+/]);
    for (let index = 0; index + 1 < words.length; index += 1) {
      if ((words[index] === "bearer" || words[index] === "basic") && words[index + 1].length >= 12) {
        return true;
      }
    }
    const tokens = apply(stringSplit, trimmed, [/[^A-Za-z0-9_.-]+/]);
    for (const token of tokens) {
      if (token.length < 8) continue;
      const tokenLower = apply(stringToLowerCase, token, []);
      if (
        tokenLower.startsWith("sk-") ||
        tokenLower.startsWith("sk_live_") ||
        tokenLower.startsWith("rk_live_") ||
        tokenLower.startsWith("ghp_") ||
        tokenLower.startsWith("github_pat_") ||
        tokenLower.startsWith("glpat-") ||
        tokenLower.startsWith("xoxb-") ||
        tokenLower.startsWith("xoxp-") ||
        token.startsWith("AKIA") ||
        token.startsWith("ASIA") ||
        token.startsWith("AIza")
      ) {
        return true;
      }
      const parts = apply(stringSplit, token, ["."]);
      if (
        parts.length === 3 &&
        parts[0].startsWith("eyJ") &&
        parts[1].length >= 8 &&
        parts[2].length >= 8
      ) {
        return true;
      }
    }
    return false;
  }

  function lower(value) {
    return apply(stringToLowerCase, value, []);
  }

  function attribute(element, name, limit = 1024) {
    const value = apply(getAttribute, element, [name]);
    if (typeof value !== "string") return null;
    return value.length > limit ? apply(stringSlice, value, [0, limit]) : value;
  }

  function has(element, name) {
    return apply(hasAttribute, element, [name]);
  }

  function tagName(element) {
    const value = read(elementTagGetter, element);
    return typeof value === "string" ? lower(value) : "";
  }

  function nodeType(node) {
    return read(nodeTypeGetter, node);
  }

  function mark(state, completeness, stop = true) {
    if (state.completeness === "complete") state.completeness = completeness;
    if (stop) state.stopped = true;
  }

  function visit(state) {
    if (state.visited >= state.request.b.x) {
      mark(state, "inspection_limit");
      return false;
    }
    state.visited += 1;
    return true;
  }

  function belongsToCurrentDocument(node) {
    if (read(nodeConnectedGetter, node) !== true) return false;
    const owner = read(nodeOwnerDocumentGetter, node);
    return node === document || owner === document;
  }

  function sweepIdentities(generation) {
    const earliestGeneration = generation > 1 ? generation - 1 : 1;
    for (const [key, entry] of keyNodes) {
      if (
        entry.generation < earliestGeneration ||
        !belongsToCurrentDocument(entry.node)
      ) {
        keyNodes.delete(key);
      }
    }
  }

  function keyFor(node, generation) {
    const current = apply(weakMapGet, nodeKeys, [node]);
    if (current !== undefined) {
      const entry = keyNodes.get(current);
      if (entry === undefined) {
        if (keyNodes.size >= MAX_TRACKED_IDENTITIES) sweepIdentities(generation);
        if (keyNodes.size >= MAX_TRACKED_IDENTITIES) throw IDENTITY_EXHAUSTED;
        keyNodes.set(current, { node, generation });
      } else {
        entry.node = node;
        entry.generation = generation;
      }
      return current;
    }
    if (keyNodes.size >= MAX_TRACKED_IDENTITIES) sweepIdentities(generation);
    if (keyNodes.size >= MAX_TRACKED_IDENTITIES || nextNodeKey > MAX_SAFE_INTEGER) {
      throw IDENTITY_EXHAUSTED;
    }
    const key = nextNodeKey;
    nextNodeKey += 1;
    apply(weakMapSet, nodeKeys, [node, key]);
    keyNodes.set(key, { node, generation });
    return key;
  }

  function resolveKey(key) {
    const entry = keyNodes.get(key);
    if (entry === undefined) return null;
    if (!belongsToCurrentDocument(entry.node)) {
      keyNodes.delete(key);
      return null;
    }
    return entry.node;
  }

  const ariaRoles = objectFreeze({
    group: "group",
    document: "document",
    article: "document",
    region: "landmark",
    navigation: "landmark",
    main: "landmark",
    banner: "landmark",
    complementary: "landmark",
    contentinfo: "landmark",
    form: "landmark",
    search: "landmark",
    heading: "heading",
    paragraph: "paragraph",
    link: "link",
    button: "button",
    textbox: "textbox",
    searchbox: "searchbox",
    checkbox: "checkbox",
    radio: "radio",
    combobox: "combobox",
    listbox: "listbox",
    option: "option",
    spinbutton: "spinbutton",
    slider: "slider",
    tab: "tab",
    menuitem: "menu_item",
    menuitemcheckbox: "menu_item",
    menuitemradio: "menu_item",
    dialog: "dialog",
    alertdialog: "dialog",
    list: "list",
    listitem: "list_item",
    table: "table",
    grid: "table",
    treegrid: "table",
    row: "row",
    columnheader: "cell_header",
    rowheader: "cell_header",
    cell: "cell",
    gridcell: "cell",
    img: "image",
    progressbar: "progress",
    meter: "progress",
    status: "status",
    alert: "status",
    log: "status"
  });

  function explicitRole(element) {
    const raw = attribute(element, "role", 256);
    if (raw === null) return null;
    const tokens = apply(stringSplit, lower(raw), [/\s+/]);
    for (const token of tokens) {
      if (token === "none" || token === "presentation") return { suppressed: true };
      if (objectHasOwn(ariaRoles, token)) return { role: ariaRoles[token] };
    }
    return null;
  }

  function hasAuthorName(element) {
    return (
      attribute(element, "aria-labelledby", 2) !== null ||
      attribute(element, "aria-label", 2) !== null ||
      attribute(element, "title", 2) !== null
    );
  }

  function classify(node) {
    if (node === document) return { role: "document", tag: "#document", inputType: "" };
    if (nodeType(node) !== 1) return null;
    const tag = tagName(node);
    if (tag === "iframe" || tag === "frame") {
      return { role: "frame_boundary", tag, inputType: "" };
    }
    const inputType = tag === "input" ? lower(attribute(node, "type", 64) || "text") : "";
    if (tag === "input" && inputType === "hidden") return null;
    if (tag === "input" && inputType === "password") {
      return { role: "password", tag, inputType };
    }

    const explicit = explicitRole(node);
    if (explicit !== null) {
      if (explicit.suppressed === true) return null;
      return { role: explicit.role, tag, inputType };
    }

    if (tag === "html" || tag === "body" || tag === "div" || tag === "fieldset" || tag === "details") {
      return tag === "fieldset" || tag === "details"
        ? { role: "group", tag, inputType }
        : null;
    }
    if (tag === "article") return { role: "document", tag, inputType };
    if (tag === "main" || tag === "nav" || tag === "header" || tag === "footer" || tag === "aside") {
      return { role: "landmark", tag, inputType };
    }
    if ((tag === "section" || tag === "form") && hasAuthorName(node)) {
      return { role: "landmark", tag, inputType };
    }
    if (/^h[1-6]$/.test(tag)) {
      return { role: "heading", tag, inputType, level: Number(tag[1]) };
    }
    if (tag === "p" || tag === "pre" || tag === "blockquote" || tag === "dt" || tag === "dd") {
      return { role: "paragraph", tag, inputType };
    }
    if (tag === "a" && attribute(node, "href", 1) !== null) {
      return { role: "link", tag, inputType };
    }
    if (tag === "button" || tag === "summary") return { role: "button", tag, inputType };
    if (tag === "input") {
      if (["button", "submit", "reset", "image"].includes(inputType)) {
        return { role: "button", tag, inputType };
      }
      if (inputType === "checkbox") return { role: "checkbox", tag, inputType };
      if (inputType === "radio") return { role: "radio", tag, inputType };
      if (inputType === "range") return { role: "slider", tag, inputType };
      if (inputType === "number") return { role: "spinbutton", tag, inputType };
      if (inputType === "search") return { role: "searchbox", tag, inputType };
      if (inputType === "file") return { role: "button", tag, inputType, noOperations: true };
      return { role: "textbox", tag, inputType };
    }
    if (tag === "textarea") return { role: "textbox", tag, inputType };
    if (tag === "select") {
      const multiple = has(node, "multiple");
      const size = Number(attribute(node, "size", 16) || "0");
      return { role: multiple || size > 1 ? "listbox" : "combobox", tag, inputType };
    }
    if (tag === "option") return { role: "option", tag, inputType };
    if (tag === "dialog") return { role: "dialog", tag, inputType };
    if (tag === "ul" || tag === "ol" || tag === "menu") return { role: "list", tag, inputType };
    if (tag === "li") return { role: "list_item", tag, inputType };
    if (tag === "table") return { role: "table", tag, inputType };
    if (tag === "tr") return { role: "row", tag, inputType };
    if (tag === "th") return { role: "cell_header", tag, inputType };
    if (tag === "td") return { role: "cell", tag, inputType };
    if (tag === "img" && attribute(node, "alt", 1) !== null) {
      return { role: "image", tag, inputType };
    }
    if (tag === "progress" || tag === "meter") return { role: "progress", tag, inputType };
    if (tag === "output") return { role: "status", tag, inputType };
    const editable = lower(attribute(node, "contenteditable", 16) || "");
    if (editable === "" || editable === "true" || editable === "plaintext-only") {
      if (attribute(node, "contenteditable", 16) !== null) {
        return { role: "textbox", tag, inputType, contentEditable: true };
      }
    }
    return null;
  }

  function shouldSkipSubtree(element) {
    const tag = tagName(element);
    return (
      tag === "script" ||
      tag === "style" ||
      tag === "template" ||
      tag === "noscript" ||
      tag === "head" ||
      tag === "meta" ||
      tag === "link" ||
      has(element, "hidden") ||
      has(element, "inert") ||
      lower(attribute(element, "aria-hidden", 16) || "") === "true"
    );
  }

  function styleIsVisible(element) {
    let style;
    try {
      style = apply(getComputedStyleFixed, globalThis, [element]);
    } catch (_) {
      return false;
    }
    return !(
      style.display === "none" ||
      style.visibility === "hidden" ||
      style.visibility === "collapse" ||
      style.contentVisibility === "hidden"
    );
  }

  function elementRect(element) {
    let rect;
    try {
      rect = apply(getBoundingClientRect, element, []);
    } catch (_) {
      return null;
    }
    const x = Number(rect.x);
    const y = Number(rect.y);
    const width = Number(rect.width);
    const height = Number(rect.height);
    if (
      !numberIsFinite(x) ||
      !numberIsFinite(y) ||
      !numberIsFinite(width) ||
      !numberIsFinite(height) ||
      width <= 0 ||
      height <= 0
    ) {
      return null;
    }
    return { x, y, width, height };
  }

  function inViewport(rect) {
    const width = numberIsFinite(Number(globalThis.innerWidth)) ? Number(globalThis.innerWidth) : 0;
    const height = numberIsFinite(Number(globalThis.innerHeight)) ? Number(globalThis.innerHeight) : 0;
    return rect.x + rect.width > 0 && rect.y + rect.height > 0 && rect.x < width && rect.y < height;
  }

  function wireRect(rect) {
    const clampSigned = (value) => mathMax(-1000000, mathMin(1000000, mathRound(value)));
    const clampUnsigned = (value) => mathMax(0, mathMin(1000000, mathRound(value)));
    return {
      x: clampSigned(rect.x),
      y: clampSigned(rect.y),
      w: clampUnsigned(rect.width),
      h: clampUnsigned(rect.height)
    };
  }

  function booleanAttribute(element, ariaName, nativeGetter) {
    const aria = lower(attribute(element, ariaName, 16) || "");
    if (aria === "true") return true;
    if (aria === "false") return false;
    if (nativeGetter !== null) {
      try {
        return read(nativeGetter, element) === true;
      } catch (_) {
        return false;
      }
    }
    return false;
  }

  function disabledState(element, inherited) {
    return inherited || has(element, "disabled") || lower(attribute(element, "aria-disabled", 16) || "") === "true";
  }

  function stateBits(element, descriptor, disabled, focused) {
    let bits = 0;
    if (
      (descriptor.role === "checkbox" || descriptor.role === "radio") &&
      booleanAttribute(element, "aria-checked", inputCheckedGetter)
    ) {
      bits |= 1;
    }
    if (
      lower(attribute(element, "aria-selected", 16) || "") === "true" ||
      (descriptor.role === "option" && booleanAttribute(element, "aria-selected", optionSelectedGetter))
    ) {
      bits |= 2;
    }
    if (lower(attribute(element, "aria-expanded", 16) || "") === "true") bits |= 4;
    if (disabled) bits |= 8;
    if (has(element, "required") || lower(attribute(element, "aria-required", 16) || "") === "true") {
      bits |= 16;
    }
    const invalid = lower(attribute(element, "aria-invalid", 32) || "");
    if (invalid !== "" && invalid !== "false") bits |= 32;
    if (focused) bits |= 64;
    return bits;
  }

  function operationBits(descriptor, disabled, readonly) {
    if (disabled || descriptor.noOperations === true) return 0;
    switch (descriptor.role) {
      case "link":
      case "button":
      case "checkbox":
      case "radio":
      case "option":
      case "slider":
      case "tab":
      case "menu_item":
        return 1 | 8;
      case "textbox":
      case "password":
      case "searchbox":
      case "spinbutton":
        return readonly ? 1 | 8 : 1 | 2 | 8;
      case "combobox":
        return readonly ? 1 | 8 : 1 | 4 | 8;
      case "listbox":
        return 4 | 8 | 16;
      case "group":
      case "document":
      case "landmark":
      case "list":
      case "table":
        return 16;
      default:
        return 0;
    }
  }

  function focusedElements() {
    const result = new Set();
    let root = document;
    for (let depth = 0; depth <= MAX_TREE_DEPTH; depth += 1) {
      const active = root === document ? read(documentActiveGetter, root) : read(shadowActiveGetter, root);
      if (active === null || active === undefined || result.has(active)) break;
      result.add(active);
      const shadow = read(elementShadowGetter, active);
      if (shadow === null || shadow === undefined) break;
      root = shadow;
    }
    return result;
  }

  function listLength(list) {
    const length = read(nodeListLengthGetter, list);
    return numberIsSafeInteger(length) && length >= 0 ? length : 0;
  }

  function listItem(list, index) {
    return apply(nodeListItem, list, [index]);
  }

  function childNodes(node) {
    return node.childNodes;
  }

  function pushChildren(stack, node, inherited, state) {
    let shadowList = null;
    if (nodeType(node) === 1) {
      const shadow = read(elementShadowGetter, node);
      if (shadow !== null && shadow !== undefined) shadowList = childNodes(shadow);
    }
    const lightList = childNodes(node);
    let remaining = mathMax(0, state.request.b.x - state.visited - stack.length);
    const shadowLength = shadowList === null ? 0 : listLength(shadowList);
    const shadowTake = mathMin(shadowLength, remaining);
    remaining -= shadowTake;
    const lightLength = listLength(lightList);
    const lightTake = mathMin(lightLength, remaining);
    if (shadowTake < shadowLength || lightTake < lightLength) {
      mark(state, "inspection_limit", false);
    }
    for (let index = lightTake - 1; index >= 0; index -= 1) {
      const child = listItem(lightList, index);
      if (child !== null) stack.push({ node: child, ...inherited });
    }
    for (let index = shadowTake - 1; index >= 0; index -= 1) {
      const child = listItem(shadowList, index);
      if (child !== null) stack.push({ node: child, ...inherited });
    }
  }

  function flatText(root, state, limit) {
    const chunks = [];
    let bytes = 0;
    const stack = [{ node: root }];
    while (stack.length !== 0 && bytes < limit && !state.stopped) {
      const current = stack.pop().node;
      if (!visit(state)) break;
      const type = nodeType(current);
      if (type === 3) {
        const raw = read(characterDataGetter, current);
        const normalized = normalizeText(raw, limit - bytes);
        if (normalized.value !== "") {
          if (bytes !== 0 && bytes < limit) {
            chunks.push(" ");
            bytes += 1;
          }
          if (bytes < limit) {
            const clipped = normalizeText(normalized.value, limit - bytes);
            chunks.push(clipped.value);
            bytes += clipped.bytes;
            if (clipped.truncated) mark(state, "text_limit");
          }
        }
        if (normalized.truncated) mark(state, "text_limit");
        continue;
      }
      if (type === 1 && shouldSkipSubtree(current)) continue;
      if (type === 1 || type === 9 || type === 11) pushChildren(stack, current, {}, state);
    }
    return chunks.join("");
  }

  function labelledText(element, descriptor, state) {
    const labelledBy = attribute(element, "aria-labelledby", 1024);
    if (labelledBy !== null) {
      const identifiers = apply(stringSplit, labelledBy, [/\s+/]);
      const labels = [];
      for (let index = 0; index < identifiers.length && index < 8 && !state.stopped; index += 1) {
        const identifier = identifiers[index];
        if (identifier.length === 0 || identifier.length > 128) continue;
        const target = apply(documentGetElementById, document, [identifier]);
        if (target !== null) {
          const text = flatText(target, state, MAX_NAME_BYTES);
          if (text !== "") labels.push(text);
        }
      }
      if (labels.length !== 0) return labels.join(" ");
    }

    const ariaLabel = attribute(element, "aria-label", MAX_NAME_BYTES * 4);
    if (ariaLabel !== null && ariaLabel !== "") return ariaLabel;

    let labelsGetter = null;
    if (descriptor.tag === "input") labelsGetter = inputLabelsGetter;
    if (descriptor.tag === "textarea") labelsGetter = textareaLabelsGetter;
    if (descriptor.tag === "select") labelsGetter = selectLabelsGetter;
    if (labelsGetter !== null) {
      let labels;
      try {
        labels = read(labelsGetter, element);
      } catch (_) {
        labels = null;
      }
      if (labels !== null && labels !== undefined) {
        const length = mathMin(listLength(labels), 4);
        const values = [];
        for (let index = 0; index < length && !state.stopped; index += 1) {
          const label = listItem(labels, index);
          if (label !== null) {
            const text = flatText(label, state, MAX_NAME_BYTES);
            if (text !== "") values.push(text);
          }
        }
        if (values.length !== 0) return values.join(" ");
      }
    }

    if (descriptor.role === "image" || descriptor.inputType === "image") {
      const alt = attribute(element, "alt", MAX_NAME_BYTES * 4);
      if (alt !== null && alt !== "") return alt;
    }
    if (
      descriptor.tag === "input" &&
      ["button", "submit", "reset"].includes(descriptor.inputType)
    ) {
      const value = attribute(element, "value", MAX_NAME_BYTES * 4);
      if (value !== null && value !== "") return value;
    }
    const placeholder = attribute(element, "placeholder", MAX_NAME_BYTES * 4);
    if (placeholder !== null && placeholder !== "") return placeholder;
    const title = attribute(element, "title", MAX_NAME_BYTES * 4);
    return title !== null && title !== "" ? title : null;
  }

  function sensitivityFor(element) {
    const autocomplete = lower(attribute(element, "autocomplete", 256) || "");
    if (
      autocomplete.includes("email") ||
      autocomplete.includes("tel") ||
      autocomplete.includes("name") ||
      autocomplete.includes("address") ||
      autocomplete.includes("postal") ||
      autocomplete.includes("cc-")
    ) {
      return "sensitive";
    }
    return "public";
  }

  function credentialField(element, descriptor, accessibleName) {
    if (descriptor.role === "password") return true;
    const metadata = [
      attribute(element, "autocomplete", 256) || "",
      attribute(element, "name", 256) || "",
      attribute(element, "id", 256) || "",
      accessibleName || ""
    ];
    const joined = lower(metadata.join(" "));
    return (
      joined.includes("password") ||
      joined.includes("passcode") ||
      joined.includes("one-time-code") ||
      joined.includes("verification code") ||
      joined.includes("security code") ||
      joined.includes("api key") ||
      joined.includes("access token") ||
      joined.includes("secret key") ||
      joined.includes("private key") ||
      joined.includes("cc-number") ||
      joined.includes("cc-csc") ||
      joined.includes("card number") ||
      joined.includes("cvv") ||
      joined.includes("cvc")
    );
  }

  function consumeField(raw, fieldLimit, state) {
    const remaining = mathMax(0, state.request.b.t - state.textBytes);
    const normalized = normalizeText(raw, mathMin(fieldLimit, remaining));
    let value = normalized.value;
    let bytes = normalized.bytes;
    let secret = false;
    if (value !== "" && looksLikeSecret(value)) {
      value = "[redacted]";
      bytes = 10;
      secret = true;
      if (bytes > remaining) {
        value = "";
        bytes = 0;
        mark(state, "text_limit");
      }
    }
    state.textBytes += bytes;
    if (normalized.truncated || (raw.length !== 0 && remaining === 0)) mark(state, "text_limit");
    return { value, bytes, secret };
  }

  function setSensitivity(record, sensitivity) {
    if (sensitivity === "secret" || (sensitivity === "sensitive" && record.sensitivity === "public")) {
      record.sensitivity = sensitivity;
      record.wire.q = sensitivity;
    }
  }

  function addName(record, raw, state) {
    const field = consumeField(raw, MAX_NAME_BYTES, state);
    if (field.value !== "") record.wire.n = field.value;
    record.sinkBytes = field.bytes;
    if (field.secret) setSensitivity(record, "secret");
  }

  function addValue(record, raw, state) {
    const field = consumeField(raw, MAX_VALUE_BYTES, state);
    if (field.value !== "") record.wire.v = { k: field.secret ? "redacted" : "text", value: field.value };
    if (field.secret) {
      record.wire.v = { k: "redacted" };
      setSensitivity(record, "secret");
    }
  }

  function appendSink(record, raw, state) {
    if (record.sink === null || state.stopped) return;
    let current = "";
    if (record.sink === "name") current = record.wire.n || "";
    if (record.sink === "text") current = record.wire.t || "";
    if (record.sink === "value" && record.wire.v && record.wire.v.k === "text") {
      current = record.wire.v.value;
    }
    if (current === "[redacted]") return;
    const fieldLimit = record.sink === "name" ? MAX_NAME_BYTES : record.sink === "value" ? MAX_VALUE_BYTES : MAX_NODE_TEXT_BYTES;
    const separator = current === "" ? "" : " ";
    const remainingField = mathMax(0, fieldLimit - record.sinkBytes - (separator === "" ? 0 : 1));
    const separatorBytes = separator === "" ? 0 : 1;
    const remainingGlobal = mathMax(0, state.request.b.t - state.textBytes - separatorBytes);
    const field = consumeField(raw, mathMin(remainingField, remainingGlobal), state);
    if (field.value === "") return;
    const combined = `${current}${separator}${field.value}`;
    if (separator !== "") {
      state.textBytes += 1;
      record.sinkBytes += 1;
    }
    record.sinkBytes += field.bytes;
    if (looksLikeSecret(combined) || field.secret) {
      state.textBytes -= record.sinkBytes;
      if (state.textBytes + 10 > state.request.b.t) {
        mark(state, "text_limit");
        return;
      }
      state.textBytes += 10;
      record.sinkBytes = 10;
      if (record.sink === "name") record.wire.n = "[redacted]";
      if (record.sink === "text") record.wire.t = "[redacted]";
      if (record.sink === "value") record.wire.v = { k: "redacted" };
      setSensitivity(record, "secret");
      return;
    }
    if (record.sink === "name") record.wire.n = combined;
    if (record.sink === "text") record.wire.t = combined;
    if (record.sink === "value") record.wire.v = { k: "text", value: combined };
  }

  function recordSink(descriptor, hasName) {
    if (
      descriptor.role === "button" ||
      descriptor.role === "link" ||
      descriptor.role === "heading" ||
      descriptor.role === "checkbox" ||
      descriptor.role === "radio" ||
      descriptor.role === "option" ||
      descriptor.role === "tab" ||
      descriptor.role === "menu_item" ||
      descriptor.role === "image"
    ) {
      return hasName ? null : "name";
    }
    if (descriptor.contentEditable === true) return "value";
    if (
      descriptor.role === "paragraph" ||
      descriptor.role === "list_item" ||
      descriptor.role === "cell_header" ||
      descriptor.role === "cell" ||
      descriptor.role === "status"
    ) {
      return "text";
    }
    return null;
  }

  function boundedOrdinal(value) {
    const number = Number(value);
    if (!numberIsFinite(number)) return 0;
    return mathMax(0, mathMin(65535, mathRound(number)));
  }

  function buildRecord(element, descriptor, parent, rect, disabled, focused, state) {
    const wire = { k: keyFor(element, state.request.g) };
    if (parent !== null) wire.p = parent;
    wire.r = descriptor.role;
    if (descriptor.role === "heading") {
      const ariaLevel = Number(attribute(element, "aria-level", 8) || "0");
      const level = descriptor.level || ariaLevel;
      if (numberIsSafeInteger(level) && level >= 1 && level <= 6) wire.l = level;
      else wire.l = 2;
    }

    const isDocument = element === document;
    const readonly =
      !isDocument &&
      (has(element, "readonly") || lower(attribute(element, "aria-readonly", 16) || "") === "true");
    const states = isDocument ? 0 : stateBits(element, descriptor, disabled, focused);
    const operations = operationBits(descriptor, disabled, readonly);
    if (states !== 0) wire.s = states;
    if (operations !== 0) wire.o = operations;
    if (rect !== null && state.request.b.geo) wire.b = wireRect(rect);

    const record = { wire, sink: null, sinkBytes: 0, sensitivity: "public", element };
    if (!isDocument) {
      const sensitivity = sensitivityFor(element);
      if (sensitivity !== "public") setSensitivity(record, sensitivity);
      const name = labelledText(element, descriptor, state);
      if (name !== null && name !== "") addName(record, name, state);
      record.sink = recordSink(descriptor, wire.n !== undefined);

      if (descriptor.role === "password") {
        wire.v = { k: "redacted" };
      } else if (descriptor.role === "checkbox" || descriptor.role === "radio") {
        wire.v = { k: "boolean", value: (states & 1) !== 0 };
      } else if (
        descriptor.role === "combobox" ||
        descriptor.role === "listbox" ||
        descriptor.role === "option"
      ) {
        let index = 0;
        try {
          index = descriptor.role === "option" ? read(optionIndexGetter, element) : read(selectIndexGetter, element);
        } catch (_) {
          index = 0;
        }
        wire.v = { k: "ordinal", value: boundedOrdinal(index) };
      } else if (descriptor.role === "slider" || descriptor.role === "progress") {
        const raw = attribute(element, "aria-valuenow", 64) || attribute(element, "value", 64) || "0";
        wire.v = { k: "ordinal", value: boundedOrdinal(raw) };
      } else if (
        descriptor.role === "textbox" ||
        descriptor.role === "searchbox" ||
        descriptor.role === "spinbutton"
      ) {
        if (credentialField(element, descriptor, wire.n || "")) {
          wire.v = { k: "redacted" };
        } else if (descriptor.contentEditable !== true) {
          let value = null;
          try {
            if (descriptor.tag === "input") value = read(inputValueGetter, element);
            if (descriptor.tag === "textarea") value = read(textareaValueGetter, element);
          } catch (_) {
            value = null;
          }
          if (typeof value === "string" && value !== "") addValue(record, value, state);
        }
      }
    }
    return record;
  }

  function textNodeVisible(node) {
    const parent = read(nodeParentGetter, node);
    return parent !== null && nodeType(parent) === 1 && !shouldSkipSubtree(parent) && styleIsVisible(parent) && elementRect(parent) !== null;
  }

  function addRecord(records, record, state) {
    if (records.length >= state.request.b.n) {
      mark(state, "node_limit");
      return null;
    }
    records.push(record);
    return records.length - 1;
  }

  function traverse(root, state, anchored) {
    const records = [];
    const focused = focusedElements();
    const stack = [];

    if (!anchored) {
      const documentRecord = buildRecord(document, classify(document), null, null, false, false, state);
      documentRecord.depth = 0;
      const index = addRecord(records, documentRecord, state);
      const rootElement = read(documentElementGetter, document);
      if (index !== null && rootElement !== null && rootElement !== undefined) {
        stack.push({ node: rootElement, parent: index, sink: null, depth: 1, disabled: false });
      }
    } else if (root === document) {
      const documentRecord = buildRecord(document, classify(document), null, null, false, false, state);
      documentRecord.depth = 0;
      const index = addRecord(records, documentRecord, state);
      const rootElement = read(documentElementGetter, document);
      if (index !== null && rootElement !== null && rootElement !== undefined) {
        stack.push({ node: rootElement, parent: index, sink: null, depth: 1, disabled: false });
      }
    } else {
      stack.push({ node: root, parent: null, sink: null, depth: 0, disabled: false });
    }

    while (stack.length !== 0 && !state.stopped) {
      const item = stack.pop();
      if (!visit(state)) break;
      const type = nodeType(item.node);
      if (type === 3) {
        if (item.sink !== null && textNodeVisible(item.node)) {
          const raw = read(characterDataGetter, item.node);
          if (typeof raw === "string") appendSink(records[item.sink], raw, state);
        }
        continue;
      }
      if (type !== 1 && type !== 9 && type !== 11) continue;
      if (type === 1 && shouldSkipSubtree(item.node)) continue;

      const descriptor = classify(item.node);
      let parent = item.parent;
      let sink = item.sink;
      let disabled = item.disabled;
      if (type === 1) {
        disabled = disabledState(item.node, item.disabled);
        if (descriptor !== null) {
          const visibleStyle = styleIsVisible(item.node);
          const rect = visibleStyle ? elementRect(item.node) : null;
          const optionInExpansion = descriptor.role === "option" && anchored;
          const visible = visibleStyle && (rect !== null || optionInExpansion);
          const initialPriority =
            descriptor.role === "dialog" ||
            descriptor.role === "landmark" ||
            focused.has(item.node);
          const admitted =
            visible && (!anchored ? rect !== null && (initialPriority || inViewport(rect)) : true);
          if (admitted) {
            const semanticDepth = parent === null ? 0 : records[parent].depth + 1;
            if (semanticDepth > MAX_TREE_DEPTH) {
              mark(state, "depth_limit");
              break;
            }
            const record = buildRecord(
              item.node,
              descriptor,
              parent,
              rect,
              disabled,
              focused.has(item.node),
              state
            );
            record.depth = semanticDepth;
            const index = addRecord(records, record, state);
            if (index === null) break;
            parent = index;
            sink = record.sink === null ? null : index;
          } else {
            sink = null;
          }
        }
      }
      if (!state.stopped) {
        pushChildren(
          stack,
          item.node,
          { parent, sink, depth: item.depth + 1, disabled },
          state
        );
      }
    }
    return records;
  }

  function compatibleScope(scope, descriptor) {
    if (descriptor === null) return false;
    if (scope === "region") {
      return ["document", "landmark", "group", "dialog"].includes(descriptor.role);
    }
    if (scope === "table") return descriptor.role === "table";
    if (scope === "frame") return descriptor.role === "frame_boundary";
    if (scope === "subtree" || scope === "surrounding_text") {
      return descriptor.role !== "frame_boundary";
    }
    return false;
  }

  function addRollingChunk(chunks, chunk, byteLimit) {
    if (chunk === "" || byteLimit === 0) return;
    chunks.push(chunk);
    let bytes = 0;
    for (let index = chunks.length - 1; index >= 0; index -= 1) {
      const separator = index === chunks.length - 1 ? 0 : 1;
      const chunkBytes = utf8Length(chunks[index], byteLimit + 1);
      if (bytes + separator + chunkBytes <= byteLimit) {
        bytes += separator + chunkBytes;
      } else {
        const keep = mathMax(0, byteLimit - bytes - separator);
        chunks.splice(0, index + 1, utf8Suffix(chunks[index], keep));
        return;
      }
    }
  }

  function utf8Suffix(value, byteLimit) {
    if (byteLimit <= 0) return "";
    const characters = Array.from(value);
    const suffix = [];
    let bytes = 0;
    for (let index = characters.length - 1; index >= 0; index -= 1) {
      const point = characters[index].codePointAt(0);
      const characterBytes = point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4;
      if (bytes + characterBytes > byteLimit) break;
      suffix.push(characters[index]);
      bytes += characterBytes;
    }
    suffix.reverse();
    return suffix.join("");
  }

  function surroundingText(anchor, state, beforeLimit, afterLimit) {
    const before = [];
    const after = [];
    let afterBytes = 0;
    let seenAnchor = false;
    const root = read(documentElementGetter, document);
    if (root === null || root === undefined) return "";
    const stack = [{ node: root }];
    while (stack.length !== 0 && !state.stopped) {
      const current = stack.pop().node;
      if (!visit(state)) break;
      if (current === anchor) {
        seenAnchor = true;
        continue;
      }
      const type = nodeType(current);
      if (type === 3) {
        if (!textNodeVisible(current)) continue;
        const raw = read(characterDataGetter, current);
        if (typeof raw !== "string") continue;
        if (!seenAnchor) {
          const normalized = normalizeText(raw, beforeLimit);
          addRollingChunk(before, normalized.value, beforeLimit);
        } else if (afterBytes < afterLimit) {
          const normalized = normalizeText(raw, afterLimit - afterBytes);
          if (normalized.value !== "") {
            if (afterBytes !== 0 && afterBytes < afterLimit) {
              after.push(" ");
              afterBytes += 1;
            }
            const clipped = normalizeText(normalized.value, afterLimit - afterBytes);
            after.push(clipped.value);
            afterBytes += clipped.bytes;
          }
        }
        continue;
      }
      if (type === 1 && shouldSkipSubtree(current)) continue;
      if (type === 1 || type === 9 || type === 11) pushChildren(stack, current, {}, state);
      if (seenAnchor && afterBytes >= afterLimit) break;
    }
    if (!seenAnchor) return "";
    const beforeText = before.join(" ");
    const afterText = after.join("");
    if (beforeText === "") return afterText;
    if (afterText === "") return beforeText;
    return `${beforeText} | ${afterText}`;
  }

  function surroundingRecords(anchor, descriptor, state) {
    const visibleStyle = nodeType(anchor) !== 1 || styleIsVisible(anchor);
    const rect = nodeType(anchor) === 1 && visibleStyle ? elementRect(anchor) : null;
    if (nodeType(anchor) === 1 && rect === null && descriptor.role !== "option") return null;
    const focused = focusedElements();
    const disabled = nodeType(anchor) === 1 ? disabledState(anchor, false) : false;
    const record = buildRecord(anchor, descriptor, null, rect, disabled, focused.has(anchor), state);
    record.depth = 0;
    record.sink = "text";
    record.sinkBytes = 0;
    const context = surroundingText(anchor, state, state.request.s.p, state.request.s.n);
    if (context !== "") appendSink(record, context, state);
    return [record];
  }

  function encodeSnapshot(request, records, completeness) {
    const encodedNodes = [];
    for (const record of records) encodedNodes.push(apply(jsonStringify, JSON, [record.wire]));

    const compose = (status) => {
      const header = `{"v":${WIRE_VERSION},"i":${request.i},"g":${request.g},"c":"${status}","n":[`;
      const parts = [];
      let bytes = utf8Length(header, request.b.w + 1) + 2;
      let truncated = false;
      for (const encodedNode of encodedNodes) {
        const nodeBytes = utf8Length(encodedNode, request.b.w + 1);
        const separator = parts.length === 0 ? 0 : 1;
        if (bytes + separator + nodeBytes > request.b.w) {
          truncated = true;
          break;
        }
        parts.push(encodedNode);
        bytes += separator + nodeBytes;
      }
      return { value: `${header}${parts.join(",")}]}`, truncated };
    };

    let encoded = compose(completeness);
    if (encoded.truncated) encoded = compose("wire_limit");
    if (utf8Length(encoded.value, request.b.w + 1) > request.b.w) return fault("output_limit");
    return encoded.value;
  }

  function run(request) {
    sweepIdentities(request.g);
    const state = {
      request,
      visited: 0,
      textBytes: 0,
      completeness: "complete",
      stopped: false
    };

    let records;
    if (request.s.k === "initial") {
      records = traverse(document, state, false);
    } else {
      const anchor = resolveKey(request.s.a);
      if (anchor === null) return fault("anchor_missing");
      const descriptor = classify(anchor);
      if (!compatibleScope(request.s.k, descriptor)) return fault("unsupported_scope");
      if (nodeType(anchor) === 1 && descriptor.role !== "option") {
        if (!styleIsVisible(anchor) || elementRect(anchor) === null) return fault("anchor_missing");
      }
      if (request.s.k === "surrounding_text") {
        records = surroundingRecords(anchor, descriptor, state);
        if (records === null) return fault("anchor_missing");
      } else if (request.s.k === "frame") {
        records = traverse(anchor, state, true);
        if (state.completeness === "complete") state.completeness = "scope_boundary";
      } else {
        records = traverse(anchor, state, true);
      }
    }
    return encodeSnapshot(request, records, state.completeness);
  }

  function invoke(encoded) {
    if (busy) return fault("busy");
    const request = parseRequest(encoded);
    if (request === null) return fault("invalid_request");
    busy = true;
    try {
      return run(request);
    } catch (error) {
      if (error === IDENTITY_EXHAUSTED) return fault("identity_exhausted");
      return fault("internal");
    } finally {
      busy = false;
    }
  }

  objectFreeze(invoke);
  const api = objectFreeze({ invoke });
  objectDefineProperty(globalThis, GLOBAL_NAME, {
    value: api,
    writable: false,
    configurable: false,
    enumerable: false
  });
})();
