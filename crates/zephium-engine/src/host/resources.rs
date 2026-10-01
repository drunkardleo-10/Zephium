//! Process-local admission and lifetime accounting for native web resources.
//!
//! Every admitted native object owns exactly one [`NativeResourceLease`].
//! Class ceilings are independent policy pools inside the one hard native
//! ceiling, so pressure in one feature cannot evict or borrow from another.
//! A lease remains owned through the platform object's Rust teardown path.
//! On Windows, an unsuccessful explicit native teardown transfers that same
//! lease to the retryable cleanup-debt owner instead of releasing it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The process-wide hard ceiling for native webview-like resources: the sum
/// of every class budget, so no feature can silently consume tab or teardown
/// capacity.
#[cfg(feature = "agentic-browser")]
pub(super) const MAX_NATIVE_VIEW_RESOURCES: usize =
    51 + if cfg!(target_os = "windows") { 9 } else { 0 };
#[cfg(not(feature = "agentic-browser"))]
pub(super) const MAX_NATIVE_VIEW_RESOURCES: usize =
    43 + if cfg!(target_os = "windows") { 9 } else { 0 };
pub(super) const MAX_NATIVE_TEARDOWN_DEBTS: usize = 8;
#[cfg(feature = "agentic-browser")]
pub(super) const MAX_AGENT_CONTEXT_RESOURCES: usize = 8;
#[cfg(feature = "agentic-browser")]
const _: () = assert!(MAX_AGENT_CONTEXT_RESOURCES == zephium_agentic::MAX_LIVE_CONTEXTS);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NativeResourceClass {
    Tab,
    WarmSpare,
    TeardownDebt,
    #[cfg(feature = "agentic-browser")]
    AgentContext,
    #[cfg(target_os = "windows")]
    Extension,
    TransientConstruction,
}

impl NativeResourceClass {
    #[cfg(feature = "agentic-browser")]
    const COUNT: usize = 5 + cfg!(target_os = "windows") as usize;
    #[cfg(not(feature = "agentic-browser"))]
    const COUNT: usize = 4 + cfg!(target_os = "windows") as usize;
    const ALL: [Self; Self::COUNT] = [
        Self::Tab,
        Self::WarmSpare,
        Self::TeardownDebt,
        #[cfg(feature = "agentic-browser")]
        Self::AgentContext,
        #[cfg(target_os = "windows")]
        Self::Extension,
        Self::TransientConstruction,
    ];

    const fn index(self) -> usize {
        match self {
            Self::Tab => 0,
            Self::WarmSpare => 1,
            Self::TeardownDebt => 2,
            #[cfg(feature = "agentic-browser")]
            Self::AgentContext => 3,
            #[cfg(target_os = "windows")]
            Self::Extension => Self::COUNT - 2,
            Self::TransientConstruction => Self::COUNT - 1,
        }
    }

    pub(super) const fn limit(self) -> usize {
        match self {
            Self::Tab => 32,
            Self::WarmSpare => 1,
            Self::TeardownDebt => MAX_NATIVE_TEARDOWN_DEBTS,
            #[cfg(feature = "agentic-browser")]
            Self::AgentContext => MAX_AGENT_CONTEXT_RESOURCES,
            // Eight observer pages and one foreground popup. This pool never
            // borrows tab, Work, warm-spare, or cleanup-debt capacity.
            #[cfg(target_os = "windows")]
            Self::Extension => 9,
            Self::TransientConstruction => 2,
        }
    }
}

