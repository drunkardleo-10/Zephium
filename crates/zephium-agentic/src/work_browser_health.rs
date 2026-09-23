//! Bounded resource-lifetime health, independent of every actor mailbox.

use super::WorkBrowserResourceJoin;
use std::fmt;
use std::sync::{
    atomic::{AtomicBool, AtomicU8, Ordering},
    Arc, Mutex,
};
use std::task::Waker;

const PENDING: u8 = 0;
const CURRENT: u8 = 1;
const RETIRED: u8 = 2;
const UNCERTAIN: u8 = 3;

struct HealthState {
    state: AtomicU8,
    construction_timed_out: AtomicBool,
    receiver_alive: AtomicBool,
    reporter_retired: AtomicBool,
    pending_wake: AtomicBool,
    waker: Mutex<Option<Arc<Waker>>>,
}

impl HealthState {
    fn uncertain(&self) {
        self.state.store(UNCERTAIN, Ordering::Release);
    }
    fn wake(&self) {
        let delivered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let waker = self.waker.lock().ok().and_then(|waker| waker.clone());
            let Some(waker) = waker else {
                return false;
            };
            waker.wake_by_ref();
            true
        }));
        if !matches!(delivered, Ok(true)) {
            self.uncertain();
        }
    }
    fn publish(&self, state: u8) {
        // Finite sticky states, not an overwriteable event queue or a counter
        // which could overflow. Uncertainty dominates retirement and cannot heal.
        self.state.fetch_max(state, Ordering::AcqRel);
        if !self.pending_wake.swap(true, Ordering::AcqRel) {
            self.wake();
        }
    }
    fn current(&self) -> bool {
        self.receiver_alive.load(Ordering::Acquire)
            && self.state.load(Ordering::Acquire) == CURRENT
            && self.waker.lock().is_ok_and(|waker| waker.is_some())
    }
}

/// A resource-native health fact, never account, task or execution authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkBrowserResourceHealthState {
    /// The native adapter has not acknowledged observer installation.
    Pending,
    /// Installed and not yet invalidated; every operation still rechecks health.
    Current,
    /// The original native reporting owner retired; this is not absence proof.
    Retired,
    /// Native uncertainty, missing delivery owner or notification failure.
    Uncertain,
}

/// One application-owned, resource/incarnation-bound health receiver.
/// It must outlive actor leases; do not move it into a run's mailbox.
#[must_use]
pub struct WorkBrowserResourceHealth {
    resource: WorkBrowserResourceJoin,
    state: Arc<HealthState>,
}

impl WorkBrowserResourceHealth {
    /// A native construction deadline elapsed while the document was loading.
    pub fn construction_timed_out(&self) -> bool {
        self.state.construction_timed_out.load(Ordering::Acquire)
    }

    /// The original native reporting owner's final health publication and wake
    /// have returned. Cleanup polls this separately from health: the final wake
    /// can run before this completion becomes observable. Sticky uncertainty is
    /// preserved, and the native owner's counted lane must still be audited.
    pub fn reporter_retired(&self) -> bool {
        self.state.reporter_retired.load(Ordering::Acquire)
    }

    /// Exact resource incarnation observed by this receiver, not a run identity.
    pub const fn resource(&self) -> &WorkBrowserResourceJoin {
        &self.resource
    }

    /// Installs the original application wake once, then rechecks pending state.
    /// Registration precedes native dispatch. Waking invokes no page/native work.
    pub fn register(&mut self, waker: Waker) -> WorkBrowserResourceHealthState {
        match self.state.waker.lock() {
            Ok(mut slot) if slot.is_none() => *slot = Some(Arc::new(waker)),
            Ok(_) => self.state.uncertain(),
            Err(_) => self.state.uncertain(),
        }
        if self.state.pending_wake.load(Ordering::Acquire) {
            self.state.wake();
        }
        self.snapshot()
    }

    /// Consumes the coalesced wake only, never the sticky resource-health fact.
    pub fn poll(&mut self) -> WorkBrowserResourceHealthState {
        // Consume the publisher's release on this signal atomic. A plain store
        // could clear a coalesced publication without acquiring its state: if
        // publication wins, snapshot sees it; if this exchange wins, it wakes.
        self.state.pending_wake.swap(false, Ordering::AcqRel);
        self.snapshot()
    }

    /// Current descriptive health. Pending/retired/uncertain all refuse admission.
    pub fn snapshot(&self) -> WorkBrowserResourceHealthState {
        if self.state.waker.is_poisoned() {
            self.state.uncertain();
        }
        match self.state.state.load(Ordering::Acquire) {
            PENDING => WorkBrowserResourceHealthState::Pending,
            CURRENT if self.state.current() => WorkBrowserResourceHealthState::Current,
            RETIRED => WorkBrowserResourceHealthState::Retired,
            _ => WorkBrowserResourceHealthState::Uncertain,
        }
    }
}

impl Drop for WorkBrowserResourceHealth {
    fn drop(&mut self) {
        self.state.receiver_alive.store(false, Ordering::Release);
        self.state.uncertain();
    }
}
impl fmt::Debug for WorkBrowserResourceHealth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WorkBrowserResourceHealth([redacted])")
    }
}

