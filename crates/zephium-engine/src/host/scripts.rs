use zephium_core::ids::ProfileId;
use zephium_core::ports::engine::{
    ContentScope, Partition, Shortcut, UserContent, UserScript, World,
};

use super::EngineHost;

// Installed before every page script. It records otherwise non-enumerable
// unload/audio/capture state, while the query itself directly compares form
// controls with their defaults. This is a data-loss guard, not a capability:
// it exposes no Rust/native object and returns only a fixed boolean schema.
// Any hook replacement, excessive state, child frame, or exception becomes
// `uncertain`, which vetoes discard.
pub(super) const DISCARD_SAFETY_BOOTSTRAP_JS: &str = r#"(function(){
  'use strict';
  var key = '__zephium_discard_safety_v1__';
  if (Object.prototype.hasOwnProperty.call(globalThis, key)) return;
  var uncertain = false;
  var beforeUnload = [];
  var audioContexts = new Set();
  var captureTracks = new Set();
  var MAX_TRACKED = 4096;
  var MAX_SHADOW_ROOTS = 256;
  var shadowRoots = [];
  var shadowRootSet = new WeakSet();
  var apply = Reflect.apply;
  var construct = Reflect.construct;
  var weakSetAdd = WeakSet.prototype.add;
  var weakSetHas = WeakSet.prototype.has;
  var arrayPush = Array.prototype.push;

  function captureGetter(proto, name) {
    try {
      var descriptor = Object.getOwnPropertyDescriptor(proto, name);
      return descriptor && typeof descriptor.get === 'function' ? descriptor.get : null;
    } catch (_) { uncertain = true; return null; }
  }
  function callGetter(getter, value) {
    if (!getter) throw new Error('missing platform getter');
    return apply(getter, value, []);
  }

  var documentQueryAll = Document.prototype.querySelectorAll;
  var fragmentQueryAll = DocumentFragment.prototype.querySelectorAll;
  var readyState = captureGetter(Document.prototype, 'readyState');
  var shadowRootGetter = captureGetter(Element.prototype, 'shadowRoot');
  var inputType = captureGetter(HTMLInputElement.prototype, 'type');
  var inputValue = captureGetter(HTMLInputElement.prototype, 'value');
  var inputDefaultValue = captureGetter(HTMLInputElement.prototype, 'defaultValue');
  var inputChecked = captureGetter(HTMLInputElement.prototype, 'checked');
  var inputDefaultChecked = captureGetter(HTMLInputElement.prototype, 'defaultChecked');
  var inputFiles = captureGetter(HTMLInputElement.prototype, 'files');
  var textareaValue = captureGetter(HTMLTextAreaElement.prototype, 'value');
  var textareaDefaultValue = captureGetter(HTMLTextAreaElement.prototype, 'defaultValue');
  var selectOptions = captureGetter(HTMLSelectElement.prototype, 'options');
  var optionSelected = captureGetter(HTMLOptionElement.prototype, 'selected');
  var optionDefaultSelected = captureGetter(HTMLOptionElement.prototype, 'defaultSelected');
  var mediaPaused = captureGetter(HTMLMediaElement.prototype, 'paused');
  var mediaEnded = captureGetter(HTMLMediaElement.prototype, 'ended');
  var mediaReadyState = captureGetter(HTMLMediaElement.prototype, 'readyState');
  var mediaSrcObject = captureGetter(HTMLMediaElement.prototype, 'srcObject');
  var trackReadyState = globalThis.MediaStreamTrack &&
      captureGetter(MediaStreamTrack.prototype, 'readyState');
  var getTracks = globalThis.MediaStream && MediaStream.prototype.getTracks;
  var contextState = globalThis.BaseAudioContext &&
      captureGetter(BaseAudioContext.prototype, 'state');

  var elementProto = globalThis.Element && Element.prototype;
  var originalAttachShadow = elementProto && elementProto.attachShadow;
  var wrappedAttachShadow = originalAttachShadow;
  function rememberShadowRoot(root) {
    if (!root || apply(weakSetHas, shadowRootSet, [root])) return;
    if (shadowRoots.length >= MAX_SHADOW_ROOTS) {
      uncertain = true;
      return;
    }
    apply(weakSetAdd, shadowRootSet, [root]);
    apply(arrayPush, shadowRoots, [root]);
  }
  try {
    if (typeof originalAttachShadow !== 'function' || !fragmentQueryAll || !shadowRootGetter) {
      uncertain = true;
    } else {
      wrappedAttachShadow = function() {
        var root = apply(originalAttachShadow, this, arguments);
        rememberShadowRoot(root);
        return root;
      };
      elementProto.attachShadow = wrappedAttachShadow;
    }
  } catch (_) { uncertain = true; }

  var eventTarget = globalThis.EventTarget && EventTarget.prototype;
  var originalAdd = eventTarget && eventTarget.addEventListener;
  var originalRemove = eventTarget && eventTarget.removeEventListener;
  var wrappedAdd = originalAdd;
  var wrappedRemove = originalRemove;
  function captureOption(options) {
    return options === true || (!!options && typeof options === 'object' && options.capture === true);
  }
  try {
    wrappedAdd = function(type, listener, options) {
      if (this === globalThis && String(type).toLowerCase() === 'beforeunload' && listener != null) {
        var capture = captureOption(options);
        var found = false;
        for (var i = 0; i < beforeUnload.length; i++) {
          if (beforeUnload[i][0] === listener && beforeUnload[i][1] === capture) { found = true; break; }
        }
        if (!found) {
          if (beforeUnload.length < MAX_TRACKED) beforeUnload.push([listener, capture]);
          else uncertain = true;
        }
      }
      return apply(originalAdd, this, arguments);
    };
    wrappedRemove = function(type, listener, options) {
      var result = apply(originalRemove, this, arguments);
      if (this === globalThis && String(type).toLowerCase() === 'beforeunload' && listener != null) {
        var capture = captureOption(options);
        for (var i = 0; i < beforeUnload.length; i++) {
          if (beforeUnload[i][0] === listener && beforeUnload[i][1] === capture) {
            beforeUnload.splice(i, 1); break;
          }
        }
      }
      return result;
    };
    eventTarget.addEventListener = wrappedAdd;
    eventTarget.removeEventListener = wrappedRemove;
  } catch (_) { uncertain = true; }

  function observeStream(stream) {
    try {
      var tracks = apply(getTracks, stream, []);
      for (var i = 0; i < tracks.length; i++) {
        if (captureTracks.size < MAX_TRACKED) captureTracks.add(tracks[i]);
        else uncertain = true;
      }
    } catch (_) { uncertain = true; }
    return stream;
  }
  function wrapMediaMethod(proto, name) {
    try {
      if (!proto || typeof proto[name] !== 'function') return;
      var original = proto[name];
      var wrapped = function() {
        var result = apply(original, this, arguments);
        return Promise.resolve(result).then(observeStream);
      };
      proto[name] = wrapped;
      return [proto, name, wrapped];
    } catch (_) { uncertain = true; return null; }
  }
  var mediaHooks = [];
  if (globalThis.MediaDevices) {
    mediaHooks.push(wrapMediaMethod(MediaDevices.prototype, 'getUserMedia'));
    mediaHooks.push(wrapMediaMethod(MediaDevices.prototype, 'getDisplayMedia'));
  }

  var audioHooks = [];
  function wrapAudioConstructor(name) {
    try {
      var original = globalThis[name];
      if (typeof original !== 'function') return;
      var wrapped = new Proxy(original, {
        construct: function(target, args, newTarget) {
          var value = construct(target, args, newTarget === wrapped ? target : newTarget);
          if (audioContexts.size < MAX_TRACKED) audioContexts.add(value);
          else uncertain = true;
          return value;
        }
      });
      globalThis[name] = wrapped;
      audioHooks.push([name, wrapped]);
    } catch (_) { uncertain = true; }
  }
  wrapAudioConstructor('AudioContext');
  if (globalThis.webkitAudioContext !== globalThis.AudioContext) {
    wrapAudioConstructor('webkitAudioContext');
  }

  function liveTrack(track) {
    return callGetter(trackReadyState, track) === 'live';
  }
  function report() {
    var dirtyForm = false;
    var editable = false;
    var media = false;
    var audioContext = false;
    var capture = false;
    var childFrames = false;
    var localUncertain = uncertain;
    try {
      if (eventTarget.addEventListener !== wrappedAdd || eventTarget.removeEventListener !== wrappedRemove) {
        localUncertain = true;
      }
      for (var h = 0; h < mediaHooks.length; h++) {
        var hook = mediaHooks[h];
        if (hook && hook[0][hook[1]] !== hook[2]) localUncertain = true;
      }
      for (var a = 0; a < audioHooks.length; a++) {
        if (globalThis[audioHooks[a][0]] !== audioHooks[a][1]) localUncertain = true;
      }
      if (!elementProto || elementProto.attachShadow !== wrappedAttachShadow) {
        localUncertain = true;
      }

      function collect(selector) {
        var values = [];
        function append(scope, query) {
          var found = apply(query, scope, [selector]);
          var remaining = MAX_TRACKED - values.length;
          if (found.length > remaining) localUncertain = true;
          for (var n = 0; n < found.length && n < remaining; n++) {
            apply(arrayPush, values, [found[n]]);
          }
        }
        append(document, documentQueryAll);
        for (var r = 0; r < shadowRoots.length; r++) {
          if (values.length >= MAX_TRACKED) {
            localUncertain = true;
            break;
          }
          append(shadowRoots[r], fragmentQueryAll);
        }
        return values;
      }

      // Parser-created declarative roots do not necessarily pass through the
      // JS attachShadow hook. Open roots are detectable via the captured
      // native getter; retain them for inspection but veto this discard as an
      // untracked state transition. Closed declarative roots are not exposed
      // by the platform, so any still-observable declarative template also
      // makes the result uncertain.
      var discoveryPasses = 0;
      var discovered;
      do {
        discovered = false;
        var hosts = collect('*');
        for (var s = 0; s < hosts.length; s++) {
          var untracked = callGetter(shadowRootGetter, hosts[s]);
          if (untracked && !apply(weakSetHas, shadowRootSet, [untracked])) {
            localUncertain = true;
            rememberShadowRoot(untracked);
            discovered = true;
          }
        }
        discoveryPasses++;
        if (discoveryPasses > MAX_SHADOW_ROOTS) {
          localUncertain = true;
          break;
        }
      } while (discovered);
      if (collect('template[shadowrootmode],template[shadowroot]').length !== 0) {
        localUncertain = true;
      }

      var controls = collect('input,textarea,select');
      if (controls.length > MAX_TRACKED) localUncertain = true;
      for (var i = 0; i < controls.length && i < MAX_TRACKED && !dirtyForm; i++) {
        var control = controls[i];
        if (control instanceof HTMLInputElement) {
          var type = String(callGetter(inputType, control)).toLowerCase();
          if (type === 'checkbox' || type === 'radio') {
            dirtyForm = callGetter(inputChecked, control) !== callGetter(inputDefaultChecked, control);
          } else if (type === 'file') {
            var files = callGetter(inputFiles, control);
            dirtyForm = !!files && files.length !== 0;
          } else if (type !== 'button' && type !== 'submit' && type !== 'reset' &&
                     type !== 'image' && type !== 'hidden') {
            dirtyForm = callGetter(inputValue, control) !== callGetter(inputDefaultValue, control);
          }
        } else if (control instanceof HTMLTextAreaElement) {
          dirtyForm = callGetter(textareaValue, control) !== callGetter(textareaDefaultValue, control);
        } else if (control instanceof HTMLSelectElement) {
          var options = callGetter(selectOptions, control);
          if (options.length > MAX_TRACKED) localUncertain = true;
          for (var o = 0; o < options.length && o < MAX_TRACKED; o++) {
            if (callGetter(optionSelected, options[o]) !== callGetter(optionDefaultSelected, options[o])) {
              dirtyForm = true; break;
            }
          }
        }
      }

      editable = document.designMode === 'on' ||
          collect('[contenteditable]:not([contenteditable="false" i])').length !== 0;
      var elements = collect('audio,video');
      if (elements.length > MAX_TRACKED) localUncertain = true;
      for (var m = 0; m < elements.length && m < MAX_TRACKED; m++) {
        var element = elements[m];
        if (!callGetter(mediaPaused, element) && !callGetter(mediaEnded, element) &&
            callGetter(mediaReadyState, element) > 0) media = true;
        var stream = callGetter(mediaSrcObject, element);
        if (stream && getTracks) {
          var tracks = apply(getTracks, stream, []);
          for (var t = 0; t < tracks.length; t++) if (liveTrack(tracks[t])) capture = true;
        }
      }
      audioContexts.forEach(function(context) {
        if (callGetter(contextState, context) === 'running') audioContext = true;
      });
      captureTracks.forEach(function(track) { if (liveTrack(track)) capture = true; });
      childFrames = collect('iframe,frame').length !== 0;
    } catch (_) { localUncertain = true; }

    // Fixed primitive bitmask: ready=bit0; every protection/uncertainty fact
    // occupies bits1..8. Native JSON conversion can therefore produce at
    // most three ASCII bytes and never traverses a page-controlled toJSON.
    return (callGetter(readyState, document) === 'complete' ? 1 : 0) |
      ((beforeUnload.length !== 0 || typeof globalThis.onbeforeunload === 'function') ? 2 : 0) |
      (dirtyForm ? 4 : 0) | (editable ? 8 : 0) | (media ? 16 : 0) |
      (audioContext ? 32 : 0) | (capture ? 64 : 0) |
      (childFrames ? 128 : 0) | (localUncertain ? 256 : 0);
  }
  try {
    Object.defineProperty(globalThis, key, {
      value: report, writable: false, configurable: false, enumerable: false
    });
  } catch (_) { uncertain = true; }
})()"#;

