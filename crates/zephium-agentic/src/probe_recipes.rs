//! Closed script recipes for release-excluded native input probes.

use std::borrow::Cow;

use crate::{FixtureCase, InputBackend};

/// Native handler name compiled into the macOS probe runtime.
///
/// The handler is registered only in [`MACOS_PROBE_CONTENT_WORLD_V1`]; page
/// world JavaScript has no function with this name.
pub const MACOS_PROBE_HANDLER_V1: &str = "zephiumAgenticProbeV1";

/// Dedicated content-world name for the release-excluded macOS probe.
pub const MACOS_PROBE_CONTENT_WORLD_V1: &str = "zephium-agentic-probe-v1";

/// Version of the closed isolated-runtime message envelope.
pub const NATIVE_INPUT_RUNTIME_PROTOCOL_V1: u8 = 1;

/// Maximum correlated row identifier accepted by the fixed runtime.
pub const MAX_NATIVE_INPUT_RUNTIME_ROW: u16 = 128;

/// Immutable macOS probe runtime installed at document end in a custom
/// `WKContentWorld`.
///
/// This program accepts no native command, selector, expression, or reply. It
/// derives one allowlisted row from the fixed fixture URL, emits one bounded
/// ready envelope, optionally executes one compiled DOM recipe, and emits one
/// bounded result envelope. Public `WKWebView` evaluation is deliberately not
/// used because WebKit executes that API with a forced user gesture.
pub const MACOS_NATIVE_INPUT_RUNTIME_V1: &str = r###"(() => {
  'use strict';
  const PROTOCOL = 1;
  const MAX_ROW = 128;
  const HANDLER = 'zephiumAgenticProbeV1';
  const cases = new Set([
    'button', 'link', 'text_input', 'content_editable', 'select',
    'pointer_mouse', 'keyboard', 'transient_activation', 'popup',
    'clipboard_gate', 'drag', 'iframe', 'open_shadow', 'closed_shadow'
  ]);
  const backends = new Set([
    'fixed_dom_recipe', 'macos_app_kit_event', 'macos_accessibility',
    'macos_focused_os_input', 'windows_hwnd_input',
    'windows_composition_input', 'windows_cdp_input', 'human_baseline'
  ]);
  const emit = (message) => {
    try {
      const endpoint = window.webkit && window.webkit.messageHandlers &&
        window.webkit.messageHandlers[HANDLER];
      if (!endpoint || typeof endpoint.postMessage !== 'function') return;
      const encoded = JSON.stringify(message);
      if (encoded.length <= 32768) endpoint.postMessage(encoded);
    } catch (_) {}
  };
  const fail = (row, caseName, backend, code) => emit({
    phase: 'fault', protocol: PROTOCOL, row, case: caseName, backend, code
  });
  if (location.protocol !== 'http:' || location.hostname !== '127.0.0.1' ||
      location.pathname !== '/native-input-v1.html') return;
  const query = new URLSearchParams(location.search);
  if ([...query.keys()].length !== 3) return;
  const rowText = query.get('row');
  const caseName = query.get('case');
  const backend = query.get('backend');
  if (!rowText || !/^[1-9][0-9]{0,2}$/.test(rowText) || !caseName || !backend) return;
  const row = Number(rowText);
  if (!Number.isSafeInteger(row) || row > MAX_ROW || !cases.has(caseName) ||
      !backends.has(backend)) return;

  const elementFor = (name) => {
    switch (name) {
      case 'button': case 'pointer_mouse': return document.getElementById('button');
      case 'link': return document.getElementById('link');
      case 'text_input': case 'keyboard': return document.getElementById('text-input');
      case 'content_editable': return document.getElementById('content-editable');
      case 'select': return document.getElementById('select');
      case 'transient_activation': return document.getElementById('activation-button');
      case 'popup': return document.getElementById('popup-button');
      case 'clipboard_gate': return document.getElementById('clipboard-button');
      case 'open_shadow': return document.getElementById('open-shadow-host')?.shadowRoot?.
        getElementById('open-shadow-button') || null;
      case 'closed_shadow': return document.getElementById('closed-shadow-host');
      default: return null;
    }
  };
  const rectValue = (element, offsetX = 0, offsetY = 0) => {
    if (!element) return null;
    const rect = element.getBoundingClientRect();
    const values = [rect.x, rect.y, rect.width, rect.height, window.devicePixelRatio];
    if (!values.every(Number.isFinite) || rect.width <= 0 || rect.height <= 0) return null;
    return {x: offsetX + rect.x, y: offsetY + rect.y, width: rect.width,
      height: rect.height, endX: null, endY: null,
      devicePixelRatio: window.devicePixelRatio};
  };
  const geometryFor = (name) => {
    if (name === 'drag') {
      const source = document.getElementById('drag-source');
      const destination = document.getElementById('drop-target');
      const result = rectValue(source);
      if (!result || !destination) return null;
      const end = destination.getBoundingClientRect();
      result.endX = end.x + end.width / 2;
      result.endY = end.y + end.height / 2;
      return result;
    }
    if (name === 'iframe') {
      const frame = document.getElementById('frame');
      const button = frame?.contentDocument?.getElementById('frame-button');
      if (!frame || !button) return null;
      const frameRect = frame.getBoundingClientRect();
      return rectValue(button, frameRect.x, frameRect.y);
    }
    return rectValue(elementFor(name));
  };
  const domRecipe = (name) => {
    try {
      if (name === 'closed_shadow') return 'unsupported';
      if (name === 'drag') {
        const source = document.getElementById('drag-source');
        const destination = document.getElementById('drop-target');
        if (!source || !destination) return 'failed';
        const transfer = new DataTransfer();
        source.dispatchEvent(new DragEvent('dragstart', {bubbles: true, dataTransfer: transfer}));
        destination.dispatchEvent(new DragEvent('dragenter', {bubbles: true, dataTransfer: transfer}));
        destination.dispatchEvent(new DragEvent('dragover', {bubbles: true,
          cancelable: true, dataTransfer: transfer}));
        destination.dispatchEvent(new DragEvent('drop', {bubbles: true,
          cancelable: true, dataTransfer: transfer}));
        source.dispatchEvent(new DragEvent('dragend', {bubbles: true, dataTransfer: transfer}));
        return 'ok';
      }
      if (name === 'iframe') {
        const button = document.getElementById('frame')?.contentDocument?.
          getElementById('frame-button');
        if (!button) return 'failed';
        button.click();
        return 'ok';
      }
      const element = elementFor(name);
      if (!element) return 'failed';
      if (name === 'text_input') {
        element.focus(); element.value = 'fixturex';
        element.dispatchEvent(new InputEvent('input', {bubbles: true,
          inputType: 'insertText', data: 'x'}));
      } else if (name === 'content_editable') {
        element.focus(); element.textContent = 'fixturex';
        element.dispatchEvent(new InputEvent('input', {bubbles: true,
          inputType: 'insertText', data: 'x'}));
      } else if (name === 'select') {
        element.focus(); element.selectedIndex = 1;
        element.dispatchEvent(new Event('change', {bubbles: true}));
      } else if (name === 'keyboard') {
        element.focus();
        element.dispatchEvent(new KeyboardEvent('keydown', {key: 'x', code: 'KeyX',
          bubbles: true}));
        element.dispatchEvent(new KeyboardEvent('keyup', {key: 'x', code: 'KeyX',
          bubbles: true}));
      } else {
        element.click();
      }
      return 'ok';
    } catch (_) {
      return 'failed';
    }
  };
  const start = () => {
    const root = document.documentElement;
    if (!root || root.dataset.fixtureReady !== 'v1' ||
        root.dataset.probeRow !== rowText || root.dataset.probeCase !== caseName ||
        root.dataset.probeBackend !== backend) {
      fail(row, caseName, backend, 'fixture_not_ready');
      return;
    }
    const geometry = geometryFor(caseName);
    if (!geometry) {
      fail(row, caseName, backend, 'missing_target');
      return;
    }
    emit({phase: 'ready', protocol: PROTOCOL, row, case: caseName, backend, geometry});
    if (backend === 'fixed_dom_recipe') setTimeout(() => domRecipe(caseName), 0);
    setTimeout(() => {
      root.dataset.probeReadRequest = rowText;
      const deadline = Date.now() + 500;
      const read = () => {
        if (root.dataset.probeEvidenceRow === rowText) {
          try {
            const state = JSON.parse(root.dataset.probeEvidence || '');
            emit({phase: 'result', protocol: PROTOCOL, row, case: caseName,
              backend, state});
          } catch (_) {
            fail(row, caseName, backend, 'invalid_evidence');
          }
          return;
        }
        if (Date.now() >= deadline) {
          fail(row, caseName, backend, 'missing_evidence');
          return;
        }
        setTimeout(read, 5);
      };
      queueMicrotask(read);
    }, 200);
  };
  if (document.readyState === 'complete') start();
  else window.addEventListener('load', start, {once: true});
})()"###;

