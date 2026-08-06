use std::time::Instant;

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

/// Move-only application lifecycle boundary for the extension service.
///
/// The unique owner is held behind a `Box` and consumed by shutdown. This
/// keeps implementation-specific mutation and cleanup authority out of the
/// application layer, while allowing the owner to move onto the application
/// actor thread. No cloneable or borrowed shutdown operation exists.
pub trait ExtensionServiceLifecycle: Send {
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
        let lifecycle: Box<dyn ExtensionServiceLifecycle> = Box::new(TestLifecycle {
            consumed: Arc::clone(&consumed),
        });

        assert_eq!(
            lifecycle.shutdown_until(Instant::now()),
            ExtensionServiceShutdownOutcome::Clean
        );
        assert!(consumed.load(Ordering::Acquire));
    }
}