pub(super) const DISCARD_SAFETY_QUERY_JS: &str = r#"(function(){
  'use strict';
  try {
    var query = globalThis.__zephium_discard_safety_v1__;
    if (typeof query !== 'function') throw new Error('missing safety tracker');
    return query();
  } catch (_) {
    return 256;
  }
})()"#;

// Fetch and decode entirely inside the untrusted site renderer. The callback
// surface is a fixed 32x32 RGBA raster; privileged Rust/chrome never parse a
// page-controlled image container. Calling this script again polls the
// renderer-owned asynchronous Image decode without adding an IPC bridge.
pub(super) const FAVICON_JS: &str = r#"(function(){
  'use strict';
  var pageUrl = String(document.location.href).slice(0, 8192);
  var key = '__zephium_favicon_rgba32_v1__';
  var readyState = document.readyState === 'complete' ? 'complete' :
      (document.readyState === 'interactive' ? 'interactive' : 'loading');
  var state = globalThis[key];
  var rebuildAfterComplete = false;
  function validRgba(value) {
    // 32 * 32 * 4 bytes encode to exactly 5464 canonical base64
    // characters, with two padding characters. Check length before the
    // regular expression so a page-preseeded huge string is never copied
    // into the renderer-to-host callback result.
    return typeof value === 'string' && value.length === 5464 &&
        /^[A-Za-z0-9+/]{5462}==$/.test(value);
  }
  if (state && state.pageUrl === pageUrl) {
    // `state` is page-readable and may have been replaced with a Proxy or
    // accessor-bearing object between polls. Snapshot every value used for
    // the native return exactly once: validating one getter result and then
    // reading it again would let a hostile getter substitute an arbitrarily
    // large string for Wry to serialize before Rust can apply its bound.
    var rgba = state.rgba;
    var done = state.done;
    var completeRebuildUsed = state.completeRebuildUsed;
    var initialReadyState = state.initialReadyState;
    if (validRgba(rgba)) return rgba;
    if (rgba == null && typeof done === 'boolean') {
      // UrlChanged can run while the parser has not inserted icon links yet.
      // Once that bounded attempt has finished, permit exactly one fresh
      // candidate scan after the same document transitions to complete.
      if (done && completeRebuildUsed === false &&
          (initialReadyState === 'loading' ||
           initialReadyState === 'interactive') &&
          readyState === 'complete') {
        rebuildAfterComplete = true;
      } else {
        return null;
      }
    }
    // Invalid page-mutable state is ignored and rebuilt below. In
    // particular, never echo an attacker-sized string to native code.
    state = null;
  }

  state = Object.create(null);
  state.pageUrl = pageUrl;
  state.rgba = null;
  state.done = false;
  state.initialReadyState = readyState;
  state.completeRebuildUsed = rebuildAfterComplete;
  state.index = 0;
  state.candidates = [];
  globalThis[key] = state;

  var ranked = [];
  var links = document.querySelectorAll('link[rel~="icon"]');
  for (var i = 0; i < links.length && i < 32; i++) {
    var href = links[i].getAttribute('href');
    if (!href || href.length > 2048) continue;
    try {
      var candidate = new URL(href, document.location.href);
      if ((candidate.protocol !== 'http:' && candidate.protocol !== 'https:') ||
          candidate.origin !== document.location.origin ||
          candidate.username || candidate.password ||
          candidate.href.length > 2048) continue;
      var declared = parseInt((links[i].getAttribute('sizes') || '').split('x')[0], 10);
      var size = Number.isFinite(declared) && declared > 0 ? declared : 32;
      ranked.push([Math.abs(size - 32), candidate.href]);
    } catch (_) {}
  }
  ranked.sort(function(a, b) { return a[0] - b[0]; });
  for (var j = 0; j < ranked.length && state.candidates.length < 4; j++) {
    if (state.candidates.indexOf(ranked[j][1]) === -1) state.candidates.push(ranked[j][1]);
  }
  try {
    var fallback = new URL('/favicon.ico', document.location.origin).href;
    if (fallback.length <= 2048 && state.candidates.indexOf(fallback) === -1) {
      state.candidates.push(fallback);
    }
  } catch (_) {}

  function next() {
    if (state.index >= state.candidates.length) {
      state.done = true;
      return;
    }
    var image = new Image();
    image.decoding = 'async';
    image.onload = function() {
      try {
        var width = image.naturalWidth;
        var height = image.naturalHeight;
        if (!Number.isFinite(width) || !Number.isFinite(height) ||
            width < 1 || height < 1 || width > 16384 || height > 16384) {
          next();
          return;
        }
        var canvas = document.createElement('canvas');
        canvas.width = 32;
        canvas.height = 32;
        var context = canvas.getContext('2d', {alpha: true, willReadFrequently: true});
        if (!context) {
          state.done = true;
          return;
        }
        context.clearRect(0, 0, 32, 32);
        var scale = Math.min(32 / width, 32 / height);
        var drawWidth = Math.max(1, Math.round(width * scale));
        var drawHeight = Math.max(1, Math.round(height * scale));
        context.drawImage(
          image,
          Math.floor((32 - drawWidth) / 2),
          Math.floor((32 - drawHeight) / 2),
          drawWidth,
          drawHeight
        );
        var bytes = context.getImageData(0, 0, 32, 32).data;
        var binary = '';
        for (var k = 0; k < bytes.length; k++) binary += String.fromCharCode(bytes[k]);
        state.rgba = btoa(binary);
        state.done = true;
      } catch (_) {
        next();
      }
    };
    image.onerror = next;
    image.src = state.candidates[state.index++];
  }
  next();
  return null;
})()"#;