const _: () = {
    let mut total = 0;
    let mut index = 0;
    while index < NativeResourceClass::COUNT {
        total += NativeResourceClass::ALL[index].limit();
        index += 1;
    }
    assert!(total == MAX_NATIVE_VIEW_RESOURCES);
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NativeResourceAdmissionError {
    ClassExhausted(NativeResourceClass),
    GlobalExhausted,
    AccountingInvariant,
}

#[derive(Default)]
struct NativeResourceState {
    counts: [usize; NativeResourceClass::COUNT],
    total: usize,
}

#[derive(Default)]
struct SharedNativeResourceState {
    state: RefCell<NativeResourceState>,
    invariant_failed: Cell<bool>,
}

impl SharedNativeResourceState {
    fn fail(&self) {
        self.invariant_failed.set(true);
    }

    fn try_release(&self, class: NativeResourceClass) -> Result<(), ()> {
        let Ok(mut state) = self.state.try_borrow_mut() else {
            self.fail();
            return Err(());
        };
        let index = class.index();
        let Some(class_count) = state.counts[index].checked_sub(1) else {
            self.fail();
            return Err(());
        };
        let Some(total) = state.total.checked_sub(1) else {
            self.fail();
            return Err(());
        };
        state.counts[index] = class_count;
        state.total = total;
        Ok(())
    }
}

/// The sole admission authority for native web resources owned by one host.
///
/// It is intentionally neither `Send` nor `Sync`: WebKit/WebView2 objects and
/// their teardown proofs are confined to the native UI thread as well.
#[derive(Default)]
pub(super) struct NativeResourceLedger {
    shared: Rc<SharedNativeResourceState>,
}

impl NativeResourceLedger {
    pub(super) fn try_acquire(
        &self,
        class: NativeResourceClass,
    ) -> Result<NativeResourceLease, NativeResourceAdmissionError> {
        if self.shared.invariant_failed.get() {
            return Err(NativeResourceAdmissionError::AccountingInvariant);
        }
        let Ok(mut state) = self.shared.state.try_borrow_mut() else {
            self.shared.fail();
            return Err(NativeResourceAdmissionError::AccountingInvariant);
        };
        if state.total >= MAX_NATIVE_VIEW_RESOURCES {
            return Err(NativeResourceAdmissionError::GlobalExhausted);
        }
        let index = class.index();
        if state.counts[index] >= class.limit() {
            return Err(NativeResourceAdmissionError::ClassExhausted(class));
        }
        let Some(class_count) = state.counts[index].checked_add(1) else {
            self.shared.fail();
            return Err(NativeResourceAdmissionError::AccountingInvariant);
        };
        let Some(total) = state.total.checked_add(1) else {
            self.shared.fail();
            return Err(NativeResourceAdmissionError::AccountingInvariant);
        };
        state.counts[index] = class_count;
        state.total = total;
        drop(state);
        Ok(NativeResourceLease {
            shared: self.shared.clone(),
            class: Some(class),
        })
    }

    pub(super) fn is_healthy(&self) -> bool {
        !self.shared.invariant_failed.get()
    }

    fn is_empty(&self) -> bool {
        self.shared
            .state
            .try_borrow()
            .map(|state| state.total == 0)
            .unwrap_or_else(|_| {
                self.shared.fail();
                false
            })
    }

    /// The only resource-ledger state admissible at a clean host shutdown.
    pub(super) fn is_quiescent(&self) -> bool {
        self.is_healthy() && self.is_empty()
    }

    #[cfg(any(
        test,
        all(
            feature = "agentic-browser",
            any(target_os = "macos", target_os = "windows")
        )
    ))]
    pub(super) fn count_for_audit(&self, class: NativeResourceClass) -> Option<usize> {
        match self.shared.state.try_borrow() {
            Ok(state) => Some(state.counts[class.index()]),
            Err(_) => {
                self.shared.fail();
                None
            }
        }
    }

    #[cfg(test)]
    fn total(&self) -> Option<usize> {
        self.shared.state.try_borrow().ok().map(|state| state.total)
    }
}

/// Unique RAII ownership of one admitted native resource.
pub(crate) struct NativeResourceLease {
    shared: Rc<SharedNativeResourceState>,
    class: Option<NativeResourceClass>,
}

impl NativeResourceLease {
    #[cfg(test)]
    fn class(&self) -> Option<NativeResourceClass> {
        self.class
    }