/// The only JavaScript execution world one fixed probe recipe may request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeScriptWorld {
    /// The trusted fixed fixture's own world, used only for its control API.
    FixturePage,
    /// The engine's default client/host world, where the platform supports it.
    DefaultClient,
}

/// Closed fixed-script vocabulary shared by native probe adapters.
///
/// No string, selector, expression, or page value can enter this enum. It is
/// compiled only with the release-forbidden `probe-harness` feature.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixedProbeScript {
    /// Check that the fixed fixture has completed initialization.
    Ready,
    /// Reset the fixed fixture to one allowlisted case.
    Reset(FixtureCase),
    /// Read the fixture's bounded JSON evidence.
    Read,
    /// Read bounded geometry for one allowlisted case.
    Geometry(FixtureCase),
    /// Execute one fixed semantic recipe for an allowlisted case.
    DomRecipe(FixtureCase),
}

impl FixedProbeScript {
    /// Returns the fixed source associated with this closed recipe.
    ///
    /// `None` is an intentional unsupported classification, currently the
    /// closed-shadow semantic recipe. Callers must not substitute page-world
    /// introspection when a recipe is absent.
    pub fn source(self) -> Option<Cow<'static, str>> {
        match self {
            Self::Ready => Some(Cow::Borrowed(
                "typeof window.__zephiumNativeInputFixtureV1 === 'object' && document.documentElement.dataset.fixtureReady === 'v1' ? 'ready' : 'pending'",
            )),
            Self::Reset(case) => Some(Cow::Owned(format!(
                "window.__zephiumNativeInputFixtureV1.reset('{}') ? 'ok' : 'failed'",
                case_name(case)
            ))),
            Self::Read => Some(Cow::Borrowed(
                "window.__zephiumNativeInputFixtureV1.readJson()",
            )),
            Self::Geometry(case) => geometry_script(case).map(Cow::Owned),
            Self::DomRecipe(case) => dom_recipe(case).map(Cow::Borrowed),
        }
    }

    /// Returns the narrow execution-world class for this recipe.
    pub const fn world(self) -> ProbeScriptWorld {
        match self {
            Self::Ready | Self::Reset(_) | Self::Read => ProbeScriptWorld::FixturePage,
            Self::Geometry(_) | Self::DomRecipe(_) => ProbeScriptWorld::DefaultClient,
        }
    }
}

