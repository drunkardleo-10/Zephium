//! Native chrome protection follows actual observation rendering, on its STA.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

type BackingCallback = Rc<dyn Fn(bool) -> bool>;

struct Registration {
    count: Cell<usize>,
    callback: RefCell<Option<BackingCallback>>,
}

thread_local! {
    static REGISTRATIONS: RefCell<HashMap<usize, Rc<Registration>>> = RefCell::new(HashMap::new());
}

/// Install on the native UI thread before admitting Work views. The callback
/// completes synchronously, before any observation child is shown.
pub fn install_work_rendering_backing(
    parent: usize,
    callback: impl Fn(bool) -> bool + 'static,
) -> bool {
    REGISTRATIONS.with(|registrations| {
        let mut registrations = registrations.borrow_mut();
        if registrations.contains_key(&parent) {
            return false;
        }
        registrations.insert(
            parent,
            Rc::new(Registration {
                count: Cell::new(0),
                callback: RefCell::new(Some(Rc::new(callback))),
            }),
        );
        true
    })
}

/// Remove on that same UI thread after the native parent is destroyed. Existing
/// leases become inert; none can address a replacement window with a reused HWND.
pub fn remove_work_rendering_backing(parent: usize) {
    let registration =
        REGISTRATIONS.with(|registrations| registrations.borrow_mut().remove(&parent));
    if let Some(registration) = registration {
        registration.callback.borrow_mut().take();
    }
}

pub(crate) struct WorkRenderingLease(Option<Rc<Registration>>);

impl WorkRenderingLease {
    pub(crate) fn acquire(parent: usize) -> Option<Self> {
        let registration =
            REGISTRATIONS.with(|registrations| registrations.borrow().get(&parent).cloned());
        // Standalone native qualifiers may have no privileged chrome to protect.
        let Some(registration) = registration else {
            return Some(Self(None));
        };
        let count = registration.count.get().checked_add(1)?;
        registration.count.set(count);
        // Native calls can synchronously dispatch destruction. Never retain a
        // registry/entry borrow while invoking the desktop callback.
        let callback = registration.callback.borrow().clone();
        if count == 1 && !callback.as_ref().is_none_or(|callback| callback(true)) {
            registration.count.set(registration.count.get() - 1);
            return None;
        }
        Some(Self(Some(registration)))
    }
}

impl Drop for WorkRenderingLease {
    fn drop(&mut self) {
        let Some(registration) = self.0.take() else {
            return;
        };
        let remaining = registration.count.get() - 1;
        registration.count.set(remaining);
        if remaining == 0 {
            let callback = registration.callback.borrow().clone();
            if let Some(callback) = callback {
                let _ = callback(false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_pages_and_replacement_presentations_share_one_backing() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let sink = events.clone();
        assert!(install_work_rendering_backing(1, move |active| {
            sink.borrow_mut().push(active);
            true
        }));
        assert!(events.borrow().is_empty());
        let first = WorkRenderingLease::acquire(1).unwrap();
        let second = WorkRenderingLease::acquire(1).unwrap();
        drop(first);
        assert_eq!(*events.borrow(), [true]);
        drop(second);
        assert_eq!(*events.borrow(), [true, false]);
        remove_work_rendering_backing(1);
    }

    #[test]
    fn refused_backing_does_not_admit_rendering_or_restore_a_color_it_never_owned() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let sink = events.clone();
        assert!(install_work_rendering_backing(2, move |active| {
            sink.borrow_mut().push(active);
            false
        }));
        assert!(WorkRenderingLease::acquire(2).is_none());
        assert_eq!(*events.borrow(), [true]);
        remove_work_rendering_backing(2);
    }

    #[test]
    fn capture_debt_keeps_backing_after_presenter_retires() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let sink = events.clone();
        assert!(install_work_rendering_backing(4, move |active| {
            sink.borrow_mut().push(active);
            true
        }));
        let rendering_owner = Rc::new(WorkRenderingLease::acquire(4).unwrap());
        let original_capture_guard = rendering_owner.clone();
        drop(rendering_owner);
        assert_eq!(*events.borrow(), [true]);
        drop(original_capture_guard);
        assert_eq!(*events.borrow(), [true, false]);
        remove_work_rendering_backing(4);
    }

    #[test]
    fn destroyed_parent_detaches_old_leases_from_reused_handle() {
        assert!(install_work_rendering_backing(3, |_| true));
        let old = WorkRenderingLease::acquire(3).unwrap();
        remove_work_rendering_backing(3);
        let events = Rc::new(RefCell::new(Vec::new()));
        let sink = events.clone();
        assert!(install_work_rendering_backing(3, move |active| {
            sink.borrow_mut().push(active);
            true
        }));
        drop(old);
        assert!(events.borrow().is_empty());
        remove_work_rendering_backing(3);
    }

    #[test]
    fn synchronous_native_destruction_can_detach_during_activation() {
        assert!(install_work_rendering_backing(5, |active| {
            if active {
                remove_work_rendering_backing(5);
            }
            true
        }));
        let lease = WorkRenderingLease::acquire(5).unwrap();
        drop(lease);
        assert!(install_work_rendering_backing(5, |_| true));
        remove_work_rendering_backing(5);
    }
}
