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

#[path = "work_inspection.rs"]
mod inspection;
#[path = "work_navigation.rs"]
mod navigation;

#[path = "work_retained.rs"]
mod retained;
use retained::WorkBrowser;
pub use retained::{
    AgentWorkRetainedBrowser, AgentWorkRetainedController, AgentWorkRetainedHandle,
    AgentWorkRetainedOutcome, AgentWorkRetainedRecovery,
};

/// Trusted task-level decision from fresh independently collected state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentWorkTaskProgress {
    /// The approved task still requires work.
    Continue,
    /// Fresh trusted departure state permits the next frozen exact checkpoint.
    ReadyForNavigation,
    /// Fresh trusted state satisfies the action postcondition. Further actions
    /// are refused; extraction requires the registered schema and result predicate.
    ReadyForExtraction,
    /// The product's own task predicate is satisfied.
    Complete,
}

/// Trusted pre-model readiness of the initial document, never action progress.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentWorkInitialReadiness {
    /// The task can evaluate this fresh observation normally.
    Ready,
    /// The exact task context exists but has not finished becoming usable.
    /// The controller may obtain a bounded number of fresh read-only samples.
    Pending,
}

/// A document can finish navigating while its initial application shell still
/// consists of loading placeholders. This is scheduling evidence only: neither
/// its presence nor absence grants action authority or proves task readiness.
///
/// A cohort must dominate the usable controls before it delays startup. Named
/// meters, nonzero progress, and an isolated spinner are normal page content;
/// treating every progress role as a loading document would block dashboards.
fn has_dominant_loading_placeholders(observation: &SemanticObservation) -> bool {
    let mut placeholders = 0usize;
    let mut controls = 0usize;
    for node in observation
        .frames()
        .iter()
        .flat_map(SemanticSnapshot::nodes)
    {
        let anonymous = node.name().is_none_or(SemanticText::is_empty)
            && node.text().is_none_or(SemanticText::is_empty);
        if node.role() == SemanticRole::Progress
            && anonymous
            && matches!(node.value(), None | Some(SemanticValueSummary::Ordinal(0)))
        {
            placeholders += 1;
        } else if !anonymous
            && !node.states().contains(SemanticState::Disabled)
            && [
                SemanticOperationClass::Click,
                SemanticOperationClass::Fill,
                SemanticOperationClass::Select,
                SemanticOperationClass::Press,
            ]
            .into_iter()
            .any(|operation| node.operations().contains(operation))
        {
            controls += 1;
        }
    }
    placeholders >= 3 && placeholders > controls
}

fn finish_initial_readiness_wait(
    observation: SemanticObservation,
    readiness: AgentWorkInitialReadiness,
) -> Result<SemanticObservation, AgentWorkFailure> {
    match readiness {
        // Loading placeholders are page-controlled scheduling evidence. They
        // may buy the application bounded hydration time, but they must never
        // let a page veto a task that its trusted predicate considers ready.
        AgentWorkInitialReadiness::Ready => Ok(observation),
        AgentWorkInitialReadiness::Pending => Err(AgentWorkFailure::Observation(
            SemanticRuntimePortFailure::NotReady,
        )),
    }
}

fn validate_initial_readiness_successor(
    prior: &SemanticObservation,
    next: &SemanticObservation,
) -> Result<(), AgentWorkFailure> {
    if prior.request().context() != next.request().context()
        || prior.request().scope() != next.request().scope()
        || prior.request().generation() != next.request().generation()
        || prior.request().id() == next.request().id()
        || prior.frames().len() != next.frames().len()
        || prior.frames().iter().zip(next.frames()).any(|(a, b)| {
            a.frame() != b.frame()
                || a.invocation() == b.invocation()
                || a.generation() >= b.generation()
        })
    {
        return Err(AgentWorkFailure::Context);
    }
    Ok(())
}

/// Trusted product execution contract. Never implement this from model text,
/// page instructions, or a model-authored predicate. The UI does not receive
/// this port; it receives only the content-free handle below.
pub trait AgentWorkTask: Send {
    /// Read-only startup gate, called only before the first task evaluation or
    /// provider request. Pending grants no effect/ref authority or progress.
    /// Implementations must reject changed/ambiguous task identity rather than
    /// treating arbitrary missing content as hydration. Ready is the default;
    /// the controller's generic loading-placeholder gate still applies.
    fn initial_readiness(
        &self,
        _: &SemanticObservation,
    ) -> Result<AgentWorkInitialReadiness, AgentWorkFailure> {
        Ok(AgentWorkInitialReadiness::Ready)
    }
    /// One immutable same-origin exact destination. The first navigation task
    /// shape is read-only, initial-extraction only, with no redirects/repeats.
    /// The task must independently prove departure and arrival from fresh state.
    fn navigation_target(&self) -> Option<&ContextNavigationTarget> {
        None
    }
    /// Optional immutable finite route, exactly equal to the selected manifest
    /// node's route. Mutually exclusive with the legacy one-hop target. Every
    /// intermediate document must independently prove arrival and departure;
    /// only the final document may become ready for extraction.
    fn navigation_route(&self) -> Option<&AgentNavigationRoute> {
        None
    }
    /// Frozen public observed-link discovery authority, mutually exclusive with
    /// fixed routes. The model chooses a route and answer inside this scope.
    fn navigation_discovery(&self) -> Option<&AgentNavigationDiscovery> {
        None
    }
    /// Opts into nonterminal public reads of the exact initial baseline. Reads
    /// grant no task progress, new observation/ref or native effect authority.
    /// This setting is frozen at admission with the other task capabilities.
    fn allows_baseline_read(&self) -> bool {
        false
    }
    /// Frozen opt-in to exact-reference native inspection and fresh delivery.
    fn allows_progressive_observation(&self) -> bool {
        false
    }
    /// Explicitly permits one-shot viewport captures when semantic inspection
    /// cannot represent a visual/layout question. Captures remain bound to an
    /// acknowledged complete public observation and the provider disclosure
    /// policy; this opt-in alone grants neither pixels nor native authority.
    fn allows_viewport_screenshot(&self) -> bool {
        false
    }
    /// Enables bounded standalone waits over semantic change or one exact
    /// target-state predicate. The model cannot select timer-only success.
    fn allows_standalone_wait(&self) -> bool {
        false
    }
    /// Lets the model stop cleanly and request a person using one closed reason.
    /// This grants no resume capability; successor admission belongs to the host.
    fn allows_human_request(&self) -> bool {
        false
    }
    /// Enables a native Back step only when policy and the retained platform
    /// both prove an exact predecessor enrolled by this run.
    fn allows_history_back(&self) -> bool {
        false
    }
    /// Explicitly permits one terminal native subtree read anchored to the
    /// exact model-acknowledged observation. Frozen at admission; default is
    /// initial-scope only. This grants no action or navigation authority.
    fn allows_subtree_extraction(&self) -> bool {
        false
    }
    /// Explicitly enables verified snapshot actions before extraction. Default
    /// schema tasks remain read-only. This setting and the schema are frozen at
    /// admission; it grants no effect permission beyond the original policy.
    fn allows_actions_before_extraction(&self) -> bool {
        false
    }
    /// Returns the task-approved operation subset for one node in the exact
    /// freshly evaluated observation. The controller intersects this with the
    /// node's semantic operations before constructing provider-visible action
    /// affordances. The default denies every action.
    fn model_action_operations(
        &self,
        _: &SemanticNode,
        _: &SemanticObservation,
    ) -> Result<SemanticOperations, AgentWorkFailure> {
        Ok(SemanticOperations::NONE)
    }
    /// Advances trusted task state from one independently verified native
    /// action terminal. The default has no action-progress contract.
    fn accept_verified_action(
        &mut self,
        _: &SemanticActionBatchResult,
        _: &SemanticObservation,
    ) -> Result<(), AgentWorkFailure> {
        Ok(())
    }
    /// Whether terminal extraction is currently authorized by trusted task
    /// state. This is dynamic progress, not a frozen capability grant.
    fn terminal_extraction_ready(&self) -> bool {
        true
    }
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
    /// Assesses a proposal against the controller's exact current observation.
    /// Open objectives use this port to resolve model-selected targets without
    /// accepting caller-provided refs. Existing frozen task assessors retain
    /// their own independently bound baseline through the default implementation.
    fn assess_observed(
        &self,
        action: &SemanticPreparedAction,
        _: &SemanticObservation,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        self.assess(action)
    }
    /// Supplies independently sourced current account facts for this exact
    /// context. Called at startup and before each provider/effect admission,
    /// including nonterminal inspection and extraction mapping. It must be
    /// bounded and nonblocking. Do not renew a cached sample's timestamp: return
    /// its original identity/time or obtain a new sample from the trusted
    /// adapter. A changed account/context refuses the run, not a scope switch.
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
    document_policy: WorkBrowserDocumentPolicy,
}

impl AgentWorkContextSpec {
    /// Validates one owned page and exact initial target; manifest scope is
    /// independently checked when constructing the run.
    pub fn try_new(
        identity: ContextIdentity,
        storage: ContextProfileStorageClass,
        target: ContextNavigationTarget,
    ) -> Result<Self, AgentWorkFailure> {
        Self::try_new_with_document_policy(
            identity,
            storage,
            target,
            WorkBrowserDocumentPolicy::Exact,
        )
    }

    /// Validates one trusted initial-document policy together with the exact
    /// owned target. This policy is application-authored before native work;
    /// it grants no model or successor-navigation authority.
    pub fn try_new_with_document_policy(
        identity: ContextIdentity,
        storage: ContextProfileStorageClass,
        target: ContextNavigationTarget,
        document_policy: WorkBrowserDocumentPolicy,
    ) -> Result<Self, AgentWorkFailure> {
        if identity.kind() != ContextKind::Owned {
            return Err(AgentWorkFailure::Contract);
        }
        let origin = SemanticOrigin::parse(target.as_url().as_ref())
            .map_err(|_| AgentWorkFailure::Contract)?;
        if !document_policy.admits_request(&target) {
            return Err(AgentWorkFailure::Contract);
        }
        Ok(Self {
            identity,
            storage,
            target,
            origin,
            document_policy,
        })
    }
}

