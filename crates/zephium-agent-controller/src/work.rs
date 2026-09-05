//! Product-owned execution and bounded content-free projection.

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use zephium_agent_provider_transport::{
    AgentProviderCredential, AgentProviderTransport, AgentProviderTransportConfig,
};
use zephium_agent_runtime::{
    AgentRuntimeBrowser, AgentRuntimeController, AgentRuntimeControllerFuture,
    AgentRuntimeControllerTerminalClass, AgentRuntimeControllerTerminalRefusal, AgentRuntimeEvent,
    AgentRuntimeStopReason, AgentRuntimeWorker,
};
use zephium_agentic::*;

use super::{
    AgentBrowserModel, AgentBrowserProviderError, AgentBrowserProviderTurn, AgentBrowserRetention,
    AgentBrowserSession, TerraControllerClock, TerraControllerIds, TerraControllerRunInput,
};

#[cfg(test)]
#[path = "work_tests.rs"]
mod tests;

/// Trusted task-level decision from fresh independently collected state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentWorkTaskProgress {
    /// The approved task still requires work.
    Continue,
    /// The product's own task predicate is satisfied.
    Complete,
}

/// Trusted product execution contract. Never implement this from model text,
/// page instructions, or a model-authored predicate. The UI does not receive
/// this port; it receives only the content-free handle below.
pub trait AgentWorkTask: Send {
    /// Optional single trusted extraction schema. Identity 1 is run-local;
    /// model/page content cannot register or replace it. Default is no extraction.
    fn extraction_schema(&self) -> Option<&SemanticExtractionSchema> {
        None
    }
    /// Accepts a validated but explicitly model-mapped result against the
    /// trusted task contract. Default refuses; shape alone is not task authority.
    fn accept_extraction(
        &mut self,
        _: &SemanticExtractionResult<'_>,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        Err(AgentWorkFailure::Contract)
    }
    /// Evaluates the approved task against fresh semantic state.
    fn evaluate(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure>;
    /// Independently classifies the actual bounded native effect.
    fn assess(
        &self,
        action: &SemanticPreparedAction,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure>;
    /// Supplies the trusted account attestation for this exact current context.
    fn attest_account(
        &self,
        context: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentContextAccountBinding, AgentWorkFailure>;
}

/// Explicit Work-owned page and authoritatively selected profile storage.
/// This does not accept ordinary Browse tabs or load user extensions.
pub struct AgentWorkContextSpec {
    identity: ContextIdentity,
    storage: ContextProfileStorageClass,
    target: ContextNavigationTarget,
    origin: SemanticOrigin,
}

impl AgentWorkContextSpec {
    /// Validates one owned page and exact initial target; manifest scope is
    /// independently checked when constructing the run.
    pub fn try_new(
        identity: ContextIdentity,
        storage: ContextProfileStorageClass,
        target: ContextNavigationTarget,
    ) -> Result<Self, AgentWorkFailure> {
        if identity.kind() != ContextKind::Owned {
            return Err(AgentWorkFailure::Contract);
        }
        let origin = SemanticOrigin::parse(target.as_url().as_ref())
            .map_err(|_| AgentWorkFailure::Contract)?;
        Ok(Self {
            identity,
            storage,
            target,
            origin,
        })
    }
}

/// Bounded model/run configuration supplied by the trusted application.
pub struct AgentWorkRunSettings {
    model: AgentBrowserModel,
    ids: TerraControllerIds,
    clock: Arc<dyn TerraControllerClock>,
    deadline: Instant,
}

impl AgentWorkRunSettings {
    /// Selects the same catalog model, identifiers, clock and absolute deadline
    /// used by the shared provider/session architecture.
    pub fn new(
        model: AgentBrowserModel,
        ids: TerraControllerIds,
        clock: Arc<dyn TerraControllerClock>,
        deadline: Instant,
    ) -> Self {
        Self {
            model,
            ids,
            clock,
            deadline,
        }
    }
}

/// Approved product input, admitted before native or provider work begins.
#[must_use]
pub struct AgentWorkRunInput {
    manifest: AgentRunManifest,
    lease: AgentPlanLeaseBinding,
    context: AgentWorkContextSpec,
    objective: Option<AgentProviderObjective>,
    settings: AgentWorkRunSettings,
    durable_result: bool,
}

impl AgentWorkRunInput {
    /// Joins approved scope, explicit profile, bounded objective and model.
    pub fn try_new(
        manifest: AgentRunManifest,
        lease: AgentPlanLeaseBinding,
        context: AgentWorkContextSpec,
        objective: String,
        settings: AgentWorkRunSettings,
    ) -> Result<Self, AgentWorkFailure> {
        let horizon = settings
            .deadline
            .checked_duration_since(Instant::now())
            .ok_or(AgentWorkFailure::Deadline)?;
        if horizon.is_zero() || horizon > super::MAX_TERRA_CONTROLLER_HARD_DEADLINE {
            return Err(AgentWorkFailure::Deadline);
        }
        let root = manifest
            .plan_node(lease.node())
            .ok_or(AgentWorkFailure::Contract)?;
        let now = settings
            .clock
            .now()
            .map_err(|_| AgentWorkFailure::Contract)?;
        let remaining = root
            .expires_at()
            .millis()
            .checked_sub(now.millis())
            .ok_or(AgentWorkFailure::Deadline)?;
        if now < manifest.issued_at() || horizon > Duration::from_millis(remaining) {
            return Err(AgentWorkFailure::Deadline);
        }
        if manifest.plan_nodes().len() != 1
            || manifest.run() != context.identity.owner()
            || !root.profiles().contains(&context.identity.profile())
            || !root.origins().contains(&context.origin)
        {
            return Err(AgentWorkFailure::Contract);
        }
        let config = match settings.model {
            AgentBrowserModel::Terra => super::try_terra_provider_exact_call_config(
                super::TERRA_CONTROLLER_MAX_OUTPUT_TOKENS,
            )
            .map_err(|_| AgentWorkFailure::Contract)?,
            AgentBrowserModel::Luna => super::try_luna_provider_exact_call_config(
                super::TERRA_CONTROLLER_MAX_OUTPUT_TOKENS,
            )
            .map_err(|_| AgentWorkFailure::Contract)?,
        };
        let objective =
            AgentProviderObjective::try_admit_conservative_utf8(objective, config.tokenizer())
                .map_err(|_| AgentWorkFailure::Contract)?;
        Ok(Self {
            manifest,
            lease,
            context,
            objective: Some(objective),
            settings,
            durable_result: false,
        })
    }
    /// Explicitly opts a trusted extraction task into profile-owned result
    /// persistence. Private/ephemeral contexts cannot silently write artifacts.
    pub fn persist_extraction_result(mut self) -> Result<Self, AgentWorkFailure> {
        if self.context.storage != ContextProfileStorageClass::Durable {
            return Err(AgentWorkFailure::Contract);
        }
        self.durable_result = true;
        Ok(self)
    }
    fn admission(
        &self,
        owner: AgentWorkIncarnation,
    ) -> Result<AgentWorkJournalMutation, AgentWorkFailure> {
        if self.durable_result {
            AgentWorkJournalMutation::admit_with_result(
                &self.manifest,
                owner,
                self.context.identity.profile(),
            )
            .map_err(|_| AgentWorkFailure::Contract)
        } else {
            Ok(AgentWorkJournalMutation::admit(&self.manifest, owner))
        }
    }
}

/// Content-free shell/application observation port. It exposes no browser,
/// native handles, credentials, page data or mutable policy authority.
pub struct AgentWorkHandle {
    events: Arc<Mutex<WorkEvents>>,
    terminal: Arc<Mutex<Option<AgentWorkOutcome>>>,
}

impl AgentWorkHandle {
    /// Registers one content-free, nonblocking application wake. The consumer
    /// must enqueue work without re-entering this handle from its waker.
    pub fn set_waker(&self, waker: std::task::Waker) {
        let mut events = lock(&self.events);
        events.waker = Some(waker);
        if !events.queue.is_empty() {
            events.wake();
        }
    }
    /// Removes the oldest bounded event, preserving its stable sequence.
    pub fn take_event(&self) -> Option<AgentWorkEvent> {
        lock(&self.events).queue.pop_front()
    }
    /// Moves the sole terminal/recovery owner out exactly once.
    pub fn take_outcome(&mut self) -> Option<AgentWorkOutcome> {
        lock(&self.terminal).take()
    }
}

/// Product outcome; recovery explicitly retains every unresolved core owner.
#[must_use]
pub enum AgentWorkOutcome {
    /// Trusted task completion with durable policy/accounting closure. Native
    /// and provider shutdown proofs are consumed by the runtime lifecycle.
    Succeeded(AgentWorkSuccess),
    /// Task failed or was cancelled, but all original execution owners drained.
    /// This never carries an extraction result or authorizes another run.
    ClosedUnsuccessfully(AgentWorkClosedUnsuccessfully),
    /// Execution stopped without enough evidence for clean closure.
    Recovery(AgentWorkRecovery),
}

/// Clean execution ownership plus an optional bounded model-mapped result.
/// Result data is not a native-effect, factual-verification or durability proof.
pub struct AgentWorkSuccess {
    settlement: AgentRunPolicySettlement,
    extraction: Option<Box<SemanticOwnedExtractionResult>>,
}

/// Unsuccessful business outcome with original policy/audit closure. Native
/// and provider proofs remain owned by the exact runtime lifecycle join.
pub struct AgentWorkClosedUnsuccessfully {
    settlement: AgentRunPolicySettlement,
    failure: AgentWorkFailure,
}

impl AgentWorkClosedUnsuccessfully {
    /// Original typed cause; clean resource drain does not mean task success.
    pub const fn failure(&self) -> AgentWorkFailure {
        self.failure
    }
    /// Exact failed/cancelled policy closure, never a replacement proof.
    pub const fn policy_settlement(&self) -> AgentRunPolicySettlement {
        self.settlement
    }
}
impl AgentWorkSuccess {
    /// Borrowed private result for exact terminal/artifact publication only.
    pub fn extraction(&self) -> Option<&SemanticOwnedExtractionResult> {
        self.extraction.as_deref()
    }
    /// Original content-free closure metrics, not result factual verification.
    pub const fn closure(&self) -> AgentRunMetricClosure {
        self.settlement.closure()
    }
    /// Original successful policy/audit closure; never a replacement proof.
    pub const fn policy_settlement(&self) -> AgentRunPolicySettlement {
        self.settlement
    }
    /// Moves the result once. Application adapters must additionally gate
    /// publication on their original clean lifecycle and durable terminal ACK.
    pub fn take_extraction(&mut self) -> Option<SemanticOwnedExtractionResult> {
        self.extraction.take().map(|result| *result)
    }
}

/// Trusted public-data extraction task over one initial observation. The
/// product supplies the schema before admission; no native action is allowed.
pub struct AgentWorkExtractionTask {
    schema: SemanticExtractionSchema,
    account: AgentAccountScope,
}
impl AgentWorkExtractionTask {
    /// Registers one run-local schema and explicitly trusted account scope.
    pub fn try_new(
        fields: Vec<SemanticExtractionFieldSchema>,
        account: AgentAccountScope,
    ) -> Result<Self, AgentWorkFailure> {
        let id = SemanticExtractionSchemaId::new(1).ok_or(AgentWorkFailure::Contract)?;
        let schema = SemanticExtractionSchema::try_new(id, fields)
            .map_err(|_| AgentWorkFailure::Contract)?;
        Ok(Self { schema, account })
    }
}
impl AgentWorkTask for AgentWorkExtractionTask {
    fn extraction_schema(&self) -> Option<&SemanticExtractionSchema> {
        Some(&self.schema)
    }
    fn evaluate(
        &mut self,
        _: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        Ok(AgentWorkTaskProgress::Continue)
    }
    fn assess(
        &self,
        _: &SemanticPreparedAction,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        Err(AgentWorkFailure::Contract)
    }
    fn attest_account(
        &self,
        context: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        Ok(AgentContextAccountBinding::new(
            AgentAccountAttestationId::generate(),
            context,
            self.account,
            now,
        ))
    }
    fn accept_extraction(
        &mut self,
        result: &SemanticExtractionResult<'_>,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        if result.schema() != self.schema.id() || result.stats().sensitive_source_edges() != 0 {
            return Err(AgentWorkFailure::Contract);
        }
        // Core validated schema, types, bounds, source delivery and secret
        // rejection. Completion means a model-mapped result, not proven truth.
        Ok(AgentWorkTaskProgress::Complete)
    }
}

/// Opaque, content-redacted recovery ownership; never permission to replay.
#[must_use]
pub struct AgentWorkRecovery {
    failure: AgentWorkFailure,
    state: Box<WorkState>,
}

impl AgentWorkRecovery {
    /// Content-free admission identity retained when execution never started.
    /// Recovery of a consumed input cannot create a replacement admission.
    pub fn journal_admission(
        &self,
        owner: AgentWorkIncarnation,
    ) -> Result<AgentWorkJournalMutation, AgentWorkFailure> {
        let input = self
            .state
            .input
            .as_ref()
            .ok_or(AgentWorkFailure::Contract)?;
        input.admission(owner)
    }
    /// Seals the original retained ledger and prepares its exact pending batch,
    /// or the next never-dispatched batch. This grants no browser/provider work.
    pub fn prepare_audit_reconciliation(
        &mut self,
    ) -> Result<Option<AgentAuditDelivery>, AgentWorkFailure> {
        let audit = &mut self.state.journal_mut()?.audit;
        audit
            .seal_for_shutdown()
            .map_err(|_| AgentWorkFailure::Audit)?;
        if let Some(delivery) = audit
            .current_delivery()
            .map_err(|_| AgentWorkFailure::Audit)?
        {
            return Ok(Some(delivery));
        }
        if audit.is_quiescent() {
            return Ok(None);
        }
        audit
            .begin_next_delivery(MAX_AGENT_AUDIT_DELIVERY_EVENTS)
            .map(Some)
            .map_err(|_| AgentWorkFailure::Audit)
    }

    /// Applies only the exact original ledger's delivery receipt. Audit drain
    /// never clears native, provider, policy or runtime recovery obligations.
    pub fn settle_audit_reconciliation(
        &mut self,
        settlement: AgentAuditDeliverySettlement,
    ) -> Result<AgentAuditDeliveryOutcome, AgentWorkFailure> {
        self.state
            .journal_mut()?
            .audit
            .settle_delivery(settlement)
            .map_err(|_| AgentWorkFailure::Audit)
    }

    /// Content-free status of the original retained audit ledger.
    pub fn audit_reconciliation_status(
        &mut self,
    ) -> Result<AgentAuditLedgerStatus, AgentWorkFailure> {
        Ok(self.state.journal_mut()?.audit.status())
    }
    /// Exact closed stop reason.
    pub const fn failure(&self) -> AgentWorkFailure {
        self.failure
    }
    /// Count of retained unsolicited/terminal callbacks requiring reconciliation.
    pub fn retained_callbacks(&self) -> usize {
        self.state.native.deferred.len()
    }
}

impl fmt::Debug for AgentWorkOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Succeeded(_) => formatter.write_str("AgentWorkOutcome::Succeeded"),
            Self::ClosedUnsuccessfully(value) => formatter
                .debug_tuple("AgentWorkOutcome::ClosedUnsuccessfully")
                .field(&value.failure)
                .finish(),
            Self::Recovery(value) => formatter
                .debug_tuple("AgentWorkOutcome::Recovery")
                .field(&value.failure)
                .finish(),
        }
    }
}