// Capture native string/DOM intrinsics before page script can replace them.
// The locked function returns one primitive string: truncation flag + at most
// the requested number of UTF-16 units. This gives native JSON conversion a
// calculable bound and never traverses a page-controlled object/toJSON hook.
pub(super) const EXTRACT_HTML_BOOTSTRAP_JS: &str = r#"(function(){
  'use strict';
  var key = '__zephium_extract_html_v1__';
  if (Object.prototype.hasOwnProperty.call(globalThis, key)) return;
  try {
    var apply = Reflect.apply;
    var slice = String.prototype.slice;
    var descriptor = Object.getOwnPropertyDescriptor(Element.prototype, 'outerHTML');
    var getter = descriptor && descriptor.get;
    if (typeof apply !== 'function' || typeof slice !== 'function' || typeof getter !== 'function') return;
    var extract = function(max) {
      try {
        var root = document.documentElement;
        var html = root ? apply(getter, root, []) : '';
        if (typeof html !== 'string') return null;
        return (html.length > max ? '1' : '0') + apply(slice, html, [0, max]);
      } catch (_) { return null; }
    };
    Object.defineProperty(globalThis, key, {
      value: extract, writable: false, configurable: false, enumerable: false
    });
  } catch (_) {}
})()"#;

pub(super) const MAX_HTML_CHARS: usize = 2 * 1024 * 1024;
// JSON may encode each UTF-16 unit as six ASCII bytes (`\uXXXX`), plus the
// one-unit flag and surrounding quotes. The bootstrap enforces this before
// the platform creates the callback string.
pub(super) const MAX_HTML_RESULT_BYTES: usize = (MAX_HTML_CHARS + 1) * 6 + 2;
pub(super) const EXTRACT_HTML_JS: &str = "(function(){try{var f=globalThis.__zephium_extract_html_v1__;return typeof f==='function'?f(__MAX__):null}catch(_){return null}})()";

