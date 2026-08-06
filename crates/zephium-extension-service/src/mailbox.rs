use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, MutexGuard};

/// Maximum number of ordinary extension-service commands retained at once.
pub const EXTENSION_SERVICE_NORMAL_CAPACITY: usize = 1024;

/// Complete physical mailbox bound: ordinary work, one profile-retirement
/// barrier, and the independently reserved shutdown slot.
pub const EXTENSION_SERVICE_MAILBOX_CAPACITY: usize = EXTENSION_SERVICE_NORMAL_CAPACITY + 2;

const _: () = assert!(EXTENSION_SERVICE_MAILBOX_CAPACITY == 1026);

enum Entry<T> {
    #[allow(dead_code)]
    Normal(T),
    #[allow(dead_code)]
    Barrier(T),
    Shutdown,
}

struct State<T> {
    entries: VecDeque<Entry<T>>,
    ordinary_queued: usize,
    barrier_enqueued: bool,
    shutdown_enqueued: bool,
    closed: bool,
    // Both ordinary and barrier commands enter the same worker completion
    // counter. Shutdown evidence therefore covers every accepted command
    // without granting the barrier ordinary-capacity admission.
    accepted_commands: u64,
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

#[allow(dead_code)]
pub(crate) enum BarrierAdmission<T> {
    Accepted,
    Occupied(T),
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
    Shutdown { accepted_commands: u64 },
    Closed,
}

impl<T> Mailbox<T> {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(State {
                entries: VecDeque::with_capacity(EXTENSION_SERVICE_MAILBOX_CAPACITY),
                ordinary_queued: 0,
                barrier_enqueued: false,
                shutdown_enqueued: false,
                closed: false,
                accepted_commands: 0,
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
        if state.ordinary_queued >= EXTENSION_SERVICE_NORMAL_CAPACITY {
            return NormalAdmission::Full(value);
        }
        let Some(accepted_commands) = state.accepted_commands.checked_add(1) else {
            state.shutdown_enqueued = true;
            state.entries.push_back(Entry::Shutdown);
            debug_assert!(state.entries.len() <= EXTENSION_SERVICE_MAILBOX_CAPACITY);
            self.ready.notify_one();
            return NormalAdmission::CounterExhausted(value);
        };
        state.accepted_commands = accepted_commands;
        state.ordinary_queued += 1;
        state.entries.push_back(Entry::Normal(value));
        debug_assert!(state.ordinary_queued <= EXTENSION_SERVICE_NORMAL_CAPACITY);
        debug_assert!(state.entries.len() < EXTENSION_SERVICE_MAILBOX_CAPACITY);
        self.ready.notify_one();
        NormalAdmission::Accepted
    }

    /// Admits one FIFO-ordered profile-retirement barrier independently of
    /// ordinary saturation.
    ///
    /// The barrier does not seal ordinary admission: work admitted afterward
    /// remains behind it in the same FIFO and must observe the retirement
    /// fence installed by the worker. Only one barrier may be queued at once,
    /// while shutdown always retains its own final physical slot.
    #[allow(dead_code)]
    pub(crate) fn try_push_barrier(&self, value: T) -> BarrierAdmission<T> {
        let mut state = self.lock();
        if state.closed {
            return BarrierAdmission::Closed(value);
        }
        if state.shutdown_enqueued {
            return BarrierAdmission::Sealed(value);
        }
        if state.barrier_enqueued {
            return BarrierAdmission::Occupied(value);
        }
        let Some(accepted_commands) = state.accepted_commands.checked_add(1) else {
            state.shutdown_enqueued = true;
            state.entries.push_back(Entry::Shutdown);
            debug_assert!(state.entries.len() <= EXTENSION_SERVICE_MAILBOX_CAPACITY);
            self.ready.notify_one();
            return BarrierAdmission::CounterExhausted(value);
        };
        state.accepted_commands = accepted_commands;
        state.barrier_enqueued = true;
        state.entries.push_back(Entry::Barrier(value));
        debug_assert!(state.entries.len() < EXTENSION_SERVICE_MAILBOX_CAPACITY);
        self.ready.notify_one();
        BarrierAdmission::Accepted
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
                    Entry::Normal(value) => {
                        state.ordinary_queued = state
                            .ordinary_queued
                            .checked_sub(1)
                            .expect("ordinary entry accounting is exact");
                        Delivery::Normal(value)
                    }
                    Entry::Barrier(value) => {
                        debug_assert!(state.barrier_enqueued);
                        state.barrier_enqueued = false;
                        Delivery::Normal(value)
                    }
                    Entry::Shutdown => Delivery::Shutdown {
                        accepted_commands: state.accepted_commands,
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
        state.ordinary_queued = 0;
        state.barrier_enqueued = false;
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
        state.accepted_commands = u64::MAX;
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
    fn full_normal_fifo_retains_profile_barrier_and_shutdown_slots() {
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
        assert!(matches!(
            mailbox.try_push_barrier(EXTENSION_SERVICE_NORMAL_CAPACITY),
            BarrierAdmission::Accepted
        ));
        assert!(matches!(
            mailbox.try_push_barrier(usize::MAX),
            BarrierAdmission::Occupied(usize::MAX)
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
        assert!(matches!(
            mailbox.try_push_barrier(usize::MAX),
            BarrierAdmission::Sealed(usize::MAX)
        ));

        for expected in 0..EXTENSION_SERVICE_NORMAL_CAPACITY {
            assert!(matches!(mailbox.receive(), Delivery::Normal(value) if value == expected));
        }
        assert!(matches!(
            mailbox.receive(),
            Delivery::Normal(value) if value == EXTENSION_SERVICE_NORMAL_CAPACITY
        ));
        assert!(matches!(
            mailbox.receive(),
            Delivery::Shutdown { accepted_commands }
                if accepted_commands == (EXTENSION_SERVICE_NORMAL_CAPACITY + 1) as u64
        ));
    }

    #[test]
    fn barrier_is_fifo_ordered_and_permits_ordinary_work_behind_it() {
        let mailbox = Mailbox::new();
        assert!(matches!(
            mailbox.try_push_normal(1),
            NormalAdmission::Accepted
        ));
        assert!(matches!(
            mailbox.try_push_normal(2),
            NormalAdmission::Accepted
        ));
        assert!(matches!(
            mailbox.try_push_barrier(3),
            BarrierAdmission::Accepted
        ));
        assert!(matches!(
            mailbox.try_push_normal(4),
            NormalAdmission::Accepted
        ));
        assert_eq!(mailbox.try_push_shutdown(), ShutdownAdmission::Accepted);

        for expected in 1..=4 {
            assert!(matches!(mailbox.receive(), Delivery::Normal(value) if value == expected));
        }
        assert!(matches!(
            mailbox.receive(),
            Delivery::Shutdown {
                accepted_commands: 4
            }
        ));
    }

    #[test]
    fn barrier_does_not_consume_any_ordinary_capacity_behind_it() {
        let mailbox = Mailbox::new();
        assert!(matches!(
            mailbox.try_push_barrier(usize::MAX),
            BarrierAdmission::Accepted
        ));
        for value in 0..EXTENSION_SERVICE_NORMAL_CAPACITY {
            assert!(matches!(
                mailbox.try_push_normal(value),
                NormalAdmission::Accepted
            ));
        }
        assert!(matches!(
            mailbox.try_push_normal(EXTENSION_SERVICE_NORMAL_CAPACITY),
            NormalAdmission::Full(EXTENSION_SERVICE_NORMAL_CAPACITY)
        ));
        assert_eq!(mailbox.try_push_shutdown(), ShutdownAdmission::Accepted);
        assert_eq!(mailbox.len(), EXTENSION_SERVICE_MAILBOX_CAPACITY);

        assert!(matches!(mailbox.receive(), Delivery::Normal(usize::MAX)));
        for expected in 0..EXTENSION_SERVICE_NORMAL_CAPACITY {
            assert!(matches!(mailbox.receive(), Delivery::Normal(value) if value == expected));
        }
        assert!(matches!(
            mailbox.receive(),
            Delivery::Shutdown { accepted_commands }
                if accepted_commands == (EXTENSION_SERVICE_NORMAL_CAPACITY + 1) as u64
        ));
    }

    #[test]
    fn barrier_slot_reopens_only_after_its_exact_entry_is_delivered() {
        let mailbox = Mailbox::new();
        assert!(matches!(
            mailbox.try_push_barrier(7),
            BarrierAdmission::Accepted
        ));
        assert!(matches!(
            mailbox.try_push_barrier(8),
            BarrierAdmission::Occupied(8)
        ));
        assert!(matches!(mailbox.receive(), Delivery::Normal(7)));
        assert!(matches!(
            mailbox.try_push_barrier(8),
            BarrierAdmission::Accepted
        ));
        assert!(matches!(mailbox.receive(), Delivery::Normal(8)));

        assert_eq!(mailbox.try_push_shutdown(), ShutdownAdmission::Accepted);
        assert!(matches!(
            mailbox.receive(),
            Delivery::Shutdown {
                accepted_commands: 2
            }
        ));
    }

    #[test]
    fn barrier_counter_exhaustion_queues_shutdown_and_seals_every_admission_lane() {
        let mailbox = Mailbox::new();
        mailbox.exhaust_normal_counter_for_test();

        assert!(matches!(
            mailbox.try_push_barrier(7),
            BarrierAdmission::CounterExhausted(7)
        ));
        assert_eq!(mailbox.len(), 1);
        assert!(matches!(
            mailbox.try_push_normal(8),
            NormalAdmission::Sealed(8)
        ));
        assert!(matches!(
            mailbox.try_push_barrier(9),
            BarrierAdmission::Sealed(9)
        ));
        assert_eq!(
            mailbox.try_push_shutdown(),
            ShutdownAdmission::AlreadyEnqueued
        );
        assert!(matches!(
            mailbox.receive(),
            Delivery::Shutdown {
                accepted_commands: u64::MAX
            }
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
        assert!(matches!(
            mailbox.try_push_barrier(9),
            BarrierAdmission::Closed(9)
        ));
        assert_eq!(mailbox.try_push_shutdown(), ShutdownAdmission::Closed);
        assert!(matches!(mailbox.receive(), Delivery::Closed));
    }
}
