//! Private retained adapter; original Work ownership never enters the worker.

use super::*;
use std::sync::Weak;
use std::task::Waker;
use zephium_agent_controller::{AgentWorkFailure, AgentWorkRetainedBrowser};

#[cfg(all(test, feature = "work-execution-probe"))]
#[path = "work_resources_controller_tests.rs"]
mod tests;

/// One immutable lease listener. It carries no terminal, registry or port.
pub(super) struct LeaseSignal {
    lease: WorkBrowserExecutionLease,
    resource: Weak<Resource>,
    retired: Arc<AtomicBool>,
    waker: Waker,
    pending: AtomicBool,
    running: AtomicBool,
    failed: AtomicBool,
}
impl LeaseSignal {
    fn fail(&self) {
        self.failed.store(true, Ordering::Release);
        if !self.retired.load(Ordering::Acquire) {
            if let Some(resource) = self.resource.upgrade() {
                resource.fail();
            }
        }
    }
    fn notify(&self) -> bool {
        if self.retired.load(Ordering::Acquire) {
            return true;
        }
        if self.failed.load(Ordering::Acquire) {
            return false;
        }
        if self.running.swap(true, Ordering::AcqRel) {
            self.fail();
            return false;
        }
        if !self.pending.swap(true, Ordering::AcqRel)
            && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.waker.wake_by_ref()))
                .is_err()
        {
            self.fail();
        }
        self.running.store(false, Ordering::Release);
        !self.failed.load(Ordering::Acquire)
    }
    fn rearm(&self) -> Result<(), Refusal> {
        self.pending.swap(false, Ordering::AcqRel);
        if self.failed.load(Ordering::Acquire) {
            Err(Refusal::Uncertain)
        } else {
            Ok(())
        }
    }
}
impl Wake for LeaseSignal {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        // Native delivery owns this invocation and contains the panic. A failed
        // listener must quarantine, never be mistaken for returned notification.
        assert!(self.notify(), "retained lease notification failed");
    }
}

impl Notifications {
    pub(super) fn publish_actor_wakes(&self) {
        let mut listeners: [Option<Arc<LeaseSignal>>; MAX_LIVE_CONTEXTS] =
            std::array::from_fn(|_| None);
        match self.actors.lock() {
            Ok(actors) => {
                for (slot, actor) in listeners.iter_mut().zip(actors.iter()) {
                    *slot = actor.upgrade();
                }
            }
            Err(_) => {
                self.failed.store(true, Ordering::Release);
                return;
            }
        }
        // Neither invocation nor last-Waker drop occurs under the lane mutex.
        for listener in listeners.into_iter().flatten() {
            listener.notify();
        }
    }
}

