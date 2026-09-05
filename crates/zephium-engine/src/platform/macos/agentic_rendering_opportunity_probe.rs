//! Closed public-API comparisons, never a production rendering policy.
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]

use super::*;

use block2::RcBlock;
use objc2_foundation::{NSError, NSNumber};
use objc2_web_kit::{WKInactiveSchedulingPolicy, WKPreferences, WKSnapshotConfiguration};

const SNAPSHOT_OPPORTUNITY_TIMEOUT: Duration = Duration::from_secs(1);

/// Fixed provider-free comparison selected only by the excluded CLI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderingOpportunity {
    /// Temporarily request normal process scheduling while keeping the view hidden.
    Unthrottled,
    /// Request exactly one one-logical-pixel native snapshot, without exporting pixels.
    NativeSnapshot,
}

/// Content-free result returned after restoration and original native teardown.
#[derive(Debug)]
pub struct MacosAgenticRenderingOpportunityReport {
    /// Which fixed public native API was exercised.
    pub opportunity: RenderingOpportunity,
    /// Time spent acquiring the opportunity before the first semantic sample.
    pub opportunity_elapsed_ms: u64,
    /// Same bounded sampling contract; offsets start after opportunity acquisition.
    pub measurement: MacosAgenticRenderingProbeReport,
}

struct SchedulingScope {
    preferences: Retained<WKPreferences>,
}

impl SchedulingScope {
    fn begin(page: &WKWebView) -> Result<Self, &'static str> {
        let _mtm = MainThreadMarker::new().ok_or("rendering_opportunity_main_thread")?;
        // SAFETY: the exact owned page and its returned configuration/preferences
        // are retained; the marker proves all messages execute on the main thread.
        let preferences = unsafe { page.configuration().preferences() };
        // SAFETY: same retained main-thread preferences. Check before creating
        // a restore owner, so an unexpected initial policy is never overwritten.
        let original = objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
            preferences.inactiveSchedulingPolicy()
        }))
        .map_err(|_| "rendering_opportunity_policy_read")?;
        if original != WKInactiveSchedulingPolicy::Throttle {
            return Err("rendering_opportunity_original_policy");
        }
        let scope = Self { preferences };
        // SAFETY: this is the public API on the retained exact view preferences.
        // No visibility, focus, native input, script or other preference changes.
        objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
            scope
                .preferences
                .setInactiveSchedulingPolicy(WKInactiveSchedulingPolicy::None);
        }))
        .map_err(|_| "rendering_opportunity_policy_set")?;
        if scope.policy()? != WKInactiveSchedulingPolicy::None {
            return Err("rendering_opportunity_policy_attestation");
        }
        Ok(scope)
    }

    fn policy(&self) -> Result<WKInactiveSchedulingPolicy, &'static str> {
        // SAFETY: this scope is main-thread-only and retains the original preferences.
        objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
            self.preferences.inactiveSchedulingPolicy()
        }))
        .map_err(|_| "rendering_opportunity_policy_read")
    }

    fn restore(&self) -> Result<(), &'static str> {
        // SAFETY: restore only the single public preference changed by this scope.
        objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
            self.preferences
                .setInactiveSchedulingPolicy(WKInactiveSchedulingPolicy::Throttle);
        }))
        .map_err(|_| "rendering_opportunity_policy_restore")?;
        if self.policy()? != WKInactiveSchedulingPolicy::Throttle {
            return Err("rendering_opportunity_restore_attestation");
        }
        Ok(())
    }
}

impl Drop for SchedulingScope {
    fn drop(&mut self) {
        // Backstop for early refusal/unwind; successful evidence additionally
        // requires the explicit checked restoration and original view attestation.
        let _ = self.restore();
    }
}

#[derive(Default)]
struct SnapshotTerminal {
    result: Cell<Option<bool>>,
    duplicate: Cell<bool>,
}

impl SnapshotTerminal {
    fn deliver(&self, valid: bool) {
        if self.result.replace(Some(valid)).is_some() {
            self.duplicate.set(true);
        }
    }

    fn attest(&self) -> Result<(), &'static str> {
        if self.duplicate.get() {
            return Err("rendering_opportunity_duplicate_callback");
        }
        if self.result.get() != Some(true) {
            return Err("rendering_opportunity_snapshot_refused");
        }
        Ok(())
    }
}