pub(super) fn decode_favicon_eval_result(result: &str) -> Option<Vec<u8>> {
    // Wry returns a JSON serialization of the primitive JavaScript string.
    // Foundation is allowed to spell every base64 solidus as `\/`, while
    // JavaScriptCore/Chromium commonly leave it unescaped. Bound the raw JSON
    // before parsing, then require the exact canonical base64 payload and
    // fixed decoded raster. This accepts both native spellings without
    // widening the callback to objects or attacker-sized strings.
    const MAX_RESULT_BYTES: usize = zephium_core::icon::RGBA32_BASE64_BYTES * 2 + 2;
    if result.len() > MAX_RESULT_BYTES {
        return None;
    }
    let encoded = serde_json::from_str::<String>(result).ok()?;
    zephium_core::icon::decode_rgba32(&encoded)
}

fn style_script(css: &str) -> UserScript {
    // WebView2 runs document-start scripts before <html> exists; WebKit does
    // not. The observer path injects the instant the root appears.
    let source = format!(
        "(function(){{var css={};function add(){{var s=document.createElement('style');s.textContent=css;(document.head||document.documentElement).appendChild(s)}}if(document.head||document.documentElement){{add()}}else{{new MutationObserver(function(_,o){{if(document.documentElement){{o.disconnect();add()}}}}).observe(document,{{childList:true}})}}}})()",
        serde_json::to_string(css).unwrap_or_default()
    );
    UserScript {
        source,
        world: World::Page,
        at_start: true,
    }
}