/// Shipping Work execution actor on the existing runtime worker and mailbox.
/// Construction is idle: provider/native requests begin only after RunStarted.
#[must_use]
pub struct AgentWorkController {
    state: Option<WorkState>,
    terminal: Arc<Mutex<Option<AgentWorkOutcome>>>,
}

impl AgentWorkController {
    /// Trusted durable destination, fixed before native/provider admission.
    pub fn durable_result_profile(&self) -> Result<Option<AgentWorkProfileId>, AgentWorkFailure> {
        let input = self
            .state
            .as_ref()
            .and_then(|state| state.input.as_ref())
            .ok_or(AgentWorkFailure::Contract)?;
        Ok(input
            .durable_result
            .then_some(input.context.identity.profile()))
    }
    /// Prepares content-free durable admission while this controller is still
    /// dormant. This does not start a provider request, runtime or native page.
    pub fn journal_admission(
        &self,
        owner: AgentWorkIncarnation,
    ) -> Result<AgentWorkJournalMutation, AgentWorkFailure> {
        let input = self
            .state
            .as_ref()
            .and_then(|state| state.input.as_ref())
            .ok_or(AgentWorkFailure::Contract)?;
        input.admission(owner)
    }

    /// Stable content-free run identity selected by the trusted input.
    pub fn run_identity(&self) -> Result<ContextRunId, AgentWorkFailure> {
        self.state
            .as_ref()
            .and_then(|state| state.input.as_ref())
            .map(|input| input.manifest.run())
            .ok_or(AgentWorkFailure::Contract)
    }

    /// Original bounded absolute deadline, never extended by persistence waits.
    pub fn deadline(&self) -> Result<Instant, AgentWorkFailure> {
        self.state
            .as_ref()
            .and_then(|state| state.input.as_ref())
            .map(|input| input.settings.deadline)
            .ok_or(AgentWorkFailure::Contract)
    }
}

impl AgentWorkController {
    /// Constructs the actor and content-free shell port. The application moves
    /// this actor into `PendingAgentRuntime::spawn_suspended_with_controller`,
    /// then binds the real engine port and owns the existing runtime controls.
    pub fn try_new(
        input: AgentWorkRunInput,
        transport: AgentProviderTransportConfig,
        credential: AgentProviderCredential,
        audit: Arc<dyn AgentAuditPort>,
        task: Box<dyn AgentWorkTask>,
    ) -> Result<(Self, AgentWorkHandle), AgentWorkFailure> {
        let transport = AgentProviderTransport::try_new(transport)
            .map_err(|_| AgentWorkFailure::Browser(AgentBrowserProviderError::Transport))?;
        Self::with_transport(
            input,
            transport,
            credential,
            audit,
            task,
            AgentBrowserRetention::Stateless,
        )
    }

    /// Release-excluded qualification adapter. The caller supplies a fresh
    /// dedicated transport and explicitly public retention mode; all runtime,
    /// policy, browser, audit and task-completion semantics remain identical.
    #[cfg(feature = "probe-harness")]
    pub fn try_new_for_probe(
        input: AgentWorkRunInput,
        transport: AgentProviderTransport,
        credential: AgentProviderCredential,
        audit: Arc<dyn AgentAuditPort>,
        task: Box<dyn AgentWorkTask>,
        retention: AgentBrowserRetention,
    ) -> Result<(Self, AgentWorkHandle), AgentWorkFailure> {
        let snapshot = transport
            .snapshot()
            .map_err(|_| AgentWorkFailure::Contract)?;
        if snapshot.is_sealed() || !snapshot.is_idle() {
            return Err(AgentWorkFailure::Contract);
        }
        Self::with_transport(input, transport, credential, audit, task, retention)
    }

    fn with_transport(
        input: AgentWorkRunInput,
        transport: AgentProviderTransport,
        credential: AgentProviderCredential,
        audit: Arc<dyn AgentAuditPort>,
        task: Box<dyn AgentWorkTask>,
        retention: AgentBrowserRetention,
    ) -> Result<(Self, AgentWorkHandle), AgentWorkFailure> {
        if (input.durable_result && task.extraction_schema().is_none())
            || task
                .extraction_schema()
                .is_some_and(|schema| schema.id().get() != 1)
        {
            return Err(AgentWorkFailure::Contract);
        }
        let events = Arc::new(Mutex::new(WorkEvents::new(input.manifest.run())?));
        let journal = WorkJournal::new(
            &input.manifest,
            input.lease.node(),
            input.settings.ids.supervisor,
            input.settings.ids.cancellation,
            Arc::clone(&input.settings.clock),
            Arc::clone(&events),
        )?;
        let native = WorkNative::new(
            input.context.identity,
            input.context.origin.clone(),
            input.settings.deadline,
        )?;
        let terminal = Arc::new(Mutex::new(None));
        Ok((
            Self {
                state: Some(WorkState {
                    input: Some(input),
                    session: None,
                    drained: None,
                    journal: Some(journal),
                    native,
                    credential: Some(credential),
                    transport: Some(super::BrowserSessionTransport(transport)),
                    retention,
                    audit,
                    task,
                    extraction: None,
                    failure: None,
                    observation: None,
                    native_terminal: None,
                }),
                terminal: Arc::clone(&terminal),
            },
            AgentWorkHandle { events, terminal },
        ))
    }
}

