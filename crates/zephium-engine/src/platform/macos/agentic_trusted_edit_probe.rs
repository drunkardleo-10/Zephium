//! Fixed local trusted-input experiment, compiled only by the excluded probe.
//! This grant is context-wide local-fixture authority, never an exact-leaf Fill
//! permit. No production caller, provider payload, selector, or script enters it.

use std::panic::AssertUnwindSafe;

use objc2::{msg_send, sel};
use objc2_foundation::{NSRange, NSString};

use super::*;

pub(super) const ENV: &str = "ZEPHIUM_LOCAL_TRUSTED_EDIT_PROBE";

pub(super) fn case_from_env() -> Result<Option<&'static str>, &'static str> {
    match std::env::var(ENV) {
        Err(std::env::VarError::NotPresent) => Ok(None),
        Ok(value) => match value.as_str() {
            "normal" => Ok(Some("normal")),
            "retarget" => Ok(Some("retarget")),
            "retarget-before-native" => Ok(Some("retarget-before-native")),
            "retarget-text-input" => Ok(Some("retarget-text-input")),
            "cancel" => Ok(Some("cancel")),
            "navigation" => Ok(Some("navigation")),
            "takeover-before" => Ok(Some("takeover-before")),
            "takeover-after" => Ok(Some("takeover-after")),
            _ => Err("trusted_fixture_case"),
        },
        _ => Err("trusted_fixture_case"),
    }
}

/// No reset exists: after dispatch entry even an exception is nonretryable.
/// Activation is conservatively tainted before crossing the native boundary,
/// independently of a page's subsequent userActivation claim or timeout.
#[derive(Default)]
struct OneShotInteraction {
    entered: bool,
    revoked: bool,
    activation_tainted: bool,
}

impl OneShotInteraction {
    fn enter(&mut self, authority_matches: bool) -> Result<(), &'static str> {
        if self.entered || self.revoked || !authority_matches {
            self.revoked = true;
            return Err("trusted_dispatch_refused");
        }
        self.entered = true;
        self.activation_tainted = true;
        Ok(())
    }

    fn revoke(&mut self) {
        self.revoked = true;
    }
}

