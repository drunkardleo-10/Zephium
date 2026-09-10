//! Retained observation, navigation and authorized-action backing for Work.

use super::*;
use std::task::Waker;

/// Narrow application-owned lease adapter. The implementation retains original
/// operation slots outside the worker; no native port or registry is exposed.
/// Registration is immutable, bounded and precedes every native dispatch.
pub trait AgentWorkRetainedBrowser: Send {
    /// Whether this exact retained backing can own a bounded native viewport
    /// capture callback for its leased document.
    fn supports_screenshots(&self) -> bool {
        false
    }
    /// Dispatches one already policy-bound native screenshot half. Returning
    /// `Scheduled` transfers exactly one completion obligation.
    fn dispatch_screenshot(
        &mut self,
        _request: SemanticScreenshotNativeRequest,
        _completion: SemanticScreenshotNativeCompletion,
        _now: AgentPolicyInstant,
    ) -> ContextDispatch {
        ContextDispatch::Unsupported
    }
    /// Accounts the exact screenshot callback after the runtime mailbox has
    /// transferred its terminal. This releases retained-resource exclusion;
    /// it does not validate or disclose image bytes.
    fn account_screenshot_terminal(
        &mut self,
        _now: AgentPolicyInstant,
    ) -> Result<(), AgentWorkFailure> {
        Err(AgentWorkFailure::Contract)
    }
    /// Whether the exact backing owns authorized semantic action callbacks.
    /// This is ownership support only; the task and policy still approve
    /// effects, and the native platform may reject an unsupported recipe.
    fn supports_actions(&self) -> bool {
        false
    }
    /// Dispatches one existing policy-authorized recipe under the current lease.
    /// Synchronous refusal transfers no native callback obligation.
    fn dispatch_action(
        &mut self,
        _request: SemanticActionNativeRequest,
        _now: AgentPolicyInstant,
    ) -> ContextDispatch {
        ContextDispatch::Unsupported
    }
    /// Accounts the resource callback and returns the original native terminal
    /// to the independent policy owner, including after lease revocation.
    fn poll_action(
        &mut self,
        _now: AgentPolicyInstant,
    ) -> Result<Option<SemanticActionNativeSettlement>, AgentWorkFailure> {
        Err(AgentWorkFailure::Contract)
    }
    /// Original-row description only; this is not read admission.
    fn binding(&self) -> &WorkBrowserReadBinding;
    /// Installs the one original worker listener, without replacing the Work sink.
    fn register_listener(&mut self, waker: Waker) -> Result<(), AgentWorkFailure>;
    /// Rearms notifications and checks exact lease, deadline and sticky health.
    fn check_health(&self, now: AgentPolicyInstant) -> Result<(), AgentWorkFailure>;
    /// Whether this exact backing supports policy-bound document transitions.
    fn supports_navigation(&self) -> bool {
        false
    }
    /// Current acknowledged observation authority, not a navigation permit.
    fn automation_state(
        &self,
        _now: AgentPolicyInstant,
    ) -> Result<ContextAutomationState, AgentWorkFailure> {
        Err(AgentWorkFailure::Contract)
    }
    /// Retires old read authority and reserves one original successor owner.
    fn prepare_navigation(
        &mut self,
        _source: ContextJoin,
        _now: AgentPolicyInstant,
    ) -> Result<ContextOperationJoin, AgentWorkFailure> {
        Err(AgentWorkFailure::Contract)
    }
    /// Explicitly accounts an original preparation that never reached dispatch.
    fn cancel_navigation_preparation(&mut self) -> Result<(), AgentWorkFailure> {
        Err(AgentWorkFailure::Contract)
    }
    /// Binds only the original policy's active operation; no arbitrary URL port.
    fn dispatch_navigation(&mut self, _active: &AgentActiveNavigation) -> ContextDispatch {
        ContextDispatch::Unsupported
    }
    /// Returns the original core-accounted terminal, including after revocation.
    /// The caller must settle its independent policy/audit owner before closure.
    fn poll_navigation(
        &mut self,
        _now: AgentPolicyInstant,
    ) -> Result<Option<ContextNavigationSettlement>, AgentWorkFailure> {
        Err(AgentWorkFailure::Contract)
    }
    /// Whether a pre-dispatch NotReady may consume another initial read.
    /// One-shot capture adapters refuse retries without replacing that receipt.
    fn allows_readiness_retry(&self) -> bool {
        true
    }
    /// Reserves and dispatches one exact bounded initial read.
    fn begin_observation(&mut self, now: AgentPolicyInstant) -> Result<(), AgentWorkFailure>;
    /// Whether the original observation owner supports acknowledged expansions.
    fn supports_expansion(&self) -> bool {
        false
    }
    /// One exact same-document capture; no ordinary read or native effect.
    fn begin_expansion(
        &mut self,
        _previous: &SemanticObservation,
        _acknowledgement: &SemanticObservationAcknowledgement,
        _target: SemanticReferenceId,
        _kind: SemanticExpansionKind,
        _now: AgentPolicyInstant,
    ) -> Result<(), AgentWorkFailure> {
        Err(AgentWorkFailure::Contract)
    }
    /// Accounts the original terminal and returns its original observation.
    /// `InspectionAnchorLost` is reserved for an accounted, anchored expansion
    /// whose exact native result is AnchorMissing and whose original lease and
    /// document binding remain live. Initial reads, dispatch refusals and stale
    /// callbacks must not use that recoverable classification.
    fn poll_observation(
        &mut self,
        now: AgentPolicyInstant,
    ) -> Result<Option<SemanticObservation>, AgentWorkFailure>;
    /// Seals this lease before dispatch; never stops loading or destroys the page.
    fn begin_revocation(&mut self) -> Result<(), AgentWorkFailure>;
    /// Drains original read/revocation/delivery owners after the caller consumed
    /// any original navigation/action terminal. Wake is not a receipt.
    fn poll_revocation(
        &mut self,
        now: AgentPolicyInstant,
    ) -> Result<Option<WorkBrowserLeaseDeliveryProof>, AgentWorkFailure>;
}

