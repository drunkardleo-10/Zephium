#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]

//! Exact callback/timeout ownership for one WebView2 suspend attempt.
//!
//! WebView2 does not expose cancellation for `TrySuspend`. A native callback
//! that loses to the bounded timeout or logical cancellation therefore owes a
//! reconciliation pass instead of being silently discarded.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

const PENDING: u8 = 0;
const NATIVE_COMPLETED: u8 = 1;
const TIMED_OUT: u8 = 2;
const CANCELLED: u8 = 3;
const RETIRED: u8 = 4;
const LATE_NATIVE_COMPLETED: u8 = 5;

/// What the owning UI-thread host must do with one native completion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentSuspendNativeDisposition {
    /// Native completion won and owns the one external transition terminal.
    Terminal,
    /// Timeout or cancellation already settled externally; restore and attest
    /// an active native state before clearing the retained cleanup debt.
    Reconcile,
    /// The view was retired, so its callback carries no live native authority.
    Retired,
    /// A supposedly one-shot native callback ran more than once.
    Duplicate,
}

/// Cloneable claim shared only by the pending owner, timer, and native callback.
#[derive(Clone)]
pub(crate) struct AgentSuspendClaim {
    state: Arc<AtomicU8>,
}

impl AgentSuspendClaim {
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(AtomicU8::new(PENDING)),
        }
    }

    /// Claims the external terminal for the fixed timeout.
    pub(crate) fn timeout(&self) -> bool {
        self.state
            .compare_exchange(PENDING, TIMED_OUT, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Revokes a pending terminal while preserving late reconciliation debt.
    pub(crate) fn cancel(&self) -> bool {
        self.state
            .compare_exchange(PENDING, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Permanently retires callback authority because the view is closing.
    pub(crate) fn retire(&self) {
        self.state.store(RETIRED, Ordering::Release);
    }

    /// Classifies the sole native completion without dropping late cleanup.
    pub(crate) fn native_completed(&self) -> AgentSuspendNativeDisposition {
        let mut observed = self.state.load(Ordering::Acquire);
        loop {
            let (next, disposition) = match observed {
                PENDING => (NATIVE_COMPLETED, AgentSuspendNativeDisposition::Terminal),
                TIMED_OUT | CANCELLED => (
                    LATE_NATIVE_COMPLETED,
                    AgentSuspendNativeDisposition::Reconcile,
                ),
                RETIRED => return AgentSuspendNativeDisposition::Retired,
                NATIVE_COMPLETED | LATE_NATIVE_COMPLETED => {
                    return AgentSuspendNativeDisposition::Duplicate;
                }
                _ => return AgentSuspendNativeDisposition::Duplicate,
            };
            match self.state.compare_exchange_weak(
                observed,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return disposition,
                Err(current) => observed = current,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_timeout_and_cancellation_have_one_external_terminal() {
        let native = AgentSuspendClaim::new();
        assert_eq!(
            native.native_completed(),
            AgentSuspendNativeDisposition::Terminal
        );
        assert!(!native.timeout());
        assert_eq!(
            native.native_completed(),
            AgentSuspendNativeDisposition::Duplicate
        );

        let timeout = AgentSuspendClaim::new();
        assert!(timeout.timeout());
        assert_eq!(
            timeout.native_completed(),
            AgentSuspendNativeDisposition::Reconcile
        );

        let cancelled = AgentSuspendClaim::new();
        assert!(cancelled.cancel());
        assert!(!cancelled.cancel());
        assert!(!cancelled.timeout());
        assert_eq!(
            cancelled.native_completed(),
            AgentSuspendNativeDisposition::Reconcile
        );
        assert_eq!(
            cancelled.native_completed(),
            AgentSuspendNativeDisposition::Duplicate
        );
    }

    #[test]
    fn retirement_drops_all_late_callback_authority() {
        let claim = AgentSuspendClaim::new();
        claim.retire();
        assert!(!claim.cancel());
        assert!(!claim.timeout());
        assert_eq!(
            claim.native_completed(),
            AgentSuspendNativeDisposition::Retired
        );
    }

    #[test]
    fn a_native_terminal_that_won_cannot_be_reclassified() {
        let claim = AgentSuspendClaim::new();
        assert_eq!(
            claim.native_completed(),
            AgentSuspendNativeDisposition::Terminal
        );
        assert!(!claim.cancel());
        assert_eq!(
            claim.native_completed(),
            AgentSuspendNativeDisposition::Duplicate
        );
    }
}