pub(super) fn run(
    view: &AgentOwnedView,
    window: &NSWindow,
    store: &Retained<WKWebsiteDataStore>,
    initial: CapturedSnapshot,
    fixture: ProbeCase<'_>,
    runtime: &ProbeRuntime<'_, '_>,
) -> Result<(), &'static str> {
    let ProbeCase {
        url,
        case,
        next_invocation,
        successful_snapshots,
    } = fixture;
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(5))
        .ok_or("trusted_deadline")?;
    let context = initial.snapshot.frame().context();
    let fresh = capture_snapshot(
        view,
        context,
        url,
        initial
            .snapshot
            .generation()
            .next()
            .ok_or("trusted_generation")?,
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
        generation.next().ok_or("trusted_generation")?,
        next_invocation,
        successful_snapshots,
        runtime,
    )?;
    verify_primary_click_execution(pending, &prepared.snapshot)?;
    if !snapshot_contains(&prepared.snapshot, "Trusted unit setup exact logical range") {
        return Err("trusted_fixture_setup");
    }
    if case == "retarget-before-native" {
        // Deliberately allow the fixed fixture's timer to invalidate selection
        // after observation. Native ownership remains exact; DOM focus does not.
        wait_for_action_security_settle(runtime, Duration::from_millis(700))?;
    }
    let page = super::super::native_webview(view.view());
    // Full hidden configuration/profile attestation runs before presentation.
    // Recheck its exact store and extension isolation at the dispatch boundary;
    // presentation geometry is independently owned by the presented guard.
    // SAFETY: retained page/configuration reads on this process's main thread.
    let storage_matches = unsafe {
        let configuration = page.configuration();
        let actual = configuration.websiteDataStore();
        Retained::as_ptr(&actual) == Retained::as_ptr(store)
            && !actual.isPersistent()
            && actual.identifier().is_none()
            && configuration.webExtensionController().is_none()
    };
    let mut grant = OneShotInteraction::default();
    if case == "takeover-before" {
        grant.revoke();
    }
    runtime.native_guard.sample();
    let authority_matches = storage_matches
        && !runtime.failed()
        && Instant::now() < deadline
        && prepared.snapshot.frame().context() == context
        && view.view().url().ok().as_deref() == Some(url)
        && window.firstResponder().is_some_and(|responder| {
            Retained::as_ptr(&responder).cast::<c_void>()
                == Retained::as_ptr(&page).cast::<c_void>()
        });
    if grant.enter(authority_matches).is_ok() {
        eprintln!("trusted-edit-dispatch: case={case} entered=true activation_tainted=true postdispatch_retry=false");
        // One fixed public native call into the exact owned WKWebView. Default
        // range intentionally models browser typing into its current selection;
        // it does not pretend public range getters attest a semantic leaf.
        objc2::exception::catch(AssertUnwindSafe(|| {
            // SAFETY: NSObject introspection on the retained owned page.
            let supported: bool = unsafe {
                msg_send![&*page, respondsToSelector: sel!(insertText:replacementRange:)]
            };
            if !supported {
                return Err("trusted_insert_unsupported_nonretryable");
            }
            // SAFETY: public NSTextInputClient selector, retained NSString and
            // valid by-value NSRange; exact page ownership checked above. The
            // fixture-only grant explicitly permits current-selection editing.
            let _: () = unsafe {
                msg_send![&*page,
                insertText: &*NSString::from_str("unit replacement"),
                replacementRange: NSRange::new(isize::MAX as usize, 0)]
            };
            Ok(())
        }))
        .map_err(|_| "trusted_insert_exception_nonretryable")??;
    } else if case != "takeover-before" {
        return Err("trusted_native_authority");
    }
    if case == "takeover-after" {
        // Revocation cannot retract input already queued in the Web process.
        grant.revoke();
    }
    wait_for_action_security_settle(runtime, Duration::from_millis(1500)).map_err(|_| {
        if grant.entered {
            "trusted_settle_failed_nonretryable"
        } else {
            "trusted_settle_failed_before_dispatch"
        }
    })?;
    let settled = capture_snapshot(
        view,
        context,
        url,
        prepared
            .snapshot
            .generation()
            .next()
            .ok_or("trusted_generation")?,
        next_invocation,
        successful_snapshots,
        runtime,
    )
    .map_err(|_| {
        if grant.entered {
            "trusted_snapshot_failed_nonretryable"
        } else {
            "trusted_snapshot_failed_before_dispatch"
        }
    })?;
    let witness = settled
        .snapshot
        .nodes()
        .iter()
        .find_map(|node| {
            node.name()
                .map(|name| name.as_str())
                .filter(|name| name.starts_with("Trusted unit witness"))
        })
        .ok_or("trusted_witness_missing_nonretryable")?;
    // Redacted fixture-only closed words/counters, never real page text.
    eprintln!("trusted-edit-witness: case={case} {witness}");
    let expected = match case {
        "cancel" => "Trusted unit witness inputs 0 trusted 0 before 1 model original rerender no leaf same sibling intact decoy intact active no sticky no popup denied retained yes",
        "takeover-before" => "Trusted unit witness inputs 0 trusted 0 before 0 model original rerender no leaf same sibling intact decoy intact active no sticky no popup denied retained yes",
        "retarget-before-native" | "retarget-text-input" => "Trusted unit witness inputs 1 trusted 1 before 0 model original rerender no leaf same sibling intact decoy changed active no sticky no popup denied retained yes",
        _ => "Trusted unit witness inputs 1 trusted 1 before 1 model accepted rerender yes leaf replaced sibling intact decoy intact active no sticky no popup denied retained yes",
    };
    if witness != expected {
        return Err("trusted_witness_changed_nonretryable");
    }
    // Independent current value of the decoy, not just application telemetry.
    let decoy_expected = if matches!(case, "retarget-before-native" | "retarget-text-input") {
        "unit replacement"
    } else {
        "unit decoy"
    };
    if !snapshot_value_is(&settled.snapshot, "Trusted unit decoy", decoy_expected) {
        return Err("trusted_decoy_snapshot_mismatch_nonretryable");
    }
    if view.view().url().ok().as_deref() != Some(url) {
        return Err("trusted_document_changed_nonretryable");
    }
    if !witness.contains("sibling intact") || !witness.contains("popup denied") {
        return Err("trusted_fixture_boundary_nonretryable");
    }
    let verified = witness.contains("model accepted")
        && witness.contains("retained yes")
        && witness.contains("trusted 1")
        && witness.contains("decoy intact")
        && snapshot_value_is(&settled.snapshot, "Trusted unit text", "unit replacement");
    if matches!(case, "normal" | "navigation" | "takeover-after") && !verified {
        return Err("trusted_model_unverified_nonretryable");
    }
    if case == "takeover-before" && (grant.entered || !witness.contains("inputs 0")) {
        return Err("trusted_takeover_before_failed");
    }
    eprintln!("trusted-edit-authority: case={case} dispatch_entered={} activation_tainted={} revoked={} observed_model_verified={verified} action_success={} retry_permitted=false taint_release=owned_view_teardown production_admission=absent",
        grant.entered, grant.activation_tainted, grant.revoked, verified && !grant.revoked);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::OneShotInteraction;

    #[test]
    fn revocation_and_uncertainty_never_regrant_or_clear_taint() {
        let mut before = OneShotInteraction::default();
        before.revoke();
        assert!(before.enter(true).is_err());
        assert!(!before.entered && !before.activation_tainted);
        let mut mismatched = OneShotInteraction::default();
        assert!(mismatched.enter(false).is_err());
        assert!(mismatched.enter(true).is_err());
        let mut after = OneShotInteraction::default();
        assert!(after.enter(true).is_ok());
        after.revoke();
        assert!(after.enter(true).is_err());
        assert!(after.activation_tainted);
        let mut exception = OneShotInteraction::default();
        assert!(exception.enter(true).is_ok());
        assert!(exception.enter(true).is_err());
        assert!(exception.activation_tainted);
    }
}