/// Same controller algorithm on the scoped runtime, without native ownership.
pub struct AgentWorkRetainedController(pub(super) AgentWorkController);

/// Non-durable result of a scoped controller. It grants neither successor
/// admission nor global browser closure; actual worker drain is still separate.
#[must_use]
pub enum AgentWorkRetainedOutcome {
    /// Trusted task acceptance and exact accounting, not factual verification.
    Accepted {
        /// Original run accounting, not a Store acknowledgement.
        settlement: AgentRunPolicySettlement,
        /// Bounded, source-bound but explicitly model-mapped data.
        extraction: Box<SemanticOwnedExtractionResult>,
    },
    /// An unsuccessful run whose original scoped operands were consumed.
    ClosedUnsuccessfully(AgentWorkClosedUnsuccessfully),
    /// Run-scoped uncertainty; the original application retains page cleanup.
    Recovery(AgentWorkRetainedRecovery),
}

/// Opaque scoped recovery, with no durable-record or native-shutdown adapter.
#[must_use]
pub struct AgentWorkRetainedRecovery(pub(super) AgentWorkRecovery);
impl AgentWorkRetainedRecovery {
    /// Prepares one explicit read of a separately constructed, freshly admitted
    /// resource in the same run/profile. The trusted application must authorize
    /// that exact document/read/account independently. This neither reuses the
    /// quarantined page nor resumes this sealed controller or its model context.
    pub fn prepare_effect_reinspection(
        &mut self,
        resources: &mut WorkBrowserResources,
        lease: &WorkBrowserExecutionLease,
        target: crate::AgentWorkEffectReadTarget,
        now: AgentPolicyInstant,
    ) -> Result<
        (
            crate::AgentWorkEffectReinspection,
            WorkBrowserObservationRequest,
        ),
        crate::AgentWorkEffectReinspectionError,
    > {
        use crate::AgentWorkEffectReinspectionError as Error;
        if self.0.state.native.action_pending || self.0.state.native_terminal.is_some() {
            return Err(Error::Unavailable);
        }
        let source = self
            .0
            .state
            .native
            .retained
            .as_ref()
            .ok_or(Error::Unavailable)?
            .binding()
            .lease()
            .resource();
        // Stable Work IDs are not registry authority. The original private
        // resource allocation must still belong to this exact registry.
        resources.phase(source).map_err(Error::Resource)?;
        let source = source.identity();
        if lease.resource().identity().work() != source.work()
            || lease.resource().identity().resource() == source.resource()
        {
            return Err(Error::Binding);
        }
        let session = self.0.state.session.as_mut().ok_or(Error::Unavailable)?;
        let account = session.account;
        session
            .action
            .as_mut()
            .ok_or(Error::Unavailable)?
            .prepare_reinspection(account, resources, lease, target, now)
    }

