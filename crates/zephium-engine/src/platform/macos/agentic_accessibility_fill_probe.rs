//! Release-excluded in-process AX value capability experiment. Never an action backend.

use std::panic::AssertUnwindSafe;

use objc2::{msg_send, rc::Retained, runtime::AnyObject, sel, Encode};
use objc2_app_kit::NSObjectNSAccessibility as _;
use objc2_foundation::{NSNumber, NSPoint, NSRect, NSString, NSValue};
use objc2_web_kit::WKWebView;

use super::*;

pub(super) const ENV: &str = "ZEPHIUM_LOCAL_AX_FILL_PROBE";

struct Target {
    element: Retained<AnyObject>,
    point: NSPoint,
    frame: NSRect,
    legacy: bool,
}

fn inspect_legacy(
    element: Retained<AnyObject>,
    point: NSPoint,
    frame: NSRect,
    name: &str,
    expected: &str,
) -> Result<Target, &'static str> {
    for selector in [
        sel!(accessibilityAttributeValue:),
        sel!(accessibilityIsAttributeSettable:),
        sel!(accessibilitySetValue:forAttribute:),
    ] {
        // SAFETY: public compatibility selectors on a retained owned AX object.
        let supported: bool = unsafe { msg_send![&*element, respondsToSelector: selector] };
        if !supported {
            return Err("ax_public_legacy_unavailable");
        }
    }
    let attribute = |key: &str| -> Option<Retained<AnyObject>> {
        let key = NSString::from_str(key);
        // SAFETY: public getter checked above; returned Objective-C object is retained.
        unsafe { msg_send![&*element, accessibilityAttributeValue: &*key] }
    };
    let string = |key| {
        attribute(key)
            .and_then(|value| value.downcast::<NSString>().ok())
            .map(|value| value.to_string())
    };
    let role = string("AXRole");
    let role_match = role.is_some_and(|role| matches!(role.as_str(), "AXTextArea" | "AXTextField"));
    let secure = string("AXSubrole").is_some_and(|role| role == "AXSecureTextField");
    let label_match = string("AXDescription").is_some_and(|value| value == name)
        || string("AXTitle").is_some_and(|value| value == name);
    let value_match = string("AXValue").is_some_and(|value| value == expected);
    let enabled = attribute("AXEnabled")
        .and_then(|value| value.downcast::<NSNumber>().ok())
        .is_some_and(|value| value.boolValue());
    let protected = attribute("AXContainsProtectedContent")
        .and_then(|value| value.downcast::<NSNumber>().ok())
        .map(|value| value.boolValue());
    let position = attribute("AXPosition")
        .and_then(|value| value.downcast::<NSValue>().ok())
        .and_then(|value| {
            // SAFETY: NSValue owns the terminated type encoding; pointValue is
            // called only after exact public NSPoint encoding verification.
            unsafe {
                (std::ffi::CStr::from_ptr(value.objCType().as_ptr()).to_bytes()
                    == NSPoint::ENCODING.to_string().as_bytes())
                .then(|| value.pointValue())
            }
        });
    let size = attribute("AXSize")
        .and_then(|value| value.downcast::<NSValue>().ok())
        .and_then(|value| {
            // SAFETY: same exact encoding gate, here for the NSSize payload.
            unsafe {
                (std::ffi::CStr::from_ptr(value.objCType().as_ptr()).to_bytes()
                    == NSSize::ENCODING.to_string().as_bytes())
                .then(|| value.sizeValue())
            }
        });
    let geometry_match = position.zip(size).is_some_and(|(position, size)| {
        [
            position.x - frame.origin.x,
            position.y - frame.origin.y,
            size.width - frame.size.width,
            size.height - frame.size.height,
        ]
        .into_iter()
        .all(|delta| delta.is_finite() && delta.abs() <= 1.0)
    });
    let value_key = NSString::from_str("AXValue");
    // SAFETY: public attribute writability query was checked on this exact object.
    let settable: bool =
        unsafe { msg_send![&*element, accessibilityIsAttributeSettable: &*value_key] };
    eprintln!("ax-fill-legacy-binding: role={role_match} value={value_match} label={label_match} geometry={geometry_match} enabled={enabled} secure={secure} protected={protected:?} settable={settable} content=redacted");
    if !role_match
        || !value_match
        || !label_match
        || !geometry_match
        || !enabled
        || secure
        || protected != Some(false)
        || !settable
    {
        return Err("ax_legacy_exact_target_unproven");
    }
    Ok(Target {
        element,
        point,
        frame,
        legacy: true,
    })
}