struct WorkState {
    extraction: Option<SemanticOwnedExtractionResult>,
    input: Option<AgentWorkRunInput>,
    session: Option<AgentBrowserSession>,
    drained: Option<WorkDrained>,
    journal: Option<WorkJournal>,
    native: WorkNative,
    credential: Option<AgentProviderCredential>,
    transport: Option<super::BrowserSessionTransport>,
    retention: AgentBrowserRetention,
    audit: Arc<dyn AgentAuditPort>,
    task: Box<dyn AgentWorkTask>,
    failure: Option<AgentWorkFailure>,
    observation: Option<SemanticObservation>,
    native_terminal: Option<SemanticActionNativeSettlement>,
}

impl WorkState {
    fn journal_mut(&mut self) -> Result<&mut WorkJournal, AgentWorkFailure> {
        if let Some(session) = self.session.as_mut() {
            return session.journal.as_mut().ok_or(AgentWorkFailure::Contract);
        }
        if let Some(drained) = self.drained.as_mut() {
            return drained.journal.as_mut().ok_or(AgentWorkFailure::Contract);
        }
        self.journal.as_mut().ok_or(AgentWorkFailure::Contract)
    }
}

struct WorkDrained {
    policy: Option<AgentRunPolicy>,
    journal: Option<WorkJournal>,
    provider: Option<AgentProviderShutdownProof>,
    native: Option<AgentNativeShutdownCoordinator>,
    resources: Option<AgentNativeShutdownResources>,
    proof: Option<AgentNativeShutdownProof>,
}

struct WorkNative {
    resources: Option<WorkContextResources>,
    identity: ContextIdentity,
    origin: SemanticOrigin,
    profile: Option<ContextProfileLease>,
    operation: Option<ContextOperationJoin>,
    recovery_close: Option<ContextOperationJoin>,
    observation: Option<SemanticRuntimeCorrelation>,
    snapshot_generation: Option<SemanticSnapshotGeneration>,
    action_pending: bool,
    cancellation: Option<ContextJoin>,
    shutdown_audit: Option<ContextResourceAuditId>,
    close_attempted: bool,
    deferred: Vec<AgentRuntimeEvent>,
    next: u64,
    deadline: Instant,
    revoked: bool,
}

struct WorkContextResources {
    contexts: ContextRegistry,
    profiles: ContextProfileLeaseRegistry,
    cookies: ContextCookieTransferRegistry,
    screenshots: SemanticScreenshotCoordinator,
}

impl WorkNative {
    fn new(
        identity: ContextIdentity,
        origin: SemanticOrigin,
        deadline: Instant,
    ) -> Result<Self, AgentWorkFailure> {
        let mut deferred = Vec::new();
        deferred
            .try_reserve_exact(super::MAX_DEFERRED_RUNTIME_EVENTS)
            .map_err(|_| AgentWorkFailure::Backpressure)?;
        Ok(Self {
            resources: Some(WorkContextResources {
                contexts: ContextRegistry::new(),
                profiles: ContextProfileLeaseRegistry::new(),
                cookies: ContextCookieTransferRegistry::new(),
                screenshots: SemanticScreenshotCoordinator::new(),
            }),
            identity,
            origin,
            profile: None,
            operation: None,
            recovery_close: None,
            observation: None,
            snapshot_generation: None,
            action_pending: false,
            cancellation: None,
            shutdown_audit: None,
            close_attempted: false,
            deferred,
            next: 1,
            deadline,
            revoked: false,
        })
    }

    fn id(&mut self) -> Result<u64, AgentWorkFailure> {
        let id = self.next;
        self.next = id.checked_add(1).ok_or(AgentWorkFailure::Contract)?;
        Ok(id)
    }

    fn contexts(&mut self) -> Result<&mut ContextRegistry, AgentWorkFailure> {
        self.resources
            .as_mut()
            .map(|resources| &mut resources.contexts)
            .ok_or(AgentWorkFailure::Context)
    }
    fn profiles(&mut self) -> Result<&mut ContextProfileLeaseRegistry, AgentWorkFailure> {
        self.resources
            .as_mut()
            .map(|resources| &mut resources.profiles)
            .ok_or(AgentWorkFailure::Context)
    }

    fn retain(&mut self, event: AgentRuntimeEvent) -> Result<(), AgentWorkFailure> {
        if self.deferred.len() >= super::MAX_DEFERRED_RUNTIME_EVENTS {
            return Err(AgentWorkFailure::Backpressure);
        }
        self.deferred.push(event);
        Ok(())
    }

    fn revoke(&mut self, browser: &AgentRuntimeBrowser) -> Result<(), AgentWorkFailure> {
        if self.revoked {
            return Ok(());
        }
        self.revoked = true;
        let id = self.identity.id();
        let registry = self.contexts()?;
        let Ok(prior) = registry.join(id) else {
            return Ok(());
        };
        let current = registry
            .cancel_run(id, prior)
            .map_err(|_| AgentWorkFailure::Context)?;
        // Rust invalidation precedes native dispatch, including provider wait.
        if browser.dispatch(ContextNativeRequest::Cancel(
            ContextCancellationRequest::new(current),
        )) == ContextDispatch::Scheduled
        {
            self.cancellation = Some(current);
        } else {
            return Err(AgentWorkFailure::Context);
        }
        Ok(())
    }

    async fn next_event(
        &mut self,
        worker: &mut AgentRuntimeWorker,
        browser: &AgentRuntimeBrowser,
    ) -> Result<AgentRuntimeEvent, AgentWorkFailure> {
        let deadline = worker
            .shutdown_deadline()
            .map_or(self.deadline, |at| at.min(self.deadline));
        if Instant::now() >= deadline {
            self.revoke(browser)?;
            return Err(AgentWorkFailure::Deadline);
        }
        let event = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            worker.next_event(),
        )
        .await;
        let event = match event {
            Ok(Ok(event)) => event,
            Ok(Err(_)) => {
                self.revoke(browser)?;
                return Err(AgentWorkFailure::Mailbox);
            }
            Err(_) => {
                self.revoke(browser)?;
                return Err(AgentWorkFailure::Deadline);
            }
        };
        match event {
            AgentRuntimeEvent::CancellationRequested => {
                self.revoke(browser)?;
                Err(match worker.stop_reason() {
                    Some(AgentRuntimeStopReason::HumanTakeover) => AgentWorkFailure::HumanTakeover,
                    Some(AgentRuntimeStopReason::Suspend) => AgentWorkFailure::SuspendRequested,
                    Some(AgentRuntimeStopReason::PolicyRevoked) => AgentWorkFailure::PolicyRevoked,
                    _ => AgentWorkFailure::Cancelled,
                })
            }
            AgentRuntimeEvent::ShutdownRequested => {
                self.revoke(browser)?;
                Err(AgentWorkFailure::Shutdown)
            }
            AgentRuntimeEvent::NavigationReplaced(replacement) => {
                let id = self.identity.id();
                if replacement.prior().identity() == self.identity {
                    self.contexts()?
                        .observe_navigation_replacement(id, replacement.prior())
                        .map_err(|_| AgentWorkFailure::Context)?;
                }
                self.revoke(browser)?;
                self.retain(AgentRuntimeEvent::NavigationReplaced(replacement))?;
                Err(AgentWorkFailure::ContextLost)
            }
            AgentRuntimeEvent::RendererLost(loss) => {
                let id = self.identity.id();
                if loss.prior().identity() == self.identity {
                    self.contexts()?
                        .renderer_lost(id, loss.prior())
                        .map_err(|_| AgentWorkFailure::Context)?;
                }
                self.revoke(browser)?;
                self.retain(AgentRuntimeEvent::RendererLost(loss))?;
                Err(AgentWorkFailure::ContextLost)
            }
            event if worker.status().mailbox_fault().is_some() => {
                self.revoke(browser)?;
                self.retain(event)?;
                Err(AgentWorkFailure::Mailbox)
            }
            event => Ok(event),
        }
    }

    async fn cleanup_event(
        worker: &mut AgentRuntimeWorker,
        deadline: Instant,
    ) -> Result<AgentRuntimeEvent, AgentWorkFailure> {
        loop {
            let deadline = worker
                .shutdown_deadline()
                .map_or(deadline, |at| at.min(deadline));
            if Instant::now() >= deadline {
                return Err(AgentWorkFailure::Deadline);
            }
            let event = tokio::time::timeout_at(
                tokio::time::Instant::from_std(deadline),
                worker.next_event_for_terminal_cleanup(),
            )
            .await
            .map_err(|_| AgentWorkFailure::Deadline)?;
            if !matches!(
                event,
                AgentRuntimeEvent::CancellationRequested | AgentRuntimeEvent::ShutdownRequested
            ) {
                return Ok(event);
            }
        }
    }

    async fn terminal_event(
        &mut self,
        worker: &mut AgentRuntimeWorker,
        browser: &AgentRuntimeBrowser,
        cleanup: Option<Instant>,
    ) -> Result<AgentRuntimeEvent, AgentWorkFailure> {
        match cleanup {
            Some(deadline) => Self::cleanup_event(worker, deadline).await,
            None => self.next_event(worker, browser).await,
        }
    }
}

impl AgentRuntimeController for AgentWorkController {
    fn run(
        self: Box<Self>,
        mut worker: AgentRuntimeWorker,
        browser: AgentRuntimeBrowser,
    ) -> AgentRuntimeControllerFuture {
        Box::pin(async move {
            let mut controller = *self;
            if let Err(failure) = controller.execute(&mut worker, &browser).await {
                if let Some(state) = controller.state.as_mut() {
                    state.failure = Some(failure);
                    let _ = state.native.revoke(&browser);
                    if let Some(session) = state.session.as_ref() {
                        session.cancel();
                    }
                    let deadline = controller.drain_recovery(&mut worker, &browser).await;
                    let _ = controller
                        .close_unsuccessful(&mut worker, &browser, deadline)
                        .await;
                }
            }
        })
    }
}