    /// Attaches one exact current-state record while retaining all original
    /// failed effect, policy, audit and resource owners. Foreign records return
    /// unchanged. No successful effect receipt or continuation is minted.
    pub fn record_effect_reobservation(
        &mut self,
        result: crate::AgentWorkEffectReobservation,
    ) -> Result<(), Box<crate::AgentWorkEffectReobservation>> {
        let Some(action) = self
            .0
            .state
            .session
            .as_mut()
            .and_then(|session| session.action.as_mut())
        else {
            return Err(Box::new(result));
        };
        action.record_reinspection(result)
    }

    /// Original independently observed state record, still requiring explicit
    /// reconciliation and fresh authorization before any further execution.
    pub fn effect_reobservation(&self) -> Option<&crate::AgentWorkEffectReobservation> {
        self.0
            .state
            .session
            .as_ref()?
            .action
            .as_ref()?
            .reinspection_result()
    }

    /// Content-free cause; original unresolved run owners remain retained.
    pub const fn failure(&self) -> AgentWorkFailure {
        self.0.failure
    }
    /// Content-free original audit debt; not reconciliation or closure authority.
    pub fn audit_status(&mut self) -> Result<AgentAuditLedgerStatus, AgentWorkFailure> {
        self.0.audit_reconciliation_status()
    }
}

/// Content-free progress plus a move-only non-durable scoped outcome.
pub struct AgentWorkRetainedHandle {
    pub(super) events: Arc<Mutex<WorkEvents>>,
    pub(super) terminal: Arc<Mutex<Option<AgentWorkRetainedOutcome>>>,
}
impl AgentWorkRetainedHandle {
    /// Registers the application's bounded progress wake, without native authority.
    pub fn set_waker(&self, waker: Waker) {
        let mut events = lock(&self.events);
        events.waker = Some(waker);
        if !events.queue.is_empty() {
            events.wake();
        }
    }
    /// Queue drain status only; not scoped closure or successor permission.
    pub fn has_pending_events(&self) -> bool {
        !lock(&self.events).queue.is_empty()
    }
    /// Removes one bounded content-free event.
    pub fn take_event(&self) -> Option<AgentWorkEvent> {
        lock(&self.events).queue.pop_front()
    }
    /// Moves the original result once; not product publication/admission.
    pub fn take_outcome(&mut self) -> Option<AgentWorkRetainedOutcome> {
        lock(&self.terminal).take()
    }
}

/// No dummy port implements the retained branch. Legacy effects are absent.
pub(super) enum WorkBrowser<'a> {
    Legacy(&'a AgentRuntimeBrowser),
    Retained,
}
impl WorkBrowser<'_> {
    pub(super) fn dispatch(&self, request: ContextNativeRequest) -> ContextDispatch {
        match self {
            Self::Legacy(browser) => browser.dispatch(request),
            Self::Retained => ContextDispatch::Unsupported,
        }
    }
    pub(super) fn invoke_semantic(&self, invocation: SemanticRuntimeInvocation) -> ContextDispatch {
        match self {
            Self::Legacy(browser) => browser.invoke_semantic(invocation),
            Self::Retained => ContextDispatch::Unsupported,
        }
    }
    pub(super) fn execute_semantic_action(
        &self,
        request: SemanticActionNativeRequest,
        completion: SemanticActionNativeCompletion,
    ) -> ContextDispatch {
        match self {
            Self::Legacy(browser) => browser.execute_semantic_action(request, completion),
            Self::Retained => ContextDispatch::Unsupported,
        }
    }
    pub(super) fn capture_semantic_screenshot(
        &self,
        request: SemanticScreenshotNativeRequest,
        completion: SemanticScreenshotNativeCompletion,
    ) -> ContextDispatch {
        match self {
            Self::Legacy(browser) => browser.capture_semantic_screenshot(request, completion),
            Self::Retained => ContextDispatch::Unsupported,
        }
    }
    pub(super) fn seal_for_shutdown(
        &self,
        audit: ContextResourceAuditId,
    ) -> Result<ContextShutdownDispatch, AgentWorkFailure> {
        match self {
            Self::Legacy(browser) => Ok(browser.seal_for_shutdown(audit)),
            Self::Retained => Err(AgentWorkFailure::Contract),
        }
    }
}

