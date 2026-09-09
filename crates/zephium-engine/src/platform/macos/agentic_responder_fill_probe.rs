//! Release-excluded native text-client authority falsification, never a backend.
//! No insertText call is compiled here: readback cannot mint a DOM target lease.

use std::panic::AssertUnwindSafe;

use objc2::{msg_send, sel};
use objc2_foundation::{NSAttributedString, NSRange};

use super::*;

const ENV: &str = "ZEPHIUM_LOCAL_RESPONDER_FILL_PROBE";

pub(super) fn case_from_env() -> Result<Option<&'static str>, &'static str> {
    match std::env::var(ENV) {
        Err(std::env::VarError::NotPresent) => Ok(None),
        Ok(value) => match value.as_str() {
            "flat" => Ok(Some("flat")),
            "nested" => Ok(Some("nested")),
            "retarget" => Ok(Some("retarget")),
            "cross-leaf" => Ok(Some("cross-leaf")),
            _ => Err("responder_fixture_case"),
        },
        _ => Err("responder_fixture_case"),
    }
}

#[derive(Debug, PartialEq)]
struct Readback {
    selection: NSRange,
    marked: NSRange,
    has_marked: bool,
    actual: NSRange,
    text: Option<String>,
}

fn inspect(page: &WKWebView, phase: &str) -> Result<Readback, &'static str> {
    objc2::exception::catch(AssertUnwindSafe(|| {
        for selector in [sel!(insertText:replacementRange:), sel!(selectedRange),
            sel!(markedRange), sel!(hasMarkedText),
            sel!(attributedSubstringForProposedRange:actualRange:)] {
            // SAFETY: public NSObject introspection on the owned WKWebView.
            let supported: bool = unsafe { msg_send![page, respondsToSelector: selector] };
            if !supported { return Err("responder_public_method_absent"); }
        }
        // SAFETY: public NSTextInputClient getters checked on this owned object.
        let (selection, marked, has_marked): (NSRange, NSRange, bool) = unsafe {
            (msg_send![page, selectedRange], msg_send![page, markedRange],
                msg_send![page, hasMarkedText])
        };
        let mut actual = NSRange::new(usize::MAX, 0);
        // Bounded fixed local-fixture read, never arbitrary page/account content.
        // SAFETY: checked public getter, writable range pointer valid for call.
        let text: Option<Retained<NSAttributedString>> = unsafe {
            msg_send![page, attributedSubstringForProposedRange: NSRange::new(0, 64),
                actualRange: &mut actual as *mut NSRange]
        };
        let text = text.map(|value| value.string().to_string());
        eprintln!("responder-fill-read: phase={phase} selected_location={} selected_length={} marked_location={} marked_length={} has_marked={has_marked} actual_location={} actual_length={} text_present={} contains_sibling={} content=redacted",
            selection.location, selection.length, marked.location, marked.length,
            actual.location, actual.length, text.is_some(),
            text.as_ref().is_some_and(|text| text.contains("retained sibling")));
        Ok(Readback { selection, marked, has_marked, actual, text })
    })).map_err(|_| "responder_read_exception")?
}

