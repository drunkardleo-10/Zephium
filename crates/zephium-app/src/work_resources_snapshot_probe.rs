//! Release-excluded capture/presentation ordering, not a new controller loop.
//!
//! The original application owner retains `SnapshotRelease` after worker loss.
//! The native holder's existing callback/task reservation and later global audit
//! remain independent: this slot proves only that its exact retirement result
//! arrived, not that the native callback returned or the page was destroyed.

use super::*;

type Retire = Box<dyn FnOnce(Box<dyn FnOnce(bool) + Send>) -> bool + Send>;

enum ReleaseState {
    Waiting(Option<Retire>),
    Running,
    Returned,
    Failed,
}

/// App-owned one-shot slot; neither actor drop nor a late terminal erases it.
pub(super) struct SnapshotRelease {
    resource: WorkBrowserResourceJoin,
    state: Mutex<ReleaseState>,
    listener: Mutex<Option<Arc<Waker>>>,
    failed: AtomicBool,
}
struct ReleaseCompletion(Option<Arc<SnapshotRelease>>);
impl ReleaseCompletion {
    fn complete(mut self, retired: bool) {
        if let Some(slot) = self.0.take() {
            assert!(slot.complete(retired), "snapshot retirement wake failed");
        }
    }
}
impl Drop for ReleaseCompletion {
    fn drop(&mut self) {
        if let Some(slot) = self.0.take() {
            // Lost completion is sticky, but destructors never propagate a
            // second panic. The original platform still owns its native debt.
            let _ = slot.complete(false);
        }
    }
}
impl SnapshotRelease {
    pub(super) fn new(resource: WorkBrowserResourceJoin, retire: Retire) -> Arc<Self> {
        Arc::new(Self {
            resource,
            state: Mutex::new(ReleaseState::Waiting(Some(retire))),
            listener: Mutex::new(None),
            failed: AtomicBool::new(false),
        })
    }
    fn fail(&self) {
        self.failed.store(true, Ordering::Release);
    }
    fn register(&self, waker: Waker) -> Result<(), AgentWorkFailure> {
        let mut listener = self.listener.lock().map_err(|_| {
            self.fail();
            AgentWorkFailure::ContextLost
        })?;
        if listener.is_some() {
            self.fail();
            return Err(AgentWorkFailure::Contract);
        }
        *listener = Some(Arc::new(waker));
        Ok(())
    }
    fn start(self: &Arc<Self>) -> Result<(), AgentWorkFailure> {
        let retire = {
            let mut state = self.state.lock().map_err(|_| {
                self.fail();
                AgentWorkFailure::ContextLost
            })?;
            let ReleaseState::Waiting(retire) = &mut *state else {
                return Err(AgentWorkFailure::Contract);
            };
            let retire = retire.take().ok_or(AgentWorkFailure::Contract)?;
            *state = ReleaseState::Running;
            retire
        };
        let returned = ReleaseCompletion(Some(self.clone()));
        // No arbitrary scheduling, completion or Waker code under a slot lock.
        let dispatched = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            retire(Box::new(move |retired| returned.complete(retired)))
        }));
        if !matches!(dispatched, Ok(true)) {
            self.fail();
            return Err(AgentWorkFailure::ContextLost);
        }
        Ok(())
    }
    fn complete(&self, retired: bool) -> bool {
        match self.state.lock() {
            Ok(mut state) => {
                if !matches!(*state, ReleaseState::Running) {
                    self.fail();
                }
                *state = if retired {
                    ReleaseState::Returned
                } else {
                    ReleaseState::Failed
                };
            }
            Err(_) => self.fail(),
        }
        let listener = match self.listener.lock() {
            Ok(listener) => listener.clone(),
            Err(_) => {
                self.fail();
                None
            }
        };
        // Terminal publication precedes this immutable, one-shot wake. Worker
        // polls before parking; no repeated-publication coalescing is needed.
        if listener.is_none_or(|waker| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake_by_ref())).is_err()
        }) {
            self.fail();
        }
        // The original native callback owner contains this and invalidates its
        // exact resource. Do not turn a failed wake into healthy native return.
        !self.failed.load(Ordering::Acquire)
    }
    pub(super) fn returned(&self) -> Result<bool, AgentWorkFailure> {
        if self.failed.load(Ordering::Acquire) {
            return Err(AgentWorkFailure::ContextLost);
        }
        match self.state.lock() {
            Ok(state) => match *state {
                ReleaseState::Returned => Ok(true),
                ReleaseState::Failed => Err(AgentWorkFailure::ContextLost),
                _ => Ok(false),
            },
            Err(_) => {
                self.fail();
                Err(AgentWorkFailure::ContextLost)
            }
        }
    }
}