pub(super) enum TerminalClaim {
    Legacy(zephium_agent_runtime::AgentRuntimeControllerTerminalClaim),
    Scoped(zephium_agent_runtime::AgentRuntimeScopedClaim),
}

impl AgentWorkRetainedController {
    /// Original dormant hard deadline, including the retained lease intersection.
    pub fn deadline(&self) -> Result<Instant, AgentWorkFailure> {
        self.0.deadline()
    }
    /// Dormant admission intent from the original approved manifest. A trusted
    /// application must receive exact Admitted and Running acknowledgements
    /// before releasing this controller's scoped runtime startup gate.
    pub fn journal_admission(
        &self,
        owner: AgentWorkIncarnation,
    ) -> Result<AgentWorkJournalMutation, AgentWorkFailure> {
        self.0.journal_admission(owner)
    }
    /// Uses a dedicated original provider transport and the same session loop.
    /// This is primitive composition, not product or durable admission.
    pub fn try_new(
        input: AgentWorkRunInput,
        browser: Box<dyn AgentWorkRetainedBrowser>,
        transport: AgentProviderTransportConfig,
        credential: AgentProviderCredential,
        audit: Arc<dyn AgentAuditPort>,
        task: Box<dyn AgentWorkTask>,
    ) -> Result<
        (
            Self,
            AgentWorkRetainedHandle,
            zephium_agent_runtime::AgentRuntimeScopedBinding,
        ),
        AgentWorkFailure,
    > {
        let transport = AgentProviderTransport::try_new(transport)
            .map_err(|_| AgentWorkFailure::Browser(AgentBrowserProviderError::Transport))?;
        Self::with_transport(
            input,
            browser,
            transport,
            credential,
            audit,
            task,
            AgentBrowserRetention::Stateless,
        )
    }

    /// Release-excluded loopback adapter, with the same exact original transport.
    #[cfg(feature = "probe-harness")]
    pub fn try_new_for_probe(
        input: AgentWorkRunInput,
        browser: Box<dyn AgentWorkRetainedBrowser>,
        transport: AgentProviderTransport,
        credential: AgentProviderCredential,
        audit: Arc<dyn AgentAuditPort>,
        task: Box<dyn AgentWorkTask>,
    ) -> Result<
        (
            Self,
            AgentWorkRetainedHandle,
            zephium_agent_runtime::AgentRuntimeScopedBinding,
        ),
        AgentWorkFailure,
    > {
        Self::with_transport(
            input,
            browser,
            transport,
            credential,
            audit,
            task,
            AgentBrowserRetention::Stateless,
        )
    }

