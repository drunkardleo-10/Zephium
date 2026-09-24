//! Shipping single-page retained Work attachment. No UI or native authority in handles.
pub(crate) use super::application::RetainedWorkGroup;
use super::application::{ActorRequest, AdmissionPhase, RetainedWork, StagedActor};
pub use super::application::{RetainedHumanPhase, RetainedHumanResume, RetainedHumanSnapshot};
use super::*;
use crate::{AgentWorkProfileBinding, AgentWorkProfileReadiness, CallbackHandle, Command};
use std::collections::VecDeque;
use std::time::Instant;
use zephium_agent_controller::*;
use zephium_agent_provider_transport::AgentProviderCredential;

/// Fresh trusted successor; contains no retained native owner or old model session.
pub struct PreparedRetainedContinuation {
    generation: u32,
    spec: AgentWorkRetainedResourceSpec,
    actor: ActorRequest,
}
impl PreparedRetainedContinuation {
    pub fn try_new(
        generation: u32,
        input: AgentWorkRunInput,
        config: crate::AgentWorkApplicationConfig,
        credential: AgentProviderCredential,
        task: Box<dyn AgentWorkTask>,
    ) -> Result<Self, AgentWorkFailure> {
        let spec = input.retained_resource_spec()?;
        let (runtime, provider) = config.into_parts();
        let actor = ActorRequest {
            run: spec.identity.owner(),
            deadline: spec.expires_at,
            prepare: Box::new(move |browser, audit, waiting| {
                let waiting = waiting.ok_or(AgentWorkFailure::Contract)?;
                StagedActor::try_new(
                    input,
                    browser,
                    runtime,
                    provider,
                    credential,
                    audit,
                    task,
                    Some(waiting),
                )
            }),
        };
        Ok(Self {
            generation,
            spec,
            actor,
        })
    }
}

enum HumanCommand {
    Present(u32, WorkBrowserHumanRegion),
    Continue(u32),
}

#[cfg(feature = "work-execution-probe")]
#[path = "work_resources_public_product.rs"]
mod public_qualification;

/// Move-only original Engine port factory. The stable sink belongs to the
/// application resource owner, not to an actor runtime's private mailbox.
pub type RetainedWorkNativeFactory = Box<
    dyn FnOnce(Arc<dyn Fn(ContextNativeEvent) + Send + Sync>) -> Option<Arc<dyn AgentBrowserPort>>
        + Send,
>;

/// Original trusted composition owners and deferred native factory.
pub struct RetainedWorkPorts {
    engine: crate::SharedEngine,
    journal: Arc<dyn AgentWorkJournalPort>,
    audit: Arc<dyn AgentAuditPort>,
    native: RetainedWorkNativeFactory,
}
impl RetainedWorkPorts {
    pub fn new(
        engine: crate::SharedEngine,
        journal: Arc<dyn AgentWorkJournalPort>,
        audit: Arc<dyn AgentAuditPort>,
        native: RetainedWorkNativeFactory,
    ) -> Self {
        Self {
            engine,
            journal,
            audit,
            native,
        }
    }
}

/// Dormant trusted product request. No native resource or provider request is
/// created until the original Shell accepts Engine/Store and selected profile.
#[must_use]
pub struct PreparedRetainedWork {
    construction_attempt: zephium_agentic::WorkBrowserConstructionAttempt,
    anonymous_session: Option<zephium_agentic::WorkBrowserSession>,
    page: Option<RetainedPageAdmission>,
    work: Option<WorkId>,
    engine: crate::SharedEngine,
    journal: Arc<dyn AgentWorkJournalPort>,
    audit: Arc<dyn AgentAuditPort>,
    native: RetainedWorkNativeFactory,
    profile: AgentWorkProfileBinding,
    spec: AgentWorkRetainedResourceSpec,
    actor: ActorRequest,
}
impl PreparedRetainedWork {
    pub fn with_construction_attempt(
        mut self,
        attempt: zephium_agentic::WorkBrowserConstructionAttempt,
    ) -> Result<Self, AgentWorkFailure> {
        if !self.spec.isolated_public
            && attempt != zephium_agentic::WorkBrowserConstructionAttempt::Initial
        {
            return Err(AgentWorkFailure::Contract);
        }
        self.construction_attempt = attempt;
        Ok(self)
    }