/// Bounded model/run configuration supplied by the trusted application.
pub struct AgentWorkRunSettings {
    model: AgentBrowserModel,
    ids: TerraControllerIds,
    clock: Arc<dyn TerraControllerClock>,
    deadline: Instant,
    max_model_calls: u8,
    max_actions: u64,
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
            max_model_calls: super::MAX_BROWSER_MODEL_TURNS,
            max_actions: super::MAX_BROWSER_ACTIONS,
        }
    }

    /// Sets this run's total provider-call allowance, including terminal
    /// mapping (2–64; default 8). This trusted application setting grants no
    /// manifest operation, token, cost, scope or deadline authority. Stateless
    /// context retention and provider input bounds remain independently enforced.
    pub fn with_max_model_calls(mut self, max_calls: u8) -> Result<Self, AgentWorkFailure> {
        if !(2..=super::MAX_WORK_MODEL_CALLS).contains(&max_calls) {
            return Err(AgentWorkFailure::Contract);
        }
        self.max_model_calls = max_calls;
        Ok(self)
    }

    /// Sets the native action-attempt allowance (0–64; default 8). Zero
    /// disables action admission. Each attempt still needs the approved
    /// manifest's effect authority and remaining operation budget.
    pub fn with_max_actions(mut self, max_actions: u64) -> Result<Self, AgentWorkFailure> {
        if max_actions > super::MAX_WORK_ACTIONS {
            return Err(AgentWorkFailure::Contract);
        }
        self.max_actions = max_actions;
        Ok(self)
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
    /// Descriptive original resource construction operands for the trusted
    /// application. These do not mint a browser, account or execution lease.
    pub fn retained_resource_spec(
        &self,
    ) -> Result<AgentWorkRetainedResourceSpec, AgentWorkFailure> {
        Ok(AgentWorkRetainedResourceSpec {
            identity: self.context.identity,
            storage: self.context.storage,
            target: self.context.target.clone(),
            document_policy: self.context.document_policy,
            clock: self.settings.clock.clone(),
            deadline: self.settings.deadline,
            expires_at: self
                .manifest
                .plan_node(self.lease.node())
                .ok_or(AgentWorkFailure::Contract)?
                .expires_at(),
        })
    }
    /// Joins approved scope, explicit profile, bounded objective and model.
    pub fn try_new(
        manifest: AgentRunManifest,
        lease: AgentPlanLeaseBinding,
        context: AgentWorkContextSpec,
        objective: String,
        settings: AgentWorkRunSettings,
    ) -> Result<Self, AgentWorkFailure> {
        Self::try_new_with_monotonic_now(
            manifest,
            lease,
            context,
            objective,
            settings,
            Instant::now,
        )
    }

    fn try_new_with_monotonic_now(
        manifest: AgentRunManifest,
        lease: AgentPlanLeaseBinding,
        context: AgentWorkContextSpec,
        objective: String,
        settings: AgentWorkRunSettings,
        monotonic_now: impl FnOnce() -> Instant,
    ) -> Result<Self, AgentWorkFailure> {
        let root = manifest
            .plan_node(lease.node())
            .ok_or(AgentWorkFailure::Contract)?;
        let now = settings
            .clock
            .now()
            .map_err(|_| AgentWorkFailure::Contract)?;
        // Policy time floors elapsed milliseconds. Sample it first: comparing
        // an earlier, larger wall horizon with a later, smaller policy remainder
        // can falsely reject the same absolute deadline at a millisecond edge.
        // Neither approved expiry nor the caller's absolute deadline is changed;
        // later execution still independently enforces both original bounds.
        let horizon = settings
            .deadline
            .checked_duration_since(monotonic_now())
            .ok_or(AgentWorkFailure::Deadline)?;
        if horizon.is_zero() || horizon > super::MAX_TERRA_CONTROLLER_HARD_DEADLINE {
            return Err(AgentWorkFailure::Deadline);
        }
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

/// Original approved input facts, not resource or execution authority.
pub struct AgentWorkRetainedResourceSpec {
    /// Exact context/run/profile identity in the approved input.
    pub identity: ContextIdentity,
    /// Original selected session persistence class.
    pub storage: ContextProfileStorageClass,
    /// Exact approved initial document, not navigation authority.
    pub target: ContextNavigationTarget,
    /// Trusted initial-document construction policy, frozen before native work.
    pub document_policy: WorkBrowserDocumentPolicy,
    /// Original policy clock, shared with controller admission.
    pub clock: Arc<dyn TerraControllerClock>,
    /// Original absolute execution deadline.
    pub deadline: Instant,
    /// Original plan-node policy expiry ceiling for resource acquisition.
    pub expires_at: AgentPolicyInstant,
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
    /// Read-only drain status. This is not terminal proof; the application
    /// also retains the original outcome and joined runtime lifecycle.
    pub fn has_pending_events(&self) -> bool {
        !lock(&self.events).queue.is_empty()
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
    /// The current run closed cleanly after the model requested a person. All
    /// execution authority is revoked; only a trusted host may admit a fresh
    /// successor run.
    WaitingForHuman(AgentWorkWaitingForHuman),
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
    human_review: Option<AgentNeedsHumanTransition>,
}

/// Durable, non-authorizing model request for human intervention.
///
/// It is bound to the exact delivered observation at which the run stopped.
/// It carries no provider continuation, old ref authority, or resume token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentWorkHumanRequest {
    context: ContextJoin,
    observation: SemanticObservationId,
    generation: SemanticObservationGeneration,
    reason: AgentBrowserHumanReason,
    retained_resource: Option<WorkBrowserResourceIdentity>,
}

impl AgentWorkHumanRequest {
    /// Exact document/cancellation authority at the handoff request.
    pub const fn context(self) -> ContextJoin {
        self.context
    }

    /// Exact model-delivered observation at the handoff request.
    pub const fn observation(self) -> SemanticObservationId {
        self.observation
    }

    /// Exact progressive-observation generation at the handoff request.
    pub const fn generation(self) -> SemanticObservationGeneration {
        self.generation
    }

    /// Closed model-selected reason for requesting a person.
    pub const fn reason(self) -> AgentBrowserHumanReason {
        self.reason
    }

    /// Exact retained resource whose run lease was revoked at handoff. Legacy
    /// owned contexts return `None` because clean closure destroys that page.
    pub const fn retained_resource(self) -> Option<WorkBrowserResourceIdentity> {
        self.retained_resource
    }
}

/// Clean terminal handoff. This contains durable accounting and descriptive
/// correlation only: no provider continuation, native lease, refs, or model
/// authority survives the stopped run.
pub struct AgentWorkWaitingForHuman {
    settlement: AgentRunPolicySettlement,
    request: AgentWorkHumanRequest,
}

impl AgentWorkWaitingForHuman {
    /// Closed model request bound to the last delivered observation.
    pub const fn request(&self) -> AgentWorkHumanRequest {
        self.request
    }

    /// Original clean policy/accounting closure for the stopped actor.
    pub const fn policy_settlement(&self) -> AgentRunPolicySettlement {
        self.settlement
    }

    /// Content-free metric closure for product persistence and diagnostics.
    pub const fn closure(&self) -> AgentRunMetricClosure {
        self.settlement.closure()
    }
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
    /// Exact never-dispatched refusal after original resource/policy closure.
    /// This classifies review; it grants no action or continuation authority.
    pub const fn human_review(&self) -> Option<AgentNeedsHumanTransition> {
        self.human_review
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

/// Trusted public-data extraction task, initially limited to the initial read.
/// The product may opt into one acknowledged-ref subtree capture. The schema
/// is frozen before admission; no native action is allowed.
pub struct AgentWorkExtractionTask {
    baseline_read: bool,
    progressive_observation: bool,
    schema: SemanticExtractionSchema,
    account: AgentAccountScope,
    account_sample: std::cell::Cell<Option<AgentContextAccountBinding>>,
    subtree: bool,
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
        Ok(Self {
            schema,
            account,
            account_sample: std::cell::Cell::new(None),
            subtree: false,
            baseline_read: false,
            progressive_observation: false,
        })
    }

    /// Allows initial scope or one exact acknowledged-ref native subtree read.
    /// Field, sensitivity, read and provider ceilings remain unchanged.
    pub fn with_subtree_extraction(mut self) -> Self {
        self.subtree = true;
        self
    }

    /// Narrows extraction evidence to trusted semantic roles after the existing
    /// native capture. Frozen with the schema; all privacy and byte ceilings
    /// remain unchanged. Baseline inspection and native scope are unaffected.
    pub fn with_source_roles(mut self, roles: SemanticReadRoleSelection) -> Self {
        self.schema = self.schema.with_source_roles(roles);
        self
    }

    /// Allows nonterminal public inspection of the existing initial baseline
    /// before selecting the terminal extraction scope. No fresh capture or
    /// mapping result is implied by a read.
    pub fn with_baseline_read(mut self) -> Self {
        self.baseline_read = true;
        self
    }

    /// Allows bounded, reference-anchored expansion of the acknowledged
    /// current document before terminal mapping. This grants neither
    /// navigation nor native action authority and requires baseline reads so
    /// the model can inspect the evidence from which it selects a scope.
    pub fn with_progressive_observation(mut self) -> Self {
        self.progressive_observation = true;
        self
    }
}
impl AgentWorkTask for AgentWorkExtractionTask {
    fn allows_baseline_read(&self) -> bool {
        self.baseline_read
    }
    fn allows_progressive_observation(&self) -> bool {
        self.progressive_observation
    }
    fn allows_subtree_extraction(&self) -> bool {
        self.subtree
    }
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
        if let Some(sample) = self.account_sample.get() {
            return if sample.context() == context {
                Ok(sample)
            } else {
                Err(AgentWorkFailure::Contract)
            };
        }
        // Static product scope must not become synthetic freshness authority.
        let sample = AgentContextAccountBinding::new(
            AgentAccountAttestationId::generate(),
            context,
            self.account,
            now,
        );
        self.account_sample.set(Some(sample));
        Ok(sample)
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
            Self::WaitingForHuman(value) => formatter
                .debug_tuple("AgentWorkOutcome::WaitingForHuman")
                .field(&value.request.reason)
                .finish(),
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
    retained_terminal: Option<Arc<Mutex<Option<AgentWorkRetainedOutcome>>>>,
}

impl AgentWorkController {
    /// Immutable session storage selected in the trusted input, while dormant.
    pub fn profile_storage_binding(
        &self,
    ) -> Result<(AgentWorkProfileId, ContextProfileStorageClass), AgentWorkFailure> {
        let context = &self
            .state
            .as_ref()
            .and_then(|state| state.input.as_ref())
            .ok_or(AgentWorkFailure::Contract)?
            .context;
        Ok((context.identity.profile(), context.storage))
    }
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
            None,
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
        Self::with_transport(input, transport, credential, audit, task, retention, None)
    }

    fn with_transport(
        input: AgentWorkRunInput,
        transport: AgentProviderTransport,
        credential: AgentProviderCredential,
        audit: Arc<dyn AgentAuditPort>,
        task: Box<dyn AgentWorkTask>,
        retention: AgentBrowserRetention,
        retained: Option<Box<dyn AgentWorkRetainedBrowser>>,
    ) -> Result<(Self, AgentWorkHandle), AgentWorkFailure> {
        let extraction_schema = task.extraction_schema().cloned();
        let actions_before_extraction = task.allows_actions_before_extraction();
        let subtree_extraction = task.allows_subtree_extraction();
        let baseline_read = task.allows_baseline_read();
        let progressive_observation = task.allows_progressive_observation();
        let viewport_screenshot = task.allows_viewport_screenshot();
        let standalone_wait = task.allows_standalone_wait();
        let human_request = task.allows_human_request();
        let history_back = task.allows_history_back();
        let navigation_target = task.navigation_target().cloned();
        let navigation_route = task.navigation_route().cloned();
        let navigation_discovery = task.navigation_discovery().cloned();
        if progressive_observation
            && (extraction_schema.is_none()
                || !baseline_read
                || retained
                    .as_ref()
                    .is_some_and(|browser| !browser.supports_expansion()))
        {
            return Err(AgentWorkFailure::Contract);
        }
        if retained.is_some()
            && (extraction_schema.is_none()
                || (actions_before_extraction
                    && !retained
                        .as_ref()
                        .is_some_and(|browser| browser.supports_actions()))
                || subtree_extraction
                || navigation_target.is_some()
                || navigation_route.is_some()
                || (navigation_discovery.is_some()
                    && !retained
                        .as_ref()
                        .is_some_and(|browser| browser.supports_navigation()))
                || (history_back
                    && !retained
                        .as_ref()
                        .is_some_and(|browser| browser.supports_history_back()))
                || (viewport_screenshot
                    && !retained
                        .as_ref()
                        .is_some_and(|browser| browser.supports_screenshots())))
        {
            return Err(AgentWorkFailure::Contract);
        }
        let approved_route = input
            .manifest
            .plan_node(input.lease.node())
            .ok_or(AgentWorkFailure::Contract)?
            .navigation_route();
        if navigation_route.as_ref() != approved_route
            || (navigation_target.is_some() && navigation_route.is_some())
            || navigation_route.as_ref().is_some_and(|route| {
                route.departure() != &input.context.target
                    || route.origin() != &input.context.origin
            })
        {
            return Err(AgentWorkFailure::Contract);
        }
        let approved_discovery = input
            .manifest
            .plan_node(input.lease.node())
            .ok_or(AgentWorkFailure::Contract)?
            .navigation_discovery();
        if navigation_discovery.as_ref() != approved_discovery
            || navigation_discovery.as_ref().is_some_and(|scope| {
                navigation_target.is_some()
                    || navigation_route.is_some()
                    || scope.departure() != &input.context.target
                    || scope.origin() != &input.context.origin
                    || extraction_schema.is_none()
                    || subtree_extraction
            })
        {
            return Err(AgentWorkFailure::Contract);
        }
        if let Some(target) = &navigation_target {
            Self::validate_navigation_target(target, &input.context)?;
        }
        if (navigation_target.is_some() || navigation_route.is_some())
            && (extraction_schema.is_none()
                || actions_before_extraction
                || subtree_extraction
                || baseline_read)
        {
            return Err(AgentWorkFailure::Contract);
        }
        if ((input.durable_result || actions_before_extraction || subtree_extraction)
            && extraction_schema.is_none())
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
        let mut native = WorkNative::new(
            input.context.identity,
            input.context.origin.clone(),
            input.settings.deadline,
            retained,
        )?;
        native.clock = Some(input.settings.clock.clone());
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
                    extraction_schema,
                    actions_before_extraction,
                    subtree_extraction,
                    baseline_read,
                    progressive_observation,
                    viewport_screenshot,
                    standalone_wait,
                    human_request,
                    history_back,
                    navigation_target,
                    navigation_route,
                    navigation_discovery,
                    navigation_hops: 0,
                    extraction: None,
                    retained_read_evidence: SemanticRetainedReadEvidence::default(),
                    failure: None,
                    observation: None,
                    native_terminal: None,
                    model_human_request: None,
                    terminal_intent: None,
                }),
                terminal: Arc::clone(&terminal),
                retained_terminal: None,
            },
            AgentWorkHandle { events, terminal },
        ))
    }
}