pub(crate) const fn case_name(case: FixtureCase) -> &'static str {
    match case {
        FixtureCase::Button => "button",
        FixtureCase::Link => "link",
        FixtureCase::TextInput => "text_input",
        FixtureCase::ContentEditable => "content_editable",
        FixtureCase::Select => "select",
        FixtureCase::PointerMouse => "pointer_mouse",
        FixtureCase::Keyboard => "keyboard",
        FixtureCase::TransientActivation => "transient_activation",
        FixtureCase::Popup => "popup",
        FixtureCase::ClipboardGate => "clipboard_gate",
        FixtureCase::Drag => "drag",
        FixtureCase::Iframe => "iframe",
        FixtureCase::OpenShadow => "open_shadow",
        FixtureCase::ClosedShadow => "closed_shadow",
    }
}

pub(crate) const fn backend_name(backend: InputBackend) -> &'static str {
    match backend {
        InputBackend::FixedDomRecipe => "fixed_dom_recipe",
        InputBackend::MacosAppKitEvent => "macos_app_kit_event",
        InputBackend::MacosAccessibility => "macos_accessibility",
        InputBackend::MacosFocusedOsInput => "macos_focused_os_input",
        InputBackend::WindowsHwndInput => "windows_hwnd_input",
        InputBackend::WindowsCompositionInput => "windows_composition_input",
        InputBackend::WindowsCdpInput => "windows_cdp_input",
        InputBackend::HumanBaseline => "human_baseline",
    }
}