pub(super) fn run(
    view: &AgentOwnedView,
    window: &NSWindow,
    initial: CapturedSnapshot,
    url: &str,
    case: &str,
    runtime: &ProbeRuntime<'_, '_>,
    next_invocation: &mut u64,
    successful_snapshots: &mut u8,
) -> Result<(), &'static str> {
    let context = initial.snapshot.frame().context();
    // Presentation changed geometry. Capture a fresh ref before the one-shot
    // fixture setup click, which itself is observable and is never retried.
    let fresh = capture_snapshot(
        view,
        context,
        url,
        initial
            .snapshot
            .generation()
            .next()
            .ok_or("responder_generation")?,
        next_invocation,
        successful_snapshots,
        runtime,
    )?;
    let generation = fresh.snapshot.generation();
    let observation = assemble_observation(fresh)?;
    let pending = execute_primary_click(view, &observation, runtime)?;
    wait_for_action_security_settle(runtime, ACTION_SECURITY_SETTLE)?;
    let prepared = capture_snapshot(
        view,
        context,
        url,
        generation.next().ok_or("responder_generation")?,
        next_invocation,
        successful_snapshots,
        runtime,
    )?;
    verify_primary_click_execution(pending, &prepared.snapshot)?;
    if !snapshot_contains(&prepared.snapshot, "Responder setup selected exact leaf") {
        return Err("responder_fixture_setup_unobserved");
    }
    let dom_focus_leaf = snapshot_contains(&prepared.snapshot, "Responder setup focus exact leaf");
    let dom_focus_ancestor =
        snapshot_contains(&prepared.snapshot, "Responder setup focus editing ancestor");
    if !dom_focus_leaf && !dom_focus_ancestor {
        return Err("responder_fixture_focus_unobserved");
    }
    let page = super::super::native_webview(view.view());
    let before = inspect(&page, "prepared")?;
    // Fixed fixture timer challenges read/query continuity. It does not edit
    // field values, and the fresh semantic witness is evidence, not authority.
    wait_for_action_security_settle(runtime, Duration::from_millis(700))?;
    let after = inspect(&page, "challenged")?;
    let settled = capture_snapshot(
        view,
        context,
        url,
        prepared
            .snapshot
            .generation()
            .next()
            .ok_or("responder_generation")?,
        next_invocation,
        successful_snapshots,
        runtime,
    )?;
    let snapshot = &settled.snapshot;
    if !snapshot_contains(
        snapshot,
        "Responder witness fields intact events zero model unchanged",
    ) {
        return Err("responder_fixture_preservation");
    }
    if !snapshot_contains(
        snapshot,
        "activation during inactive sticky inactive popup denied",
    ) {
        return Err("responder_fixture_activation");
    }
    if !snapshot_contains(snapshot, "settle inactive sticky inactive") {
        return Err("responder_fixture_activation_settle");
    }
    let expected = match case {
        "retarget" => "Responder challenge focus decoy selection decoy",
        "cross-leaf" => "Responder challenge selection crosses leaf sibling",
        _ => "Responder challenge selection remains exact leaf",
    };
    let cross_leaf_normalized = case == "cross-leaf"
        && snapshot_contains(
            snapshot,
            "Responder challenge selection normalized by engine",
        );
    if !snapshot_contains(snapshot, expected) && !cross_leaf_normalized {
        return Err("responder_challenge_unobserved");
    }
    if !snapshot_value_is(snapshot, "Semantic fill editable", "fixture editable")
        || !snapshot_value_is(snapshot, "Fill support editable ancestor", "diagnostic")
        || !snapshot_value_is(snapshot, "Semantic fill text", "fixture text")
        || !snapshot_value_is(snapshot, "Semantic fill search", "fixture search")
    {
        return Err("responder_fields_changed");
    }
    runtime.native_guard.sample();
    if runtime.failed() {
        return Err("responder_native_lifecycle");
    }
    let exact_page_responder = window.firstResponder().is_some_and(|responder| {
        Retained::as_ptr(&responder).cast::<c_void>() == Retained::as_ptr(&page).cast::<c_void>()
    });
    eprintln!("responder-fill-authority: case={case} readback_equal={} dom_focus_leaf={dom_focus_leaf} dom_focus_ancestor={dom_focus_ancestor} cross_leaf_normalized={cross_leaf_normalized} exact_page_responder={exact_page_responder} app_active={} window_key={} window_main={} target_lease=false native_insert_dispatches=0 refusal=exact_semantic_target_unproven setup_nonretryable=true content=redacted",
        before == after, NSApplication::sharedApplication(MainThreadMarker::new().ok_or("responder_main_thread")?).isActive(),
        window.isKeyWindow(), window.isMainWindow());
    // Successful falsification/measurement, NOT a successful Fill. Unknown or
    // stale composition, invalid/default ranges, and beforeinput retargeting
    // cannot be made safe by these read-only snapshots; never dispatch or retry.
    Ok(())
}