impl AgentWorkController {
    async fn execute(
        &mut self,
        worker: &mut AgentRuntimeWorker,
        browser: &AgentRuntimeBrowser,
    ) -> Result<(), AgentWorkFailure> {
        let state = self.state.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let started = state.native.next_event(worker, browser).await?;
        if !matches!(started, AgentRuntimeEvent::RunStarted(_)) {
            state.native.retain(started)?;
            return Err(AgentWorkFailure::Mailbox);
        }
        let input = state.input.as_ref().ok_or(AgentWorkFailure::Contract)?;
        let journal = state.journal.as_mut().ok_or(AgentWorkFailure::Contract)?;
        journal.start(input.settings.ids.attempt)?;
        let caps = ContextCapabilities::try_new(
            ContextKind::Owned,
            &[
                ContextCapability::Navigate,
                ContextCapability::Observe,
                ContextCapability::Act,
            ],
        )
        .map_err(|_| AgentWorkFailure::Contract)?;
        let id = state.native.identity.id();
        journal
            .supervisor
            .reserve_context(
                journal
                    .execution
                    .as_ref()
                    .ok_or(AgentWorkFailure::Contract)?,
                &input.manifest,
                state.native.contexts()?,
                input.context.identity,
                caps,
            )
            .map_err(|_| AgentWorkFailure::Context)?;
        journal.record()?;
        journal.emit(AgentWorkEventKind::ContextActive)?;
        let lease_id =
            ContextProfileLeaseId::new(state.native.id()?).ok_or(AgentWorkFailure::Contract)?;
        let lease = state
            .native
            .profiles()?
            .acquire(
                lease_id,
                input.context.identity,
                input.context.storage,
                ContextProfileLeasePurpose::Owned,
            )
            .map_err(|_| AgentWorkFailure::Context)?;
        state.native.profile = Some(lease);
        let op = ContextOperationId::new(state.native.id()?).ok_or(AgentWorkFailure::Contract)?;
        let operation = state
            .native
            .contexts()?
            .begin_context(id, op)
            .map_err(|_| AgentWorkFailure::Context)?;
        let request = ContextConstructionRequest::try_new(
            operation,
            caps,
            lease,
            ContextConstructionSource::Owned,
        )
        .map_err(|_| AgentWorkFailure::Context)?;
        state.native.operation = Some(operation);
        if browser.dispatch(ContextNativeRequest::Construct(request)) != ContextDispatch::Scheduled
        {
            state.native.operation = None;
            state
                .native
                .contexts()?
                .settle_construction(id, operation, ContextSettlement::Refused)
                .map_err(|_| AgentWorkFailure::Context)?;
            return Err(AgentWorkFailure::Context);
        }
        match state.native.next_event(worker, browser).await? {
            AgentRuntimeEvent::NativeTerminal(ContextNativeEvent::ConstructionSettled(
                settlement,
            )) if settlement.operation() == operation => {
                state.native.operation = None;
                let applied = settlement.outcome().is_ok();
                state
                    .native
                    .contexts()?
                    .settle_construction(
                        id,
                        operation,
                        if applied {
                            ContextSettlement::Applied
                        } else {
                            ContextSettlement::Refused
                        },
                    )
                    .map_err(|_| AgentWorkFailure::Context)?;
                if !applied {
                    return Err(AgentWorkFailure::Native(
                        settlement
                            .outcome()
                            .err()
                            .ok_or(AgentWorkFailure::Context)?,
                    ));
                }
            }
            event => {
                state.native.retain(event)?;
                return Err(AgentWorkFailure::Mailbox);
            }
        }
        let op = ContextOperationId::new(state.native.id()?).ok_or(AgentWorkFailure::Contract)?;
        let operation = state
            .native
            .contexts()?
            .begin_navigation(id, op)
            .map_err(|_| AgentWorkFailure::Context)?;
        let input = state.input.as_ref().ok_or(AgentWorkFailure::Contract)?;
        let request = ContextNavigationRequest::try_new(operation, input.context.target.clone())
            .map_err(|_| AgentWorkFailure::Context)?;
        state.native.operation = Some(operation);
        if browser.dispatch(ContextNativeRequest::Navigate(request)) != ContextDispatch::Scheduled {
            state.native.operation = None;
            state
                .native
                .contexts()?
                .settle_navigation(id, operation, ContextSettlement::Refused)
                .map_err(|_| AgentWorkFailure::Context)?;
            return Err(AgentWorkFailure::Context);
        }
        match state.native.next_event(worker, browser).await? {
            AgentRuntimeEvent::NativeTerminal(ContextNativeEvent::NavigationSettled(
                settlement,
            )) if settlement.operation() == operation => {
                state.native.operation = None;
                let applied = settlement.outcome().as_ref().is_ok_and(|target| {
                    Some(target) == state.input.as_ref().map(|input| &input.context.target)
                });
                state
                    .native
                    .contexts()?
                    .settle_navigation(
                        id,
                        operation,
                        if applied {
                            ContextSettlement::Applied
                        } else {
                            ContextSettlement::Refused
                        },
                    )
                    .map_err(|_| AgentWorkFailure::Context)?;
                if !applied {
                    return Err(AgentWorkFailure::Context);
                }
            }
            event => {
                state.native.retain(event)?;
                return Err(AgentWorkFailure::Mailbox);
            }
        }
        self.start_session()?;
        self.browser_loop(worker, browser).await?;
        self.close_success(worker, browser).await
    }

