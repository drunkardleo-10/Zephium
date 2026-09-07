//! Compile-time-only composition of the original private Work owner. This is
//! not shell/product admission and cannot be enabled in an optimized build.

use super::snapshot_probe::{SnapshotRelease, SnapshotReleaseBrowser};
use super::*;
use zephium_agent_controller::*;
use zephium_agent_runtime::*;

/// Original platform factory, consumed once before any scoped actor exists.
pub type RetainedProbeNativeFactory = Box<
    dyn FnOnce(Arc<dyn Fn(ContextNativeEvent) + Send + Sync>) -> Option<Arc<dyn AgentBrowserPort>>,
>;
/// Exact platform holder retirement; no page/script/model authority.
pub type RetainedProbeRetire = Box<dyn FnOnce(Box<dyn FnOnce(bool) + Send>) -> bool + Send>;

/// Application-side owner retained independently of controller/worker results.
pub struct RetainedWorkProbeOwner {
    owner: WorkResourceOwner,
    pending: Option<PendingLifecycle>,
    resource: Option<WorkBrowserResourceJoin>,
    lease: Option<WorkBrowserExecutionLease>,
    release: Option<Arc<SnapshotRelease>>,
    constructed: bool,
    acquired: bool,
    started: bool,
    audit_attempted: bool,
    audit_pending: Option<ContextResourceAuditId>,
}
impl RetainedWorkProbeOwner {
    /// Creates no resource or worker; retains the platform's original event sink.
    pub fn new(
        profile: ProfileId,
        wake: Arc<dyn Fn() -> bool + Send + Sync>,
        factory: RetainedProbeNativeFactory,
    ) -> Option<Self> {
        Some(Self {
            owner: WorkResourceOwner::new(WorkId::generate(), profile, wake, factory)?,
            pending: None,
            resource: None,
            lease: None,
            release: None,
            constructed: false,
            acquired: false,
            started: false,
            audit_attempted: false,
            audit_pending: None,
        })
    }
    /// One frozen anonymous document, never an actor-selected URL.
    pub fn construct(
        &mut self,
        target: ContextNavigationTarget,
        now: AgentPolicyInstant,
    ) -> Result<(), &'static str> {
        if self.constructed || self.pending.is_some() {
            return Err("construct_phase");
        }
        self.constructed = true;
        let pending = self
            .owner
            .construct(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                ContextProfileStorageClass::Ephemeral,
                target,
                now,
            )
            .map_err(|_| "construct_admission")?;
        // Keep the exact admitted join even if construction later fails.
        self.resource = Some(pending.resource.join.clone());
        self.pending = Some(pending);
        Ok(())
    }
    /// Descriptive identity only; no port, row or facade is exposed.
    pub fn resource(&self) -> Option<&WorkBrowserResourceJoin> {
        self.resource.as_ref()
    }
    /// Settles the original app-owned operation slot, never a synthetic receipt.
    pub fn poll_lifecycle(
        &mut self,
        now: AgentPolicyInstant,
    ) -> Result<Option<WorkBrowserResourceEvent>, &'static str> {
        let Some(pending) = self.pending.as_mut() else {
            return Ok(None);
        };
        let Some(result) = pending.poll(now).map_err(|_| "lifecycle_terminal")? else {
            return Ok(None);
        };
        self.pending = None;
        let LifecycleResult::Event(event) = result else {
            return Err("unexpected_delivery");
        };
        if let WorkBrowserResourceEvent::Acquired(lease) = &event {
            self.lease = Some(lease.clone());
        }
        Ok(Some(event))
    }
    /// Sole fixture actor only; this API has no successor admission path.
    pub fn acquire(
        &mut self,
        run: ContextRunId,
        now: AgentPolicyInstant,
        deadline: AgentPolicyInstant,
    ) -> Result<(), &'static str> {
        if self.acquired || self.pending.is_some() {
            return Err("acquire_phase");
        }
        self.acquired = true;
        self.pending = Some(
            self.owner
                .acquire(
                    self.resource.as_ref().ok_or("resource")?,
                    run,
                    now,
                    deadline,
                )
                .map_err(|_| "acquire_admission")?,
        );
        Ok(())
    }
    /// Starts the reviewed common controller and original provider transport on
    /// the reviewed scoped runtime. Native ownership stays in `self`.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        input: AgentWorkRunInput,
        credential: AgentProviderCredential,
        audit: Arc<dyn AgentAuditPort>,
        task: Box<dyn AgentWorkTask>,
        now: AgentPolicyInstant,
        retire: RetainedProbeRetire,
    ) -> Result<
        (
            AgentWorkRetainedHandle,
            AgentRuntimeHandle,
            AgentRuntimeCompletion,
            AgentRuntimeScopedLifecycle,
        ),
        &'static str,
    > {
        if self.started || self.pending.is_some() {
            return Err("actor_phase");
        }
        self.started = true;
        let lease = self.lease.take().ok_or("lease")?;
        let browser = self
            .owner
            .retained_browser(lease, now)
            .map_err(|_| "original_facade")?;
        let release = SnapshotRelease::new(self.resource.clone().ok_or("resource")?, retire);
        self.release = Some(release.clone());
        let browser =
            SnapshotReleaseBrowser::new(browser, release).map_err(|_| "presentation_binding")?;
        let (controller, result, binding) = AgentWorkRetainedController::try_new(
            input,
            Box::new(browser),
            AgentProviderTransportConfig::STANDARD,
            credential,
            audit,
            task,
        )
        .map_err(|_| "controller_admission")?;
        let (handle, completion, lifecycle) = PendingScopedAgentRuntime::spawn_suspended(
            AgentRuntimeConfig::STANDARD,
            binding,
            Box::new(controller),
        )
        .map_err(|_| "worker_spawn")?
        .bind()
        .into_parts();
        handle.start_run().map_err(|_| "worker_admission")?;
        Ok((result, handle, completion, lifecycle))
    }
    /// Observed retirement only; not callback-return, scoped or global proof.
    pub fn presentation_returned(&self) -> bool {
        self.release
            .as_ref()
            .is_some_and(|release| release.returned() == Ok(true))
    }
    /// Drains exact late slots after an actor disappears.
    pub fn drain_abandoned(&self, now: AgentPolicyInstant) -> Result<(), &'static str> {
        self.owner
            .drain_abandoned(now)
            .map_err(|_| "abandoned_debt")
    }
    /// Abandons only the actor-side pending handle; the exact original terminal
    /// slot remains application-owned for late native completion and cleanup.
    pub fn abandon_pending(&mut self) {
        self.pending.take();
    }
    /// Original owner destruction, including quarantine after uncertain actors.
    pub fn destroy(&mut self) -> Result<(), &'static str> {
        if self.pending.is_some() {
            return Err("destroy_phase");
        }
        self.pending = Some(
            self.owner
                .destroy(self.resource.as_ref().ok_or("resource")?)
                .map_err(|_| "destroy_admission")?,
        );
        Ok(())
    }
    /// Waits for the exact native reporter after resource destruction, then
    /// schedules one original native seal. The unchanged audit still accounts
    /// callback-return and queued-task debt; retirement alone mints no proof.
    pub fn poll_seal(&mut self, audit: ContextResourceAuditId) -> Result<bool, &'static str> {
        if self.pending.is_some() || self.audit_attempted {
            return Err("seal_phase");
        }
        if let Some(resource) = &self.resource {
            if self
                .owner
                .shared
                .lock_rows()
                .map_err(|_| "resource_rows")?
                .phase(resource)
                .map_err(|_| "resource_phase")?
                != WorkBrowserResourcePhase::Destroyed
            {
                return Err("seal_phase");
            }
            let resource = self
                .owner
                .shared
                .resource(resource)
                .map_err(|_| "resource_owner")?;
            if resource.flights.load(Ordering::Acquire) != 0
                || !resource
                    .lock_local(&resource.health)
                    .map_err(|_| "resource_health")?
                    .reporter_retired()
            {
                return Ok(false);
            }
        }
        // Keep the original row/receiver through both audit dispatch and its
        // terminal. A scheduling refusal must not consume the cleanup owner.
        self.audit_attempted = true;
        let scheduled = self
            .owner
            .shutdown_audit(audit)
            .map_err(|_| "native_seal")?
            == ContextShutdownDispatch::AuditScheduled;
        if !scheduled {
            return Err("native_audit_dispatch");
        }
        self.audit_pending = Some(audit);
        Ok(true)
    }
    /// Bounded original global event lane; never rebound to the worker.
    pub fn poll_native_event(&mut self) -> Result<Option<ContextNativeEvent>, &'static str> {
        let event = self.owner.poll_native_event().map_err(|_| "native_event")?;
        if let Some(ContextNativeEvent::ShutdownAuditSettled(settlement)) = &event {
            if self.audit_pending == Some(settlement.audit()) {
                self.audit_pending = None;
            }
        }
        Ok(event)
    }
    /// Final nonblocking accounting after ordinary engine teardown. This checks
    /// callback-owner retirement, not task acceptance or native absence. Failed
    /// rows are kept until every original receiver is safe to drop.
    pub fn drain_after_engine_shutdown(&mut self, now: AgentPolicyInstant) -> bool {
        self.pending.take();
        let _ = self.owner.seal_resources();
        let accounted = self.owner.drain_abandoned(now).is_ok();
        loop {
            match self.poll_native_event() {
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(_) => return false,
            }
        }
        accounted
            && self.audit_pending.is_none()
            && self.owner.shared.lock_resources().is_ok_and(|resources| {
                resources.values().all(|resource| {
                    resource.flights.load(Ordering::Acquire) == 0
                        && resource.reads.load(Ordering::Acquire) == 0
                        && resource
                            .lock_local(&resource.health)
                            .is_ok_and(|health| health.reporter_retired())
                })
            })
    }
    /// Local descriptive closure only; final application/global proof is separate.
    pub fn locally_retired(&self) -> bool {
        self.owner.locally_retired()
    }
}