/// Move-only native reporting owner extracted from the original construction.
/// Retain it with the stable resource, never in a lease-specific closure.
#[must_use]
pub struct WorkBrowserResourceHealthReporter {
    resource: WorkBrowserResourceJoin,
    state: Arc<HealthState>,
    installed: AtomicBool,
}
impl WorkBrowserResourceHealthReporter {
    /// Attests actual stable observer installation. Missing registration refuses.
    /// Construction/current document proof remains independently required.
    pub fn install(&self, resource: &WorkBrowserResourceJoin) -> bool {
        if resource != &self.resource
            || self.installed.swap(true, Ordering::AcqRel)
            || !self.state.receiver_alive.load(Ordering::Acquire)
            || !self.state.waker.lock().is_ok_and(|waker| waker.is_some())
        {
            self.invalidate();
            return false;
        }
        self.state.publish(CURRENT);
        self.is_current(resource)
    }
    /// Native adapter attests that an in-flight document load exceeded its window.
    pub fn construction_timed_out(&self) {
        self.state
            .construction_timed_out
            .store(true, Ordering::Release);
        self.invalidate();
    }
    /// Publish uncertainty before coalesced wake; no later event can reopen it.
    pub fn invalidate(&self) {
        self.state.publish(UNCERTAIN);
    }
    /// Native admission must independently reject a lost receiver or wake fault.
    pub fn is_current(&self, resource: &WorkBrowserResourceJoin) -> bool {
        resource == &self.resource && self.installed.load(Ordering::Acquire) && self.state.current()
    }
}
impl Drop for WorkBrowserResourceHealthReporter {
    fn drop(&mut self) {
        // Final publication (including arbitrary wake code) must return before
        // cleanup may reap the receiver. In particular a concurrent cleanup
        // poll cannot treat entry into this destructor as completed retirement.
        self.state
            .publish(if self.installed.load(Ordering::Acquire) {
                RETIRED
            } else {
                UNCERTAIN
            });
        self.state.reporter_retired.store(true, Ordering::Release);
    }
}
impl fmt::Debug for WorkBrowserResourceHealthReporter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WorkBrowserResourceHealthReporter([redacted])")
    }
}

pub(super) fn track(
    resource: WorkBrowserResourceJoin,
) -> (WorkBrowserResourceHealth, WorkBrowserResourceHealthReporter) {
    let state = Arc::new(HealthState {
        state: AtomicU8::new(PENDING),
        construction_timed_out: AtomicBool::new(false),
        receiver_alive: AtomicBool::new(true),
        reporter_retired: AtomicBool::new(false),
        pending_wake: AtomicBool::new(false),
        waker: Mutex::new(None),
    });
    (
        WorkBrowserResourceHealth {
            resource: resource.clone(),
            state: state.clone(),
        },
        WorkBrowserResourceHealthReporter {
            resource,
            state,
            installed: AtomicBool::new(false),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    #[test]
    fn construction_timeout_survives_retirement_without_reclassifying_other_failures() {
        struct WakeOnce;
        impl std::task::Wake for WakeOnce {
            fn wake(self: Arc<Self>) {}
        }
        for timed_out in [false, true] {
            let mut rows = WorkBrowserResources::new(
                WorkId::generate(),
                zephium_core::ids::ProfileId::generate(),
            );
            let request = rows
                .construct(
                    WorkBrowserResourceId::generate(),
                    ContextId::generate(),
                    ContextProfileStorageClass::Ephemeral,
                    AgentPolicyInstant::from_millis(0),
                )
                .unwrap();
            let (mut health, reporter) = track(request.resource().clone());
            health.register(Arc::new(WakeOnce).into());
            assert!(reporter.install(request.resource()));
            assert!(!health.construction_timed_out());
            if timed_out {
                reporter.construction_timed_out();
            } else {
                reporter.invalidate();
            }
            drop(reporter);
            assert_eq!(health.poll(), WorkBrowserResourceHealthState::Uncertain);
            assert_eq!(health.construction_timed_out(), timed_out);
            assert!(health.reporter_retired());
        }
    }

    #[test]
    fn poisoned_registration_cannot_remain_current() {
        struct WakeOnce;
        impl std::task::Wake for WakeOnce {
            fn wake(self: Arc<Self>) {}
        }
        let mut rows =
            WorkBrowserResources::new(WorkId::generate(), zephium_core::ids::ProfileId::generate());
        let request = rows
            .construct(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                ContextProfileStorageClass::Ephemeral,
                AgentPolicyInstant::from_millis(0),
            )
            .unwrap();
        let (mut health, reporter) = track(request.resource().clone());
        health.register(Arc::new(WakeOnce).into());
        assert!(reporter.install(request.resource()));
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _held = health.state.waker.lock().unwrap();
            panic!("intentional registration poison");
        }));
        assert_eq!(health.poll(), WorkBrowserResourceHealthState::Uncertain);
        assert!(!reporter.is_current(request.resource()));
    }
}
