//! Original notification epoch for a caller-blocking shutdown barrier. No
//! command receiver, native port, worker, or independent execution authority.

use super::Refusal;
use std::sync::{Condvar, Mutex};
use std::time::Instant;

pub(super) struct NotificationEpoch {
    generation: Mutex<Option<u64>>,
    changed: Condvar,
}

impl Default for NotificationEpoch {
    fn default() -> Self {
        Self {
            generation: Mutex::new(Some(0)),
            changed: Condvar::new(),
        }
    }
}
impl NotificationEpoch {
    /// Called after original callback state publication, even when the Shell
    /// wake is coalesced. Exhaustion/poison cannot be mistaken for quietness.
    pub(super) fn publish(&self) -> bool {
        let current = match self.generation.lock() {
            Ok(mut generation) => {
                *generation = generation.and_then(|value| value.checked_add(1));
                generation.is_some()
            }
            Err(_) => false,
        };
        self.changed.notify_all();
        current
    }

    pub(super) fn snapshot(&self) -> Result<u64, Refusal> {
        self.generation
            .lock()
            .map_err(|_| Refusal::Uncertain)?
            .ok_or(Refusal::Uncertain)
    }

    /// The predicate and publication share one mutex, so a callback between
    /// application poll and wait is retained. Spurious wakes never trigger an
    /// application poll or extend the original absolute wait deadline.
    pub(super) fn wait_until_changed(
        &self,
        observed: u64,
        deadline: Instant,
    ) -> Result<(), Refusal> {
        let mut generation = self.generation.lock().map_err(|_| Refusal::Uncertain)?;
        loop {
            let current = generation.ok_or(Refusal::Uncertain)?;
            if current != observed {
                return Ok(());
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Ok(());
            };
            generation = self
                .changed
                .wait_timeout(generation, remaining)
                .map_err(|_| Refusal::Uncertain)?
                .0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    #[test]
    fn publication_between_poll_and_wait_is_not_lost() {
        let epoch = NotificationEpoch::default();
        let observed = epoch.snapshot().unwrap();
        assert!(epoch.publish());
        let start = Instant::now();
        epoch
            .wait_until_changed(observed, start + Duration::from_secs(2))
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn coalesced_shell_wakes_still_advance_the_original_wait_epoch() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let notifications = super::super::Notifications {
            pending: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            wake: Arc::new(move || {
                called.fetch_add(1, Ordering::AcqRel);
                true
            }),
            actors: Mutex::new(Vec::new()),
            epoch: NotificationEpoch::default(),
        };
        let before = notifications.epoch.snapshot().unwrap();
        assert!(notifications.publish());
        assert!(notifications.publish());
        assert_eq!(calls.load(Ordering::Acquire), 1);
        assert_eq!(notifications.epoch.snapshot().unwrap(), before + 2);
    }

    #[test]
    fn spurious_notification_does_not_release_the_original_epoch_predicate() {
        let epoch = Arc::new(NotificationEpoch::default());
        let observed = epoch.snapshot().unwrap();
        let (started, starting) = mpsc::channel();
        let (finished, finishing) = mpsc::channel();
        let waiting = epoch.clone();
        let worker = std::thread::spawn(move || {
            started.send(()).unwrap();
            finished
                .send(waiting.wait_until_changed(observed, Instant::now() + Duration::from_secs(2)))
                .unwrap();
        });
        starting.recv().unwrap();
        epoch.changed.notify_all();
        assert!(finishing.recv_timeout(Duration::from_millis(20)).is_err());
        assert!(epoch.publish());
        assert!(finishing
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .is_ok());
        worker.join().unwrap();
    }

    #[test]
    fn quiet_epoch_wait_uses_the_original_absolute_deadline() {
        let epoch = NotificationEpoch::default();
        let deadline = Instant::now() + Duration::from_millis(20);
        epoch
            .wait_until_changed(epoch.snapshot().unwrap(), deadline)
            .unwrap();
        assert!(Instant::now() >= deadline);
    }

    #[test]
    fn exhausted_or_poisoned_epoch_is_uncertain_not_quiet() {
        let epoch = NotificationEpoch::default();
        *epoch.generation.lock().unwrap() = Some(u64::MAX);
        assert!(!epoch.publish());
        assert!(epoch.snapshot().is_err());
        assert!(epoch.wait_until_changed(u64::MAX, Instant::now()).is_err());
        let epoch = NotificationEpoch::default();
        let _ = std::panic::catch_unwind(|| {
            let _guard = epoch.generation.lock().unwrap();
            panic!("original epoch poison fixture");
        });
        assert!(!epoch.publish());
        assert!(epoch.snapshot().is_err());
        assert!(epoch.wait_until_changed(0, Instant::now()).is_err());
    }
}