    /// Explicit public-data diagnostic retention; absent from production builds.
    #[cfg(feature = "probe-harness")]
    pub fn try_new_for_public_probe(
        input: AgentWorkRunInput,
        browser: Box<dyn AgentWorkRetainedBrowser>,
        transport: AgentProviderTransportConfig,
        credential: AgentProviderCredential,
        audit: Arc<dyn AgentAuditPort>,
        task: Box<dyn AgentWorkTask>,
    ) -> Result<
        (
            Self,
            AgentWorkRetainedHandle,
            zephium_agent_runtime::AgentRuntimeScopedBinding,
        ),
        AgentWorkFailure,
    > {
        let transport = AgentProviderTransport::try_new(transport)
            .map_err(|_| AgentWorkFailure::Browser(AgentBrowserProviderError::Transport))?;
        Self::with_transport(
            input,
            browser,
            transport,
            credential,
            audit,
            task,
            AgentBrowserRetention::InspectablePublicData,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn with_transport(
        mut input: AgentWorkRunInput,
        browser: Box<dyn AgentWorkRetainedBrowser>,
        transport: AgentProviderTransport,
        credential: AgentProviderCredential,
        audit: Arc<dyn AgentAuditPort>,
        task: Box<dyn AgentWorkTask>,
        retention: AgentBrowserRetention,
    ) -> Result<
        (
            Self,
            AgentWorkRetainedHandle,
            zephium_agent_runtime::AgentRuntimeScopedBinding,
        ),
        AgentWorkFailure,
    > {
        let sampled_at = Instant::now();
        let now = input
            .settings
            .clock
            .now()
            .map_err(|_| AgentWorkFailure::Contract)?;
        browser.check_health(now)?;
        let binding = browser.binding();
        if binding.frame().context().identity() != input.context.identity
            || binding.frame().origin() != &input.context.origin
            || binding.requested_document() != &input.context.target
            || binding.document_policy() != input.context.document_policy
            || !input
                .context
                .document_policy
                .admits_final_document(binding.requested_document(), binding.document())
            || binding.storage() != input.context.storage
            || binding.lease().deadline()
                > input
                    .manifest
                    .plan_node(input.lease.node())
                    .ok_or(AgentWorkFailure::Contract)?
                    .expires_at()
            || binding.lease().deadline() <= now
        {
            return Err(AgentWorkFailure::Contract);
        }
        input.settings.deadline = input.settings.deadline.min(
            sampled_at + Duration::from_millis(binding.lease().deadline().millis() - now.millis()),
        );
        let snapshot = transport
            .snapshot()
            .map_err(|_| AgentWorkFailure::Contract)?;
        if snapshot.is_sealed() || !snapshot.is_idle() {
            return Err(AgentWorkFailure::Contract);
        }
        let scope = zephium_agent_runtime::AgentRuntimeScopedBinding::try_new(
            binding.lease().clone(),
            &input.manifest,
        )
        .map_err(|_| AgentWorkFailure::Contract)?;
        let (mut controller, handle) = AgentWorkController::with_transport(
            input,
            transport,
            credential,
            audit,
            task,
            retention,
            Some(browser),
        )?;
        let terminal = Arc::new(Mutex::new(None));
        controller.retained_terminal = Some(terminal.clone());
        Ok((
            Self(controller),
            AgentWorkRetainedHandle {
                events: handle.events,
                terminal,
            },
            scope,
        ))
    }
}

impl zephium_agent_runtime::AgentRuntimeScopedController for AgentWorkRetainedController {
    fn run(self: Box<Self>, mut worker: AgentRuntimeWorker) -> AgentRuntimeControllerFuture {
        Box::pin(async move {
            let mut controller = self.0;
            let registered = std::future::poll_fn(|cx| {
                std::task::Poll::Ready(
                    controller
                        .state
                        .as_mut()
                        .ok_or(AgentWorkFailure::Contract)
                        .and_then(|state| {
                            state
                                .native
                                .retained
                                .as_mut()
                                .ok_or(AgentWorkFailure::Contract)
                        })
                        .and_then(|browser| browser.register_listener(cx.waker().clone())),
                )
            })
            .await;
            let result = match registered {
                Ok(()) => {
                    controller
                        .execute(&mut worker, &WorkBrowser::Retained)
                        .await
                }
                Err(error) => Err(error),
            };
            if let Err(failure) = result {
                if let Some(state) = controller.state.as_mut() {
                    state.failure = Some(failure);
                    let _ = state.native.revoke(&WorkBrowser::Retained);
                    if let Some(session) = &state.session {
                        session.cancel();
                    }
                }
                // Closing the session already transferred its original owners
                // into WorkDrained before audit dispatch. Always reconcile that
                // accepted callback, including after control/mailbox failure.
                let deadline = controller
                    .drain_recovery(&mut worker, &WorkBrowser::Retained)
                    .await;
                if controller
                    .state
                    .as_ref()
                    .is_some_and(|state| state.drained.is_some())
                {
                    // The initial ordinary close already chose its supervisor
                    // outcome. Do not complete it again or relabel it on stop.
                    // Original metric/audit/runtime guards may still refuse.
                    if Instant::now() < deadline
                        && controller.state.as_mut().is_some_and(|state| {
                            state
                                .journal_mut()
                                .is_ok_and(|journal| journal.audit.is_quiescent())
                        })
                    {
                        let _ = controller.publish_terminal(&mut worker, false).await;
                    }
                } else {
                    let _ = controller.close_retained(&mut worker, Some(deadline)).await;
                }
            }
        })
    }
}

impl WorkNative {
    pub(super) async fn next_navigation_event(
        &mut self,
        worker: &mut AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
    ) -> Result<AgentRuntimeEvent, AgentWorkFailure> {
        if self.retained.is_none() {
            return self.next_event(worker, browser).await;
        }
        self.next_retained_operation_event(worker, browser, None)
            .await
    }
    pub(super) async fn next_action_event(
        &mut self,
        worker: &mut AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
        deadline: SemanticActionExecutionInstant,
    ) -> Result<AgentRuntimeEvent, AgentWorkFailure> {
        if self.retained.is_none() {
            return self.next_event(worker, browser).await;
        }
        self.next_retained_operation_event(worker, browser, Some(deadline))
            .await
    }
    async fn next_retained_operation_event(
        &mut self,
        worker: &mut AgentRuntimeWorker,
        browser: &WorkBrowser<'_>,
        action: Option<SemanticActionExecutionInstant>,
    ) -> Result<AgentRuntimeEvent, AgentWorkFailure> {
        if action.is_some() {
            self.check_stop(worker, browser)?;
        } else {
            self.check_control(worker, browser)?;
        }
        let clock = self
            .clock
            .as_ref()
            .ok_or(AgentWorkFailure::Contract)?
            .clone();
        let retained = self.retained.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let deadline = if let Some(deadline) = action {
            let now = clock.now().map_err(|_| AgentWorkFailure::Contract)?;
            Instant::now()
                .checked_add(Duration::from_millis(
                    deadline.millis().saturating_sub(now.millis()),
                ))
                .ok_or(AgentWorkFailure::Contract)?
                .min(self.deadline)
        } else {
            self.deadline
        };
        let result = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), async {
            let event = worker.next_event();
            tokio::pin!(event);
            std::future::poll_fn(|cx| {
                // Control wins even over an already available terminal.
                if let std::task::Poll::Ready(event) = std::future::Future::poll(event.as_mut(), cx)
                {
                    return std::task::Poll::Ready(Err(event));
                }
                match clock
                    .now()
                    .map_err(|_| AgentWorkFailure::Contract)
                    .and_then(|now| {
                        if action.is_some() {
                            // The original action is evidence debt, not a
                            // new admission. Poll before observing health,
                            // including when terminal publication wins the
                            // race with the resource-health notification.
                            retained.poll_action(now).map(|terminal| {
                                terminal.map(AgentRuntimeEvent::SemanticActionTerminal)
                            })
                        } else {
                            retained.check_health(now)?;
                            retained.poll_navigation(now).map(|terminal| {
                                terminal.map(|terminal| {
                                    AgentRuntimeEvent::NativeTerminal(
                                        ContextNativeEvent::NavigationSettled(terminal),
                                    )
                                })
                            })
                        }
                    }) {
                    Ok(None) => std::task::Poll::Pending,
                    Ok(Some(terminal)) => std::task::Poll::Ready(Ok(Ok(terminal))),
                    Err(error) => std::task::Poll::Ready(Ok(Err(error))),
                }
            })
            .await
        })
        .await;
        match result {
            Ok(Ok(Ok(terminal))) => Ok(terminal),
            Ok(Ok(Err(error))) => Err(error),
            Ok(Err(Ok(
                AgentRuntimeEvent::CancellationRequested | AgentRuntimeEvent::ShutdownRequested,
            ))) => {
                self.check_control(worker, browser)?;
                Err(AgentWorkFailure::Mailbox)
            }
            Ok(Err(Ok(event))) => {
                self.retain(event)?;
                Err(AgentWorkFailure::Mailbox)
            }
            Ok(Err(Err(_))) => Err(AgentWorkFailure::Mailbox),
            Err(_) => Err(AgentWorkFailure::Deadline),
        }
    }
    pub(super) fn check_retained_health(&self) -> Result<(), AgentWorkFailure> {
        if let Some(browser) = &self.retained {
            let now = self
                .clock
                .as_ref()
                .ok_or(AgentWorkFailure::Contract)?
                .now()
                .map_err(|_| AgentWorkFailure::Contract)?;
            browser.check_health(now)?;
        }
        Ok(())
    }