fn hit_leaf(page: &WKWebView, point: NSPoint) -> Result<Retained<AnyObject>, &'static str> {
    // WKWebView's public hit test enables accessibility asynchronously and
    // returns its remote web-area child, not the DOM leaf at this coordinate.
    let root = page
        .accessibilityHitTest(point)
        .ok_or("ax_remote_root_missing")?;
    // SAFETY: public NSObject protocol selector on the retained owned AX root.
    let supports: bool =
        unsafe { msg_send![&*root, respondsToSelector: sel!(accessibilityHitTest:)] };
    if !supports {
        return Err("ax_remote_hit_unavailable");
    }
    // SAFETY: public hit test checked above; no global AX identity is acquired.
    let leaf: Option<Retained<AnyObject>> =
        unsafe { msg_send![&*root, accessibilityHitTest: point] };
    leaf.ok_or("ax_leaf_missing")
}

fn check(
    runtime: &ProbeRuntime<'_, '_>,
    deadline: Instant,
    cancelled: bool,
) -> Result<(), &'static str> {
    if cancelled {
        return Err("ax_cancelled_before_dispatch");
    }
    if runtime.failed() {
        return Err("ax_native_lifecycle");
    }
    if Instant::now() >= deadline {
        return Err("ax_deadline_before_dispatch");
    }
    Ok(())
}

fn inspect(
    page: &WKWebView,
    window: &NSWindow,
    node: &SemanticNode,
) -> Result<Target, &'static str> {
    if node.role() != SemanticRole::Textbox
        || node.sensitivity() != SemanticSensitivity::Public
        || !node.operations().contains(SemanticOperationClass::Fill)
    {
        return Err("ax_semantic_target_refused");
    }
    let rect = node.geometry().ok_or("ax_semantic_geometry_missing")?;
    if rect.width() == 0 || rect.height() == 0 || rect.x() < 0 || rect.y() < 0 {
        return Err("ax_semantic_geometry_refused");
    }
    let expected = match node.value() {
        Some(SemanticValueSummary::Text(value)) if !value.preview().truncated() => {
            value.preview().text()
        }
        _ => return Err("ax_semantic_value_refused"),
    };
    let name = node.name().ok_or("ax_semantic_name_missing")?.as_str();
    let bounds = page.bounds();
    let x = f64::from(rect.x());
    let y = f64::from(rect.y());
    let width = f64::from(rect.width());
    let height = f64::from(rect.height());
    if x + width > bounds.size.width || y + height > bounds.size.height {
        return Err("ax_offscreen_target");
    }
    let local_y = if page.isFlipped() {
        y
    } else {
        bounds.size.height - y - height
    };
    let local = NSRect::new(NSPoint::new(x, local_y), NSSize::new(width, height));
    let frame = window.convertRectToScreen(page.convertRect_toView(local, None));
    let point = NSPoint::new(
        frame.origin.x + frame.size.width / 2.0,
        frame.origin.y + frame.size.height / 2.0,
    );
    objc2::exception::catch(AssertUnwindSafe(|| {
        let element = hit_leaf(page, point)?;
        let class = element.class().name().to_bytes();
        eprintln!("ax-fill-object-boundary: cf_proxy={} remote_proxy={} content=redacted",
            class == b"__NSCFType", class == b"NSAccessibilityRemoteUIElement");
        let mut supported = 0_u16;
        for (index, selector) in [sel!(accessibilityRole), sel!(accessibilityValue), sel!(accessibilityLabel),
            sel!(accessibilityFrame), sel!(isAccessibilityEnabled), sel!(isAccessibilityProtectedContent),
            sel!(isAccessibilitySelectorAllowed:), sel!(setAccessibilityValue:)].into_iter().enumerate() {
            // SAFETY: retained AppKit-returned object, public selector introspection only.
            let responds: bool = unsafe { msg_send![&*element, respondsToSelector: selector] };
            if responds { supported |= 1 << index; }
        }
        eprintln!("ax-fill-public-methods: supported={supported:08b} content=redacted");
        if supported != 255 { return inspect_legacy(element, point, frame, name, expected); }
        // SAFETY: every public method was checked above on this retained object.
        let (role, value, label, actual, enabled, protected, settable): (
            Option<Retained<NSString>>, Option<Retained<AnyObject>>, Option<Retained<NSString>>,
            NSRect, bool, bool, bool,
        ) = unsafe { (
            msg_send![&*element, accessibilityRole], msg_send![&*element, accessibilityValue],
            msg_send![&*element, accessibilityLabel], msg_send![&*element, accessibilityFrame],
            msg_send![&*element, isAccessibilityEnabled], msg_send![&*element, isAccessibilityProtectedContent],
            msg_send![&*element, isAccessibilitySelectorAllowed: sel!(setAccessibilityValue:)],
        ) };
        let role_match = role.is_some_and(|role| matches!(role.to_string().as_str(), "AXTextArea" | "AXTextField"));
        let value_match = value.and_then(|value| value.downcast::<NSString>().ok()).is_some_and(|value| value.to_string() == expected);
        let label_match = label.is_some_and(|label| label.to_string() == name);
        let geometry_match = [actual.origin.x - frame.origin.x, actual.origin.y - frame.origin.y,
            actual.size.width - frame.size.width, actual.size.height - frame.size.height]
            .into_iter().all(|delta| delta.is_finite() && delta.abs() <= 1.0);
        eprintln!("ax-fill-binding: role={role_match} value={value_match} label={label_match} geometry={geometry_match} enabled={enabled} protected={protected} settable={settable} content=redacted");
        if !role_match || !value_match || !label_match || !geometry_match || !enabled || protected || !settable {
            return Err("ax_exact_target_unproven");
        }
        Ok(Target { element, point, frame, legacy: false })
    })).map_err(|_| "ax_inspection_exception")?
}