impl EngineHost {
    pub(super) fn scripts_for(&self, partition: Partition) -> Vec<UserScript> {
        let mut out = Vec::new();
        for scope in [
            ContentScope::Global,
            ContentScope::Profile(partition.profile()),
        ] {
            if let Some(content) = self.user_content.get(&scope) {
                out.extend(content.scripts.iter().cloned());
                out.extend(content.styles.iter().map(|css| style_script(css)));
            }
        }
        out
    }

    pub(crate) fn set_user_content(&mut self, scope: ContentScope, content: UserContent) {
        self.user_content.insert(scope, content);
        // scripts and shortcuts are baked in at build; a stale spare lies
        self.spare = None;
    }

    pub(crate) fn set_shortcuts(&mut self, shortcuts: Vec<Shortcut>) {
        self.shortcuts = shortcuts;
        self.spare = None;
    }

    pub(crate) fn set_content_rules(&mut self, _profile: ProfileId, _compiled: String) {
        // Lands with the blocker: WKContentRuleListStore on macOS,
        // WebResourceRequested on Windows, UserContentFilter on GTK.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discard_bootstrap_bounds_and_attests_shadow_root_observation() {
        for required in [
            "MAX_SHADOW_ROOTS = 256",
            "Element.prototype",
            "originalAttachShadow",
            "elementProto.attachShadow !== wrappedAttachShadow",
            "rememberShadowRoot",
            "shadowRoots.length >= MAX_SHADOW_ROOTS",
            "DocumentFragment.prototype.querySelectorAll",
            "template[shadowrootmode]",
            "collect('input,textarea,select')",
            "collect('[contenteditable]",
            "collect('audio,video')",
        ] {
            assert!(
                DISCARD_SAFETY_BOOTSTRAP_JS.contains(required),
                "discard bootstrap lost required shadow-root invariant: {required}"
            );
        }
    }

