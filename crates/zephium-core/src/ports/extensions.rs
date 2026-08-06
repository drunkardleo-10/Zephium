use std::time::Instant;

use crate::ids::ProfileId;

/// Application-facing result of consuming the extension-service owner.
///
/// `Unclean` is terminal. The concrete service may retain more precise
/// diagnostics, but once its unique owner has been consumed there is no safe
/// in-process retry surface for application code.
#[must_use = "extension-service shutdown must be checked before Store teardown"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionServiceShutdownOutcome {
    /// The service proved that its worker terminated and released its owned
    /// resources after draining every admitted command.
    Clean,
    /// The service could not prove clean worker termination and resource
    /// release before returning.
    Unclean,
}

/// Application-facing settlement of extension-service startup.
///
/// This projection intentionally carries no package, profile, repository, or
/// native-owner identity. Only `Ready` authorizes callers to continue into
/// extension-sensitive bootstrap work such as recovered profile deletion or
/// raw content-view construction.
#[must_use = "extension-sensitive bootstrap requires an explicit Ready settlement"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionServiceStartupOutcome {
    /// Repository recovery and every durable possible-owner cleanup completed.
    Ready,
    /// One or more possible native owners remain and require cleanup.
    CleanupRequired,
    /// The exact startup attempt settled with a definite retryable condition.
    Unavailable,
    /// Startup detected corruption, invariant loss, or another closed failure.
    FailedClosed,
    /// The caller's observation deadline elapsed without terminal settlement.
    TimedOut,
    /// A retry was definitely not admitted because bounded transient capacity
    /// was unavailable. No startup frontier ran, so a later bounded retry is
    /// permitted while the same owner remains live.
    RetryableNotAdmitted,
}

/// Coarse settlement of one synchronous profile-retirement continuation.
///
/// This value is deliberately informational and publicly constructible. It is
/// not an unforgeable capability. Zephium's audited composition rule is that
/// Store authorization, native website-data erasure, and Store finalization
/// occur only inside the continuation passed to
/// [`ExtensionServiceLifecycle::with_profile_retired_until`]. The service
/// invokes that continuation only after its worker directly settles the exact
/// profile as retired. App-level behavior tests protect all three call sites;
/// the Rust type system does not independently enforce that architectural
/// rule.
#[must_use = "profile-retirement continuation settlement must be checked"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionProfileRetirementDisposition {
    /// The service proved retirement and invoked the continuation exactly
    /// once before returning.
    Continued,
    /// Retirement did not settle in this attempt. The continuation was not
    /// invoked and profile data must remain intact for a bounded retry.
    Unavailable,
    /// The service failed closed. The continuation was not invoked and the
    /// current process must not continue profile deletion.
    FailedClosed,
}

/// Move-only application lifecycle boundary for the extension service.
///
/// The unique owner is held behind a `Box` and consumed by shutdown. This
/// keeps implementation-specific mutation and cleanup authority out of the
/// application layer, while allowing the owner to move onto the application
/// actor thread. No cloneable or borrowed shutdown operation exists.
pub trait ExtensionServiceLifecycle: Send {
    /// Observes the active startup attempt, or admits one bounded retry when
    /// the previous exact attempt settled unavailable.
    ///
    /// This call may wait until the absolute `deadline` and must run away from
    /// a native UI/event-loop thread: native-owner reconciliation can require
    /// that same thread to service WebKit/WebView2 work. Only
    /// [`ExtensionServiceStartupOutcome::Ready`] permits extension-sensitive
    /// application bootstrap.
    fn settle_startup_until(&mut self, deadline: Instant) -> ExtensionServiceStartupOutcome;

    /// Permanently fences `profile`, proves every extension-owned durable,
    /// package, and native obligation absent, then invokes `continuation`
    /// exactly once before returning [`ExtensionProfileRetirementDisposition::Continued`].
    ///
    /// The continuation is the authority boundary. Implementations must drop
    /// it without invocation for every unavailable or failed-closed result.
    /// Zephium callers must put Store authorization, native website-data
    /// erasure, or Store finalization *inside* this continuation and must not
    /// branch on the copied return value to perform those effects later. This
    /// is an audited composition rule, not a type-level capability guarantee.
    ///
    /// This call may wait until the absolute `deadline` and follows the same
    /// native-event-loop restriction as [`Self::settle_startup_until`]. The
    /// default is fail-closed so test-only or compatibility lifecycle adapters
    /// cannot accidentally authorize deletion when they have no retirement
    /// implementation.
    fn with_profile_retired_until(
        &mut self,
        _profile: ProfileId,
        _deadline: Instant,
        continuation: Box<dyn FnOnce() + '_>,
    ) -> ExtensionProfileRetirementDisposition {
        drop(continuation);
        ExtensionProfileRetirementDisposition::FailedClosed
    }

    /// Seals service admission and consumes the unique owner while attempting
    /// to prove worker termination and resource release by `deadline`.
    ///
    /// The deadline is absolute. Every non-clean concrete service outcome
    /// must project to [`ExtensionServiceShutdownOutcome::Unclean`]; ownership
    /// has already been consumed and cannot be retried in-process.
    fn shutdown_until(self: Box<Self>, deadline: Instant) -> ExtensionServiceShutdownOutcome;
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use super::*;

    struct TestLifecycle {
        consumed: Arc<AtomicBool>,
    }

    impl ExtensionServiceLifecycle for TestLifecycle {
        fn settle_startup_until(&mut self, _deadline: Instant) -> ExtensionServiceStartupOutcome {
            ExtensionServiceStartupOutcome::Ready
        }

        fn with_profile_retired_until(
            &mut self,
            _profile: ProfileId,
            _deadline: Instant,
            continuation: Box<dyn FnOnce() + '_>,
        ) -> ExtensionProfileRetirementDisposition {
            continuation();
            ExtensionProfileRetirementDisposition::Continued
        }

        fn shutdown_until(self: Box<Self>, _deadline: Instant) -> ExtensionServiceShutdownOutcome {
            self.consumed.store(true, Ordering::Release);
            ExtensionServiceShutdownOutcome::Clean
        }
    }

    fn assert_send<T: Send>() {}

    #[test]
    fn lifecycle_is_send_object_safe_and_consumed_by_shutdown() {
        assert_send::<Box<dyn ExtensionServiceLifecycle>>();
        let consumed = Arc::new(AtomicBool::new(false));
        let mut lifecycle: Box<dyn ExtensionServiceLifecycle> = Box::new(TestLifecycle {
            consumed: Arc::clone(&consumed),
        });

        assert_eq!(
            lifecycle.settle_startup_until(Instant::now()),
            ExtensionServiceStartupOutcome::Ready
        );
        let continued = Arc::new(AtomicBool::new(false));
        let continued_by_callback = Arc::clone(&continued);
        assert_eq!(
            lifecycle.with_profile_retired_until(
                ProfileId::from(7),
                Instant::now(),
                Box::new(move || continued_by_callback.store(true, Ordering::Release)),
            ),
            ExtensionProfileRetirementDisposition::Continued
        );
        assert!(continued.load(Ordering::Acquire));
        assert_eq!(
            lifecycle.shutdown_until(Instant::now()),
            ExtensionServiceShutdownOutcome::Clean
        );
        assert!(consumed.load(Ordering::Acquire));
    }
}
