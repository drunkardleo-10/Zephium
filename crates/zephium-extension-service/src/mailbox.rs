use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, MutexGuard};

/// Maximum number of ordinary extension-service commands retained at once.
pub const EXTENSION_SERVICE_NORMAL_CAPACITY: usize = 1024;

/// Complete physical mailbox bound, including the reserved shutdown slot.
pub const EXTENSION_SERVICE_MAILBOX_CAPACITY: usize = EXTENSION_SERVICE_NORMAL_CAPACITY + 1;

const _: () = assert!(EXTENSION_SERVICE_MAILBOX_CAPACITY == 1025);

enum Entry<T> {
    #[allow(dead_code)]
    Normal(T),
    Shutdown,
}

struct State<T> {
    entries: VecDeque<Entry<T>>,
    shutdown_enqueued: bool,
    closed: bool,
    accepted_normal: u64,
}

pub(crate) struct Mailbox<T> {
    state: Mutex<State<T>>,
    ready: Condvar,
}

#[allow(dead_code)]
pub(crate) enum NormalAdmission<T> {
    Accepted,
    Full(T),
    Sealed(T),
    Closed(T),
    CounterExhausted(T),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ShutdownAdmission {
    Accepted,
    AlreadyEnqueued,
    Closed,
}

pub(crate) enum Delivery<T> {
    Normal(T),
    Shutdown { accepted_normal: u64 },
    Closed,
}

impl<T> Mailbox<T> {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(State {
                entries: VecDeque::with_capacity(EXTENSION_SERVICE_MAILBOX_CAPACITY),
                shutdown_enqueued: false,
                closed: false,
                accepted_normal: 0,
            }),
            ready: Condvar::new(),
        }
    }

    // The typed operation surface lands after repository recovery. Keeping
    // admission private until then prevents raw closures or repository inputs
    // from becoming a provisional public API.
    #[allow(dead_code)]
    pub(crate) fn try_push_normal(&self, value: T) -> NormalAdmission<T> {
        let mut state = self.lock();
        if state.closed {
            return NormalAdmission::Closed(value);
        }
        if state.shutdown_enqueued {
            return NormalAdmission::Sealed(value);
        }
        if state.entries.len() >= EXTENSION_SERVICE_NORMAL_CAPACITY {
            return NormalAdmission::Full(value);
        }
        let Some(accepted_normal) = state.accepted_normal.checked_add(1) else {
            state.shutdown_enqueued = true;
            state.entries.push_back(Entry::Shutdown);
            debug_assert!(state.entries.len() <= EXTENSION_SERVICE_MAILBOX_CAPACITY);
            self.ready.notify_one();
            return NormalAdmission::CounterExhausted(value);
        };
        state.accepted_normal = accepted_normal;
        state.entries.push_back(Entry::Normal(value));
        debug_assert!(state.entries.len() <= EXTENSION_SERVICE_NORMAL_CAPACITY);
        self.ready.notify_one();
        NormalAdmission::Accepted
    }

    pub(crate) fn try_push_shutdown(&self) -> ShutdownAdmission {
        let mut state = self.lock();
        if state.closed {
            return ShutdownAdmission::Closed;
        }
        if state.shutdown_enqueued {
            return ShutdownAdmission::AlreadyEnqueued;
        }
        state.shutdown_enqueued = true;
        state.entries.push_back(Entry::Shutdown);
        debug_assert!(state.entries.len() <= EXTENSION_SERVICE_MAILBOX_CAPACITY);
        self.ready.notify_one();
        ShutdownAdmission::Accepted
    }

    pub(crate) fn receive(&self) -> Delivery<T> {
        let mut state = self.lock();
        loop {
            if let Some(entry) = state.entries.pop_front() {
                return match entry {
                    Entry::Normal(value) => Delivery::Normal(value),
                    Entry::Shutdown => Delivery::Shutdown {
                        accepted_normal: state.accepted_normal,
                    },
                };
            }
            if state.closed {
                return Delivery::Closed;
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    pub(crate) fn close(&self) {
        let mut state = self.lock();
        state.closed = true;
        state.entries.clear();
        self.ready.notify_all();
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.lock().entries.len()
    }

    #[cfg(test)]
    pub(crate) fn exhaust_normal_counter_for_test(&self) {
        let mut state = self.lock();
        debug_assert!(!state.closed);
        debug_assert!(!state.shutdown_enqueued);
        debug_assert!(state.entries.is_empty());
        state.accepted_normal = u64::MAX;
    }

    fn lock(&self) -> MutexGuard<'_, State<T>> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_normal_fifo_retains_the_reserved_shutdown_slot() {
        let mailbox = Mailbox::new();
        for value in 0..EXTENSION_SERVICE_NORMAL_CAPACITY {
            assert!(matches!(
                mailbox.try_push_normal(value),
                NormalAdmission::Accepted
            ));
        }
        assert!(matches!(
            mailbox.try_push_normal(usize::MAX),
            NormalAdmission::Full(usize::MAX)
        ));
        assert_eq!(mailbox.try_push_shutdown(), ShutdownAdmission::Accepted);
        assert_eq!(mailbox.len(), EXTENSION_SERVICE_MAILBOX_CAPACITY);
        assert!(matches!(
            mailbox.try_push_normal(usize::MAX),
            NormalAdmission::Sealed(usize::MAX)
        ));
        assert_eq!(
            mailbox.try_push_shutdown(),
            ShutdownAdmission::AlreadyEnqueued
        );

        for expected in 0..EXTENSION_SERVICE_NORMAL_CAPACITY {
            assert!(matches!(mailbox.receive(), Delivery::Normal(value) if value == expected));
        }
        assert!(matches!(
            mailbox.receive(),
            Delivery::Shutdown { accepted_normal }
                if accepted_normal == EXTENSION_SERVICE_NORMAL_CAPACITY as u64
        ));
    }

    #[test]
    fn close_is_terminal_and_discards_unconsumed_work() {
        let mailbox = Mailbox::new();
        assert!(matches!(
            mailbox.try_push_normal(7),
            NormalAdmission::Accepted
        ));
        mailbox.close();
        assert_eq!(mailbox.len(), 0);
        assert!(matches!(
            mailbox.try_push_normal(8),
            NormalAdmission::Closed(8)
        ));
        assert_eq!(mailbox.try_push_shutdown(), ShutdownAdmission::Closed);
        assert!(matches!(mailbox.receive(), Delivery::Closed));
    }
}
