use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Deref;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};
use wry::dpi::{LogicalPosition, LogicalSize, Position, Size};
use wry::{DownloadPolicy, NewWindowResponse, PageLoadEvent, WebView, WebViewBuilder};

use zephium_core::geometry::Rect;
use zephium_core::ids::{ItemId, ProfileId, WindowId};
use zephium_core::navigation;
use zephium_core::ports::engine::{
    ContentScope, DiscardProbeId, EngineEvent, NavigationRequestId, Partition, Shortcut,
    UserContent, UserScript, World,
};
use zephium_core::split::Pane;

#[cfg(target_os = "macos")]
use {
    crate::platform::imp::ContentStage, objc2::rc::Retained, objc2_app_kit::NSView,
    objc2_foundation::MainThreadMarker,
};

#[cfg(target_os = "windows")]
use {
    crate::platform::imp::Stage,
    webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment,
    windows::Win32::Foundation::HWND,
};

thread_local! {
    static HOST: RefCell<Option<EngineHost>> = const { RefCell::new(None) };
    static PENDING: RefCell<VecDeque<QueuedHostTask>> = const { RefCell::new(VecDeque::new()) };
    static HOST_SEALED: Cell<bool> = const { Cell::new(false) };
    #[cfg(target_os = "windows")]
    static PENDING_WINDOWS_CLEANUP_DEBTS: RefCell<Vec<(ProfileId, wry::WebView2CleanupDebt)>> =
        const { RefCell::new(Vec::new()) };
    #[cfg(target_os = "windows")]
    static WINDOWS_CLEANUP_INVARIANT_FAILED: Cell<bool> = const { Cell::new(false) };
}

type HostTask = Box<dyn FnOnce(&mut EngineHost)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum HostTaskPriority {
    Normal,
    #[cfg(target_os = "windows")]
    Maintenance,
    Observation,
    Lifecycle,
    Close,
    ProfileErasure,
    Shutdown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HostTaskKey {
    // A renderer death supersedes a pending source observation for the same
    // view; an explicit close supersedes both. Priority prevents a later
    // lower-level callback from replacing the stronger transition.
    View(ItemId),
    #[cfg(target_os = "windows")]
    Profile(ProfileId, crate::platform::imp::BrowserProcessGeneration),
    #[cfg(target_os = "windows")]
    Suspend(ItemId),
}

struct QueuedHostTask {
    priority: HostTaskPriority,
    key: Option<HostTaskKey>,
    task: HostTask,
}

const GAP: f64 = 8.0;
const NORMAL_PENDING_HOST_TASK_CAPACITY: usize = 960;
// Normal UI work cannot consume this lifecycle band. One keyed native fact
// per maximum view/profile plus the bounded suspend batch stays below this
// ceiling, even during a nested native message-loop pump. One additional slot
// per maximum live profile is reserved for non-coalescible erasure tasks, and
// the final physical slot is reserved exclusively for shutdown.
const PENDING_HOST_TASK_CAPACITY: usize = 4096;
const NON_SHUTDOWN_PENDING_HOST_TASK_CAPACITY: usize = PENDING_HOST_TASK_CAPACITY - 1;
const PROFILE_ERASURE_PENDING_HOST_TASK_CAPACITY: usize =
    zephium_core::session::MAX_SESSION_PROFILES;
const NON_PROFILE_ERASURE_PENDING_HOST_TASK_CAPACITY: usize =
    NON_SHUTDOWN_PENDING_HOST_TASK_CAPACITY - PROFILE_ERASURE_PENDING_HOST_TASK_CAPACITY;
#[cfg(all(unix, not(target_os = "macos")))]
const MAX_LINUX_RETAINED_DATA_MANAGERS: usize = zephium_core::session::MAX_SESSION_PROFILES * 2;
#[cfg(target_os = "windows")]
const MAX_CONCURRENT_SUSPENDS: usize = 8;
#[cfg(target_os = "windows")]
const MAX_WINDOWS_CLEANUP_DEBTS: usize =
    zephium_core::session::MAX_SESSION_ITEMS + zephium_core::session::MAX_SESSION_PROFILES;
// The app's current hard live-view budget is 32. This independent native
// ceiling leaves sixteen emergency slots: enough to reconstruct all eight
// visible panes, retain one warm spare, and still carry bounded teardown debt.
// Every incomplete cleanup debt consumes a slot, even after Controller::Close
// succeeds: a stuck subclass/HWND is still a native resource. Retries can
// therefore never create around any failed teardown obligation.
const MAX_NATIVE_VIEW_RESOURCES: usize = 48;
const _: () = assert!(MAX_NATIVE_VIEW_RESOURCES >= 32 + 1 + 8);
// A distinct WebView2 environment/UDF owns its own browser/network process
// group. This is separate from the 64-profile persistence format limit and
// from the per-view ceiling above: retaining zero-view environments for every
// historical profile must not turn profile count into unbounded process/RAM
// growth. Exact idle-group retirement is future profile-UX work; until then,
// refuse construction before a ninth native group can be created.
#[cfg(any(target_os = "windows", test))]
const MAX_NATIVE_PROFILE_PROCESS_GROUPS: usize = 8;
static PENDING_OVERFLOW_LOGS_REMAINING: AtomicUsize = AtomicUsize::new(4);

#[derive(Default)]
struct NativeViewReservations {
    in_construction: usize,
}

impl NativeViewReservations {
    fn try_reserve(&mut self, already_owned: usize) -> Result<bool, ()> {
        let Some(total) = already_owned.checked_add(self.in_construction) else {
            return Err(());
        };
        if total >= MAX_NATIVE_VIEW_RESOURCES {
            return Ok(false);
        }
        self.in_construction = self.in_construction.checked_add(1).ok_or(())?;
        Ok(true)
    }

    fn release(&mut self) -> Result<(), ()> {
        self.in_construction = self.in_construction.checked_sub(1).ok_or(())?;
        Ok(())
    }

    #[cfg(test)]
    fn in_construction(&self) -> usize {
        self.in_construction
    }
}

fn owned_native_view_resources(
    live_views: usize,
    has_warm_spare: bool,
    cleanup_debts: usize,
) -> Option<usize> {
    live_views
        .checked_add(usize::from(has_warm_spare))?
        .checked_add(cleanup_debts)
}

#[cfg(any(target_os = "windows", test))]
fn profile_process_group_capacity_allows(
    existing: impl IntoIterator<Item = ProfileId>,
    requested: ProfileId,
) -> bool {
    let mut distinct = HashSet::with_capacity(MAX_NATIVE_PROFILE_PROCESS_GROUPS + 1);
    for profile in existing {
        if profile == requested {
            return true;
        }
        distinct.insert(profile);
    }
    distinct.len() < MAX_NATIVE_PROFILE_PROCESS_GROUPS
}

// Installed before every page script. It records otherwise non-enumerable
// unload/audio/capture state, while the query itself directly compares form
// controls with their defaults. This is a data-loss guard, not a capability:
// it exposes no Rust/native object and returns only a fixed boolean schema.
// Any hook replacement, excessive state, child frame, or exception becomes
// `uncertain`, which vetoes discard.
const DISCARD_SAFETY_BOOTSTRAP_JS: &str = r#"(function(){
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

const DISCARD_SAFETY_QUERY_JS: &str = r#"(function(){
  'use strict';
  try {
    var query = globalThis.__zephium_discard_safety_v1__;
    if (typeof query !== 'function') throw new Error('missing safety tracker');
    return query();
  } catch (_) {
    return 256;
  }
})()"#;

fn renderer_report_allows_discard(result: &str) -> bool {
    // Safe means the primitive mask contains only the ready bit. Reject every
    // alternate number/string/object representation without parsing.
    result == "1"
}

fn discard_probe_identity_matches(
    current_permit: &EventPermit,
    current_navigation: &NavigationEpochTracker,
    requested_permit: &EventPermit,
    requested_navigation: &NavigationEpochTracker,
    requested_epoch: NavigationEpoch,
) -> bool {
    current_permit.same_generation(requested_permit)
        && current_navigation.same_generation(requested_navigation)
        && requested_navigation.is_current(requested_epoch)
}

// Fetch and decode entirely inside the untrusted site renderer. The callback
// surface is a fixed 32x32 RGBA raster; privileged Rust/chrome never parse a
// page-controlled image container. Calling this script again polls the
// renderer-owned asynchronous Image decode without adding an IPC bridge.
const FAVICON_JS: &str = r#"(function(){
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
const EXTRACT_HTML_BOOTSTRAP_JS: &str = r#"(function(){
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

const MAX_HTML_CHARS: usize = 2 * 1024 * 1024;
// JSON may encode each UTF-16 unit as six ASCII bytes (`\uXXXX`), plus the
// one-unit flag and surrounding quotes. The bootstrap enforces this before
// the platform creates the callback string.
const MAX_HTML_RESULT_BYTES: usize = (MAX_HTML_CHARS + 1) * 6 + 2;
const EXTRACT_HTML_JS: &str = "(function(){try{var f=globalThis.__zephium_extract_html_v1__;return typeof f==='function'?f(__MAX__):null}catch(_){return null}})()";

fn bounded_title(title: &str) -> String {
    zephium_core::item::sanitize_page_title(title)
}

fn decode_favicon_eval_result(result: &str) -> Option<Vec<u8>> {
    if result.len() != zephium_core::icon::RGBA32_BASE64_BYTES + 2 {
        return None;
    }
    let encoded = result
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))?;
    zephium_core::icon::decode_rgba32(encoded)
}

pub(crate) fn install(
    parent: RawWindowHandle,
    data_root: PathBuf,
    sink: crate::EngineEventIngressSink,
) -> Result<(), String> {
    HOST_SEALED.with(|sealed| sealed.set(false));
    PENDING.with(|pending| {
        pending
            .try_borrow_mut()
            .map_err(|_| "engine host pending queue is re-entrantly borrowed".to_owned())?
            .clear();
        Ok::<(), String>(())
    })?;
    // WebView2 needs a user-data folder even for InPrivate controllers. It
    // must never be the privileged Tauri chrome's folder, and stale runtime
    // metadata must not accumulate across browser sessions.
    #[cfg(not(target_os = "macos"))]
    let profiles_root = crate::erasure::canonical_owned_root(&data_root.join("profiles"))
        .map_err(|error| format!("cannot secure engine profile root: {error}"))?;
    #[cfg(target_os = "windows")]
    let private_runtime = zephium_core::webview2::RuntimeGeneration::prepare(
        &data_root.join("private-runtime"),
        zephium_core::webview2::RuntimeGenerationKind::RawPrivate,
    )
    .map_err(|error| format!("cannot create private WebView2 generation: {error}"))?;
    #[cfg(target_os = "macos")]
    let _ = &data_root;
    HOST.with(|cell| {
        let mut host = cell
            .try_borrow_mut()
            .map_err(|_| "engine host is re-entrantly borrowed during install".to_owned())?;
        if host.is_some() {
            return Err("engine host is already installed on this thread".to_owned());
        }
        *host = Some(EngineHost {
            parent: ParentHandle(parent),
            #[cfg(not(target_os = "macos"))]
            profiles_root,
            #[cfg(target_os = "windows")]
            private_runtime,
            views: HashMap::new(),
            native_view_reservations: NativeViewReservations::default(),
            native_resource_accounting_failed: false,
            navigation_snapshots: HashMap::new(),
            partitions: HashMap::new(),
            profile_persistence_classes: HashMap::new(),
            spare: None,
            user_content: HashMap::new(),
            shortcuts: Vec::new(),
            stages: HashMap::new(),
            #[cfg(target_os = "macos")]
            macos_ephemeral_data_stores: HashMap::new(),
            #[cfg(target_os = "windows")]
            hidden: std::collections::HashSet::new(),
            #[cfg(target_os = "windows")]
            dormant: std::collections::HashSet::new(),
            #[cfg(target_os = "windows")]
            desired_dormant: std::collections::HashSet::new(),
            #[cfg(target_os = "windows")]
            suspending: std::collections::HashSet::new(),
            #[cfg(target_os = "windows")]
            suspend_failed: std::collections::HashSet::new(),
            #[cfg(not(target_os = "macos"))]
            web_contexts: HashMap::new(),
            #[cfg(all(unix, not(target_os = "macos")))]
            linux_data_managers: HashMap::new(),
            #[cfg(all(unix, not(target_os = "macos")))]
            linux_unverifiable_data_managers: HashSet::new(),
            #[cfg(target_os = "windows")]
            browser_version_observers: HashMap::new(),
            #[cfg(target_os = "windows")]
            environments: HashMap::new(),
            #[cfg(target_os = "windows")]
            browser_processes: HashMap::new(),
            #[cfg(target_os = "windows")]
            browser_process_exit_observers: HashMap::new(),
            #[cfg(target_os = "windows")]
            pending_profile_recovery: HashMap::new(),
            #[cfg(target_os = "windows")]
            exiting_browser_processes: HashSet::new(),
            #[cfg(target_os = "windows")]
            unverifiable_browser_processes: HashSet::new(),
            #[cfg(target_os = "windows")]
            construction_unproven: HashSet::new(),
            #[cfg(target_os = "windows")]
            unproven_browser_processes: HashMap::new(),
            #[cfg(target_os = "windows")]
            unproven_environments: HashMap::new(),
            #[cfg(target_os = "windows")]
            windows_cleanup_debts: HashMap::new(),
            #[cfg(target_os = "windows")]
            windows_cleanup_invariant_failed: false,
            erasure_tombstones: HashSet::new(),
            erasure_attempts: HashMap::new(),
            sink: Sink(sink),
        });
        Ok(())
    })
}

// WebView2 construction pumps the Windows message loop. A nested main-thread
// dispatch must not re-enter the host, but dropping it can lose a close,
// navigation or security transition. Queue it and drain after the outer
// operation releases the mutable borrow.
/// Admit work whose loss is explicitly fail-safe and observed by a later
/// retry/timeout. Authoritative mutations must use `try_with` (or a stronger
/// priority-specific variant) and handle `false`.
pub(crate) fn best_effort_with<F>(f: F)
where
    F: FnOnce(&mut EngineHost) + 'static,
{
    let _ = with_priority(HostTaskPriority::Normal, None, f);
}

pub(crate) fn try_with<F>(f: F) -> bool
where
    F: FnOnce(&mut EngineHost) + 'static,
{
    with_priority(HostTaskPriority::Normal, None, f)
}

#[cfg(test)]
pub(crate) fn make_unavailable_for_test() {
    HOST_SEALED.with(|sealed| sealed.set(false));
    HOST.with(|host| *host.borrow_mut() = None);
    PENDING.with(|pending| pending.borrow_mut().clear());
}

/// Release main-thread-bound WebsiteDataManager proof handles only after the
/// exact erasure attempt that used them verified disk absence. Failure to
/// enqueue this housekeeping closure is safe: it retains proof instead of
/// forgetting it.
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn release_linux_erasure_obligations(profile: ProfileId, attempt: Arc<AtomicBool>) {
    let _ = try_with(move |host| {
        let exact_settled_attempt =
            linux_erasure_release_matches(host.erasure_attempts.get(&profile), &attempt);
        if exact_settled_attempt {
            host.linux_data_managers.remove(&profile);
        }
    });
}

/// Release a private profile's last host-owned WKWebsiteDataStore handle only
/// after the exact native erasure attempt has positively settled. A failed,
/// timed-out, or superseded callback must retain the handle so a retry cannot
/// mistake forgotten in-memory state for verified deletion.
#[cfg(target_os = "macos")]
pub(crate) fn release_macos_erasure_obligation(profile: ProfileId, attempt: Arc<AtomicBool>) {
    let _ = try_with(move |host| {
        if macos_erasure_release_matches(host.erasure_attempts.get(&profile), &attempt) {
            host.macos_ephemeral_data_stores.remove(&profile);
        }
    });
}

