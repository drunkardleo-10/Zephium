//! Shipping single-page retained Work attachment. No UI or native authority in handles.
use super::application::{ActorRequest, AdmissionPhase, RetainedWork, StagedActor};
use super::*;
use crate::{AgentWorkProfileBinding, AgentWorkProfileReadiness, CallbackHandle, Command};
use std::collections::VecDeque;
use std::time::Instant;
use zephium_agent_controller::*;
use zephium_agent_provider_transport::AgentProviderCredential;

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
    engine: crate::SharedEngine,
    journal: Arc<dyn AgentWorkJournalPort>,
    audit: Arc<dyn AgentAuditPort>,
    native: RetainedWorkNativeFactory,
    profile: AgentWorkProfileBinding,
    spec: AgentWorkRetainedResourceSpec,
    actor: ActorRequest,
}
impl PreparedRetainedWork {
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
            prepare: Box::new(move |browser, audit| {
                StagedActor::try_new(input, browser, runtime, provider, credential, audit, task)
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
    pub failure: Option<AgentWorkFailure>,
    pub persistence_failure: Option<AgentWorkJournalError>,
    /// Atomic durable result identity, present only after publication ACK.
    pub artifact: Option<AgentWorkArtifactDescriptor>,
    /// Explicit archived-result read status. Content is retrieved separately.
    pub artifact_read: Option<Result<bool, AgentWorkJournalError>>,
    /// Exact historical review acknowledgement; its debt is never cleared.
    pub last_review: Option<Result<AgentWorkRecord, AgentWorkJournalError>>,
}
struct Projection {
    #[cfg(feature = "work-execution-probe")]
    construction_resource: Option<WorkBrowserResourceJoin>,
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
    pub fn snapshot(&self) -> RetainedWorkSnapshot {
        match self.signal.projection.lock() {
            Ok(projection) => projection.snapshot,
            Err(error) => {
                let mut snapshot = error.into_inner().snapshot;
                snapshot.phase = RetainedWorkPhase::Uncertain;
                snapshot.failure = Some(AgentWorkFailure::Contract);
                snapshot
            }
        }
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
    /// Explicit human review of the exact claimed historical interruption.
    /// Queue acceptance is not a Store acknowledgement or resumed execution.
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
            || record.disposition() != AgentWorkDisposition::Interrupted
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
            projection: Mutex::new(Projection {
                #[cfg(feature = "work-execution-probe")]
                construction_resource: None,
                snapshot: RetainedWorkSnapshot {
                    phase: RetainedWorkPhase::Attaching,
                    run: prepared.spec.identity.owner(),
                    record: None,
                    failure: None,
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
        let work = ProductWork {
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
                WorkId::generate(),
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
        match owner.construct_with_policy(
            WorkBrowserResourceId::generate(),
            prepared.spec.identity.id(),
            prepared.spec.storage,
            prepared.spec.target,
            prepared.spec.document_policy,
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
        if self.signal.stop.load(Ordering::Acquire) {
            self.request.take();
            work.cancel();
        }
        if self.signal.reconcile.swap(false, Ordering::AcqRel) {
            work.reconcile();
        }
        work.poll(now);
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
        projection.snapshot.record = work.record();
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
    }
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.coordinator
            .as_ref()
            .and_then(RetainedWork::next_deadline)
            .into_iter()
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