    fn start_session(&mut self) -> Result<(), AgentWorkFailure> {
        let state = self.state.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let mut input = state.input.take().ok_or(AgentWorkFailure::Contract)?;
        let id = state.native.identity.id();
        let context = state
            .native
            .contexts()?
            .join(id)
            .map_err(|_| AgentWorkFailure::Context)?;
        let now = input
            .settings
            .clock
            .now()
            .map_err(|_| AgentWorkFailure::Contract)?;
        let account = state.task.attest_account(context, now)?;
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            state.native.origin.clone(),
            SemanticFrameTrust::SameOrigin,
        )
        .map_err(|_| AgentWorkFailure::Context)?;
        let run = TerraControllerRunInput {
            manifest: input.manifest,
            lease: input.lease,
            account,
            observation: SemanticObservationRequest::initial(
                SemanticObservationId::new(1).ok_or(AgentWorkFailure::Contract)?,
                context,
                SemanticObservationBudget::INITIAL_FILTERED,
            ),
            frame,
            invocation: SemanticInvocationId::new(1).ok_or(AgentWorkFailure::Contract)?,
            snapshot_generation: SemanticSnapshotGeneration::INITIAL,
            objective: input.objective.take().ok_or(AgentWorkFailure::Contract)?,
            ids: input.settings.ids,
            clock: input.settings.clock,
            deadline: input.settings.deadline,
        };
        let mut session = AgentBrowserSession::try_new_with_transport(
            run,
            state.transport.take().ok_or(AgentWorkFailure::Contract)?,
            state.credential.take().ok_or(AgentWorkFailure::Contract)?,
            input.settings.model,
            state.retention,
        )
        .map_err(AgentWorkFailure::Browser)?;
        session.journal = state.journal.take();
        if state.task.extraction_schema().is_some() {
            session.config = session.config.restrict_to_extraction();
        }
        state.session = Some(session);
        Ok(())
    }

    async fn observe(
        state: &mut WorkState,
        worker: &mut AgentRuntimeWorker,
        browser: &AgentRuntimeBrowser,
    ) -> Result<SemanticObservation, AgentWorkFailure> {
        state.journal_mut()?.emit(AgentWorkEventKind::Observing)?;
        // Only an explicit pre-dispatch NotReady receipt can start another
        // read-only readiness check. Never retry a mutation, stale reference,
        // replaced document, malformed callback or provider turn.
        for _ in 0..64 {
            match Self::observe_once(state, worker, browser).await {
                Err(AgentWorkFailure::Observation(SemanticRuntimePortFailure::NotReady)) => {
                    tokio::select! {
                        biased;
                        event = state.native.next_event(worker, browser) => {
                            state.native.retain(event?)?;
                            return Err(AgentWorkFailure::Mailbox);
                        }
                        () = tokio::time::sleep(Duration::from_millis(50)) => {}
                    }
                }
                result => return result,
            }
        }
        Err(AgentWorkFailure::Observation(
            SemanticRuntimePortFailure::NotReady,
        ))
    }

    async fn observe_once(
        state: &mut WorkState,
        worker: &mut AgentRuntimeWorker,
        browser: &AgentRuntimeBrowser,
    ) -> Result<SemanticObservation, AgentWorkFailure> {
        let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
        session.check_live().map_err(AgentWorkFailure::Browser)?;
        let id = state.native.identity.id();
        let context = state
            .native
            .contexts()?
            .join(id)
            .map_err(|_| AgentWorkFailure::Context)?;
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            state.native.origin.clone(),
            SemanticFrameTrust::SameOrigin,
        )
        .map_err(|_| AgentWorkFailure::Context)?;
        let next = state.native.id()?;
        let generation = state
            .native
            .snapshot_generation
            .map_or(
                Some(SemanticSnapshotGeneration::INITIAL),
                SemanticSnapshotGeneration::next,
            )
            .ok_or(AgentWorkFailure::Contract)?;
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(next).ok_or(AgentWorkFailure::Contract)?,
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        let invocation = encode_semantic_runtime_invocation(
            &request,
            frame,
            SemanticInvocationId::new(next).ok_or(AgentWorkFailure::Contract)?,
            generation,
            SemanticRuntimeBudget::INITIAL_FILTERED,
        )
        .map_err(|_| AgentWorkFailure::Context)?;
        let correlation = invocation.correlation();
        state.native.observation = Some(correlation.clone());
        if browser.invoke_semantic(invocation) != ContextDispatch::Scheduled {
            state.native.observation = None;
            return Err(AgentWorkFailure::Context);
        }
        match state.native.next_event(worker, browser).await? {
            AgentRuntimeEvent::NativeTerminal(ContextNativeEvent::SemanticRuntimeSettled(
                settlement,
            )) if settlement.correlation() == &correlation => {
                state.native.observation = None;
                let snapshot = settlement
                    .into_outcome()
                    .map_err(AgentWorkFailure::Observation)?;
                state.native.snapshot_generation = Some(generation);
                let observation = SemanticObservationAssembler::new(request, snapshot)
                    .and_then(SemanticObservationAssembler::finish)
                    .map_err(|_| AgentWorkFailure::Context)?;
                state
                    .native
                    .contexts()?
                    .acknowledge_observation(id, context)
                    .map_err(|_| AgentWorkFailure::Context)?;
                Ok(observation)
            }
            event => {
                state.native.retain(event)?;
                Err(AgentWorkFailure::Mailbox)
            }
        }
    }

    async fn provider<T>(
        native: &mut WorkNative,
        worker: &mut AgentRuntimeWorker,
        browser: &AgentRuntimeBrowser,
        cancellation: AgentProviderCancellation,
        future: impl std::future::Future<Output = Result<T, AgentBrowserProviderError>>,
    ) -> Result<T, AgentWorkFailure> {
        tokio::pin!(future);
        tokio::select! {
            biased;
            event = native.next_event(worker, browser) => {
                cancellation.cancel();
                match event { Ok(event) => { native.retain(event)?; Err(AgentWorkFailure::Mailbox) }, Err(failure) => Err(failure) }
            }
            result = &mut future => result.map_err(AgentWorkFailure::Browser),
        }
    }

    async fn browser_loop(
        &mut self,
        worker: &mut AgentRuntimeWorker,
        browser: &AgentRuntimeBrowser,
    ) -> Result<(), AgentWorkFailure> {
        let state = self.state.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let mut observation = Self::observe(state, worker, browser).await?;
        let captured_at = SemanticCaptureInstant::from_millis(
            state
                .journal_mut()?
                .clock
                .now()
                .map_err(|_| AgentWorkFailure::Contract)?
                .millis(),
        );
        if state.task.evaluate(&observation)? == AgentWorkTaskProgress::Complete {
            state.observation = Some(observation);
            return Ok(());
        }
        let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let mut turn: AgentBrowserProviderTurn = Self::provider(
            &mut state.native,
            worker,
            browser,
            session.cancellation.clone(),
            session.start_initial(&observation),
        )
        .await?;
        if let Some(schema) = state.task.extraction_schema() {
            let frames = observation
                .frames()
                .iter()
                .map(|snapshot| snapshot.frame().clone())
                .collect::<Vec<_>>();
            let result = Self::provider(
                &mut state.native,
                worker,
                browser,
                session.cancellation.clone(),
                session.extract(turn, &observation, &frames, captured_at, schema),
            )
            .await?;
            if state.task.accept_extraction(&result)? != AgentWorkTaskProgress::Complete {
                return Err(AgentWorkFailure::Contract);
            }
            state.extraction = Some(
                result
                    .into_owned()
                    .map_err(|_| AgentWorkFailure::Contract)?,
            );
            state.observation = Some(observation);
            return Ok(());
        }
        loop {
            let frames = observation
                .frames()
                .iter()
                .map(|snapshot| snapshot.frame().clone())
                .collect::<Vec<_>>();
            let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
            let proposal = Self::provider(
                &mut state.native,
                worker,
                browser,
                session.cancellation.clone(),
                session.next_action(turn, &observation, &frames, |_, _, _| {}),
            )
            .await?;
            let assessment = state.task.assess(proposal.action())?;
            let id = state.native.identity.id();
            let automation = state
                .native
                .contexts()?
                .automation_state(id)
                .map_err(|_| AgentWorkFailure::Context)?;
            let now = session.policy_now().map_err(AgentWorkFailure::Browser)?;
            let request = session
                .authorize_action(
                    proposal,
                    &assessment,
                    automation,
                    SemanticActionExecutionInstant::from_millis(now.millis()),
                )
                .map_err(AgentWorkFailure::Browser)?;
            let dispatch =
                browser.execute_semantic_action(request, worker.semantic_action_completion());
            session
                .action
                .as_mut()
                .ok_or(AgentWorkFailure::Contract)?
                .account_dispatch(
                    dispatch,
                    &mut session.policy,
                    &mut session.action_executions,
                )
                .map_err(|error| {
                    AgentWorkFailure::Browser(AgentBrowserProviderError::Action(error))
                })?;
            state.native.action_pending = true;
            let terminal = match state.native.next_event(worker, browser).await? {
                AgentRuntimeEvent::SemanticActionTerminal(terminal)
                    if session.action.as_ref().is_some_and(|action| {
                        action.accepts_settlement(&session.action_executions, &terminal)
                    }) =>
                {
                    terminal
                }
                event => {
                    state.native.retain(event)?;
                    return Err(AgentWorkFailure::Mailbox);
                }
            };
            state.native.action_pending = false;
            state.native_terminal = Some(terminal);
            let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
            let terminal = state
                .native_terminal
                .take()
                .ok_or(AgentWorkFailure::Contract)?;
            let mut wake = session
                .begin_action_settlement(terminal)
                .map_err(AgentWorkFailure::Browser)?;
            for _ in 0..8 {
                let Some(next_wake) = wake else {
                    break;
                };
                let now = session.policy_now().map_err(AgentWorkFailure::Browser)?;
                let delay = Duration::from_millis(next_wake.millis().saturating_sub(now.millis()));
                tokio::select! {
                    biased;
                    event = state.native.next_event(worker, browser) => {
                        state.native.retain(event?)?;
                        return Err(AgentWorkFailure::Mailbox);
                    }
                    () = tokio::time::sleep(delay) => {}
                }
                let now = session.policy_now().map_err(AgentWorkFailure::Browser)?;
                wake = session
                    .wake_action_settlement(SemanticSettleInstant::from_millis(now.millis()))
                    .map_err(AgentWorkFailure::Browser)?;
            }
            if wake.is_some() {
                return Err(AgentWorkFailure::Contract);
            }
            let current = Self::observe(state, worker, browser).await?;
            let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
            let now = session.policy_now().map_err(AgentWorkFailure::Browser)?;
            let (_, transition) = session
                .verify_action_settlement(
                    &observation,
                    &current,
                    SemanticSettleInstant::from_millis(now.millis()),
                )
                .map_err(AgentWorkFailure::Browser)?;
            observation = current;
            if state.task.evaluate(&observation)? == AgentWorkTaskProgress::Complete {
                state.observation = Some(observation);
                return Ok(());
            }
            turn = Self::provider(
                &mut state.native,
                worker,
                browser,
                session.cancellation.clone(),
                session.continue_after_verified_action(transition),
            )
            .await?;
        }
    }

    async fn close_context(
        state: &mut WorkState,
        worker: &mut AgentRuntimeWorker,
        browser: &AgentRuntimeBrowser,
    ) -> Result<(), AgentWorkFailure> {
        let id = state.native.identity.id();
        // Revoke before teardown even on trusted successful completion.
        state.native.revoke(browser)?;
        if let Some(expected) = state.native.cancellation {
            let event = state.native.next_event(worker, browser).await?;
            match event {
                AgentRuntimeEvent::NativeTerminal(ContextNativeEvent::CancellationSettled(
                    settlement,
                )) if settlement.current() == expected => {
                    state.native.cancellation = None;
                    if settlement.outcome().is_err() {
                        state.native.retain(AgentRuntimeEvent::NativeTerminal(
                            ContextNativeEvent::CancellationSettled(settlement),
                        ))?;
                        return Err(AgentWorkFailure::Context);
                    }
                }
                event => {
                    state.native.retain(event)?;
                    return Err(AgentWorkFailure::Mailbox);
                }
            }
        }
        if state.native.close_attempted {
            return Err(AgentWorkFailure::Context);
        }
        state.native.close_attempted = true;
        let op = ContextOperationId::new(state.native.id()?).ok_or(AgentWorkFailure::Contract)?;
        let operation = state
            .native
            .contexts()?
            .begin_close(id, op)
            .map_err(|_| AgentWorkFailure::Context)?;
        state.native.operation = Some(operation);
        let request =
            ContextTransitionRequest::try_new(operation).map_err(|_| AgentWorkFailure::Context)?;
        if browser.dispatch(ContextNativeRequest::Transition(request)) != ContextDispatch::Scheduled
        {
            state.native.operation = None;
            state
                .native
                .contexts()?
                .settle_close(id, operation, ContextSettlement::Refused)
                .map_err(|_| AgentWorkFailure::Context)?;
            return Err(AgentWorkFailure::Context);
        }
        let event = state.native.next_event(worker, browser).await?;
        match event {
            AgentRuntimeEvent::NativeTerminal(ContextNativeEvent::TransitionSettled(
                settlement,
            )) if settlement.operation() == operation => {
                state.native.operation = None;
                let applied = settlement.outcome().is_ok();
                state
                    .native
                    .contexts()?
                    .settle_close(
                        id,
                        operation,
                        if applied {
                            ContextSettlement::Applied
                        } else {
                            ContextSettlement::Refused
                        },
                    )
                    .map_err(|_| AgentWorkFailure::Context)?;
                if !applied {
                    return Err(AgentWorkFailure::Context);
                }
            }
            event => {
                state.native.retain(event)?;
                return Err(AgentWorkFailure::Mailbox);
            }
        }
        Self::release_closed_context(state)
    }

    fn release_closed_context(state: &mut WorkState) -> Result<(), AgentWorkFailure> {
        let id = state.native.identity.id();
        let journal = state
            .session
            .as_mut()
            .and_then(|session| session.journal.as_mut())
            .or(state.journal.as_mut())
            .ok_or(AgentWorkFailure::Contract)?;
        let release = journal
            .supervisor
            .reap_terminal_context(state.native.contexts()?, id)
            .map_err(|_| AgentWorkFailure::Context)?;
        let lease = state.native.profile.ok_or(AgentWorkFailure::Context)?;
        state
            .native
            .profiles()?
            .release(lease, release)
            .map_err(|_| AgentWorkFailure::Context)?;
        state.native.profile = None;
        journal.record()
    }

    fn begin_recovery_close(state: &mut WorkState, browser: &AgentRuntimeBrowser) {
        if !state.native.revoked || state.native.profile.is_none() || state.native.close_attempted {
            return;
        }
        // Teardown is not business-action replay. It must not wait for a lost
        // observation/cancellation callback to consume the cleanup deadline.
        // The independent slot preserves every previously dispatched owner.
        state.native.close_attempted = true;
        let operation = (|| {
            let id = state.native.identity.id();
            let op =
                ContextOperationId::new(state.native.id()?).ok_or(AgentWorkFailure::Contract)?;
            state
                .native
                .contexts()?
                .begin_close(id, op)
                .map_err(|_| AgentWorkFailure::Context)
        })();
        let Ok(operation) = operation else {
            return;
        };
        let Ok(request) = ContextTransitionRequest::try_new(operation) else {
            return;
        };
        state.native.recovery_close = Some(operation);
        if browser.dispatch(ContextNativeRequest::Transition(request)) != ContextDispatch::Scheduled
        {
            state.native.recovery_close = None;
            let id = state.native.identity.id();
            if let Ok(contexts) = state.native.contexts() {
                let _ = contexts.settle_close(id, operation, ContextSettlement::Refused);
            }
        }
    }

    async fn close_success(
        &mut self,
        worker: &mut AgentRuntimeWorker,
        browser: &AgentRuntimeBrowser,
    ) -> Result<(), AgentWorkFailure> {
        let state = self.state.as_mut().ok_or(AgentWorkFailure::Contract)?;
        Self::close_context(state, worker, browser).await?;
        self.close_resources(worker, browser, None).await
    }

    async fn close_unsuccessful(
        &mut self,
        worker: &mut AgentRuntimeWorker,
        browser: &AgentRuntimeBrowser,
        deadline: Instant,
    ) -> Result<(), AgentWorkFailure> {
        let state = self.state.as_ref().ok_or(AgentWorkFailure::Contract)?;
        // This is a terminal resource join, not an alternative interpretation
        // of a previous failed close or an acknowledged successful task.
        if state.failure.is_none()
            || state.session.is_none()
            || state.drained.is_some()
            || state.native.profile.is_some()
            || !state.native.revoked
            || !state.native.close_attempted
            || !state.native.deferred.is_empty()
            || state.native.operation.is_some()
            || state.native.recovery_close.is_some()
            || state.native.observation.is_some()
            || state.native.action_pending
            || state.native.cancellation.is_some()
            || state.native.shutdown_audit.is_some()
            || state.native_terminal.is_some()
            || Instant::now() >= deadline
        {
            return Err(AgentWorkFailure::Shutdown);
        }
        self.close_resources(worker, browser, Some(deadline)).await
    }

    async fn close_resources(
        &mut self,
        worker: &mut AgentRuntimeWorker,
        browser: &AgentRuntimeBrowser,
        cleanup: Option<Instant>,
    ) -> Result<(), AgentWorkFailure> {
        let state = self.state.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let resources = state
            .native
            .resources
            .as_mut()
            .ok_or(AgentWorkFailure::Context)?;
        resources
            .contexts
            .seal_for_shutdown()
            .map_err(|_| AgentWorkFailure::Shutdown)?;
        resources
            .profiles
            .seal_for_shutdown()
            .map_err(|_| AgentWorkFailure::Shutdown)?;
        resources
            .cookies
            .seal_for_shutdown()
            .map_err(|_| AgentWorkFailure::Shutdown)?;
        resources.screenshots.seal_for_shutdown();
        let session = state.session.take().ok_or(AgentWorkFailure::Contract)?;
        let finished = if cleanup.is_some() {
            session.try_finish_unsuccessful()
        } else {
            session.try_finish()
        };
        let terminal = match finished {
            Ok(terminal) => terminal,
            Err(refusal) => {
                let failure = refusal.error();
                state.session = Some(refusal.into_session());
                return Err(AgentWorkFailure::Browser(failure));
            }
        };
        let resources = match state.native.resources.take() {
            Some(resources) => resources,
            None => {
                state.session = Some(*terminal.session);
                return Err(AgentWorkFailure::Context);
            }
        };
        let AgentBrowserSession {
            policy,
            journal,
            action_executions,
            action_settlements,
            ..
        } = *terminal.session;
        let provider = terminal.provider;
        // These exact original owners, never replacement empty coordinators,
        // enter the constructor-closed native shutdown cohort.
        state.drained = Some(WorkDrained {
            policy: Some(policy),
            journal,
            provider: Some(provider),
            native: None,
            resources: Some(AgentNativeShutdownResources::new(
                resources.contexts,
                resources.profiles,
                resources.cookies,
                action_executions,
                action_settlements,
                resources.screenshots,
            )),
            proof: None,
        });
        let drained = state.drained.as_mut().ok_or(AgentWorkFailure::Contract)?;
        match AgentNativeShutdownCoordinator::try_new(
            drained.resources.take().ok_or(AgentWorkFailure::Contract)?,
        ) {
            Ok(native) => drained.native = Some(native),
            Err(refusal) => {
                drained.resources = Some(refusal.into_resources());
                return Err(AgentWorkFailure::Shutdown);
            }
        }
        let audit =
            ContextResourceAuditId::new(state.native.id()?).ok_or(AgentWorkFailure::Contract)?;
        let native = drained.native.as_mut().ok_or(AgentWorkFailure::Contract)?;
        native
            .begin_port_seal(audit)
            .map_err(|_| AgentWorkFailure::Shutdown)?;
        state.native.shutdown_audit = Some(audit);
        native
            .account_port_seal(audit, browser.seal_for_shutdown(audit))
            .map_err(|_| AgentWorkFailure::Shutdown)?;
        // No automatic mutation retry. A nonzero native barrier is retained
        // recovery; subsequent read-only resource qualification is explicit.
        if native.status().stage() != AgentNativeShutdownStage::ShutdownAuditPending {
            state.native.shutdown_audit = None;
            return Err(AgentWorkFailure::Shutdown);
        }
        match state
            .native
            .terminal_event(worker, browser, cleanup)
            .await?
        {
            AgentRuntimeEvent::NativeTerminal(ContextNativeEvent::ShutdownAuditSettled(
                settlement,
            )) if settlement.audit() == audit => {
                native
                    .settle_shutdown_audit(settlement)
                    .map_err(|_| AgentWorkFailure::Shutdown)?;
                state.native.shutdown_audit = None;
            }
            event => {
                state.native.retain(event)?;
                return Err(AgentWorkFailure::Mailbox);
            }
        }
        let native = drained.native.take().ok_or(AgentWorkFailure::Contract)?;
        match native.finish() {
            Ok(proof) => drained.proof = Some(proof),
            Err(refusal) => {
                drained.native = Some(refusal.into_coordinator());
                return Err(AgentWorkFailure::Shutdown);
            }
        }
        let journal = drained.journal.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let completion = match state.failure.filter(|_| cleanup.is_some()) {
            Some(failure) => {
                let cancellation = match worker.stop_reason() {
                    Some(AgentRuntimeStopReason::HumanTakeover) => {
                        Some(AgentSupervisorCancellationReason::HumanTakeover)
                    }
                    Some(AgentRuntimeStopReason::PolicyRevoked) => {
                        Some(AgentSupervisorCancellationReason::PolicyRevoked)
                    }
                    Some(AgentRuntimeStopReason::Cancelled | AgentRuntimeStopReason::Suspend) => {
                        Some(AgentSupervisorCancellationReason::UserRequested)
                    }
                    None if failure == AgentWorkFailure::Deadline => {
                        Some(AgentSupervisorCancellationReason::DeadlineExceeded)
                    }
                    None => None,
                };
                let cancellation = if worker.shutdown_deadline().is_some() {
                    Some(AgentSupervisorCancellationReason::Shutdown)
                } else {
                    cancellation
                };
                if let Some(reason) = cancellation {
                    let _ = journal
                        .supervisor
                        .cancel_subtree(journal.root, journal.cancellation, reason)
                        .map_err(|_| AgentWorkFailure::Accounting)?;
                    journal.record()?;
                }
                AgentSupervisorCompletion::Failed(match failure {
                    AgentWorkFailure::Browser(_) => AgentSupervisorFailure::ProviderFailed,
                    _ => AgentSupervisorFailure::PolicyDenied,
                })
            }
            None => AgentSupervisorCompletion::Succeeded,
        };
        let execution = journal.execution.take().ok_or(AgentWorkFailure::Contract)?;
        journal
            .supervisor
            .complete(execution, completion)
            .map_err(|_| AgentWorkFailure::Accounting)?;
        journal.record()?;
        journal
            .audit
            .seal_for_shutdown()
            .map_err(|_| AgentWorkFailure::Audit)?;
        self.deliver_audit(worker, browser, cleanup).await?;
        self.publish_terminal(worker, cleanup.is_some()).await
    }

    async fn deliver_audit(
        &mut self,
        worker: &mut AgentRuntimeWorker,
        browser: &AgentRuntimeBrowser,
        cleanup: Option<Instant>,
    ) -> Result<(), AgentWorkFailure> {
        let state = self.state.as_mut().ok_or(AgentWorkFailure::Contract)?;
        for raw in 1..=4 {
            let journal = state.journal_mut()?;
            if journal.audit.is_quiescent() {
                return Ok(());
            }
            let batch = journal
                .audit
                .begin_delivery(
                    AgentAuditDeliveryId::new(raw).ok_or(AgentWorkFailure::Contract)?,
                    MAX_AGENT_AUDIT_DELIVERY_EVENTS,
                )
                .map_err(|_| AgentWorkFailure::Audit)?;
            let expected = batch.proof();
            let settlement = match state.audit.append(batch, worker.audit_completion()) {
                AgentAuditDispatch::Refused(settlement)
                    if settlement.proof() == expected
                        && settlement.outcome() != AgentAuditDeliveryOutcome::Committed =>
                {
                    settlement
                }
                AgentAuditDispatch::Accepted(proof) if proof == expected => {
                    match state
                        .native
                        .terminal_event(worker, browser, cleanup)
                        .await?
                    {
                        AgentRuntimeEvent::AuditTerminal(settlement)
                            if settlement.proof() == expected =>
                        {
                            settlement
                        }
                        event => {
                            state.native.retain(event)?;
                            return Err(AgentWorkFailure::Mailbox);
                        }
                    }
                }
                AgentAuditDispatch::Refused(settlement) => {
                    state
                        .native
                        .retain(AgentRuntimeEvent::AuditTerminal(settlement))?;
                    return Err(AgentWorkFailure::Audit);
                }
                AgentAuditDispatch::Accepted(_) => return Err(AgentWorkFailure::Audit),
            };
            if state
                .journal_mut()?
                .audit
                .settle_delivery(settlement)
                .map_err(|_| AgentWorkFailure::Audit)?
                != AgentAuditDeliveryOutcome::Committed
            {
                return Err(AgentWorkFailure::Audit);
            }
        }
        state
            .journal_mut()?
            .audit
            .is_quiescent()
            .then_some(())
            .ok_or(AgentWorkFailure::Audit)
    }

    async fn publish_terminal(
        &mut self,
        worker: &mut AgentRuntimeWorker,
        unsuccessful: bool,
    ) -> Result<(), AgentWorkFailure> {
        let state = self.state.as_mut().ok_or(AgentWorkFailure::Contract)?;
        if !state.native.deferred.is_empty()
            || state.native.operation.is_some()
            || state.native.recovery_close.is_some()
            || state.native.observation.is_some()
            || state.native.action_pending
            || state.native.cancellation.is_some()
            || state.native.shutdown_audit.is_some()
            || state.native_terminal.is_some()
        {
            return Err(AgentWorkFailure::Mailbox);
        }
        let drained = state.drained.as_mut().ok_or(AgentWorkFailure::Contract)?;
        if drained.proof.is_none() || drained.provider.is_none() {
            return Err(AgentWorkFailure::Shutdown);
        }
        let journal = drained.journal.as_ref().ok_or(AgentWorkFailure::Contract)?;
        let closure = AgentRunMetricClosure::try_close(
            drained
                .policy
                .as_ref()
                .ok_or(AgentWorkFailure::Contract)?
                .manifest(),
            &journal.supervisor,
            &journal.accounting,
            &journal.progress,
            &journal.actions,
            &journal.inputs,
        )
        .map_err(|_| AgentWorkFailure::Accounting)?;
        let class = if unsuccessful && worker.shutdown_deadline().is_some() {
            AgentRuntimeControllerTerminalClass::Shutdown
        } else if unsuccessful && worker.stop_reason().is_some() {
            AgentRuntimeControllerTerminalClass::Cancelled
        } else {
            AgentRuntimeControllerTerminalClass::Ordinary
        };
        let failure = if unsuccessful {
            Some(state.failure.ok_or(AgentWorkFailure::Contract)?)
        } else {
            None
        };
        if (closure.outcome() == AgentRunProgressOutcome::Succeeded) == unsuccessful {
            return Err(AgentWorkFailure::Accounting);
        }
        let claim = match worker.try_claim_controller_terminal(class).await {
            Ok(claim) => claim,
            Err(refusal) => {
                while let Some(event) = worker.try_drain_terminal_claim_refusal_event() {
                    state.native.retain(event)?;
                }
                return Err(AgentWorkFailure::RuntimeTerminal {
                    refusal,
                    stop: worker.stop_reason(),
                });
            }
        };
        let policy = drained.policy.take().ok_or(AgentWorkFailure::Contract)?;
        let mut journal = drained.journal.take().ok_or(AgentWorkFailure::Contract)?;
        // The only irreversible boundary is after runtime ingress closes and
        // native/provider proofs already exist. Refusal returns both owners.
        let settlement =
            match policy.settle_metric_closure(closure, &journal.accounting, journal.audit) {
                Ok(settlement) => settlement,
                Err(refusal) => {
                    let (policy, audit) = refusal.into_parts();
                    drained.policy = Some(policy);
                    journal.audit = audit;
                    drained.journal = Some(journal);
                    return Err(AgentWorkFailure::Accounting);
                }
            };
        let proof = drained.proof.take().ok_or(AgentWorkFailure::Shutdown)?;
        let provider = drained.provider.take().ok_or(AgentWorkFailure::Shutdown)?;
        claim.commit_with_shutdown(proof, settlement, provider);
        // Completion is separate from the bounded progress lane: saturation
        // cannot discard an already-consumed clean terminal owner.
        let _ = lock(&journal.events).publish(AgentWorkEventKind::Terminal);
        *lock(&self.terminal) = Some(match failure {
            Some(failure) => {
                AgentWorkOutcome::ClosedUnsuccessfully(AgentWorkClosedUnsuccessfully {
                    settlement,
                    failure,
                })
            }
            None => AgentWorkOutcome::Succeeded(AgentWorkSuccess {
                settlement,
                extraction: state.extraction.take().map(Box::new),
            }),
        });
        self.state.take();
        Ok(())
    }

    async fn drain_recovery(
        &mut self,
        worker: &mut AgentRuntimeWorker,
        browser: &AgentRuntimeBrowser,
    ) -> Instant {
        let deadline = worker
            .shutdown_deadline()
            .unwrap_or_else(|| Instant::now() + Duration::from_secs(1));
        let Some(state) = self.state.as_mut() else {
            return deadline;
        };
        if Instant::now() < deadline {
            Self::begin_recovery_close(state, browser);
        }
        // Drain already-dispatched callbacks only; no action or provider retry.
        while state.native.operation.is_some()
            || state.native.recovery_close.is_some()
            || state.native.observation.is_some()
            || state.native.action_pending
            || state.native.cancellation.is_some()
            || state.native.shutdown_audit.is_some()
            || state
                .journal_mut()
                .is_ok_and(|journal| journal.audit.status().in_flight() != 0)
        {
            let event = WorkNative::cleanup_event(worker, deadline).await;
            let Ok(event) = event else {
                break;
            };
            match &event {
                AgentRuntimeEvent::NativeTerminal(ContextNativeEvent::ConstructionSettled(
                    value,
                )) if state.native.operation == Some(value.operation()) => {
                    state.native.operation = None
                }
                AgentRuntimeEvent::NativeTerminal(ContextNativeEvent::NavigationSettled(value))
                    if state.native.operation == Some(value.operation()) =>
                {
                    state.native.operation = None
                }
                AgentRuntimeEvent::NativeTerminal(ContextNativeEvent::TransitionSettled(value))
                    if state.native.operation == Some(value.operation()) =>
                {
                    state.native.operation = None
                }
                AgentRuntimeEvent::SemanticActionTerminal(value)
                    if state.session.as_ref().is_some_and(|session| {
                        session.action.as_ref().is_some_and(|action| {
                            action.accepts_settlement(&session.action_executions, value)
                        })
                    }) =>
                {
                    state.native.action_pending = false
                }
                _ => {}
            }
            let accounted = match &event {
                // These exact callbacks settle read-only/cancellation owners,
                // never a dispatched action or a replacement observation.
                AgentRuntimeEvent::NativeTerminal(ContextNativeEvent::SemanticRuntimeSettled(
                    value,
                )) if state.native.observation.as_ref() == Some(value.correlation()) => {
                    state.native.observation = None;
                    true
                }
                AgentRuntimeEvent::NativeTerminal(ContextNativeEvent::CancellationSettled(
                    value,
                )) if state.native.cancellation == Some(value.current()) => {
                    state.native.cancellation = None;
                    value.outcome().is_ok()
                }
                AgentRuntimeEvent::NativeTerminal(ContextNativeEvent::TransitionSettled(value))
                    if state.native.recovery_close == Some(value.operation()) =>
                {
                    state.native.recovery_close = None;
                    let id = state.native.identity.id();
                    let applied = value.outcome().is_ok();
                    let settled = state.native.contexts().is_ok_and(|contexts| {
                        contexts
                            .settle_close(
                                id,
                                value.operation(),
                                if applied {
                                    ContextSettlement::Applied
                                } else {
                                    ContextSettlement::Refused
                                },
                            )
                            .is_ok()
                    });
                    settled && applied && Self::release_closed_context(state).is_ok()
                }
                AgentRuntimeEvent::AuditTerminal(settlement) => {
                    state.journal_mut().is_ok_and(|journal| {
                        journal.audit.current_delivery().is_ok_and(|delivery| {
                            delivery.is_some_and(|delivery| delivery.proof() == settlement.proof())
                        }) && journal.audit.settle_delivery(*settlement).is_ok()
                    })
                }
                AgentRuntimeEvent::NativeTerminal(ContextNativeEvent::ShutdownAuditSettled(
                    settlement,
                )) if state.native.shutdown_audit == Some(settlement.audit()) => {
                    let settled = state
                        .drained
                        .as_mut()
                        .and_then(|drained| drained.native.as_mut())
                        .is_some_and(|native| native.settle_shutdown_audit(*settlement).is_ok());
                    if settled {
                        state.native.shutdown_audit = None;
                    }
                    settled
                }
                _ => false,
            };
            if !accounted && state.native.retain(event).is_err() {
                break;
            }
        }
        deadline
    }
}