/// Delegates the original facade and returns its *same* first observation only
/// after holder retirement. No recapture, semantic conversion or native handle.
pub(super) struct SnapshotReleaseBrowser {
    browser: RetainedBrowser,
    release: Arc<SnapshotRelease>,
    observation: Option<SemanticObservation>,
    observed: bool,
}
impl SnapshotReleaseBrowser {
    pub(super) fn new(
        browser: RetainedBrowser,
        release: Arc<SnapshotRelease>,
    ) -> Result<Self, AgentWorkFailure> {
        if browser.binding().lease().resource() != &release.resource {
            return Err(AgentWorkFailure::Contract);
        }
        Ok(Self {
            browser,
            release,
            observation: None,
            observed: false,
        })
    }
}
impl AgentWorkRetainedBrowser for SnapshotReleaseBrowser {
    fn binding(&self) -> &WorkBrowserReadBinding {
        self.browser.binding()
    }
    fn register_listener(&mut self, waker: Waker) -> Result<(), AgentWorkFailure> {
        self.browser.register_listener(waker.clone())?;
        self.release.register(waker)
    }
    fn check_health(&self, now: AgentPolicyInstant) -> Result<(), AgentWorkFailure> {
        self.release.returned()?;
        self.browser.check_health(now)
    }
    fn begin_observation(&mut self, now: AgentPolicyInstant) -> Result<(), AgentWorkFailure> {
        if self.observed {
            return Err(AgentWorkFailure::Contract);
        }
        self.observed = true;
        self.browser.begin_observation(now)
    }
    fn poll_observation(
        &mut self,
        now: AgentPolicyInstant,
    ) -> Result<Option<SemanticObservation>, AgentWorkFailure> {
        if self.observation.is_none() {
            let Some(observation) = self.browser.poll_observation(now)? else {
                return Ok(None);
            };
            self.observation = Some(observation);
            self.release.start()?;
        }
        if self.release.returned()? {
            Ok(self.observation.take())
        } else {
            Ok(None)
        }
    }
    fn begin_revocation(&mut self) -> Result<(), AgentWorkFailure> {
        // The app retains release/cleanup ownership if cancellation overtakes
        // initial capture. Revoking the actor is not retiring its presentation.
        self.browser.begin_revocation()
    }
    fn poll_revocation(
        &mut self,
        now: AgentPolicyInstant,
    ) -> Result<Option<WorkBrowserLeaseDeliveryProof>, AgentWorkFailure> {
        self.browser.poll_revocation(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource() -> WorkBrowserResourceJoin {
        WorkBrowserResources::new(WorkId::generate(), ProfileId::generate())
            .construct_document(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                ContextProfileStorageClass::Ephemeral,
                ContextNavigationTarget::parse("http://127.0.0.1:12345/semantic-rendering-v1.html")
                    .unwrap(),
                AgentPolicyInstant::from_millis(1),
            )
            .unwrap()
            .resource()
            .clone()
    }
    struct ReentrantWake {
        slot: Weak<SnapshotRelease>,
        panic: bool,
        saw_returned: AtomicBool,
    }
    impl Wake for ReentrantWake {
        fn wake(self: Arc<Self>) {
            self.saw_returned.store(
                self.slot.upgrade().unwrap().returned().unwrap(),
                Ordering::Release,
            );
            assert!(!self.panic, "hostile diagnostic wake");
        }
    }
    #[test]
    fn release_publishes_before_lock_free_reentrant_wake_and_propagates_panic() {
        for panic in [false, true] {
            let (tx, rx) = mpsc::sync_channel(1);
            let slot = SnapshotRelease::new(
                resource(),
                Box::new(move |callback| {
                    tx.send(callback).unwrap();
                    true
                }),
            );
            let wake = Arc::new(ReentrantWake {
                slot: Arc::downgrade(&slot),
                panic,
                saw_returned: AtomicBool::new(false),
            });
            slot.register(wake.clone().into()).unwrap();
            slot.start().unwrap();
            let callback = rx.recv().unwrap();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(true)));
            assert_eq!(result.is_err(), panic);
            assert!(wake.saw_returned.load(Ordering::Acquire));
            if panic {
                assert!(slot.returned().is_err());
            } else {
                assert!(slot.returned().unwrap());
            }
        }
    }
    #[test]
    fn scheduler_refusal_or_panic_after_completion_cannot_release_observation() {
        for panic in [false, true] {
            let slot = SnapshotRelease::new(
                resource(),
                Box::new(move |callback| {
                    callback(true);
                    assert!(!panic, "hostile diagnostic dispatch");
                    false
                }),
            );
            slot.register(
                Arc::new(ReentrantWake {
                    slot: Arc::downgrade(&slot),
                    panic: false,
                    saw_returned: AtomicBool::new(false),
                })
                .into(),
            )
            .unwrap();
            assert!(slot.start().is_err());
            assert!(slot.returned().is_err());
        }
    }
}
