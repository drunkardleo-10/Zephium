//! Single-flight admission and cancellation for native probes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

use thiserror::Error;

/// Refusal returned by the bounded single-flight controller.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ProbeAdmissionError {
    /// Request identifiers must be non-zero.
    #[error("probe request identifiers must be non-zero")]
    ZeroRequestId,
    /// Exactly one probe run may own native input at a time.
    #[error("a probe run already owns the native-input lease")]
    Busy,
}

#[derive(Debug)]
struct ActiveRun {
    request_id: u64,
    generation: u64,
    cancelled: Arc<AtomicBool>,
}

#[derive(Debug, Default)]
struct GateState {
    next_generation: u64,
    active: Option<ActiveRun>,
}

/// Bounded controller admitting at most one native-input run.
#[derive(Clone, Debug, Default)]
pub struct ProbeGate {
    state: Arc<Mutex<GateState>>,
}

impl ProbeGate {
    /// Creates an idle gate with no worker, timer, or background activity.
    pub fn new() -> Self {
        Self::default()
    }

    /// Attempts to acquire the sole native-input run lease.
    pub fn try_start(&self, request_id: u64) -> Result<ProbeRunPermit, ProbeAdmissionError> {
        if request_id == 0 {
            return Err(ProbeAdmissionError::ZeroRequestId);
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.active.is_some() {
            return Err(ProbeAdmissionError::Busy);
        }
        state.next_generation = state.next_generation.wrapping_add(1).max(1);
        let generation = state.next_generation;
        let cancelled = Arc::new(AtomicBool::new(false));
        state.active = Some(ActiveRun {
            request_id,
            generation,
            cancelled: Arc::clone(&cancelled),
        });
        Ok(ProbeRunPermit {
            owner: Arc::downgrade(&self.state),
            request_id,
            generation,
            cancelled,
        })
    }

    /// Cancels the exact active request, returning false for stale or absent ids.
    pub fn cancel(&self, request_id: u64) -> bool {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(active) = &state.active else {
            return false;
        };
        if active.request_id != request_id {
            return false;
        }
        active.cancelled.store(true, Ordering::Release);
        true
    }

    /// Returns whether a run currently owns the gate.
    pub fn is_busy(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active
            .is_some()
    }
}

/// Non-cloneable ownership and cancellation token for one admitted run.
#[derive(Debug)]
pub struct ProbeRunPermit {
    owner: Weak<Mutex<GateState>>,
    request_id: u64,
    generation: u64,
    cancelled: Arc<AtomicBool>,
}

impl ProbeRunPermit {
    /// Exact request identifier owning this permit.
    pub fn request_id(&self) -> u64 {
        self.request_id
    }

    /// Returns true once cancellation was requested for this exact generation.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

impl Drop for ProbeRunPermit {
    fn drop(&mut self) {
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        let mut state = owner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state
            .active
            .as_ref()
            .is_some_and(|active| active.generation == self.generation)
        {
            state.active = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_permit_is_single_flight_and_releases_on_drop() {
        let gate = ProbeGate::new();
        let first = gate.try_start(7).expect("first run");
        assert_eq!(gate.try_start(8).unwrap_err(), ProbeAdmissionError::Busy);
        assert!(gate.is_busy());
        drop(first);
        assert_eq!(gate.try_start(8).expect("second run").request_id(), 8);
    }

    #[test]
    fn cancellation_is_exact_and_non_reusable() {
        let gate = ProbeGate::new();
        let first = gate.try_start(17).expect("first run");
        assert!(!gate.cancel(18));
        assert!(!first.is_cancelled());
        assert!(gate.cancel(17));
        assert!(first.is_cancelled());
        drop(first);

        let second = gate.try_start(17).expect("reused external id");
        assert!(!second.is_cancelled());
    }

    #[test]
    fn zero_request_id_is_rejected() {
        assert_eq!(
            ProbeGate::new().try_start(0).unwrap_err(),
            ProbeAdmissionError::ZeroRequestId
        );
    }
}
