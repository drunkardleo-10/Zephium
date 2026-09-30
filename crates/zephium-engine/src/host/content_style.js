// Browser-owned DOM presentation only. This object grants no native bridge,
// storage, network, permissions, or cross-origin access. Every result returned
// to Rust is untrusted document data and requires native document attribution.
(() => {
  "use strict";
  const key = "__zephium_content_style_v1__";
  if (Object.prototype.hasOwnProperty.call(globalThis, key)) return;
  const apply = Reflect.apply;
  const create = Object.create;
  const stringify = JSON.stringify;
  const descriptor = Object.getOwnPropertyDescriptor(Document.prototype, "adoptedStyleSheets");
  if (!descriptor?.get || !descriptor?.set || typeof CSSStyleSheet !== "function") return;
  const replace = CSSStyleSheet.prototype.replaceSync;
  const getSheets = () => apply(descriptor.get, document, []);
  const setSheets = sheets => apply(descriptor.set, document, [sheets]);
  const queryAll = Document.prototype.querySelectorAll;
  const random = new Uint32Array(4);
  crypto.getRandomValues(random);
  const token = Array.from(random, n => n.toString(16).padStart(8, "0")).join("");
  const slots = new Map();
  const allowed = new Set(["subscription", "personal", "preview"]);
  let picker = null;

  function matches(expectedToken, expectedUrl) {
    return expectedToken === token && expectedUrl === location.href;
  }

  function setStyle(slot, expectedToken, expectedUrl, generation, fingerprint, css) {
    if (!allowed.has(slot) || !matches(expectedToken, expectedUrl) ||
        !/^[0-9a-f]{16}$/.test(generation) || !/^[0-9a-f]{64}$/.test(fingerprint) ||
        typeof css !== "string" || css.length > 1048576) return false;
    const old = slots.get(slot);
    if (old && generation < old.generation) return false;
    if (old && generation === old.generation && fingerprint !== old.fingerprint) return false;
    try {
      if (old && old.fingerprint === fingerprint && getSheets().includes(old.sheet)) {
        old.generation = generation;
        return true;
      }
      const sheet = new CSSStyleSheet();
      apply(replace, sheet, [css]);
      const sheets = getSheets().filter(s => !old || s !== old.sheet);
      if (css) sheets.push(sheet);
      setSheets(sheets);
      // Keep only the native-supplied identity, not a second copy of the CSS
      // text alongside the browser's parsed stylesheet in every document.
      slots.set(slot, { sheet, fingerprint, generation });
      return true;
    } catch (_) { return false; }
  }

  function clearStyle(slot) {
    const old = slots.get(slot);
    if (!old) return;
    try { setSheets(getSheets().filter(s => s !== old.sheet)); } catch (_) {}
    slots.delete(slot);
  }

  function stopPicker() {
    if (!picker) return;
    const current = picker;
    picker = null;
    current.abort.abort();
    if (current.frame) cancelAnimationFrame(current.frame);
    current.cover.remove();
    clearStyle("preview");
  }

  function cssString(value) {
    return String(value).replace(/[\0-\x1f\x7f"\\]/g, c => "\\" + c.charCodeAt(0).toString(16) + " ");
  }

  function candidate(element) {
    if (!(element instanceof Element) || element === document.documentElement || element === document.body) return null;
    let selector = "";
    let positional = false;
    const id = element.getAttribute("id");
    if (id && id.length <= 128) selector = `[id="${cssString(id)}"]`;
    else {
      const classes = Array.from(element.classList).filter(c => c.length <= 80 && !/^(css|jsx|sc)-/.test(c)).slice(0, 2);
      if (classes.length) selector = classes.map(c => `[class~="${cssString(c)}"]`).join("");
    }
    // A bounded ancestor path is the fallback, with its instability exposed to
    // the UI. Never keep a DOM snapshot, arbitrary element text, or form values.
    if (!selector) {
      positional = true;
      const path = [];
      let node = element;
      for (let depth = 0; depth < 8 && node && node !== document.body; depth++, node = node.parentElement) {
        const name = node.localName;
        if (!/^[a-z][a-z0-9-]{0,63}$/.test(name)) return null;
        let index = 1;
        let previous = node.previousElementSibling;
        while (previous && index <= 10000) {
          if (previous.localName === name) index++;
          previous = previous.previousElementSibling;
        }
        if (index > 10000) return null;
        path.unshift(`${name}:nth-of-type(${index})`);
      }
      selector = path.join(" > ");
    }
    if (!selector || selector.length > 2048) return null;
    try {
      const count = apply(queryAll, document, [selector]).length;
      if (count === 0 || count > 100) return null;
      const label = id ? `${element.localName} #${id}` : element.localName === "iframe" ? "Embedded content" : element.localName;
      const result = create(null);
      result.selector = selector; result.label = Array.from(label).slice(0, 80).join("");
      result.count = count; result.positional = positional;
      return result;
    } catch (_) { return null; }
  }

  function startPicker(expectedToken, expectedUrl, session) {
    if (!matches(expectedToken, expectedUrl) || !/^[0-9a-f]{16}$/.test(session) || !document.documentElement) return false;
    stopPicker();
    try {
      const cover = document.createElement("div");
      cover.setAttribute("popover", "manual");
      cover.style.cssText = "position:fixed;inset:0;margin:0;width:100%;height:100%;padding:0;border:0;background:transparent;z-index:2147483647;cursor:crosshair";
      const shadow = cover.attachShadow({ mode: "closed" });
      const box = document.createElement("div");
      box.style.cssText = "position:fixed;pointer-events:none;border:2px solid Highlight;background:transparent;box-sizing:border-box;display:none";
      shadow.append(box);
      document.documentElement.append(cover);
      const abort = new AbortController();
      picker = { cover, box, abort, session, selected: null, selectedElement: null, hovered: null, frame: 0, point: null };
      if (typeof cover.showPopover === "function") cover.showPopover();
      const current = picker;
      function targetAt(x, y) {
        cover.style.pointerEvents = "none";
        const target = document.elementFromPoint(x, y);
        cover.style.pointerEvents = "auto";
        return target;
      }
      function highlight(target) {
        if (!(target instanceof Element) || target === cover || target === document.documentElement || target === document.body) {
          box.style.display = "none";
          return;
        }
        const rect = target.getBoundingClientRect();
        box.style.display = "block";
        box.style.left = `${rect.left}px`; box.style.top = `${rect.top}px`;
        box.style.width = `${rect.width}px`; box.style.height = `${rect.height}px`;
      }
      cover.addEventListener("pointermove", event => {
        if (!event.isTrusted) return;
        if (current.selected) return;
        current.point = [event.clientX, event.clientY];
        if (current.frame) return;
        current.frame = requestAnimationFrame(() => {
          current.frame = 0;
          if (picker !== current) return;
          current.hovered = targetAt(...current.point);
          highlight(current.hovered);
        });
      }, { signal: abort.signal, passive: true });
      cover.addEventListener("click", event => {
        if (!event.isTrusted) return;
        event.preventDefault(); event.stopImmediatePropagation();
        const target = targetAt(event.clientX, event.clientY);
        clearStyle("preview");
        current.selected = candidate(target);
        current.selectedElement = current.selected ? new WeakRef(target) : null;
        highlight(target);
      }, { signal: abort.signal, capture: true });
      document.addEventListener("keydown", event => {
        if (event.key === "Escape") { event.preventDefault(); event.stopImmediatePropagation(); stopPicker(); }
      }, { signal: abort.signal, capture: true });
      globalThis.addEventListener("pagehide", stopPicker, { signal: abort.signal, once: true });
      return true;
    } catch (_) { stopPicker(); return false; }
  }

  Object.defineProperty(globalThis, key, { value: Object.freeze({
    version: 1,
    inspect: () => ({ token, url: location.href }),
    inspectEncoded: () => {
      if (location.href.length > 32768) return null;
      const result = create(null); result.token = token; result.url = location.href;
      return stringify(result);
    },
    apply: setStyle,
    startPicker,
    selection: (expectedToken, expectedUrl, session) => matches(expectedToken, expectedUrl) && picker?.session === session ? picker.selected : null,
    pickerEncoded: (expectedToken, expectedUrl, session) => {
      const result = create(null);
      result.active = matches(expectedToken, expectedUrl) && picker?.session === session && picker.cover.isConnected;
      result.selection = result.active && picker.selectedElement?.deref()?.isConnected ? picker.selected : null;
      return stringify(result);
    },
    preview: (expectedToken, expectedUrl, session, enabled) => {
      if (!matches(expectedToken, expectedUrl) || picker?.session !== session || !picker.selected) return false;
      if (!enabled) { clearStyle("preview"); return true; }
      return setStyle("preview", expectedToken, expectedUrl, session, session.padStart(64, "0"), `${picker.selected.selector}{display:none!important}`);
    },
    stopPicker: (expectedToken, expectedUrl) => {
      if (!matches(expectedToken, expectedUrl)) return false;
      stopPicker(); return true;
    }
  }), writable: false, configurable: false });
})();