struct WorkState {
    retained_read_evidence: SemanticRetainedReadEvidence,
    navigation_target: Option<ContextNavigationTarget>,
    navigation_route: Option<AgentNavigationRoute>,
    navigation_discovery: Option<AgentNavigationDiscovery>,
    navigation_hops: usize,
    baseline_read: bool,
    progressive_observation: bool,
    viewport_screenshot: bool,
    standalone_wait: bool,
    human_request: bool,
    history_back: bool,
    extraction_schema: Option<SemanticExtractionSchema>,
    actions_before_extraction: bool,
    subtree_extraction: bool,
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
    model_human_request: Option<AgentWorkHumanRequest>,
    terminal_intent: Option<WorkTerminalIntent>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkTerminalIntent {
    Succeeded,
    WaitingForHuman(AgentWorkHumanRequest),
    ClosedUnsuccessfully(AgentWorkFailure),
}

impl WorkState {
    fn navigation_length(&self) -> usize {
        if let Some(scope) = &self.navigation_discovery {
            return scope.max_hops();
        }
        self.navigation_route
            .as_ref()
            .map_or(usize::from(self.navigation_target.is_some()), |route| {
                route.destinations().len()
            })
    }

    fn has_navigation(&self) -> bool {
        self.navigation_target.is_some()
            || self.navigation_route.is_some()
            || self.navigation_discovery.is_some()
    }

    fn observation_capability(&self) -> WorkBrowserObservationCapability {
        WorkBrowserObservationCapability::for_navigation_discovery(
            self.navigation_discovery.as_ref(),
        )
    }

    fn requires_decision_budget(&self) -> bool {
        self.extraction_schema.is_some()
            && (self.has_navigation()
                || self.baseline_read
                || self.progressive_observation
                || self.viewport_screenshot
                || self.standalone_wait
                || self.human_request
                || self.history_back
                || self.actions_before_extraction
                || self.subtree_extraction)
    }

    fn navigation_complete(&self) -> bool {
        self.navigation_hops == self.navigation_length()
    }

    fn current_navigation_target(&self) -> Option<&ContextNavigationTarget> {
        if let Some(route) = &self.navigation_route {
            route.destinations().get(self.navigation_hops)
        } else if self.navigation_hops == 0 {
            self.navigation_target.as_ref()
        } else {
            None
        }
    }

    fn refresh_account(
        &mut self,
        worker: &AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
    ) -> Result<(), AgentWorkFailure> {
        self.check_task_contract()?;
        self.native.check_control(worker, browser)?;
        let context = self.native.context()?;
        let session = self.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
        session.check_live().map_err(AgentWorkFailure::Browser)?;
        let now = session.policy_now().map_err(AgentWorkFailure::Browser)?;
        let account = self.task.attest_account(context, now);
        self.native.check_control(worker, browser)?;
        let account = account?;
        let session = self.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
        session
            .refresh_account(account)
            .map_err(AgentWorkFailure::Browser)?;
        self.check_task_contract()
    }

    fn task_progress(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        self.check_task_contract()?;
        let progress = self.task.evaluate(observation)?;
        self.check_task_contract()?;
        if self.navigation_discovery.is_some() {
            if progress != AgentWorkTaskProgress::Continue {
                return Err(AgentWorkFailure::Contract);
            }
        } else if self.has_navigation() {
            let expected = if self.navigation_complete() {
                AgentWorkTaskProgress::ReadyForExtraction
            } else {
                AgentWorkTaskProgress::ReadyForNavigation
            };
            if progress != expected {
                return Err(AgentWorkFailure::Contract);
            }
        } else if progress == AgentWorkTaskProgress::ReadyForNavigation {
            return Err(AgentWorkFailure::Contract);
        }
        if (progress == AgentWorkTaskProgress::ReadyForExtraction
            && self.extraction_schema.is_none())
            || (progress == AgentWorkTaskProgress::Complete && self.actions_before_extraction)
        {
            return Err(AgentWorkFailure::Contract);
        }
        Ok(progress)
    }

    fn action_authority(
        &self,
        observation: &SemanticObservation,
    ) -> Result<AgentProviderActionAuthority, AgentWorkFailure> {
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(usize::from(observation.node_count()))
            .map_err(|_| AgentWorkFailure::Contract)?;
        for node in observation
            .frames()
            .iter()
            .flat_map(SemanticSnapshot::nodes)
        {
            let approved = self.task.model_action_operations(node, observation)?;
            let operations = [
                SemanticOperationClass::Click,
                SemanticOperationClass::Fill,
                SemanticOperationClass::Select,
                SemanticOperationClass::Press,
                SemanticOperationClass::Scroll,
            ]
            .into_iter()
            .filter(|operation| {
                node.operations().contains(*operation) && approved.contains(*operation)
            })
            .collect::<Vec<_>>();
            let operations =
                SemanticOperations::try_new(&operations).map_err(|_| AgentWorkFailure::Contract)?;
            if !operations.is_empty() {
                entries.push((node.reference(), operations));
            }
        }
        AgentProviderActionAuthority::try_new(observation, &entries)
            .ok_or(AgentWorkFailure::Contract)
    }

    fn check_task_contract(&self) -> Result<(), AgentWorkFailure> {
        if self.task.extraction_schema() != self.extraction_schema.as_ref()
            || self.task.navigation_target() != self.navigation_target.as_ref()
            || self.task.navigation_route() != self.navigation_route.as_ref()
            || self.task.navigation_discovery() != self.navigation_discovery.as_ref()
            || self.task.allows_actions_before_extraction() != self.actions_before_extraction
            || self.task.allows_subtree_extraction() != self.subtree_extraction
            || self.task.allows_baseline_read() != self.baseline_read
            || self.task.allows_progressive_observation() != self.progressive_observation
            || self.task.allows_viewport_screenshot() != self.viewport_screenshot
            || self.task.allows_standalone_wait() != self.standalone_wait
            || self.task.allows_human_request() != self.human_request
            || self.task.allows_history_back() != self.history_back
        {
            return Err(AgentWorkFailure::Contract);
        }
        Ok(())
    }
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
    proposal_refusal: Option<crate::action::AgentBrowserActionProposalRefusal>,
    policy: Option<AgentRunPolicy>,
    journal: Option<WorkJournal>,
    provider: Option<AgentProviderShutdownProof>,
    native: Option<AgentNativeShutdownCoordinator>,
    resources: Option<AgentNativeShutdownResources>,
    proof: Option<AgentNativeShutdownProof>,
    delivery: Option<WorkBrowserLeaseDeliveryProof>,
    scoped_refusal: Option<Box<zephium_agent_runtime::AgentRuntimeScopedCommitRefusal>>,
}

struct WorkNative {
    resources: Option<WorkContextResources>,
    retained: Option<Box<dyn AgentWorkRetainedBrowser>>,
    retained_delivery: Option<WorkBrowserLeaseDeliveryProof>,
    clock: Option<Arc<dyn TerraControllerClock>>,
    identity: ContextIdentity,
    origin: SemanticOrigin,
    profile: Option<ContextProfileLease>,
    operation: Option<ContextOperationJoin>,
    recovery_close: Option<ContextOperationJoin>,
    observation: Option<SemanticRuntimeCorrelation>,
    snapshot_generation: Option<SemanticSnapshotGeneration>,
    screenshots: SemanticScreenshotCoordinator,
    screenshot_pending: Option<SemanticScreenshotPending>,
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
}

impl WorkNative {
    // A synchronous trusted adapter cannot yield to the provider/event pump.
    // Recheck sticky controls after it returns, before admitting more work.
    fn check_control(
        &mut self,
        worker: &AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
    ) -> Result<(), AgentWorkFailure> {
        self.check_stop(worker, browser)?;
        self.check_retained_health()
    }