struct RetainedBrowser {
    browser: LeaseBrowser,
    binding: WorkBrowserReadBinding,
    listener: Option<Arc<LeaseSignal>>,
    read: Option<(PendingRead, SemanticObservationRequest)>,
    revoke: Option<PendingLifecycle>,
    delivered: bool,
}
impl WorkResourceOwner {
    fn retained_browser(
        &self,
        lease: WorkBrowserExecutionLease,
        now: AgentPolicyInstant,
    ) -> Result<RetainedBrowser, Refusal> {
        let binding = self.shared.lock_rows()?.read_binding(&lease, now)?;
        let browser = self.browser(lease, now)?;
        Ok(RetainedBrowser {
            browser,
            binding,
            listener: None,
            read: None,
            revoke: None,
            delivered: false,
        })
    }
}
impl RetainedBrowser {
    fn listener(&self) -> Result<&Arc<LeaseSignal>, Refusal> {
        self.listener
            .as_ref()
            .filter(|listener| listener.lease == self.browser.lease)
            .ok_or_else(|| self.browser.resource.refusal())
    }
    fn rearm(&self) -> Result<(), Refusal> {
        self.listener()?.rearm()?;
        // The stable resource observer remains app-owned, but the active worker
        // also rearms it so an idle health failure cannot be coalesced forever.
        let health = self
            .browser
            .resource
            .lock_local(&self.browser.resource.health)?
            .poll();
        if health != WorkBrowserResourceHealthState::Current {
            return Err(self.browser.resource.refusal());
        }
        Ok(())
    }
    fn error(error: Refusal) -> AgentWorkFailure {
        match error {
            Refusal::NativeAdmission(failure) => AgentWorkFailure::Native(failure),
            _ => AgentWorkFailure::ContextLost,
        }
    }
}
impl AgentWorkRetainedBrowser for RetainedBrowser {
    fn binding(&self) -> &WorkBrowserReadBinding {
        &self.binding
    }
    fn register_listener(&mut self, waker: Waker) -> Result<(), AgentWorkFailure> {
        if self.listener.is_some() || self.read.is_some() || self.revoke.is_some() {
            return Err(Self::error(self.browser.resource.refusal()));
        }
        let listener = Arc::new(LeaseSignal {
            lease: self.browser.lease.clone(),
            resource: Arc::downgrade(&self.browser.resource),
            retired: self.browser.retired.clone(),
            waker,
            pending: AtomicBool::new(false),
            running: AtomicBool::new(false),
            failed: AtomicBool::new(false),
        });
        {
            let mut actors = self
                .browser
                .shared
                .notifications
                .actors
                .lock()
                .map_err(|_| Self::error(self.browser.shared.refusal()))?;
            actors.retain(|actor| actor.strong_count() != 0);
            if actors.len() >= MAX_LIVE_CONTEXTS {
                return Err(Self::error(self.browser.resource.refusal()));
            }
            actors.push(Arc::downgrade(&listener));
        }
        self.listener = Some(listener);
        self.rearm().map_err(Self::error)?;
        self.browser
            .shared
            .current(&self.browser.resource)
            .map_err(Self::error)
    }
    fn check_health(&self, now: AgentPolicyInstant) -> Result<(), AgentWorkFailure> {
        // Construction may inspect before worker registration; every dispatch
        // below independently requires that immutable listener to exist.
        if self.listener.is_some() {
            self.rearm().map_err(Self::error)?;
        }
        self.browser.health(now).map_err(Self::error)
    }
    fn begin_observation(&mut self, now: AgentPolicyInstant) -> Result<(), AgentWorkFailure> {
        self.listener().map_err(Self::error)?;
        self.check_health(now)?;
        if self.read.is_some() || self.revoke.is_some() || self.delivered {
            return Err(AgentWorkFailure::Contract);
        }
        let request = self
            .browser
            .shared
            .lock_rows()
            .map_err(Self::error)?
            .observe_initial(&self.browser.lease, now)
            .map_err(Refusal::from)
            .map_err(Self::error)?;
        let correlation = request.invocation().correlation();
        let observation = SemanticObservationRequest::initial(
            SemanticObservationId::new(correlation.invocation().get())
                .ok_or(AgentWorkFailure::Contract)?,
            correlation.frame().context(),
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        let pending = PendingRead::dispatch(
            self.browser.shared.clone(),
            self.browser.resource.clone(),
            request,
        )
        .map_err(Self::error)?;
        self.read = Some((pending, observation));
        Ok(())
    }
    fn poll_observation(
        &mut self,
        now: AgentPolicyInstant,
    ) -> Result<Option<SemanticObservation>, AgentWorkFailure> {
        self.rearm().map_err(Self::error)?;
        let (pending, _) = self.read.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let Some(event) = pending.poll(now).map_err(Self::error)? else {
            return Ok(None);
        };
        let (_, request) = self.read.take().ok_or(AgentWorkFailure::Contract)?;
        let snapshot = match event {
            WorkBrowserObservationEvent::Snapshot(snapshot) => *snapshot,
            WorkBrowserObservationEvent::Refused(failure) => {
                return Err(AgentWorkFailure::Observation(failure))
            }
            WorkBrowserObservationEvent::DebtSettled => return Err(AgentWorkFailure::ContextLost),
        };
        if snapshot.frame() != self.binding.frame() {
            return Err(AgentWorkFailure::ContextLost);
        }
        let boundaries = snapshot
            .nodes()
            .iter()
            .filter(|node| node.role() == SemanticRole::FrameBoundary)
            .map(SemanticNode::reference)
            .collect::<Vec<_>>();
        let mut assembler = SemanticObservationAssembler::new(request, snapshot)
            .map_err(|_| AgentWorkFailure::Context)?;
        for boundary in boundaries {
            assembler
                .mark_frame_unsupported(
                    FrameId::MAIN,
                    boundary,
                    SemanticFrameUnsupported::PolicyBlocked,
                )
                .map_err(|_| AgentWorkFailure::Context)?;
        }
        let observation = assembler.finish().map_err(|_| AgentWorkFailure::Context)?;
        self.check_health(now)?;
        Ok(Some(observation))
    }
    fn begin_revocation(&mut self) -> Result<(), AgentWorkFailure> {
        if self.revoke.is_some() || self.delivered {
            return Ok(());
        }
        let listener = self.listener().map_err(Self::error)?.clone();
        let (request, mut ticket) = self
            .browser
            .shared
            .lock_rows()
            .map_err(Self::error)?
            .revoke_with_delivery(&self.browser.lease)
            .map_err(Refusal::from)
            .map_err(Self::error)?;
        // Original ticket is registered BEFORE dispatch. Registration never
        // runs the wake and does not manufacture a physical delivery receipt.
        ticket
            .register_waker(listener.into())
            .map_err(|_| Self::error(self.browser.resource.refusal()))?;
        self.revoke = Some(
            PendingLifecycle::dispatch(
                self.browser.shared.clone(),
                self.browser.resource.clone(),
                request,
                Some(ticket),
                Some(self.browser.retired.clone()),
            )
            .map_err(Self::error)?,
        );
        Ok(())
    }
    fn poll_revocation(
        &mut self,
        now: AgentPolicyInstant,
    ) -> Result<Option<WorkBrowserLeaseDeliveryProof>, AgentWorkFailure> {
        self.listener()
            .map_err(Self::error)?
            .rearm()
            .map_err(Self::error)?;
        if let Some((pending, _)) = self.read.as_mut() {
            match pending.poll(now) {
                Ok(None) => return Ok(None),
                Ok(Some(_)) | Err(_) => {
                    self.read.take();
                }
            }
        }
        // Physical receipt may be consumed during notifier Running. Scoped
        // actor drain is not notifier completion or successor admission: the
        // original native reservation and application health still gate those.
        let pending = self.revoke.as_mut().ok_or(AgentWorkFailure::Contract)?;
        match pending.poll(now).map_err(Self::error)? {
            None => Ok(None),
            Some(LifecycleResult::Delivered(proof)) => {
                self.delivered = true;
                self.revoke.take();
                Ok(Some(proof))
            }
            Some(LifecycleResult::Event(_)) => Err(AgentWorkFailure::ContextLost),
        }
    }
}