fn dom_recipe(case: FixtureCase) -> Option<&'static str> {
    Some(match case {
        FixtureCase::Button | FixtureCase::PointerMouse => {
            "(()=>{const e=document.getElementById('button');if(!e)return 'failed';e.click();return 'ok';})()"
        }
        FixtureCase::Link => {
            "(()=>{const e=document.getElementById('link');if(!e)return 'failed';e.click();return 'ok';})()"
        }
        FixtureCase::TextInput => {
            "(()=>{const e=document.getElementById('text-input');if(!e)return 'failed';e.focus();e.value='fixturex';e.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:'x'}));return 'ok';})()"
        }
        FixtureCase::ContentEditable => {
            "(()=>{const e=document.getElementById('content-editable');if(!e)return 'failed';e.focus();e.textContent='fixturex';e.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:'x'}));return 'ok';})()"
        }
        FixtureCase::Select => {
            "(()=>{const e=document.getElementById('select');if(!e)return 'failed';e.focus();e.selectedIndex=1;e.dispatchEvent(new Event('change',{bubbles:true}));return 'ok';})()"
        }
        FixtureCase::Keyboard => {
            "(()=>{const e=document.getElementById('text-input');if(!e)return 'failed';e.focus();e.dispatchEvent(new KeyboardEvent('keydown',{key:'x',code:'KeyX',bubbles:true}));e.dispatchEvent(new KeyboardEvent('keyup',{key:'x',code:'KeyX',bubbles:true}));return 'ok';})()"
        }
        FixtureCase::TransientActivation => {
            "(()=>{const e=document.getElementById('activation-button');if(!e)return 'failed';e.click();return 'ok';})()"
        }
        FixtureCase::Popup => {
            "(()=>{const e=document.getElementById('popup-button');if(!e)return 'failed';e.click();return 'ok';})()"
        }
        FixtureCase::ClipboardGate => {
            "(()=>{const e=document.getElementById('clipboard-button');if(!e)return 'failed';e.click();return 'ok';})()"
        }
        FixtureCase::Drag => {
            "(()=>{try{const s=document.getElementById('drag-source');const d=document.getElementById('drop-target');if(!s||!d)return 'failed';const t=new DataTransfer();s.dispatchEvent(new DragEvent('dragstart',{bubbles:true,dataTransfer:t}));d.dispatchEvent(new DragEvent('dragenter',{bubbles:true,dataTransfer:t}));d.dispatchEvent(new DragEvent('dragover',{bubbles:true,cancelable:true,dataTransfer:t}));d.dispatchEvent(new DragEvent('drop',{bubbles:true,cancelable:true,dataTransfer:t}));s.dispatchEvent(new DragEvent('dragend',{bubbles:true,dataTransfer:t}));return 'ok';}catch{return 'failed';}})()"
        }
        FixtureCase::Iframe => {
            "(()=>{const e=document.getElementById('frame')?.contentDocument?.getElementById('frame-button');if(!e)return 'failed';e.click();return 'ok';})()"
        }
        FixtureCase::OpenShadow => {
            "(()=>{const e=document.getElementById('open-shadow-host')?.shadowRoot?.getElementById('open-shadow-button');if(!e)return 'failed';e.click();return 'ok';})()"
        }
        FixtureCase::ClosedShadow => return None,
    })
}