impl Drop for AgentWorkController {
    fn drop(&mut self) {
        let Some(mut state) = self.state.take() else {
            return;
        };
        state.credential.take();
        // Dropping undispatched objective data is not dropping runtime debt.
        if let Some(input) = state.input.as_mut() {
            input.objective.take();
        }
        state.transport.take();
        if let Some(session) = state.session.take() {
            match session.try_finish() {
                Ok(terminal) => state.session = Some(*terminal.session),
                Err(refusal) => state.session = Some(refusal.into_session()),
            }
        }
        let failure = state.failure.unwrap_or(AgentWorkFailure::Shutdown);
        if let Ok(journal) = state.journal_mut() {
            let _ = journal.emit(AgentWorkEventKind::Recovery);
        }
        let mut slot = lock(&self.terminal);
        if slot.is_none() {
            *slot = Some(AgentWorkOutcome::Recovery(AgentWorkRecovery {
                failure,
                state: Box::new(state),
            }));
        }
    }
}

/// Maximum undrained product events. Saturation pauses execution; events are
/// never silently overwritten and the terminal owner has a separate slot.
pub const MAX_AGENT_WORK_EVENTS: usize = 64;

/// Content-free product phase. Model text never declares task completion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentWorkEventKind {
    /// The exact runtime admission has started.
    Started,
    /// A Work-owned browser context is being prepared.
    ContextActive,
    /// Native semantic observation is active.
    Observing,
    /// One policy-reserved provider turn is active.
    ModelActive,
    /// An exact provider terminal was accounted.
    ModelSettled {
        /// Exact model call correlation; complete receipts remain Rust-owned.
        call: AgentModelCallId,
        /// Charged input tokens.
        input_tokens: u64,
        /// Charged output tokens.
        output_tokens: u64,
        /// Charged micro-USD.
        cost_micro_usd: u64,
        /// Exact serialized provider request bytes.
        request_bytes: u32,
        /// Exact semantic disclosure bytes.
        semantic_bytes: u32,
        /// Exact or conservative usage accounting attribution.
        accounting: AgentModelUsageAccounting,
        /// Count plus streaming turn wall time, not server-only inference time.
        elapsed_millis: u64,
    },
    /// The model proposed a bounded typed tool.
    ToolProposed(AgentBrowserToolKind),
    /// An independently authorized native effect is active.
    ActionActive,
    /// A native effect was independently verified and accounted.
    Verified,
    /// An explicit policy/human boundary stopped execution.
    NeedsHuman(AgentNeedsHumanReason),
    /// Execution owners closed; the separate outcome distinguishes task
    /// success from a fully drained failure/cancellation.
    Terminal,
    /// Exact retained ownership needs reconciliation; this is not success.
    Recovery,
}