#[cfg(any(target_os = "macos", test))]
fn macos_erasure_release_matches(
    current: Option<&Arc<AtomicBool>>,
    completed: &Arc<AtomicBool>,
) -> bool {
    current.is_some_and(|current| {
        Arc::ptr_eq(current, completed) && !completed.load(Ordering::Acquire)
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_erasure_release_matches(
    current: Option<&Arc<AtomicBool>>,
    completed: &Arc<AtomicBool>,
) -> bool {
    current.is_some_and(|current| {
        Arc::ptr_eq(current, completed) && !completed.load(Ordering::Acquire)
    })
}

/// Profile retirement owns a dedicated bounded band above every ordinary and
/// native-lifecycle task. It intentionally has no coalescing key: replacing a
/// duplicate would drop that request's exactly-once completion obligation.
pub(crate) fn try_with_profile_erasure<F>(f: F) -> bool
where
    F: FnOnce(&mut EngineHost) + 'static,
{
    with_priority(HostTaskPriority::ProfileErasure, None, f)
}

pub(crate) fn try_with_close<F>(id: ItemId, f: F) -> bool
where
    F: FnOnce(&mut EngineHost) + 'static,
{
    with_priority(HostTaskPriority::Close, Some(HostTaskKey::View(id)), f)
}

fn with_observation<F>(id: ItemId, f: F)
where
    F: FnOnce(&mut EngineHost) + 'static,
{
    let _ = with_priority(
        HostTaskPriority::Observation,
        Some(HostTaskKey::View(id)),
        f,
    );
}

fn with_renderer_exit<F>(id: ItemId, f: F)
where
    F: FnOnce(&mut EngineHost) + 'static,
{
    let _ = with_priority(HostTaskPriority::Lifecycle, Some(HostTaskKey::View(id)), f);
}

#[cfg(target_os = "windows")]
fn with_profile_exit<F>(
    profile: ProfileId,
    generation: crate::platform::imp::BrowserProcessGeneration,
    f: F,
) where
    F: FnOnce(&mut EngineHost) + 'static,
{
    let _ = with_priority(
        HostTaskPriority::Lifecycle,
        Some(HostTaskKey::Profile(profile, generation)),
        f,
    );
}

#[cfg(target_os = "windows")]
fn with_suspend_result<F>(id: ItemId, f: F)
where
    F: FnOnce(&mut EngineHost) + 'static,
{
    let _ = with_priority(
        HostTaskPriority::Maintenance,
        Some(HostTaskKey::Suspend(id)),
        f,
    );
}

fn with_priority<F>(priority: HostTaskPriority, key: Option<HostTaskKey>, f: F) -> bool
where
    F: FnOnce(&mut EngineHost) + 'static,
{
    enum Access {
        Executed,
        Reentrant,
        Unavailable,
    }

    if HOST_SEALED.with(Cell::get) {
        return false;
    }

    let mut task: Option<HostTask> = Some(Box::new(f));
    let access = HOST.with(|cell| match cell.try_borrow_mut() {
        Ok(mut slot) => {
            let Some(host) = slot.as_mut() else {
                return Access::Unavailable;
            };
            if priority == HostTaskPriority::Shutdown {
                // The host exists and the barrier is about to execute. Seal
                // before native teardown so a callback pumped by teardown
                // cannot recreate a controller behind it.
                HOST_SEALED.with(|sealed| sealed.set(true));
            }
            if let Some(task) = task.take() {
                task(host);
            }
            #[cfg(target_os = "windows")]
            host.retry_windows_cleanup_debts(1);
            Access::Executed
        }
        Err(_) => Access::Reentrant,
    });
    match access {
        Access::Unavailable => return false,
        Access::Reentrant => {
            return PENDING.with(|pending| {
                let Ok(mut pending) = pending.try_borrow_mut() else {
                    // Queue mutation can run destructors for replaced work.
                    // If one reenters here, reject this admission explicitly;
                    // a RefCell panic would abort production builds.
                    eprintln!("engine: rejected recursively borrowed host queue admission");
                    return false;
                };
                let Some(task) = task.take() else {
                    HOST_SEALED.with(|sealed| sealed.set(true));
                    return false;
                };
                let queued = QueuedHostTask {
                    priority,
                    key,
                    task,
                };
                let accepted = enqueue_pending(&mut pending, queued);
                if accepted && priority == HostTaskPriority::Shutdown {
                    // Non-shutdown admission stops one slot early, so a first
                    // shutdown is guaranteed a physical queue slot. Seal only
                    // after that barrier has actually been admitted.
                    HOST_SEALED.with(|sealed| sealed.set(true));
                }
                if !accepted
                    && PENDING_OVERFLOW_LOGS_REMAINING
                        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                            value.checked_sub(1)
                        })
                        .is_ok()
                {
                    // Reentrant WebView2 construction pumps the native loop. A
                    // hostile renderer must not turn that into an unbounded queue;
                    // blocking here would deadlock the same main-thread borrow.
                    eprintln!("engine: dropping reentrant native task at bounded capacity");
                }
                accepted
            });
        }
        Access::Executed => {}
    }

    loop {
        let queued = PENDING.with(|pending| {
            pending
                .try_borrow_mut()
                .map(|mut pending| pending.pop_front())
        });
        let queued = match queued {
            Ok(Some(queued)) => queued,
            Ok(None) => break,
            Err(_) => {
                // An authoritative drain cannot be resumed in an unknown
                // ordering state. Seal ingress rather than aborting or
                // executing accepted work out of order.
                HOST_SEALED.with(|sealed| sealed.set(true));
                return false;
            }
        };
        let mut queued = Some(queued);
        let accessed = HOST.with(|cell| {
            let Ok(mut slot) = cell.try_borrow_mut() else {
                return false;
            };
            if let Some(host) = slot.as_mut() {
                if let Some(queued) = queued.take() {
                    (queued.task)(host);
                    #[cfg(target_os = "windows")]
                    host.retry_windows_cleanup_debts(1);
                }
                true
            } else {
                false
            }
        });
        if !accessed {
            // Preserve the already-admitted task if the host was unexpectedly
            // still borrowed, but seal all new ingress because its ordering
            // relative to the active callback can no longer be proved.
            if let Some(queued) = queued.take() {
                let _ = PENDING.with(|pending| {
                    pending
                        .try_borrow_mut()
                        .map(|mut pending| pending.push_front(queued))
                });
            }
            HOST_SEALED.with(|sealed| sealed.set(true));
            return false;
        }
    }
    true
}

fn enqueue_pending(pending: &mut VecDeque<QueuedHostTask>, queued: QueuedHostTask) -> bool {
    let contains_shutdown = pending
        .iter()
        .any(|task| task.priority == HostTaskPriority::Shutdown);
    if contains_shutdown {
        // Nothing may be admitted behind the teardown barrier. `HOST_SEALED`
        // enforces this at the public ingress; keep the queue primitive safe
        // when exercised directly as well.
        return false;
    }
    if let Some(key) = queued.key {
        if let Some(back) = pending.back().filter(|task| task.key == Some(key)) {
            // Coalesce only an adjacent callback. Crossing an intervening host
            // task can invert native facts around a create/navigation (most
            // critically, a profile-process exit around a profile rebuild).
            if queued.priority < back.priority {
                return true;
            }
            pending.pop_back();
            pending.push_back(queued);
            return true;
        }
    }
    if queued.priority == HostTaskPriority::Normal
        && pending.len() >= NORMAL_PENDING_HOST_TASK_CAPACITY
    {
        return false;
    }
    let capacity = match queued.priority {
        HostTaskPriority::Shutdown => PENDING_HOST_TASK_CAPACITY,
        HostTaskPriority::ProfileErasure => NON_SHUTDOWN_PENDING_HOST_TASK_CAPACITY,
        _ => NON_PROFILE_ERASURE_PENDING_HOST_TASK_CAPACITY,
    };
    if pending.len() < capacity {
        pending.push_back(queued);
        return true;
    }

    // At this priority class's bounded ceiling only, retain the newest fact
    // for the same native object even across intervening work. This overload
    // escape hatch prevents one object from crowding itself out; ordinary
    // operation above preserves every ordering barrier.
    if let Some(key) = queued.key {
        if let Some(index) = pending.iter().rposition(|task| task.key == Some(key)) {
            if queued.priority < pending[index].priority {
                return true;
            }
            pending.remove(index);
            pending.push_back(queued);
            return true;
        }
    }

    let replace = match queued.priority {
        HostTaskPriority::Normal => None,
        #[cfg(target_os = "windows")]
        HostTaskPriority::Maintenance => pending
            .iter()
            .position(|task| task.priority < queued.priority),
        HostTaskPriority::Observation | HostTaskPriority::Lifecycle | HostTaskPriority::Close => {
            pending
                .iter()
                .position(|task| task.priority < queued.priority)
        }
        // Its dedicated band guarantees the bounded first cohort. Past that
        // point rejecting this attempt is safer than dropping an already
        // admitted close/lifecycle obligation; the public retirement gate has
        // already made the requested profile inaccessible.
        HostTaskPriority::ProfileErasure => None,
        // The non-shutdown ceiling guarantees this arm cannot be reached for
        // the first shutdown barrier.
        HostTaskPriority::Shutdown => None,
    };
    if let Some(index) = replace {
        pending.remove(index);
        pending.push_back(queued);
        return true;
    }

    false
}

pub(crate) fn shutdown(done: Box<dyn FnOnce(bool) + Send>) {
    // Keep completion in the host task itself: `with` may queue during a
    // reentrant WebView2 construction pump, and acknowledging before that
    // queued task runs would let the process exit with live controllers.
    let completion = Arc::new(std::sync::Mutex::new(Some(done)));
    let queued_completion = completion.clone();
    let admitted = with_priority(HostTaskPriority::Shutdown, None, move |host| {
        let Some(done) = queued_completion
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        else {
            return;
        };
        #[cfg(target_os = "windows")]
        {
            let (browser_processes, process_provenance_valid) = host.shutdown();
            let private_runtime_cleanup = host.private_runtime.cleanup_ticket();
            let worker_completion = Arc::new(std::sync::Mutex::new(Some(done)));
            let spawn_failure = worker_completion.clone();
            // Environment5, not the main process HANDLE alone, proves every
            // child process and UDF resource has been released. Keep the app
            // shutdown barrier open for one globally bounded proof wait.
            let spawned = std::thread::Builder::new()
                .name("zephium-webview2-shutdown".into())
                .spawn(move || {
                    let finish = |clean| {
                        if let Some(done) = worker_completion
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .take()
                        {
                            done(clean);
                        }
                    };
                    if !process_provenance_valid
                        || !crate::platform::imp::wait_for_browser_process_shutdown(
                            browser_processes,
                            std::time::Duration::from_secs(5),
                        )
                    {
                        eprintln!(
                            "privacy: WebView2 full process-group shutdown could not be proven"
                        );
                        finish(false);
                        return;
                    }
                    let cleaned = match private_runtime_cleanup.cleanup_after_proven_exit() {
                        Ok(()) => true,
                        Err(error) => {
                            eprintln!(
                                "privacy: could not remove private WebView2 runtime data at {}: {error}",
                                private_runtime_cleanup.root().display()
                            );
                            false
                        }
                    };
                    finish(cleaned);
                });
            if let Err(error) = spawned {
                eprintln!("shutdown: could not start WebView2 cleanup worker: {error}");
                if let Some(done) = spawn_failure
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take()
                {
                    done(false);
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            done(host.shutdown());
        }
    });
    if !admitted {
        if let Some(done) = completion
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            done(false);
        }
    }
}

#[derive(Clone)]
struct Sink(crate::EngineEventIngressSink);

impl Sink {
    fn emit(&self, ev: EngineEvent) {
        (self.0)(crate::EngineEventIngress::global(ev));
    }

    fn emit_for(&self, token: Arc<AtomicBool>, ev: EngineEvent) {
        (self.0)(crate::EngineEventIngress::for_item(ev, token));
    }
}

/// Every native WebView owns one permit identity. A normal view is bound at
/// construction; a warm spare may be bound exactly once when it is adopted.
/// Revocation is terminal, so a callback retained by a dropped same-id view
/// can never acquire the token of a replacement view.
#[derive(Clone)]
struct EventPermit {
    state: Arc<Mutex<EventPermitState>>,
}

enum EventPermitState {
    Inactive,
    Bound(Weak<AtomicBool>),
    Revoked,
}

impl EventPermit {
    fn inactive() -> Self {
        Self {
            state: Arc::new(Mutex::new(EventPermitState::Inactive)),
        }
    }

    fn bound(token: &Arc<AtomicBool>) -> Self {
        Self {
            state: Arc::new(Mutex::new(EventPermitState::Bound(Arc::downgrade(token)))),
        }
    }

    fn bind_once(&self, token: &Arc<AtomicBool>) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match *state {
            EventPermitState::Inactive => {
                *state = EventPermitState::Bound(Arc::downgrade(token));
                true
            }
            EventPermitState::Bound(_) | EventPermitState::Revoked => false,
        }
    }

    fn revoke(&self) {
        *self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = EventPermitState::Revoked;
    }

    fn active_token(&self) -> Option<Arc<AtomicBool>> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match &*state {
            EventPermitState::Bound(token) => token
                .upgrade()
                .filter(|token| token.load(Ordering::Acquire)),
            EventPermitState::Inactive | EventPermitState::Revoked => None,
        }
    }

    fn same_generation(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }

    fn matches_token(&self, token: &Arc<AtomicBool>) -> bool {
        self.active_token()
            .is_some_and(|bound| Arc::ptr_eq(&bound, token))
    }

    fn allows_navigation(&self, target: &str) -> bool {
        if !navigation::is_allowed_str(target) {
            return false;
        }
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match &*state {
            EventPermitState::Inactive => target == "about:blank",
            EventPermitState::Bound(token) => token
                .upgrade()
                .is_some_and(|token| token.load(Ordering::Acquire)),
            EventPermitState::Revoked => false,
        }
    }

    fn emit(&self, sink: &Sink, event: EngineEvent) {
        if let Some(token) = self.active_token() {
            sink.emit_for(token, event);
        }
    }
}

/// A native WebView generation can perform many navigations. Generation-only
/// callback checks are therefore insufficient for a warm spare: callbacks
/// queued by its bootstrap `about:blank` load can arrive after the same native
/// object has been bound to a real item. Keep a non-wrapping navigation epoch
/// beside the generation permit and require both identities at every
/// page-state callback boundary.
#[derive(Clone)]
struct NavigationEpochTracker {
    state: Arc<Mutex<NavigationEpochState>>,
}

#[derive(Debug)]
struct NavigationEpochState {
    next: u64,
    current: Option<CurrentNavigation>,
    revoked: bool,
}

#[derive(Clone, Debug)]
struct CurrentNavigation {
    epoch: NavigationEpoch,
    target: String,
    phase: NavigationPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NavigationEpoch(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NavigationPhase {
    AwaitingStart,
    Started,
    Committed,
}

impl NavigationEpochTracker {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(NavigationEpochState {
                next: 0,
                current: None,
                revoked: false,
            })),
        }
    }

    fn begin(&self, target: &str) -> Option<NavigationEpoch> {
        let target = canonical_navigation_target(target)?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::begin_locked(&mut state, &target)
    }

    fn begin_locked(state: &mut NavigationEpochState, target: &str) -> Option<NavigationEpoch> {
        if state.revoked {
            return None;
        }
        let Some(next) = state.next.checked_add(1) else {
            // A wrapping epoch could make a callback from the first
            // navigation indistinguishable from the newest one. Permanently
            // retire this tracker instead of panicking in a native callback.
            state.revoked = true;
            state.current = None;
            return None;
        };
        state.next = next;
        let epoch = NavigationEpoch(next);
        state.current = Some(CurrentNavigation {
            epoch,
            target: target.to_owned(),
            phase: NavigationPhase::AwaitingStart,
        });
        Some(epoch)
    }

    /// Apply navigation policy without deriving epoch identity from this
    /// callback. Some engines invoke the policy hook for subframes, and none
    /// exposes a portable navigation identifier here. Epochs advance at
    /// explicit host navigation and main-frame page-load start instead.
    fn admit_target(&self, permit: &EventPermit, target: &str) -> bool {
        if !permit.allows_navigation(target) || canonical_navigation_target(target).is_none() {
            return false;
        }
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        !state.revoked
    }

    /// Attribute a page-load event only when its URL is the exact target of
    /// the current epoch. This is the critical warm-spare barrier: a delayed
    /// `about:blank` completion cannot acquire the adopted item's epoch.
    fn observe_load(&self, target: &str, event: &PageLoadEvent) -> Option<NavigationEpoch> {
        let target = canonical_navigation_target(target)?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.revoked {
            return None;
        }
        let current = state.current.as_mut()?;
        match event {
            PageLoadEvent::Started => match current.phase {
                NavigationPhase::AwaitingStart => {
                    if current.target != target {
                        return None;
                    }
                    current.phase = NavigationPhase::Started;
                    Some(current.epoch)
                }
                NavigationPhase::Started => {
                    if current.target == target {
                        return Some(current.epoch);
                    }
                    // A warm spare's only predecessor is `about:blank`.
                    // Never reinterpret its late start as a redirect of the
                    // adopted page after that page has already started.
                    if target == "about:blank" && current.target != "about:blank" {
                        return None;
                    }
                    let epoch = Self::begin_locked(&mut state, &target)?;
                    state.current.as_mut()?.phase = NavigationPhase::Started;
                    Some(epoch)
                }
                NavigationPhase::Committed => {
                    let epoch = Self::begin_locked(&mut state, &target)?;
                    state.current.as_mut()?.phase = NavigationPhase::Started;
                    Some(epoch)
                }
            },
            PageLoadEvent::Finished => {
                if current.target != target {
                    return None;
                }
                current.phase = NavigationPhase::Committed;
                Some(current.epoch)
            }
        }
    }

    fn current(&self) -> Option<NavigationEpoch> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (!state.revoked)
            .then(|| state.current.as_ref().map(|current| current.epoch))
            .flatten()
    }

    fn current_committed(&self) -> Option<NavigationEpoch> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (!state.revoked)
            .then(|| {
                state.current.as_ref().and_then(|current| {
                    (current.phase == NavigationPhase::Committed).then_some(current.epoch)
                })
            })
            .flatten()
    }

    fn committed_snapshot(&self) -> Option<(NavigationEpoch, String)> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.revoked {
            return None;
        }
        let current = state.current.as_ref()?;
        (current.phase == NavigationPhase::Committed)
            .then(|| (current.epoch, current.target.clone()))
    }

    fn matches_committed_snapshot(&self, epoch: NavigationEpoch, target: &str) -> bool {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        !state.revoked
            && state.current.as_ref().is_some_and(|current| {
                current.phase == NavigationPhase::Committed
                    && current.epoch == epoch
                    && current.target == target
            })
    }

    fn is_current(&self, epoch: NavigationEpoch) -> bool {
        self.current() == Some(epoch)
    }

    fn same_generation(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }

    /// Validate the source queried from the native view for a queued source or
    /// history callback. Before commit it must equal the epoch's policy target;
    /// after commit, an allowed same-document History API URL remains in the
    /// same epoch and becomes the new observed target.
    fn observe_source(&self, epoch: NavigationEpoch, source: &str) -> bool {
        let Some(source) = canonical_navigation_target(source) else {
            return false;
        };
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.revoked {
            return false;
        }
        let Some(current) = state.current.as_mut() else {
            return false;
        };
        if current.epoch != epoch {
            return false;
        }
        if current.phase != NavigationPhase::Committed && current.target != source {
            return false;
        }
        current.target = source;
        current.phase = NavigationPhase::Committed;
        true
    }

    fn revoke(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.revoked = true;
        state.current = None;
    }
}

fn canonical_navigation_target(target: &str) -> Option<String> {
    if !navigation::is_allowed_str(target) {
        return None;
    }
    url::Url::parse(target).ok().map(|url| url.to_string())
}

fn navigation_callback_matches(
    view_permit: &EventPermit,
    view_navigation: &NavigationEpochTracker,
    source_permit: &EventPermit,
    source_navigation: &NavigationEpochTracker,
    epoch: NavigationEpoch,
) -> bool {
    view_permit.same_generation(source_permit)
        && view_navigation.same_generation(source_navigation)
        && view_navigation.is_current(epoch)
}

struct ParentHandle(RawWindowHandle);

impl HasWindowHandle for ParentHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // SAFETY: the parent is the app window, which outlives every child
        // content webview created from it.
        Ok(unsafe { WindowHandle::borrow_raw(self.0) })
    }
}

// A prebuilt hidden webview: renderer spawn costs hundreds of ms on weak
// machines, so the next navigation adopts this one and rebinds its id.
struct Spare {
    partition: Partition,
    view: ObservedView,
    id: Rc<Cell<ItemId>>,
}

// Keep native observer registrations adjacent to their WebView and drop them
// first. Platform observers never strongly capture this wrapper or WebView.
struct ObservedView {
    event_permit: EventPermit,
    navigation: NavigationEpochTracker,
    #[cfg(target_os = "windows")]
    _crash_observer: crate::platform::imp::CrashObserver,
    #[cfg(target_os = "windows")]
    _accelerator_registration: Option<crate::platform::imp::AcceleratorRegistration>,
    #[cfg(target_os = "windows")]
    _security_policy: crate::platform::imp::SecurityPolicy,
    _observer: crate::platform::imp::InstalledNavigationObserver,
    #[cfg(target_os = "windows")]
    cleanup_profile: ProfileId,
    view: WebView,
}

