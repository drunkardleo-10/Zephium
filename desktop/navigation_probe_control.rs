//! Linearization fence only, not a Work or native lifecycle.
use std::sync::Mutex;

pub fn profile_wait_failure(
    started: std::time::Instant,
    now: std::time::Instant,
    cancelled: bool,
) -> Option<&'static str> {
    if cancelled {
        Some("profile_cancelled")
    } else if now.saturating_duration_since(started) >= std::time::Duration::from_secs(150) {
        Some("profile_deadline")
    } else {
        None
    }
}

pub struct AdmissionFence<T>(Mutex<FenceState<T>>);
struct FenceState<T> {
    cancelled: bool,
    terminal: Option<Result<T, &'static str>>,
}
impl<T> Default for AdmissionFence<T> {
    fn default() -> Self {
        Self(Mutex::new(FenceState {
            cancelled: false,
            terminal: None,
        }))
    }
}
impl<T: Copy> AdmissionFence<T> {
    /// True retains exit: cancellation won before terminal settlement. False
    /// observes immutable settlement and permits the original worker to join.
    pub fn cancel(&self) -> bool {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.terminal.is_some() {
            return false;
        }
        state.cancelled = true;
        true
    }
    pub fn cancelled(&self) -> bool {
        self.0.lock().map_or(true, |state| state.cancelled)
    }
    /// Lookup happens outside the fence. Cancellation and actual trusted
    /// admission compete at this one boundary; a late credential cannot admit.
    pub fn admit<U>(&self, admission: impl FnOnce() -> U) -> Option<U> {
        let state = self.0.lock().ok()?;
        if state.cancelled || state.terminal.is_some() {
            None
        } else {
            Some(admission())
        }
    }
    /// Called only after releasing credential/request/projection owners. The
    /// terminal itself is the readiness fact; publication and cancellation
    /// cannot straddle different locks or an independent atomic ready flag.
    pub fn settle(&self, result: Result<T, &'static str>) -> Option<Result<T, &'static str>> {
        let mut state = self.0.lock().unwrap_or_else(|poison| {
            let mut state = poison.into_inner();
            state.cancelled = true;
            state
        });
        if state.terminal.is_some() {
            return None;
        }
        let result = if state.cancelled {
            Err("cancelled_before_terminal")
        } else {
            result
        };
        state.terminal = Some(result);
        Some(result)
    }
    pub fn terminal(&self) -> Option<Result<T, &'static str>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .terminal
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };
    #[test]
    fn profile_wait_cannot_refresh_deadline_or_accept_after_cancellation() {
        let started = std::time::Instant::now();
        for seconds in [0, 1, 149] {
            assert_eq!(
                profile_wait_failure(
                    started,
                    started + std::time::Duration::from_secs(seconds),
                    false
                ),
                None
            );
        }
        for seconds in [150, 151, 300] {
            assert_eq!(
                profile_wait_failure(
                    started,
                    started + std::time::Duration::from_secs(seconds),
                    false
                ),
                Some("profile_deadline")
            );
        }
        assert_eq!(
            profile_wait_failure(started, started, true),
            Some("profile_cancelled")
        );
    }
    #[test]
    fn cancelled_lookup_completion_cannot_call_admission() {
        let fence = Arc::new(AdmissionFence::<()>::default());
        let loaded = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let calls = Arc::new(AtomicUsize::new(0));
        let worker = {
            let (fence, loaded, release, calls) = (
                fence.clone(),
                loaded.clone(),
                release.clone(),
                calls.clone(),
            );
            std::thread::spawn(move || {
                loaded.wait();
                release.wait();
                fence.admit(|| calls.fetch_add(1, Ordering::SeqCst))
            })
        };
        loaded.wait();
        fence.cancel();
        release.wait();
        assert!(worker.join().unwrap().is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(fence.cancelled());
    }
    #[test]
    fn original_admission_precedes_later_cancellation() {
        let fence = AdmissionFence::<()>::default();
        assert_eq!(fence.admit(|| 1), Some(1));
        fence.cancel();
        assert_eq!(fence.admit(|| 2), None);
    }
    #[test]
    fn refused_late_input_is_dropped_without_running_the_admission_closure() {
        struct Input(Arc<AtomicUsize>);
        impl Drop for Input {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let input = Input(drops.clone());
        let fence = AdmissionFence::<()>::default();
        fence.cancel();
        assert!(fence.admit(move || input).is_none());
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn poisoned_admission_remains_closed() {
        let fence = AdmissionFence::<()>::default();
        assert!(std::panic::catch_unwind(|| fence.admit(|| panic!("admission"))).is_err());
        assert!(fence.cancelled());
        assert_eq!(fence.admit(|| 1), None);
        fence.cancel();
        assert_eq!(fence.admit(|| 2), None);
        assert_eq!(fence.settle(Ok(())), Some(Err("cancelled_before_terminal")));
        assert_eq!(fence.terminal(), Some(Err("cancelled_before_terminal")));
        assert!(!fence.cancel());
    }
    #[test]
    fn cancellation_between_observer_success_and_publication_wins() {
        let fence = Arc::new(AdmissionFence::<bool>::default());
        let observed = Arc::new(Barrier::new(2));
        let publish = Arc::new(Barrier::new(2));
        let worker = {
            let (fence, observed, publish) = (fence.clone(), observed.clone(), publish.clone());
            std::thread::spawn(move || {
                let candidate = Ok(true);
                observed.wait();
                publish.wait();
                fence.settle(candidate)
            })
        };
        observed.wait();
        assert_eq!(fence.terminal(), None);
        assert!(fence.cancel(), "exit must retain the unsettled worker");
        publish.wait();
        assert_eq!(
            worker.join().unwrap(),
            Some(Err("cancelled_before_terminal"))
        );
        assert_eq!(fence.terminal(), Some(Err("cancelled_before_terminal")));
        assert!(
            !fence.cancel(),
            "settled failure permits joining and ordinary shutdown"
        );
        assert_eq!(fence.settle(Ok(true)), None);
    }
    #[test]
    fn settlement_first_is_immutable_and_never_reopens_admission() {
        for candidate in [Ok(true), Ok(false), Err("observer_failure")] {
            let fence = AdmissionFence::default();
            assert_eq!(fence.settle(candidate), Some(candidate));
            assert!(!fence.cancel());
            assert_eq!(fence.terminal(), Some(candidate));
            assert_eq!(fence.admit(|| 1), None);
            assert_eq!(fence.settle(Ok(true)), None);
            assert_eq!(fence.terminal(), Some(candidate));
        }
    }
}