    // Exact in-flight terminal reconciliation must obey sticky controls even
    // when document/resource authority is already gone.
    fn check_stop(
        &mut self,
        worker: &AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
    ) -> Result<(), AgentWorkFailure> {
        let failure = if worker.shutdown_deadline().is_some() {
            Some(AgentWorkFailure::Shutdown)
        } else if worker.status().cancelled() {
            Some(match worker.stop_reason() {
                Some(AgentRuntimeStopReason::HumanTakeover) => AgentWorkFailure::HumanTakeover,
                Some(AgentRuntimeStopReason::Suspend) => AgentWorkFailure::SuspendRequested,
                Some(AgentRuntimeStopReason::PolicyRevoked) => AgentWorkFailure::PolicyRevoked,
                _ => AgentWorkFailure::Cancelled,
            })
        } else if worker.status().mailbox_fault().is_some() {
            Some(AgentWorkFailure::Mailbox)
        } else if Instant::now() >= self.deadline {
            Some(AgentWorkFailure::Deadline)
        } else {
            None
        };
        if let Some(failure) = failure {
            self.revoke(browser)?;
            return Err(failure);
        }
        Ok(())
    }
    fn new(
        identity: ContextIdentity,
        origin: SemanticOrigin,
        deadline: Instant,
        retained: Option<Box<dyn AgentWorkRetainedBrowser>>,
    ) -> Result<Self, AgentWorkFailure> {
        let mut deferred = Vec::new();
        deferred
            .try_reserve_exact(super::MAX_DEFERRED_RUNTIME_EVENTS)
            .map_err(|_| AgentWorkFailure::Backpressure)?;
        Ok(Self {
            resources: retained.is_none().then(|| WorkContextResources {
                contexts: ContextRegistry::new(),
                profiles: ContextProfileLeaseRegistry::new(),
                cookies: ContextCookieTransferRegistry::new(),
            }),
            retained,
            retained_delivery: None,
            clock: None,
            identity,
            origin,
            profile: None,
            operation: None,
            recovery_close: None,
            observation: None,
            snapshot_generation: None,
            screenshots: SemanticScreenshotCoordinator::new(),
            screenshot_pending: None,
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
    fn context(&mut self) -> Result<ContextJoin, AgentWorkFailure> {
        if let Some(retained) = &self.retained {
            Ok(retained.binding().frame().context())
        } else {
            let id = self.identity.id();
            self.contexts()?
                .join(id)
                .map_err(|_| AgentWorkFailure::Context)
        }
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

    fn revoke(&mut self, browser: &WorkBrowser<'_>) -> Result<(), AgentWorkFailure> {
        if self.revoked {
            return Ok(());
        }
        self.revoked = true;
        if let Some(retained) = self.retained.as_mut() {
            return retained.begin_revocation();
        }
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
        browser: &WorkBrowser<'_>,
    ) -> Result<AgentRuntimeEvent, AgentWorkFailure> {
        let deadline = worker
            .shutdown_deadline()
            .map_or(self.deadline, |at| at.min(self.deadline));
        if Instant::now() >= deadline {
            self.revoke(browser)?;
            return Err(AgentWorkFailure::Deadline);
        }
        let health = if self.revoked {
            None
        } else {
            self.retained.as_ref()
        };
        let clock = &self.clock;
        let event = {
            let event = worker.next_event();
            tokio::pin!(event);
            tokio::time::timeout_at(
                tokio::time::Instant::from_std(deadline),
                std::future::poll_fn(|cx| {
                    if let Some(retained) = health {
                        let result = clock
                            .as_ref()
                            .ok_or(AgentWorkFailure::Contract)
                            .and_then(|clock| clock.now().map_err(|_| AgentWorkFailure::Contract))
                            .and_then(|now| retained.check_health(now));
                        if let Err(error) = result {
                            return std::task::Poll::Ready(Err(error));
                        }
                    }
                    std::future::Future::poll(event.as_mut(), cx)
                        .map(|event| event.map_err(|_| AgentWorkFailure::Mailbox))
                }),
            )
            .await
        };
        let event = match event {
            Ok(Ok(event)) => event,
            Ok(Err(failure)) => {
                self.revoke(browser)?;
                return Err(failure);
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
        browser: &WorkBrowser<'_>,
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
            let browser = WorkBrowser::Legacy(&browser);
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
        browser: &WorkBrowser<'_>,
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
        if state.native.retained.is_some() {
            journal.emit(AgentWorkEventKind::ContextActive)?;
            self.start_session()?;
            self.browser_loop(worker, browser).await?;
            return self.close_retained(worker, None).await;
        }
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
        let context = state.native.context()?;
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
            max_model_calls: input.settings.max_model_calls,
            max_actions: input.settings.max_actions,
        };
        let decision_budget = state.requires_decision_budget();
        let mut session = AgentBrowserSession::try_new_with_transport(
            run,
            state.transport.take().ok_or(AgentWorkFailure::Contract)?,
            state.credential.take().ok_or(AgentWorkFailure::Contract)?,
            input.settings.model,
            state.retention,
        )
        .map_err(AgentWorkFailure::Browser)?;
        if state.navigation_discovery.is_some() {
            if let Some(retained) = &state.native.retained {
                session
                    .policy
                    .bind_retained_initial_document(retained.binding())
                    .map_err(|_| AgentWorkFailure::Contract)?;
            }
        }
        session.journal = state.journal.take();
        if state.extraction_schema.is_some() {
            session.config = match (state.actions_before_extraction, state.subtree_extraction) {
                (true, true) => session.config.restrict_to_actions_and_scoped_extraction(),
                (false, true) => session.config.restrict_to_scoped_extraction(),
                (true, false) => session.config.restrict_to_actions_and_extraction(),
                (false, false) => session.config.restrict_to_extraction(),
            };
        }
        if state.has_navigation() {
            session.config = if state.actions_before_extraction {
                session
                    .config
                    .restrict_to_navigation_actions_and_extraction()
            } else {
                session.config.restrict_to_navigation_and_extraction()
            };
        }
        if state.baseline_read {
            session.config = session.config.with_baseline_read();
        }
        if state.progressive_observation {
            session.config = session.config.with_progressive_observation();
        }
        if state.viewport_screenshot {
            session.config = session.config.with_viewport_screenshot();
        }
        if state.standalone_wait {
            session.config = session.config.with_standalone_wait();
        }
        if state.human_request {
            session.config = session.config.with_human_request();
        }
        if state.history_back {
            session.config = session.config.with_history_back();
        }
        if decision_budget {
            session.config = session
                .config
                .with_decision_budget(
                    AgentModelCallId::new(session.next_call).ok_or(AgentWorkFailure::Contract)?,
                    session.max_model_calls,
                )
                .map_err(|_| AgentWorkFailure::Contract)?;
        }
        state.session = Some(session);
        Ok(())
    }

    async fn observe(
        state: &mut WorkState,
        worker: &mut AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
    ) -> Result<SemanticObservation, AgentWorkFailure> {
        state.journal_mut()?.emit(AgentWorkEventKind::Observing)?;
        if state
            .native
            .retained
            .as_ref()
            .is_some_and(|browser| !browser.allows_readiness_retry())
        {
            return Self::observe_once(state, worker, browser).await;
        }
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
        browser: &WorkBrowser<'_>,
    ) -> Result<SemanticObservation, AgentWorkFailure> {
        if state.native.retained.is_some() {
            let capability = state.observation_capability();
            return state.native.observe_retained(worker, capability).await;
        }
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
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(next).ok_or(AgentWorkFailure::Contract)?,
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        Self::capture_once(state, worker, browser, request, frame).await
    }

    async fn capture_once(
        state: &mut WorkState,
        worker: &mut AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
        request: SemanticObservationRequest,
        frame: SemanticFrameJoin,
    ) -> Result<SemanticObservation, AgentWorkFailure> {
        let id = state.native.identity.id();
        let context = state
            .native
            .contexts()?
            .join(id)
            .map_err(|_| AgentWorkFailure::Context)?;
        if context != request.context()
            || frame.context() != context
            || frame.origin() != &state.native.origin
        {
            return Err(AgentWorkFailure::Context);
        }
        let next = state.native.id()?;
        let generation = state
            .native
            .snapshot_generation
            .map_or(
                Some(SemanticSnapshotGeneration::INITIAL),
                SemanticSnapshotGeneration::next,
            )
            .ok_or(AgentWorkFailure::Contract)?;
        let runtime_budget = if state
            .navigation_discovery
            .as_ref()
            .is_some_and(AgentNavigationDiscovery::is_production)
        {
            SemanticRuntimeBudget::INITIAL_FILTERED.with_link_url_state()
        } else {
            SemanticRuntimeBudget::INITIAL_FILTERED
        };
        let invocation = encode_semantic_runtime_invocation(
            &request,
            frame,
            SemanticInvocationId::new(next).ok_or(AgentWorkFailure::Contract)?,
            generation,
            runtime_budget,
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
                // This controller admits only its exact main frame. Keep each
                // encountered child boundary visible with an explicit refusal;
                // do not infer a child origin, capture it, or lose the usable
                // main document merely because it embeds an iframe.
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
        browser: &WorkBrowser<'_>,
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

    async fn observe_initial_ready(
        state: &mut WorkState,
        worker: &mut AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
    ) -> Result<SemanticObservation, AgentWorkFailure> {
        // Seven samples at most; backoff schedules work, it is never evidence
        // of readiness. The original run deadline/control lane remains live.
        const DELAYS_MS: [u64; 6] = [250, 500, 1000, 2000, 2000, 2000];
        let mut observation = Self::observe(state, worker, browser).await?;
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(10))
            .ok_or(AgentWorkFailure::Deadline)?
            .min(state.native.deadline);
        let mut delays = DELAYS_MS.into_iter();
        loop {
            state.check_task_contract()?;
            let readiness = state.task.initial_readiness(&observation)?;
            state.check_task_contract()?;
            state.native.check_control(worker, browser)?;
            if readiness == AgentWorkInitialReadiness::Ready
                && !has_dominant_loading_placeholders(&observation)
            {
                return Ok(observation);
            }
            state.refresh_account(worker, browser)?;
            if state
                .native
                .retained
                .as_ref()
                .is_some_and(|b| !b.allows_readiness_retry())
            {
                return finish_initial_readiness_wait(observation, readiness);
            }
            let Some(delay) = delays.next() else {
                return finish_initial_readiness_wait(observation, readiness);
            };
            let wake = Instant::now()
                .checked_add(Duration::from_millis(delay))
                .ok_or(AgentWorkFailure::Deadline)?;
            if wake >= deadline {
                return finish_initial_readiness_wait(observation, readiness);
            }
            tokio::select! {
                biased;
                event = state.native.next_event(worker, browser) => {
                    state.native.retain(event?)?;
                    return Err(AgentWorkFailure::Mailbox);
                }
                () = tokio::time::sleep_until(tokio::time::Instant::from_std(wake)) => {}
            }
            state.refresh_account(worker, browser)?;
            let next = Self::observe(state, worker, browser).await?;
            validate_initial_readiness_successor(&observation, &next)?;
            if Instant::now() >= deadline {
                state.check_task_contract()?;
                let readiness = state.task.initial_readiness(&next)?;
                state.check_task_contract()?;
                state.native.check_control(worker, browser)?;
                return finish_initial_readiness_wait(next, readiness);
            }
            observation = next;
        }
    }

    async fn browser_loop(
        &mut self,
        worker: &mut AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
    ) -> Result<(), AgentWorkFailure> {
        let state = self.state.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let mut observation = Self::observe_initial_ready(state, worker, browser).await?;
        let mut captured_at = SemanticCaptureInstant::from_millis(
            state
                .journal_mut()?
                .clock
                .now()
                .map_err(|_| AgentWorkFailure::Contract)?
                .millis(),
        );
        let mut progress = state.task_progress(&observation)?;
        if progress == AgentWorkTaskProgress::Complete {
            state.observation = Some(observation);
            return Ok(());
        }
        state.refresh_account(worker, browser)?;
        let action_authority = state
            .session
            .as_ref()
            .ok_or(AgentWorkFailure::Contract)?
            .config
            .permits_tool(AgentBrowserToolKind::Act)
            .then(|| state.action_authority(&observation))
            .transpose()?;
        let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let mut turn: AgentBrowserProviderTurn = Self::provider(
            &mut state.native,
            worker,
            browser,
            session.cancellation.clone(),
            session.start_initial_with_action_authority(&observation, action_authority.as_ref()),
        )
        .await?;
        if turn.turn.proposal().kind() == AgentBrowserToolKind::ShowForHuman {
            Self::accept_model_human_request(state, turn, &observation)?;
            state.observation = Some(observation);
            return Ok(());
        }
        if state.extraction_schema.is_some()
            && !state.has_navigation()
            && !state.actions_before_extraction
            && !state.subtree_extraction
            && !state.baseline_read
        {
            Self::extract_current(state, worker, browser, turn, &observation, captured_at).await?;
            state.observation = Some(observation);
            return Ok(());
        }
        let mut frames = observation
            .frames()
            .iter()
            .map(|snapshot| snapshot.frame().clone())
            .collect::<Vec<_>>();
        // One corrective model decision is allowed for each exact rejected
        // operation/target/baseline. Retaining only content-free keys prevents
        // secret fill values from entering controller state while ensuring a
        // weak model cannot consume the rest of a run repeating an impossible
        // proposal. Fresh observations carry distinct identities.
        let mut action_refusals = Vec::<AgentProviderActionRefusalKey>::new();
        loop {
            state.check_task_contract()?;
            // Deliver acknowledged audit batches while idle so long runs keep
            // their fixed pending-event bound and leave room for terminal debt.
            if state.journal_mut()?.audit.status().pending() >= 32 {
                Self::drain_audit(state, worker, browser, None).await?;
            }
            // The last decision was advertised as Extract-only. Independently
            // enforce that narrowing before any native capture, navigation or
            // local inspection can consume the reserved mapping call.
            let session = state.session.as_ref().ok_or(AgentWorkFailure::Contract)?;
            if state.requires_decision_budget()
                && (session.turns.saturating_add(1) >= session.max_model_calls
                    || session
                        .policy
                        .remaining_operations(session.lease.lease())
                        .map_err(|_| {
                            AgentWorkFailure::Browser(AgentBrowserProviderError::Authority)
                        })?
                        <= 1)
                && turn.turn.proposal().kind() != AgentBrowserToolKind::Extract
                && !(state.human_request
                    && turn.turn.proposal().kind() == AgentBrowserToolKind::ShowForHuman)
            {
                return Err(AgentWorkFailure::Browser(
                    AgentBrowserProviderError::TurnLimit,
                ));
            }
            if turn.turn.proposal().kind() == AgentBrowserToolKind::Snapshot {
                if session.turns.saturating_add(2) > session.max_model_calls
                    || session
                        .policy
                        .remaining_operations(session.lease.lease())
                        .map_err(|_| {
                            AgentWorkFailure::Browser(AgentBrowserProviderError::Authority)
                        })?
                        < 2
                {
                    return Err(AgentWorkFailure::Browser(
                        AgentBrowserProviderError::TurnLimit,
                    ));
                }
                let resolution = turn
                    .into_tool_turn()
                    .into_parts()
                    .1
                    .resolve_observation(&observation, &session.config)
                    .map_err(|_| {
                        AgentWorkFailure::Browser(AgentBrowserProviderError::Continuation)
                    })?;
                let checkpoint = match resolution {
                    AgentProviderObservationResolution::Capture(checkpoint) => {
                        let checkpoint = *checkpoint;
                        if let Some(schema) = &state.extraction_schema {
                            let session =
                                state.session.as_ref().ok_or(AgentWorkFailure::Contract)?;
                            session.check_live().map_err(AgentWorkFailure::Browser)?;
                            let read = read_selected_semantic_observation(
                                &observation,
                                SemanticReadAuthority::Acknowledged(checkpoint.baseline()),
                                captured_at,
                                SemanticReadSensitivityLimit::PublicOnly,
                                SemanticReadBudget::STANDARD,
                                schema.source_roles(),
                            )
                            .map_err(|error| {
                                AgentWorkFailure::Browser(AgentBrowserProviderError::Read(error))
                            })?;
                            state
                                .retained_read_evidence
                                .retain(&read, checkpoint.baseline())
                                .map_err(|error| {
                                    AgentWorkFailure::Browser(AgentBrowserProviderError::Read(
                                        error,
                                    ))
                                })?;
                        }
                        checkpoint
                    }
                    AgentProviderObservationResolution::Refused(refusal) => {
                        state.native.check_control(worker, browser)?;
                        state.refresh_account(worker, browser)?;
                        state.journal_mut()?.emit(AgentWorkEventKind::ToolProposed(
                            AgentBrowserToolKind::Snapshot,
                        ))?;
                        state
                            .journal_mut()?
                            .emit(AgentWorkEventKind::InspectionRefused)?;
                        let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
                        turn = Self::provider(
                            &mut state.native,
                            worker,
                            browser,
                            session.cancellation.clone(),
                            session.continue_after_scope_refusal(*refusal, &observation),
                        )
                        .await?;
                        continue;
                    }
                };
                let next =
                    Self::inspect_current(state, worker, browser, checkpoint, &observation).await?;
                observation = next.0;
                captured_at = next.1;
                progress = next.2;
                turn = next.3;
                frames = observation
                    .frames()
                    .iter()
                    .map(|snapshot| snapshot.frame().clone())
                    .collect();
                continue;
            }
            if matches!(
                turn.turn.proposal().kind(),
                AgentBrowserToolKind::Locate | AgentBrowserToolKind::Read
            ) {
                let session = state.session.as_ref().ok_or(AgentWorkFailure::Contract)?;
                if session.turns >= session.max_model_calls {
                    return Err(AgentWorkFailure::Browser(
                        AgentBrowserProviderError::TurnLimit,
                    ));
                }
                state.refresh_account(worker, browser)?;
                let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
                turn = Self::provider(
                    &mut state.native,
                    worker,
                    browser,
                    session.cancellation.clone(),
                    session.continue_inspection(
                        turn.into_tool_turn(),
                        &observation,
                        &frames,
                        Some(captured_at),
                    ),
                )
                .await?;
                continue;
            }
            if turn.turn.proposal().kind() == AgentBrowserToolKind::Screenshot {
                turn = Self::screenshot_current(state, worker, browser, turn, &observation).await?;
                continue;
            }
            if turn.turn.proposal().kind() == AgentBrowserToolKind::ShowForHuman {
                Self::accept_model_human_request(state, turn, &observation)?;
                state.observation = Some(observation);
                return Ok(());
            }
            if turn.turn.proposal().kind() == AgentBrowserToolKind::Wait {
                let next = Self::wait_current(state, worker, browser, turn, observation).await?;
                observation = next.0;
                captured_at = next.1;
                turn = next.2;
                frames.clear();
                frames.extend(
                    observation
                        .frames()
                        .iter()
                        .map(|snapshot| snapshot.frame().clone()),
                );
                continue;
            }
            let step = turn;
            if matches!(
                step.turn.proposal().kind(),
                AgentBrowserToolKind::Navigate | AgentBrowserToolKind::Back
            ) {
                if state.navigation_discovery.is_none() {
                    state.retained_read_evidence.clear();
                }
                let next = Self::navigate_current(
                    state,
                    worker,
                    browser,
                    step,
                    &observation,
                    captured_at,
                    progress,
                )
                .await?;
                observation = next.0;
                captured_at = next.1;
                progress = next.2;
                turn = next.3;
                frames.clear();
                frames.extend(
                    observation
                        .frames()
                        .iter()
                        .map(|snapshot| snapshot.frame().clone()),
                );
                continue;
            }
            let navigation_incomplete = state.has_navigation() && !state.navigation_complete();
            let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
            let proposal = match step.turn.proposal().kind() {
                AgentBrowserToolKind::Extract => {
                    if !state.task.terminal_extraction_ready() {
                        return Err(AgentWorkFailure::TaskPhase {
                            expected: progress,
                            proposed: AgentBrowserToolKind::Extract,
                        });
                    }
                    if state.has_navigation()
                        && state.navigation_discovery.is_none()
                        && (navigation_incomplete
                            || progress != AgentWorkTaskProgress::ReadyForExtraction)
                    {
                        return Err(AgentWorkFailure::TaskPhase {
                            expected: progress,
                            proposed: AgentBrowserToolKind::Extract,
                        });
                    }
                    if state.actions_before_extraction
                        && state.navigation_discovery.is_none()
                        && progress != AgentWorkTaskProgress::ReadyForExtraction
                    {
                        return Err(AgentWorkFailure::TaskPhase {
                            expected: progress,
                            proposed: AgentBrowserToolKind::Extract,
                        });
                    }
                    Self::extract_current(state, worker, browser, step, &observation, captured_at)
                        .await?;
                    state.observation = Some(observation);
                    return Ok(());
                }
                AgentBrowserToolKind::Act => {
                    // Reserve the effect, the next decision and terminal mapping
                    // before starting a mutation in an open objective.
                    if state.navigation_discovery.is_some()
                        && session
                            .policy
                            .remaining_operations(session.lease.lease())
                            .map_err(|_| {
                                AgentWorkFailure::Browser(AgentBrowserProviderError::Authority)
                            })?
                            < 3
                    {
                        return Err(AgentWorkFailure::Browser(
                            AgentBrowserProviderError::TurnLimit,
                        ));
                    }
                    if state.extraction_schema.is_some() && !state.actions_before_extraction {
                        return Err(AgentWorkFailure::Contract);
                    }
                    if progress == AgentWorkTaskProgress::ReadyForExtraction {
                        return Err(AgentWorkFailure::TaskPhase {
                            expected: progress,
                            proposed: AgentBrowserToolKind::Act,
                        });
                    }
                    match session
                        .bind_action_turn(step, &observation, &frames)
                        .map_err(AgentWorkFailure::Browser)?
                    {
                        crate::action::AgentBrowserActionBinding::Prepared(proposal) => proposal,
                        crate::action::AgentBrowserActionBinding::Refused(refusal) => {
                            state.native.check_control(worker, browser)?;
                            state.refresh_account(worker, browser)?;
                            state.journal_mut()?.emit(
                                AgentWorkEventKind::ActionProposalRefused(refusal.reason()),
                            )?;
                            if let Some(key) = refusal.key() {
                                if action_refusals.contains(&key) {
                                    return Err(AgentWorkFailure::Browser(
                                        AgentBrowserProviderError::ActionProposalLoop,
                                    ));
                                }
                                action_refusals
                                    .try_reserve(1)
                                    .map_err(|_| AgentWorkFailure::Contract)?;
                                action_refusals.push(key);
                            }
                            let session =
                                state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
                            turn = Self::provider(
                                &mut state.native,
                                worker,
                                browser,
                                session.cancellation.clone(),
                                session.continue_after_action_refusal(refusal, &observation),
                            )
                            .await?;
                            continue;
                        }
                    }
                }
                kind => {
                    return Err(AgentWorkFailure::Browser(
                        AgentBrowserProviderError::UnsupportedTool(kind),
                    ))
                }
            };
            let assessment = state
                .task
                .assess_observed(proposal.action(), &observation)?;
            state.refresh_account(worker, browser)?;
            let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
            let id = state.native.identity.id();
            let now = session.policy_now().map_err(AgentWorkFailure::Browser)?;
            let automation = if let Some(retained) = &state.native.retained {
                retained.automation_state(now)?
            } else {
                state
                    .native
                    .contexts()?
                    .automation_state(id)
                    .map_err(|_| AgentWorkFailure::Context)?
            };
            let request = session
                .authorize_action(
                    proposal,
                    &assessment,
                    automation,
                    SemanticActionExecutionInstant::from_millis(now.millis()),
                )
                .map_err(AgentWorkFailure::Browser)?;
            let action_deadline = request.deadline();
            let dispatch = if let Some(retained) = &mut state.native.retained {
                retained.dispatch_action(request, now)
            } else {
                browser.execute_semantic_action(request, worker.semantic_action_completion())
            };
            // Native ownership starts at dispatch, before fallible policy
            // accounting. Recovery must drain even if that accounting fails.
            state.native.action_pending = matches!(dispatch, ContextDispatch::Scheduled);
            session
                .account_action_dispatch(dispatch)
                .map_err(AgentWorkFailure::Browser)?;
            let terminal = match state
                .native
                .next_action_event(worker, browser, action_deadline)
                .await?
            {
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
            // Native evidence cannot restore a revoked document or authorize
            // postcondition reads. Settle its original owner first, then fail
            // closed before any continuation, success, or retry.
            state.native.check_control(worker, browser)?;
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
            captured_at = SemanticCaptureInstant::from_millis(now.millis());
            state.task.accept_verified_action(
                transition
                    .batch_result()
                    .ok_or(AgentWorkFailure::Contract)?,
                &observation,
            )?;
            progress = state.task_progress(&observation)?;
            if progress == AgentWorkTaskProgress::Complete {
                state.observation = Some(observation);
                return Ok(());
            }
            let action_authority = state.action_authority(&observation)?;
            // Reads/locates retain this exact frame cohort. Reuse its bounded
            // storage; only an independently captured observation replaces it.
            frames.clear();
            frames.extend(
                observation
                    .frames()
                    .iter()
                    .map(|snapshot| snapshot.frame().clone()),
            );
            let session = state.session.as_ref().ok_or(AgentWorkFailure::Contract)?;
            if session.turns >= session.max_model_calls {
                return Err(AgentWorkFailure::Browser(
                    AgentBrowserProviderError::TurnLimit,
                ));
            }
            state.refresh_account(worker, browser)?;
            let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
            turn = Self::provider(
                &mut state.native,
                worker,
                browser,
                session.cancellation.clone(),
                session.continue_after_verified_action_with_authority(
                    transition,
                    Some(&action_authority),
                ),
            )
            .await?;
        }
    }

    fn accept_model_human_request(
        state: &mut WorkState,
        turn: AgentBrowserProviderTurn,
        observation: &SemanticObservation,
    ) -> Result<(), AgentWorkFailure> {
        state.check_task_contract()?;
        if !state.human_request || state.model_human_request.is_some() {
            return Err(AgentWorkFailure::Browser(
                AgentBrowserProviderError::UnsupportedTool(AgentBrowserToolKind::ShowForHuman),
            ));
        }
        let (proposal, continuation) = turn.into_tool_turn().into_parts();
        let AgentBrowserToolProposal::ShowForHuman(reason) = proposal else {
            return Err(AgentWorkFailure::Contract);
        };
        if !continuation.baseline().authenticates(observation) {
            return Err(AgentWorkFailure::Browser(
                AgentBrowserProviderError::Authority,
            ));
        }
        let request = AgentWorkHumanRequest {
            context: observation.request().context(),
            observation: observation.request().id(),
            generation: observation.request().generation(),
            reason,
            retained_resource: state
                .native
                .retained
                .as_ref()
                .map(|browser| browser.binding().lease().resource().identity()),
        };
        state
            .journal_mut()?
            .emit(AgentWorkEventKind::ModelRequestedHuman(reason))?;
        state.model_human_request = Some(request);
        Ok(())
    }

    async fn screenshot_current(
        state: &mut WorkState,
        worker: &mut AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
        turn: AgentBrowserProviderTurn,
        observation: &SemanticObservation,
    ) -> Result<AgentBrowserProviderTurn, AgentWorkFailure> {
        state.check_task_contract()?;
        if !state.viewport_screenshot
            || state.native.screenshot_pending.is_some()
            || state
                .native
                .retained
                .as_ref()
                .is_some_and(|browser| !browser.supports_screenshots())
        {
            return Err(AgentWorkFailure::Browser(
                AgentBrowserProviderError::UnsupportedTool(AgentBrowserToolKind::Screenshot),
            ));
        }
        let baseline = turn.turn.continuation().baseline();
        let now = state
            .session
            .as_mut()
            .ok_or(AgentWorkFailure::Contract)?
            .policy_now()
            .map_err(AgentWorkFailure::Browser)?;
        let remaining = state
            .native
            .deadline
            .saturating_duration_since(Instant::now());
        let window_millis = u64::try_from(remaining.as_millis())
            .unwrap_or(u64::MAX)
            .min(MAX_SEMANTIC_SCREENSHOT_CAPTURE_MILLIS);
        if window_millis == 0 {
            return Err(AgentWorkFailure::Deadline);
        }
        let requested_at = SemanticCaptureInstant::from_millis(now.millis());
        let deadline = SemanticCaptureInstant::from_millis(
            now.millis()
                .checked_add(window_millis)
                .ok_or(AgentWorkFailure::Contract)?,
        );
        let request = prepare_semantic_screenshot(
            SemanticScreenshotRequestId::new(state.native.id()?)
                .ok_or(AgentWorkFailure::Contract)?,
            observation,
            baseline,
            requested_at,
            deadline,
            SemanticScreenshotBudget::STANDARD,
        )
        .map_err(|_| AgentWorkFailure::Browser(AgentBrowserProviderError::Authority))?;
        let (pending, native) = state
            .native
            .screenshots
            .begin(request)
            .map_err(|_| AgentWorkFailure::Contract)?;
        state.native.screenshot_pending = Some(pending);
        let dispatch = if let Some(retained) = &mut state.native.retained {
            retained.dispatch_screenshot(native, worker.semantic_screenshot_completion(), now)
        } else {
            browser.capture_semantic_screenshot(native, worker.semantic_screenshot_completion())
        };
        if dispatch != ContextDispatch::Scheduled {
            let pending = state
                .native
                .screenshot_pending
                .take()
                .ok_or(AgentWorkFailure::Contract)?;
            state
                .native
                .screenshots
                .cancel(pending)
                .map_err(|_| AgentWorkFailure::Contract)?;
            return Err(AgentWorkFailure::Context);
        }
        let capture = match state.native.next_event(worker, browser).await? {
            AgentRuntimeEvent::SemanticScreenshotTerminal(capture) => {
                if let Some(retained) = &mut state.native.retained {
                    retained.account_screenshot_terminal(now)?;
                }
                capture.map_err(AgentWorkFailure::Screenshot)?
            }
            event => {
                state.native.retain(event)?;
                return Err(AgentWorkFailure::Mailbox);
            }
        };
        let pending = state
            .native
            .screenshot_pending
            .take()
            .ok_or(AgentWorkFailure::Contract)?;
        let context = observation.request().context();
        let screenshot = state
            .native
            .screenshots
            .admit(pending, context, capture)
            .map_err(|_| AgentWorkFailure::ContextLost)?;
        state.native.check_control(worker, browser)?;
        state.refresh_account(worker, browser)?;
        let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
        Self::provider(
            &mut state.native,
            worker,
            browser,
            session.cancellation.clone(),
            session.continue_after_screenshot(turn.into_tool_turn(), screenshot, observation),
        )
        .await
    }

    async fn wait_current(
        state: &mut WorkState,
        worker: &mut AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
        turn: AgentBrowserProviderTurn,
        observation: SemanticObservation,
    ) -> Result<
        (
            SemanticObservation,
            SemanticCaptureInstant,
            AgentBrowserProviderTurn,
        ),
        AgentWorkFailure,
    > {
        state.check_task_contract()?;
        if !state.standalone_wait {
            return Err(AgentWorkFailure::Browser(
                AgentBrowserProviderError::UnsupportedTool(AgentBrowserToolKind::Wait),
            ));
        }
        let AgentBrowserToolProposal::Wait { condition, timeout } = turn.turn.proposal() else {
            return Err(AgentWorkFailure::Contract);
        };
        let wait_deadline = Instant::now()
            .checked_add(Duration::from_millis(u64::from(timeout.millis())))
            .map_or(state.native.deadline, |deadline| {
                deadline.min(state.native.deadline)
            });
        let mut wait = SemanticStandaloneWait::prepare(
            *condition,
            observation,
            turn.turn.continuation().baseline(),
        )
        .map_err(|_| AgentWorkFailure::Browser(AgentBrowserProviderError::Authority))?;
        let mut backoff = SemanticStandaloneWaitBackoff::new();
        let result = loop {
            let now = Instant::now();
            if now >= wait_deadline {
                break wait.time_out();
            }
            let wake = now
                .checked_add(Duration::from_millis(u64::from(
                    backoff.next_delay_millis(),
                )))
                .map_or(wait_deadline, |wake| wake.min(wait_deadline));
            tokio::select! {
                biased;
                event = state.native.next_event(worker, browser) => {
                    state.native.retain(event?)?;
                    return Err(AgentWorkFailure::Mailbox);
                }
                () = tokio::time::sleep_until(tokio::time::Instant::from_std(wake)) => {}
            }
            if Instant::now() >= wait_deadline {
                break wait.time_out();
            }
            state.journal_mut()?.emit(AgentWorkEventKind::Observing)?;
            let current = match Self::observe_once(state, worker, browser).await {
                Err(AgentWorkFailure::Observation(SemanticRuntimePortFailure::NotReady)) => {
                    continue;
                }
                result => result?,
            };
            let step = wait
                .advance(current)
                .map_err(|_| AgentWorkFailure::Browser(AgentBrowserProviderError::Authority))?;
            if Instant::now() >= wait_deadline {
                break match step {
                    SemanticStandaloneWaitStep::Pending(pending) => pending.time_out(),
                    SemanticStandaloneWaitStep::Satisfied(result) => result.into_timed_out(),
                };
            }
            match step {
                SemanticStandaloneWaitStep::Pending(pending) => wait = pending,
                SemanticStandaloneWaitStep::Satisfied(result) => break result,
            }
        };
        state.native.check_control(worker, browser)?;
        state.refresh_account(worker, browser)?;
        let action_authority = state.action_authority(result.observation())?;
        let captured_at = {
            let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
            let now = session.policy_now().map_err(AgentWorkFailure::Browser)?;
            SemanticCaptureInstant::from_millis(now.millis())
        };
        let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let cancellation = session.cancellation.clone();
        let (next_turn, observation) = Self::provider(
            &mut state.native,
            worker,
            browser,
            cancellation,
            session.continue_after_standalone_wait(
                turn.into_tool_turn(),
                result,
                Some(&action_authority),
            ),
        )
        .await?;
        Ok((observation, captured_at, next_turn))
    }

    async fn extract_current(
        state: &mut WorkState,
        worker: &mut AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
        turn: AgentBrowserProviderTurn,
        observation: &SemanticObservation,
        captured_at: SemanticCaptureInstant,
    ) -> Result<(), AgentWorkFailure> {
        state.check_task_contract()?;
        let session = state.session.as_ref().ok_or(AgentWorkFailure::Contract)?;
        session.check_live().map_err(AgentWorkFailure::Browser)?;
        if session.turns >= session.max_model_calls {
            // Do not capture data when no mapping call can be admitted.
            return Err(AgentWorkFailure::Browser(
                AgentBrowserProviderError::TurnLimit,
            ));
        }
        let expanded = if matches!(
            turn.turn.proposal(),
            AgentBrowserToolProposal::Extract {
                scope: AgentBrowserScopeProposal::Subtree(_),
                ..
            }
        ) {
            if !state.subtree_extraction {
                return Err(AgentWorkFailure::Browser(
                    AgentBrowserProviderError::UnsupportedTool(AgentBrowserToolKind::Extract),
                ));
            }
            let frames = observation
                .frames()
                .iter()
                .map(|frame| frame.frame().clone())
                .collect::<Vec<_>>();
            let schema = state
                .extraction_schema
                .as_ref()
                .ok_or(AgentWorkFailure::Contract)?;
            let next = state.native.id()?;
            let request = turn
                .turn
                .continuation()
                .begin_extraction_subtree(
                    observation,
                    &frames,
                    SemanticObservationId::new(next).ok_or(AgentWorkFailure::Contract)?,
                    schema,
                    SemanticObservationBudget::INITIAL_FILTERED,
                )
                .map_err(|_| AgentWorkFailure::Browser(AgentBrowserProviderError::Continuation))?;
            let frame = request
                .scope()
                .anchor()
                .ok_or(AgentWorkFailure::Contract)?
                .frame()
                .clone();
            state.journal_mut()?.emit(AgentWorkEventKind::Observing)?;
            // Exactly one requested capture, not an observation/mutation retry.
            Some(Self::capture_once(state, worker, browser, request, frame).await?)
        } else {
            None
        };
        state.check_task_contract()?;
        let (source, captured_at) = if let Some(expanded) = &expanded {
            (
                expanded,
                SemanticCaptureInstant::from_millis(
                    state
                        .journal_mut()?
                        .clock
                        .now()
                        .map_err(|_| AgentWorkFailure::Contract)?
                        .millis(),
                ),
            )
        } else {
            (observation, captured_at)
        };
        let frames = source
            .frames()
            .iter()
            .map(|snapshot| snapshot.frame().clone())
            .collect::<Vec<_>>();
        state.refresh_account(worker, browser)?;
        let schema = state
            .extraction_schema
            .as_ref()
            .ok_or(AgentWorkFailure::Contract)?;
        let session = state.session.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let result = Self::provider(
            &mut state.native,
            worker,
            browser,
            session.cancellation.clone(),
            session.extract_from_with_evidence(
                turn,
                source,
                expanded.as_ref().map(|_| observation),
                &frames,
                captured_at,
                schema,
                if state.progressive_observation || state.navigation_discovery.is_some() {
                    Some(&state.retained_read_evidence)
                } else {
                    None
                },
            ),
        )
        .await?;
        if state.task.accept_extraction(&result)? != AgentWorkTaskProgress::Complete {
            return Err(AgentWorkFailure::Contract);
        }
        state.check_task_contract()?;
        state.extraction = Some(
            result
                .into_owned()
                .map_err(|_| AgentWorkFailure::Contract)?,
        );
        Ok(())
    }

    async fn close_context(
        state: &mut WorkState,
        worker: &mut AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
    ) -> Result<(), AgentWorkFailure> {
        state.retained_read_evidence.clear();
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

    fn begin_recovery_close(state: &mut WorkState, browser: &WorkBrowser<'_>) {
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
        browser: &WorkBrowser<'_>,
    ) -> Result<(), AgentWorkFailure> {
        let state = self.state.as_mut().ok_or(AgentWorkFailure::Contract)?;
        Self::close_context(state, worker, browser).await?;
        self.close_resources(worker, browser, None).await
    }

    async fn close_unsuccessful(
        &mut self,
        worker: &mut AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
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
            || state.native.screenshot_pending.is_some()
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
        browser: &WorkBrowser<'_>,
        cleanup: Option<Instant>,
    ) -> Result<(), AgentWorkFailure> {
        let state = self.state.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let terminal_intent = match cleanup {
            Some(_) => WorkTerminalIntent::ClosedUnsuccessfully(
                state.failure.ok_or(AgentWorkFailure::Contract)?,
            ),
            None => state.model_human_request.map_or(
                WorkTerminalIntent::Succeeded,
                WorkTerminalIntent::WaitingForHuman,
            ),
        };
        state.native.screenshots.seal_for_shutdown();
        let screenshots = std::mem::replace(
            &mut state.native.screenshots,
            SemanticScreenshotCoordinator::new(),
        );
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
        let session = state.session.take().ok_or(AgentWorkFailure::Contract)?;
        let unsuccessful = !matches!(terminal_intent, WorkTerminalIntent::Succeeded);
        let finished = if unsuccessful {
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
        if state.terminal_intent.replace(terminal_intent).is_some() {
            state.session = Some(*terminal.session);
            return Err(AgentWorkFailure::Contract);
        }
        // Once the provider session has closed around an accepted human
        // request, that clean handoff wins over a later stop notification.
        // Continue only the bounded terminal cleanup lane: it ignores control
        // messages but still requires exact native/audit receipts before the
        // terminal claim. A stop observed before this boundary still prevents
        // the handoff from being frozen.
        let cleanup = cleanup.or_else(|| {
            matches!(terminal_intent, WorkTerminalIntent::WaitingForHuman(_))
                .then_some(state.native.deadline)
        });
        state.model_human_request = None;
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
            action_proposal_failure,
            action_executions,
            action_settlements,
            ..
        } = *terminal.session;
        let provider = terminal.provider;
        // These exact original owners, never replacement empty coordinators,
        // enter the constructor-closed native shutdown cohort.
        state.drained = Some(WorkDrained {
            proposal_refusal: action_proposal_failure,
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
                screenshots,
            )),
            proof: None,
            delivery: None,
            scoped_refusal: None,
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
            .account_port_seal(audit, browser.seal_for_shutdown(audit)?)
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
        self.finish_accounting(worker, browser, cleanup).await
    }

    async fn finish_accounting(
        &mut self,
        worker: &mut AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
        cleanup: Option<Instant>,
    ) -> Result<(), AgentWorkFailure> {
        let state = self.state.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let drained = state.drained.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let journal = drained.journal.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let terminal_intent = state.terminal_intent.ok_or(AgentWorkFailure::Contract)?;
        let completion = match terminal_intent {
            WorkTerminalIntent::WaitingForHuman(_) => {
                AgentSupervisorCompletion::Failed(AgentSupervisorFailure::PolicyDenied)
            }
            WorkTerminalIntent::ClosedUnsuccessfully(failure) => {
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
                    AgentWorkFailure::Browser(AgentBrowserProviderError::Action(
                        crate::AgentBrowserActionError::NeedsHuman(_),
                    )) => AgentSupervisorFailure::PolicyDenied,
                    AgentWorkFailure::Browser(
                        AgentBrowserProviderError::Account(_)
                        | AgentBrowserProviderError::NoExtractionEvidence,
                    ) => AgentSupervisorFailure::PolicyDenied,
                    AgentWorkFailure::Browser(_) => AgentSupervisorFailure::ProviderFailed,
                    _ => AgentSupervisorFailure::PolicyDenied,
                })
            }
            WorkTerminalIntent::Succeeded => AgentSupervisorCompletion::Succeeded,
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
        self.publish_terminal(worker).await
    }

    async fn deliver_audit(
        &mut self,
        worker: &mut AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
        cleanup: Option<Instant>,
    ) -> Result<(), AgentWorkFailure> {
        let state = self.state.as_mut().ok_or(AgentWorkFailure::Contract)?;
        Self::drain_audit(state, worker, browser, cleanup).await?;
        state
            .journal_mut()?
            .audit
            .is_quiescent()
            .then_some(())
            .ok_or(AgentWorkFailure::Audit)
    }

    async fn drain_audit(
        state: &mut WorkState,
        worker: &mut AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
        cleanup: Option<Instant>,
    ) -> Result<(), AgentWorkFailure> {
        for _ in 0..MAX_PENDING_AGENT_AUDIT_EVENTS.div_ceil(MAX_AGENT_AUDIT_DELIVERY_EVENTS) {
            let journal = state.journal_mut()?;
            if journal.audit.status().pending() == 0 {
                return Ok(());
            }
            let batch = journal
                .audit
                .begin_next_delivery(MAX_AGENT_AUDIT_DELIVERY_EVENTS)
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
        (state.journal_mut()?.audit.status().pending() == 0)
            .then_some(())
            .ok_or(AgentWorkFailure::Audit)
    }

    async fn publish_terminal(
        &mut self,
        worker: &mut AgentRuntimeWorker,
    ) -> Result<(), AgentWorkFailure> {
        let state = self.state.as_mut().ok_or(AgentWorkFailure::Contract)?;
        if !state.native.deferred.is_empty()
            || state.native.operation.is_some()
            || state.native.recovery_close.is_some()
            || state.native.observation.is_some()
            || state.native.action_pending
            || state.native.screenshot_pending.is_some()
            || state.native.cancellation.is_some()
            || state.native.shutdown_audit.is_some()
            || state.native_terminal.is_some()
        {
            return Err(AgentWorkFailure::Mailbox);
        }
        let drained = state.drained.as_mut().ok_or(AgentWorkFailure::Contract)?;
        if (drained.proof.is_none() && drained.delivery.is_none()) || drained.provider.is_none() {
            return Err(AgentWorkFailure::Shutdown);
        }
        let journal = drained.journal.as_ref().ok_or(AgentWorkFailure::Contract)?;
        let terminal_intent = state.terminal_intent.ok_or(AgentWorkFailure::Contract)?;
        if self.retained_terminal.is_some()
            && matches!(terminal_intent, WorkTerminalIntent::Succeeded)
            && state.extraction.is_none()
        {
            return Err(AgentWorkFailure::Contract);
        }
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
        // A clean success is claimed only as the ordinary terminal it proved.
        // If control arrived after its intent was frozen but before the claim,
        // the runtime must refuse that claim and retain recovery ownership;
        // accepting it as a cancelled terminal would publish `Accepted` from a
        // cancelled run. Waiting/unsuccessful intents already closed through
        // the unsuccessful accounting lane and may truthfully settle under the
        // matching control class without being relabelled.
        let controlled = !matches!(terminal_intent, WorkTerminalIntent::Succeeded);
        let class = if controlled && worker.shutdown_deadline().is_some() {
            AgentRuntimeControllerTerminalClass::Shutdown
        } else if controlled && worker.stop_reason().is_some() {
            AgentRuntimeControllerTerminalClass::Cancelled
        } else {
            AgentRuntimeControllerTerminalClass::Ordinary
        };
        let closure_matches_intent = match terminal_intent {
            WorkTerminalIntent::Succeeded => {
                closure.outcome() == AgentRunProgressOutcome::Succeeded
            }
            WorkTerminalIntent::WaitingForHuman(_) => {
                closure.outcome()
                    == AgentRunProgressOutcome::Failed(AgentSupervisorFailure::PolicyDenied)
            }
            WorkTerminalIntent::ClosedUnsuccessfully(_) => {
                closure.outcome() != AgentRunProgressOutcome::Succeeded
            }
        };
        if !closure_matches_intent {
            return Err(AgentWorkFailure::Accounting);
        }
        let claim = match if self.retained_terminal.is_some() {
            worker
                .try_claim_scoped_terminal(class)
                .await
                .map(retained::TerminalClaim::Scoped)
        } else {
            worker
                .try_claim_controller_terminal(class)
                .await
                .map(retained::TerminalClaim::Legacy)
        } {
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
        let provider = drained.provider.take().ok_or(AgentWorkFailure::Shutdown)?;
        match claim {
            retained::TerminalClaim::Legacy(claim) => {
                let proof = drained.proof.take().ok_or(AgentWorkFailure::Shutdown)?;
                claim.commit_with_shutdown(proof, settlement, provider);
            }
            retained::TerminalClaim::Scoped(claim) => {
                let delivery = drained.delivery.take().ok_or(AgentWorkFailure::Shutdown)?;
                if let Err(refusal) = claim.commit(delivery, settlement, provider) {
                    drained.scoped_refusal = Some(refusal);
                    return Err(AgentWorkFailure::Shutdown);
                }
            }
        }
        let human_review = drained
            .proposal_refusal
            .take()
            .and_then(crate::action::AgentBrowserActionProposalRefusal::discard_after_closure)
            .filter(|_| {
                closure.outcome()
                    == AgentRunProgressOutcome::Failed(AgentSupervisorFailure::PolicyDenied)
            });
        // Completion is separate from the bounded progress lane: saturation
        // cannot discard an already-consumed clean terminal owner.
        let _ = lock(&journal.events).publish(AgentWorkEventKind::Terminal);
        if let Some(terminal) = &self.retained_terminal {
            *lock(terminal) = Some(match terminal_intent {
                WorkTerminalIntent::WaitingForHuman(request) => {
                    AgentWorkRetainedOutcome::WaitingForHuman(AgentWorkWaitingForHuman {
                        settlement,
                        request,
                    })
                }
                WorkTerminalIntent::ClosedUnsuccessfully(failure) => {
                    AgentWorkRetainedOutcome::ClosedUnsuccessfully(AgentWorkClosedUnsuccessfully {
                        settlement,
                        failure,
                        human_review,
                    })
                }
                WorkTerminalIntent::Succeeded => AgentWorkRetainedOutcome::Accepted {
                    settlement,
                    extraction: Box::new(
                        state.extraction.take().ok_or(AgentWorkFailure::Contract)?,
                    ),
                },
            });
            self.state.take();
            return Ok(());
        }
        *lock(&self.terminal) = Some(match terminal_intent {
            WorkTerminalIntent::WaitingForHuman(request) => {
                AgentWorkOutcome::WaitingForHuman(AgentWorkWaitingForHuman {
                    settlement,
                    request,
                })
            }
            WorkTerminalIntent::ClosedUnsuccessfully(failure) => {
                AgentWorkOutcome::ClosedUnsuccessfully(AgentWorkClosedUnsuccessfully {
                    settlement,
                    failure,
                    human_review,
                })
            }
            WorkTerminalIntent::Succeeded => AgentWorkOutcome::Succeeded(AgentWorkSuccess {
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
        browser: &WorkBrowser<'_>,
    ) -> Instant {
        let deadline = worker
            .shutdown_deadline()
            .unwrap_or_else(|| Instant::now() + Duration::from_secs(1));
        let Some(state) = self.state.as_mut() else {
            return deadline;
        };
        state.retained_read_evidence.clear();
        if Instant::now() < deadline {
            // Reconcile one already-known synchronous terminal, never reissue
            // native work. Time/audit refusal retains its exact original owner.
            if let Some(session) = state.session.as_mut() {
                if session.navigation_refusal.is_some() {
                    let _ = session.settle_navigation_refusal();
                }
            }
            Self::begin_recovery_close(state, browser);
            // A mailbox fault can have moved the exact audit terminal into the
            // bounded deferred lane before cleanup starts. Consume it there,
            // preserving foreign or otherwise unaccounted terminals in order.
            Self::reconcile_deferred_audit(state);
            Self::drain_retained_navigation(state, deadline).await;
            Self::drain_retained_action(state, deadline).await;
        }
        // Drain already-dispatched callbacks only; no action or provider retry.
        while state.native.operation.is_some()
            || state.native.recovery_close.is_some()
            || state.native.observation.is_some()
            || state.native.action_pending
            || state.native.screenshot_pending.is_some()
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
                AgentRuntimeEvent::SemanticScreenshotTerminal(_) => {}
                _ => {}
            }
            let accounted = match &event {
                AgentRuntimeEvent::NativeTerminal(ContextNativeEvent::NavigationSettled(value))
                    if state.session.as_ref().is_some_and(|session| {
                        session
                            .navigation
                            .as_ref()
                            .is_some_and(|active| active.operation() == value.operation())
                    }) =>
                {
                    // A stop revokes continuation, not the original dispatched
                    // operation's accounting and durable terminal obligation.
                    state
                        .session
                        .as_mut()
                        .is_some_and(|session| session.settle_navigation_terminal(value).is_ok())
                }
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
                AgentRuntimeEvent::SemanticScreenshotTerminal(_) => {
                    let pending = state.native.screenshot_pending.take();
                    let now = state
                        .native
                        .clock
                        .as_ref()
                        .and_then(|clock| clock.now().ok());
                    let retained = state.native.retained.as_mut();
                    let accounted = match (retained, now) {
                        (Some(retained), Some(now)) => {
                            retained.account_screenshot_terminal(now).is_ok()
                        }
                        (None, _) => true,
                        _ => false,
                    };
                    pending.is_some_and(|pending| {
                        state.native.screenshots.cancel(pending).is_ok() && accounted
                    })
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
                    Self::settle_recovery_audit(state, *settlement)
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

    fn reconcile_deferred_audit(state: &mut WorkState) {
        let mut index = 0;
        while index < state.native.deferred.len() {
            let accounted = match &state.native.deferred[index] {
                AgentRuntimeEvent::AuditTerminal(settlement) => {
                    let settlement = *settlement;
                    Self::settle_recovery_audit(state, settlement)
                }
                _ => false,
            };
            if accounted {
                state.native.deferred.remove(index);
            } else {
                index += 1;
            }
        }
    }

    fn settle_recovery_audit(
        state: &mut WorkState,
        settlement: AgentAuditDeliverySettlement,
    ) -> bool {
        state.journal_mut().is_ok_and(|journal| {
            journal.audit.current_delivery().is_ok_and(|delivery| {
                delivery.is_some_and(|delivery| delivery.proof() == settlement.proof())
            }) && journal.audit.settle_delivery(settlement).is_ok()
        })
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
        // Only actor handles are abandoned here. Original operation receivers,
        // native port, health and destruction remain with the application.
        state.native.retained.take();
        if let Ok(journal) = state.journal_mut() {
            let _ = journal.emit(AgentWorkEventKind::Recovery);
        }
        if let Some(terminal) = &self.retained_terminal {
            let mut slot = lock(terminal);
            if slot.is_none() {
                *slot = Some(AgentWorkRetainedOutcome::Recovery(
                    AgentWorkRetainedRecovery(AgentWorkRecovery {
                        failure,
                        state: Box::new(state),
                    }),
                ));
            }
            return;
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
    /// Snapshot scope was incompatible with the delivered baseline. No native
    /// capture ran; one budgeted provider turn can select a different operation.
    InspectionRefused,
    /// A model action failed binding before preparation, policy, or dispatch.
    /// Correcting the proposal consumes another ordinary budgeted model call.
    ActionProposalRefused(SemanticActionBindingError),
    /// An exact native scoped capture lost its anchor. A separate initial
    /// capture may restore current refs under the original run authority.
    InspectionAnchorLost,
    /// An independently authorized native effect is active.
    ActionActive,
    /// Native synchronously refused admission; the failed effect and batch
    /// were accounted. No native execution or retry is implied.
    ActionRejected(SemanticActionFailure),
    /// A native effect was independently verified and accounted.
    Verified,
    /// An explicit policy/human boundary stopped execution.
    NeedsHuman(AgentNeedsHumanReason),
    /// The model deliberately stopped and requested a person with a closed reason.
    ModelRequestedHuman(AgentBrowserHumanReason),
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
    /// Accounted scoped read reported AnchorMissing while its original retained
    /// document and lease remained live. Only progressive inspection may recover.
    InspectionAnchorLost,
    /// A settled model proposal crossed the trusted task's fresh phase gate.
    TaskPhase {
        /// Current product-side task state, never model completion text.
        expected: AgentWorkTaskProgress,
        /// Proposed bounded tool; no arguments or page content are retained.
        proposed: AgentBrowserToolKind,
    },
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
    /// Exact bounded native viewport-capture refusal.
    Screenshot(SemanticScreenshotNativeFailure),
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
            .record_human_refusal(
                self.execution.as_ref().ok_or(AgentWorkFailure::Contract)?,
                transition,
            )
            .map_err(|_| AgentWorkFailure::Accounting)?;
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

    pub(super) fn action_rejected(
        &mut self,
        batch: &SemanticActionBatchResult,
    ) -> Result<(), AgentWorkFailure> {
        let failure = batch.failure().ok_or(AgentWorkFailure::Accounting)?;
        let receipt = failure.receipt();
        let AgentEffectSettlement::Failed(reason) = receipt.settlement() else {
            return Err(AgentWorkFailure::Accounting);
        };
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
        self.emit(AgentWorkEventKind::ActionRejected(reason))
    }
}

impl fmt::Debug for WorkJournal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WorkJournal([owned, content-free])")
    }
}