    pub fn with_page_admission(
        mut self,
        page: RetainedPageAdmission,
    ) -> Result<Self, AgentWorkFailure> {
        if !self.spec.isolated_public
            || self.work != Some(page.work)
            || self.profile.profile() != page.profile
            || self.spec.deadline > page.deadline
        {
            return Err(AgentWorkFailure::Contract);
        }
        self.page = Some(page);
        Ok(self)
    }
    pub fn with_anonymous_session(
        mut self,
        session: zephium_agentic::WorkBrowserSession,
    ) -> Result<Self, AgentWorkFailure> {
        if !self.spec.isolated_public
            || !self
                .work
                .is_some_and(|work| session.admits(self.profile.profile(), work))
        {
            return Err(AgentWorkFailure::Contract);
        }
        self.anonymous_session = Some(session);
        Ok(self)
    }

    /// Preserve the durable aggregate selected by the trusted runtime owner.
    /// This is an identity join and grants no additional native capability.
    pub fn with_work_identity(mut self, work: WorkId) -> Self {
        self.work = Some(work);
        self
    }
    pub fn try_new(
        input: AgentWorkRunInput,
        profile: AgentWorkProfileBinding,
        config: crate::AgentWorkApplicationConfig,
        credential: AgentProviderCredential,
        task: Box<dyn AgentWorkTask>,
        ports: RetainedWorkPorts,
    ) -> Result<Self, AgentWorkFailure> {
        let (runtime, provider) = config.into_parts();
        let spec = input.retained_resource_spec()?;
        let actor = ActorRequest {
            run: spec.identity.owner(),
            deadline: spec.expires_at,
            prepare: Box::new(move |browser, audit, waiting| {
                StagedActor::try_new(
                    input, browser, runtime, provider, credential, audit, task, waiting,
                )
            }),
        };
        Self::from_actor(
            spec,
            actor,
            profile,
            ports.engine,
            ports.journal,
            ports.audit,
            ports.native,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn from_actor(
        spec: AgentWorkRetainedResourceSpec,
        actor: ActorRequest,
        profile: AgentWorkProfileBinding,
        engine: crate::SharedEngine,
        journal: Arc<dyn AgentWorkJournalPort>,
        audit: Arc<dyn AgentAuditPort>,
        native: RetainedWorkNativeFactory,
    ) -> Result<Self, AgentWorkFailure> {
        if spec.identity.profile() != profile.profile()
            || spec.storage != profile.storage_class()
            || !std::ptr::addr_eq(Arc::as_ptr(&journal), Arc::as_ptr(&audit))
            || spec.deadline <= Instant::now()
        {
            return Err(AgentWorkFailure::Contract);
        }
        Ok(Self {
            construction_attempt: Default::default(),
            anonymous_session: None,
            page: None,
            work: None,
            engine,
            journal,
            audit,
            native,
            profile,
            spec,
            actor,
        })
    }

    #[cfg(all(test, feature = "work-execution-probe"))]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn for_test(
        spec: AgentWorkRetainedResourceSpec,
        actor: ActorRequest,
        profile: AgentWorkProfileBinding,
        engine: crate::SharedEngine,
        journal: Arc<dyn AgentWorkJournalPort>,
        audit: Arc<dyn AgentAuditPort>,
        native: RetainedWorkNativeFactory,
    ) -> Result<Self, AgentWorkFailure> {
        Self::from_actor(spec, actor, profile, engine, journal, audit, native)
    }
}

/// Move-only admission minted from an owned, running durable read step.
pub struct RetainedPageAdmission {
    pub(crate) profile: zephium_core::ids::ProfileId,
    pub(crate) work: WorkId,
    pub(crate) execution: zephium_core::work::WorkExecutionId,
    pub(crate) attempt: zephium_core::work::WorkAttemptId,
    pub(crate) step: zephium_core::work::WorkStepId,
    pub(crate) workers: u8,
    pub(crate) deadline: Instant,
}
impl RetainedPageAdmission {
    pub fn native_group(&self) -> (WorkId, u8) {
        (self.work, self.workers.min(3))
    }
    fn same_group(&self, other: &Self) -> bool {
        self.profile == other.profile
            && self.work == other.work
            && self.execution == other.execution
            && self.attempt == other.attempt
            && self.workers == other.workers
            && self.deadline == other.deadline
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetainedWorkPhase {
    Attaching,
    Constructing,
    Loading,
    NeedsReview,
    Reviewing,
    Ready,
    Acquiring,
    Admitting,
    Starting,
    Running,
    Closing,
    Terminal,
    Uncertain,
    Refused,
}

/// Content-free projection. Terminal means durable acknowledgement, not factual
/// verification, human-input ownership, resource destruction or global shutdown.
#[derive(Clone, Copy, Debug)]
pub struct RetainedWorkSnapshot {
    pub phase: RetainedWorkPhase,
    pub run: ContextRunId,
    pub record: Option<AgentWorkRecord>,
    /// Closed original ledger totals, exposed only after exact terminal acknowledgement.
    pub usage: Option<zephium_core::work::runtime::WorkUsage>,
    pub failure: Option<AgentWorkFailure>,
    pub construction_timed_out: bool,
    pub persistence_failure: Option<AgentWorkJournalError>,
    /// Atomic durable result identity, present only after publication ACK.
    pub artifact: Option<AgentWorkArtifactDescriptor>,
    /// Explicit archived-result read status. Content is retrieved separately.
    pub artifact_read: Option<Result<bool, AgentWorkJournalError>>,
    /// Exact historical review acknowledgement; its debt is never cleared.
    pub last_review: Option<Result<AgentWorkRecord, AgentWorkJournalError>>,
}
struct Projection {
    human: Option<RetainedHumanSnapshot>,
    human_resume: Option<RetainedHumanResume>,
    human_command: Option<HumanCommand>,
    continuation: Option<PreparedRetainedContinuation>,
    #[cfg(feature = "work-execution-probe")]
    construction_resource: Option<WorkBrowserResourceJoin>,
    frame: Option<Arc<zephium_agentic::WorkBrowserFrame>>,
    snapshot: RetainedWorkSnapshot,
    events: VecDeque<AgentWorkEvent>,
    extraction: Option<Box<SemanticOwnedExtractionResult>>,
    archived: Option<AgentWorkArchivedExtraction>,
    records: Vec<AgentWorkRecord>,
    read_requested: Option<AgentWorkRecord>,
    read_active: bool,
    review_requested: Option<(AgentWorkRecord, crate::AgentWorkReviewDecision)>,
    review_active: bool,
}
struct ProductSignal {
    close: AtomicBool,
    closed: AtomicBool,
    group_locally_retired: AtomicBool,
    projection: Mutex<Projection>,
    stop: AtomicBool,
    reconcile: AtomicBool,
}

/// Bounded observation/control only. No resource selector, browser facade,
/// account proof, native handles, policy mutation or successor admission.
#[derive(Clone)]
pub struct RetainedWorkHandle {
    signal: Arc<ProductSignal>,
    callback: CallbackHandle,
}
impl RetainedWorkHandle {
    pub fn human_snapshot(&self) -> Option<RetainedHumanSnapshot> {
        self.signal.projection.lock().ok()?.human
    }
    pub fn human_resume(&self) -> Option<RetainedHumanResume> {
        self.signal.projection.lock().ok()?.human_resume.clone()
    }
    /// Queue one explicit presentation of this exact page. Acceptance is not completion.
    pub fn present_human(&self, generation: u32, region: WorkBrowserHumanRegion) -> bool {
        self.human_command(
            generation,
            RetainedHumanPhase::WaitingForHuman,
            HumanCommand::Present(generation, region),
        )
    }
    /// Retire human input and freeze the page; fresh trusted admission remains separate.
    pub fn continue_human(&self, generation: u32) -> bool {
        self.human_command(
            generation,
            RetainedHumanPhase::Presented,
            HumanCommand::Continue(generation),
        )
    }
    fn human_command(
        &self,
        generation: u32,
        phase: RetainedHumanPhase,
        command: HumanCommand,
    ) -> bool {
        if self.signal.close.load(Ordering::Acquire) || self.signal.stop.load(Ordering::Acquire) {
            return false;
        }
        let Ok(mut projection) = self.signal.projection.lock() else {
            return false;
        };
        if projection.human_command.is_some()
            || projection.human.is_none_or(|human| {
                human.generation != generation
                    || human.phase != phase
                    || Instant::now() >= human.deadline
            })
        {
            return false;
        }
        projection.human_command = Some(command);
        drop(projection);
        self.callback.dispatch(Command::WorkWake)
    }
    pub fn resume_after_human(&self, prepared: PreparedRetainedContinuation) -> bool {
        if self.signal.close.load(Ordering::Acquire) || self.signal.stop.load(Ordering::Acquire) {
            return false;
        }
        let Ok(mut projection) = self.signal.projection.lock() else {
            return false;
        };
        if projection.continuation.is_some()
            || projection.human.is_none_or(|human| {
                human.generation != prepared.generation
                    || human.phase != RetainedHumanPhase::ReadyToResume
                    || Instant::now() >= human.deadline
            })
        {
            return false;
        }
        projection.continuation = Some(prepared);
        drop(projection);
        self.callback.dispatch(Command::WorkWake)
    }
    /// Request destruction of this exact owned resource after scoped drain.
    /// Queue acceptance is not a cleanup acknowledgement.
    pub fn close(&self) -> bool {
        self.signal.close.store(true, Ordering::Release);
        self.stop()
    }
    /// Set only by the original resource owner's native shutdown proof.
    pub fn is_closed(&self) -> bool {
        self.signal.closed.load(Ordering::Acquire)
    }
    /// Scoped worker, resource and journal cleanup is complete. The Shell still
    /// owns the group-wide native audit; this never substitutes for is_closed.
    pub fn is_group_locally_retired(&self) -> bool {
        self.signal.group_locally_retired.load(Ordering::Acquire)
    }
    pub fn snapshot(&self) -> RetainedWorkSnapshot {
        match self.signal.projection.lock() {
            Ok(projection) => projection.snapshot,
            Err(error) => {
                let mut snapshot = error.into_inner().snapshot;
                snapshot.phase = RetainedWorkPhase::Uncertain;
                snapshot.usage = None;
                snapshot.failure = Some(AgentWorkFailure::Contract);
                snapshot
            }
        }
    }
    /// The newest canvas frame of the hosted page; replaced in place, never queued.
    pub fn frame(&self) -> Option<Arc<zephium_agentic::WorkBrowserFrame>> {
        self.signal.projection.lock().ok()?.frame.clone()
    }
    pub fn take_event(&self) -> Option<AgentWorkEvent> {
        let event = self.signal.projection.lock().ok()?.events.pop_front();
        if event.is_some() {
            let _ = self.callback.dispatch(Command::WorkWake);
        }
        event
    }
    /// Moves source-bound ModelMapped content only after original scoped drain
    /// and durable Succeeded ACK. When requested at admission, this additionally
    /// requires atomic artifact publication; the archived result remains in Store.
    pub fn take_extraction(&self) -> Option<Box<SemanticOwnedExtractionResult>> {
        self.signal.projection.lock().ok()?.extraction.take()
    }
    /// Bounded durable inventory, including terminal records from prior launches.
    /// These facts never grant native or model execution authority.
    pub fn records(&self) -> Vec<AgentWorkRecord> {
        self.signal
            .projection
            .lock()
            .map(|value| value.records.clone())
            .unwrap_or_default()
    }
    /// Explicit human review of the exact claimed historical interruption, or
    /// of this process's own scoped recovery. Queue acceptance is not a Store
    /// acknowledgement or resumed execution.
    pub fn review(
        &self,
        record: AgentWorkRecord,
        decision: crate::AgentWorkReviewDecision,
    ) -> bool {
        let Ok(mut projection) = self.signal.projection.lock() else {
            return false;
        };
        if projection.snapshot.phase != RetainedWorkPhase::NeedsReview
            || projection.review_requested.is_some()
            || projection.review_active
            || !projection.records.contains(&record)
            || !super::application::reviewable(record.disposition())
        {
            return false;
        }
        projection.review_requested = Some((record, decision));
        projection.snapshot.last_review = None;
        drop(projection);
        self.callback.dispatch(Command::WorkWake)
    }
    /// Requests a stored result in the original selected profile. One pending
    /// read and one returned body are retained; consume before rereading.
    pub fn read_artifact(&self, record: AgentWorkRecord) -> bool {
        let Ok(mut projection) = self.signal.projection.lock() else {
            return false;
        };
        if projection.read_requested.is_some()
            || projection.read_active
            || projection.archived.is_some()
            || !matches!(
                projection.snapshot.phase,
                RetainedWorkPhase::Ready | RetainedWorkPhase::Terminal
            )
            || !projection.records.contains(&record)
            || record.disposition() != AgentWorkDisposition::Succeeded
        {
            return false;
        }
        projection.read_requested = Some(record);
        projection.snapshot.artifact_read = None;
        drop(projection);
        self.callback.dispatch(Command::WorkWake)
    }
    /// Moves archived ModelMapped data. It cannot resume an old run.
    pub fn take_archived_extraction(&self) -> Option<AgentWorkArchivedExtraction> {
        self.signal.projection.lock().ok()?.archived.take()
    }
    /// Requests actor cancellation; true acknowledges queue admission only.
    pub fn stop(&self) -> bool {
        self.signal.stop.store(true, Ordering::Release);
        self.callback.dispatch(Command::WorkWake)
    }
    /// Explicitly retries only the original bounded uncertain Store CAS.
    pub fn reconcile(&self) -> bool {
        self.signal.reconcile.store(true, Ordering::Release);
        self.callback.dispatch(Command::WorkWake)
    }
}

#[derive(Clone)]
pub struct RetainedWorkAttachment(Arc<Mutex<Option<ProductWork>>>);
impl std::fmt::Debug for RetainedWorkAttachment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RetainedWorkAttachment([owned, redacted])")
    }
}
impl CallbackHandle {
    /// Submits one dormant run. Shell rechecks original allocation identities
    /// and current selected-profile readiness before any native construction.
    pub fn attach_retained_work(
        &self,
        prepared: PreparedRetainedWork,
    ) -> Option<RetainedWorkHandle> {
        let signal = Arc::new(ProductSignal {
            close: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            group_locally_retired: AtomicBool::new(false),
            projection: Mutex::new(Projection {
                human: None,
                human_resume: None,
                human_command: None,
                continuation: None,
                #[cfg(feature = "work-execution-probe")]
                construction_resource: None,
                frame: None,
                snapshot: RetainedWorkSnapshot {
                    phase: RetainedWorkPhase::Attaching,
                    run: prepared.spec.identity.owner(),
                    record: None,
                    usage: None,
                    failure: None,
                    construction_timed_out: false,
                    persistence_failure: None,
                    artifact: None,
                    artifact_read: None,
                    last_review: None,
                },
                events: VecDeque::with_capacity(MAX_AGENT_WORK_EVENTS),
                extraction: None,
                archived: None,
                records: Vec::new(),
                read_requested: None,
                read_active: false,
                review_requested: None,
                review_active: false,
            }),
            stop: AtomicBool::new(false),
            reconcile: AtomicBool::new(false),
        });
        let mut prepared = prepared;
        let work = ProductWork {
            page: prepared.page.take(),
            runtime_group: None,
            group_shutdown: false,
            clock: prepared.spec.clock.clone(),
            deadline: prepared.spec.deadline,
            prepared: Some(prepared),
            coordinator: None,
            request: None,
            failed_owner: None,
            native_uncertain: false,
            deadline_expired: false,
            signal: signal.clone(),
            callback: self.clone(),
        };
        let attachment = RetainedWorkAttachment(Arc::new(Mutex::new(Some(work))));
        self.dispatch(Command::AttachRetainedWork(attachment))
            .then(|| RetainedWorkHandle {
                signal,
                callback: self.clone(),
            })
    }
}

pub(crate) struct ProductWork {
    page: Option<RetainedPageAdmission>,
    runtime_group: Option<RetainedWorkGroup>,
    group_shutdown: bool,
    prepared: Option<PreparedRetainedWork>,
    coordinator: Option<RetainedWork>,
    request: Option<ActorRequest>,
    // A native factory panic or uncertain construction admission cannot lose
    // its original owner or be promoted into a clean empty-native proof.
    failed_owner: Option<WorkResourceOwner>,
    native_uncertain: bool,
    deadline_expired: bool,
    clock: Arc<dyn TerraControllerClock>,
    deadline: Instant,
    signal: Arc<ProductSignal>,
    callback: CallbackHandle,
}
impl ProductWork {
    pub(crate) fn ready_for_group_shutdown(&self) -> bool {
        self.is_closed()
            || self
                .coordinator
                .as_ref()
                .is_some_and(RetainedWork::ready_for_group_shutdown)
    }
    pub(crate) fn allow_group_shutdown(&mut self) {
        self.group_shutdown = true;
        if let Some(coordinator) = &mut self.coordinator {
            coordinator.allow_group_shutdown();
        }
    }
    pub(crate) fn new_runtime_group(
        &self,
    ) -> Result<RetainedWorkGroup, zephium_agent_runtime::RuntimeSpawnError> {
        let page = self
            .page
            .as_ref()
            .ok_or(zephium_agent_runtime::RuntimeSpawnError::Group)?;
        RetainedWorkGroup::try_new(page.work, page.workers.min(3))
    }
    pub(crate) fn set_runtime_group(&mut self, group: RetainedWorkGroup) {
        self.runtime_group = Some(group);
    }
    pub(crate) fn is_page(&self) -> bool {
        self.page.is_some()
    }
    /// Live peers and settled members all hold a native seat in the group; a
    /// settled member's step may be read again.
    pub(crate) fn admits_peers<'a>(
        &self,
        live: &[ProductWork],
        settled: impl Iterator<Item = &'a ProductWork>,
    ) -> bool {
        let Some(page) = &self.page else {
            return false;
        };
        let admits = |peer: &ProductWork, live: bool| {
            !peer.group_shutdown
                && peer.page.as_ref().is_some_and(|other| {
                    page.same_group(other) && (!live || page.step != other.step)
                })
        };
        let mut seats = live.len();
        Instant::now() < page.deadline
            && live.iter().all(|peer| admits(peer, true))
            && settled.into_iter().all(|peer| {
                seats += 1;
                admits(peer, false)
            })
            && seats < usize::from(page.workers.min(3))
    }
    /// An unclosed page that owns a native resource owner in its group.
    pub(crate) fn native_member(&self) -> bool {
        self.page.is_some() && self.coordinator.is_some() && !self.is_closed()
    }
    /// A member whose native audit has not ended; it can still close.
    pub(crate) fn awaits_native_close(&self) -> bool {
        self.native_member()
            && !self
                .coordinator
                .as_ref()
                .is_some_and(RetainedWork::native_audit_settled)
    }
    pub(crate) fn is_closed(&self) -> bool {
        self.signal.closed.load(Ordering::Acquire)
    }
    /// The durable runtime gave this work up while its coordinator cannot
    /// prove a clean close. It keeps its debt and its shutdown duty, but it
    /// must not hold the one retained slot against fresh pages.
    pub(crate) fn is_stuck(&self) -> bool {
        (self.signal.close.load(Ordering::Acquire) || Instant::now() >= self.deadline)
            && !self.is_closed()
            && self.signal.projection.lock().is_ok_and(|projection| {
                matches!(
                    projection.snapshot.phase,
                    RetainedWorkPhase::Uncertain | RetainedWorkPhase::Refused
                )
            })
    }
    /// A settled page that can no longer close with its group: stuck, or its
    /// native audit already ended without a clean close.
    pub(crate) fn leaves_group(&self) -> bool {
        !self.is_closed()
            && (self.is_stuck()
                || self
                    .coordinator
                    .as_ref()
                    .is_some_and(RetainedWork::native_audit_settled))
    }
    /// Drops this page's share of the group runtime slot; its audit stays
    /// gated on an explicit turn.
    pub(crate) fn leave_group(&mut self) {
        self.runtime_group = None;
        if let Some(coordinator) = &mut self.coordinator {
            coordinator.leave_group();
        }
    }
    pub(crate) fn holds_native_audit(&self) -> bool {
        !self.is_closed()
            && self
                .coordinator
                .as_ref()
                .is_some_and(RetainedWork::holds_native_audit)
    }
    pub(crate) fn awaits_group_audit(&self) -> bool {
        !self.is_closed()
            && self
                .coordinator
                .as_ref()
                .is_some_and(RetainedWork::awaits_group_audit)
    }
    pub(crate) fn take(attachment: &RetainedWorkAttachment) -> Option<Self> {
        attachment.0.lock().ok()?.take()
    }
    pub(crate) fn admits(
        &self,
        engine: &crate::SharedEngine,
        store: &crate::SharedStore,
        readiness: AgentWorkProfileReadiness,
    ) -> bool {
        self.prepared.as_ref().is_some_and(|prepared| {
            Arc::ptr_eq(engine, &prepared.engine)
                && std::ptr::addr_eq(Arc::as_ptr(store), Arc::as_ptr(&prepared.journal))
                && std::ptr::addr_eq(Arc::as_ptr(store), Arc::as_ptr(&prepared.audit))
                && readiness == AgentWorkProfileReadiness::Ready(prepared.profile)
                && self.deadline > Instant::now()
                && !self.signal.stop.load(Ordering::Acquire)
        })
    }
    pub(crate) fn refuse(&mut self) {
        self.prepared.take();
        if let Ok(mut projection) = self.signal.projection.lock() {
            projection.snapshot.phase = RetainedWorkPhase::Refused;
        }
    }
    pub(crate) fn initialize(&mut self) {
        let Some(prepared) = self.prepared.take() else {
            return;
        };
        let Ok(now) = self.clock.now() else {
            self.refuse();
            return;
        };
        if now >= prepared.spec.expires_at || Instant::now() >= self.deadline {
            self.refuse();
            return;
        }
        let callback = self.callback.clone();
        let owner = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            WorkResourceOwner::new(
                prepared.work.unwrap_or_else(WorkId::generate),
                prepared.profile.profile(),
                Arc::new(move || callback.wake_retained_work()),
                prepared.native,
            )
        }));
        let Ok(Some(owner)) = owner else {
            self.native_uncertain = true;
            self.fail();
            return;
        };
        match owner.construct_isolated(
            WorkBrowserResourceId::generate(),
            prepared.spec.identity.id(),
            prepared.spec.storage,
            prepared.spec.target,
            prepared.spec.document_policy,
            prepared.spec.isolated_public,
            prepared.anonymous_session,
            Some((prepared.construction_attempt, prepared.spec.deadline)),
            now,
        ) {
            Ok(pending) => {
                #[cfg(feature = "work-execution-probe")]
                if let Ok(mut projection) = self.signal.projection.lock() {
                    projection.construction_resource = Some(pending.resource.join.clone());
                }
                self.coordinator = Some(RetainedWork::constructing(
                    owner,
                    pending,
                    prepared.journal,
                    prepared.audit,
                ));
                if let Some(group) = self.runtime_group.take() {
                    if let Some(coordinator) = self.coordinator.as_mut() {
                        coordinator.set_runtime_group(group);
                    }
                }
                self.request = Some(prepared.actor);
            }
            Err(_) => {
                self.failed_owner = Some(owner);
                self.native_uncertain = true;
                self.fail();
            }
        }
        self.poll();
    }
    fn fail(&mut self) {
        self.signal.stop.store(true, Ordering::Release);
        self.request.take();
        if let Some(work) = &mut self.coordinator {
            work.cancel();
        }
        if let Ok(mut projection) = self.signal.projection.lock() {
            projection.snapshot.phase = RetainedWorkPhase::Uncertain;
            projection.snapshot.failure = Some(AgentWorkFailure::ContextLost);
        }
    }
    pub(crate) fn poll(&mut self) {
        let Ok(now) = self.clock.now() else {
            self.fail();
            return;
        };
        if self.signal.projection.is_poisoned() {
            self.fail();
            return;
        }
        let Some(work) = &mut self.coordinator else {
            return;
        };
        if !self.deadline_expired && Instant::now() >= self.deadline {
            self.deadline_expired = true;
            self.signal.stop.store(true, Ordering::Release);
        }
        if work
            .human_deadline()
            .is_some_and(|deadline| Instant::now() >= deadline)
            || work
                .human_snapshot()
                .is_some_and(|human| human.phase == RetainedHumanPhase::Released)
        {
            self.signal.close.store(true, Ordering::Release);
        }
        if self.signal.stop.load(Ordering::Acquire) {
            self.request.take();
            work.cancel();
        }
        if self.signal.reconcile.swap(false, Ordering::AcqRel) {
            work.reconcile();
        }
        let (human_command, continuation) = match self.signal.projection.lock() {
            Ok(mut projection) => (
                projection.human_command.take(),
                projection.continuation.take(),
            ),
            Err(_) => return,
        };
        if !self.signal.close.load(Ordering::Acquire) && !self.signal.stop.load(Ordering::Acquire) {
            if let Some(command) = human_command {
                let accepted = match command {
                    HumanCommand::Present(generation, region) => {
                        work.present_human(generation, region, now)
                    }
                    HumanCommand::Continue(generation) => work.continue_human(generation, now),
                };
                if !accepted {
                    self.signal.close.store(true, Ordering::Release);
                }
            }
            if let Some(continuation) = continuation {
                let valid = self.request.is_none()
                    && work.human_resume().is_some_and(|resume| {
                        resume.generation == continuation.generation
                            && resume.context == continuation.spec.identity.id()
                            && resume.document == continuation.spec.target
                    })
                    && continuation.spec.deadline <= self.deadline
                    && continuation.spec.deadline > Instant::now();
                if valid {
                    if let Ok(mut projection) = self.signal.projection.lock() {
                        projection.snapshot.run = continuation.actor.run;
                    }
                    self.request = Some(continuation.actor);
                } else {
                    self.signal.close.store(true, Ordering::Release);
                }
            }
        }
        let closed = if self.signal.close.load(Ordering::Acquire) {
            work.begin_shutdown();
            matches!(work.poll_shutdown(now), Ok(true))
        } else {
            work.poll(now);
            false
        };
        if self.page.is_some() && self.signal.close.load(Ordering::Acquire) && work.locally_closed()
        {
            self.signal
                .group_locally_retired
                .store(true, Ordering::Release);
        }
        if let Ok(mut projection) = self.signal.projection.lock() {
            if let Some((record, decision)) = projection.review_requested.take() {
                work.review(record, decision);
                projection.review_active = true;
            }
            if let Some(record) = projection.read_requested {
                if projection.archived.is_none() && work.read_artifact(record) {
                    projection.read_requested = None;
                    projection.read_active = true;
                }
            }
        }
        if work.ready() {
            if let Some(request) = self.request.take() {
                if let Err(request) = work.submit(request, now) {
                    self.request = Some(request);
                }
            }
        }
        let Ok(mut projection) = self.signal.projection.lock() else {
            return;
        };
        while projection.events.len() < MAX_AGENT_WORK_EVENTS {
            let Some(event) = work.take_event() else {
                break;
            };
            projection.events.push_back(event);
        }
        work.start_human_wait(self.deadline, now);
        projection.human = work.human_snapshot();
        projection.human_resume = work.human_resume();
        projection.frame = work.latest_frame();
        #[cfg(feature = "work-execution-probe")]
        if work.phase() == AdmissionPhase::Terminal
            && projection.snapshot.phase != RetainedWorkPhase::Terminal
        {
            work.public_retention_diagnostic();
        }
        projection.snapshot.phase = match work.phase() {
            AdmissionPhase::Constructing => RetainedWorkPhase::Constructing,
            AdmissionPhase::Loading => RetainedWorkPhase::Loading,
            AdmissionPhase::NeedsReview => RetainedWorkPhase::NeedsReview,
            AdmissionPhase::Reviewing => RetainedWorkPhase::Reviewing,
            AdmissionPhase::Ready => RetainedWorkPhase::Ready,
            AdmissionPhase::Acquiring => RetainedWorkPhase::Acquiring,
            AdmissionPhase::Admitting => RetainedWorkPhase::Admitting,
            AdmissionPhase::Starting => RetainedWorkPhase::Starting,
            AdmissionPhase::Running => RetainedWorkPhase::Running,
            AdmissionPhase::Closing => RetainedWorkPhase::Closing,
            AdmissionPhase::Terminal => RetainedWorkPhase::Terminal,
            AdmissionPhase::Uncertain => RetainedWorkPhase::Uncertain,
        };
        projection.snapshot.construction_timed_out |= work.construction_timed_out();
        projection.snapshot.record = work.record();
        projection.snapshot.usage = work.usage();
        projection.snapshot.artifact = work.artifact();
        projection.snapshot.artifact_read = work.artifact_read();
        projection.snapshot.last_review = work.last_review();
        if projection.snapshot.last_review.is_some() {
            projection.review_active = false;
        }
        if projection.snapshot.artifact_read.is_some() {
            projection.read_active = false;
        }
        if projection.records.as_slice() != work.records() {
            projection.records.clear();
            projection.records.extend_from_slice(work.records());
        }
        if projection.archived.is_none() {
            projection.archived = work.take_archived_extraction();
        }
        (
            projection.snapshot.failure,
            projection.snapshot.persistence_failure,
        ) = work.failures();
        if projection.extraction.is_none() {
            projection.extraction = work.take_extraction();
        }
        drop(projection);
        if closed {
            // Publish closure only after the terminal usage and artifact projection.
            self.signal.closed.store(true, Ordering::Release);
        }
    }
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.coordinator
            .as_ref()
            .and_then(RetainedWork::next_deadline)
            .into_iter()
            .chain(
                self.coordinator
                    .as_ref()
                    .and_then(RetainedWork::human_deadline),
            )
            .chain(
                (!self.deadline_expired
                    && self
                        .coordinator
                        .as_ref()
                        .is_some_and(|work| work.phase() != AdmissionPhase::Terminal))
                .then_some(self.deadline),
            )
            .min()
    }
    pub(crate) fn begin_shutdown(&mut self) {
        self.prepared.take();
        self.request.take();
        self.signal.close.store(true, Ordering::Release);
        self.signal.stop.store(true, Ordering::Release);
        if let Some(work) = &mut self.coordinator {
            work.begin_shutdown();
        }
    }
    pub(crate) fn shutdown_until(&mut self, deadline: Instant) -> bool {
        self.begin_shutdown();
        let clean = self
            .coordinator
            .as_mut()
            .is_none_or(|work| work.shutdown_until(self.clock.as_ref(), deadline));
        clean && !self.native_uncertain && self.failed_owner.is_none()
    }
}

impl Drop for ProductWork {
    fn drop(&mut self) {
        // An unprocessed attachment never created external ownership. Report
        // refusal when the queue/actor discards it instead of leaving Attaching.
        if self.prepared.is_some() {
            self.refuse();
        }
    }
}
