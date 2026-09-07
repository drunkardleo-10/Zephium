(() => {
  "use strict";

  const GLOBAL_NAME = "__zephiumSemanticRuntimeV1";
  const PROTOCOL_VERSION = 1;
  const WIRE_VERSION = 1;
  const MAX_REQUEST_BYTES = 17701;
  const MAX_SAFE_INTEGER = 9007199254740991;
  const MAX_NODES = 512;
  const MAX_TEXT_BYTES = 131072;
  const MIN_WIRE_BYTES = 1024;
  const MAX_WIRE_BYTES = 262144;
  const MAX_VISITED_NODES = 32768;
  const MAX_TREE_DEPTH = 32;
  const MAX_NAME_BYTES = 512;
  const MAX_NODE_TEXT_BYTES = 4096;
  const MAX_VALUE_BYTES = 4096;
  const MAX_SURROUNDING_BYTES = 8192;
  const MAX_TRACKED_IDENTITIES = 2048;
  const MAX_DOCUMENT_INVOCATIONS = 4096;
  const MAX_ACTION_DESCRIPTOR_NODES = 128;
  const MAX_ACTION_DESCRIPTOR_TEXT_BYTES = 4096;
  const MAX_ACTION_DESCRIPTOR_WIRE_BYTES = 16384;
  const MAX_ACTION_DESCRIPTOR_VISITED_NODES = 2048;
  const MAX_ACTION_TEXT_BYTES = 4096;
  // Independently bounded page-world transport: fixed command grammar plus
  // worst-case two-byte JSON expansion of one legal 4-KiB replacement.
  const MAX_PAGE_RELAY_COMMAND_BYTES = 8320;
  const CHANNEL_PULL = "P1";
  const CHANNEL_RESULT_PREFIX = "R1:";
  const CHANNEL_ACK = "A1";
  const CHANNEL_STOP = "S1";
  const CHANNEL_EXHAUSTED = "X1";
  const PAGE_RELAY_READY = "data-zephium-fill-relay-ready-v1";
  const PAGE_RELAY_COMMAND = "data-zephium-fill-relay-command-v1";
  const PAGE_RELAY_TERMINAL = "data-zephium-fill-relay-terminal-v1";

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
  const mathAbs = Math.abs;
  const mathMin = Math.min;
  const mathMax = Math.max;
  const stringToLowerCase = String.prototype.toLowerCase;
  const stringCharCodeAt = String.prototype.charCodeAt;
  const stringSlice = String.prototype.slice;
  const stringSplit = String.prototype.split;
  const stringTrim = String.prototype.trim;
  const stringStartsWith = String.prototype.startsWith;
  const stringIncludes = String.prototype.includes;

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
  const setter = (prototype, name) => {
    if (typeof prototype !== "object" || prototype === null) return null;
    const descriptor = objectGetOwnPropertyDescriptor(prototype, name);
    return descriptor && typeof descriptor.set === "function" ? descriptor.set : null;
  };
  const read = (accessor, receiver) =>
    accessor === null ? undefined : apply(accessor, receiver, []);
  const write = (accessor, receiver, value) =>
    accessor === null ? undefined : apply(accessor, receiver, [value]);

  const nodeTypeGetter = getter(Node.prototype, "nodeType");
  const nodeParentGetter = getter(Node.prototype, "parentNode");
  const nodeOwnerDocumentGetter = getter(Node.prototype, "ownerDocument");
  const nodeConnectedGetter = getter(Node.prototype, "isConnected");
  const nodeChildNodesGetter = getter(Node.prototype, "childNodes");
  const nodeContains = Node.prototype.contains;
  const characterDataGetter = getter(CharacterData.prototype, "data");
  const elementTagGetter = getter(Element.prototype, "tagName");
  const elementShadowGetter = getter(Element.prototype, "shadowRoot");
  const documentElementGetter = getter(Document.prototype, "documentElement");
  const documentActiveGetter = getter(Document.prototype, "activeElement");
  const documentReadyStateGetter = getter(Document.prototype, "readyState");
  const shadowActiveGetter =
    typeof ShadowRoot === "function" ? getter(ShadowRoot.prototype, "activeElement") : null;
  const nodeListLengthGetter = getter(NodeList.prototype, "length");
  const nodeListItem = NodeList.prototype.item;
  const getAttribute = Element.prototype.getAttribute;
  const hasAttribute = Element.prototype.hasAttribute;
  const setAttribute = Element.prototype.setAttribute;
  const removeAttribute = Element.prototype.removeAttribute;
  const getBoundingClientRect = Element.prototype.getBoundingClientRect;
  const documentGetElementById = Document.prototype.getElementById;
  const documentElementFromPoint = Document.prototype.elementFromPoint;
  const getComputedStyleFixed = globalThis.getComputedStyle;
  const htmlElementClick =
    typeof HTMLElement === "function" ? HTMLElement.prototype.click : null;
  const nativePromise = Promise;
  const promiseResolve = Promise.resolve;
  const promiseThen = Promise.prototype.then;
  const weakMapGet = WeakMap.prototype.get;
  const weakMapSet = WeakMap.prototype.set;

  const inputValueGetter =
    typeof HTMLInputElement === "function" ? getter(HTMLInputElement.prototype, "value") : null;
  const anchorHrefGetter =
    typeof HTMLAnchorElement === "function" ? getter(HTMLAnchorElement.prototype, "href") : null;
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
  const selectIndexSetter =
    typeof HTMLSelectElement === "function"
      ? setter(HTMLSelectElement.prototype, "selectedIndex")
      : null;
  const selectLabelsGetter =
    typeof HTMLSelectElement === "function" ? getter(HTMLSelectElement.prototype, "labels") : null;
  const optionIndexGetter =
    typeof HTMLOptionElement === "function" ? getter(HTMLOptionElement.prototype, "index") : null;
  const optionLabelGetter =
    typeof HTMLOptionElement === "function" ? getter(HTMLOptionElement.prototype, "label") : null;
  const optionSelectedGetter =
    typeof HTMLOptionElement === "function"
      ? getter(HTMLOptionElement.prototype, "selected")
      : null;
  const shadowHostGetter =
    typeof ShadowRoot === "function" ? getter(ShadowRoot.prototype, "host") : null;

  const nodeKeys = new WeakMap();
  const keyNodes = new Map();
  let nextNodeKey = 1;
  let busy = false;
  let lastObservationInvocation = 0;
  let lastObservationGeneration = 0;

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
    if (
      typeof encoded !== "string" || encoded.length === 0 ||
      utf8Length(encoded, MAX_REQUEST_BYTES + 1) > MAX_REQUEST_BYTES
    ) {
      return null;
    }

    let request;
    try {
      request = apply(jsonParse, JSON, [encoded]);
    } catch (_) {
      return null;
    }
    if (!isPlainObject(request)) return null;
    if (objectHasOwn(request, "o")) return parseActionRequest(request);
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

  function parseActionRequest(request) {
    if (!hasExactKeys(request, ["v", "o", "a", "i", "g", "t", "r", "k", "e", "p", "z", "f", "of"])) {
      return null;
    }
    if (
      request.v !== PROTOCOL_VERSION ||
      request.o !== "action_execute" ||
      !isPositiveSafeInteger(request.a) ||
      !isPositiveSafeInteger(request.i) ||
      !isPositiveSafeInteger(request.g) ||
      !isPositiveSafeInteger(request.t) ||
      typeof request.r !== "string" ||
      typeof request.k !== "string" ||
      !validRuntimeDescriptor(request.f) ||
      !numberIsSafeInteger(request.p) ||
      request.p < 0 ||
      request.p > MAX_SAFE_INTEGER
    ) {
      return null;
    }
    const roles = [
      "group", "document", "landmark", "heading", "paragraph", "link", "button",
      "textbox", "password", "searchbox", "checkbox", "radio", "combobox", "listbox",
      "option", "spinbutton", "slider", "tab", "menu_item", "dialog", "list",
      "list_item", "table", "row", "cell_header", "cell", "image", "progress", "status",
      "frame_boundary"
    ];
    if (!roles.includes(request.r) || !["click", "fill", "select", "press", "scroll"].includes(request.k)) {
      return null;
    }
    if ((request.k === "select") !== (request.p > 0)) return null;
    if (request.k === "select") {
      if (!validRuntimeDescriptor(request.of)) return null;
    } else if (request.of !== null) {
      return null;
    }
    if (request.k === "fill") {
      if (!validActionText(request.z)) return null;
    } else if (request.z !== null) {
      return null;
    }
    const expected = request.e;
    if (
      !hasExactKeys(expected, ["x", "y", "w", "h"]) ||
      !numberIsSafeInteger(expected.x) ||
      mathAbs(expected.x) > 1000000 ||
      !numberIsSafeInteger(expected.y) ||
      mathAbs(expected.y) > 1000000 ||
      !numberIsSafeInteger(expected.w) ||
      expected.w < 1 ||
      expected.w > 1000000 ||
      !numberIsSafeInteger(expected.h) ||
      expected.h < 1 ||
      expected.h > 1000000
    ) {
      return null;
    }
    return request;
  }

  function validActionText(value) {
    if (typeof value !== "string" || utf8Length(value, MAX_ACTION_TEXT_BYTES + 1) > MAX_ACTION_TEXT_BYTES) {
      return false;
    }
    for (const character of value) {
      const point = character.codePointAt(0);
      if (
        (point < 0x20 && point !== 0x09 && point !== 0x0a) ||
        (point >= 0x7f && point <= 0x9f) ||
        point === 0xad || point === 0x61c || point === 0x180e ||
        (point >= 0x200b && point <= 0x200f) ||
        (point >= 0x202a && point <= 0x202e) ||
        (point >= 0x2060 && point <= 0x2064) ||
        (point >= 0x2066 && point <= 0x206f) ||
        point === 0xfeff || (point >= 0xfff9 && point <= 0xfffb) ||
        point === 0xe0001 || (point >= 0xe0020 && point <= 0xe007f)
      ) {
        return false;
      }
    }
    return true;
  }

  function validRuntimeDescriptor(value) {
    if (!hasExactKeys(value, ["r", "o", "q", "s", "n", "vk", "vt", "vo", "vb"])) return false;
    if (
      !numberIsSafeInteger(value.r) || value.r < 1 || value.r > 30 ||
      !numberIsSafeInteger(value.o) || value.o < 0 || value.o > 31 ||
      !numberIsSafeInteger(value.q) || value.q < 1 || value.q > 3 ||
      !numberIsSafeInteger(value.s) || value.s < 0 || value.s > 127 ||
      (value.n !== null && (typeof value.n !== "string" || value.n.length > 2048)) ||
      !numberIsSafeInteger(value.vk) || value.vk < 0 || value.vk > 4 ||
      !numberIsSafeInteger(value.vo) || value.vo < 0 || value.vo > 65535 ||
      typeof value.vb !== "boolean"
    ) return false;
    if (value.vk === 1) {
      return typeof value.vt === "string" &&
        utf8Length(value.vt, MAX_VALUE_BYTES + 1) <= MAX_VALUE_BYTES &&
        value.vo === 0 && !value.vb;
    }
    if (value.vt !== null) return false;
    if (value.vk === 3) return value.vo === 0;
    if (value.vk === 4) return !value.vb;
    return value.vo === 0 && !value.vb;
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

  function exactValueText(raw, byteLimit) {
    if (typeof raw !== "string" || byteLimit <= 0) {
      return { value: "", bytes: 0, truncated: typeof raw === "string" && raw.length > 0 };
    }
    let value = "";
    let bytes = 0;
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
      if (
        (isForbiddenTextPoint(point) && point !== 0x09 && point !== 0x0a) ||
        point === 0x0d
      ) {
        truncated = true;
        break;
      }
      const characterBytes = point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4;
      if (bytes + characterBytes > byteLimit) {
        truncated = true;
        break;
      }
      value += character;
      bytes += characterBytes;
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

  function classifyLiveValue(value) {
    let bytes = 0;
    for (let index = 0; index < value.length; index += 1) {
      const first = apply(stringCharCodeAt, value, [index]);
      let point = first;
      if (first >= 0xd800 && first <= 0xdbff) {
        if (index + 1 >= value.length) return 0;
        const second = apply(stringCharCodeAt, value, [index + 1]);
        if (second < 0xdc00 || second > 0xdfff) return 0;
        point = 0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00);
        index += 1;
      } else if (first >= 0xdc00 && first <= 0xdfff) {
        return 0;
      }
      if (
        (isForbiddenTextPoint(point) && point !== 0x09 && point !== 0x0a) ||
        point === 0x0d
      ) {
        return 0;
      }
      bytes += point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4;
      if (bytes > MAX_VALUE_BYTES) return 1;
    }
    return 2;
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

  function resolveKeyAtGeneration(key, generation) {
    const entry = keyNodes.get(key);
    if (entry === undefined || entry.generation !== generation) return null;
    return resolveKey(key);
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

    if (descriptor.tag === "option" && optionLabelGetter !== null) {
      let label;
      try {
        label = read(optionLabelGetter, element);
      } catch (_) {
        label = null;
      }
      if (typeof label === "string" && label !== "") return label;
    }

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
    if (descriptor.role === "password") return 1;
    const metadata = [];
    const addMetadata = (raw, limit) => {
      if (raw === null || raw === "") return true;
      const bounded = boundedCredentialMetadata(raw, limit);
      if (bounded === null) return false;
      metadata.push(bounded);
      return true;
    };
    for (const [name, limit] of [
      ["autocomplete", 256], ["name", 256], ["id", 256],
      ["aria-label", MAX_NAME_BYTES], ["placeholder", MAX_NAME_BYTES],
      ["title", MAX_NAME_BYTES]
    ]) {
      let raw;
      try {
        raw = apply(getAttribute, element, [name]);
      } catch (_) {
        return 2;
      }
      if (raw !== null && typeof raw !== "string") return 2;
      if (!addMetadata(raw, limit)) return 2;
    }
    if (!addMetadata(accessibleName || "", MAX_NAME_BYTES)) return 2;

    let labelledBy;
    try {
      labelledBy = apply(getAttribute, element, ["aria-labelledby"]);
    } catch (_) {
      return 2;
    }
    if (labelledBy !== null) {
      if (typeof labelledBy !== "string" || !addMetadata(labelledBy, 1024)) return 2;
      const identifiers = apply(stringSplit, labelledBy, [/\s+/]).filter((value) => value !== "");
      if (identifiers.length > 8) return 2;
      for (const identifier of identifiers) {
        if (identifier.length > 128) return 2;
        const label = apply(documentGetElementById, document, [identifier]);
        if (label !== null) {
          const text = boundedCredentialLabelText(label);
          if (text === null || !addMetadata(text, MAX_NAME_BYTES)) return 2;
        }
      }
    }

    let labelsGetter = null;
    if (descriptor.tag === "input") labelsGetter = inputLabelsGetter;
    if (descriptor.tag === "textarea") labelsGetter = textareaLabelsGetter;
    if (descriptor.tag === "select") labelsGetter = selectLabelsGetter;
    if (labelsGetter !== null) {
      let labels;
      try {
        labels = read(labelsGetter, element);
      } catch (_) {
        return 2;
      }
      if (labels !== null && labels !== undefined) {
        const length = listLength(labels);
        if (length > 4) return 2;
        for (let index = 0; index < length; index += 1) {
          const label = listItem(labels, index);
          if (label === null) return 2;
          const text = boundedCredentialLabelText(label);
          if (text === null || !addMetadata(text, MAX_NAME_BYTES)) return 2;
        }
      }
    }

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
    ) ? 1 : 0;
  }

  function boundedCredentialMetadata(raw, limit) {
    if (typeof raw !== "string") return null;
    let bytes = 0;
    for (let index = 0; index < raw.length; index += 1) {
      const first = apply(stringCharCodeAt, raw, [index]);
      let point = first;
      if (first >= 0xd800 && first <= 0xdbff) {
        if (index + 1 >= raw.length) return null;
        const second = apply(stringCharCodeAt, raw, [index + 1]);
        if (second < 0xdc00 || second > 0xdfff) return null;
        point = 0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00);
        index += 1;
      } else if (first >= 0xdc00 && first <= 0xdfff) {
        return null;
      }
      if (
        (isForbiddenTextPoint(point) && point !== 0x09 && point !== 0x0a) ||
        point === 0x0d
      ) {
        return null;
      }
      bytes += point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4;
      if (bytes > limit) return null;
    }
    return raw;
  }

  function boundedCredentialLabelText(root) {
    const stack = [root];
    const chunks = [];
    let bytes = 0;
    let visited = 0;
    while (stack.length !== 0) {
      if (visited >= 128) return null;
      visited += 1;
      const current = stack.pop();
      const type = nodeType(current);
      if (type === 3) {
        const raw = read(characterDataGetter, current);
        if (typeof raw !== "string") return null;
        const separator = chunks.length === 0 || raw.length === 0 ? 0 : 1;
        if (bytes + separator > MAX_NAME_BYTES) return null;
        const bounded = boundedCredentialMetadata(raw, MAX_NAME_BYTES - bytes - separator);
        if (bounded === null) return null;
        if (bounded !== "") {
          if (separator !== 0) {
            chunks.push(" ");
            bytes += 1;
          }
          chunks.push(bounded);
          bytes += utf8Length(bounded, MAX_NAME_BYTES + 1);
        }
        continue;
      }
      if (type !== 1 && type !== 9 && type !== 11) return null;
      const children = read(nodeChildNodesGetter, current);
      if (children === null || children === undefined) return null;
      const length = listLength(children);
      if (length + stack.length + visited > 128) return null;
      for (let index = length - 1; index >= 0; index -= 1) {
        const child = listItem(children, index);
        if (child === null) return null;
        stack.push(child);
      }
    }
    return chunks.join("");
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

  function consumeValueField(raw, state) {
    const remaining = mathMax(0, state.request.b.t - state.textBytes);
    // Bound hostile-page work before trim/lower/split can allocate. Oversized
    // live values are conservatively redacted; only a complete <=4-KiB value
    // reaches secret classification and the exact verification projection.
    const valueClass = classifyLiveValue(raw);
    if (valueClass !== 2 || (raw !== "" && looksLikeSecret(raw))) {
      const redactedBytes = 10;
      if (redactedBytes > remaining) {
        mark(state, "text_limit");
        return { value: "", bytes: 0, secret: true };
      }
      state.textBytes += redactedBytes;
      return { value: "[redacted]", bytes: redactedBytes, secret: true };
    }
    const exact = exactValueText(raw, mathMin(MAX_VALUE_BYTES, remaining));
    state.textBytes += exact.bytes;
    if (exact.truncated || (raw.length !== 0 && remaining === 0)) mark(state, "text_limit");
    return { value: exact.value, bytes: exact.bytes, secret: false };
  }

  function setSensitivity(record, sensitivity) {
    if (sensitivity === "secret" || (sensitivity === "sensitive" && record.sensitivity === "public")) {
      record.sensitivity = sensitivity;
      record.wire.q = sensitivity;
      delete record.wire.u;
    }
  }

  function addName(record, raw, state) {
    const field = consumeField(raw, MAX_NAME_BYTES, state);
    if (field.value !== "") record.wire.n = field.value;
    record.sinkBytes = field.bytes;
    if (field.secret) setSensitivity(record, "secret");
  }

  function addValue(record, raw, state) {
    const field = consumeValueField(raw, state);
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

  function appendVisibleText(records, item, raw, state) {
    if (item.sink !== null) appendSink(records[item.sink], raw, state);
    if (item.nameAncestors === false) return;
    let proseSeen = item.sink !== null && records[item.sink].sink === "text";
    // A link/button/heading may contain semantic children (for example a
    // product heading and price paragraph). Their own text must not erase the
    // enclosing control's content-derived name. Walk only the already bounded
    // retained ancestry: no second DOM traversal, selector, or unbounded text
    // getter. The nearest prose sink also retains inline link labels in DOM
    // order, without flattening nested paragraphs/list items into outer prose.
    // Every copy remains charged to the same field/global text budget.
    for (let index = item.parent, depth = 0;
      index !== null && depth <= MAX_TREE_DEPTH && !state.stopped;
      index = records[index].wire.p === undefined ? null : records[index].wire.p, depth += 1) {
      const record = records[index];
      // Editable/credential and selectable values are not ancestor name text.
      // Preserve existing private-field classification across the new path.
      const role = record.wire.r;
      if (record.wire.v !== undefined || record.sensitivity !== "public" ||
        role === "textbox" || role === "password" || role === "searchbox" ||
        role === "spinbutton" || role === "combobox" || role === "listbox" || role === "option") break;
      if (index !== item.sink && record.sink === "name") appendSink(record, raw, state);
      if (!proseSeen && record.sink === "text") {
        proseSeen = true;
        if (index !== item.sink) appendSink(record, raw, state);
      }
    }
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
      if (descriptor.role === "link" && descriptor.tag === "a" && record.sensitivity === "public") {
        // Use the captured native getter, never an element-owned accessor or
        // a click. Exact URL bytes share the original text/wire ceilings.
        let destination;
        try { destination = read(anchorHrefGetter, element); } catch (_) { destination = undefined; }
        if (typeof destination === "string" && destination !== "" &&
            utf8Length(destination, 2049) <= 2048 &&
            (apply(stringStartsWith, destination, ["https://"]) || apply(stringStartsWith, destination, ["http://"])) &&
            !apply(stringIncludes, destination, ["?"]) && !apply(stringIncludes, destination, ["#"]) &&
            !apply(stringIncludes, destination, ["@"])) {
          const field = consumeField(destination, 2048, state);
          if (!field.secret && field.value === destination) wire.u = destination;
        }
      }

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
          setSensitivity(record, "secret");
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
    // Only metadata is queued during the ordinary viewport traversal. Deferred
    // headings consume spare output capacity afterward, never displacing a
    // visible control or prose record that appears later in document order.
    const headings = [];
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
          if (typeof raw === "string") appendVisibleText(records, item, raw, state);
        }
        continue;
      }
      if (type !== 1 && type !== 9 && type !== 11) continue;
      if (type === 1 && shouldSkipSubtree(item.node)) continue;

      const descriptor = classify(item.node);
      let parent = item.parent;
      let sink = item.sink;
      let disabled = item.disabled;
      let nameAncestors = item.nameAncestors !== false;
      if (type === 1) {
        const editable = attribute(item.node, "contenteditable", 16);
        if (editable !== null && lower(editable) !== "false") {
          // Generic editable containers need not produce a semantic record.
          // They still fence ancestor-name inheritance before their children.
          nameAncestors = false;
          sink = null;
        }
        disabled = disabledState(item.node, item.disabled);
        if (descriptor !== null) {
          const visibleStyle = styleIsVisible(item.node);
          const rect = visibleStyle ? elementRect(item.node) : null;
          const optionInExpansion = descriptor.role === "option" && anchored;
          // Native <option> elements do not have independent page geometry in
          // WebKit while their owning <select> is closed. Retain them only when
          // their nearest retained semantic parent is an already-admitted native
          // select. This exposes bounded, ref-addressable choices without making
          // arbitrary non-rendered DOM actionable or treating offscreen selects
          // as visible. A filtered select can leave the Document as its option's
          // nearest retained parent; brand-check before using an Element getter.
          const optionOfAdmittedSelect =
            descriptor.role === "option" &&
            descriptor.tag === "option" &&
            parent !== null &&
            records[parent] !== undefined &&
            nodeType(records[parent].element) === 1 &&
            tagName(records[parent].element) === "select";
          const visible =
            visibleStyle && (rect !== null || optionInExpansion || optionOfAdmittedSelect);
          const initialPriority =
            descriptor.role === "dialog" ||
            descriptor.role === "landmark" ||
            focused.has(item.node);
          const admitted = visible && (
            !anchored
              ? optionOfAdmittedSelect || (rect !== null && (initialPriority || inViewport(rect)))
              : true
          );
          if (!anchored && !admitted && visible && descriptor.role === "heading") {
            if (headings.length < 24) headings.push({ element: item.node, descriptor, parent, rect, disabled });
            else mark(state, "node_limit", false);
          }
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
            // An explicitly named inline link still has visible descendant
            // words belonging to its prose parent. Keep that existing sink;
            // never substitute its aria-label or overwrite its explicit name.
            sink = record.sink !== null ? index :
              descriptor.role === "link" && record.sensitivity === "public" &&
                sink !== null && records[sink].sink === "text" ? sink : null;
          } else {
            sink = null;
          }
        }
      }
      if (!state.stopped) {
        pushChildren(
          stack,
          item.node,
          { parent, sink, depth: item.depth + 1, disabled, nameAncestors },
          state
        );
      }
    }
    if (!anchored && !state.stopped) {
      const original = state.request;
      state.request = { ...original, b: { ...original.b, t: mathMin(original.b.t, state.textBytes + 2048) } };
      for (const heading of headings) {
        if (state.stopped || records.length >= original.b.n) {
          if (!state.stopped) mark(state, "node_limit", false);
          break;
        }
        // Reuse the ordinary name-inheritance fences (including editable and
        // credential descendants), not a textContent/flat-text shortcut.
        const scoped = traverse(heading.element, state, true);
        const record = scoped[0];
        if (record === undefined) continue;
        if (heading.parent !== null) record.wire.p = heading.parent;
        record.depth = heading.parent === null ? 0 : records[heading.parent].depth + 1;
        if (record.depth > MAX_TREE_DEPTH) { mark(state, "depth_limit"); break; }
        addRecord(records, record, state);
      }
      state.request = original;
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

  function actionFault(code) {
    return `E2:${code}`;
  }

  function actionOperationBit(kind) {
    if (kind === "click") return 1;
    if (kind === "fill") return 2;
    if (kind === "select") return 4;
    if (kind === "press") return 8;
    if (kind === "scroll") return 16;
    return 0;
  }

  function composedContains(target, candidate) {
    let current = candidate;
    for (let depth = 0; depth <= MAX_TREE_DEPTH && current !== null; depth += 1) {
      if (current === target) return true;
      let parent = read(nodeParentGetter, current);
      if (parent !== null && nodeType(parent) === 11 && shadowHostGetter !== null) {
        const host = read(shadowHostGetter, parent);
        if (host !== null && host !== undefined) parent = host;
      }
      current = parent;
    }
    try {
      return apply(nodeContains, target, [candidate]) === true;
    } catch (_) {
      return false;
    }
  }

  function boundedViewport() {
    const width = mathRound(Number(globalThis.innerWidth));
    const height = mathRound(Number(globalThis.innerHeight));
    if (
      !numberIsSafeInteger(width) ||
      !numberIsSafeInteger(height) ||
      width < 1 ||
      height < 1 ||
      width > 32768 ||
      height > 32768
    ) {
      return null;
    }
    return { width, height };
  }

  function actionPoint(target, rect, viewport) {
    const left = mathMax(0, rect.x);
    const top = mathMax(0, rect.y);
    const right = mathMin(viewport.width, rect.x + rect.width);
    const bottom = mathMin(viewport.height, rect.y + rect.height);
    if (right <= left || bottom <= top) return null;
    const insetX = mathMin(4, mathMax(1, (right - left) / 4));
    const insetY = mathMin(4, mathMax(1, (bottom - top) / 4));
    const candidates = [
      [(left + right) / 2, (top + bottom) / 2],
      [left + insetX, top + insetY],
      [right - insetX, top + insetY],
      [left + insetX, bottom - insetY],
      [right - insetX, bottom - insetY]
    ];
    for (const candidate of candidates) {
      const x = mathMax(0, mathMin(viewport.width - 1, mathRound(candidate[0])));
      const y = mathMax(0, mathMin(viewport.height - 1, mathRound(candidate[1])));
      let hit;
      try {
        hit = apply(documentElementFromPoint, document, [x, y]);
      } catch (_) {
        return null;
      }
      if (hit !== null && composedContains(target, hit)) return { x, y };
    }
    return null;
  }

  function geometryCompatible(expected, actual) {
    const xTolerance = mathMax(8, mathMin(64, mathRound(expected.w / 2)));
    const yTolerance = mathMax(8, mathMin(64, mathRound(expected.h / 2)));
    const widthTolerance = mathMax(4, mathRound(expected.w / 4));
    const heightTolerance = mathMax(4, mathRound(expected.h / 4));
    return (
      mathAbs(mathRound(actual.x) - expected.x) <= xTolerance &&
      mathAbs(mathRound(actual.y) - expected.y) <= yTolerance &&
      mathAbs(mathRound(actual.width) - expected.w) <= widthTolerance &&
      mathAbs(mathRound(actual.height) - expected.h) <= heightTolerance
    );
  }

  function selectDelta(target, option) {
    const descriptor = classify(option);
    if (descriptor === null || descriptor.role !== "option") return null;
    let owner = read(nodeParentGetter, option);
    for (let depth = 0; depth <= 2 && owner !== null && owner !== target; depth += 1) {
      owner = read(nodeParentGetter, owner);
    }
    if (owner !== target) return null;
    let selected;
    let desired;
    try {
      selected = read(selectIndexGetter, target);
      desired = read(optionIndexGetter, option);
    } catch (_) {
      return null;
    }
    if (
      !numberIsSafeInteger(selected) ||
      !numberIsSafeInteger(desired) ||
      selected < -1 ||
      desired < 0 ||
      desired > 65535
    ) {
      return null;
    }
    const delta = desired - mathMax(0, selected);
    return mathAbs(delta) <= 65535 ? delta : null;
  }

  function descriptorRoleCode(role) {
    const roles = {
      group: 1, document: 2, landmark: 3, heading: 4, paragraph: 5, link: 6,
      button: 7, textbox: 8, password: 9, searchbox: 10, checkbox: 11, radio: 12,
      combobox: 13, listbox: 14, option: 15, spinbutton: 16, slider: 17, tab: 18,
      menu_item: 19, dialog: 20, list: 21, list_item: 22, table: 23, row: 24,
      cell_header: 25, cell: 26, image: 27, progress: 28, status: 29, frame_boundary: 30
    };
    return objectHasOwn(roles, role) ? roles[role] : 0;
  }

  function descriptorSensitivityCode(sensitivity) {
    if (sensitivity === "public" || sensitivity === undefined) return 1;
    if (sensitivity === "sensitive") return 2;
    if (sensitivity === "secret") return 3;
    return 0;
  }

  function descriptorValue(value) {
    if (value === undefined) return { kind: 0, text: null, ordinal: 0, boolean: false };
    if (!isPlainObject(value) || typeof value.k !== "string") return null;
    if (value.k === "text" && typeof value.value === "string") {
      return { kind: 1, text: value.value, ordinal: 0, boolean: false };
    }
    if (value.k === "redacted") return { kind: 2, text: null, ordinal: 0, boolean: false };
    if (value.k === "boolean" && typeof value.value === "boolean") {
      return { kind: 3, text: null, ordinal: 0, boolean: value.value };
    }
    if (value.k === "ordinal" && numberIsSafeInteger(value.value) && value.value >= 0 && value.value <= 65535) {
      return { kind: 4, text: null, ordinal: value.value, boolean: false };
    }
    return null;
  }

  function runtimeDescriptor(element, generation) {
    const state = {
      request: {
        g: generation,
        b: {
          n: MAX_ACTION_DESCRIPTOR_NODES,
          t: MAX_ACTION_DESCRIPTOR_TEXT_BYTES,
          w: MAX_ACTION_DESCRIPTOR_WIRE_BYTES,
          x: MAX_ACTION_DESCRIPTOR_VISITED_NODES,
          geo: true
        }
      },
      visited: 0,
      textBytes: 0,
      completeness: "complete",
      stopped: false
    };
    const records = traverse(element, state, true);
    if (records.length === 0 || records[0].element !== element || state.completeness !== "complete") {
      return null;
    }
    const wire = records[0].wire;
    const value = descriptorValue(wire.v);
    const role = descriptorRoleCode(wire.r);
    const sensitivity = descriptorSensitivityCode(wire.q);
    if (value === null || role === 0 || sensitivity === 0) return null;
    return {
      r: role,
      o: wire.o || 0,
      q: sensitivity,
      s: wire.s || 0,
      n: wire.n === undefined ? null : wire.n,
      vk: value.kind,
      vt: value.text,
      vo: value.ordinal,
      vb: value.boolean
    };
  }

  function descriptorMatches(expected, actual) {
    if (!validRuntimeDescriptor(expected) || !validRuntimeDescriptor(actual)) return false;
    return (
      expected.r === actual.r && expected.o === actual.o && expected.q === actual.q &&
      expected.s === actual.s && expected.n === actual.n && expected.vk === actual.vk &&
      expected.vt === actual.vt && expected.vo === actual.vo && expected.vb === actual.vb
    );
  }

  function descriptorMatchesFilledValue(expected, actual, value) {
    if (!validRuntimeDescriptor(expected) || !validRuntimeDescriptor(actual)) return false;
    const valueMatches = value === ""
      ? actual.vk === 0 && actual.vt === null && actual.vo === 0 && !actual.vb
      : actual.vk === 1 && actual.vt === value && actual.vo === 0 && !actual.vb;
    return (
      expected.r === actual.r && expected.o === actual.o && expected.q === actual.q &&
      expected.s === actual.s && expected.n === actual.n && valueMatches
    );
  }

  function descriptorMatchesSelectedValue(expected, actual, desired) {
    if (!validRuntimeDescriptor(expected) || !validRuntimeDescriptor(actual)) return false;
    return (
      expected.r === actual.r && expected.o === actual.o && expected.q === actual.q &&
      expected.s === actual.s && expected.n === actual.n &&
      actual.vk === 4 && actual.vt === null && actual.vo === desired && !actual.vb
    );
  }

  function descriptorMatchesSelectedOption(expected, actual) {
    if (!validRuntimeDescriptor(expected) || !validRuntimeDescriptor(actual)) return false;
    const selectedBit = 2;
    return (
      expected.r === actual.r && expected.o === actual.o && expected.q === actual.q &&
      (expected.s & ~selectedBit) === (actual.s & ~selectedBit) &&
      (actual.s & selectedBit) !== 0 && expected.n === actual.n &&
      expected.vk === actual.vk && expected.vt === actual.vt &&
      expected.vo === actual.vo && expected.vb === actual.vb
    );
  }

  function fillControlKind(descriptor, value) {
    if (descriptor.tag === "input") {
      if (
        (descriptor.inputType !== "text" && descriptor.inputType !== "search") ||
        (descriptor.role !== "textbox" && descriptor.role !== "searchbox")
      ) {
        return 0;
      }
      for (const character of value) {
        const point = character.codePointAt(0);
        if (point === 0x09 || point === 0x0a || point === 0x0d) return 0;
      }
      return 1;
    }
    if (descriptor.tag === "textarea" && descriptor.role === "textbox") {
      return 2;
    }
    return 0;
  }

  function pageRelayAttribute(target, name) {
    try {
      return apply(getAttribute, target, [name]);
    } catch (_) {
      return null;
    }
  }

  function clearPageRelayAttributes(target) {
    try {
      apply(removeAttribute, target, [PAGE_RELAY_COMMAND]);
      apply(removeAttribute, target, [PAGE_RELAY_TERMINAL]);
    } catch (_) {
      // A replaced document or target is handled as closed transport failure.
    }
  }

  function runPageRelayFill(target, descriptor, value, attempt) {
    const projected = exactValueText(value, MAX_VALUE_BYTES);
    if (
      projected.truncated || projected.value !== value ||
      utf8Length(value, MAX_VALUE_BYTES + 1) > MAX_VALUE_BYTES
    ) {
      return "unsupported_interaction";
    }
    const control = fillControlKind(descriptor, value);
    if (control === 0) {
      return "unsupported_interaction";
    }

    let root;
    let command;
    try {
      root = read(documentElementGetter, document);
      if (root === null || pageRelayAttribute(root, PAGE_RELAY_READY) !== "1") {
        return "page_relay_not_ready";
      }
      if (pageRelayAttribute(target, PAGE_RELAY_COMMAND) !== null) {
        clearPageRelayAttributes(target);
        return "stale_reference";
      }
      apply(removeAttribute, target, [PAGE_RELAY_TERMINAL]);
      command = apply(jsonStringify, JSON, [{ v: 1, a: attempt, z: value }]);
      if (
        typeof command !== "string" ||
        utf8Length(command, MAX_PAGE_RELAY_COMMAND_BYTES + 1) > MAX_PAGE_RELAY_COMMAND_BYTES
      ) {
        return "unsupported_interaction";
      }
      apply(setAttribute, target, [PAGE_RELAY_COMMAND, command]);
    } catch (_) {
      return "unsupported_interaction";
    }

    const checkpoint = apply(promiseResolve, nativePromise, []);
    try {
      return apply(promiseThen, checkpoint, [() => {
        const terminal = pageRelayAttribute(target, PAGE_RELAY_TERMINAL);
        clearPageRelayAttributes(target);
        if (terminal === `1|${attempt}|ok`) return "ok";
        if (terminal === `1|${attempt}|refused-command`) return "invalid_request";
        if (terminal === `1|${attempt}|refused-identity`) return "stale_reference";
        if (terminal === `1|${attempt}|refused-type`) return "unsupported_interaction";
        if (terminal === `1|${attempt}|refused-state`) return "target_disabled";
        if (terminal === `1|${attempt}|refused-credential`) return "credential_boundary";
        if (terminal === `1|${attempt}|refused-construct`) return "unsupported_interaction";
        if (terminal === `1|${attempt}|indeterminate`) return "applied_unverified";
        if (terminal === `1|0|duplicate`) return "target_occluded";
        if (terminal === `1|0|invalid`) return "internal";
        // The page-world recipe may already have called the native setter.
        // This must never be surfaced as a clean retryable refusal.
        return "applied_unverified";
      }]);
    } catch (_) {
      clearPageRelayAttributes(target);
      return "applied_unverified";
    }
  }

  function runFixedSelect(target, desired) {
    if (selectIndexGetter === null || selectIndexSetter === null) {
      return "unsupported_interaction";
    }
    try {
      write(selectIndexSetter, target, desired);
    } catch (_) {
      return "applied_unverified";
    }
    try {
      return read(selectIndexGetter, target) === desired ? "ok" : "applied_unverified";
    } catch (_) {
      return "applied_unverified";
    }
  }

  function encodeActionEvidence(request, backend, readiness, geometry, viewport, point, delta) {
    return apply(jsonStringify, JSON, [{
      v: PROTOCOL_VERSION,
      a: request.a,
      i: request.i,
      g: request.g,
      r: readiness,
      x: geometry.x,
      y: geometry.y,
      w: geometry.w,
      h: geometry.h,
      vw: viewport.width,
      vh: viewport.height,
      px: point.x,
      py: point.y,
      d: delta,
      b: backend
    }]);
  }

  function runAction(request) {
    let readyState;
    try {
      readyState = read(documentReadyStateGetter, document);
    } catch (_) {
      return actionFault("document_loading");
    }
    if (readyState !== "interactive" && readyState !== "complete") {
      return actionFault("document_loading");
    }
    if (
      lastObservationInvocation !== request.i ||
      lastObservationGeneration !== request.g
    ) {
      return actionFault("target_changed");
    }
    sweepIdentities(request.g);
    const target = resolveKeyAtGeneration(request.t, request.g);
    if (target === null || nodeType(target) !== 1) return actionFault("stale_reference");
    const descriptor = classify(target);
    if (descriptor === null) return actionFault("target_changed");
    const credential = credentialField(target, descriptor, attribute(target, "aria-label", 512) || "");
    if (credential || descriptor.role === "password") return actionFault("credential_boundary");
    if (descriptor.role !== request.r) return actionFault("target_changed");
    const disabled = disabledState(target, false);
    const readonly = has(target, "readonly") || lower(attribute(target, "aria-readonly", 16) || "") === "true";
    if (disabled || ((request.k === "fill" || request.k === "select") && readonly)) {
      return actionFault("target_disabled");
    }
    const required = actionOperationBit(request.k);
    if (required === 0 || (operationBits(descriptor, disabled, readonly) & required) === 0) {
      return actionFault("unsupported_interaction");
    }
    const targetDescriptor = runtimeDescriptor(target, request.g);
    if (!descriptorMatches(request.f, targetDescriptor)) {
      return actionFault("target_changed");
    }
    let delta = 0;
    let selectedOption = null;
    if (request.k === "select") {
      const option = resolveKeyAtGeneration(request.p, request.g);
      if (option === null) return actionFault("stale_reference");
      selectedOption = option;
      const optionDescriptor = runtimeDescriptor(option, request.g);
      if (!descriptorMatches(request.of, optionDescriptor)) {
        return actionFault("target_changed");
      }
      const computed = selectDelta(target, option);
      if (computed === null) return actionFault("target_changed");
      delta = computed;
    }

    const finalTargetDescriptor = runtimeDescriptor(target, request.g);
    const finalOptionDescriptor = selectedOption === null
      ? null
      : runtimeDescriptor(selectedOption, request.g);
    if (
      !descriptorMatches(request.f, finalTargetDescriptor) ||
      (selectedOption !== null && !descriptorMatches(request.of, finalOptionDescriptor)) ||
      (selectedOption !== null && resolveKeyAtGeneration(request.p, request.g) !== selectedOption) ||
      resolveKeyAtGeneration(request.t, request.g) !== target
    ) {
      return actionFault("target_changed");
    }
    if (!styleIsVisible(target)) return actionFault("target_occluded");
    const rect = elementRect(target);
    if (rect === null) return actionFault("target_occluded");
    if (!geometryCompatible(request.e, rect)) return actionFault("target_changed");
    const viewport = boundedViewport();
    if (viewport === null) return actionFault("internal");

    let point;
    let readiness;
    if (request.k === "scroll") {
      point = { x: mathRound(viewport.width / 2), y: mathRound(viewport.height / 2) };
      readiness = "scroll";
    } else {
      point = actionPoint(target, rect, viewport);
      if (point === null) return actionFault("target_occluded");
      readiness = "visible";
    }
    const geometry = wireRect(rect);
    if (request.k === "click") {
      if (typeof htmlElementClick !== "function") return actionFault("unsupported_interaction");
      try {
        apply(htmlElementClick, target, []);
      } catch (_) {
        return actionFault("unsupported_interaction");
      }
      return encodeActionEvidence(request, "fixed_semantic_recipe", readiness, geometry, viewport, point, delta);
    } else if (request.k === "fill") {
      const finishFill = (result) => {
        if (result !== "ok") {
          return actionFault(result);
        }
        const finalTarget = resolveKeyAtGeneration(request.t, request.g);
        const finalDescriptor = finalTarget === target ? classify(target) : null;
        // The page-world setter has run once an `ok` terminal is observed.
        // Every later mismatch is indeterminate, never a retryable target fault.
        if (finalDescriptor === null || finalDescriptor.role !== request.r) {
          return actionFault("applied_unverified");
        }
        const finalDisabled = disabledState(target, false);
        const finalReadonly = has(target, "readonly") || lower(attribute(target, "aria-readonly", 16) || "") === "true";
        if (finalDisabled || finalReadonly) return actionFault("applied_unverified");
        if (
          credentialField(target, finalDescriptor, attribute(target, "aria-label", 512) || "") ||
          finalDescriptor.role === "password" || fillControlKind(finalDescriptor, request.z) === 0
        ) {
          return actionFault("applied_unverified");
        }
        const filledDescriptor = runtimeDescriptor(target, request.g);
        if (!descriptorMatchesFilledValue(request.f, filledDescriptor, request.z)) {
          return actionFault("applied_unverified");
        }
        return encodeActionEvidence(
          request,
          "page_world_compatibility_fill",
          "form",
          geometry,
          viewport,
          point,
          delta
        );
      };
      const result = runPageRelayFill(target, descriptor, request.z, request.a);
      if (typeof result === "string") return finishFill(result);
      try {
        return apply(promiseThen, result, [finishFill, () => actionFault("applied_unverified")]);
      } catch (_) {
        return actionFault("applied_unverified");
      }
    } else if (request.k === "select") {
      const desired = read(optionIndexGetter, selectedOption);
      const result = runFixedSelect(target, desired);
      if (result !== "ok") return actionFault(result);
      const finalTarget = resolveKeyAtGeneration(request.t, request.g);
      const finalOption = resolveKeyAtGeneration(request.p, request.g);
      const finalTargetDescriptor = finalTarget === target ? runtimeDescriptor(target, request.g) : null;
      const finalOptionDescriptor = finalOption === selectedOption
        ? runtimeDescriptor(selectedOption, request.g)
        : null;
      if (
        !descriptorMatchesSelectedValue(request.f, finalTargetDescriptor, desired) ||
        !descriptorMatchesSelectedOption(request.of, finalOptionDescriptor) ||
        selectDelta(target, selectedOption) !== 0
      ) {
        return actionFault("applied_unverified");
      }
      return encodeActionEvidence(
        request,
        "fixed_semantic_recipe",
        readiness,
        geometry,
        viewport,
        point,
        delta
      );
    } else {
      return actionFault("unsupported_interaction");
    }
  }

  function run(request) {
    let readyState;
    try {
      readyState = read(documentReadyStateGetter, document);
    } catch (_) {
      return fault("document_loading");
    }
    if (readyState !== "interactive" && readyState !== "complete") {
      return fault("document_loading");
    }
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
    if (request.o === "action_execute") {
      let result;
      try {
        result = runAction(request);
      } catch (_) {
        busy = false;
        return actionFault("internal");
      }
      if (typeof result === "string") {
        busy = false;
        return result;
      }
      try {
        return apply(promiseThen, result, [
          (value) => {
            busy = false;
            return typeof value === "string" ? value : actionFault("internal");
          },
          () => {
            busy = false;
            return actionFault("internal");
          }
        ]);
      } catch (_) {
        busy = false;
        return actionFault("internal");
      }
    }
    try {
      const result = run(request);
      if (typeof result === "string" && !result.startsWith("E1:")) {
        lastObservationInvocation = request.i;
        lastObservationGeneration = request.g;
      }
      return result;
    } catch (error) {
      if (error === IDENTITY_EXHAUSTED) return fault("identity_exhausted");
      return fault("internal");
    } finally {
      busy = false;
    }
  }

  async function serveNativeInvocations(channel, post) {
    for (let completed = 0; completed < MAX_DOCUMENT_INVOCATIONS; completed += 1) {
      let encoded;
      try {
        encoded = await apply(post, channel, [CHANNEL_PULL]);
      } catch (_) {
        return;
      }
      if (encoded === CHANNEL_STOP) return;

      const invoked = invoke(encoded);
      const result = typeof invoked === "string" ? invoked : await invoked;
      let acknowledgement;
      try {
        acknowledgement = await apply(post, channel, [`${CHANNEL_RESULT_PREFIX}${result}`]);
      } catch (_) {
        return;
      }
      if (acknowledgement !== CHANNEL_ACK) return;
    }

    try {
      await apply(post, channel, [CHANNEL_EXHAUSTED]);
    } catch (_) {
      // Native teardown or document replacement rejects the final notice.
    }
  }

  function startNativeTransport() {
    let channel;
    let post;
    try {
      const webkit = globalThis.webkit;
      const handlers = webkit && webkit.messageHandlers;
      channel = handlers && handlers.zephiumSemanticRuntimeV1;
      post = channel && channel.postMessage;
    } catch (_) {
      return;
    }
    if (typeof post !== "function") return;
    void serveNativeInvocations(channel, post);
  }

  objectFreeze(invoke);
  const api = objectFreeze({ invoke });
  objectDefineProperty(globalThis, GLOBAL_NAME, {
    value: api,
    writable: false,
    configurable: false,
    enumerable: false
  });
  startNativeTransport();
})();