    pub(super) async fn observe_retained(
        &mut self,
        worker: &mut AgentRuntimeWorker,
    ) -> Result<SemanticObservation, AgentWorkFailure> {
        self.observe_retained_scope(worker, None).await
    }

    pub(super) async fn observe_retained_scope(
        &mut self,
        worker: &mut AgentRuntimeWorker,
        expansion: Option<(
            &SemanticObservation,
            &SemanticObservationAcknowledgement,
            SemanticReferenceId,
            SemanticExpansionKind,
        )>,
    ) -> Result<SemanticObservation, AgentWorkFailure> {
        self.check_control(worker, &WorkBrowser::Retained)?;
        let clock = self
            .clock
            .as_ref()
            .ok_or(AgentWorkFailure::Contract)?
            .clone();
        let browser = self.retained.as_mut().ok_or(AgentWorkFailure::Contract)?;
        let now = clock.now().map_err(|_| AgentWorkFailure::Contract)?;
        match expansion {
            Some((previous, acknowledgement, target, kind)) => {
                browser.begin_expansion(previous, acknowledgement, target, kind, now)?
            }
            None => browser.begin_observation(now)?,
        }
        let result =
            tokio::time::timeout_at(tokio::time::Instant::from_std(self.deadline), async {
                let event = worker.next_event();
                tokio::pin!(event);
                std::future::poll_fn(|cx| {
                    if let std::task::Poll::Ready(event) =
                        std::future::Future::poll(event.as_mut(), cx)
                    {
                        return std::task::Poll::Ready(Err(event));
                    }
                    let result = clock
                        .now()
                        .map_err(|_| AgentWorkFailure::Contract)
                        .and_then(|now| {
                            browser.check_health(now)?;
                            browser.poll_observation(now)
                        });
                    match result {
                        Ok(None) => std::task::Poll::Pending,
                        Ok(Some(observation)) => std::task::Poll::Ready(Ok(Ok(observation))),
                        Err(error) => std::task::Poll::Ready(Ok(Err(error))),
                    }
                })
                .await
            })
            .await;
        match result {
            Ok(Ok(result)) => result,
            Ok(Err(Ok(
                AgentRuntimeEvent::CancellationRequested | AgentRuntimeEvent::ShutdownRequested,
            ))) => {
                self.check_control(worker, &WorkBrowser::Retained)?;
                Err(AgentWorkFailure::Mailbox)
            }
            Ok(Err(Ok(event))) => {
                self.retain(event)?;
                Err(AgentWorkFailure::Mailbox)
            }
            Ok(Err(Err(_))) => Err(AgentWorkFailure::Mailbox),
            Err(_) => Err(AgentWorkFailure::Deadline),
        }
    }
}

