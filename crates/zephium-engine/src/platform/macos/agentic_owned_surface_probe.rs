//! Release-excluded, fixed loopback challenges against the actual owned view.
//! A denial without activation is a measurement, never general containment.

use std::panic::AssertUnwindSafe;

use objc2::{msg_send, sel};
use objc2_foundation::{NSRange, NSString};

use super::*;

pub(super) const ENV: &str = "ZEPHIUM_LOCAL_OWNED_SURFACE_PROBE";

pub(super) fn case_from_env() -> Result<Option<&'static str>, &'static str> {
    match std::env::var(ENV) {
        Err(std::env::VarError::NotPresent) => Ok(None),
        Ok(value) => match value.as_str() {
            "ui" => Ok(Some("ui")),
            "media" => Ok(Some("media")),
            "capture" => Ok(Some("capture")),
            "display" => Ok(Some("display")),
            "display-command" => Ok(Some("display-command")),
            "display-command-direct" => Ok(Some("display-command-direct")),
            "display-document-start" => Ok(Some("display-document-start")),
            "passkey" => Ok(Some("passkey")),
            "geolocation" => Ok(Some("geolocation")),
            "isolated-fill-normal" => Ok(Some("isolated-fill-normal")),
            "isolated-fill-cancel" => Ok(Some("isolated-fill-cancel")),
            "isolated-fill-replace" => Ok(Some("isolated-fill-replace")),
            "isolated-fill-adopt" => Ok(Some("isolated-fill-adopt")),
            "isolated-fill-retarget" => Ok(Some("isolated-fill-retarget")),
            "isolated-fill-protected" => Ok(Some("isolated-fill-protected")),
            "isolated-fill-ancestor" => Ok(Some("isolated-fill-ancestor")),
            "isolated-fill-div-rich" => Ok(Some("isolated-fill-div-rich")),
            "isolated-fill-div-nested" => Ok(Some("isolated-fill-div-nested")),
            "isolated-fill-div-identity" => Ok(Some("isolated-fill-div-identity")),
            "isolated-fill-div-editable" => Ok(Some("isolated-fill-div-editable")),
            "isolated-fill-div-structure" => Ok(Some("isolated-fill-div-structure")),
            "isolated-fill-prepare-selection" => Ok(Some("isolated-fill-prepare-selection")),
            "isolated-fill-prepare-ancestor" => Ok(Some("isolated-fill-prepare-ancestor")),
            _ => Err("surface_fixture_case"),
        },
        _ => Err("surface_fixture_case"),
    }
}

