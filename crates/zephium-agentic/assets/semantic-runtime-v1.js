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
  const MAX_DIALOG_SAMPLE_NODES = 16384;
  const MAX_DIALOG_SAMPLE_DIALOGS = 16;
  const CHANNEL_PULL = "P1";
  const CHANNEL_RESULT_PREFIX = "R1:";
  const CHANNEL_ACK = "A1";
  const CHANNEL_STOP = "S1";
  const CHANNEL_PARK = "K1";
  const CHANNEL_PARKED = "K1:A";
  const CHANNEL_EXHAUSTED = "X1";

  const objectDefineProperty = Object.defineProperty;
  const objectFreeze = Object.freeze;
  const objectGetOwnPropertyDescriptor = Object.getOwnPropertyDescriptor;
  const objectKeys = Object.keys;
  const objectHasOwn = Function.call.bind(Object.prototype.hasOwnProperty);
  const arrayIsArray = Array.isArray;
  const jsonParse = JSON.parse;
  const jsonStringify = JSON.stringify;
  const reflectApply = Reflect.apply;
  const eventTargetAddEventListener = EventTarget.prototype.addEventListener;
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
  const nodeGetRoot = Node.prototype.getRootNode;
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
  const nativeInputEvent = globalThis.InputEvent;
  const fixedDispatchEvent = EventTarget.prototype.dispatchEvent;
  const inputValueSetter = setter(HTMLInputElement.prototype, "value");
  const textareaValueSetter = setter(HTMLTextAreaElement.prototype, "value");
  const nodeTextSetter = setter(Node.prototype, "textContent");
  const editableGetter = getter(HTMLElement.prototype, "isContentEditable");
  const promiseResolve = Promise.resolve;
  const promiseThen = Promise.prototype.then;
  const weakMapGet = WeakMap.prototype.get;
  const weakMapSet = WeakMap.prototype.set;

  const inputValueGetter =
    typeof HTMLInputElement === "function" ? getter(HTMLInputElement.prototype, "value") : null;
  const anchorHrefGetter =
    typeof HTMLAnchorElement === "function" ? getter(HTMLAnchorElement.prototype, "href") : null;
  const imageCurrentSrcGetter = typeof HTMLImageElement === "function" ? getter(HTMLImageElement.prototype, "currentSrc") : null;
  const imageSrcGetter = typeof HTMLImageElement === "function" ? getter(HTMLImageElement.prototype, "src") : null;
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

  let nodeKeys = new WeakMap();
  const keyNodes = new Map();
  let nextNodeKey = 1;
  let busy = false;
  let lastObservationInvocation = 0;
  let lastObservationGeneration = 0;
  // Only a requested UI-transition action pays for these bounded samples.
  // Weak identities do not retain DOM nodes or enter the model projection.
  let dialogKeys = new WeakMap();
  let nextDialogKey = 1;
  let pendingDialogSample = null;
  let transportActive = false;

  function clearDocumentState() {
    nodeKeys = new WeakMap();
    keyNodes.clear();
    nextNodeKey = 1;
    busy = false;
    lastObservationInvocation = 0;
    lastObservationGeneration = 0;
    dialogKeys = new WeakMap();
    nextDialogKey = 1;
    pendingDialogSample = null;
  }

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
    if (!hasExactKeys(budget, budget.lu === true ? ["n", "t", "w", "x", "geo", "lu"] : ["n", "t", "w", "x", "geo"])) return null;
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
      typeof budget.geo !== "boolean" ||
      (budget.lu !== undefined && budget.lu !== true)
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
    } else if (scope.k === "text_search") {
      if (!hasExactKeys(scope, ["k", "a", "q"]) || !isPositiveSafeInteger(scope.a) ||
          typeof scope.q !== "string" || utf8Length(scope.q, 257) > 256 ||
          /[\u0000-\u001f\u007f-\u009f]/u.test(scope.q) || apply(stringTrim, scope.q, []) === "") return null;
      for (const character of scope.q) if (isForbiddenTextPoint(character.codePointAt(0))) return null;
      if (looksLikeSecret(scope.q)) return null;
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
    const actionKeys = ["v", "o", "a", "i", "g", "t", "r", "k", "e", "p", "z", "f", "of"];
    if (objectHasOwn(request, "u")) actionKeys.push("u");
    if (!hasExactKeys(request, actionKeys) ||
        (objectHasOwn(request, "u") && (request.u !== true || request.k !== "click"))) {
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

  function normalizeText(raw, byteLimit, scanBytesLimit = Infinity) {
    if (typeof raw !== "string" || byteLimit <= 0) {
      return { value: "", bytes: 0, scannedBytes: 0, truncated: typeof raw === "string" && raw.length > 0 };
    }
    let value = "";
    let bytes = 0;
    let pendingSpace = false;
    let inspected = 0;
    let scannedBytes = 0;
    let truncated = false;
    const scanLimit = byteLimit * 8 + 256;
    for (const character of raw) {
      inspected += 1;
      if (inspected > scanLimit) {
        truncated = true;
        break;
      }
      const point = character.codePointAt(0);
      const characterBytes = point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4;
      if (scannedBytes + characterBytes > scanBytesLimit) { truncated = true; break; }
      scannedBytes += characterBytes;
      if (isWhitespace(point)) {
        if (value.length !== 0) pendingSpace = true;
        continue;
      }
      if (isForbiddenTextPoint(point)) continue;
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
    return { value, bytes, scannedBytes, truncated };
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

  function attribute(element, name, limit = 1024, state = null) {
    const value = apply(getAttribute, element, [name]);
    if (typeof value !== "string") return null;
    if (state !== null && value.length > limit) mark(state, "field_limit", false);
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
    if (completeness === "field_limit") state.fieldTruncations = (state.fieldTruncations || 0) + 1;
    if (state.completeness === "complete" || completeness === "text_limit" ||
        (state.completeness === "field_limit" && completeness !== "field_limit")) {
      state.completeness = completeness;
    }
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
      if (objectHasOwn(ariaRoles, token)) return { role: ariaRoles[token], landmark: ariaRoles[token] === "landmark" ? token : undefined };
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

    const editableAttribute = attribute(node, "contenteditable", 16);
    const editable = editableAttribute !== null &&
      ["", "true", "plaintext-only"].includes(lower(editableAttribute));
    const editableStructure = editable ? [] : null;
    const editableWitness = editable ? { context: null, empty: true } : null;
    const editableSupport = editable ? editableHostSupport(node, editableStructure, editableWitness) : 2;
    const plainTextEditable = editableSupport === 1;
    const explicit = explicitRole(node);
    if (explicit !== null) {
      if (explicit.suppressed === true) return null;
      return { role: explicit.role, landmark: explicit.landmark, tag, inputType, contentEditable: editable, plainTextEditable, editableSupport, editableStructure, editableEmpty: editableWitness && editableWitness.empty, editingContext: editableWitness && editableWitness.context };
    }

    if (editable) return { role: "textbox", tag, inputType, contentEditable: true, plainTextEditable, editableSupport, editableStructure, editableEmpty: editableWitness.empty, editingContext: editableWitness.context };

    if (tag === "html" || tag === "body" || tag === "div" || tag === "fieldset" || tag === "details") {
      return tag === "fieldset" || tag === "details"
        ? { role: "group", tag, inputType }
        : null;
    }
    if (tag === "article") return { role: "document", tag, inputType };
    if (tag === "main" || tag === "nav" || tag === "header" || tag === "footer" || tag === "aside") {
      const landmark = { main: "main", nav: "navigation", header: "banner", footer: "contentinfo", aside: "complementary" }[tag];
      return { role: "landmark", landmark, tag, inputType };
    }
    if ((tag === "section" || tag === "form") && hasAuthorName(node)) {
      return { role: "landmark", landmark: tag === "form" ? "form" : "region", tag, inputType };
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
      case "searchbox":
        return readonly || fillControlKind(descriptor, "") === 0 ? 1 | 8 : 1 | 2 | 8;
      case "password":
      case "spinbutton":
        return readonly ? 1 | 8 : 1 | 2 | 8;
      case "combobox":
        // Role alone grants neither Fill nor Select.
        return readonly ? 1 | 8 : descriptor.tag === "select" ? 1 | 4 | 8 :
          fillControlKind(descriptor, "") !== 0 ? 1 | 2 | 8 : 1 | 8;
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
      // Later queued siblings must not be joined to an earlier source across
      // children that were never inspected (which may contain a negation).
      // Retain the proven prefix and end this traversal at the first gap.
      mark(state, "inspection_limit");
      return;
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
            if (clipped.truncated) mark(state, "field_limit", false);
          } else mark(state, "field_limit", false);
        }
        if (normalized.truncated) mark(state, "field_limit", false);
        continue;
      }
      if (type === 1 && shouldSkipSubtree(current)) continue;
      if (type === 1 || type === 9 || type === 11) pushChildren(stack, current, {}, state);
    }
    if (stack.length !== 0 && bytes >= limit) mark(state, "field_limit", false);
    return chunks.join("");
  }

  function labelledText(element, descriptor, state) {
    const labelledBy = attribute(element, "aria-labelledby", 1024, state);
    if (labelledBy !== null) {
      const identifiers = apply(stringSplit, labelledBy, [/\s+/]);
      if (identifiers.length > 8) mark(state, "field_limit", false);
      const labels = [];
      for (let index = 0; index < identifiers.length && index < 8 && !state.stopped; index += 1) {
        const identifier = identifiers[index];
        if (identifier.length === 0) continue;
        if (identifier.length > 128) { mark(state, "field_limit", false); continue; }
        const target = apply(documentGetElementById, document, [identifier]);
        if (target !== null) {
          const text = flatText(target, state, MAX_NAME_BYTES);
          if (text !== "") labels.push(text);
        }
      }
      if (labels.length !== 0) return labels.join(" ");
    }

    const ariaLabel = attribute(element, "aria-label", MAX_NAME_BYTES * 4, state);
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
        if (listLength(labels) > 4) mark(state, "field_limit", false);
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
      const alt = attribute(element, "alt", MAX_NAME_BYTES * 4, state);
      if (alt !== null && alt !== "") return alt;
    }
    if (
      descriptor.tag === "input" &&
      ["button", "submit", "reset"].includes(descriptor.inputType)
    ) {
      const value = attribute(element, "value", MAX_NAME_BYTES * 4, state);
      if (value !== null && value !== "") return value;
    }
    const placeholder = attribute(element, "placeholder", MAX_NAME_BYTES * 4, state);
    if (placeholder !== null && placeholder !== "") return placeholder;
    const title = attribute(element, "title", MAX_NAME_BYTES * 4, state);
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

  function consumeField(raw, fieldLimit, state, reserved = 0) {
    const remaining = mathMax(0, state.request.b.t - state.textBytes - reserved);
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
    if (normalized.truncated || (raw.length !== 0 && remaining === 0)) {
      // Field clipping preserves siblings; global exhaustion stops traversal.
      const aggregate = remaining <= fieldLimit;
      mark(state, aggregate ? "text_limit" : "field_limit", aggregate);
    }
    return { value, bytes, secret, truncated: normalized.truncated };
  }

  function consumeValueField(raw, state, fieldLimit = MAX_VALUE_BYTES) {
    const remaining = mathMax(0, state.request.b.t - state.textBytes);
    // Bound allocation; only complete <=4-KiB values reach classification.
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
    const exact = exactValueText(raw, mathMin(fieldLimit, remaining));
    state.textBytes += exact.bytes;
    if (exact.truncated || (raw.length !== 0 && remaining === 0)) {
      const aggregate = remaining <= fieldLimit;
      mark(state, aggregate ? "text_limit" : "field_limit", aggregate);
    }
    return { value: exact.value, bytes: exact.bytes, secret: false, truncated: exact.truncated };
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
    // Preserve proven public emptiness, never infer it from missing/clipped data.
    const observedEmpty = raw === "" && record.sensitivity === "public" && (record.wire.o & 2) !== 0;
    if (field.value !== "" || observedEmpty) record.wire.v = { k: field.secret ? "redacted" : "text", value: field.value };
    if (field.secret) {
      record.wire.v = { k: "redacted" };
      setSensitivity(record, "secret");
    }
  }

  function appendSink(record, raw, state) {
    if (record.sink === null || record.saturated || state.stopped || record.sensitivity === "secret") return;
    let current = "";
    if (record.sink === "name") current = record.wire.n || "";
    if (record.sink === "text") current = record.wire.t || "";
    if (record.sink === "value" && record.wire.v && record.wire.v.k === "text") {
      current = record.wire.v.value;
    }
    if (current === "[redacted]") return;
    const fieldLimit = record.sink === "name" ? MAX_NAME_BYTES : record.sink === "value" ? MAX_VALUE_BYTES : MAX_NODE_TEXT_BYTES;
    // Editable values preserve exact whitespace and inline adjacency.
    const separator = current === "" || record.sink === "value" ? "" : " ";
    const remainingField = mathMax(0, fieldLimit - record.sinkBytes - (separator === "" ? 0 : 1));
    const separatorBytes = separator === "" ? 0 : 1;
    const field = record.sink === "value"
      ? consumeValueField(raw, state, remainingField)
      : consumeField(raw, remainingField, state, separatorBytes);
    if (field.truncated) {
      record.saturated = true;
      record.wire.fc = false;
    }
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
    // Preserve control names and nearest prose across inline semantic children.
    // Walk bounded retained ancestry, charging copies to existing text limits.
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
    if (descriptor.contentEditable === true && textFillRole(descriptor.role)) return "value";
    if (
      descriptor.role === "paragraph" ||
      (descriptor.role === "document" && descriptor.tag === "article") ||
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
    const beforeFieldTruncations = state.fieldTruncations || 0;
    const wire = { k: keyFor(element, state.request.g), fc: true };
    if (state.recordEditingWitness === true) {
      keyNodes.get(wire.k).editingContext = descriptor.contentEditable === true && descriptor.plainTextEditable === true
        ? descriptor.editingContext || null : undefined;
    }
    if (parent !== null) wire.p = parent;
    wire.r = descriptor.role;
    if (descriptor.role === "landmark" && descriptor.landmark !== undefined) wire.lm = descriptor.landmark;
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
    // Closed host diagnostics, omitted from model context.
    if (textFillRole(descriptor.role)) {
      wire.fs = disabled ? 11 : readonly ? 10 :
        fillControlKind(descriptor, "") !== 0 ? 1 :
        descriptor.tag === "input" || descriptor.tag === "textarea" ? 12 :
        descriptor.editableSupport || 2;
      if (descriptor.editableStructure !== null && descriptor.editableStructure !== undefined &&
          descriptor.editableStructure.length === 3) wire.es = descriptor.editableStructure;
    }
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
      if (record.sink === "value") record.sinkBytes = 0;
      const imageSource = descriptor.role === "image" && descriptor.tag === "img";
      if (((descriptor.role === "link" && descriptor.tag === "a") || imageSource) && record.sensitivity === "public") {
        let destination;
        try {
          destination = read(imageSource ? imageCurrentSrcGetter : anchorHrefGetter, element);
          if (imageSource && !destination) destination = read(imageSrcGetter, element);
        } catch (_) { destination = undefined; }
        if (typeof destination === "string" && destination !== "" &&
            utf8Length(destination, 2049) <= 2048 &&
            (apply(stringStartsWith, destination, ["https://"]) || apply(stringStartsWith, destination, ["http://"])) &&
            (state.request.b.lu === true ||
              (!apply(stringIncludes, destination, ["?"]) && !apply(stringIncludes, destination, ["#"]))) &&
            !apply(stringIncludes, destination, ["@"])) {
          const field = consumeField(destination, 2048, state);
          if (!field.secret && field.value === destination) wire[imageSource ? "m" : "u"] = destination;
        }
      }

      if (descriptor.role === "password") {
        wire.v = { k: "redacted" };
      } else if (descriptor.role === "checkbox" || descriptor.role === "radio") {
        wire.v = { k: "boolean", value: (states & 1) !== 0 };
      } else if (
        (descriptor.role === "combobox" && descriptor.tag === "select") ||
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
      } else if (textFillRole(descriptor.role) || descriptor.role === "spinbutton") {
        if (credentialField(element, descriptor, wire.n || "")) {
          wire.v = { k: "redacted" };
          setSensitivity(record, "secret");
        } else if (descriptor.plainTextEditable && descriptor.editableEmpty) {
          addValue(record, "", state);
        } else if (descriptor.contentEditable !== true) {
          let value = null;
          try {
            if (descriptor.tag === "input") value = read(inputValueGetter, element);
            if (descriptor.tag === "textarea") value = read(textareaValueGetter, element);
          } catch (_) {
            value = null;
          }
          if (typeof value === "string") addValue(record, value, state);
        }
      }
    }
    if ((state.fieldTruncations || 0) !== beforeFieldTruncations) wire.fc = false;
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
          // Closed native options lack geometry. Admit only under a retained
          // select, never an offscreen/filtered select. Brand-check the parent:
          // filtering can leave Document here, which rejects Element getters.
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
            // Structural wrappers and named links retain their enclosing prose
            // sink; their own labels never replace the visible descendant text.
            const inheritsProse = descriptor.role === "link" || descriptor.role === "document" ||
              descriptor.role === "group" || descriptor.role === "landmark" || descriptor.role === "list";
            sink = record.sink !== null ? index :
              inheritsProse && record.sensitivity === "public" &&
                sink !== null && records[sink].sink === "text" ? sink : null;
            // A region is one structural level, not a full-DOM subtree. Keep
            // nested regions as current, independently expandable anchors so
            // an earlier navigation/sidebar cannot consume the parent's prose
            // budget. Subtree retains explicit recursive semantics.
            if (anchored && state.request.s?.k === "region" && item.node !== root &&
                (descriptor.role === "landmark" || descriptor.role === "document")) {
              state.regionBoundary = true;
              continue;
            }
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
    if (scope === "region" || scope === "text_search") {
      return ["document", "landmark", "group", "dialog"].includes(descriptor.role);
    }
    if (scope === "table") return descriptor.role === "table";
    if (scope === "frame") return descriptor.role === "frame_boundary";
    if (scope === "subtree" || scope === "surrounding_text") {
      return descriptor.role !== "frame_boundary";
    }
    return false;
  }

  function appendWindowChunk(window, chunk, bytes) {
    if (chunk.text === "") return;
    const last = window.chunks[window.chunks.length - 1];
    const separator = window.bytes === 0 ? 0 : 1;
    if (last !== undefined && last.run === chunk.run) {
      last.text += ` ${chunk.text}`;
      last.bytes += 1 + bytes;
    } else {
      chunk.bytes = bytes;
      window.chunks.push(chunk);
    }
    window.bytes += separator + bytes;
  }

  function addRollingChunk(window, chunk, byteLimit, state) {
    if (chunk.text === "" || byteLimit === 0) return;
    appendWindowChunk(window, chunk, utf8Length(chunk.text, byteLimit + 1));
    // The byte window bounds storage independently of the output-node budget.
    // A deque avoids rescanning/shifting every retained source per DOM fragment.
    while (window.bytes > byteLimit) {
      const first = window.chunks[window.head];
      const excess = window.bytes - byteLimit;
      if (first.bytes <= excess) {
        window.head += 1;
        window.bytes -= first.bytes + (window.head < window.chunks.length ? 1 : 0);
      } else {
        first.text = utf8Suffix(first.text, first.bytes - excess);
        const kept = utf8Length(first.text, byteLimit + 1);
        window.bytes -= first.bytes - kept;
        first.bytes = kept;
        if (kept === 0) {
          window.head += 1;
          if (window.head < window.chunks.length) window.bytes -= 1;
        }
      }
      mark(state, "scope_boundary", false);
    }
    if (window.head >= 128 && window.head * 2 >= window.chunks.length) {
      window.chunks = window.chunks.slice(window.head);
      window.head = 0;
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

  function surroundingChunks(anchor, state, beforeLimit, afterLimit) {
    const before = { chunks: [], head: 0, bytes: 0 };
    const after = { chunks: [], head: 0, bytes: 0 };
    let seenAnchor = false;
    let previousSource = null;
    let run = 0;
    const root = read(documentElementGetter, document);
    if (root === null || root === undefined) return { before: [], after: [] };
    const stack = [{ node: root, source: document, textual: false }];
    while (stack.length !== 0 && !state.stopped) {
      const item = stack.pop();
      const current = item.node;
      if (!visit(state)) break;
      if (current === anchor) {
        seenAnchor = true;
        previousSource = null;
        continue;
      }
      const type = nodeType(current);
      if (type === 3) {
        if (!textNodeVisible(current)) continue;
        const raw = read(characterDataGetter, current);
        if (typeof raw !== "string") continue;
        if (previousSource !== item.source) { run += 1; previousSource = item.source; }
        if (!seenAnchor) {
          // Keep the nearest suffix with a bounded tail scan and storage.
          const scan = beforeLimit * 8 + 256;
          const tail = apply(stringSlice, raw, [-scan]);
          const normalized = normalizeText(tail, scan * 3);
          addRollingChunk(before, { element: item.source, run, text: utf8Suffix(normalized.value, beforeLimit) }, beforeLimit, state);
          if (normalized.truncated || normalized.bytes > beforeLimit || tail.length < raw.length) {
            mark(state, "scope_boundary", false);
          }
        } else if (after.bytes < afterLimit) {
          const separator = after.bytes === 0 ? 0 : 1;
          const normalized = normalizeText(raw, mathMax(0, afterLimit - after.bytes - separator));
          if (normalized.value !== "") {
            appendWindowChunk(after, { element: item.source, run, text: normalized.value }, normalized.bytes);
          }
          if (normalized.truncated) { mark(state, "scope_boundary", false); break; }
        }
        if (seenAnchor && after.bytes >= afterLimit) {
          if (stack.length !== 0) mark(state, "scope_boundary", false);
          break;
        }
        continue;
      }
      let source = item.source;
      let textual = item.textual;
      if (type === 1) {
        // A read window does not expose editable values, hidden ancestors, or
        // credential descendants through an unrelated prose/heading source.
        if (shouldSkipSubtree(current) || !styleIsVisible(current)) continue;
        const editable = attribute(current, "contenteditable", 16);
        const descriptor = classify(current);
        const role = descriptor === null ? null : descriptor.role;
        if ((editable !== null && lower(editable) !== "false") ||
            sensitivityFor(current) !== "public" || role === "textbox" || role === "password" ||
            role === "searchbox" || role === "spinbutton" || role === "combobox" ||
            role === "listbox" || role === "option" || role === "frame_boundary") {
          previousSource = null;
          continue;
        }
        if (descriptor !== null && elementRect(current) !== null) {
          const sink = recordSink(descriptor, false);
          // Visible inline names belong to their surrounding prose/name too.
          // Nested block prose remains an independent semantic source.
          if (!textual || sink === "text") {
            source = current;
            textual = sink === "name" || sink === "text";
          }
        }
      }
      if (type === 1 || type === 9 || type === 11) pushChildren(stack, current, { source, textual }, state);
      if (seenAnchor && afterLimit === 0) {
        if (stack.length !== 0) mark(state, "scope_boundary", false);
        break;
      }
    }
    return seenAnchor ? { before: before.chunks.slice(before.head), after: after.chunks } : { before: [], after: [] };
  }

  function surroundingRecords(anchor, descriptor, state) {
    const visibleStyle = nodeType(anchor) !== 1 || styleIsVisible(anchor);
    const rect = nodeType(anchor) === 1 && visibleStyle ? elementRect(anchor) : null;
    if (nodeType(anchor) === 1 && rect === null && descriptor.role !== "option") return null;
    const focused = focusedElements();
    const disabled = nodeType(anchor) === 1 ? disabledState(anchor, false) : false;
    const record = buildRecord(anchor, descriptor, null, rect, disabled, focused.has(anchor), state);
    record.depth = 0;
    delete record.wire.o;
    delete record.wire.u;
    const records = [record];
    const window = surroundingChunks(anchor, state, state.request.s.p, state.request.s.n);
    // Admit the nearest source on each side in turn. Preceding page furniture
    // must not consume every output slot before following evidence is considered.
    // The selected roots are subsequently emitted in their document order.
    const chunks = [];
    const admitted = new Set([anchor]);
    let beforeIndex = window.before.length - 1;
    let afterIndex = 0;
    while (beforeIndex >= 0 || afterIndex < window.after.length) {
      for (const preceding of [true, false]) {
        const chunk = preceding ? window.before[beforeIndex--] : window.after[afterIndex++];
        if (chunk === undefined) continue;
        if (admitted.has(chunk.element)) { mark(state, "scope_boundary", false); continue; }
        chunk.descriptor = classify(chunk.element);
        if (chunk.descriptor === null) continue;
        if (admitted.size >= state.request.b.n) { mark(state, "node_limit", false); continue; }
        admitted.add(chunk.element);
        chunks.push(chunk);
      }
    }
    chunks.sort((left, right) => left.run - right.run);
    // Project the collected prefix without resuming an exhausted DOM walk.
    const inspectionStopped = state.stopped;
    state.stopped = false;
    for (const chunk of chunks) {
      if (state.stopped) break;
      const sourceDescriptor = chunk.descriptor;
      // Admission retained at most one contiguous run per actual source. Never
      // stitch across another source, the omitted anchor, or a privacy boundary.
      const wire = { k: keyFor(chunk.element, state.request.g), r: sourceDescriptor.role };
      if (sourceDescriptor.role === "heading") {
        const level = sourceDescriptor.level || Number(attribute(chunk.element, "aria-level", 8));
        wire.l = numberIsSafeInteger(level) && level >= 1 && level <= 6 ? level : 2;
      }
      const source = { wire, element: chunk.element, sink: "text", sinkBytes: 0, sensitivity: "public", depth: 0 };
      records.push(source);
      appendSink(source, chunk.text, state);
    }
    state.stopped = state.stopped || inspectionStopped;
    return records;
  }

  function searchTextExcluded(element) {
    if (shouldSkipSubtree(element) || !styleIsVisible(element)) return true;
    const editable = attribute(element, "contenteditable", 16);
    const described = classify(element);
    const role = described === null ? null : described.role;
    return (editable !== null && lower(editable) !== "false") || sensitivityFor(element) !== "public" ||
      ["textbox", "password", "searchbox", "spinbutton", "combobox", "listbox", "option", "frame_boundary"].includes(role);
  }

  function searchTextRecords(anchor, descriptor, state) {
    // Fixed native recipe: bounded literal-token matching over rendered source
    // passages. It does not query selectors, inspect hidden values, or execute
    // query text. Scan and retained output have independent hard ceilings.
    // A fresh region ref cannot jump over an excluded ancestor. Include open
    // shadow hosts when checking the path back to the original document.
    let ancestor = anchor;
    while (ancestor !== document) {
      if (ancestor === null || !visit(state)) return null;
      const type = nodeType(ancestor);
      if (type === 1 && searchTextExcluded(ancestor)) return null;
      const parent = read(nodeParentGetter, ancestor);
      ancestor = parent === null && type === 11 ? read(shadowHostGetter, ancestor) : parent;
      if (ancestor === undefined) return null;
    }
    const terms = new Set(apply(stringSplit, lower(state.request.s.q), [/[^\p{Alphabetic}\p{N}]+/u]));
    terms.delete("");
    // Symbol-only queries (for example "$" on an unlabeled price) are plain
    // substrings. Never interpret them as regular expressions or add sentence
    // punctuation to the exact-word OR terms of normal language queries.
    const literal = terms.size === 0 ? apply(stringTrim, state.request.s.q, []) : null;
    const candidates = [];
    let pending = null;
    let ordinal = 0;
    let scannedBytes = 0;
    const flush = () => {
      if (pending === null) return;
      const matched = new Set();
      if (literal !== null) {
        if (apply(stringIncludes, pending.text, [literal])) matched.add(literal);
      } else {
        for (const word of apply(stringSplit, lower(pending.text), [/[^\p{Alphabetic}\p{N}]+/u])) {
          if (terms.has(word)) matched.add(word);
        }
      }
      if (matched.size !== 0) {
        pending.score = matched.size;
        const duplicate = candidates.findIndex((candidate) => candidate.element === pending.element);
        if (duplicate >= 0) {
          // Keep one contiguous passage per real source; never stitch separate
          // runs across descendants or a privacy boundary into a false quote.
          if (candidates[duplicate].score >= pending.score) { pending = null; return; }
          candidates.splice(duplicate, 1);
        }
        candidates.push(pending);
        candidates.sort((left, right) => right.score - left.score || left.ordinal - right.ordinal);
        if (candidates.length > 16) { candidates.pop(); mark(state, "scope_boundary", false); }
      }
      pending = null;
    };
    const stack = [{ node: anchor, source: anchor, textual: false }];
    while (stack.length !== 0 && !state.stopped) {
      const item = stack.pop();
      if (!visit(state)) break;
      const current = item.node;
      const type = nodeType(current);
      if (type === 3) {
        if (!textNodeVisible(current)) { flush(); continue; }
        const raw = read(characterDataGetter, current);
        if (typeof raw !== "string") continue;
        const remaining = 131072 - scannedBytes;
        const normalized = normalizeText(raw, mathMin(4096, remaining), remaining);
        scannedBytes += normalized.scannedBytes;
        if (normalized.truncated) mark(state, "scope_boundary", false);
        if (normalized.value !== "") {
          if (pending !== null && pending.element !== item.source) flush();
          if (pending === null) pending = { element: item.source, text: "", bytes: 0, ordinal: ordinal++ };
          const part = normalizeText(normalized.value, mathMax(0, 4096 - pending.bytes - (pending.bytes === 0 ? 0 : 1)));
          if (part.value !== "") {
            pending.text += `${pending.bytes === 0 ? "" : " "}${part.value}`;
            pending.bytes += part.bytes + (pending.bytes === 0 ? 0 : 1);
          }
          if (part.truncated) { mark(state, "scope_boundary", false); flush(); }
        }
        // A raw scan can skip meaningful text after a tiny normalized prefix
        // (for example whitespace followed by a negation). End that quote here
        // before any later sibling fragment from the same source is admitted.
        if (normalized.truncated) flush();
        if (scannedBytes >= 131072) { state.completeness = "inspection_limit"; state.stopped = true; break; }
        continue;
      }
      let source = item.source;
      let textual = item.textual;
      if (type === 1) {
        if (searchTextExcluded(current)) { flush(); continue; }
        const described = classify(current);
        if (described !== null && elementRect(current) !== null) {
          const sink = recordSink(described, false);
          if (!textual || sink === "text") { source = current; textual = sink === "name" || sink === "text"; }
        }
      }
      if (type === 1 || type === 9 || type === 11) {
        pushChildren(stack, current, { source, textual }, state);
      }
    }
    flush();
    const record = { wire: { k: keyFor(anchor, state.request.g), r: descriptor.role }, element: anchor,
      sink: "text", sinkBytes: 0, sensitivity: "public", depth: 0 };
    const records = [record];
    const stopped = state.stopped;
    state.stopped = false;
    // Highest coverage first is deterministic. Each independent result keeps
    // its actual source key, never the searched region's provenance.
    let outputBytes = 0;
    for (const chunk of candidates) {
      if (records.length >= state.request.b.n && chunk.element !== anchor) { mark(state, "node_limit", false); break; }
      const part = normalizeText(chunk.text, mathMax(0, 8192 - outputBytes));
      if (part.value === "") { mark(state, "text_limit", false); break; }
      outputBytes += part.bytes;
      const described = classify(chunk.element);
      if (described === null) continue;
      const source = chunk.element === anchor ? record : {
        wire: { k: keyFor(chunk.element, state.request.g), r: described.role }, element: chunk.element,
        sink: "text", sinkBytes: 0, sensitivity: "public", depth: 0
      };
      if (described.role === "heading") {
        const level = described.level || Number(attribute(chunk.element, "aria-level", 8));
        source.wire.l = numberIsSafeInteger(level) && level >= 1 && level <= 6 ? level : 2;
      }
      if (source !== record) records.push(source);
      appendSink(source, part.value, state);
      if (part.truncated) mark(state, "text_limit", false);
      if (state.stopped) break;
    }
    state.stopped = state.stopped || stopped;
    return records;
  }

  function encodeSnapshot(request, records, completeness) {
    let dialogSample = "";
    const pending = pendingDialogSample;
    pendingDialogSample = null;
    if (pending !== null && request.i === pending.i + 1 && request.g === pending.g + 1) {
      const after = sampleVisiblePageDialogs();
      if (arrayIsArray(after)) dialogSample = `,"u":${apply(jsonStringify, JSON, [{...pending, after}])}`;
    }
    const encodedNodes = [];
    for (const record of records) encodedNodes.push(apply(jsonStringify, JSON, [record.wire]));

    const compose = (status) => {
      const header = `{"v":${WIRE_VERSION},"i":${request.i},"g":${request.g},"c":"${status}"${dialogSample},"n":[`;
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

  function sampleVisiblePageDialogs() {
    const state = { request: { b: { x: MAX_DIALOG_SAMPLE_NODES } }, visited: 0, stopped: false, completeness: "complete" };
    const root = read(documentElementGetter, document);
    if (root === null || root === undefined) return "page_dialog_sample_unavailable";
    const stack = [{node: root}];
    const result = [];
    const viewport = boundedViewport();
    if (viewport === null) return "page_dialog_sample_unavailable";
    // This is a complete DOM census, not a semantic projection: framework
    // wrappers must not consume the semantic tree's depth budget. The visited
    // and queued node ceiling bounds both traversal work and stack memory,
    // including deep trees. Incomplete samples never prove an
    // absent dialog and never authorize dispatch.
    while (stack.length > 0 && !state.stopped) {
      const item = stack.pop();
      if (++state.visited > MAX_DIALOG_SAMPLE_NODES) return "page_dialog_sample_limit";
      const element = nodeType(item.node) === 1;
      const hidden = element && shouldSkipSubtree(item.node);
      if (element && !hidden) {
        const tag = lower(read(elementTagGetter, item.node) || "");
        const role = lower(attribute(item.node, "role", 32) || "");
        if (tag === "dialog" || role === "dialog" || role === "alertdialog") {
          const rect = styleIsVisible(item.node) ? elementRect(item.node) : null;
          if (rect !== null && actionPoint(item.node, rect, viewport) !== null) {
            if (result.length === MAX_DIALOG_SAMPLE_DIALOGS) return "page_dialog_sample_limit";
            let key = apply(weakMapGet, dialogKeys, [item.node]);
            if (key === undefined) {
              if (!isPositiveSafeInteger(nextDialogKey)) return "page_dialog_sample_limit";
              key = nextDialogKey++;
              apply(weakMapSet, dialogKeys, [item.node, key]);
            }
            result.push(key);
          }
        }
      }
      if (!hidden) pushChildren(stack, item.node, {}, state);
    }
    return state.stopped ? "page_dialog_sample_limit" : result;
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
    const valueMatches = actual.vo === 0 && !actual.vb && actual.vk === 1 && actual.vt === value;
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

  function captureEditingContext(node) {
    const context = [];
    let current = read(nodeParentGetter, node);
    for (let depth = 0; depth < MAX_TREE_DEPTH; depth += 1) {
      if (current === document) return context;
      if (current === null || nodeType(current) !== 1 || read(nodeConnectedGetter, current) !== true ||
          apply(nodeGetRoot, current, []) !== document || disabledState(current, false) ||
          has(current, "readonly") || has(current, "inert") || has(current, "hidden") ||
          lower(attribute(current, "aria-readonly", 16) || "") === "true" ||
          lower(attribute(current, "aria-hidden", 16) || "") === "true" || !styleIsVisible(current) ||
          sensitivityFor(current) !== "public" ||
          credentialField(current, { role: "group", tag: tagName(current) }, attribute(current, "aria-label", 512) || "")) return null;
      context.push({ node: current, editable: read(editableGetter, current) === true });
      current = read(nodeParentGetter, current);
    }
    return null;
  }

  function editingContextMatches(target, request, current) {
    const entry = keyNodes.get(request.t);
    if (entry === undefined || entry.node !== target || entry.generation !== request.g ||
        entry.editingContext === undefined) return false;
    const prior = entry.editingContext;
    current = current || null;
    if (prior === null || current === null) return prior === current;
    return prior.length === current.length && prior.every((item, index) =>
      item.node === current[index].node && item.editable === current[index].editable);
  }

  function editableHostSupport(node, structure, witness) {
    try {
      if (read(editableGetter, node) !== true) return 3;
      if (!["div", "span", "p", "h1", "h2", "h3", "h4", "h5", "h6"].includes(tagName(node))) return 4;
      const parent = read(nodeParentGetter, node);
      const editableParent = parent !== null && nodeType(parent) === 1 && read(editableGetter, parent) === true;
      const children = read(nodeChildNodesGetter, node);
      const length = listLength(children);
      let kinds = 0;
      let firstUnsupported = 0;
      for (let index = 0; index < mathMin(length, 128); index += 1) {
        const kind = nodeType(listItem(children, index));
        if (kind !== 3 || read(characterDataGetter, listItem(children, index)) !== "") witness.empty = false;
        kinds |= kind === 3 ? 1 : kind === 1 ? 2 : 4;
        if (kind !== 3 && firstUnsupported === 0) firstUnsupported = kind === 1 ? 7 : 8;
      }
      structure.push(mathMin(length, 129), kinds, editableParent);
      if (editableParent) {
        if (length > 128 || firstUnsupported !== 0) return 5;
        const context = captureEditingContext(node);
        if (context === null) return 5;
        witness.context = context;
      }
      if (length > 128) return 6;
      return firstUnsupported || 1;
    } catch (_) { return 9; }
  }

  function textFillRole(role) {
    return role === "textbox" || role === "searchbox" || role === "combobox";
  }

  function fillControlKind(descriptor, value) {
    if (!textFillRole(descriptor.role)) return 0;
    if (descriptor.tag === "input") {
      if (descriptor.inputType !== "text" && descriptor.inputType !== "search") return 0;
      for (const character of value) {
        const point = character.codePointAt(0);
        if (point === 0x09 || point === 0x0a || point === 0x0d) return 0;
      }
      return 1;
    }
    if (descriptor.tag === "textarea" && descriptor.role !== "searchbox") return 2;
    if (descriptor.contentEditable === true && descriptor.plainTextEditable === true) return 3;
    return 0;
  }

  // Only runAction's admitted private ref can reach this recipe. There is no
  // page-world request listener, shared callback, transport marker or token.
  function runFixedFill(target, descriptor, request) {
    const value = request.z;
    const kind = fillControlKind(descriptor, value);
    const valueSetter = kind === 1 ? inputValueSetter :
      kind === 2 ? textareaValueSetter : kind === 3 ? nodeTextSetter : null;
    if (valueSetter === null || typeof nativeInputEvent !== "function" ||
        typeof fixedDispatchEvent !== "function" || !validActionText(value)) {
      return "unsupported_interaction";
    }
    const revalidate = () => {
      if (resolveKeyAtGeneration(request.t, request.g) !== target ||
          read(nodeConnectedGetter, target) !== true ||
          apply(nodeGetRoot, target, []) !== document) return false;
      const current = classify(target);
      if (current === null || fillControlKind(current, value) !== kind ||
          disabledState(target, false) || has(target, "readonly") ||
          lower(attribute(target, "aria-readonly", 16) || "") === "true" ||
          credentialField(target, current, attribute(target, "aria-label", 512) || "")) return false;
      if (kind === 3 && !editingContextMatches(target, request, current.editingContext)) return false;
      return descriptorMatches(request.f, runtimeDescriptor(target, request.g));
    };
    let before;
    let input;
    try {
      if (!revalidate()) return "target_changed";
      before = new nativeInputEvent("beforeinput", {
        bubbles: true, cancelable: true, composed: true, data: value,
        inputType: "insertReplacementText", isComposing: false
      });
      input = new nativeInputEvent("input", {
        bubbles: true, cancelable: false, composed: true, data: value,
        inputType: "insertReplacementText", isComposing: false
      });
    } catch (_) { return "unsupported_interaction"; }
    try {
      if (apply(fixedDispatchEvent, target, [before]) !== true) {
        return "applied_unverified_beforeinput_cancelled";
      }
      if (!revalidate()) return "applied_unverified_beforeinput_revalidation";
    } catch (_) { return "applied_unverified_beforeinput_revalidation"; }
    try {
      write(valueSetter, target, value);
      apply(fixedDispatchEvent, target, [input]);
    } catch (_) { return "applied_unverified_mutation"; }
    try {
      if (kind === 3) {
        const current = classify(target);
        if (current === null || !editingContextMatches(target, request, current.editingContext))
          return "applied_unverified_postcondition";
      }
    } catch (_) { return "applied_unverified_postcondition"; }
    return "ok";
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
    pendingDialogSample = null;
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
      if (request.u === true) {
        const before = sampleVisiblePageDialogs();
        if (!arrayIsArray(before)) return actionFault(before);
        pendingDialogSample = {a: request.a, i: request.i, g: request.g, before};
      }
      try {
        apply(htmlElementClick, target, []);
      } catch (_) {
        pendingDialogSample = null;
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
        // The captured isolated-world setter has run when the recipe returns ok.
        // Every later mismatch is indeterminate, never a retryable target fault.
        if (finalDescriptor === null || finalDescriptor.role !== request.r) {
          return actionFault("applied_unverified_postcondition");
        }
        const finalDisabled = disabledState(target, false);
        const finalReadonly = has(target, "readonly") || lower(attribute(target, "aria-readonly", 16) || "") === "true";
        if (finalDisabled || finalReadonly) return actionFault("applied_unverified_postcondition");
        if (
          credentialField(target, finalDescriptor, attribute(target, "aria-label", 512) || "") ||
          finalDescriptor.role === "password" || fillControlKind(finalDescriptor, request.z) === 0
        ) {
          return actionFault("applied_unverified_postcondition");
        }
        const filledDescriptor = runtimeDescriptor(target, request.g);
        if (!descriptorMatchesFilledValue(request.f, filledDescriptor, request.z)) {
          return actionFault("applied_unverified_postcondition");
        }
        return encodeActionEvidence(
          request,
          "fixed_semantic_recipe",
          "form",
          geometry,
          viewport,
          point,
          delta
        );
      };
      const result = runFixedFill(target, descriptor, request);
      try {
        return finishFill(result);
      } catch (_) {
        return actionFault("applied_unverified_postcondition");
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
      recordEditingWitness: true,
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
      if (request.s.k === "text_search") {
        records = searchTextRecords(anchor, descriptor, state);
        if (records === null) return fault("anchor_missing");
      } else if (request.s.k === "surrounding_text") {
        records = surroundingRecords(anchor, descriptor, state);
        if (records === null) return fault("anchor_missing");
      } else if (request.s.k === "frame") {
        records = traverse(anchor, state, true);
        mark(state, "scope_boundary", false);
      } else {
        records = traverse(anchor, state, true);
      }
    }
    if (state.regionBoundary) mark(state, "scope_boundary", false);
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
      if (encoded === CHANNEL_PARK) {
        clearDocumentState();
        let parked;
        try {
          parked = await apply(post, channel, [CHANNEL_PARKED]);
        } catch (_) {
          return;
        }
        if (parked !== CHANNEL_ACK) return;
        return;
      }

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
    if (transportActive) return;
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
    transportActive = true;
    const serving = serveNativeInvocations(channel, post);
    void apply(promiseThen, serving, [
      () => { transportActive = false; },
      () => { transportActive = false; }
    ]);
  }

  objectFreeze(invoke);
  const api = objectFreeze({ invoke });
  objectDefineProperty(globalThis, GLOBAL_NAME, {
    value: api,
    writable: false,
    configurable: false,
    enumerable: false
  });
  apply(eventTargetAddEventListener, globalThis, ["pageshow", startNativeTransport, true]);
  startNativeTransport();
})();
