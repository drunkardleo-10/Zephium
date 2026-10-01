//! Read-only, bounded inventory of the owned page's public accessibility tree.
#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]

use objc2::{msg_send, rc::Retained, runtime::AnyObject, sel};
use objc2_app_kit::NSObjectNSAccessibility as _;
use objc2_foundation::{MainThreadMarker, NSArray, NSNumber, NSPoint, NSString, NSURL};
use objc2_web_kit::WKWebView;
use std::{
    collections::HashSet,
    ffi::c_void,
    time::{Duration, Instant},
};

const MAX_NODES: usize = 256;
const MAX_DEPTH: u8 = 16;
const READ_WINDOW: Duration = Duration::from_millis(500);
pub(super) const WINDOW_ID: &str = "zephium-liveness-probe-window-v1";

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> *mut AnyObject;
    fn AXUIElementGetTypeID() -> usize;
    fn AXUIElementSetMessagingTimeout(element: *const c_void, timeout: f32) -> i32;
    fn AXUIElementCopyAttributeValue(
        element: *const c_void,
        attribute: *const c_void,
        value: *mut *mut AnyObject,
    ) -> i32;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFGetTypeID(value: *const c_void) -> usize;
}

#[derive(Clone, Copy, Debug)]
pub(super) enum BridgeStatus {
    NotCfProxy,
    Available,
    Disabled,
    Unsupported,
    Unavailable,
}

fn cf_attribute(
    element: &AnyObject,
    name: &'static str,
) -> Result<Option<Retained<AnyObject>>, BridgeStatus> {
    if element.class().name().to_bytes() != b"__NSCFType" {
        return Err(BridgeStatus::NotCfProxy);
    }
    let pointer = std::ptr::from_ref(element).cast::<c_void>();
    // SAFETY: the object is WebKit's retained CF proxy; validate its public type before AX calls.
    unsafe {
        if CFGetTypeID(pointer) != AXUIElementGetTypeID() {
            return Err(BridgeStatus::NotCfProxy);
        }
        if AXUIElementSetMessagingTimeout(pointer, 0.05) != 0 {
            return Err(BridgeStatus::Unavailable);
        }
        let key = NSString::from_str(name);
        let mut value = std::ptr::null_mut();
        let status =
            AXUIElementCopyAttributeValue(pointer, Retained::as_ptr(&key).cast(), &mut value);
        match status {
            // The allowlisted attributes return toll-free bridged strings, arrays,
            // numbers, URLs or AX proxies under the Copy ownership rule.
            0 => Ok(Retained::from_raw(value)),
            -25211 => Err(BridgeStatus::Disabled),
            -25205 | -25212 => Err(BridgeStatus::Unsupported),
            _ => Err(BridgeStatus::Unavailable),
        }
    }
}

#[derive(Default)]
pub(super) struct Facts {
    pub nodes: usize,
    pub web_areas: usize,
    pub challenge_areas: usize,
    pub checkboxes: usize,
    pub scoped_checkboxes: usize,
    pub named_checkboxes: usize,
    pub enabled_checkboxes: usize,
    pub candidates: usize,
    pub unresolved_remote: usize,
    pub windows: usize,
    pub root_getters: u16,
    pub bridge: Option<BridgeStatus>,
    pub truncated: bool,
}

fn attribute(element: &AnyObject, name: &'static str) -> Option<Retained<AnyObject>> {
    if let Ok(value) = cf_attribute(element, name) {
        return value;
    }
    // SAFETY: retained AppKit-returned element; only the public compatibility getter is used.
    let supports: bool =
        unsafe { msg_send![element, respondsToSelector: sel!(accessibilityAttributeValue:)] };
    if supports {
        let key = NSString::from_str(name);
        // SAFETY: getter availability was checked on this exact retained object.
        let value: Option<Retained<AnyObject>> =
            unsafe { msg_send![element, accessibilityAttributeValue: &*key] };
        if value.is_some() {
            return value;
        }
    }
    let selector = match name {
        "AXRole" => sel!(accessibilityRole),
        "AXChildren" => sel!(accessibilityChildren),
        "AXParent" => sel!(accessibilityParent),
        "AXURL" => sel!(accessibilityURL),
        "AXTitle" => sel!(accessibilityTitle),
        "AXDescription" => sel!(accessibilityLabel),
        "AXIdentifier" => sel!(accessibilityIdentifier),
        _ => return None,
    };
    // SAFETY: only allowlisted public object-valued accessibility getters are invoked.
    unsafe {
        let supports: bool = msg_send![element, respondsToSelector: selector];
        if !supports {
            return None;
        }
        // performSelector is restricted above to zero-argument object-valued getters.
        msg_send![element, performSelector: selector]
    }
}