    /// Atomically transfers an already-owned physical resource between policy
    /// pools. A refused transfer leaves the original class and count intact.
    pub(super) fn reclassify(
        &mut self,
        target: NativeResourceClass,
    ) -> Result<(), NativeResourceAdmissionError> {
        if self.shared.invariant_failed.get() {
            return Err(NativeResourceAdmissionError::AccountingInvariant);
        }
        let Some(source) = self.class else {
            self.shared.fail();
            return Err(NativeResourceAdmissionError::AccountingInvariant);
        };
        let Ok(mut state) = self.shared.state.try_borrow_mut() else {
            self.shared.fail();
            return Err(NativeResourceAdmissionError::AccountingInvariant);
        };
        let source_index = source.index();
        if state.counts[source_index] == 0 {
            self.shared.fail();
            return Err(NativeResourceAdmissionError::AccountingInvariant);
        }
        if source == target {
            return Ok(());
        }
        let target_index = target.index();
        if state.counts[target_index] >= target.limit() {
            return Err(NativeResourceAdmissionError::ClassExhausted(target));
        }
        let Some(source_count) = state.counts[source_index].checked_sub(1) else {
            self.shared.fail();
            return Err(NativeResourceAdmissionError::AccountingInvariant);
        };
        let Some(target_count) = state.counts[target_index].checked_add(1) else {
            self.shared.fail();
            return Err(NativeResourceAdmissionError::AccountingInvariant);
        };
        state.counts[source_index] = source_count;
        state.counts[target_index] = target_count;
        drop(state);
        self.class = Some(target);
        Ok(())
    }

    /// Transfers a resource whose native teardown failed into the debt pool.
    /// Exhausting that reserved pool is not an ordinary admission refusal:
    /// the native object already exists, so retain its source-class lease and
    /// poison all subsequent admission until process teardown.
    #[cfg(any(target_os = "windows", test))]
    pub(super) fn mark_as_teardown_debt(&mut self) -> Result<(), NativeResourceAdmissionError> {
        let result = self.reclassify(NativeResourceClass::TeardownDebt);
        if result.is_err() {
            self.shared.fail();
        }
        result
    }

    fn release_once(&mut self) -> Result<(), ()> {
        let Some(class) = self.class.take() else {
            self.shared.fail();
            return Err(());
        };
        self.shared.try_release(class)
    }
}