    #[test]
    fn html_extraction_uses_locked_intrinsics_and_a_bounded_primitive() {
        for required in [
            "var apply = Reflect.apply",
            "var slice = String.prototype.slice",
            "Object.getOwnPropertyDescriptor(Element.prototype, 'outerHTML')",
            "writable: false, configurable: false",
            "html.length > max ? '1' : '0'",
            "apply(slice, html, [0, max])",
        ] {
            assert!(
                EXTRACT_HTML_BOOTSTRAP_JS.contains(required),
                "HTML bootstrap lost native-bound invariant: {required}"
            );
        }
        assert!(!EXTRACT_HTML_JS.contains("return {"));
        assert_eq!(MAX_HTML_RESULT_BYTES, (MAX_HTML_CHARS + 1) * 6 + 2);
    }

    #[test]
    fn page_print_guard_locks_all_known_scripted_print_lookup_paths() {
        for required in [
            "apply(defineProperty, Object, [Window.prototype, 'print'",
            "apply(defineProperty, Object, [globalThis, 'print'",
            "apply(getOwnPropertyDescriptor, Object, [Document.prototype, 'execCommand'])",
            "apply(defineProperty, Object, [Document.prototype, 'execCommand'",
            "apply(defineProperty, Object, [document, 'execCommand'",
            "primitive = apply(string, undefined, [command])",
            "apply(trim, primitive",
            "apply(toLowerCase",
            "normalized === 'print'",
            "return apply(execCommand, this, [primitive",
            "writable: false",
            "configurable: false",
        ] {
            assert!(
                crate::PAGE_PRINT_DENY_SCRIPT.contains(required),
                "page print guard lost required invariant: {required}"
            );
        }
        assert!(!crate::PAGE_PRINT_DENY_SCRIPT.contains("window.print()"));
        assert!(!crate::PAGE_PRINT_DENY_SCRIPT.contains("apply(execCommand, this, arguments)"));
    }