/// Stable content-free correlation for a product event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentWorkEvent {
    run: ContextRunId,
    sequence: u64,
    kind: AgentWorkEventKind,
    elapsed_millis: u64,
}

impl AgentWorkEvent {
    /// Exact run identity, never derived from page/model contents.
    pub const fn run(self) -> ContextRunId {
        self.run
    }
    /// Strictly increasing event identity within this run.
    pub const fn sequence(self) -> u64 {
        self.sequence
    }
    /// Closed product phase.
    pub const fn kind(self) -> AgentWorkEventKind {
        self.kind
    }
    /// Monotonic wall time since this actor's event journal was constructed.
    pub const fn elapsed_millis(self) -> u64 {
        self.elapsed_millis
    }
}

/// Closed failure vocabulary; no variant contains page, provider or user text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentWorkFailure {
    /// Runtime cancellation or human revocation won admission.
    Cancelled,
    /// Human takeover requested revocation; no transfer is implied by recovery.
    HumanTakeover,
    /// Suspension requested revocation; native suspension is not fabricated.
    SuspendRequested,
    /// Trusted policy authority was revoked.
    PolicyRevoked,
    /// The caller's absolute deadline expired.
    Deadline,
    /// Bounded product event storage requires a consumer to drain it.
    Backpressure,
    /// Runtime callback/control correlation could not be reconciled.
    Mailbox,
    /// Exact atomic-close refusal and first trusted stop request, if present.
    RuntimeTerminal {
        /// Runtime-owned claim refusal, never flattened into generic failure.
        refusal: AgentRuntimeControllerTerminalRefusal,
        /// Product control reason; a request is not native acknowledgement.
        stop: Option<AgentRuntimeStopReason>,
    },
    /// Context/profile/native state refused the exact transition.
    Context,
    /// Exact content-free native port refusal.
    Native(ContextPortFailure),
    /// Exact content-free semantic runtime refusal.
    Observation(SemanticRuntimePortFailure),
    /// Renderer or navigation authority changed; stale refs are revoked.
    ContextLost,
    /// Provider or action authority returned a closed refusal.
    Browser(AgentBrowserProviderError),
    /// A progress/receipt/metric join was refused.
    Accounting,
    /// Durable audit delivery or acknowledgement was refused.
    Audit,
    /// Native or provider resources could not prove terminal drain.
    Shutdown,
    /// Trusted input, clock, state, or task contract was invalid.
    Contract,
}