fn string(element: &AnyObject, name: &'static str) -> Option<String> {
    attribute(element, name)
        .and_then(|value| value.downcast::<NSString>().ok())
        .filter(|value| value.length() <= 128)
        .map(|value| value.to_string())
}

fn challenge_area(element: &AnyObject) -> bool {
    attribute(element, "AXURL")
        .and_then(|value| value.downcast::<NSURL>().ok())
        .and_then(|value| value.absoluteString())
        .filter(|value| value.length() <= 8192)
        .is_some_and(|value| is_challenge_origin(&value.to_string()))
}

fn is_challenge_origin(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|value| {
        value.scheme() == "https"
            && value.host_str() == Some("challenges.cloudflare.com")
            && value.port_or_known_default() == Some(443)
            && value.username().is_empty()
            && value.password().is_none()
    })
}

pub(super) fn inspect(page: &WKWebView) -> Facts {
    let started = Instant::now();
    let mut facts = Facts::default();
    let result = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
        let Some(window) = page.window() else {
            return;
        };
        let bounds = page.bounds();
        let point = window.convertPointToScreen(page.convertPoint_toView(
            NSPoint::new(
                bounds.origin.x + bounds.size.width / 2.0,
                bounds.origin.y + bounds.size.height / 2.0,
            ),
            None,
        ));
        let Some(mut root) = page.accessibilityHitTest(point) else {
            return;
        };
        for (index, selector) in [
            sel!(accessibilityAttributeValue:),
            sel!(accessibilityRole),
            sel!(accessibilityChildren),
            sel!(accessibilityHitTest:),
        ]
        .into_iter()
        .enumerate()
        {
            // SAFETY: selector introspection on the retained owned-page AX root.
            let supports: bool = unsafe { msg_send![&*root, respondsToSelector: selector] };
            if supports {
                facts.root_getters |= 1 << index;
            }
        }
        if facts.root_getters & 8 != 0 && string(&root, "AXRole").is_none() {
            // SAFETY: checked public hit test on the exact owned-page remote root.
            let leaf: Option<Retained<AnyObject>> =
                unsafe { msg_send![&*root, accessibilityHitTest: point] };
            if let Some(leaf) = leaf {
                root = leaf;
            }
            facts.bridge = Some(match cf_attribute(&root, "AXRole") {
                Ok(Some(_)) => BridgeStatus::Available,
                Ok(None) => BridgeStatus::Unsupported,
                Err(status) => status,
            });
            for _ in 0..MAX_DEPTH {
                if string(&root, "AXRole").as_deref() == Some("AXWebArea") {
                    break;
                }
                let Some(parent) = attribute(&root, "AXParent") else {
                    break;
                };
                root = parent;
            }
        }
        collect(root, started, &mut facts);
    }));
    facts.truncated |= result.is_err();
    facts
}