impl AgentWorkController {
    /// Reconcile resource debt after stop without inventing effect verification.
    /// The original policy owner and native terminal remain in recovery whenever
    /// cancellation prevents fresh post-action verification.
    pub(super) async fn drain_retained_action(state: &mut WorkState, deadline: Instant) {
        if state.native.retained.is_none() || !state.native.action_pending {
            return;
        }
        let Some(clock) = state.native.clock.clone() else {
            return;
        };
        let Some(browser) = state.native.retained.as_mut() else {
            return;
        };
        let terminal = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            std::future::poll_fn(|_| {
                match clock
                    .now()
                    .map_err(|_| AgentWorkFailure::Contract)
                    .and_then(|now| browser.poll_action(now))
                {
                    Ok(None) => std::task::Poll::Pending,
                    Ok(Some(terminal)) => std::task::Poll::Ready(Some(terminal)),
                    Err(_) => std::task::Poll::Ready(None),
                }
            }),
        )
        .await
        .ok()
        .flatten();
        if let Some(terminal) = terminal {
            if state.session.as_ref().is_some_and(|session| {
                session.action.as_ref().is_some_and(|action| {
                    action.accepts_settlement(&session.action_executions, &terminal)
                })
            }) {
                state.native.action_pending = false;
            }
            let _ = state
                .native
                .retain(AgentRuntimeEvent::SemanticActionTerminal(terminal));
        }
    }
    /// Resource receipt and policy/audit receipt are two independent owners.
    /// Stop cannot discard the former while leaving the latter unaccounted.
    pub(super) async fn drain_retained_navigation(state: &mut WorkState, deadline: Instant) {
        if state.native.retained.is_none() || state.native.operation.is_none() {
            return;
        }
        let operation = state.native.operation;
        let deferred = state.native.deferred.iter().position(|event| {
            matches!(event,
            AgentRuntimeEvent::NativeTerminal(ContextNativeEvent::NavigationSettled(terminal))
            if Some(terminal.operation()) == operation)
        });
        let terminal = if let Some(index) = deferred {
            match state.native.deferred.remove(index) {
                AgentRuntimeEvent::NativeTerminal(ContextNativeEvent::NavigationSettled(
                    terminal,
                )) => Some(terminal),
                _ => None,
            }
        } else {
            let Some(clock) = state.native.clock.clone() else {
                return;
            };
            let Some(browser) = state.native.retained.as_mut() else {
                return;
            };
            tokio::time::timeout_at(
                tokio::time::Instant::from_std(deadline),
                std::future::poll_fn(|_| {
                    match clock
                        .now()
                        .map_err(|_| AgentWorkFailure::Contract)
                        .and_then(|now| browser.poll_navigation(now))
                    {
                        Ok(None) => std::task::Poll::Pending,
                        Ok(Some(terminal)) => std::task::Poll::Ready(Some(terminal)),
                        Err(_) => std::task::Poll::Ready(None),
                    }
                }),
            )
            .await
            .ok()
            .flatten()
        };
        let Some(terminal) = terminal else {
            return;
        };
        if Some(terminal.operation()) == operation
            && state
                .session
                .as_mut()
                .is_some_and(|session| session.settle_navigation_terminal(&terminal).is_ok())
        {
            state.native.operation = None;
        } else {
            let _ = state.native.retain(AgentRuntimeEvent::NativeTerminal(
                ContextNativeEvent::NavigationSettled(terminal),
            ));
        }
    }
    pub(super) async fn close_retained(
        &mut self,
        worker: &mut AgentRuntimeWorker,
        cleanup: Option<Instant>,
    ) -> Result<(), AgentWorkFailure> {
        let state = self.state.as_mut().ok_or(AgentWorkFailure::Contract)?;
        if state.drained.is_some() || state.native.resources.is_some() {
            return Err(AgentWorkFailure::Contract);
        }
        // A retained accepted outcome must contain the independently accepted
        // source-bound result; task `Complete` before mapping is insufficient.
        if cleanup.is_none() && state.extraction.is_none() {
            return Err(AgentWorkFailure::Contract);
        }
        if cleanup.is_none() {
            state.native.check_control(worker, &WorkBrowser::Retained)?;
        }
        state.native.revoke(&WorkBrowser::Retained)?;
        let deadline = cleanup.unwrap_or(state.native.deadline);
        let clock = state
            .native
            .clock
            .as_ref()
            .ok_or(AgentWorkFailure::Contract)?
            .clone();
        let browser = state
            .native
            .retained
            .as_mut()
            .ok_or(AgentWorkFailure::Contract)?;
        if state.native.retained_delivery.is_none() {
            let delivery = tokio::time::timeout_at(
                tokio::time::Instant::from_std(deadline),
                std::future::poll_fn(|_| {
                    match clock
                        .now()
                        .map_err(|_| AgentWorkFailure::Contract)
                        .and_then(|now| browser.poll_revocation(now))
                    {
                        Ok(None) => std::task::Poll::Pending,
                        Ok(Some(proof)) => std::task::Poll::Ready(Ok(proof)),
                        Err(error) => std::task::Poll::Ready(Err(error)),
                    }
                }),
            )
            .await
            .map_err(|_| AgentWorkFailure::Deadline)??;
            state.native.retained_delivery = Some(delivery);
        }
        let session = state.session.take().ok_or(AgentWorkFailure::Contract)?;
        let terminal = match if cleanup.is_some() {
            session.try_finish_unsuccessful()
        } else {
            session.try_finish()
        } {
            Ok(terminal) => terminal,
            Err(refusal) => {
                let error = refusal.error();
                state.session = Some(refusal.into_session());
                return Err(AgentWorkFailure::Browser(error));
            }
        };
        let AgentBrowserSession {
            policy,
            journal,
            action_proposal_failure,
            ..
        } = *terminal.session;
        state.drained = Some(WorkDrained {
            proposal_refusal: action_proposal_failure,
            policy: Some(policy),
            journal,
            provider: Some(terminal.provider),
            native: None,
            resources: None,
            proof: None,
            delivery: state.native.retained_delivery.take(),
            scoped_refusal: None,
        });
        self.finish_accounting(
            worker,
            &WorkBrowser::Retained,
            Some(deadline).filter(|_| cleanup.is_some()),
        )
        .await
    }
}
