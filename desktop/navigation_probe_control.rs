//! Linearization fence only, not a Work or native lifecycle.
use std::sync::Mutex;

#[derive(Default)]
pub struct AdmissionFence(Mutex<bool>);
impl AdmissionFence {
    pub fn cancel(&self) {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
    }
    pub fn cancelled(&self) -> bool {
        self.0.lock().map_or(true, |cancelled| *cancelled)
    }
    /// Lookup happens outside the fence. Cancellation and actual trusted
    /// admission compete at this one boundary; a late credential cannot admit.
    pub fn admit<T>(&self, admission: impl FnOnce() -> T) -> Option<T> {
        let cancelled = self.0.lock().ok()?;
        if *cancelled {
            None
        } else {
            Some(admission())
        }
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
    fn cancelled_lookup_completion_cannot_call_admission() {
        let fence = Arc::new(AdmissionFence::default());
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
        let fence = AdmissionFence::default();
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
        let fence = AdmissionFence::default();
        fence.cancel();
        assert!(fence.admit(move || input).is_none());
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn poisoned_admission_remains_closed() {
        let fence = AdmissionFence::default();
        assert!(std::panic::catch_unwind(|| fence.admit(|| panic!("admission"))).is_err());
        assert!(fence.cancelled());
        assert_eq!(fence.admit(|| 1), None);
        fence.cancel();
        assert_eq!(fence.admit(|| 2), None);
    }
}
