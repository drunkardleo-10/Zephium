//! Attempt-owned anonymous storage lifetime; never an account or action grant.
use crate::{ContextRunId, WorkId};
use std::{
    fmt,
    sync::{Arc, Mutex, Weak},
    time::Instant,
};
use zephium_core::ids::ProfileId;

/// Process-local anonymous storage scope owned by one active Work attempt.
#[derive(Clone)]
pub struct WorkBrowserSession(Arc<State>);
/// Native cache witness that cannot extend the owning attempt.
pub struct WeakWorkBrowserSession(Weak<State>);
struct State {
    id: ContextRunId,
    profile: ProfileId,
    work: WorkId,
    deadline: Instant,
    retirement: Mutex<Retirement>,
}
#[derive(Default)]
struct Retirement {
    closed: bool,
    registered: bool,
    callback: Option<Box<dyn FnOnce() + Send>>,
}
impl WorkBrowserSession {
    /// Creates a fresh storage scope, without allocating native resources.
    pub fn new(profile: ProfileId, work: WorkId, deadline: Instant) -> Self {
        Self(Arc::new(State {
            id: ContextRunId::generate(),
            profile,
            work,
            deadline,
            retirement: Mutex::new(Retirement::default()),
        }))
    }
    /// Opaque storage key; never a native resource or execution capability.
    pub fn id(&self) -> ContextRunId {
        self.0.id
    }
    /// Profile owning policy and erasure for this anonymous scope.
    pub fn profile(&self) -> ProfileId {
        self.0.profile
    }
    /// Checks exact profile, Work ownership, deadline and retirement.
    pub fn admits(&self, profile: ProfileId, work: WorkId) -> bool {
        self.0.profile == profile && self.0.work == work && self.is_current()
    }
    /// Whether the original session is alive and within its deadline.
    pub fn is_current(&self) -> bool {
        Instant::now() < self.0.deadline && self.0.retirement.lock().is_ok_and(|s| !s.closed)
    }
    /// Observes lifetime without keeping the scope alive.
    pub fn downgrade(&self) -> WeakWorkBrowserSession {
        WeakWorkBrowserSession(Arc::downgrade(&self.0))
    }
    /// Native storage has one owner. Registration racing closure refuses reuse.
    pub fn register_retirement(&self, callback: Box<dyn FnOnce() + Send>) -> bool {
        let Ok(mut state) = self.0.retirement.lock() else {
            return false;
        };
        if state.closed || state.registered || Instant::now() >= self.0.deadline {
            return false;
        }
        state.registered = true;
        state.callback = Some(callback);
        true
    }
    /// Revokes future attachment immediately; existing resources still drain normally.
    pub fn close(&self) {
        self.0.close();
    }
}
impl State {
    fn close(&self) {
        let callback = {
            let mut state = self
                .retirement
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.closed = true;
            state.callback.take()
        };
        if let Some(callback) = callback {
            callback();
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        self.close();
    }
}
impl WeakWorkBrowserSession {
    /// Whether the original session is alive and within its deadline.
    pub fn is_current(&self) -> bool {
        self.0
            .upgrade()
            .is_some_and(|state| WorkBrowserSession(state).is_current())
    }
}
impl fmt::Debug for WorkBrowserSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WorkBrowserSession([anonymous, redacted])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    fn session() -> WorkBrowserSession {
        WorkBrowserSession::new(
            1_u128.into(),
            WorkId::from(2_u128),
            Instant::now() + Duration::from_secs(60),
        )
    }

    #[test]
    fn anonymous_session_revokes_clones_without_extending_deadline_or_ownership() {
        let session = session();
        let child = session.clone();
        let weak = session.downgrade();
        assert!(child.admits(1_u128.into(), WorkId::from(2_u128)));
        assert!(!child.admits(3_u128.into(), WorkId::from(2_u128)));
        assert!(!child.admits(1_u128.into(), WorkId::from(3_u128)));
        assert_ne!(
            session.id(),
            WorkBrowserSession::new(session.profile(), WorkId::from(2_u128), Instant::now()).id()
        );
        assert!(
            !WorkBrowserSession::new(session.profile(), WorkId::from(2_u128), Instant::now())
                .is_current()
        );
        session.close();
        assert!(!child.is_current());
        assert!(!weak.is_current());
        assert!(
            !child.register_retirement(Box::new(|| panic!("closed session acquired a new store")))
        );
    }

    #[test]
    fn anonymous_session_retires_once_on_close_or_last_owner_drop() {
        for explicit_close in [false, true] {
            let session = session();
            let child = session.clone();
            let calls = Arc::new(AtomicUsize::new(0));
            let count = calls.clone();
            assert!(session.register_retirement(Box::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
            })));
            assert!(!child.register_retirement(Box::new(|| panic!("duplicate store owner"))));
            drop(session);
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            if explicit_close {
                child.close();
                child.close();
            }
            drop(child);
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn anonymous_session_close_racing_registration_cannot_lose_retirement() {
        for _ in 0..32 {
            let session = session();
            let child = session.clone();
            let calls = Arc::new(AtomicUsize::new(0));
            let count = calls.clone();
            let thread = std::thread::spawn(move || {
                child.register_retirement(Box::new(move || {
                    count.fetch_add(1, Ordering::SeqCst);
                }))
            });
            session.close();
            let registered = thread.join().unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), usize::from(registered));
            assert!(!session.is_current());
        }
    }
}
