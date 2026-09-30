//! UI-thread-owned pause signal shared by overlapping registration cohorts.

#[cfg(any(not(target_os = "windows"), test))]
use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;
#[cfg(any(not(target_os = "windows"), test))]
use std::rc::Weak;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

type ApplyPause = Rc<dyn Fn(bool)>;

#[derive(Clone, Default)]
pub(crate) struct ContentPause(Rc<Inner>);

#[derive(Default)]
struct Inner {
    paused: Arc<AtomicBool>,
    callbacks: RefCell<Vec<ApplyPause>>,
}

#[cfg(any(not(target_os = "windows"), test))]
pub(crate) struct PauseRegistration {
    owner: Weak<Inner>,
    callback: ApplyPause,
    alive: Rc<Cell<bool>>,
}

impl ContentPause {
    #[cfg(any(target_os = "windows", test))]
    pub(crate) fn signal(&self) -> Arc<AtomicBool> {
        self.0.paused.clone()
    }
    pub(crate) fn paused(&self) -> bool {
        self.0.paused.load(Ordering::Relaxed)
    }

    pub(crate) fn set(&self, paused: bool) {
        if self.0.paused.swap(paused, Ordering::Relaxed) == paused {
            return;
        }
        // No RefCell borrow is held through a native mutation/re-entry.
        let callbacks = self.0.callbacks.borrow().clone();
        for callback in callbacks {
            callback(self.paused());
        }
    }

    #[cfg(any(not(target_os = "windows"), test))]
    pub(crate) fn register(&self, callback: impl Fn(bool) + 'static) -> Option<PauseRegistration> {
        let alive = Rc::new(Cell::new(true));
        let active = alive.clone();
        let callback: ApplyPause = Rc::new(move |paused| {
            if active.get() {
                callback(paused);
            }
        });
        {
            let mut callbacks = self.0.callbacks.borrow_mut();
            if callbacks.len() >= 4 {
                return None;
            }
            callbacks.push(callback.clone());
        }
        callback(self.paused());
        Some(PauseRegistration {
            owner: Rc::downgrade(&self.0),
            callback,
            alive,
        })
    }
}

#[cfg(any(not(target_os = "windows"), test))]
impl Drop for PauseRegistration {
    fn drop(&mut self) {
        self.alive.set(false);
        if let Some(owner) = self.owner.upgrade() {
            owner
                .callbacks
                .borrow_mut()
                .retain(|entry| !Rc::ptr_eq(entry, &self.callback));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlapping_cohorts_share_pause_and_retired_callbacks_disappear() {
        let pause = ContentPause::default();
        let signal = pause.signal();
        let changes = Rc::new(RefCell::new(Vec::new()));
        let first_changes = changes.clone();
        let first = pause
            .register(move |paused| first_changes.borrow_mut().push((1, paused)))
            .unwrap();
        let second_changes = changes.clone();
        let second = pause
            .register(move |paused| second_changes.borrow_mut().push((2, paused)))
            .unwrap();
        let cosmetic_old = pause.register(|_| {}).unwrap();
        let cosmetic_new = pause.register(|_| {}).unwrap();
        assert!(pause.register(|_| {}).is_none());
        drop(cosmetic_old);
        drop(cosmetic_new);
        pause.set(true);
        assert!(signal.load(Ordering::Relaxed));
        drop(first);
        pause.set(false);
        assert!(!signal.load(Ordering::Relaxed));
        assert_eq!(
            *changes.borrow(),
            vec![(1, false), (2, false), (1, true), (2, true), (2, false)]
        );
        drop(second);
        assert!(pause.0.callbacks.borrow().is_empty());
    }
}