impl Drop for ObservedView {
    fn drop(&mut self) {
        // Revoke before native observers and the WebView are dropped. Any
        // callback already queued elsewhere still carries this permit and is
        // rejected even if the shell reuses the same logical ItemId.
        self.event_permit.revoke();
        self.navigation.revoke();
        #[cfg(target_os = "windows")]
        {
            use wry::WebViewExtWindows;
            if let Err(debt) = self.view.close() {
                queue_windows_cleanup_debt(self.cleanup_profile, debt);
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn queue_windows_cleanup_debt(profile: ProfileId, debt: wry::WebView2CleanupDebt) {
    PENDING_WINDOWS_CLEANUP_DEBTS.with(|pending| {
        let Ok(mut pending) = pending.try_borrow_mut() else {
            WINDOWS_CLEANUP_INVARIANT_FAILED.with(|failed| failed.set(true));
            std::mem::forget(debt);
            return;
        };
        if pending.len() >= MAX_WINDOWS_CLEANUP_DEBTS {
            WINDOWS_CLEANUP_INVARIANT_FAILED.with(|failed| failed.set(true));
            std::mem::forget(debt);
            return;
        }
        pending.push((profile, debt));
    });
}

impl ObservedView {
    #[cfg(target_os = "windows")]
    fn close_explicit(mut self) -> Option<wry::WebView2CleanupDebt> {
        use wry::WebViewExtWindows;
        self.event_permit.revoke();
        self.navigation.revoke();
        self.view.close().err()
    }
}

impl Deref for ObservedView {
    type Target = WebView;

    fn deref(&self) -> &Self::Target {
        &self.view
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct NavigationSnapshot {
    url: Option<String>,
    history: Option<(bool, bool)>,
}

#[derive(Debug, PartialEq, Eq)]
enum ObservedUrl {
    Unavailable,
    Allowed(String),
    Forbidden,
}

fn classify_observed_url(url: Option<String>) -> ObservedUrl {
    match url {
        None => ObservedUrl::Unavailable,
        Some(url) if url.is_empty() => ObservedUrl::Unavailable,
        Some(url) if navigation::is_allowed_str(&url) => ObservedUrl::Allowed(url),
        Some(_) => ObservedUrl::Forbidden,
    }
}

fn navigation_observation_events(
    id: ItemId,
    previous: &mut NavigationSnapshot,
    url: Option<String>,
    history: Option<(bool, bool)>,
) -> Vec<EngineEvent> {
    // A single native notification produces at most these two bounded events,
    // and duplicate Source/History/KVO notifications become no-ops.
    let mut events = Vec::with_capacity(2);
    if let Some(url) = url.filter(|url| navigation::is_allowed_str(url)) {
        if previous.url.as_deref() != Some(url.as_str()) {
            previous.url = Some(url.clone());
            events.push(EngineEvent::UrlChanged { id, url });
        }
    }
    if let Some((can_go_back, can_go_forward)) = history {
        if previous.history != Some((can_go_back, can_go_forward)) {
            previous.history = Some((can_go_back, can_go_forward));
            events.push(EngineEvent::NavState {
                id,
                can_go_back,
                can_go_forward,
            });
        }
    }
    events
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RendererCrashTarget {
    Spare,
    Live,
    Retired,
}

fn renderer_crash_target(spare: Option<ItemId>, live: bool, id: ItemId) -> RendererCrashTarget {
    if spare == Some(id) {
        RendererCrashTarget::Spare
    } else if live {
        RendererCrashTarget::Live
    } else {
        RendererCrashTarget::Retired
    }
}

#[cfg(any(target_os = "windows", test))]
fn browser_group_absence_is_proven(
    had_environment: bool,
    had_native_profile: bool,
    construction_unproven: bool,
) -> bool {
    !had_environment && !had_native_profile && !construction_unproven
}

#[cfg(any(target_os = "windows", test))]
fn exact_browser_process_exit_proves_recovery(
    retained_process: Option<(u32, bool)>,
    observer_process_id: u32,
    callback_process_id: u32,
    callback_generation_matches: bool,
) -> bool {
    matches!(
        retained_process,
        Some((retained_process_id, true))
            if retained_process_id == observer_process_id
                && observer_process_id == callback_process_id
                && callback_generation_matches
    )
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransferredErasureExitSettlement {
    Stale,
    Pending,
    Proven,
    Invalid,
}

#[cfg(any(target_os = "windows", test))]
fn transferred_erasure_exit_settlement(
    observer_process_id: u32,
    event_process_id: u32,
    generation_matches: bool,
    proof_exited: bool,
    proof_invalid: bool,
) -> TransferredErasureExitSettlement {
    if observer_process_id != event_process_id || !generation_matches {
        TransferredErasureExitSettlement::Stale
    } else if proof_invalid {
        TransferredErasureExitSettlement::Invalid
    } else if proof_exited {
        TransferredErasureExitSettlement::Proven
    } else {
        TransferredErasureExitSettlement::Pending
    }
}

#[cfg(any(target_os = "windows", test))]
fn windows_profile_provenance_presence_is_consistent(
    environment: bool,
    process: bool,
    exit_observer: bool,
    version_observer: bool,
) -> bool {
    matches!(
        (environment, process, exit_observer, version_observer),
        (false, false, false, false) | (true, true, true, true)
    )
}

fn admit_profile_erasure(
    tombstones: &mut HashSet<ProfileId>,
    attempts: &mut HashMap<ProfileId, Arc<std::sync::atomic::AtomicBool>>,
    profile: ProfileId,
    completion: &Arc<crate::erasure::Completion>,
) -> bool {
    use std::sync::atomic::Ordering;

    if !completion.is_active() {
        return false;
    }
    if attempts
        .get(&profile)
        .is_some_and(|active| active.load(Ordering::Acquire))
    {
        completion.finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Failed);
        return false;
    }
    if (!tombstones.contains(&profile)
        && tombstones.len() >= zephium_core::session::MAX_SESSION_PROFILES)
        || (!attempts.contains_key(&profile)
            && attempts.len() >= zephium_core::session::MAX_SESSION_PROFILES)
    {
        // Never evict an old process-lifetime proof to admit an arbitrary new
        // identifier. The outer gate has already globally sealed access when
        // its matching bound is reached.
        completion.finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Failed);
        return false;
    }
    tombstones.insert(profile);
    attempts.insert(profile, completion.attempt_flag());
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProfilePersistenceClass {
    Durable,
    Ephemeral,
}

const MAX_PROFILE_PERSISTENCE_BINDINGS: usize = zephium_core::session::MAX_SESSION_PROFILES;

fn profile_persistence_class(partition: Partition) -> ProfilePersistenceClass {
    match partition {
        Partition::Default(_) | Partition::Persistent(_) => ProfilePersistenceClass::Durable,
        Partition::Ephemeral(_) => ProfilePersistenceClass::Ephemeral,
    }
}

fn bind_profile_persistence_class(
    bindings: &mut HashMap<ProfileId, ProfilePersistenceClass>,
    partition: Partition,
) -> bool {
    let profile = partition.profile();
    let class = profile_persistence_class(partition);
    if let Some(bound) = bindings.get(&profile) {
        return *bound == class;
    }
    if bindings.len() >= MAX_PROFILE_PERSISTENCE_BINDINGS {
        return false;
    }
    bindings.insert(profile, class);
    true
}

#[cfg(any(target_os = "macos", test))]
fn profile_scoped_value<T: Clone, E>(
    values: &mut HashMap<ProfileId, T>,
    profile: ProfileId,
    create: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    match values.entry(profile) {
        std::collections::hash_map::Entry::Occupied(entry) => Ok(entry.get().clone()),
        std::collections::hash_map::Entry::Vacant(entry) => {
            let value = create()?;
            entry.insert(value.clone());
            Ok(value)
        }
    }
}

#[cfg(any(target_os = "macos", test))]
fn profile_value_is_isolated<T>(
    values: &HashMap<ProfileId, T>,
    profile: ProfileId,
    value: &T,
    same_native_value: impl Fn(&T, &T) -> bool,
) -> bool {
    values.iter().all(|(other_profile, other_value)| {
        *other_profile == profile || !same_native_value(value, other_value)
    })
}

pub(crate) struct EngineHost {
    // Views are parented via the gtk container on Linux, not the raw handle.
    #[cfg_attr(all(unix, not(target_os = "macos")), allow(dead_code))]
    parent: ParentHandle,
    // Content web data never shares a directory or WebContext with the
    // privileged Tauri chrome. A context is further partitioned per profile.
    #[cfg(not(target_os = "macos"))]
    profiles_root: PathBuf,
    #[cfg(target_os = "windows")]
    private_runtime: zephium_core::webview2::RuntimeGeneration,
    views: HashMap<ItemId, ObservedView>,
    native_view_reservations: NativeViewReservations,
    native_resource_accounting_failed: bool,
    navigation_snapshots: HashMap<ItemId, NavigationSnapshot>,
    partitions: HashMap<ItemId, Partition>,
    // A ProfileId can never change between disk-backed and ephemeral native
    // storage in one process. Closing, crashing, or erasing a profile does not
    // relax this binding and therefore cannot resurrect a UDF in private mode.
    profile_persistence_classes: HashMap<ProfileId, ProfilePersistenceClass>,
    spare: Option<Spare>,
    user_content: HashMap<ContentScope, UserContent>,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    shortcuts: Vec<Shortcut>,
    #[cfg(target_os = "macos")]
    stages: HashMap<WindowId, Retained<ContentStage>>,
    #[cfg(not(target_os = "macos"))]
    stages: HashMap<WindowId, crate::platform::imp::Stage>,
    // A private profile owns exactly one non-persistent WKWebsiteDataStore for
    // its entire host lifetime. Each tab gets a fresh configuration pointing
    // at this retained store; distinct profile ids can never share one.
    #[cfg(target_os = "macos")]
    macos_ephemeral_data_stores: HashMap<ProfileId, crate::platform::imp::WebsiteDataStore>,
    // Off-screen views carrying the low-memory hint, and the subset the
    // shell's idle policy asked WebView2 to suspend.
    #[cfg(target_os = "windows")]
    hidden: std::collections::HashSet<ItemId>,
    #[cfg(target_os = "windows")]
    dormant: std::collections::HashSet<ItemId>,
    #[cfg(target_os = "windows")]
    desired_dormant: std::collections::HashSet<ItemId>,
    #[cfg(target_os = "windows")]
    suspending: std::collections::HashSet<ItemId>,
    #[cfg(target_os = "windows")]
    suspend_failed: std::collections::HashSet<ItemId>,
    #[cfg(not(target_os = "macos"))]
    web_contexts: HashMap<ProfileId, wry::WebContext>,
    // Native managers outlive every associated view/context until a profile
    // erasure has both cleared+fetched each manager and verified disk absence.
    // This closes the last-tab and failed-post-build proof gaps.
    #[cfg(all(unix, not(target_os = "macos")))]
    linux_data_managers: HashMap<ProfileId, Vec<webkit2gtk::WebsiteDataManager>>,
    #[cfg(all(unix, not(target_os = "macos")))]
    linux_unverifiable_data_managers: HashSet<ProfileId>,
    // Wry's WebContext is only a data-path holder on Windows. Reusing the
    // actual environment is what keeps one browser/network process group per
    // profile instead of creating one per tab.
    #[cfg(target_os = "windows")]
    browser_version_observers: HashMap<ProfileId, crate::platform::imp::BrowserVersionObserver>,
    #[cfg(target_os = "windows")]
    environments: HashMap<ProfileId, ICoreWebView2Environment>,
    #[cfg(target_os = "windows")]
    browser_processes: HashMap<ProfileId, crate::platform::imp::BrowserProcess>,
    #[cfg(target_os = "windows")]
    browser_process_exit_observers:
        HashMap<ProfileId, crate::platform::imp::BrowserProcessExitObserver>,
    // ProcessFailed retires controllers, but only the exact Environment5
    // BrowserProcessExited proof authorizes replacements. Retain every logical
    // id across that gap and emit it after the construction gate is reopened.
    #[cfg(target_os = "windows")]
    pending_profile_recovery: HashMap<ProfileId, Vec<ItemId>>,
    #[cfg(target_os = "windows")]
    exiting_browser_processes: HashSet<ProfileId>,
    #[cfg(target_os = "windows")]
    unverifiable_browser_processes: HashSet<ProfileId>,
    // Set before Wry begins a fallible controller build and cleared only once
    // the resulting environment, exact process HANDLE and Environment5 proof
    // are installed/revalidated. Empty process maps are not proof of absence
    // while this marker exists.
    #[cfg(target_os = "windows")]
    construction_unproven: HashSet<ProfileId>,
    // Unexpected or partially captured groups remain retained even though
    // their missing Environment5 proof makes erasure/shutdown fail closed.
    #[cfg(target_os = "windows")]
    unproven_browser_processes: HashMap<ProfileId, crate::platform::imp::BrowserProcess>,
    #[cfg(target_os = "windows")]
    unproven_environments: HashMap<ProfileId, ICoreWebView2Environment>,
    // A failed Controller::Close, parent-subclass removal, or Wry container
    // destruction remains an owned native obligation. It is never converted
    // into successful close/erasure merely because Rust released other COM
    // references.
    #[cfg(target_os = "windows")]
    windows_cleanup_debts: HashMap<ProfileId, Vec<wry::WebView2CleanupDebt>>,
    #[cfg(target_os = "windows")]
    windows_cleanup_invariant_failed: bool,
    // A tombstone is process-lifetime state: no failed/partial deletion may
    // silently make the profile usable again. Attempts are separate so a
    // settled failure can be retried, while a caller-visible timeout remains
    // in flight until native work reaches a terminal state.
    erasure_tombstones: HashSet<ProfileId>,
    erasure_attempts: HashMap<ProfileId, Arc<std::sync::atomic::AtomicBool>>,
    sink: Sink,
}

impl EngineHost {
    pub(crate) fn create_view(
        &mut self,
        id: ItemId,
        partition: Partition,
        url: &str,
        bounds: Rect,
        event_token: Arc<AtomicBool>,
    ) {
        if !event_token.load(Ordering::Acquire) {
            // This closure may have waited in the reentrant host queue after
            // the outer retirement check. Token state is the actual host-side
            // admission proof at physical execution time.
            return;
        }
        if self.erasure_tombstones.contains(&partition.profile()) {
            eprintln!("privacy: rejected view creation for tombstoned profile");
            self.sink
                .emit_for(event_token, EngineEvent::ViewCreationFailed { id });
            return;
        }
        if !bind_profile_persistence_class(&mut self.profile_persistence_classes, partition) {
            eprintln!("privacy: rejected profile persistence-class mismatch or capacity");
            self.sink
                .emit_for(event_token, EngineEvent::ViewCreationFailed { id });
            return;
        }
        if self.views.contains_key(&id) {
            self.sink
                .emit_for(event_token, EngineEvent::ViewCreationFailed { id });
            return;
        }
        if !navigation::is_allowed_str(url) {
            eprintln!("security: rejected invalid native view target");
            self.sink
                .emit_for(event_token, EngineEvent::ViewCreationFailed { id });
            return;
        }
        if let Some(spare) = self.spare.take_if(|s| s.partition == partition) {
            // Update the logical id before binding. Neither this Cell write,
            // binding, nor epoch advance enters native code, so a queued
            // bootstrap callback cannot observe a half-adopted state.
            spare.id.set(id);
            if !spare.view.event_permit.bind_once(&event_token) {
                // A spare is a one-shot native generation. Rebinding it would
                // let callbacks retained by its former logical owner acquire
                // a replacement item's token.
                eprintln!("engine: rejected reuse of an already-bound spare view");
                self.sink
                    .emit_for(event_token, EngineEvent::ViewCreationFailed { id });
                return;
            }
            if spare.view.navigation.begin(url).is_none() {
                eprintln!("engine: could not establish adopted-view navigation epoch");
                self.sink
                    .emit_for(event_token, EngineEvent::ViewCreationFailed { id });
                return;
            }
            if !spare.view.event_permit.allows_navigation(url) {
                return;
            }
            if let Err(error) = spare.view.load_url(url) {
                eprintln!("engine: spare navigation failed: {error}");
                self.sink
                    .emit_for(event_token, EngineEvent::ViewCreationFailed { id });
                return;
            }
            if !spare.view.event_permit.matches_token(&event_token) {
                // Navigation may pump native callbacks. Retirement can revoke
                // the outer token while load_url is in progress.
                return;
            }
            self.partitions.insert(id, partition);
            self.views.insert(id, spare.view);
            return;
        }
        let cell = Rc::new(Cell::new(id));
        if let Some(view) = self.build_view(
            cell,
            partition,
            url,
            bounds,
            true,
            EventPermit::bound(&event_token),
        ) {
            if !event_token.load(Ordering::Acquire)
                || !view.event_permit.matches_token(&event_token)
            {
                return;
            }
            self.partitions.insert(id, partition);
            self.views.insert(id, view);
        }
    }

    // Rebuilt after adoption from a page-load-finished hook, when the spawn
    // cost hides behind the page render.
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    pub(crate) fn ensure_spare(&mut self, partition: Partition) {
        // Keep at most one warm renderer process. A load in another profile
        // must not destroy and rebuild an existing spare: profile activity
        // would otherwise churn processes, CPU and private working sets while
        // neither profile opens a tab. The matching profile eventually adopts
        // the spare; a later completed load can then replenish its partition.
        if self.erasure_tombstones.contains(&partition.profile()) {
            return;
        }
        if !bind_profile_persistence_class(&mut self.profile_persistence_classes, partition) {
            eprintln!("privacy: rejected spare profile persistence-class mismatch or capacity");
            return;
        }
        if matches!(partition, Partition::Ephemeral(_)) || self.spare.is_some() {
            return;
        }
        let cell = Rc::new(Cell::new(ItemId::generate()));
        if let Some(view) = self.build_view(
            cell.clone(),
            partition,
            "about:blank",
            Rect::default(),
            false,
            EventPermit::inactive(),
        ) {
            self.spare = Some(Spare {
                partition,
                view,
                id: cell,
            });
        }
    }

    // An unstaged WebKitGTK view is mapped and keeps a renderer alive, while
    // an unmapped one can fail to acquire a compositing surface. Do not warm a
    // Linux spare until it has a measured, lifecycle-safe implementation.
    #[cfg(all(unix, not(target_os = "macos")))]
    pub(crate) fn ensure_spare(&mut self, partition: Partition) {
        if !self.erasure_tombstones.contains(&partition.profile())
            && !bind_profile_persistence_class(&mut self.profile_persistence_classes, partition)
        {
            eprintln!("privacy: rejected spare profile persistence-class mismatch or capacity");
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    fn retain_linux_data_manager_obligation(
        &mut self,
        profile: ProfileId,
        obligation: crate::platform::imp::WebsiteDataManagerObligation,
    ) {
        use gtk::glib::prelude::ObjectType;

        if !obligation.provenance_complete {
            // Sticky by design: later successful constructions cannot prove
            // what an earlier opaque native object used for storage.
            self.linux_unverifiable_data_managers.insert(profile);
        }
        for manager in obligation.managers {
            let already_retained = self
                .linux_data_managers
                .get(&profile)
                .is_some_and(|managers| {
                    managers
                        .iter()
                        .any(|existing| existing.as_ptr() == manager.as_ptr())
                });
            if already_retained {
                continue;
            }
            let retained = self
                .linux_data_managers
                .values()
                .map(Vec::len)
                .sum::<usize>();
            if retained >= MAX_LINUX_RETAINED_DATA_MANAGERS {
                // Keep the unexpected native handle alive through process
                // exit and make deletion permanently unverifiable. A release
                // abort here would let page-driven construction terminate the
                // whole browser.
                self.linux_unverifiable_data_managers.insert(profile);
                std::mem::forget(manager);
                continue;
            }
            self.linux_data_managers
                .entry(profile)
                .or_default()
                .push(manager);
        }
    }

    #[cfg(target_os = "windows")]
    fn windows_profile_process_group_capacity_allows(&self, requested: ProfileId) -> bool {
        profile_process_group_capacity_allows(
            self.environments
                .keys()
                .chain(self.browser_processes.keys())
                .chain(self.browser_process_exit_observers.keys())
                .chain(self.browser_version_observers.keys())
                .chain(self.exiting_browser_processes.iter())
                .chain(self.unverifiable_browser_processes.iter())
                .chain(self.construction_unproven.iter())
                .chain(self.unproven_browser_processes.keys())
                .chain(self.unproven_environments.keys())
                .chain(self.windows_cleanup_debts.keys())
                .copied(),
            requested,
        )
    }

    #[cfg(target_os = "windows")]
    fn capture_windows_environment(
        &mut self,
        profile: ProfileId,
        environment: ICoreWebView2Environment,
    ) -> windows_core::Result<(u32, crate::platform::imp::BrowserProcessGeneration)> {
        let process = match crate::platform::imp::browser_process_for_environment(&environment) {
            Ok(process) => process,
            Err(error) => {
                self.unproven_environments
                    .entry(profile)
                    .or_insert(environment);
                self.unverifiable_browser_processes.insert(profile);
                self.exiting_browser_processes.insert(profile);
                return Err(error);
            }
        };
        let process_id = process.id();
        if let Some(existing) = self.browser_processes.get(&profile) {
            let observer = self.browser_process_exit_observers.get(&profile);
            let observer_matches = observer.is_some_and(|observer| {
                crate::platform::imp::browser_process_reuse_is_safe(
                    existing.id(),
                    observer.expected_process_id(),
                    process_id,
                    observer.is_pending(),
                    existing.is_running(),
                )
            });
            let environment_matches = self.environments.get(&profile).is_some_and(|existing| {
                crate::platform::imp::same_environment(existing, &environment)
            });
            if !observer_matches
                || !environment_matches
                || !self.browser_version_observers.contains_key(&profile)
            {
                self.unproven_environments
                    .entry(profile)
                    .or_insert(environment);
                self.unproven_browser_processes
                    .entry(profile)
                    .or_insert(process);
                self.unverifiable_browser_processes.insert(profile);
                self.exiting_browser_processes.insert(profile);
                return Err(windows_core::Error::new(
                    windows::Win32::Foundation::E_UNEXPECTED,
                    "profile environment changed browser-process generation unexpectedly",
                ));
            }
            if let Some(observer) = observer {
                return Ok((process_id, observer.generation()));
            }
            self.unproven_environments
                .entry(profile)
                .or_insert(environment);
            self.unproven_browser_processes
                .entry(profile)
                .or_insert(process);
            self.unverifiable_browser_processes.insert(profile);
            self.exiting_browser_processes.insert(profile);
            return Err(windows_core::Error::new(
                windows::Win32::Foundation::E_UNEXPECTED,
                "validated WebView2 process had no exit observer",
            ));
        }

        if self.environments.contains_key(&profile)
            || self.browser_process_exit_observers.contains_key(&profile)
            || self.browser_version_observers.contains_key(&profile)
        {
            self.unproven_environments
                .entry(profile)
                .or_insert(environment);
            self.unproven_browser_processes
                .entry(profile)
                .or_insert(process);
            self.unverifiable_browser_processes.insert(profile);
            self.exiting_browser_processes.insert(profile);
            return Err(windows_core::Error::new(
                windows::Win32::Foundation::E_UNEXPECTED,
                "incomplete WebView2 browser-process provenance",
            ));
        }

        let observer = match crate::platform::imp::install_browser_process_exit_observer(
            &environment,
            process_id,
            move |event| {
                with_profile_exit(profile, event.generation(), move |host| {
                    host.on_browser_process_exit_event(profile, event);
                });
            },
        ) {
            Ok(observer) => observer,
            Err(error) => {
                // Exact environment and process HANDLE are retained. The
                // missing Environment5 registration makes release proof
                // impossible, so this profile remains terminally fail-closed.
                self.environments.insert(profile, environment);
                self.browser_processes.insert(profile, process);
                self.unverifiable_browser_processes.insert(profile);
                self.exiting_browser_processes.insert(profile);
                return Err(error);
            }
        };
        let generation = observer.generation();
        let update_sink = self.sink.clone();
        let version_observer =
            match crate::platform::imp::install_browser_version_observer(&environment, move || {
                update_sink.emit(EngineEvent::RuntimeRestartRequired);
            }) {
                Ok(observer) => observer,
                Err(error) => {
                    // Keep the exact environment, process and Environment5
                    // proof. Browsing is still rejected because admitting an
                    // unobserved environment could silently miss a security
                    // runtime update for the remainder of the process.
                    self.environments.insert(profile, environment);
                    self.browser_processes.insert(profile, process);
                    self.browser_process_exit_observers
                        .insert(profile, observer);
                    self.unverifiable_browser_processes.insert(profile);
                    self.exiting_browser_processes.insert(profile);
                    return Err(error);
                }
            };
        self.browser_version_observers
            .insert(profile, version_observer);
        self.environments.insert(profile, environment);
        self.browser_processes.insert(profile, process);
        self.browser_process_exit_observers
            .insert(profile, observer);
        Ok((process_id, generation))
    }

    fn native_owned_view_resources(&self) -> Option<usize> {
        let live = self.views.len();
        let has_spare = self.spare.is_some();
        #[cfg(target_os = "windows")]
        let cleanup_debts = self.windows_cleanup_debts.values().flatten().count();
        #[cfg(not(target_os = "windows"))]
        let cleanup_debts = 0;
        owned_native_view_resources(live, has_spare, cleanup_debts)
    }

    fn reserve_native_view_resource(&mut self) -> bool {
        if self.native_resource_accounting_failed {
            return false;
        }
        let Some(owned) = self.native_owned_view_resources() else {
            self.native_resource_accounting_failed = true;
            return false;
        };
        match self.native_view_reservations.try_reserve(owned) {
            Ok(admitted) => admitted,
            Err(()) => {
                self.native_resource_accounting_failed = true;
                false
            }
        }
    }

    fn release_native_view_resource_reservation(&mut self) -> bool {
        if self.native_view_reservations.release().is_err() {
            self.native_resource_accounting_failed = true;
            return false;
        }
        true
    }

    fn build_view(
        &mut self,
        id: Rc<Cell<ItemId>>,
        partition: Partition,
        url: &str,
        bounds: Rect,
        report_failure: bool,
        event_permit: EventPermit,
    ) -> Option<ObservedView> {
        let logical_id = id.get();
        let reservation_failure_permit = event_permit.clone();
        #[cfg(target_os = "windows")]
        {
            // The Wry fallback queue must be empty at an engine construction
            // boundary. Live ObservedViews report their profile directly; any Wry
            // fallback produced inside this call therefore belongs to this exact
            // partition, including errors after controller construction.
            let stale_debts = wry::pending_webview2_cleanup_debts();
            if !stale_debts.is_empty() {
                self.fail_windows_cleanup_invariant();
                for debt in stale_debts {
                    self.retain_windows_cleanup_debt(partition.profile(), debt);
                }
            }
            self.collect_pending_windows_cleanup_debts();
        }

        if !self.reserve_native_view_resource() {
            if report_failure {
                event_permit.emit(
                    &self.sink,
                    EngineEvent::ViewCreationFailed { id: logical_id },
                );
            }
            return None;
        }
        let built = self.build_view_inner(id, partition, url, bounds, report_failure, event_permit);
        #[cfg(target_os = "windows")]
        {
            for debt in wry::pending_webview2_cleanup_debts() {
                self.retain_windows_cleanup_debt(partition.profile(), debt);
            }
            if wry::webview2_cleanup_overflowed() {
                self.fail_windows_cleanup_invariant();
            }
            self.collect_pending_windows_cleanup_debts();
        }
        if !self.release_native_view_resource_reservation() {
            if report_failure {
                reservation_failure_permit.emit(
                    &self.sink,
                    EngineEvent::ViewCreationFailed { id: logical_id },
                );
            }
            return None;
        }
        built
    }

    fn build_view_inner(
        &mut self,
        id: Rc<Cell<ItemId>>,
        partition: Partition,
        url: &str,
        bounds: Rect,
        report_failure: bool,
        event_permit: EventPermit,
    ) -> Option<ObservedView> {
        if self.erasure_tombstones.contains(&partition.profile()) {
            if report_failure {
                event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
            }
            return None;
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        if self
            .linux_unverifiable_data_managers
            .contains(&partition.profile())
        {
            // Sticky native-storage debt is a construction barrier as well as
            // a deletion barrier. Repeated retries must not accumulate one
            // inaccessible manager per failed or malformed WebView.
            if report_failure {
                event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
            }
            return None;
        }
        #[cfg(target_os = "windows")]
        if self
            .exiting_browser_processes
            .contains(&partition.profile())
            || self
                .unverifiable_browser_processes
                .contains(&partition.profile())
            || self.construction_unproven.contains(&partition.profile())
            || self
                .windows_cleanup_debts
                .contains_key(&partition.profile())
            || self.windows_cleanup_invariant_failed
        {
            // A ProcessFailed callback is not the process-group release
            // barrier. Do not bind a replacement controller until the
            // Environment5 event retires the exact previous PID.
            if report_failure {
                event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
            }
            return None;
        }
        #[cfg(target_os = "windows")]
        if !self.windows_profile_process_group_capacity_allows(partition.profile()) {
            eprintln!(
                "engine: WebView2 native profile process-group ceiling ({MAX_NATIVE_PROFILE_PROCESS_GROUPS}) reached"
            );
            if report_failure {
                event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
            }
            return None;
        }
        if !bind_profile_persistence_class(&mut self.profile_persistence_classes, partition) {
            eprintln!("privacy: rejected profile persistence-class mismatch or capacity");
            if report_failure {
                event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
            }
            return None;
        }
        let on_title = self.sink.clone();
        let title_permit = event_permit.clone();
        let navigation = NavigationEpochTracker::new();
        let title_navigation = navigation.clone();
        let on_load = self.sink.clone();
        let load_permit = event_permit.clone();
        let load_navigation = navigation.clone();
        let crash_permit = event_permit.clone();
        let crash_id = id.clone();
        let navigation_permit = event_permit.clone();
        let policy_navigation = navigation.clone();
        let (title_id, load_id) = (id.clone(), id.clone());
        let scripts = self.scripts_for(partition);
        #[cfg(target_os = "windows")]
        let cached_environment = self.environments.get(&partition.profile()).cloned();
        #[cfg(target_os = "windows")]
        let construction_environment: Rc<RefCell<Option<ICoreWebView2Environment>>> =
            Rc::new(RefCell::new(None));
        #[cfg(target_os = "windows")]
        let construction_environment_capture_failed = Rc::new(Cell::new(false));

        // A profile owns its network/storage context. This is both the cookie
        // boundary and (on Windows) the WebView2 process-group boundary. The
        // audited Wry patch permits Linux incognito views to share only an
        // explicitly ephemeral supplied context, bounding native managers and
        // giving one private profile coherent in-memory cookie/storage state.
        #[cfg(all(unix, not(target_os = "macos")))]
        let mut pending_web_context: Option<wry::WebContext> = None;
        #[cfg(all(unix, not(target_os = "macos")))]
        let (builder, expected_website_data_directory, untracked_manager_on_build_failure) = {
            let profile = partition.profile();
            let expected_directory = match partition {
                Partition::Ephemeral(_) => None,
                Partition::Default(_) | Partition::Persistent(_) => {
                    match crate::erasure::prepare_profile_directory(&self.profiles_root, profile) {
                        Ok(path) => Some(path),
                        Err(error) => {
                            eprintln!("engine: cannot secure profile data directory: {error}");
                            if report_failure {
                                event_permit.emit(
                                    &self.sink,
                                    EngineEvent::ViewCreationFailed { id: id.get() },
                                );
                            }
                            return None;
                        }
                    }
                }
            };
            // Do not commit a newly-created Wry context to the host map before
            // a native view exposes its exact manager. Wry has no public
            // context->manager accessor; retaining an opaque context after
            // build failure would otherwise let a later empty retry fabricate
            // Verified.
            let context_is_new = !self.web_contexts.contains_key(&profile);
            let builder = if context_is_new {
                let context = match match partition {
                    Partition::Ephemeral(_) => wry::WebContext::new_ephemeral(),
                    Partition::Default(_) | Partition::Persistent(_) => {
                        wry::WebContext::try_new(expected_directory.clone())
                    }
                } {
                    Ok(context) => context,
                    Err(error) => {
                        self.linux_unverifiable_data_managers.insert(profile);
                        eprintln!("security: required WebKitGTK context policy failed: {error}");
                        if report_failure {
                            event_permit
                                .emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                        }
                        return None;
                    }
                };
                pending_web_context = Some(context);
                let Some(context) = pending_web_context.as_mut() else {
                    self.linux_unverifiable_data_managers.insert(profile);
                    return None;
                };
                WebViewBuilder::new_with_web_context(context)
            } else {
                let Some(context) = self.web_contexts.get_mut(&profile) else {
                    self.linux_unverifiable_data_managers.insert(profile);
                    return None;
                };
                WebViewBuilder::new_with_web_context(context)
            };
            (builder, expected_directory, context_is_new)
        };
        #[cfg(target_os = "windows")]
        let (builder, expected_user_data_folder) = {
            let profile = partition.profile();
            let root = match partition {
                Partition::Ephemeral(_) => self.private_runtime.root(),
                _ => &self.profiles_root,
            };
            let path = match crate::erasure::prepare_profile_directory(root, profile) {
                Ok(path) => path,
                Err(error) => {
                    eprintln!("engine: cannot secure profile data directory: {error}");
                    if report_failure {
                        event_permit
                            .emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                    }
                    return None;
                }
            };
            let builder = WebViewBuilder::new_with_web_context(
                self.web_contexts
                    .entry(profile)
                    .or_insert_with(|| wry::WebContext::new(Some(path.clone()))),
            );
            (builder, path)
        };
        #[cfg(target_os = "macos")]
        let (builder, expected_ephemeral_data_store) = {
            let builder = WebViewBuilder::new();
            match partition {
                Partition::Ephemeral(profile) => {
                    if self.macos_ephemeral_data_stores.len() >= MAX_PROFILE_PERSISTENCE_BINDINGS
                        && !self.macos_ephemeral_data_stores.contains_key(&profile)
                    {
                        eprintln!("privacy: macOS private data-store capacity exceeded");
                        if report_failure {
                            event_permit
                                .emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                        }
                        return None;
                    }
                    let store = match profile_scoped_value(
                        &mut self.macos_ephemeral_data_stores,
                        profile,
                        crate::platform::imp::new_ephemeral_data_store,
                    ) {
                        Ok(store) => store,
                        Err(error) => {
                            eprintln!(
                                "privacy: cannot allocate private WKWebsiteDataStore: {error}"
                            );
                            if report_failure {
                                event_permit.emit(
                                    &self.sink,
                                    EngineEvent::ViewCreationFailed { id: id.get() },
                                );
                            }
                            return None;
                        }
                    };
                    if !profile_value_is_isolated(
                        &self.macos_ephemeral_data_stores,
                        profile,
                        &store,
                        |left, right| Retained::as_ptr(left) == Retained::as_ptr(right),
                    ) {
                        // Do not permit an unexpected framework singleton to
                        // collapse two private profiles into one cookie jar.
                        // The other profile still owns the shared native
                        // handle, so removing this duplicate map entry loses no
                        // erasure obligation.
                        self.macos_ephemeral_data_stores.remove(&profile);
                        eprintln!("privacy: WKWebsiteDataStore crossed private profiles");
                        if report_failure {
                            event_permit
                                .emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                        }
                        return None;
                    }
                    let configuration =
                        match crate::platform::imp::new_configuration_with_data_store(&store) {
                            Ok(configuration) => configuration,
                            Err(error) => {
                                // Keep the newly-created store in the host map.
                                // It is now a native privacy obligation even
                                // though no view was successfully constructed.
                                eprintln!("privacy: cannot configure private WKWebView: {error}");
                                if report_failure {
                                    event_permit.emit(
                                        &self.sink,
                                        EngineEvent::ViewCreationFailed { id: id.get() },
                                    );
                                }
                                return None;
                            }
                        };
                    use wry::WebViewBuilderExtMacos;
                    (
                        builder.with_webview_configuration(configuration),
                        Some(store),
                    )
                }
                Partition::Default(_) | Partition::Persistent(_) => (builder, None),
            }
        };

        // No custom background color: the scrollbar gutter and unpainted
        // regions show the webview background, and anything but the engine
        // default reads as a detached strip along the page edge. The spawn
        // flash fix belongs to the theme->engine channel, not a hardcode.
        let mut builder = builder
            .with_bounds(to_wry(bounds))
            .with_devtools(cfg!(debug_assertions))
            .with_autoplay(false)
            // WebView2 otherwise enables its address/contact suggestions by
            // default. Raw content should not silently inherit ambient form
            // data before Zephium has an explicit, profile-scoped autofill
            // policy. Wry currently ignores this setting on WebKit platforms.
            .with_general_autofill_enabled(false)
            .with_navigation_handler(move |target| {
                policy_navigation.admit_target(&navigation_permit, &target)
            })
            // Raw content starts with no device or ambient capabilities. The
            // pinned Wry revision carries this callback consistently across
            // WKWebView, WebView2 and WebKitGTK; a future origin-scoped broker
            // can selectively replace the hard deny.
            .with_permission_handler(|_| wry::PermissionResponse::Deny)
            // This is a construction-time native policy, not a callback that
            // first materializes attacker-controlled URL/path metadata. Wry
            // installs the cancel handler before initial navigation on every
            // shipped desktop engine, and denial dominates callback settings.
            .with_download_policy(DownloadPolicy::DenyWithoutMetadata)
            // Browser chrome is the only authority for logical tab closure;
            // DOM close requests must not destroy a native child behind the
            // host's view/controller accounting.
            .with_page_close_policy(wry::PageClosePolicy::Ignore)
            .with_document_title_changed_handler(move |title| {
                // Title callbacks carry no navigation identifier. Do not let
                // an inactive spare or an adopted-but-uncommitted target emit
                // its bootstrap title under the new logical item id.
                if let Some(epoch) = title_navigation.current_committed() {
                    if title_navigation.is_current(epoch) {
                        title_permit.emit(
                            &on_title,
                            EngineEvent::TitleChanged {
                                id: title_id.get(),
                                title: bounded_title(&title),
                            },
                        );
                    }
                }
            })
            // A native popup is denied and must not be translated into a shell
            // tab. Wry does not expose enough trustworthy user-gesture and
            // opener metadata to distinguish an intentional link from popup
            // abuse. Keep the EngineEvent API for a future broker that can.
            .with_new_window_req_handler(|_url, _features| NewWindowResponse::Deny);

        // This host-owned guard must precede page/user content so its captured
        // platform intrinsics and event registrations cannot be replaced
        // before observation starts.
        builder = builder.with_initialization_script(DISCARD_SAFETY_BOOTSTRAP_JS);
        builder = builder.with_initialization_script(EXTRACT_HTML_BOOTSTRAP_JS);
        builder =
            builder.with_initialization_script_for_main_only(crate::PAGE_PRINT_DENY_SCRIPT, false);
        for script in scripts
            .iter()
            .filter(|s| s.world == World::Page && s.at_start)
        {
            builder = builder.with_initialization_script(&script.source);
        }

        #[cfg(target_os = "windows")]
        {
            use wry::WebViewBuilderExtWindows;
            let observed_environment = construction_environment.clone();
            let capture_failed = construction_environment_capture_failed.clone();
            builder = builder
                // Wry disables SmartScreen in its default argument set. Keep
                // only the browser-UI suppressions so content protection stays
                // enabled in the WebView2 runtime.
                .with_additional_browser_args("--disable-features=msWebOOUI,msPdfOOUI")
                .with_browser_accelerator_keys(false)
                // Runs after the exact environment exists and before Wry
                // starts controller construction. This closes the opaque-build
                // gap: even a later Wry error leaves a retained Environment5,
                // PID/generation, and exact process HANDLE obligation.
                .with_environment_created_handler(move |environment| {
                    let Ok(mut observed) = observed_environment.try_borrow_mut() else {
                        capture_failed.set(true);
                        return;
                    };
                    if observed.is_some() {
                        // The hook is an exactly-once construction stage. A
                        // duplicate callback cannot silently replace the
                        // environment whose process provenance was retained.
                        capture_failed.set(true);
                        return;
                    }
                    *observed = Some(environment.clone());
                });
            if let Some(environment) = cached_environment {
                builder = builder.with_environment(environment);
            }
        }

        #[cfg(target_os = "macos")]
        {
            use wry::WebViewBuilderExtDarwin;
            let permit = crash_permit.clone();
            builder = builder
                // Link preview is a native WebKit UI/network surface outside
                // the popup broker. Keep it disabled until chrome can label
                // the origin and verify the initiating gesture.
                .with_allow_link_preview(false)
                .with_on_web_content_process_terminate_handler(move || {
                    let id = crash_id.get();
                    let queued_permit = permit.clone();
                    with_renderer_exit(id, move |host| {
                        host.on_renderer_process_exit(id, &queued_permit)
                    });
                });
        }

        builder = match partition {
            Partition::Default(profile) | Partition::Persistent(profile) => {
                #[cfg(target_os = "macos")]
                {
                    use wry::WebViewBuilderExtDarwin;
                    builder.with_data_store_identifier(profile.bytes())
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = profile;
                    builder
                }
            }
            Partition::Ephemeral(_) => builder.with_incognito(true),
        };

        builder = builder.with_on_page_load_handler(move |event, url| {
            let id = load_id.get();
            let Some(epoch) = load_navigation.observe_load(&url, &event) else {
                return;
            };
            match event {
                PageLoadEvent::Started => {
                    if load_navigation.is_current(epoch) {
                        load_permit
                            .emit(&on_load, EngineEvent::LoadingChanged { id, loading: true });
                    }
                }
                PageLoadEvent::Finished => {
                    if load_navigation.is_current(epoch) {
                        load_permit
                            .emit(&on_load, EngineEvent::LoadingChanged { id, loading: false });
                    }
                    // Native source/history observers normally emit earlier,
                    // at commit time. This host-owned query is also the
                    // completion fallback; the callback never retains the
                    // WebView it observes.
                    let permit = load_permit.clone();
                    let navigation = load_navigation.clone();
                    if permit.active_token().is_some() {
                        with_observation(id, move |host| {
                            host.emit_navigation_observation(id, &permit, &navigation, epoch)
                        });
                    }
                }
            }
        });

        #[cfg(target_os = "windows")]
        if !self.construction_unproven.insert(partition.profile()) {
            eprintln!("engine: concurrent WebView2 construction debt for one profile");
            return None;
        }

        #[cfg(all(unix, not(target_os = "macos")))]
        let built = {
            use wry::WebViewBuilderExtUnix;
            match crate::platform::imp::container() {
                Some(container) => builder.build_gtk(&container),
                None => {
                    eprintln!("engine: gtk container not installed");
                    if report_failure {
                        event_permit
                            .emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                    }
                    return None;
                }
            }
        };
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        let built = builder.build_as_child(&self.parent);

        #[cfg(target_os = "windows")]
        let captured_process = {
            use wry::WebViewExtWindows;
            let profile = partition.profile();
            let observed_environment = construction_environment
                .try_borrow_mut()
                .map(|mut environment| environment.take())
                .unwrap_or_else(|_| {
                    construction_environment_capture_failed.set(true);
                    None
                })
                // Defensive fallback for a future Wry refactor that returns a
                // view without invoking the pre-controller hook. A successful
                // build must still never escape native obligation capture.
                .or_else(|| built.as_ref().ok().map(|view| view.environment()));
            if construction_environment_capture_failed.get() {
                self.quarantine_unverifiable_windows_profile(profile);
                eprintln!("engine: WebView2 environment construction hook was not exactly once");
                if report_failure {
                    event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                }
                return None;
            }
            let Some(environment) = observed_environment else {
                // The environment-completion hook is before controller
                // construction. If it did not run, Wry never returned an
                // environment and its construction guard owns any HWND cleanup.
                // No browser-process identity existed for Zephium to retain.
                if built.is_err() {
                    self.construction_unproven.remove(&profile);
                } else {
                    self.quarantine_unverifiable_windows_profile(profile);
                }
                eprintln!("engine: WebView2 construction exposed no environment obligation");
                if report_failure {
                    event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                }
                return None;
            };
            let captured = self.capture_windows_environment(profile, environment);
            // From this point, successful capture is represented by the exact
            // environment/process/observer maps; failed capture is represented
            // by sticky unproven/unverifiable obligations. The temporary marker
            // is no longer needed in either case.
            self.construction_unproven.remove(&profile);
            match captured {
                Ok(captured) => captured,
                Err(error) => {
                    self.quarantine_unverifiable_windows_profile(profile);
                    eprintln!("engine: cannot retain early WebView2 process obligation: {error}");
                    if report_failure {
                        event_permit
                            .emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                    }
                    return None;
                }
            }
        };

        let view = match built {
            Ok(view) => view,
            Err(e) => {
                #[cfg(all(unix, not(target_os = "macos")))]
                if untracked_manager_on_build_failure {
                    // An incognito build owns an inaccessible per-view
                    // context; a first durable build owns the still-local
                    // context. Wry may fail after native construction, so no
                    // later retry may infer manager absence from this error.
                    self.linux_unverifiable_data_managers
                        .insert(partition.profile());
                }
                eprintln!("engine: build_view failed: {e}");
                if report_failure {
                    event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                }
                return None;
            }
        };
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            // Capture before every fallible post-build step. In particular,
            // storage attestation, observer installation, and initial load
            // are not allowed to discard the only native erasure handle.
            let obligation = crate::platform::imp::website_data_manager_obligation(&view);
            self.retain_linux_data_manager_obligation(partition.profile(), obligation);
            if let Some(context) = pending_web_context.take() {
                match self.web_contexts.entry(partition.profile()) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(context);
                    }
                    std::collections::hash_map::Entry::Occupied(_) => {
                        self.linux_unverifiable_data_managers
                            .insert(partition.profile());
                        eprintln!(
                            "privacy: Linux profile context changed during native construction"
                        );
                        return None;
                    }
                }
            }
        }
        // Cross-check the controller's process identity against the environment
        // captured before controller construction. A mismatched value is a
        // terminal provenance failure, never a new generation to adopt.
        #[cfg(target_os = "windows")]
        let (browser_process_id, browser_process_generation) = {
            let profile = partition.profile();
            let controller_process = match crate::platform::imp::browser_process(&view) {
                Ok(process) => process,
                Err(error) => {
                    self.quarantine_unverifiable_windows_profile(profile);
                    eprintln!("engine: cannot cross-check WebView2 controller process: {error}");
                    if report_failure {
                        event_permit
                            .emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                    }
                    return None;
                }
            };
            if controller_process.id() != captured_process.0 {
                self.unproven_browser_processes
                    .entry(profile)
                    .or_insert(controller_process);
                self.quarantine_unverifiable_windows_profile(profile);
                eprintln!("engine: controller process did not match its captured environment");
                if report_failure {
                    event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                }
                return None;
            }
            captured_process
        };
        #[cfg(target_os = "windows")]
        if let Err(error) = self
            .environments
            .get(&partition.profile())
            .ok_or_else(|| {
                windows_core::Error::new(
                    windows::Win32::Foundation::E_UNEXPECTED,
                    "captured WebView2 environment disappeared before attestation",
                )
            })
            .and_then(|environment| {
                crate::platform::imp::attest_environment(environment, &expected_user_data_folder)
            })
        {
            self.quarantine_unverifiable_windows_profile(partition.profile());
            eprintln!("security: content WebView2 environment attestation failed: {error}");
            if report_failure {
                event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
            }
            return None;
        }
        #[cfg(target_os = "windows")]
        let security_policy = match crate::platform::imp::configure(
            &view,
            12.0,
            matches!(partition, Partition::Ephemeral(_)),
            &expected_user_data_folder,
        ) {
            Ok(policy) => policy,
            Err(error) => {
                // The exact environment was already attested above. Failures
                // from here are scoped to this new controller
                // (settings/handlers/profile postconditions); dropping Wry's
                // construction result closes it. A transient registration
                // failure must not poison healthy sibling controllers.
                eprintln!("security: content WebView2 controller hardening failed: {error}");
                if report_failure {
                    event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                }
                return None;
            }
        };
        #[cfg(target_os = "windows")]
        {
            debug_assert!(!self.construction_unproven.contains(&partition.profile()));
        }
        #[cfg(target_os = "macos")]
        if let Err(error) = crate::platform::imp::configure(
            &view,
            12.0,
            partition,
            expected_ephemeral_data_store.as_ref(),
        ) {
            eprintln!("security: content WKWebView storage attestation failed: {error}");
            if report_failure {
                event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
            }
            return None;
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        if let Err(error) = crate::platform::imp::configure(
            &view,
            12.0,
            partition,
            expected_website_data_directory.as_deref(),
        ) {
            // The retained manager did not prove its persistence mode and
            // direct owned path. Keep its handle, but permanently deny disk
            // deletion for this profile rather than clearing an unknown
            // manager or allowing a later valid view to erase the debt.
            self.linux_unverifiable_data_managers
                .insert(partition.profile());
            eprintln!("security: content WebKitGTK storage attestation failed: {error}");
            if report_failure {
                event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
            }
            return None;
        }
        #[cfg(target_os = "windows")]
        let process_failure_permit = crash_permit.clone();
        #[cfg(target_os = "windows")]
        let crash_observer =
            match crate::platform::imp::install_crash_handler(&view, move |failure| match failure {
                crate::platform::imp::ProcessFailure::Renderer => {
                    let id = crash_id.get();
                    let queued_permit = process_failure_permit.clone();
                    with_renderer_exit(id, move |host| {
                        host.on_renderer_process_exit(id, &queued_permit)
                    });
                }
                crate::platform::imp::ProcessFailure::Browser => {
                    let profile = partition.profile();
                    with_profile_exit(profile, browser_process_generation, move |host| {
                        host.on_profile_process_exit(
                            profile,
                            browser_process_id,
                            browser_process_generation,
                        );
                    });
                }
            }) {
                Ok(observer) => observer,
                Err(error) => {
                    eprintln!("engine: required WebView2 process-failure handler failed: {error}");
                    if report_failure {
                        event_permit
                            .emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                    }
                    return None;
                }
            };
        // A dead web process must surface as an event, never as a silently
        // blank pane; the shell decides whether to relaunch.
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            use webkit2gtk::WebViewExt;
            use wry::WebViewExtUnix;
            let permit = crash_permit.clone();
            view.webview()
                .connect_web_process_terminated(move |_, reason| {
                    eprintln!("engine: web process terminated: {reason:?}");
                    let id = crash_id.get();
                    let queued_permit = permit.clone();
                    with_renderer_exit(id, move |host| {
                        host.on_renderer_process_exit(id, &queued_permit)
                    });
                });
        }
        #[cfg(target_os = "windows")]
        let shortcut_item = id.clone();
        #[cfg(target_os = "windows")]
        let accelerator_permit = event_permit.clone();
        #[cfg(target_os = "windows")]
        let accelerator_sink = self.sink.clone();
        #[cfg(target_os = "windows")]
        let accelerator_registration = match crate::platform::imp::install_accelerators(
            &view,
            self.shortcuts.clone(),
            Arc::new(move |event| accelerator_permit.emit(&accelerator_sink, event)),
            move || shortcut_item.get(),
        ) {
            Ok(registration) => registration,
            Err(error) => {
                eprintln!("engine: required accelerator registration failed: {error}");
                if report_failure {
                    event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                }
                return None;
            }
        };
        #[cfg(target_os = "macos")]
        for script in scripts
            .iter()
            .filter(|s| !(s.world == World::Page && s.at_start))
        {
            crate::platform::imp::add_user_script(&view, script);
        }

        let observation_id = id.clone();
        let observation_permit = event_permit.clone();
        let observation_navigation = navigation.clone();
        let observer = match crate::platform::imp::install_navigation_observer(&view, move || {
            if observation_permit.active_token().is_none() {
                return;
            }
            let Some(epoch) = observation_navigation.current() else {
                return;
            };
            let id = observation_id.get();
            let queued_permit = observation_permit.clone();
            let queued_navigation = observation_navigation.clone();
            with_observation(id, move |host| {
                host.emit_navigation_observation(id, &queued_permit, &queued_navigation, epoch)
            });
        }) {
            Ok(observer) => observer,
            Err(error) => {
                eprintln!("engine: required native navigation observer failed: {error}");
                if report_failure {
                    event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
                }
                return None;
            }
        };
        // Not on Linux: webkitgtk never builds a compositing surface for a
        // view that loads while unmapped, and the widget stays blank after it
        // is shown (why the Linux spare is disabled). The stage hides
        // non-visible views at the first layout instead.
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        let _ = view.set_visible(false);
        if !event_permit.allows_navigation(url) {
            // WebView construction can pump WebView2 messages. Do not perform
            // the first content load after the outer generation was retired.
            return None;
        }
        if navigation.begin(url).is_none() {
            eprintln!("engine: could not establish initial navigation epoch");
            if report_failure {
                event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
            }
            return None;
        }
        if let Err(error) = view.load_url(url) {
            eprintln!("engine: initial navigation failed: {error}");
            if report_failure {
                event_permit.emit(&self.sink, EngineEvent::ViewCreationFailed { id: id.get() });
            }
            return None;
        }
        if !event_permit.allows_navigation(url) {
            // load_url itself may pump. Dropping here removes the controller
            // before it can be inserted into the live host maps.
            return None;
        }
        Some(ObservedView {
            event_permit,
            navigation,
            #[cfg(target_os = "windows")]
            _crash_observer: crash_observer,
            #[cfg(target_os = "windows")]
            _accelerator_registration: accelerator_registration,
            #[cfg(target_os = "windows")]
            _security_policy: security_policy,
            _observer: observer,
            #[cfg(target_os = "windows")]
            cleanup_profile: partition.profile(),
            view,
        })
    }

    fn emit_navigation_observation(
        &mut self,
        id: ItemId,
        source_permit: &EventPermit,
        source_navigation: &NavigationEpochTracker,
        epoch: NavigationEpoch,
    ) {
        let (event_permit, navigation, url, history) = {
            let Some(view) = self.views.get(&id) else {
                return;
            };
            if !navigation_callback_matches(
                &view.event_permit,
                &view.navigation,
                source_permit,
                source_navigation,
                epoch,
            ) {
                return;
            }
            let url = crate::platform::imp::current_url(view);
            let history = match (view.can_go_back(), view.can_go_forward()) {
                (Ok(can_go_back), Ok(can_go_forward)) => Some((can_go_back, can_go_forward)),
                _ => None,
            };
            (
                view.event_permit.clone(),
                view.navigation.clone(),
                url,
                history,
            )
        };
        let url = match classify_observed_url(url) {
            ObservedUrl::Unavailable => {
                // Source can transiently be unavailable during navigation.
                // History facts are attributable only after this exact epoch
                // has committed an allowed source.
                if navigation.current_committed() != Some(epoch) {
                    return;
                }
                None
            }
            ObservedUrl::Allowed(url) => {
                if !navigation.observe_source(epoch, &url) {
                    return;
                }
                Some(url)
            }
            ObservedUrl::Forbidden => {
                // Navigation callbacks should have prevented this. A History
                // API mutation can still create an overlong same-document URL
                // without a navigation callback, so never leave trusted
                // chrome showing the previous address over that document.
                eprintln!("security: native content source escaped the URL policy; closing view");
                let token = event_permit.active_token();
                // The terminal event must never race a still-reachable native
                // object. Revoke callbacks and remove the physical view first.
                self.close(id);
                if let Some(token) = token {
                    self.sink
                        .emit_for(token, EngineEvent::ViewCreationFailed { id });
                }
                return;
            }
        };
        if !navigation.is_current(epoch) {
            return;
        }
        let previous = self.navigation_snapshots.entry(id).or_default();
        for event in navigation_observation_events(id, previous, url, history) {
            event_permit.emit(&self.sink, event);
        }
    }

    fn scripts_for(&self, partition: Partition) -> Vec<UserScript> {
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

    pub(crate) fn navigate(
        &self,
        id: ItemId,
        url: &str,
        request: NavigationRequestId,
        event_token: Arc<AtomicBool>,
    ) {
        if self
            .partitions
            .get(&id)
            .is_some_and(|partition| self.erasure_tombstones.contains(&partition.profile()))
        {
            eprintln!("privacy: rejected navigation for tombstoned profile");
            self.sink
                .emit_for(event_token, EngineEvent::NavigationFailed { id, request });
            return;
        }
        if !navigation::is_allowed_str(url) {
            eprintln!("security: rejected invalid native navigation target");
            self.sink
                .emit_for(event_token, EngineEvent::NavigationFailed { id, request });
            return;
        }
        let Some(view) = self.views.get(&id) else {
            // Core believed this id had a live controller. A missing native
            // view is a lifecycle failure, not merely a rejected URL.
            self.sink
                .emit_for(event_token, EngineEvent::ViewCreationFailed { id });
            return;
        };
        if !view.event_permit.matches_token(&event_token) {
            // This host task belongs to a prior same-id generation that was
            // displaced while a native message loop was reentrant.
            return;
        }
        if view.navigation.begin(url).is_none() {
            eprintln!("engine: could not establish navigation epoch");
            self.sink
                .emit_for(event_token, EngineEvent::NavigationFailed { id, request });
            return;
        }
        if let Err(error) = view.load_url(url) {
            eprintln!("engine: navigation failed: {error}");
            self.sink
                .emit_for(event_token, EngineEvent::NavigationFailed { id, request });
        }
    }

    pub(crate) fn reload(&self, id: ItemId) {
        if let Some(view) = self.views.get(&id) {
            let _ = view.reload();
        }
    }

    pub(crate) fn stop(&self, id: ItemId) {
        if let Some(view) = self.views.get(&id) {
            crate::platform::imp::stop_loading(view);
        }
    }

    pub(crate) fn go_back(&self, id: ItemId) {
        if let Some(view) = self.views.get(&id) {
            let _ = view.go_back();
        }
    }

    pub(crate) fn go_forward(&self, id: ItemId) {
        if let Some(view) = self.views.get(&id) {
            let _ = view.go_forward();
        }
    }

    pub(crate) fn zoom(&self, id: ItemId, scale: f64) {
        if let Some(view) = self.views.get(&id) {
            let _ = view.zoom(scale);
        }
    }

    pub(crate) fn extract_html(&self, id: ItemId) {
        if let Some(view) = self.views.get(&id) {
            let Some(epoch) = view.navigation.current_committed() else {
                return;
            };
            let sink = self.sink.clone();
            let permit = view.event_permit.clone();
            let navigation = view.navigation.clone();
            let script = EXTRACT_HTML_JS.replace("__MAX__", &MAX_HTML_CHARS.to_string());
            let _ = view.evaluate_script_with_callback(&script, move |result| {
                if !navigation.is_current(epoch) {
                    return;
                }
                if result.len() > MAX_HTML_RESULT_BYTES {
                    return;
                }
                let Ok(value) = serde_json::from_str::<String>(&result) else {
                    return;
                };
                let (truncated, html) = if let Some(html) = value.strip_prefix('0') {
                    (false, html)
                } else if let Some(html) = value.strip_prefix('1') {
                    (true, html)
                } else {
                    return;
                };
                if html.encode_utf16().count() > MAX_HTML_CHARS {
                    return;
                }
                permit.emit(
                    &sink,
                    EngineEvent::HtmlExtracted {
                        id,
                        html: html.to_owned(),
                        truncated,
                    },
                );
            });
        }
    }

    pub(crate) fn discover_favicon(&self, id: ItemId) {
        if let Some(view) = self.views.get(&id) {
            let Some((epoch, page_url)) = view.navigation.committed_snapshot() else {
                return;
            };
            if !navigation::is_allowed_str(&page_url) {
                return;
            }
            let sink = self.sink.clone();
            let permit = view.event_permit.clone();
            let navigation = view.navigation.clone();
            let _ = view.evaluate_script_with_callback(FAVICON_JS, move |result| {
                if !navigation.matches_committed_snapshot(epoch, &page_url) {
                    return;
                }
                // Native engines JSON-serialize the primitive callback value.
                // Base64 uses no characters requiring JSON escaping, so the
                // only accepted non-null form is exactly 5464 bytes enclosed
                // by two quotes. No page object/toJSON hook is traversed.
                let Some(rgba) = decode_favicon_eval_result(&result) else {
                    return;
                };
                permit.emit(
                    &sink,
                    EngineEvent::FaviconPixels {
                        id,
                        page_url: page_url.clone(),
                        rgba,
                    },
                );
            });
        }
    }

    pub(crate) fn probe_discard_safety(&self, id: ItemId, probe: DiscardProbeId) {
        let Some(view) = self.views.get(&id) else {
            return;
        };
        let Some(epoch) = view.navigation.current_committed() else {
            return;
        };
        let permit = view.event_permit.clone();
        let navigation = view.navigation.clone();
        let queued_permit = permit.clone();
        let queued_navigation = navigation.clone();
        let _ = view.evaluate_script_with_callback(DISCARD_SAFETY_QUERY_JS, move |result| {
            // Callback completion can race focus-driven navigation or close.
            // Do not reinterpret a report from the old document under a new
            // same-id view/navigation; the shell timeout is fail-closed.
            if !queued_navigation.is_current(epoch) {
                return;
            }
            let renderer_safe = renderer_report_allows_discard(&result);
            let completion_permit = queued_permit.clone();
            let completion_navigation = queued_navigation.clone();
            with_observation(id, move |host| {
                host.complete_discard_probe(
                    id,
                    probe,
                    &completion_permit,
                    &completion_navigation,
                    epoch,
                    renderer_safe,
                )
            });
        });
    }

    fn complete_discard_probe(
        &self,
        id: ItemId,
        probe: DiscardProbeId,
        permit: &EventPermit,
        navigation: &NavigationEpochTracker,
        epoch: NavigationEpoch,
        renderer_safe: bool,
    ) {
        let Some(view) = self.views.get(&id) else {
            return;
        };
        if !discard_probe_identity_matches(
            &view.event_permit,
            &view.navigation,
            permit,
            navigation,
            epoch,
        ) {
            return;
        }

        if !renderer_safe {
            permit.emit(
                &self.sink,
                EngineEvent::DiscardSafety {
                    id,
                    probe,
                    can_discard: false,
                },
            );
            return;
        }

        // WebView2 and WebKitGTK expose native audio activity; WKWebView has
        // public asynchronous playback plus synchronous camera/microphone
        // capture state. Page JavaScript cannot spoof these cross-checks.
        let activity_permit = permit.clone();
        let activity_navigation = navigation.clone();
        let started = crate::platform::imp::query_document_activity(view, move |native_allows| {
            with_observation(id, move |host| {
                host.finish_discard_probe(
                    id,
                    probe,
                    &activity_permit,
                    &activity_navigation,
                    epoch,
                    native_allows,
                )
            });
        });
        if !started {
            // Missing native API/admission is uncertainty, never silence.
            permit.emit(
                &self.sink,
                EngineEvent::DiscardSafety {
                    id,
                    probe,
                    can_discard: false,
                },
            );
        }
    }

    fn finish_discard_probe(
        &self,
        id: ItemId,
        probe: DiscardProbeId,
        permit: &EventPermit,
        navigation: &NavigationEpochTracker,
        epoch: NavigationEpoch,
        native_allows: bool,
    ) {
        let Some(view) = self.views.get(&id) else {
            return;
        };
        if !discard_probe_identity_matches(
            &view.event_permit,
            &view.navigation,
            permit,
            navigation,
            epoch,
        ) {
            return;
        }
        // Raw-content downloads are denied at construction (and per-context
        // before load on GTK), so there is no admitted active download state
        // to query here. That remains a mandatory invariant until a broker
        // supplies an explicit download lease.
        permit.emit(
            &self.sink,
            EngineEvent::DiscardSafety {
                id,
                probe,
                can_discard: native_allows,
            },
        );
    }

    pub(crate) fn print(&self, id: ItemId) {
        if let Some(view) = self.views.get(&id) {
            let _ = view.print();
        }
    }

    #[cfg(target_os = "windows")]
    fn collect_pending_windows_cleanup_debts(&mut self) {
        let pending = PENDING_WINDOWS_CLEANUP_DEBTS.with(|pending| {
            let Ok(mut pending) = pending.try_borrow_mut() else {
                // Existing debts remain owned by the TLS queue. We cannot
                // prove which profile obligations were observed, so make the
                // global construction/erasure barrier sticky instead of
                // panicking from RefCell's dynamic borrow check.
                WINDOWS_CLEANUP_INVARIANT_FAILED.with(|failed| failed.set(true));
                return Vec::new();
            };
            std::mem::take(&mut *pending)
        });
        for (profile, debt) in pending {
            self.retain_windows_cleanup_debt(profile, debt);
        }
        let invariant_failed =
            WINDOWS_CLEANUP_INVARIANT_FAILED.with(Cell::get) || wry::webview2_cleanup_overflowed();
        if invariant_failed {
            self.fail_windows_cleanup_invariant();
        }
    }

    #[cfg(target_os = "windows")]
    fn fail_windows_cleanup_invariant(&mut self) {
        if self.windows_cleanup_invariant_failed {
            return;
        }
        self.windows_cleanup_invariant_failed = true;
        let mut profiles: HashSet<ProfileId> = self
            .partitions
            .values()
            .map(|partition| partition.profile())
            .chain(self.environments.keys().copied())
            .chain(self.windows_cleanup_debts.keys().copied())
            .collect();
        if let Some(spare) = self.spare.as_ref() {
            profiles.insert(spare.partition.profile());
        }
        for profile in profiles {
            self.unverifiable_browser_processes.insert(profile);
            self.exiting_browser_processes.insert(profile);
        }
        let ids: Vec<_> = self.views.keys().copied().collect();
        for id in ids {
            self.close(id);
        }
        self.spare = None;
    }

    #[cfg(target_os = "windows")]
    fn retain_windows_cleanup_debt(
        &mut self,
        profile: ProfileId,
        mut debt: wry::WebView2CleanupDebt,
    ) {
        if debt.retry().is_ok() {
            return;
        }
        let debt_count: usize = self.windows_cleanup_debts.values().map(Vec::len).sum();
        if debt_count >= MAX_WINDOWS_CLEANUP_DEBTS {
            // Dropping the new debt would only move it to Wry's fallback and
            // lose profile provenance. This indicates a violated global view
            // bound. Retain the native references until process exit and
            // quarantine every profile rather than aborting from teardown or
            // continuing with unknown cleanup.
            std::mem::forget(debt);
            self.fail_windows_cleanup_invariant();
            return;
        }
        self.windows_cleanup_debts
            .entry(profile)
            .or_default()
            .push(debt);

        let newly_quarantined = self.unverifiable_browser_processes.insert(profile);
        self.exiting_browser_processes.insert(profile);
        if newly_quarantined {
            let ids = self.retire_profile_process_views(profile);
            self.remember_profile_recovery_ids(profile, ids);
        }
    }

    #[cfg(target_os = "windows")]
    fn retry_windows_cleanup_debts(&mut self, attempts: usize) {
        self.collect_pending_windows_cleanup_debts();
        let profiles: Vec<_> = self.windows_cleanup_debts.keys().copied().collect();
        for profile in profiles {
            let Some(mut debts) = self.windows_cleanup_debts.remove(&profile) else {
                continue;
            };
            for _ in 0..attempts {
                debts.retain_mut(|debt| debt.retry().is_err());
                if debts.is_empty() {
                    break;
                }
            }
            if !debts.is_empty() {
                self.windows_cleanup_debts.insert(profile, debts);
            }
        }
    }

    pub(crate) fn close(&mut self, id: ItemId) {
        #[cfg(target_os = "windows")]
        let profile = self
            .partitions
            .get(&id)
            .map(|partition| partition.profile());
        let removed = self.views.remove(&id);
        self.navigation_snapshots.remove(&id);
        self.partitions.remove(&id);
        #[cfg(target_os = "windows")]
        {
            self.hidden.remove(&id);
            self.dormant.remove(&id);
            self.desired_dormant.remove(&id);
            self.suspending.remove(&id);
            self.suspend_failed.remove(&id);
        }
        for stage in self.stages.values() {
            stage.remove_view(id);
        }
        #[cfg(target_os = "windows")]
        if let (Some(profile), Some(view)) = (profile, removed) {
            if let Some(debt) = view.close_explicit() {
                self.retain_windows_cleanup_debt(profile, debt);
            }
        }
        #[cfg(not(target_os = "windows"))]
        drop(removed);
    }

    pub(crate) fn erase_profile_data(
        &mut self,
        profile: ProfileId,
        completion: Arc<crate::erasure::Completion>,
    ) {
        if !admit_profile_erasure(
            &mut self.erasure_tombstones,
            &mut self.erasure_attempts,
            profile,
            &completion,
        ) {
            return;
        }
        #[cfg(target_os = "windows")]
        self.pending_profile_recovery.remove(&profile);

        // Tombstone before touching any native reference. Reentrant creation
        // or navigation callbacks during teardown must observe the deny state,
        // and no failure path below removes it.

        #[cfg(target_os = "macos")]
        let ephemeral_stores = self
            .macos_ephemeral_data_stores
            .get(&profile)
            .cloned()
            .into_iter()
            .collect();

        #[cfg(all(unix, not(target_os = "macos")))]
        let managers = self
            .linux_data_managers
            .get(&profile)
            .cloned()
            .unwrap_or_default();
        #[cfg(all(unix, not(target_os = "macos")))]
        let manager_provenance_valid = !self.linux_unverifiable_data_managers.contains(&profile)
            && (!self.web_contexts.contains_key(&profile) || !managers.is_empty());

        #[cfg(target_os = "windows")]
        let native_profile = self
            .views
            .iter()
            .find_map(|(id, view)| {
                self.partitions
                    .get(id)
                    .is_some_and(|partition| partition.profile() == profile)
                    .then(|| crate::platform::imp::profile_for_erasure(view))
            })
            .or_else(|| {
                self.spare
                    .as_ref()
                    .filter(|spare| spare.partition.profile() == profile)
                    .map(|spare| crate::platform::imp::profile_for_erasure(&spare.view))
            })
            .and_then(|result| match result {
                Ok(profile) => Some(profile),
                Err(error) => {
                    eprintln!("privacy: cannot access WebView2 profile clear API: {error}");
                    None
                }
            });
        #[cfg(target_os = "windows")]
        let had_environment = self.environments.contains_key(&profile);
        #[cfg(target_os = "windows")]
        let browser_process_exit_proof = self
            .browser_process_exit_observers
            .get(&profile)
            .map(crate::platform::imp::BrowserProcessExitObserver::proof);
        #[cfg(target_os = "windows")]
        let browser_process = self.browser_processes.remove(&profile);
        #[cfg(target_os = "windows")]
        let process_pair_valid = match (&browser_process, &browser_process_exit_proof) {
            (Some(process), Some(proof)) => process.id() == proof.expected_process_id(),
            (None, None) => browser_group_absence_is_proven(
                had_environment,
                native_profile.is_some(),
                self.construction_unproven.contains(&profile),
            ),
            _ => false,
        };
        #[cfg(target_os = "windows")]
        if !process_pair_valid {
            self.unverifiable_browser_processes.insert(profile);
        }
        #[cfg(target_os = "windows")]
        let mut process_provenance_valid = !self.unverifiable_browser_processes.contains(&profile)
            && !self.construction_unproven.contains(&profile)
            && !self.unproven_browser_processes.contains_key(&profile)
            && !self.unproven_environments.contains_key(&profile)
            && !self.windows_cleanup_invariant_failed;

        let mut ids: Vec<ItemId> = self
            .partitions
            .iter()
            .filter_map(|(id, partition)| (partition.profile() == profile).then_some(*id))
            .collect();
        ids.sort();
        for id in ids {
            self.close(id);
        }
        if self
            .spare
            .as_ref()
            .is_some_and(|spare| spare.partition.profile() == profile)
        {
            // ObservedView drops its observer before its native WebView.
            self.spare = None;
        }
        #[cfg(target_os = "windows")]
        {
            self.retry_windows_cleanup_debts(3);
            process_provenance_valid &= !self.windows_cleanup_debts.contains_key(&profile)
                && !self.unverifiable_browser_processes.contains(&profile)
                && !self.windows_cleanup_invariant_failed;
        }
        #[cfg(not(target_os = "macos"))]
        self.web_contexts.remove(&profile);
        #[cfg(target_os = "windows")]
        self.browser_version_observers.remove(&profile);
        #[cfg(target_os = "windows")]
        self.environments.remove(&profile);

        #[cfg(target_os = "macos")]
        crate::platform::imp::erase_profile_data(profile, ephemeral_stores, completion);
        #[cfg(all(unix, not(target_os = "macos")))]
        crate::platform::imp::erase_profile_data(
            managers,
            manager_provenance_valid,
            vec![self.profiles_root.clone()],
            profile,
            completion,
        );
        #[cfg(target_os = "windows")]
        crate::platform::imp::erase_profile_data(
            native_profile,
            browser_process,
            browser_process_exit_proof,
            process_provenance_valid,
            vec![
                self.profiles_root.clone(),
                self.private_runtime.root().to_owned(),
            ],
            profile,
            completion,
        );
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn shutdown(&mut self) -> bool {
        self.shutdown_common();
        !self.native_resource_accounting_failed
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn shutdown(
        &mut self,
    ) -> (
        Vec<crate::platform::imp::BrowserProcessShutdownObligation>,
        bool,
    ) {
        self.shutdown_common();
        self.retry_windows_cleanup_debts(3);

        let mut provenance_valid = self.unverifiable_browser_processes.is_empty()
            && self.construction_unproven.is_empty()
            && self.unproven_browser_processes.is_empty()
            && self.unproven_environments.is_empty()
            && self.windows_cleanup_debts.is_empty()
            && !self.windows_cleanup_invariant_failed
            && !self.native_resource_accounting_failed
            && self
                .environments
                .keys()
                .chain(self.browser_processes.keys())
                .chain(self.browser_process_exit_observers.keys())
                .chain(self.browser_version_observers.keys())
                .all(|profile| {
                    windows_profile_provenance_presence_is_consistent(
                        self.environments.contains_key(profile),
                        self.browser_processes.contains_key(profile),
                        self.browser_process_exit_observers.contains_key(profile),
                        self.browser_version_observers.contains_key(profile),
                    )
                });
        let mut obligations = Vec::with_capacity(self.browser_processes.len());
        for (profile, process) in self.browser_processes.drain() {
            let Some(proof) = self
                .browser_process_exit_observers
                .get(&profile)
                .map(crate::platform::imp::BrowserProcessExitObserver::proof)
            else {
                provenance_valid = false;
                continue;
            };
            match crate::platform::imp::BrowserProcessShutdownObligation::new(process, proof) {
                Some(obligation) => obligations.push(obligation),
                None => provenance_valid = false,
            }
        }
        // Releasing controllers and these ordinary environment references
        // initiates normal runtime shutdown. Observer guards intentionally
        // remain UI-thread-owned until process exit signals their proofs.
        self.browser_version_observers.clear();
        self.environments.clear();
        self.exiting_browser_processes.clear();
        self.pending_profile_recovery.clear();
        self.hidden.clear();
        self.dormant.clear();
        self.desired_dormant.clear();
        self.suspending.clear();
        self.suspend_failed.clear();
        (obligations, provenance_valid)
    }

    fn shutdown_common(&mut self) {
        let ids: Vec<ItemId> = self.views.keys().copied().collect();
        for id in ids {
            self.close(id);
        }
        self.spare = None;
        self.navigation_snapshots.clear();
        self.partitions.clear();
        // Release native composition roots as part of the shutdown barrier.
        // Popups are separate native windows and macOS' parent view retains
        // subviews, so clearing only the Rust map is not sufficient.
        #[cfg(target_os = "macos")]
        for stage in self.stages.values() {
            stage.set_drop_indicator(None);
            stage.removeFromSuperview();
        }
        #[cfg(not(target_os = "macos"))]
        for stage in self.stages.values() {
            stage.set_drop_indicator(None);
        }
        self.stages.clear();
        #[cfg(target_os = "macos")]
        self.macos_ephemeral_data_stores.clear();
        #[cfg(not(target_os = "macos"))]
        self.web_contexts.clear();
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            self.linux_data_managers.clear();
            self.linux_unverifiable_data_managers.clear();
        }
    }

    fn on_renderer_process_exit(&mut self, id: ItemId, source_permit: &EventPermit) {
        let spare = self.spare.as_ref().map(|spare| spare.id.get());
        match renderer_crash_target(spare, self.views.contains_key(&id), id) {
            // A spare has no shell item. Drop the dead native object here so
            // it can never be adopted under a future real item id.
            RendererCrashTarget::Spare => {
                if self
                    .spare
                    .as_ref()
                    .is_some_and(|spare| spare.view.event_permit.same_generation(source_permit))
                {
                    self.spare = None;
                }
            }
            RendererCrashTarget::Live => {
                let token = self.views.get(&id).and_then(|view| {
                    view.event_permit
                        .same_generation(source_permit)
                        .then(|| view.event_permit.active_token())
                        .flatten()
                });
                let Some(token) = token else {
                    return;
                };
                // A crash event authorizes the shell to rebuild this logical
                // id. Remove and revoke the exact dead native generation
                // before that event can reach the shell.
                self.close(id);
                self.sink.emit_for(token, EngineEvent::Crashed { id });
            }
            // A callback can race an explicit close. The shell already owns
            // the resulting state transition, so a retired id is a no-op.
            RendererCrashTarget::Retired => {}
        }
    }

    #[cfg(target_os = "windows")]
    fn on_profile_process_exit(
        &mut self,
        profile: ProfileId,
        process_id: u32,
        generation: crate::platform::imp::BrowserProcessGeneration,
    ) {
        // The exact Environment5 callback may have recorded its proof and
        // queued settlement just before this equal-key ProcessFailed task
        // replaced it. Erasure has transferred the HANDLE out of the normal
        // maps, so settle from the observer's durable proof state first.
        if self.settle_transferred_profile_erasure_exit(profile, process_id, generation) {
            return;
        }
        // ProcessFailed and BrowserProcessExited are explicitly unordered.
        // A delayed callback from an old controller must never retire a newer
        // environment that reused the same logical ProfileId.
        let Some(process) = self.browser_processes.get(&profile) else {
            return;
        };
        let Some(observer) = self.browser_process_exit_observers.get(&profile) else {
            return;
        };
        if observer.expected_process_id() != process_id
            || !crate::platform::imp::browser_process_callback_matches(
                process.id(),
                observer.generation(),
                process_id,
                generation,
            )
        {
            return;
        }
        self.exiting_browser_processes.insert(profile);
        let ids = self.retire_profile_process_views(profile);
        self.remember_profile_recovery_ids(profile, ids);

        let observer = self.browser_process_exit_observers.get(&profile);
        if observer.is_some_and(crate::platform::imp::BrowserProcessExitObserver::is_invalid) {
            self.unverifiable_browser_processes.insert(profile);
        } else if observer
            .is_some_and(crate::platform::imp::BrowserProcessExitObserver::observed_expected_exit)
        {
            // If the Environment5 callback was delivered first, its adjacent
            // host task may be coalesced by this ProcessFailed task. The proof
            // is recorded before queuing, so this ordering still finalizes the
            // matching generation and never a replacement process.
            self.finalize_and_authorize_profile_recovery(profile, process_id, generation);
        }
    }

    #[cfg(target_os = "windows")]
    fn on_browser_process_exit_event(
        &mut self,
        profile: ProfileId,
        event: crate::platform::imp::BrowserProcessExitEvent,
    ) {
        use crate::platform::imp::BrowserProcessExitEvent;

        let (expected_process_id, generation) = match event {
            BrowserProcessExitEvent::Exited {
                expected_process_id,
                generation,
                ..
            }
            | BrowserProcessExitEvent::Invalid {
                expected_process_id,
                generation,
            } => (expected_process_id, generation),
        };
        if self.settle_transferred_profile_erasure_exit(profile, expected_process_id, generation) {
            return;
        }
        // Generation correlation is mandatory for both event families. A
        // delayed callback owned by an already-retired observer is a no-op.
        let Some(observer) = self.browser_process_exit_observers.get(&profile) else {
            return;
        };
        if observer.expected_process_id() != expected_process_id
            || observer.generation() != generation
        {
            return;
        }
        let Some(process) = self.browser_processes.get(&profile) else {
            // Outside terminal erasure, an observer without its retained
            // exact HANDLE is an unverifiable native lifecycle. Fail closed;
            // numeric PID correlation must never authorize recovery.
            self.quarantine_unverifiable_windows_profile(profile);
            return;
        };
        if !crate::platform::imp::browser_process_callback_matches(
            process.id(),
            observer.generation(),
            expected_process_id,
            generation,
        ) {
            // This event exactly matches the currently registered observer,
            // so a disagreeing retained process is not merely a stale task:
            // the profile's native provenance is internally inconsistent.
            self.quarantine_unverifiable_windows_profile(profile);
            return;
        }

        let group_exit_matches = match event {
            BrowserProcessExitEvent::Exited {
                observed_process_id,
                ..
            } => observed_process_id == expected_process_id,
            BrowserProcessExitEvent::Invalid { .. } => false,
        };
        if !group_exit_matches {
            // The registration fired but did not prove the exact generation.
            // Close controllers to drive the expected group toward exit, but
            // retain all provenance and keep erasure permanently fail-closed.
            self.unverifiable_browser_processes.insert(profile);
            self.exiting_browser_processes.insert(profile);
            let ids = self.retire_profile_process_views(profile);
            self.remember_profile_recovery_ids(profile, ids);
            return;
        }

        // BrowserProcessExited means the whole process group and UDF resources
        // for this exact PID are released. It can subsume a coalesced
        // ProcessFailed callback, so retire any still-associated controllers.
        let ids = self.retire_profile_process_views(profile);
        self.remember_profile_recovery_ids(profile, ids);
        self.finalize_and_authorize_profile_recovery(profile, expected_process_id, generation);
    }

    /// Settles the observer half retained on the UI apartment after profile
    /// erasure transferred its exact process HANDLE and cloneable exit proof
    /// to the bounded erasure continuation. Returns `true` whenever the
    /// profile is tombstoned, including stale/pending callbacks that must not
    /// enter ordinary recovery.
    #[cfg(target_os = "windows")]
    fn settle_transferred_profile_erasure_exit(
        &mut self,
        profile: ProfileId,
        process_id: u32,
        generation: crate::platform::imp::BrowserProcessGeneration,
    ) -> bool {
        if !self.erasure_tombstones.contains(&profile) {
            return false;
        }
        let settlement = self.browser_process_exit_observers.get(&profile).map_or(
            TransferredErasureExitSettlement::Stale,
            |observer| {
                transferred_erasure_exit_settlement(
                    observer.expected_process_id(),
                    process_id,
                    observer.generation() == generation,
                    observer.observed_expected_exit(),
                    observer.is_invalid(),
                )
            },
        );
        match settlement {
            TransferredErasureExitSettlement::Stale | TransferredErasureExitSettlement::Pending => {
            }
            TransferredErasureExitSettlement::Proven => {
                self.browser_process_exit_observers.remove(&profile);
                self.exiting_browser_processes.remove(&profile);
            }
            TransferredErasureExitSettlement::Invalid => {
                self.unverifiable_browser_processes.insert(profile);
                self.browser_process_exit_observers.remove(&profile);
                self.exiting_browser_processes.remove(&profile);
            }
        }
        true
    }

    /// Permanently fail-closes a profile whose WebView2 process provenance can
    /// no longer be proved. Every controller for the profile is retired so a
    /// still-live sibling cannot keep using storage owned by an untrusted or
    /// mismatched process generation. The sticky unverifiable marker prevents
    /// reconstruction even if the known Environment5 observer later proves
    /// that its own process group exited.
    #[cfg(target_os = "windows")]
    fn quarantine_unverifiable_windows_profile(&mut self, profile: ProfileId) {
        self.unverifiable_browser_processes.insert(profile);
        self.exiting_browser_processes.insert(profile);
        let ids = self.retire_profile_process_views(profile);
        self.remember_profile_recovery_ids(profile, ids);
    }

    #[cfg(target_os = "windows")]
    fn retire_profile_process_views(&mut self, profile: ProfileId) -> Vec<ItemId> {
        let mut ids: Vec<ItemId> = self
            .partitions
            .iter()
            .filter_map(|(id, partition)| (partition.profile() == profile).then_some(*id))
            .collect();
        ids.sort();
        for id in &ids {
            self.close(*id);
        }
        if self
            .spare
            .as_ref()
            .is_some_and(|spare| spare.partition.profile() == profile)
        {
            self.spare = None;
        }
        // The old context only carries this environment's UDF path. Recreate
        // it only after the full process-group event retires the generation.
        self.web_contexts.remove(&profile);
        ids
    }

    #[cfg(target_os = "windows")]
    fn remember_profile_recovery_ids(&mut self, profile: ProfileId, ids: Vec<ItemId>) {
        if ids.is_empty() {
            return;
        }
        let recovery = self.pending_profile_recovery.entry(profile).or_default();
        recovery.extend(ids);
        recovery.sort();
        recovery.dedup();
        if recovery.len() > zephium_core::session::MAX_SESSION_ITEMS {
            // This cannot occur through an admitted session, but a native
            // lifecycle inconsistency must not become an unbounded queue or an
            // incomplete recreation authorization.
            recovery.clear();
            self.unverifiable_browser_processes.insert(profile);
        }
    }

    #[cfg(target_os = "windows")]
    fn finalize_and_authorize_profile_recovery(
        &mut self,
        profile: ProfileId,
        process_id: u32,
        generation: crate::platform::imp::BrowserProcessGeneration,
    ) {
        if !self.finalize_browser_process_exit(profile, process_id, generation) {
            return;
        }
        let ids = self
            .pending_profile_recovery
            .remove(&profile)
            .unwrap_or_default();
        if self.unverifiable_browser_processes.contains(&profile) {
            // Exact release of the retained environment does not repair an
            // earlier missing/mismatched process obligation. Do not tell the
            // shell that reconstruction is authorized for a profile that is
            // intentionally poisoned until process restart/remediation.
            return;
        }
        if !ids.is_empty() {
            // The exiting gate and exact environment/process records were
            // cleared before this synchronous event. The shell may therefore
            // recreate every visible member without racing the old process.
            self.sink
                .emit(EngineEvent::ProfileProcessExited { profile, ids });
        }
    }

    #[cfg(target_os = "windows")]
    fn finalize_browser_process_exit(
        &mut self,
        profile: ProfileId,
        process_id: u32,
        generation: crate::platform::imp::BrowserProcessGeneration,
    ) -> bool {
        let Some(observer) = self.browser_process_exit_observers.get(&profile) else {
            return false;
        };
        if observer.expected_process_id() != process_id || observer.generation() != generation {
            return false;
        }
        let retained_process = self
            .browser_processes
            .get(&profile)
            .map(|process| (process.id(), process.has_exited()));
        if !exact_browser_process_exit_proves_recovery(
            retained_process,
            observer.expected_process_id(),
            process_id,
            observer.generation() == generation,
        ) {
            // Environment5 and the retained exact HANDLE disagree. Never use
            // numeric PID/event correlation alone to authorize a replacement.
            // Missing handles, WAIT_TIMEOUT, WAIT_FAILED and every unexpected
            // wait result all remain fail-closed.
            self.unverifiable_browser_processes.insert(profile);
            return false;
        }
        self.browser_version_observers.remove(&profile);
        self.environments.remove(&profile);
        self.browser_processes.remove(&profile);
        self.browser_process_exit_observers.remove(&profile);
        self.exiting_browser_processes.remove(&profile);
        true
    }

    /// Suspends the given hidden views (shell idle policy). Only Windows has
    /// an explicit primitive; WebKit suspends hidden/unmapped processes on
    /// its own. Resume is implicit: WebView2 wakes a view on SetIsVisible.
    pub(crate) fn set_dormant(&mut self, ids: Vec<ItemId>) {
        #[cfg(target_os = "windows")]
        {
            let next: std::collections::HashSet<ItemId> = ids
                .into_iter()
                .filter(|id| self.hidden.contains(id))
                .collect();
            let wake: Vec<ItemId> = self.dormant.difference(&next).copied().collect();
            for id in wake {
                if let Some(view) = self.views.get(&id) {
                    crate::platform::imp::resume(view);
                }
                self.dormant.remove(&id);
            }
            self.suspend_failed.retain(|id| next.contains(id));
            self.desired_dormant = next;
            self.pump_suspends();
        }
        #[cfg(not(target_os = "windows"))]
        let _ = ids;
    }

    #[cfg(target_os = "windows")]
    fn on_suspend_result(&mut self, id: ItemId, suspended: bool) {
        self.suspending.remove(&id);
        if suspended && self.desired_dormant.contains(&id) && self.hidden.contains(&id) {
            self.dormant.insert(id);
        } else if suspended {
            // The desired state changed while the async request was in flight.
            if let Some(view) = self.views.get(&id) {
                crate::platform::imp::resume(view);
            }
        } else if self.desired_dormant.contains(&id) {
            // Do not spin on runtimes that cannot suspend. Becoming visible
            // clears this marker, so a later hide cycle can retry.
            self.suspend_failed.insert(id);
        }
        self.pump_suspends();
    }

    #[cfg(target_os = "windows")]
    fn pump_suspends(&mut self) {
        while self.suspending.len() < MAX_CONCURRENT_SUSPENDS {
            let mut candidates: Vec<ItemId> = self
                .desired_dormant
                .iter()
                .filter(|id| {
                    !self.dormant.contains(id)
                        && !self.suspending.contains(id)
                        && !self.suspend_failed.contains(id)
                })
                .copied()
                .collect();
            candidates.sort();
            let Some(id) = candidates.first().copied() else {
                break;
            };
            self.suspending.insert(id);
            let started = self.views.get(&id).is_some_and(|view| {
                crate::platform::imp::try_suspend(view, move |suspended| {
                    with_suspend_result(id, move |host| host.on_suspend_result(id, suspended));
                })
            });
            if !started {
                // Keep synchronous admission failure iterative. Recursively
                // pumping a runtime that rejects every request would consume
                // one stack frame per dormant tab.
                self.suspending.remove(&id);
                if self.desired_dormant.contains(&id) {
                    self.suspend_failed.insert(id);
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn set_content(
        &mut self,
        window: WindowId,
        tree: Option<Pane>,
        region: Option<Rect>,
    ) {
        let Some(stage) = self.ensure_stage(window) else {
            return;
        };
        match region {
            None => stage.setHidden(true),
            Some(r) => {
                stage_set_frame(&stage, &self.parent, r);
                let tabs = tree.as_ref().map(Pane::tabs).unwrap_or_default();
                let mut invalid_views = Vec::new();
                for id in &tabs {
                    if !stage.has_view(*id) {
                        if let Some(view) = self.views.get(id) {
                            if let Some(native_view) = webview_nsview(view) {
                                stage.insert_view(*id, native_view);
                            } else {
                                invalid_views.push(*id);
                            }
                        }
                    }
                }
                if !invalid_views.is_empty() {
                    stage.setHidden(true);
                    for id in invalid_views {
                        let token = self
                            .views
                            .get(&id)
                            .and_then(|view| view.event_permit.active_token());
                        self.close(id);
                        if let Some(token) = token {
                            self.sink
                                .emit_for(token, EngineEvent::ViewCreationFailed { id });
                        }
                    }
                    return;
                }
                stage.set_tree(tree);
                stage.set_visible(&tabs);
                stage.setHidden(false);
            }
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn set_drop_indicator(&mut self, window: WindowId, zone: Option<Rect>) {
        if let Some(stage) = self.ensure_stage(window) {
            stage.set_drop_indicator(zone);
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn set_content(
        &mut self,
        window: WindowId,
        tree: Option<Pane>,
        region: Option<Rect>,
    ) {
        let Some(stage) = self.ensure_stage(window) else {
            return;
        };
        let tabs = match region {
            Some(_) => tree.as_ref().map(Pane::tabs).unwrap_or_default(),
            None => Vec::new(),
        };
        for id in &tabs {
            if !stage.has_view(*id) {
                if let Some(view) = self.views.get(id) {
                    stage.insert_view(*id, view);
                }
            }
        }
        // one native pass: frame, tree and visibility land atomically, so a
        // switch can never flash the previous pane
        stage.apply(region, tree, &tabs);
        // Off-screen views drop to the low-memory hint (reversible, nothing
        // freezes); actual suspension waits for the shell's idle verdict.
        // Becoming visible resumes a suspended view natively.
        #[cfg(target_os = "windows")]
        {
            use wry::{MemoryUsageLevel, WebViewExtWindows};
            for (id, view) in &self.views {
                let off = !tabs.contains(id);
                if off == self.hidden.contains(id) {
                    continue;
                }
                if off {
                    self.hidden.insert(*id);
                    let _ = view.set_memory_usage_level(MemoryUsageLevel::Low);
                } else {
                    self.hidden.remove(id);
                    self.dormant.remove(id);
                    self.desired_dormant.remove(id);
                    self.suspending.remove(id);
                    self.suspend_failed.remove(id);
                    let _ = view.set_memory_usage_level(MemoryUsageLevel::Normal);
                }
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn set_drop_indicator(&mut self, window: WindowId, zone: Option<Rect>) {
        if let Some(stage) = self.ensure_stage(window) {
            stage.set_drop_indicator(zone);
        }
    }

    #[cfg(target_os = "windows")]
    fn ensure_stage(&mut self, window: WindowId) -> Option<Stage> {
        if let Some(stage) = self.stages.get(&window) {
            return Some(stage.clone());
        }
        let RawWindowHandle::Win32(h) = self.parent.0 else {
            return None;
        };
        let parent = HWND(h.hwnd.get() as *mut _);
        let stage = Stage::new(parent, GAP);
        self.stages.insert(window, stage.clone());
        Some(stage)
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    fn ensure_stage(&mut self, window: WindowId) -> Option<crate::platform::imp::Stage> {
        if let Some(stage) = self.stages.get(&window) {
            return Some(stage.clone());
        }
        let fixed = crate::platform::imp::container()?;
        let stage = crate::platform::imp::Stage::new(fixed, GAP);
        self.stages.insert(window, stage.clone());
        Some(stage)
    }

    #[cfg(target_os = "macos")]
    fn ensure_stage(&mut self, window: WindowId) -> Option<Retained<ContentStage>> {
        if let Some(stage) = self.stages.get(&window) {
            return Some(stage.clone());
        }
        let mtm = MainThreadMarker::new()?;
        let content = content_view(&self.parent)?;
        let stage = ContentStage::new(mtm, GAP);
        let sink = self.sink.clone();
        stage.set_on_ratio(Box::new(move |tree| {
            sink.emit(EngineEvent::SplitChanged { window, tree })
        }));
        content.addSubview(&stage);
        self.stages.insert(window, stage.clone());
        Some(stage)
    }
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

#[cfg(target_os = "macos")]
fn content_view(parent: &ParentHandle) -> Option<Retained<NSView>> {
    if let RawWindowHandle::AppKit(h) = parent.0 {
        return unsafe { Retained::retain(h.ns_view.as_ptr() as *mut NSView) };
    }
    None
}

#[cfg(target_os = "macos")]
fn webview_nsview(view: &WebView) -> Option<Retained<NSView>> {
    use wry::WebViewExtMacOS;
    let wk = view.webview();
    unsafe { Retained::retain(Retained::as_ptr(&wk) as *mut NSView) }
}

#[cfg(target_os = "macos")]
fn stage_set_frame(stage: &ContentStage, parent: &ParentHandle, r: Rect) {
    use objc2_app_kit::NSAutoresizingMaskOptions as Mask;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let Some(content) = content_view(parent) else {
        return;
    };
    let h = content.bounds().size.height;
    stage.setFrame(NSRect::new(
        NSPoint::new(r.x, h - r.y - r.height),
        NSSize::new(r.width, r.height),
    ));
    stage.setAutoresizingMask(Mask::ViewWidthSizable | Mask::ViewHeightSizable);
}

fn to_wry(r: Rect) -> wry::Rect {
    wry::Rect {
        position: Position::Logical(LogicalPosition::new(r.x, r.y)),
        size: Size::Logical(LogicalSize::new(r.width, r.height)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_resource_ceiling_counts_spare_and_every_cleanup_debt() {
        assert_eq!(owned_native_view_resources(32, false, 0), Some(32));
        assert_eq!(owned_native_view_resources(32, true, 0), Some(33));
        assert_eq!(owned_native_view_resources(32, true, 8), Some(41));
        assert_eq!(owned_native_view_resources(usize::MAX, true, 0), None);
    }

    #[test]
    fn native_profile_process_group_ceiling_counts_distinct_profiles_only() {
        let profiles: Vec<_> = (1..=MAX_NATIVE_PROFILE_PROCESS_GROUPS)
            .map(|id| ProfileId::from(id as u128))
            .collect();
        let existing = profiles[0];
        let new = ProfileId::from(10_000);

        assert!(profile_process_group_capacity_allows(
            profiles.iter().copied().chain([existing, existing]),
            existing,
        ));
        assert!(!profile_process_group_capacity_allows(
            profiles.iter().copied().chain([profiles[0]]),
            new,
        ));
        assert!(profile_process_group_capacity_allows(
            profiles[..MAX_NATIVE_PROFILE_PROCESS_GROUPS - 1]
                .iter()
                .copied(),
            new,
        ));
    }

    #[test]
    fn native_construction_reservation_is_bounded_and_released_exactly() {
        let mut reservations = NativeViewReservations::default();
        assert_eq!(
            reservations.try_reserve(MAX_NATIVE_VIEW_RESOURCES - 1),
            Ok(true)
        );
        assert_eq!(reservations.in_construction(), 1);
        // Re-entry/retry observes the first construction reservation and may
        // not allocate the forty-ninth native resource.
        assert_eq!(
            reservations.try_reserve(MAX_NATIVE_VIEW_RESOURCES - 1),
            Ok(false)
        );
        assert_eq!(reservations.in_construction(), 1);
        assert_eq!(reservations.release(), Ok(()));
        assert_eq!(reservations.in_construction(), 0);
        assert_eq!(reservations.release(), Err(()));
    }

    fn queued(priority: HostTaskPriority) -> QueuedHostTask {
        QueuedHostTask {
            priority,
            key: None,
            task: Box::new(|_: &mut EngineHost| {}),
        }
    }

    fn keyed(priority: HostTaskPriority, key: HostTaskKey) -> QueuedHostTask {
        QueuedHostTask {
            priority,
            key: Some(key),
            task: Box::new(|_: &mut EngineHost| {}),
        }
    }

    #[test]
    fn tasks_are_rejected_when_the_host_is_unavailable() {
        HOST_SEALED.with(|sealed| sealed.set(false));
        HOST.with(|host| *host.borrow_mut() = None);
        PENDING.with(|pending| pending.borrow_mut().clear());
        assert!(!try_with(|_| panic!(
            "an unavailable host must not run work"
        )));
        assert!(!try_with_close(ItemId::from(1), |_| panic!(
            "an unavailable host must not admit close"
        )));
        assert!(PENDING.with(|pending| pending.borrow().is_empty()));
    }

    #[test]
    fn native_event_permit_is_one_shot_and_generation_exact() {
        let first = Arc::new(AtomicBool::new(true));
        let replacement = Arc::new(AtomicBool::new(true));
        let permit = EventPermit::inactive();
        let callback_copy = permit.clone();

        assert!(permit.bind_once(&first));
        assert!(permit.matches_token(&first));
        assert!(callback_copy.same_generation(&permit));
        assert!(!callback_copy.bind_once(&replacement));
        assert!(!permit.matches_token(&replacement));

        permit.revoke();
        assert!(callback_copy.active_token().is_none());
        assert!(!permit.bind_once(&replacement));

        let replacement_permit = EventPermit::bound(&replacement);
        assert!(!replacement_permit.same_generation(&permit));
        assert!(replacement_permit.matches_token(&replacement));
    }

    #[test]
    fn revoking_an_inactive_spare_is_terminal() {
        let token = Arc::new(AtomicBool::new(true));
        let permit = EventPermit::inactive();
        let callback_copy = permit.clone();

        assert!(permit.allows_navigation("about:blank"));
        permit.revoke();

        assert!(!callback_copy.allows_navigation("about:blank"));
        assert!(!callback_copy.bind_once(&token));
        assert!(permit.active_token().is_none());
    }

    #[test]
    fn adopted_spare_rejects_late_bootstrap_navigation_callbacks() {
        let token = Arc::new(AtomicBool::new(true));
        let permit = EventPermit::inactive();
        let navigation = NavigationEpochTracker::new();

        let bootstrap = navigation
            .begin("about:blank")
            .expect("bootstrap navigation epoch");
        assert_eq!(
            navigation.observe_load("about:blank", &PageLoadEvent::Started),
            Some(bootstrap)
        );

        assert!(permit.bind_once(&token));
        let adopted = navigation
            .begin("https://example.com/")
            .expect("adopted navigation epoch");
        assert_ne!(bootstrap, adopted);
        assert_eq!(navigation.current_committed(), None);

        // These callbacks belong to the spare's already-retired blank load.
        // Binding the native generation to a real item must not let them
        // acquire the adopted navigation epoch.
        assert!(navigation.admit_target(&permit, "about:blank"));
        assert!(navigation.is_current(adopted));
        assert_eq!(
            navigation.observe_load("about:blank", &PageLoadEvent::Finished),
            None
        );
        assert!(!navigation.observe_source(adopted, "about:blank"));
        assert!(navigation.is_current(adopted));

        assert_eq!(
            navigation.observe_load("https://example.com/", &PageLoadEvent::Started),
            Some(adopted)
        );
        assert!(navigation.observe_source(adopted, "https://example.com/"));
        assert_eq!(navigation.current_committed(), Some(adopted));
    }

    #[test]
    fn navigation_callbacks_require_generation_tracker_and_epoch_identity() {
        let token = Arc::new(AtomicBool::new(true));
        let permit = EventPermit::bound(&token);
        let callback_permit = permit.clone();
        let navigation = NavigationEpochTracker::new();
        let callback_navigation = navigation.clone();
        let first = navigation
            .begin("https://example.com/first")
            .expect("first navigation epoch");

        assert!(navigation_callback_matches(
            &permit,
            &navigation,
            &callback_permit,
            &callback_navigation,
            first,
        ));

        let second = navigation
            .begin("https://example.com/second")
            .expect("second navigation epoch");
        assert!(!navigation_callback_matches(
            &permit,
            &navigation,
            &callback_permit,
            &callback_navigation,
            first,
        ));
        assert!(navigation_callback_matches(
            &permit,
            &navigation,
            &callback_permit,
            &callback_navigation,
            second,
        ));

        // The same outer token does not make a replacement native generation
        // or a distinct epoch tracker equivalent to the callback owner.
        let replacement_permit = EventPermit::bound(&token);
        assert!(!navigation_callback_matches(
            &replacement_permit,
            &navigation,
            &callback_permit,
            &callback_navigation,
            second,
        ));
        let replacement_navigation = NavigationEpochTracker::new();
        let coincident = replacement_navigation
            .begin("https://example.com/second")
            .expect("replacement navigation epoch");
        assert!(!navigation_callback_matches(
            &permit,
            &replacement_navigation,
            &callback_permit,
            &callback_navigation,
            coincident,
        ));
    }

    #[test]
    fn shutdown_seals_only_after_the_barrier_is_admitted() {
        HOST_SEALED.with(|sealed| sealed.set(false));
        HOST.with(|host| *host.borrow_mut() = None);
        PENDING.with(|pending| pending.borrow_mut().clear());
        assert!(!with_priority(
            HostTaskPriority::Shutdown,
            None,
            |_| panic!("unavailable host must not run shutdown")
        ));
        assert!(!HOST_SEALED.with(Cell::get));

        HOST.with(|host| {
            let _borrow = host.borrow_mut();
            assert!(with_priority(HostTaskPriority::Shutdown, None, |_| panic!(
                "reentrant shutdown must be queued"
            )));
            assert!(HOST_SEALED.with(Cell::get));
            assert!(!try_with(|_| panic!("sealed host must reject later work")));
        });
        assert_eq!(PENDING.with(|pending| pending.borrow().len()), 1);
        PENDING.with(|pending| pending.borrow_mut().clear());
        HOST_SEALED.with(|sealed| sealed.set(false));
    }

    #[test]
    fn reentrant_queue_bounds_shutdown_and_prioritizes_close() {
        let mut pending = VecDeque::new();
        for _ in 0..NORMAL_PENDING_HOST_TASK_CAPACITY {
            assert!(enqueue_pending(
                &mut pending,
                queued(HostTaskPriority::Normal)
            ));
        }
        assert!(!enqueue_pending(
            &mut pending,
            queued(HostTaskPriority::Normal)
        ));
        for _ in pending.len()..NON_PROFILE_ERASURE_PENDING_HOST_TASK_CAPACITY {
            assert!(enqueue_pending(
                &mut pending,
                queued(HostTaskPriority::Observation)
            ));
        }
        assert!(enqueue_pending(
            &mut pending,
            queued(HostTaskPriority::Close)
        ));
        assert_eq!(
            pending.len(),
            NON_PROFILE_ERASURE_PENDING_HOST_TASK_CAPACITY
        );
        assert_eq!(pending.back().unwrap().priority, HostTaskPriority::Close);

        for _ in 0..PROFILE_ERASURE_PENDING_HOST_TASK_CAPACITY {
            assert!(enqueue_pending(
                &mut pending,
                queued(HostTaskPriority::ProfileErasure)
            ));
        }
        assert_eq!(pending.len(), NON_SHUTDOWN_PENDING_HOST_TASK_CAPACITY);
        assert!(enqueue_pending(
            &mut pending,
            queued(HostTaskPriority::Shutdown)
        ));
        assert_eq!(pending.len(), PENDING_HOST_TASK_CAPACITY);
        assert_eq!(pending.back().unwrap().priority, HostTaskPriority::Shutdown);
        assert!(!enqueue_pending(
            &mut pending,
            queued(HostTaskPriority::Shutdown)
        ));
        assert_eq!(
            pending
                .iter()
                .filter(|task| task.priority == HostTaskPriority::Shutdown)
                .count(),
            1
        );
    }

    #[test]
    fn erasure_and_shutdown_slots_survive_full_lifecycle_saturation() {
        let mut pending = VecDeque::new();
        for _ in 0..NON_PROFILE_ERASURE_PENDING_HOST_TASK_CAPACITY {
            assert!(enqueue_pending(
                &mut pending,
                queued(HostTaskPriority::Lifecycle)
            ));
        }
        assert!(!enqueue_pending(
            &mut pending,
            queued(HostTaskPriority::Lifecycle)
        ));

        for _ in 0..PROFILE_ERASURE_PENDING_HOST_TASK_CAPACITY {
            assert!(enqueue_pending(
                &mut pending,
                queued(HostTaskPriority::ProfileErasure)
            ));
        }
        assert_eq!(pending.len(), NON_SHUTDOWN_PENDING_HOST_TASK_CAPACITY);
        assert_eq!(
            pending
                .iter()
                .filter(|task| task.priority == HostTaskPriority::ProfileErasure)
                .count(),
            PROFILE_ERASURE_PENDING_HOST_TASK_CAPACITY
        );
        assert!(!enqueue_pending(
            &mut pending,
            queued(HostTaskPriority::ProfileErasure)
        ));
        assert_eq!(
            pending
                .iter()
                .filter(|task| task.priority == HostTaskPriority::Lifecycle)
                .count(),
            NON_PROFILE_ERASURE_PENDING_HOST_TASK_CAPACITY
        );
        assert_eq!(
            pending
                .iter()
                .filter(|task| task.priority == HostTaskPriority::ProfileErasure)
                .count(),
            PROFILE_ERASURE_PENDING_HOST_TASK_CAPACITY
        );

        assert!(enqueue_pending(
            &mut pending,
            queued(HostTaskPriority::Shutdown)
        ));
        assert_eq!(pending.len(), PENDING_HOST_TASK_CAPACITY);
        assert_eq!(
            pending
                .iter()
                .filter(|task| task.priority == HostTaskPriority::Lifecycle)
                .count(),
            NON_PROFILE_ERASURE_PENDING_HOST_TASK_CAPACITY
        );
        assert_eq!(
            pending
                .iter()
                .filter(|task| task.priority == HostTaskPriority::Shutdown)
                .count(),
            1
        );
    }

    #[test]
    fn keyed_native_callbacks_coalesce_and_stronger_lifecycle_wins() {
        let id = ItemId::from(7);
        let key = HostTaskKey::View(id);
        let mut pending = VecDeque::new();

        assert!(enqueue_pending(
            &mut pending,
            keyed(HostTaskPriority::Observation, key)
        ));
        assert!(enqueue_pending(
            &mut pending,
            keyed(HostTaskPriority::Observation, key)
        ));
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].priority, HostTaskPriority::Observation);

        assert!(enqueue_pending(
            &mut pending,
            keyed(HostTaskPriority::Lifecycle, key)
        ));
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].priority, HostTaskPriority::Lifecycle);

        // A stale KVO/SourceChanged callback racing after process death is
        // safely subsumed and cannot replace the queued crash transition.
        assert!(enqueue_pending(
            &mut pending,
            keyed(HostTaskPriority::Observation, key)
        ));
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].priority, HostTaskPriority::Lifecycle);
    }

    #[test]
    fn keyed_coalescing_never_crosses_intervening_host_work_below_capacity() {
        let key = HostTaskKey::View(ItemId::from(7));
        let mut pending = VecDeque::new();

        assert!(enqueue_pending(
            &mut pending,
            keyed(HostTaskPriority::Observation, key)
        ));
        assert!(enqueue_pending(
            &mut pending,
            queued(HostTaskPriority::Normal)
        ));
        assert!(enqueue_pending(
            &mut pending,
            keyed(HostTaskPriority::Observation, key)
        ));

        assert_eq!(pending.len(), 3);
        assert_eq!(pending[0].key, Some(key));
        assert_eq!(pending[1].priority, HostTaskPriority::Normal);
        assert_eq!(pending[2].key, Some(key));
    }

    #[test]
    fn same_key_may_cross_an_ordering_barrier_only_at_absolute_overload() {
        let key = HostTaskKey::View(ItemId::from(7));
        let mut pending = VecDeque::new();
        assert!(enqueue_pending(
            &mut pending,
            keyed(HostTaskPriority::Observation, key)
        ));
        assert!(enqueue_pending(
            &mut pending,
            queued(HostTaskPriority::Normal)
        ));
        while pending.len() < NON_PROFILE_ERASURE_PENDING_HOST_TASK_CAPACITY {
            assert!(enqueue_pending(
                &mut pending,
                queued(HostTaskPriority::Observation)
            ));
        }

        assert!(enqueue_pending(
            &mut pending,
            keyed(HostTaskPriority::Observation, key)
        ));
        assert_eq!(
            pending.len(),
            NON_PROFILE_ERASURE_PENDING_HOST_TASK_CAPACITY
        );
        assert_eq!(pending.front().unwrap().priority, HostTaskPriority::Normal);
        assert_eq!(pending.back().unwrap().key, Some(key));
        assert_eq!(
            pending.iter().filter(|task| task.key == Some(key)).count(),
            1
        );
    }

    #[test]
    fn native_callbacks_use_the_reserved_band_beyond_normal_saturation() {
        let mut pending = VecDeque::new();
        for _ in 0..NORMAL_PENDING_HOST_TASK_CAPACITY {
            assert!(enqueue_pending(
                &mut pending,
                queued(HostTaskPriority::Normal)
            ));
        }
        assert!(!enqueue_pending(
            &mut pending,
            queued(HostTaskPriority::Normal)
        ));
        assert!(enqueue_pending(
            &mut pending,
            keyed(
                HostTaskPriority::Observation,
                HostTaskKey::View(ItemId::from(1))
            )
        ));
        #[cfg(target_os = "windows")]
        assert!(enqueue_pending(
            &mut pending,
            keyed(
                HostTaskPriority::Maintenance,
                HostTaskKey::Suspend(ItemId::from(1))
            )
        ));
        assert!(enqueue_pending(
            &mut pending,
            queued(HostTaskPriority::ProfileErasure)
        ));
        assert_eq!(
            pending.back().map(|task| task.priority),
            Some(HostTaskPriority::ProfileErasure)
        );
    }

    #[test]
    fn profile_id_never_crosses_durable_and_ephemeral_storage_classes() {
        let durable_first = ProfileId::from(31);
        let ephemeral_first = ProfileId::from(32);
        let mut bindings = HashMap::new();

        assert!(bind_profile_persistence_class(
            &mut bindings,
            Partition::Default(durable_first)
        ));
        assert!(bind_profile_persistence_class(
            &mut bindings,
            Partition::Persistent(durable_first)
        ));
        assert!(!bind_profile_persistence_class(
            &mut bindings,
            Partition::Ephemeral(durable_first)
        ));
        assert!(bind_profile_persistence_class(
            &mut bindings,
            Partition::Default(durable_first)
        ));

        assert!(bind_profile_persistence_class(
            &mut bindings,
            Partition::Ephemeral(ephemeral_first)
        ));
        assert!(!bind_profile_persistence_class(
            &mut bindings,
            Partition::Default(ephemeral_first)
        ));
        assert!(!bind_profile_persistence_class(
            &mut bindings,
            Partition::Persistent(ephemeral_first)
        ));
        assert!(bind_profile_persistence_class(
            &mut bindings,
            Partition::Ephemeral(ephemeral_first)
        ));
    }

    #[test]
    fn profile_scoped_values_reuse_within_profile_and_isolate_profiles() {
        let first = ProfileId::from(41);
        let second = ProfileId::from(42);
        let mut values = HashMap::new();
        let mut next = 100usize;

        let first_value = profile_scoped_value(&mut values, first, || {
            next += 1;
            Ok::<_, ()>(next)
        })
        .unwrap();
        let first_again = profile_scoped_value(&mut values, first, || {
            next += 1;
            Ok::<_, ()>(next)
        })
        .unwrap();
        let second_value = profile_scoped_value(&mut values, second, || {
            next += 1;
            Ok::<_, ()>(next)
        })
        .unwrap();

        assert_eq!(first_value, first_again);
        assert_ne!(first_value, second_value);
        assert_eq!(
            next, 102,
            "the same profile must not invoke the factory twice"
        );
        assert!(profile_value_is_isolated(
            &values,
            first,
            &first_value,
            |left, right| left == right,
        ));

        values.insert(second, first_value);
        assert!(!profile_value_is_isolated(
            &values,
            first,
            &first_value,
            |left, right| left == right,
        ));
    }

    #[test]
    fn profile_persistence_binding_is_bounded_without_evicting_old_proof() {
        let mut bindings = HashMap::new();
        for value in 0..MAX_PROFILE_PERSISTENCE_BINDINGS {
            assert!(bind_profile_persistence_class(
                &mut bindings,
                Partition::Persistent(ProfileId::from(value as u128 + 1))
            ));
        }
        assert!(!bind_profile_persistence_class(
            &mut bindings,
            Partition::Persistent(ProfileId::from(
                MAX_PROFILE_PERSISTENCE_BINDINGS as u128 + 1
            ))
        ));
        assert!(bind_profile_persistence_class(
            &mut bindings,
            Partition::Default(ProfileId::from(1))
        ));
        assert!(!bind_profile_persistence_class(
            &mut bindings,
            Partition::Ephemeral(ProfileId::from(1))
        ));
    }

    #[test]
    fn failed_profile_erasure_stays_tombstoned_and_is_retryable() {
        use std::sync::mpsc;

        let profile = ProfileId::from(44);
        let mut tombstones = HashSet::new();
        let mut attempts = HashMap::new();

        let (first_tx, first_rx) = mpsc::channel();
        let first = crate::erasure::Completion::start(
            Box::new(move |outcome| {
                first_tx.send(outcome).unwrap();
            }),
            Arc::new(AtomicBool::new(true)),
        );
        assert!(admit_profile_erasure(
            &mut tombstones,
            &mut attempts,
            profile,
            &first
        ));
        assert!(tombstones.contains(&profile));

        let (duplicate_tx, duplicate_rx) = mpsc::channel();
        let duplicate = crate::erasure::Completion::start(
            Box::new(move |outcome| {
                duplicate_tx.send(outcome).unwrap();
            }),
            Arc::new(AtomicBool::new(true)),
        );
        assert!(!admit_profile_erasure(
            &mut tombstones,
            &mut attempts,
            profile,
            &duplicate
        ));
        assert_eq!(
            duplicate_rx
                .recv_timeout(std::time::Duration::from_millis(100))
                .unwrap(),
            zephium_core::ports::engine::ProfileDataErasureOutcome::Failed
        );

        first.finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Failed);
        assert_eq!(
            first_rx
                .recv_timeout(std::time::Duration::from_millis(100))
                .unwrap(),
            zephium_core::ports::engine::ProfileDataErasureOutcome::Failed
        );
        assert!(tombstones.contains(&profile));

        let retry =
            crate::erasure::Completion::start(Box::new(|_| {}), Arc::new(AtomicBool::new(true)));
        assert!(admit_profile_erasure(
            &mut tombstones,
            &mut attempts,
            profile,
            &retry
        ));
        assert!(tombstones.contains(&profile));
        retry.finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Verified);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_manager_release_is_exact_attempt_and_terminal_only() {
        let completed = Arc::new(AtomicBool::new(true));
        let replacement = Arc::new(AtomicBool::new(false));

        assert!(!linux_erasure_release_matches(Some(&completed), &completed));
        completed.store(false, Ordering::Release);
        assert!(linux_erasure_release_matches(Some(&completed), &completed));
        assert!(!linux_erasure_release_matches(
            Some(&replacement),
            &completed
        ));
        assert!(!linux_erasure_release_matches(None, &completed));
    }

    #[test]
    fn macos_store_release_is_exact_attempt_and_terminal_only() {
        let completed = Arc::new(AtomicBool::new(true));
        let replacement = Arc::new(AtomicBool::new(false));

        assert!(!macos_erasure_release_matches(Some(&completed), &completed));
        completed.store(false, Ordering::Release);
        assert!(macos_erasure_release_matches(Some(&completed), &completed));
        assert!(!macos_erasure_release_matches(
            Some(&replacement),
            &completed
        ));
        assert!(!macos_erasure_release_matches(None, &completed));
    }

    #[test]
    fn caller_timeout_before_host_dispatch_still_tombstones_and_blocks_retry() {
        use std::sync::mpsc;

        let profile = ProfileId::from(45);
        let mut tombstones = HashSet::new();
        let mut attempts = HashMap::new();
        let (tx, rx) = mpsc::channel();
        let delayed = crate::erasure::Completion::start(
            Box::new(move |outcome| {
                tx.send(outcome).unwrap();
            }),
            Arc::new(AtomicBool::new(true)),
        );

        delayed.report_unsettled(zephium_core::ports::engine::ProfileDataErasureOutcome::TimedOut);
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_millis(100))
                .unwrap(),
            zephium_core::ports::engine::ProfileDataErasureOutcome::TimedOut
        );
        assert!(admit_profile_erasure(
            &mut tombstones,
            &mut attempts,
            profile,
            &delayed
        ));
        assert!(tombstones.contains(&profile));

        let retry =
            crate::erasure::Completion::start(Box::new(|_| {}), Arc::new(AtomicBool::new(true)));
        assert!(!admit_profile_erasure(
            &mut tombstones,
            &mut attempts,
            profile,
            &retry
        ));

        delayed.finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Verified);
        let settled_retry =
            crate::erasure::Completion::start(Box::new(|_| {}), Arc::new(AtomicBool::new(true)));
        assert!(admit_profile_erasure(
            &mut tombstones,
            &mut attempts,
            profile,
            &settled_retry
        ));
        settled_retry.finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Verified);
    }

    #[test]
    fn host_erasure_proof_maps_reject_new_profiles_at_profile_bound() {
        use std::sync::mpsc;

        let mut tombstones = HashSet::new();
        let mut attempts = HashMap::new();
        for value in 0..zephium_core::session::MAX_SESSION_PROFILES {
            let profile = ProfileId::from(value as u128 + 1);
            tombstones.insert(profile);
            attempts.insert(profile, Arc::new(AtomicBool::new(false)));
        }
        let (tx, rx) = mpsc::channel();
        let completion = crate::erasure::Completion::start(
            Box::new(move |outcome| tx.send(outcome).unwrap()),
            Arc::new(AtomicBool::new(true)),
        );

        assert!(!admit_profile_erasure(
            &mut tombstones,
            &mut attempts,
            ProfileId::from(100_000),
            &completion,
        ));
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_millis(100))
                .unwrap(),
            zephium_core::ports::engine::ProfileDataErasureOutcome::Failed
        );
        assert_eq!(
            tombstones.len(),
            zephium_core::session::MAX_SESSION_PROFILES
        );
        assert_eq!(attempts.len(), zephium_core::session::MAX_SESSION_PROFILES);
    }

    #[test]
    fn renderer_crash_classification_never_exposes_dead_spare() {
        let spare = ItemId::from(1);
        let live = ItemId::from(2);
        let retired = ItemId::from(3);
        assert_eq!(
            renderer_crash_target(Some(spare), false, spare),
            RendererCrashTarget::Spare
        );
        assert_eq!(
            renderer_crash_target(Some(spare), true, live),
            RendererCrashTarget::Live
        );
        assert_eq!(
            renderer_crash_target(Some(spare), false, retired),
            RendererCrashTarget::Retired
        );
    }

    #[test]
    fn attempted_webview2_construction_prevents_empty_maps_from_proving_absence() {
        assert!(browser_group_absence_is_proven(false, false, false));
        assert!(!browser_group_absence_is_proven(false, false, true));
        assert!(!browser_group_absence_is_proven(true, false, false));
        assert!(!browser_group_absence_is_proven(false, true, false));
    }

    #[test]
    fn recovery_requires_a_present_exact_signalled_browser_process() {
        assert!(exact_browser_process_exit_proves_recovery(
            Some((41, true)),
            41,
            41,
            true,
        ));
        assert!(!exact_browser_process_exit_proves_recovery(
            None, 41, 41, true,
        ));
        assert!(!exact_browser_process_exit_proves_recovery(
            Some((41, false)),
            41,
            41,
            true,
        ));
        assert!(!exact_browser_process_exit_proves_recovery(
            Some((42, true)),
            41,
            41,
            true,
        ));
        assert!(!exact_browser_process_exit_proves_recovery(
            Some((41, true)),
            41,
            42,
            true,
        ));
        assert!(!exact_browser_process_exit_proves_recovery(
            Some((41, true)),
            41,
            41,
            false,
        ));
    }

    #[test]
    fn successful_windows_profile_erasure_restores_the_shutdown_map_invariant() {
        assert_eq!(
            transferred_erasure_exit_settlement(41, 41, true, true, false),
            TransferredErasureExitSettlement::Proven
        );
        assert!(
            !windows_profile_provenance_presence_is_consistent(false, false, true, false),
            "an observer retained after transferring the other obligations must block shutdown"
        );
        assert!(
            windows_profile_provenance_presence_is_consistent(false, false, false, false),
            "the exact exit callback releases the final observer-only entry"
        );

        assert_eq!(
            transferred_erasure_exit_settlement(41, 41, true, false, true),
            TransferredErasureExitSettlement::Invalid
        );
        assert_eq!(
            transferred_erasure_exit_settlement(41, 41, false, true, false),
            TransferredErasureExitSettlement::Stale
        );
        assert_eq!(
            transferred_erasure_exit_settlement(41, 41, true, false, false),
            TransferredErasureExitSettlement::Pending
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn coalesced_process_failed_still_settles_a_transferred_exact_exit_proof() {
        let profile = ProfileId::from(91);
        let generation = crate::platform::imp::BrowserProcessGeneration::for_test(7);
        let key = HostTaskKey::Profile(profile, generation);
        let mut pending = VecDeque::new();

        // Environment5 records the shared proof before queuing its host task.
        // A reentrant ProcessFailed callback then replaces that adjacent task
        // because both lifecycle facts intentionally share the exact key.
        assert!(enqueue_pending(
            &mut pending,
            keyed(HostTaskPriority::Lifecycle, key)
        ));
        assert!(enqueue_pending(
            &mut pending,
            keyed(HostTaskPriority::Lifecycle, key)
        ));
        assert_eq!(pending.len(), 1);
        assert_eq!(pending.front().and_then(|task| task.key), Some(key));

        // The surviving ProcessFailed path must consult the already-recorded
        // observer state. It releases the observer-only map entry, restoring
        // the exact empty-set shutdown invariant after successful erasure.
        assert_eq!(
            transferred_erasure_exit_settlement(41, 41, true, true, false),
            TransferredErasureExitSettlement::Proven
        );
        assert!(windows_profile_provenance_presence_is_consistent(
            false, false, false, false
        ));
    }

    #[test]
    fn native_navigation_observations_are_bounded_deduplicated_and_filtered() {
        let id = ItemId::from(7);
        let mut previous = NavigationSnapshot::default();

        let initial = navigation_observation_events(
            id,
            &mut previous,
            Some("https://example.com/".to_owned()),
            Some((false, false)),
        );
        assert_eq!(initial.len(), 2);
        assert!(matches!(
            &initial[0],
            EngineEvent::UrlChanged { id: observed, url }
                if *observed == id && url == "https://example.com/"
        ));
        assert!(matches!(
            initial[1],
            EngineEvent::NavState {
                id: observed,
                can_go_back: false,
                can_go_forward: false,
            } if observed == id
        ));

        assert!(navigation_observation_events(
            id,
            &mut previous,
            Some("https://example.com/".to_owned()),
            Some((false, false)),
        )
        .is_empty());

        let same_document = navigation_observation_events(
            id,
            &mut previous,
            Some("https://example.com/#state".to_owned()),
            Some((true, false)),
        );
        assert_eq!(same_document.len(), 2);

        let forbidden = navigation_observation_events(
            id,
            &mut previous,
            Some("file:///etc/passwd".to_owned()),
            Some((true, true)),
        );
        assert_eq!(forbidden.len(), 1);
        assert!(matches!(
            forbidden[0],
            EngineEvent::NavState {
                can_go_back: true,
                can_go_forward: true,
                ..
            }
        ));
        assert_eq!(previous.url.as_deref(), Some("https://example.com/#state"));
    }

    #[test]
    fn native_url_policy_distinguishes_unavailable_from_forbidden_sources() {
        assert_eq!(classify_observed_url(None), ObservedUrl::Unavailable);
        assert_eq!(
            classify_observed_url(Some(String::new())),
            ObservedUrl::Unavailable
        );
        assert_eq!(
            classify_observed_url(Some("https://example.com/#state".to_owned())),
            ObservedUrl::Allowed("https://example.com/#state".to_owned())
        );
        assert_eq!(
            classify_observed_url(Some("file:///etc/passwd".to_owned())),
            ObservedUrl::Forbidden
        );
    }

    #[test]
    fn discard_report_accepts_only_the_exact_safe_primitive_mask() {
        assert!(renderer_report_allows_discard("1"));
        for protected in [
            "0", "2", "3", "255", "256", "511", "null", "\"1\"", "{}", " 1",
        ] {
            assert!(
                !renderer_report_allows_discard(protected),
                "alternate renderer value must veto discard: {protected:?}"
            );
        }
        assert!(DISCARD_SAFETY_BOOTSTRAP_JS.contains("localUncertain ? 256 : 0"));
        assert!(DISCARD_SAFETY_QUERY_JS.contains("return 256"));
        assert!(!DISCARD_SAFETY_QUERY_JS.contains("return {"));
    }

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
    fn raw_page_print_guard_is_installed_for_subframes_before_user_scripts() {
        let source = include_str!("host.rs");
        let all_frames_call = [
            "builder.with_initialization_script_for_main_only(",
            "crate::PAGE_PRINT_DENY_SCRIPT, false)",
        ]
        .concat();
        assert_eq!(source.matches(&all_frames_call).count(), 1);
        let guard = source.find(&all_frames_call).unwrap();
        let user_scripts = source.find("for script in scripts").unwrap();
        assert!(guard < user_scripts);
    }

    #[test]
    fn windows_raw_autofill_surfaces_are_mandatory_verified_postconditions() {
        let source = include_str!("platform/windows/mod.rs");
        let configure = source
            .split("pub fn configure(")
            .nth(1)
            .expect("raw WebView2 configure function")
            .split("// Wry's navigation callback")
            .next()
            .expect("pre-navigation raw WebView2 policy");
        for required in [
            "settings.cast::<ICoreWebView2Settings4>()?",
            "SetIsPasswordAutosaveEnabled(false)?",
            "SetIsGeneralAutofillEnabled(false)?",
            "IsPasswordAutosaveEnabled(&mut password_autosave_enabled)?",
            "IsGeneralAutofillEnabled(&mut general_autofill_enabled)?",
            "password_autosave_enabled.as_bool() || general_autofill_enabled.as_bool()",
            "E_ACCESSDENIED",
        ] {
            assert!(
                configure.contains(required),
                "raw WebView2 autofill postcondition lost invariant: {required}"
            );
        }
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
    fn favicon_callback_accepts_only_the_canonical_primitive_string() {
        let rgba = vec![17_u8; zephium_core::icon::RGBA32_BYTES];
        let chrome = zephium_core::icon::chrome_value(&rgba).unwrap();
        let encoded = chrome
            .strip_prefix(zephium_core::icon::RGBA32_PREFIX)
            .unwrap();
        let serialized = serde_json::to_string(encoded).unwrap();
        assert_eq!(decode_favicon_eval_result(&serialized), Some(rgba));
        assert!(decode_favicon_eval_result("null").is_none());
        assert!(decode_favicon_eval_result(encoded).is_none());
        assert!(decode_favicon_eval_result(&format!("\"{encoded}x\"")).is_none());
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

    #[test]
    fn discard_probe_requires_exact_generation_and_current_navigation_epoch() {
        let first_token = Arc::new(AtomicBool::new(true));
        let first_permit = EventPermit::bound(&first_token);
        let first_navigation = NavigationEpochTracker::new();
        let first_epoch = first_navigation.begin("https://first.example/").unwrap();
        assert_eq!(
            first_navigation.observe_load("https://first.example/", &PageLoadEvent::Started),
            Some(first_epoch)
        );
        assert_eq!(
            first_navigation.observe_load("https://first.example/", &PageLoadEvent::Finished),
            Some(first_epoch)
        );
        assert!(discard_probe_identity_matches(
            &first_permit,
            &first_navigation,
            &first_permit,
            &first_navigation,
            first_epoch,
        ));

        let second_epoch = first_navigation.begin("https://second.example/").unwrap();
        assert_ne!(first_epoch, second_epoch);
        assert!(!discard_probe_identity_matches(
            &first_permit,
            &first_navigation,
            &first_permit,
            &first_navigation,
            first_epoch,
        ));

        let replacement_token = Arc::new(AtomicBool::new(true));
        let replacement_permit = EventPermit::bound(&replacement_token);
        let replacement_navigation = NavigationEpochTracker::new();
        let replacement_epoch = replacement_navigation
            .begin("https://second.example/")
            .unwrap();
        assert!(!discard_probe_identity_matches(
            &replacement_permit,
            &replacement_navigation,
            &first_permit,
            &first_navigation,
            second_epoch,
        ));
        assert!(!discard_probe_identity_matches(
            &first_permit,
            &first_navigation,
            &replacement_permit,
            &replacement_navigation,
            replacement_epoch,
        ));
    }
}