pub(super) fn run(
    view: &AgentOwnedView,
    window: &NSWindow,
    initial: CapturedSnapshot,
    url: &str,
    runtime: &ProbeRuntime<'_, '_>,
    next_invocation: &mut u64,
    successful_snapshots: &mut u8,
) -> Result<(), &'static str> {
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(10))
        .ok_or("ax_clock")?;
    let context = initial.snapshot.frame().context();
    let mut baseline = initial;
    let page = super::super::native_webview(view.view());
    let mut remote_ready = false;
    for sample in 0..4 {
        check(runtime, deadline, false)?;
        if sample > 0 {
            wait_for_action_security_settle(runtime, Duration::from_millis(125 * sample))?;
        }
        if page.accessibilityHitTest(NSPoint::new(0.0, 0.0)).is_some() {
            remote_ready = true;
            break;
        }
    }
    if !remote_ready {
        return Err("ax_remote_initialization_unavailable");
    }
    for (label, desired) in [
        ("Semantic fill editable", "ax flat replacement"),
        ("Fill support editable ancestor", "nested replacement"),
    ] {
        check(runtime, deadline, false)?;
        if baseline.snapshot.completeness() != SemanticCompleteness::Complete {
            return Err("ax_incomplete");
        }
        let candidates: Vec<_> = baseline
            .snapshot
            .nodes()
            .iter()
            .filter(|node| node_name_is(node, label))
            .collect();
        if candidates.len() != 1 {
            return Err("ax_target_ambiguous");
        }
        let node = candidates[0];
        let page = super::super::native_webview(view.view());
        let bound = inspect(&page, window, node)?;
        // Cancellation/deadline checks are pre-observable. A dispatched AX call
        // is never retried, including when the native setter returns no result.
        if check(runtime, deadline, true) != Err("ax_cancelled_before_dispatch")
            || check(runtime, Instant::now(), false) != Err("ax_deadline_before_dispatch")
        {
            return Err("ax_control_gate");
        }
        let generation = baseline.snapshot.generation();
        let fresh = capture_snapshot(
            view,
            context,
            url,
            generation.next().ok_or("ax_generation")?,
            next_invocation,
            successful_snapshots,
            runtime,
        )?;
        let current = fresh
            .snapshot
            .nodes()
            .iter()
            .find(|current| node_name_is(current, label))
            .ok_or("ax_target_changed")?;
        if fresh.snapshot.frame() != baseline.snapshot.frame()
            || current.geometry() != node.geometry()
            || current.value() != node.value()
            || current.role() != node.role()
            || current.name() != node.name()
        {
            return Err("ax_semantic_target_changed");
        }
        let rebound = inspect(&page, window, current)?;
        if Retained::as_ptr(&bound.element) != Retained::as_ptr(&rebound.element)
            || bound.frame != rebound.frame
            || bound.point != rebound.point
        {
            return Err("ax_identity_changed");
        }
        check(runtime, deadline, false)?;
        let replacement = NSString::from_str(desired);
        // SAFETY: public setter, retained exact hit-test object, method allowed,
        // exact role/value/label/frame reattested with adjacent semantic state.
        // This fixture-only experiment grants no production controller authority.
        objc2::exception::catch(AssertUnwindSafe(|| unsafe {
            if rebound.legacy {
                let key = NSString::from_str("AXValue");
                let _: () = msg_send![&*rebound.element, accessibilitySetValue: &*replacement, forAttribute: &*key];
            } else {
                let _: () = msg_send![&*rebound.element, setAccessibilityValue: &*replacement];
            }
        }))
        .map_err(|_| "ax_applied_unverified_exception")?;
        wait_for_action_security_settle(runtime, ACTION_SECURITY_SETTLE)?;
        if runtime.failed() || Instant::now() >= deadline {
            return Err("ax_applied_unverified_lifecycle");
        }
        baseline = capture_snapshot(
            view,
            context,
            url,
            fresh.snapshot.generation().next().ok_or("ax_generation")?,
            next_invocation,
            successful_snapshots,
            runtime,
        )?;
        if !snapshot_value_is(&baseline.snapshot, label, desired) {
            return Err("ax_outcome_not_observed");
        }
        if !snapshot_contains(
            &baseline.snapshot,
            if label == "Semantic fill editable" {
                "AX flat application retained"
            } else {
                "AX nested application retained"
            },
        ) {
            return Err("ax_application_model_not_observed");
        }
    }
    Ok(())
}