pub(super) fn run(
    view: &AgentOwnedView,
    window: &NSWindow,
    store: &Retained<WKWebsiteDataStore>,
    initial: CapturedSnapshot,
    url: &str,
    case: &str,
    runtime: &ProbeRuntime<'_, '_>,
    next_invocation: &mut u64,
    successful_snapshots: &mut u8,
) -> Result<(), &'static str> {
    let context = initial.snapshot.frame().context();
    let generation = initial.snapshot.generation();
    // Hidden snapshots carry the old presentation geometry; refresh after show.
    let visible = capture_snapshot(
        view,
        context,
        url,
        generation.next().ok_or("surface_generation")?,
        next_invocation,
        successful_snapshots,
        runtime,
    )?;
    let generation = visible.snapshot.generation();
    let observation = assemble_observation(visible)?;
    let pending = execute_primary_click(view, &observation, runtime)?;
    wait_for_action_security_settle(runtime, ACTION_SECURITY_SETTLE)?;
    let prepared = capture_snapshot(
        view,
        context,
        url,
        generation.next().ok_or("surface_generation")?,
        next_invocation,
        successful_snapshots,
        runtime,
    )?;
    verify_primary_click_execution(pending, &prepared.snapshot)?;
    if !snapshot_contains(&prepared.snapshot, "Surface setup prepared") {
        return Err("surface_setup");
    }
    let page = super::super::native_webview(view.view());
    let app = NSApplication::sharedApplication(MainThreadMarker::new().ok_or("main_thread")?);
    let windows: Vec<_> = app
        .windows()
        .iter()
        .map(|window| Retained::as_ptr(&window) as usize)
        .collect();
    // The constructor was fully attested immediately before presentation. Keep
    // exact native owner/profile and responder joins adjacent to one insertion.
    // SAFETY: retained owned page and immutable configuration reads on main.
    let matches = unsafe {
        let configuration = page.configuration();
        let actual = configuration.websiteDataStore();
        Retained::as_ptr(&actual) == Retained::as_ptr(store)
            && !actual.isPersistent()
            && actual.identifier().is_none()
            && configuration.webExtensionController().is_none()
    };
    runtime.native_guard.sample();
    if !matches
        || runtime.failed()
        || view.view().url().ok().as_deref() != Some(url)
        || !window.firstResponder().is_some_and(|responder| {
            Retained::as_ptr(&responder).cast::<c_void>()
                == Retained::as_ptr(&page).cast::<c_void>()
        })
    {
        return Err("surface_native_authority");
    }
    // No retry branch exists. Taint lasts until original owned-view teardown.
    if case.starts_with("isolated-fill-") {
        let generation = prepared.snapshot.generation();
        let observation = assemble_observation(prepared)?;
        let pending = execute_primary_fill(
            view,
            &observation,
            runtime,
            "Surface probe editor",
            "local surface witness",
            2,
            2,
        )?;
        wait_for_action_security_settle(runtime, ACTION_SECURITY_SETTLE)?;
        let settled = capture_snapshot(
            view,
            context,
            url,
            generation.next().ok_or("surface_generation")?,
            next_invocation,
            successful_snapshots,
            runtime,
        )?;
        let preflight = matches!(case, "isolated-fill-div-rich" | "isolated-fill-div-nested");
        let expected_failure = if preflight {
            zephium_agentic::SemanticActionNativeFailure::UnsupportedInteraction
        } else {
            zephium_agentic::SemanticActionNativeFailure::AppliedUnverified
        };
        if pending.settlement.qualification_failure() != Some(expected_failure) {
            return Err("isolated_fill_minted_exact_ref_proof");
        }
        let elapsed = u64::try_from(pending.admitted_at.elapsed().as_millis())
            .map_err(|_| "isolated_fill_clock")?;
        if !matches!(
            pending.execution.settle_and_verify(
                pending.settlement,
                &settled.snapshot,
                SemanticSettleInstant::from_millis(10_000 + elapsed)
            ),
            Err(SemanticActionQualificationError::Settlement)
        ) {
            return Err("isolated_fill_retryable_terminal");
        }
        let witness = settled
            .snapshot
            .nodes()
            .iter()
            .find_map(|node| {
                node.name()
                    .map(|name| name.as_str())
                    .filter(|name| name.starts_with("Command fill witness "))
            })
            .ok_or("isolated_fill_witness")?;
        eprintln!(
            "isolated-fill-result: {witness} retry_permitted=false production_admission=absent"
        );
        let success = matches!(case, "isolated-fill-normal" | "isolated-fill-retarget");
        let (inputs, trusted, sibling, model, child) = match case {
            "isolated-fill-normal" | "isolated-fill-retarget" => {
                (1, true, true, "replacement", false)
            }
            "isolated-fill-cancel" | "isolated-fill-replace" => (0, false, true, "original", false),
            "isolated-fill-prepare-selection" | "isolated-fill-prepare-ancestor" => {
                (0, false, true, "original", false)
            }
            "isolated-fill-adopt" => (1, true, true, "other", true),
            "isolated-fill-protected" => (1, true, false, "replacement", false),
            "isolated-fill-ancestor" => (1, true, true, "replacement", false),
            "isolated-fill-div-rich" | "isolated-fill-div-nested" => {
                (0, false, false, "original", false)
            }
            "isolated-fill-div-identity"
            | "isolated-fill-div-editable"
            | "isolated-fill-div-structure" => (1, true, false, "replacement", false),
            _ => return Err("isolated_fill_case"),
        };
        let preparing_attack = matches!(
            case,
            "isolated-fill-prepare-selection" | "isolated-fill-prepare-ancestor"
        );
        let spine = !matches!(
            case,
            "isolated-fill-ancestor" | "isolated-fill-prepare-ancestor"
        );
        let before = u8::from(!preflight && !preparing_attack);
        let suffix = if preparing_attack {
            " preparation_changed true"
        } else {
            ""
        };
        let expected = format!("Command fill witness {} before {before} inputs {inputs} trusted {trusted} active false sticky false sibling {sibling} model {model} child {child} hidden false spine {spine} outside true event_host true{suffix}", case.trim_start_matches("isolated-fill-"));
        if witness != expected
            || (success
                && !snapshot_value_is(
                    &settled.snapshot,
                    "Surface probe editor",
                    "local surface witness",
                ))
            || (inputs == 0
                && !snapshot_value_is(&settled.snapshot, "Surface probe editor", "original"))
            || !snapshot_value_is(&settled.snapshot, "Command decoy", "decoy original")
            || view.view().url().ok().as_deref() != Some(url)
            || runtime.failed()
        {
            return Err("isolated_fill_case_postcondition");
        }
        return Ok(());
    }
    eprintln!("owned-surface-dispatch: case={case} activation_tainted=true retry_permitted=false");
    if matches!(case, "display-command" | "display-command-direct") {
        // Fixed diagnostic only. Public evaluation forces WebKit's internal
        // user-gesture flag; even a timer boundary is not proof of its absence.
        // This case cannot qualify document-start runtime command semantics.
        let body = if case == "display-command" {
            "return new Promise(resolve => setTimeout(() => resolve(Document.prototype.execCommand.call(document, 'insertText', false, 'local surface witness')), 0));"
        } else {
            "return Document.prototype.execCommand.call(document, 'insertText', false, 'local surface witness');"
        };
        let completion = Rc::new(Cell::new(None));
        let completed = Rc::clone(&completion);
        let callback = block2::RcBlock::new(
            move |_result: *mut objc2::runtime::AnyObject,
                  error: *mut objc2_foundation::NSError| {
                completed.set(Some(error.is_null()));
            },
        );
        // SAFETY: exact retained owned page on main; a fresh isolated world,
        // fixed code and no arbitrary page/model arguments. WebKit copies block.
        unsafe {
            let world = objc2_web_kit::WKContentWorld::worldWithName(
                &NSString::from_str("zephium-fixed-owned-display-command-probe"),
                MainThreadMarker::new().ok_or("main_thread")?,
            );
            page.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
                &NSString::from_str(body),
                None,
                None,
                &world,
                Some(&callback),
            );
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while completion.get().is_none() && Instant::now() < deadline && !runtime.failed() {
            runtime.pump();
        }
        if completion.get() != Some(true) || runtime.failed() {
            return Err("surface_command_dispatch_failed_nonretryable");
        }
    } else if case != "display-document-start" {
        objc2::exception::catch(AssertUnwindSafe(|| {
            // SAFETY: public NSObject introspection on this retained owned page.
            let supported: bool = unsafe {
                msg_send![&*page, respondsToSelector: sel!(insertText:replacementRange:)]
            };
            if !supported {
                return Err("surface_insert_unsupported_nonretryable");
            }
            // SAFETY: public NSTextInputClient selector with a retained fixed string
            // and valid current-selection sentinel; exact native joins checked above.
            let _: () = unsafe {
                msg_send![&*page, insertText: &*NSString::from_str("local surface witness"),
            replacementRange: NSRange::new(isize::MAX as usize, 0)]
            };
            Ok(())
        }))
        .map_err(|_| "surface_insert_exception_nonretryable")??;
    }
    let deadline = Instant::now() + Duration::from_millis(3500);
    let mut samples = 0;
    while Instant::now() < deadline {
        runtime.pump();
        samples += 1;
        // Observe actual process windows and sheets, not a page's UI claim.
        let new_window = app
            .windows()
            .iter()
            .any(|candidate| !windows.contains(&(Retained::as_ptr(&candidate) as usize)));
        let sheet = app
            .windows()
            .iter()
            .any(|candidate| candidate.attachedSheet().is_some());
        let modal = app.modalWindow().is_some();
        if new_window || sheet || modal || runtime.failed() {
            eprintln!("owned-surface-native: case={case} new_window={new_window} sheet={sheet} modal={modal} guard_failed={} samples={samples} production_admission=absent", runtime.failed());
            return Err("surface_native_ui_or_lifecycle_nonretryable");
        }
    }
    let settled = capture_snapshot(
        view,
        context,
        url,
        prepared
            .snapshot
            .generation()
            .next()
            .ok_or("surface_generation")?,
        next_invocation,
        successful_snapshots,
        runtime,
    )?;
    let witness = settled
        .snapshot
        .nodes()
        .iter()
        .find_map(|node| {
            node.name()
                .map(|name| name.as_str())
                .filter(|name| name.starts_with("Surface witness "))
        })
        .ok_or("surface_witness_missing_nonretryable")?;
    eprintln!("owned-surface-witness: {witness}");
    if case == "display-document-start" {
        let command_witness = settled
            .snapshot
            .nodes()
            .iter()
            .find_map(|node| {
                node.name()
                    .map(|name| name.as_str())
                    .filter(|name| name.starts_with("Command witness "))
            })
            .ok_or("surface_command_visibility_missing_nonretryable")?;
        eprintln!("owned-surface-command: {command_witness}");
        if command_witness != "Command witness hidden false active false sticky false" {
            return Err("surface_command_activation_or_visibility_nonretryable");
        }
    }
    eprintln!("owned-surface-native: case={case} new_window=false sheet=false modal=false samples={samples} presentation=inactive-nonkey original_url_retained={} production_admission=absent activation_independent_containment=unqualified", view.view().url().ok().as_deref() == Some(url));
    if !witness.contains("inputs 1 trusted true")
        || !witness.contains("secure true")
        || !witness.contains("timer fired")
        || witness.contains("admitted")
        || witness.contains("selected")
        || witness.contains("pending")
        || witness.contains("resolved")
        || !snapshot_value_is(
            &settled.snapshot,
            "Surface probe editor",
            "local surface witness",
        )
        || view.view().url().ok().as_deref() != Some(url)
    {
        return Err("surface_result_uncontained_or_unsettled_nonretryable");
    }
    Ok(())
}