fn snapshot_opportunity(
    page: &WKWebView,
    runtime: &ProbeRuntime<'_, '_>,
) -> Result<Rc<SnapshotTerminal>, &'static str> {
    let mtm = MainThreadMarker::new().ok_or("rendering_opportunity_main_thread")?;
    let deadline = Instant::now()
        .checked_add(SNAPSHOT_OPPORTUNITY_TIMEOUT)
        .ok_or("rendering_opportunity_deadline")?;
    // SAFETY: creation and configuration are main-thread-only. The rectangle
    // and output width are fixed to one logical pixel, independent of page input.
    let configuration = unsafe {
        let configuration = WKSnapshotConfiguration::new(mtm);
        configuration.setRect(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0)));
        configuration.setSnapshotWidth(Some(&NSNumber::new_f64(1.0)));
        configuration.setAfterScreenUpdates(true);
        configuration
    };
    let terminal = Rc::new(SnapshotTerminal::default());
    let completion_terminal = terminal.clone();
    let callback: RcBlock<dyn Fn(*mut objc2_app_kit::NSImage, *mut NSError)> = RcBlock::new(
        move |image: *mut objc2_app_kit::NSImage, error: *mut NSError| {
            // Do not dereference, encode, retain or export image/page content.
            completion_terminal.deliver(!image.is_null() && error.is_null());
        },
    );
    // SAFETY: the exact retained owned page, fixed configuration and copied
    // completion block are live. WebKit retains its one asynchronous callback.
    objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
        page.takeSnapshotWithConfiguration_completionHandler(Some(&configuration), &callback);
    }))
    .map_err(|_| "rendering_opportunity_snapshot_dispatch")?;
    while terminal.result.get().is_none() && !runtime.failed() && Instant::now() < deadline {
        runtime.pump();
    }
    runtime.native_guard.sample();
    if runtime.failed() {
        return Err("rendering_opportunity_native_state");
    }
    if Instant::now() >= deadline {
        return Err("rendering_opportunity_snapshot_timeout");
    }
    terminal.attest()?;
    Ok(terminal)
}

pub(super) fn measure(
    view: &AgentOwnedView,
    context: zephium_agentic::ContextJoin,
    operation: zephium_agentic::ContextOperationJoin,
    url: &str,
    runtime: &ProbeRuntime<'_, '_>,
    opportunity: RenderingOpportunity,
) -> Result<MacosAgenticRenderingOpportunityReport, &'static str> {
    if operation.context() != context
        || view
            .navigation()
            .document_finished_for_audit(operation)
            .is_none()
        || runtime.failed()
    {
        return Err("rendering_opportunity_navigation_join");
    }
    let page = super::super::native_webview(view.view());
    let started = Instant::now();
    let (scope, snapshot) = match opportunity {
        RenderingOpportunity::Unthrottled => (Some(SchedulingScope::begin(&page)?), None),
        RenderingOpportunity::NativeSnapshot => (None, Some(snapshot_opportunity(&page, runtime)?)),
    };
    let opportunity_elapsed_ms =
        u64::try_from(started.elapsed().as_millis()).map_err(|_| "rendering_opportunity_clock")?;
    let measurement = rendering::measure(view, context, operation, url, runtime);
    // Preserve the original result only after checked restore. A restore fault
    // is not success even when the semantic fixture happened to converge.
    if let Some(scope) = scope {
        scope.restore()?;
    }
    if let Some(snapshot) = snapshot {
        snapshot.attest()?;
    }
    runtime.native_guard.sample();
    if runtime.failed() {
        return Err("rendering_opportunity_native_state");
    }
    Ok(MacosAgenticRenderingOpportunityReport {
        opportunity,
        opportunity_elapsed_ms,
        measurement: measurement?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_opportunity_requires_one_terminal_and_keeps_duplicate_failure_sticky() {
        let missing = SnapshotTerminal::default();
        assert!(missing.attest().is_err());
        for first in [false, true] {
            for second in [false, true] {
                let terminal = SnapshotTerminal::default();
                terminal.deliver(first);
                assert_eq!(terminal.attest().is_ok(), first);
                terminal.deliver(second);
                assert_eq!(
                    terminal.attest(),
                    Err("rendering_opportunity_duplicate_callback")
                );
                terminal.deliver(true);
                assert!(terminal.attest().is_err());
            }
        }
    }
}