pub(super) fn inspect_own_process(_mtm: MainThreadMarker) -> Facts {
    objc2::rc::autoreleasepool(|_| {
        let started = Instant::now();
        let mut facts = Facts::default();
        let Ok(pid) = i32::try_from(std::process::id()) else {
            return facts;
        };
        // SAFETY: public Create API for this diagnostic process only; the CF AX
        // object is returned retained and toll-free bridged by the framework.
        let Some(root) = (unsafe { Retained::from_raw(AXUIElementCreateApplication(pid)) }) else {
            return facts;
        };
        let result = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
            let windows = match cf_attribute(&root, "AXWindows") {
                Ok(Some(value)) => value.downcast::<NSArray<AnyObject>>().ok(),
                Ok(None) => {
                    facts.bridge = Some(BridgeStatus::Unsupported);
                    return;
                }
                Err(status) => {
                    facts.bridge = Some(status);
                    return;
                }
            };
            let Some(windows) = windows.filter(|windows| windows.len() <= 16) else {
                facts.truncated = true;
                return;
            };
            facts.windows = windows.len();
            let mut matched = windows.iter().filter(|window| {
                string(window, "AXRole").as_deref() == Some("AXWindow")
                    && string(window, "AXIdentifier").as_deref() == Some(WINDOW_ID)
            });
            let Some(window) = matched.next() else {
                facts.truncated = true;
                return;
            };
            if matched.next().is_some() {
                facts.truncated = true;
                return;
            }
            facts.bridge = Some(BridgeStatus::Available);
            collect(window, started, &mut facts);
        }));
        facts.truncated |= result.is_err();
        facts
    })
}

fn collect(root: Retained<AnyObject>, started: Instant, facts: &mut Facts) {
    let mut pending = vec![(root, false, 0_u8)];
    let mut visited = HashSet::new();
    let mut retained = Vec::new();
    while let Some((element, inherited_challenge, depth)) = pending.pop() {
        if facts.nodes >= MAX_NODES || started.elapsed() >= READ_WINDOW {
            facts.truncated = true;
            break;
        }
        if !visited.insert(Retained::as_ptr(&element).addr()) {
            continue;
        }
        retained.push(element.clone());
        facts.nodes += 1;
        let role = string(&element, "AXRole");
        facts.unresolved_remote += usize::from(
            role.is_none()
                && element.class().name().to_bytes() == b"NSAccessibilityRemoteUIElement",
        );
        let in_challenge = if role.as_deref() == Some("AXWebArea") {
            facts.web_areas += 1;
            let matched = challenge_area(&element);
            facts.challenge_areas += usize::from(matched);
            matched
        } else {
            inherited_challenge
        };
        if role.as_deref() == Some("AXCheckBox") {
            facts.checkboxes += 1;
            facts.scoped_checkboxes += usize::from(in_challenge);
            let named = ["AXTitle", "AXDescription"].into_iter().any(|key| {
                string(&element, key).is_some_and(|value| value == "Verify you are human")
            });
            facts.named_checkboxes += usize::from(named);
            let enabled = attribute(&element, "AXEnabled")
                .and_then(|value| value.downcast::<NSNumber>().ok())
                .is_some_and(|value| value.boolValue());
            facts.enabled_checkboxes += usize::from(enabled);
            facts.candidates += usize::from(in_challenge && named && enabled);
        }
        if matches!(
            role.as_deref(),
            Some("AXTextField" | "AXTextArea" | "AXSecureTextField")
        ) {
            continue;
        }
        if let Some(children) = attribute(&element, "AXChildren")
            .and_then(|value| value.downcast::<NSArray<AnyObject>>().ok())
        {
            if depth >= MAX_DEPTH && !children.is_empty() {
                facts.truncated = true;
                continue;
            }
            let available = MAX_NODES.saturating_sub(facts.nodes + pending.len());
            facts.truncated |= children.len() > available;
            pending.extend(
                children
                    .iter()
                    .take(available)
                    .map(|child| (child, in_challenge, depth + 1)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_challenge_origin;

    #[test]
    fn challenge_frame_origin_does_not_accept_lookalikes_or_embedded_urls() {
        assert!(is_challenge_origin(
            "https://challenges.cloudflare.com/widget"
        ));
        assert!(is_challenge_origin(
            "https://challenges.cloudflare.com:443/widget"
        ));
        for value in [
            "http://challenges.cloudflare.com/widget",
            "https://challenges.cloudflare.com:444/widget",
            "https://challenges.cloudflare.com.example/widget",
            "https://example.test/?frame=https://challenges.cloudflare.com/",
            "https://challenges.cloudflare.com@example.test/widget",
            "https://user@challenges.cloudflare.com/widget",
            "about:blank",
        ] {
            assert!(!is_challenge_origin(value));
        }
    }
}