fn geometry_script(case: FixtureCase) -> Option<String> {
    if case == FixtureCase::Drag {
        return Some(
            "(()=>{const s=document.getElementById('drag-source');const d=document.getElementById('drop-target');if(!s||!d)return '';const r=s.getBoundingClientRect();const q=d.getBoundingClientRect();return JSON.stringify({x:r.x,y:r.y,width:r.width,height:r.height,endX:q.x+q.width/2,endY:q.y+q.height/2,devicePixelRatio:window.devicePixelRatio});})()".to_owned(),
        );
    }
    if case == FixtureCase::Iframe {
        return Some(
            "(()=>{const f=document.getElementById('frame');const e=f?.contentDocument?.getElementById('frame-button');if(!f||!e)return '';const a=f.getBoundingClientRect();const r=e.getBoundingClientRect();return JSON.stringify({x:a.x+r.x,y:a.y+r.y,width:r.width,height:r.height,endX:null,endY:null,devicePixelRatio:window.devicePixelRatio});})()".to_owned(),
        );
    }
    let expression = match case {
        FixtureCase::Button | FixtureCase::PointerMouse => "document.getElementById('button')",
        FixtureCase::Link => "document.getElementById('link')",
        FixtureCase::TextInput | FixtureCase::Keyboard => {
            "document.getElementById('text-input')"
        }
        FixtureCase::ContentEditable => "document.getElementById('content-editable')",
        FixtureCase::Select => "document.getElementById('select')",
        FixtureCase::TransientActivation => "document.getElementById('activation-button')",
        FixtureCase::Popup => "document.getElementById('popup-button')",
        FixtureCase::ClipboardGate => "document.getElementById('clipboard-button')",
        FixtureCase::OpenShadow => {
            "document.getElementById('open-shadow-host')?.shadowRoot?.getElementById('open-shadow-button')"
        }
        FixtureCase::ClosedShadow => "document.getElementById('closed-shadow-host')",
        FixtureCase::Drag | FixtureCase::Iframe => return None,
    };
    Some(format!(
        "(()=>{{const e={expression};if(!e)return '';const r=e.getBoundingClientRect();return JSON.stringify({{x:r.x,y:r.y,width:r.width,height:r.height,endX:null,endY:null,devicePixelRatio:window.devicePixelRatio}});}})()"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CASES: [FixtureCase; 14] = [
        FixtureCase::Button,
        FixtureCase::Link,
        FixtureCase::TextInput,
        FixtureCase::ContentEditable,
        FixtureCase::Select,
        FixtureCase::PointerMouse,
        FixtureCase::Keyboard,
        FixtureCase::TransientActivation,
        FixtureCase::Popup,
        FixtureCase::ClipboardGate,
        FixtureCase::Drag,
        FixtureCase::Iframe,
        FixtureCase::OpenShadow,
        FixtureCase::ClosedShadow,
    ];

    #[test]
    fn every_case_has_reset_and_geometry_but_closed_shadow_has_no_dom_bypass() {
        for case in CASES {
            assert!(FixedProbeScript::Reset(case).source().is_some());
            assert!(FixedProbeScript::Geometry(case).source().is_some());
        }
        assert!(FixedProbeScript::DomRecipe(FixtureCase::ClosedShadow)
            .source()
            .is_none());
    }

    #[test]
    fn fixture_control_and_client_recipes_have_fixed_worlds() {
        assert_eq!(
            FixedProbeScript::Read.world(),
            ProbeScriptWorld::FixturePage
        );
        assert_eq!(
            FixedProbeScript::Geometry(FixtureCase::Button).world(),
            ProbeScriptWorld::DefaultClient
        );
    }

    #[test]
    fn isolated_runtime_has_no_dynamic_evaluation_or_native_reply_surface() {
        assert!(MACOS_NATIVE_INPUT_RUNTIME_V1.contains("phase: 'ready'"));
        assert!(MACOS_NATIVE_INPUT_RUNTIME_V1.contains("phase: 'result'"));
        assert!(MACOS_NATIVE_INPUT_RUNTIME_V1.contains("phase: 'fault'"));
        assert!(!MACOS_NATIVE_INPUT_RUNTIME_V1.contains("eval("));
        assert!(!MACOS_NATIVE_INPUT_RUNTIME_V1.contains("new Function"));
        assert!(!MACOS_NATIVE_INPUT_RUNTIME_V1.contains("await endpoint.postMessage"));
    }
}