impl Drop for NativeResourceLease {
    fn drop(&mut self) {
        if self.class.is_some() {
            let _ = self.release_once();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_budgets_are_disjoint_and_sum_to_the_hard_ceiling() {
        assert_eq!(NativeResourceClass::Tab.limit(), 32);
        assert_eq!(NativeResourceClass::WarmSpare.limit(), 1);
        assert_eq!(NativeResourceClass::TeardownDebt.limit(), 8);
        #[cfg(feature = "agentic-browser")]
        assert_eq!(
            NativeResourceClass::AgentContext.limit(),
            MAX_AGENT_CONTEXT_RESOURCES
        );
        assert_eq!(NativeResourceClass::TransientConstruction.limit(), 2);
        assert_eq!(
            NativeResourceClass::ALL
                .into_iter()
                .map(NativeResourceClass::limit)
                .sum::<usize>(),
            MAX_NATIVE_VIEW_RESOURCES
        );
    }

    #[test]
    fn every_class_refuses_its_first_over_limit_admission() {
        for class in NativeResourceClass::ALL {
            let ledger = NativeResourceLedger::default();
            let leases = (0..class.limit())
                .map(|_| ledger.try_acquire(class).expect("within class budget"))
                .collect::<Vec<_>>();
            assert!(matches!(
                ledger.try_acquire(class),
                Err(NativeResourceAdmissionError::ClassExhausted(actual)) if actual == class
            ));
            assert_eq!(ledger.count_for_audit(class), Some(class.limit()));
            drop(leases);
            assert!(ledger.is_empty());
        }
    }

    #[test]
    fn all_policy_pools_together_stop_at_the_global_ceiling() {
        let ledger = NativeResourceLedger::default();
        let mut leases = Vec::new();
        for class in NativeResourceClass::ALL {
            for _ in 0..class.limit() {
                leases.push(ledger.try_acquire(class).expect("declared global budget"));
            }
        }
        assert_eq!(ledger.total(), Some(MAX_NATIVE_VIEW_RESOURCES));
        assert_eq!(
            ledger.try_acquire(NativeResourceClass::Tab).err(),
            Some(NativeResourceAdmissionError::GlobalExhausted)
        );
        drop(leases);
        assert!(ledger.is_empty());
    }

    #[test]
    fn failed_admission_never_changes_owned_capacity() {
        let ledger = NativeResourceLedger::default();
        let spare = ledger
            .try_acquire(NativeResourceClass::WarmSpare)
            .expect("one warm spare");
        assert!(ledger.try_acquire(NativeResourceClass::WarmSpare).is_err());
        assert_eq!(ledger.total(), Some(1));
        assert_eq!(
            ledger.count_for_audit(NativeResourceClass::WarmSpare),
            Some(1)
        );
        drop(spare);
        assert!(ledger.is_empty());
    }

    #[test]
    fn reclassification_is_atomic_and_cannot_borrow_another_pool() {
        let ledger = NativeResourceLedger::default();
        let _spare = ledger
            .try_acquire(NativeResourceClass::WarmSpare)
            .expect("warm spare");
        let mut transient = ledger
            .try_acquire(NativeResourceClass::TransientConstruction)
            .expect("construction");
        assert_eq!(
            transient.reclassify(NativeResourceClass::WarmSpare),
            Err(NativeResourceAdmissionError::ClassExhausted(
                NativeResourceClass::WarmSpare
            ))
        );
        assert_eq!(
            transient.class(),
            Some(NativeResourceClass::TransientConstruction)
        );
        assert_eq!(
            ledger.count_for_audit(NativeResourceClass::WarmSpare),
            Some(1)
        );
        assert_eq!(
            ledger.count_for_audit(NativeResourceClass::TransientConstruction),
            Some(1)
        );
    }

    #[test]
    fn construction_can_transfer_to_tab_then_teardown_debt() {
        let ledger = NativeResourceLedger::default();
        let mut lease = ledger
            .try_acquire(NativeResourceClass::TransientConstruction)
            .expect("construction");
        lease
            .reclassify(NativeResourceClass::Tab)
            .expect("constructed tab");
        lease
            .reclassify(NativeResourceClass::TeardownDebt)
            .expect("failed close debt");
        assert_eq!(ledger.total(), Some(1));
        assert_eq!(ledger.count_for_audit(NativeResourceClass::Tab), Some(0));
        assert_eq!(
            ledger.count_for_audit(NativeResourceClass::TeardownDebt),
            Some(1)
        );
        drop(lease);
        assert!(ledger.is_empty());
    }

    #[test]
    fn refused_view_transfers_retain_their_source_ownership() {
        let ledger = NativeResourceLedger::default();
        let tabs = (0..NativeResourceClass::Tab.limit())
            .map(|_| {
                ledger
                    .try_acquire(NativeResourceClass::Tab)
                    .expect("tab budget")
            })
            .collect::<Vec<_>>();
        let mut transient = ledger
            .try_acquire(NativeResourceClass::TransientConstruction)
            .expect("construction");
        let mut spare = ledger
            .try_acquire(NativeResourceClass::WarmSpare)
            .expect("warm spare");

        assert_eq!(
            transient.reclassify(NativeResourceClass::Tab),
            Err(NativeResourceAdmissionError::ClassExhausted(
                NativeResourceClass::Tab
            ))
        );
        assert_eq!(
            spare.reclassify(NativeResourceClass::Tab),
            Err(NativeResourceAdmissionError::ClassExhausted(
                NativeResourceClass::Tab
            ))
        );
        assert_eq!(
            transient.class(),
            Some(NativeResourceClass::TransientConstruction)
        );
        assert_eq!(spare.class(), Some(NativeResourceClass::WarmSpare));
        assert_eq!(
            ledger.count_for_audit(NativeResourceClass::Tab),
            Some(NativeResourceClass::Tab.limit())
        );
        assert_eq!(
            ledger.count_for_audit(NativeResourceClass::TransientConstruction),
            Some(1)
        );
        assert_eq!(
            ledger.count_for_audit(NativeResourceClass::WarmSpare),
            Some(1)
        );
        assert!(ledger.is_healthy());

        drop(transient);
        drop(spare);
        drop(tabs);
        assert!(ledger.is_empty());
    }

    #[test]
    fn full_teardown_pool_never_releases_a_failed_view_early() {
        for source in [NativeResourceClass::Tab, NativeResourceClass::WarmSpare] {
            let ledger = NativeResourceLedger::default();
            let debts = (0..NativeResourceClass::TeardownDebt.limit())
                .map(|_| {
                    ledger
                        .try_acquire(NativeResourceClass::TeardownDebt)
                        .expect("teardown budget")
                })
                .collect::<Vec<_>>();
            let mut view = ledger.try_acquire(source).expect("view");

            assert_eq!(
                view.mark_as_teardown_debt(),
                Err(NativeResourceAdmissionError::ClassExhausted(
                    NativeResourceClass::TeardownDebt
                ))
            );
            assert_eq!(view.class(), Some(source));
            assert_eq!(ledger.count_for_audit(source), Some(1));
            assert_eq!(
                ledger.count_for_audit(NativeResourceClass::TeardownDebt),
                Some(NativeResourceClass::TeardownDebt.limit())
            );
            assert_eq!(ledger.total(), Some(debts.len() + 1));
            assert!(!ledger.is_healthy());
            assert!(!ledger.is_empty());
            assert!(!ledger.is_quiescent());
            assert_eq!(
                ledger
                    .try_acquire(NativeResourceClass::TransientConstruction)
                    .err(),
                Some(NativeResourceAdmissionError::AccountingInvariant)
            );
        }
    }

    #[test]
    fn explicit_double_release_poisoning_never_creates_capacity() {
        let ledger = NativeResourceLedger::default();
        let mut lease = ledger
            .try_acquire(NativeResourceClass::WarmSpare)
            .expect("warm spare");
        assert_eq!(lease.release_once(), Ok(()));
        assert_eq!(ledger.total(), Some(0));
        assert_eq!(lease.release_once(), Err(()));
        assert_eq!(ledger.total(), Some(0));
        assert!(!ledger.is_healthy());
        assert_eq!(
            ledger.try_acquire(NativeResourceClass::WarmSpare).err(),
            Some(NativeResourceAdmissionError::AccountingInvariant)
        );
    }

    #[test]
    fn reentrant_release_fails_closed_without_panicking_or_reissuing_capacity() {
        let ledger = NativeResourceLedger::default();
        let lease = ledger
            .try_acquire(NativeResourceClass::WarmSpare)
            .expect("warm spare");
        let state_borrow = ledger.shared.state.borrow_mut();
        drop(lease);
        assert!(ledger.shared.invariant_failed.get());
        assert_eq!(state_borrow.total, 1);
        drop(state_borrow);
        assert_eq!(ledger.total(), Some(1));
        assert_eq!(
            ledger.try_acquire(NativeResourceClass::WarmSpare).err(),
            Some(NativeResourceAdmissionError::AccountingInvariant)
        );
    }

    #[test]
    fn a_late_raii_release_survives_the_ledger_handle() {
        let ledger = NativeResourceLedger::default();
        let shared = Rc::downgrade(&ledger.shared);
        let lease = ledger
            .try_acquire(NativeResourceClass::TeardownDebt)
            .expect("debt");
        drop(ledger);
        assert!(shared.upgrade().is_some());
        drop(lease);
        assert!(shared.upgrade().is_none());
    }

    #[test]
    fn forgotten_ownership_cannot_be_reissued() {
        let ledger = NativeResourceLedger::default();
        let lease = ledger
            .try_acquire(NativeResourceClass::WarmSpare)
            .expect("warm spare");
        std::mem::forget(lease);
        assert_eq!(ledger.total(), Some(1));
        assert!(matches!(
            ledger.try_acquire(NativeResourceClass::WarmSpare),
            Err(NativeResourceAdmissionError::ClassExhausted(
                NativeResourceClass::WarmSpare
            ))
        ));
    }

    #[test]
    fn observed_webview_drops_before_its_raii_lease() {
        let host = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/host/mod.rs"));
        let observed_view = host
            .split_once("struct ObservedView {")
            .expect("ObservedView declaration")
            .1
            .split_once("\n}")
            .expect("ObservedView declaration end")
            .0;
        let native_view = observed_view
            .find("\n    view: WebView,")
            .expect("native view");
        let lease = observed_view
            .find("\n    native_resource: Option<NativeResourceLease>,")
            .expect("native resource lease");
        assert!(
            native_view < lease,
            "Rust drops fields in declaration order; the native view must drop before its lease"
        );
    }
}