    #[test]
    fn page_print_guard_coerces_exec_command_once_before_delegating() {
        // A hostile object may alternate its string conversion between a
        // harmless command and `print`. Authorize and delegate the exact same
        // captured primitive so Web IDL cannot invoke it a second time.
        assert_eq!(
            crate::PAGE_PRINT_DENY_SCRIPT
                .matches("apply(string, undefined, [command])")
                .count(),
            1
        );
        assert!(crate::PAGE_PRINT_DENY_SCRIPT
            .contains("return apply(execCommand, this, [primitive, arguments[1]"));
    }

    #[test]
    fn favicon_script_bounds_page_mutable_state_before_returning_it() {
        assert!(FAVICON_JS.contains("value.length === 5464"));
        assert!(FAVICON_JS.contains("[A-Za-z0-9+/]{5462}=="));
        assert!(FAVICON_JS.contains("var rgba = state.rgba;"));
        assert!(FAVICON_JS.contains("if (validRgba(rgba)) return rgba;"));
        assert!(!FAVICON_JS.contains("return state.rgba"));
        let length_check = FAVICON_JS.find("value.length === 5464").unwrap();
        let host_return = FAVICON_JS.find("return rgba").unwrap();
        assert!(length_check < host_return);
        assert!(!FAVICON_JS.contains("return {"));
        assert!(!FAVICON_JS.contains("rgba: state.rgba"));
    }

    #[test]
    fn favicon_callback_accepts_bounded_native_json_spellings_of_the_canonical_string() {
        let rgba: Vec<u8> = (0..zephium_core::icon::RGBA32_BYTES)
            .map(|index| (index % 251) as u8)
            .collect();
        let chrome = zephium_core::icon::chrome_value(&rgba).unwrap();
        let encoded = chrome
            .strip_prefix(zephium_core::icon::RGBA32_PREFIX)
            .unwrap();
        assert!(encoded.contains('/'));
        let serialized = serde_json::to_string(encoded).unwrap();
        let foundation_serialized = serialized.replace('/', "\\/");
        assert_eq!(decode_favicon_eval_result(&serialized), Some(rgba.clone()));
        assert_eq!(
            decode_favicon_eval_result(&foundation_serialized),
            Some(rgba)
        );
        assert!(decode_favicon_eval_result("null").is_none());
        assert!(decode_favicon_eval_result(encoded).is_none());
        assert!(decode_favicon_eval_result(&format!("\"{encoded}x\"")).is_none());
        assert!(decode_favicon_eval_result(&format!(
            "\"{}\"",
            "A".repeat(zephium_core::icon::RGBA32_BASE64_BYTES * 2 + 1)
        ))
        .is_none());
    }

    #[test]
    fn favicon_script_rebuilds_candidates_once_after_document_completion() {
        let ready_state = FAVICON_JS
            .find("var readyState = document.readyState")
            .unwrap();
        let completed_retry = FAVICON_JS.find("completeRebuildUsed === false").unwrap();
        let early_terminal_return = FAVICON_JS.find("return null").unwrap();
        let retry_consumed = FAVICON_JS
            .find("state.completeRebuildUsed = rebuildAfterComplete")
            .unwrap();
        let candidate_scan = FAVICON_JS
            .find("document.querySelectorAll('link[rel~=\"icon\"]')")
            .unwrap();
        assert!(ready_state < completed_retry);
        assert!(completed_retry < early_terminal_return);
        assert!(early_terminal_return < retry_consumed);
        assert!(retry_consumed < candidate_scan);
    }
}