pub(super) struct WorkEvents {
    waker: Option<std::task::Waker>,
    started: Instant,
    run: ContextRunId,
    next: u64,
    queue: VecDeque<AgentWorkEvent>,
    fault: bool,
}

impl WorkEvents {
    pub(super) fn new(run: ContextRunId) -> Result<Self, AgentWorkFailure> {
        let mut queue = VecDeque::new();
        queue
            .try_reserve_exact(MAX_AGENT_WORK_EVENTS)
            .map_err(|_| AgentWorkFailure::Backpressure)?;
        Ok(Self {
            run,
            waker: None,
            started: Instant::now(),
            next: 1,
            queue,
            fault: false,
        })
    }

    pub(super) fn publish(&mut self, kind: AgentWorkEventKind) -> Result<(), AgentWorkFailure> {
        if self.fault || self.queue.len() == MAX_AGENT_WORK_EVENTS {
            self.fault = true;
            return Err(AgentWorkFailure::Backpressure);
        }
        let sequence = self.next;
        self.next = sequence
            .checked_add(1)
            .ok_or(AgentWorkFailure::Backpressure)?;
        self.queue.push_back(AgentWorkEvent {
            run: self.run,
            sequence,
            kind,
            elapsed_millis: u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
        });
        self.wake();
        Ok(())
    }

    fn wake(&self) {
        if let Some(waker) = &self.waker {
            let _notified =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake_by_ref()));
        }
    }
}

pub(super) fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    match value.lock() {
        Ok(value) => value,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// The exact supervisor, receipt reducers and durable ledger for one run.
/// It remains inside the session while provider/native authorities are live.
pub(crate) struct WorkJournal {
    model_started: Option<Instant>,
    pub(super) supervisor: AgentRunSupervisor,
    pub(super) execution: Option<AgentNodeExecution>,
    cancellation: AgentSupervisorCancellationId,
    pub(super) audit: AgentAuditLedger,
    pub(super) accounting: AgentRunAccountingMetrics,
    pub(super) progress: AgentRunProgressMetrics,
    pub(super) actions: AgentRunActionPerformanceMetrics,
    pub(super) inputs: AgentRunProviderInputMetrics,
    pub(super) root: AgentPlanNodeId,
    events: Arc<Mutex<WorkEvents>>,
    clock: Arc<dyn TerraControllerClock>,
    last_at: AgentPolicyInstant,
    next_audit: u64,
}

impl WorkJournal {
    pub(super) fn new(
        manifest: &AgentRunManifest,
        root: AgentPlanNodeId,
        id: AgentSupervisorId,
        cancellation: AgentSupervisorCancellationId,
        clock: Arc<dyn TerraControllerClock>,
        events: Arc<Mutex<WorkEvents>>,
    ) -> Result<Self, AgentWorkFailure> {
        let topology =
            AgentDelegationTopology::try_new(manifest, vec![AgentDelegationSpec::new(root, None)])
                .map_err(|_| AgentWorkFailure::Contract)?;
        let supervisor = AgentRunSupervisor::new(id, topology);
        let audit = AgentAuditLedger::try_new(manifest, &supervisor)
            .map_err(|_| AgentWorkFailure::Accounting)?;
        let accounting = AgentRunAccountingMetrics::try_new(manifest, &supervisor)
            .map_err(|_| AgentWorkFailure::Accounting)?;
        let progress = AgentRunProgressMetrics::try_new(manifest, &supervisor)
            .map_err(|_| AgentWorkFailure::Accounting)?;
        let actions = AgentRunActionPerformanceMetrics::try_new(manifest, &supervisor)
            .map_err(|_| AgentWorkFailure::Accounting)?;
        let inputs = AgentRunProviderInputMetrics::try_new(manifest, &supervisor)
            .map_err(|_| AgentWorkFailure::Accounting)?;
        Ok(Self {
            supervisor,
            model_started: None,
            execution: None,
            cancellation,
            audit,
            accounting,
            progress,
            actions,
            inputs,
            root,
            events,
            clock,
            last_at: manifest.issued_at(),
            next_audit: 1,
        })
    }

    pub(super) fn emit(&self, kind: AgentWorkEventKind) -> Result<(), AgentWorkFailure> {
        lock(&self.events).publish(kind)
    }

    pub(super) fn record(&mut self) -> Result<(), AgentWorkFailure> {
        let now = self.clock.now().map_err(|_| AgentWorkFailure::Contract)?;
        if now < self.last_at {
            return Err(AgentWorkFailure::Contract);
        }
        let id = AgentAuditEventId::new(self.next_audit).ok_or(AgentWorkFailure::Accounting)?;
        self.next_audit = self
            .next_audit
            .checked_add(1)
            .ok_or(AgentWorkFailure::Accounting)?;
        let event = self
            .audit
            .record_current(&self.supervisor, self.root, id, now)
            .map_err(|_| AgentWorkFailure::Audit)?;
        self.last_at = now;
        self.progress
            .record_event(event)
            .map_err(|_| AgentWorkFailure::Accounting)
    }

    pub(super) fn start(
        &mut self,
        attempt: AgentSupervisorAttemptId,
    ) -> Result<(), AgentWorkFailure> {
        self.record()?;
        self.execution = Some(
            self.supervisor
                .start(self.root, attempt)
                .map_err(|_| AgentWorkFailure::Contract)?,
        );
        self.record()?;
        self.emit(AgentWorkEventKind::Started)
    }

    pub(super) fn model_active(
        &mut self,
        call: AgentProviderCallIdentity,
    ) -> Result<(), AgentWorkFailure> {
        if self.model_started.replace(Instant::now()).is_some() {
            return Err(AgentWorkFailure::Accounting);
        }
        self.supervisor
            .record_active_model_call(
                self.execution.as_ref().ok_or(AgentWorkFailure::Contract)?,
                call,
            )
            .map_err(|_| AgentWorkFailure::Accounting)?;
        self.record()?;
        self.emit(AgentWorkEventKind::ModelActive)
    }

    pub(crate) fn needs_human(
        &mut self,
        transition: AgentNeedsHumanTransition,
    ) -> Result<(), AgentWorkFailure> {
        self.supervisor
            .wait_for_human(
                self.execution.as_ref().ok_or(AgentWorkFailure::Contract)?,
                transition,
            )
            .map_err(|_| AgentWorkFailure::Accounting)?;
        self.execution.take();
        self.record()?;
        self.emit(AgentWorkEventKind::NeedsHuman(transition.reason()))
    }

    pub(super) fn model_settled(
        &mut self,
        receipt: AgentModelCallReceipt,
        input: AgentProviderInputMetricReceipt,
    ) -> Result<(), AgentWorkFailure> {
        self.accounting
            .record_model_receipt(receipt)
            .map_err(|_| AgentWorkFailure::Accounting)?;
        self.inputs
            .record(input)
            .map_err(|_| AgentWorkFailure::Accounting)?;
        self.supervisor
            .record_model_call_result(
                self.execution.as_ref().ok_or(AgentWorkFailure::Contract)?,
                receipt,
            )
            .map_err(|_| AgentWorkFailure::Accounting)?;
        self.record()?;
        let elapsed_millis = u64::try_from(
            self.model_started
                .take()
                .ok_or(AgentWorkFailure::Accounting)?
                .elapsed()
                .as_millis(),
        )
        .map_err(|_| AgentWorkFailure::Contract)?;
        self.emit(AgentWorkEventKind::ModelSettled {
            call: receipt.id(),
            input_tokens: receipt.input_tokens(),
            output_tokens: receipt.output_tokens(),
            cost_micro_usd: receipt.cost_micro_usd(),
            request_bytes: input.metrics().serialized_request_bytes(),
            semantic_bytes: input.metrics().semantic().disclosed_bytes(),
            accounting: receipt.usage_accounting(),
            elapsed_millis,
        })
    }

    pub(crate) fn action_active(
        &mut self,
        effect: &AgentActiveEffect,
    ) -> Result<(), AgentWorkFailure> {
        self.supervisor
            .record_active_effect(
                self.execution.as_ref().ok_or(AgentWorkFailure::Contract)?,
                effect,
            )
            .map_err(|_| AgentWorkFailure::Accounting)?;
        self.record()?;
        self.emit(AgentWorkEventKind::ActionActive)
    }

    pub(super) fn action_settled(
        &mut self,
        receipt: AgentEffectReceipt,
        batch: &SemanticActionBatchResult,
    ) -> Result<(), AgentWorkFailure> {
        self.accounting
            .record_effect_receipt(receipt)
            .map_err(|_| AgentWorkFailure::Accounting)?;
        self.actions
            .record_batch_result(batch)
            .map_err(|_| AgentWorkFailure::Accounting)?;
        self.supervisor
            .record_effect_result(
                self.execution.as_ref().ok_or(AgentWorkFailure::Contract)?,
                receipt,
            )
            .map_err(|_| AgentWorkFailure::Accounting)?;
        self.record()?;
        self.emit(AgentWorkEventKind::Verified)
    }
}

impl fmt::Debug for WorkJournal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WorkJournal([owned, content-free])")
    }
}
