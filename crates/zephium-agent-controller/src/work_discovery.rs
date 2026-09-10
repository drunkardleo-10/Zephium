//! Scoped read-only objective contract, without a route or answer oracle.
use crate::{AgentWorkExtractionTask, AgentWorkFailure, AgentWorkTask, AgentWorkTaskProgress};
use std::cell::Cell;
use zephium_agentic::*;

/// Trusted host account facts for the current browser document.
///
/// Implementations must return an independently collected sample, preserving
/// its original ID and collection time. Polling a cached sample is allowed;
/// changing its timestamp is not. Calls must be bounded and nonblocking. A host
/// may refresh its source independently of model turns; missing facts refuse
/// admission. Page text, model output and profile selection are not account
/// attestations. The session enforces freshness, monotonicity and replay bounds.
pub trait AgentWorkAccountSource: Send {
    /// Returns the latest independently collected sample for this exact context.
    fn sample(&self, context: ContextJoin) -> Result<AgentContextAccountBinding, AgentWorkFailure>;
}

/// Host-owned approval and independent effect classifier for local reversible
/// actions, including read-only UI interactions, during an open objective. The
/// host freezes the approved intent before admission and must establish that
/// the exact target and operands are within that intent and have only local effects.
/// Roles, names, page claims and the model-declared effect do not establish
/// this: autosaving inputs, submits, remote writes and capability boundaries
/// must refuse. Calls must be bounded and nonblocking. No native work belongs
/// here; the controller separately admits, executes and verifies each action.
pub trait AgentWorkLocalActionPolicy: Send {
    /// Returns the independently approved operation subset for this exact
    /// observed node. This is model-affordance projection only; `assess`
    /// remains the final host classification after semantic action binding.
    fn model_action_operations(
        &self,
        node: &SemanticNode,
        observation: &SemanticObservation,
    ) -> Result<SemanticOperations, AgentWorkFailure>;

    /// Resolves the model-selected proposal against fresh semantic evidence and
    /// independently returns its actual destination and effect, or refuses.
    fn assess(
        &self,
        action: &SemanticPreparedAction,
        observation: &SemanticObservation,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure>;

    /// Advances policy-owned task progress only after independent native
    /// verification. Pre-dispatch assessment must remain side-effect free.
    fn accept_verified_action(
        &mut self,
        _: &SemanticActionBatchResult,
        _: &SemanticObservation,
    ) -> Result<(), AgentWorkFailure> {
        Ok(())
    }

    /// Whether the required verified local-action sequence is complete.
    fn terminal_extraction_ready(&self) -> bool {
        true
    }
}

/// Bounded observed-link exploration followed by one current-document mapping.
/// Completion proves source-backed shape, not the usefulness or truth of an answer.
pub struct AgentWorkDiscoveryTask {
    identity: ContextIdentity,
    discovery: AgentNavigationDiscovery,
    extraction: AgentWorkExtractionTask,
    account: Cell<Option<AgentContextAccountBinding>>,
    account_scope: AgentAccountScope,
    account_source: Option<Box<dyn AgentWorkAccountSource>>,
    current: Option<(ContextJoin, SemanticObservationId)>,
    complete: bool,
    local_actions: Option<Box<dyn AgentWorkLocalActionPolicy>>,
}

impl AgentWorkDiscoveryTask {
    /// Freezes public scope and a schema before any provider/native execution.
    pub fn try_new(
        identity: ContextIdentity,
        discovery: AgentNavigationDiscovery,
        fields: Vec<SemanticExtractionFieldSchema>,
    ) -> Result<Self, AgentWorkFailure> {
        if identity.kind() != ContextKind::Owned {
            return Err(AgentWorkFailure::Contract);
        }
        Ok(Self {
            identity,
            discovery,
            extraction: AgentWorkExtractionTask::try_new(fields, AgentAccountScope::Anonymous)?
                .with_baseline_read(),
            account: Cell::new(None),
            account_scope: AgentAccountScope::Anonymous,
            account_source: None,
            current: None,
            complete: false,
            local_actions: None,
        })
    }

    /// Freezes an account and host sampling source alongside navigation scope.
    /// The manifest must independently approve that same account. This grants
    /// no additional effects or permission to disclose sensitive content.
    pub fn try_new_with_account_source(
        identity: ContextIdentity,
        discovery: AgentNavigationDiscovery,
        fields: Vec<SemanticExtractionFieldSchema>,
        account: AgentAccountScope,
        source: Box<dyn AgentWorkAccountSource>,
    ) -> Result<Self, AgentWorkFailure> {
        let mut task = Self::try_new(identity, discovery, fields)?;
        task.account_scope = account;
        task.account_source = Some(source);
        Ok(task)
    }

    /// Freezes an independently approved local-action contract. This neither
    /// grants manifest effects nor increases the run's operation/action budget.
    /// Targets and the route remain model-selected from current observations.
    pub fn with_local_actions(mut self, policy: Box<dyn AgentWorkLocalActionPolicy>) -> Self {
        self.local_actions = Some(policy);
        self
    }
}

impl AgentWorkTask for AgentWorkDiscoveryTask {
    fn navigation_discovery(&self) -> Option<&AgentNavigationDiscovery> {
        Some(&self.discovery)
    }
    fn allows_baseline_read(&self) -> bool {
        true
    }
    fn allows_progressive_observation(&self) -> bool {
        true
    }
    fn allows_standalone_wait(&self) -> bool {
        self.discovery.is_production()
    }
    fn allows_viewport_screenshot(&self) -> bool {
        self.discovery.is_production()
    }
    fn allows_human_request(&self) -> bool {
        self.discovery.is_production()
    }
    fn allows_history_back(&self) -> bool {
        self.discovery.is_production()
    }
    fn extraction_schema(&self) -> Option<&SemanticExtractionSchema> {
        self.extraction.extraction_schema()
    }
    fn allows_actions_before_extraction(&self) -> bool {
        self.local_actions.is_some()
    }
    fn model_action_operations(
        &self,
        node: &SemanticNode,
        observation: &SemanticObservation,
    ) -> Result<SemanticOperations, AgentWorkFailure> {
        self.local_actions
            .as_ref()
            .ok_or(AgentWorkFailure::Contract)?
            .model_action_operations(node, observation)
    }
    fn accept_verified_action(
        &mut self,
        result: &SemanticActionBatchResult,
        observation: &SemanticObservation,
    ) -> Result<(), AgentWorkFailure> {
        self.local_actions
            .as_mut()
            .ok_or(AgentWorkFailure::Contract)?
            .accept_verified_action(result, observation)
    }
    fn terminal_extraction_ready(&self) -> bool {
        self.local_actions
            .as_ref()
            .is_none_or(|policy| policy.terminal_extraction_ready())
    }
    fn evaluate(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        self.current = None;
        let context = observation.request().context();
        if self.complete
            || context.identity() != self.identity
            || context.frame() != FrameId::MAIN
            || observation.frames().len() != 1
            // The controller retains blocked embedded-frame boundaries as
            // evidence; they do not grant child-frame capture/model authority.
            || observation.frame_boundaries().iter().any(|boundary| {
                boundary.parent_frame() != FrameId::MAIN
                    || boundary.status()
                        != SemanticFrameBoundaryStatus::Unsupported(
                            SemanticFrameUnsupported::PolicyBlocked,
                        )
            })
            || !self
                .discovery
                .admits_origin(observation.frames()[0].frame().origin())
        {
            return Err(AgentWorkFailure::Contract);
        }
        self.current = Some((context, observation.request().id()));
        Ok(AgentWorkTaskProgress::Continue)
    }
    fn assess(
        &self,
        _: &SemanticPreparedAction,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        Err(AgentWorkFailure::Contract)
    }
    fn assess_observed(
        &self,
        action: &SemanticPreparedAction,
        observation: &SemanticObservation,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        let policy = self
            .local_actions
            .as_ref()
            .ok_or(AgentWorkFailure::Contract)?;
        let request = observation.request();
        let snapshot = observation
            .frames()
            .first()
            .ok_or(AgentWorkFailure::Contract)?;
        if self.complete
            || self.current != Some((request.context(), request.id()))
            || action.source_observation() != request.id()
            || action.source_observation_generation() != request.generation()
            || action.frame() != snapshot.frame()
            || action.bound_action().snapshot_generation() != snapshot.generation()
            || action.checkpoint_snapshot() != snapshot.generation()
            || action.checkpoint_invocation() != snapshot.invocation()
            || !matches!(
                action.effect(),
                SemanticEffectClass::Read | SemanticEffectClass::LocalWrite
            )
        {
            return Err(AgentWorkFailure::Contract);
        }
        let assessment = policy.assess(action, observation)?;
        if !assessment.matches_action(action)
            || assessment.actual_effect() != action.effect()
            || assessment.destination_origin() != snapshot.frame().origin()
            || !self
                .discovery
                .admits_origin(assessment.destination_origin())
        {
            return Err(AgentWorkFailure::Contract);
        }
        Ok(assessment)
    }
    fn attest_account(
        &self,
        context: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        if context.identity() != self.identity {
            return Err(AgentWorkFailure::Contract);
        }
        if let Some(sample) = self.account.get() {
            if sample.context() == context && self.account_source.is_none() {
                if now < sample.observed_at() {
                    return Err(AgentWorkFailure::Contract);
                }
                if now == sample.observed_at() {
                    return Ok(sample);
                }
            }
            let prior = sample.context();
            if prior != context
                && (prior.context_generation() != context.context_generation()
                    || prior.cancellation_generation() != context.cancellation_generation()
                    || prior.navigation_epoch().get().checked_add(1)
                        != Some(context.navigation_epoch().get())
                    || prior.frame_generation().get().checked_add(1)
                        != Some(context.frame_generation().get()))
            {
                return Err(AgentWorkFailure::Contract);
            }
        }
        if let Some(source) = &self.account_source {
            let sample = source.sample(context)?;
            if sample.context() != context || sample.account() != self.account_scope {
                return Err(AgentWorkFailure::Contract);
            }
            self.account.set(Some(sample));
            return Ok(sample);
        }
        // Anonymous scope has no external service identity to re-sample. The
        // trusted monotonic Work clock may therefore mint a fresh same-context
        // attestation at each idle boundary; session policy still enforces
        // identity, ordering, replay, age and the per-run attestation cap.
        let sample = AgentContextAccountBinding::new(
            AgentAccountAttestationId::generate(),
            context,
            AgentAccountScope::Anonymous,
            now,
        );
        self.account.set(Some(sample));
        Ok(sample)
    }
    fn accept_extraction(
        &mut self,
        result: &SemanticExtractionResult<'_>,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        let (context, observation) = self.current.ok_or(AgentWorkFailure::Contract)?;
        if self.complete
            || result.observation() != observation
            || self.account.get().map(|sample| sample.context()) != Some(context)
            || !self.terminal_extraction_ready()
        {
            return Err(AgentWorkFailure::Contract);
        }
        let progress = self.extraction.accept_extraction(result)?;
        self.complete = true;
        Ok(progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct LocalPolicy {
        effect: SemanticEffectClass,
        destination: SemanticOrigin,
    }
    impl AgentWorkLocalActionPolicy for LocalPolicy {
        fn model_action_operations(
            &self,
            node: &SemanticNode,
            _: &SemanticObservation,
        ) -> Result<SemanticOperations, AgentWorkFailure> {
            if node.name().is_some_and(|name| name.as_str() == "Search") {
                SemanticOperations::try_new(&[
                    SemanticOperationClass::Click,
                    SemanticOperationClass::Fill,
                ])
                .map_err(|_| AgentWorkFailure::Contract)
            } else {
                Ok(SemanticOperations::NONE)
            }
        }

        fn assess(
            &self,
            action: &SemanticPreparedAction,
            _: &SemanticObservation,
        ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
            let approved = match action.kind() {
                SemanticActionKind::Click => {
                    action.verification()
                        == SemanticVerification::TargetState {
                            state: SemanticState::Focused,
                            present: true,
                        }
                }
                SemanticActionKind::Fill => {
                    action.fill_text().map(SemanticActionText::as_str) == Some("approved")
                }
                _ => false,
            };
            if !approved {
                return Err(AgentWorkFailure::Contract);
            }
            Ok(AgentEffectAssessment::new(
                action,
                self.destination.clone(),
                self.effect,
            ))
        }
    }

    fn local_observation(context: ContextJoin, generation: u64) -> SemanticObservation {
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            SemanticOrigin::parse("https://example.test/").unwrap(),
            SemanticFrameTrust::SameOrigin,
        )
        .unwrap();
        let wire = format!(
            r#"{{"v":1,"i":{generation},"g":{generation},"c":"complete","n":[{{"k":1,"r":"document","o":16}},{{"k":2,"p":0,"r":"textbox","n":"Search","s":0,"o":3,"v":{{"k":"text","value":""}},"b":{{"x":1,"y":2,"w":100,"h":30}}}}]}}"#
        );
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(generation).unwrap(),
                frame,
                SemanticSnapshotGeneration::new(generation).unwrap(),
            ),
            wire.as_bytes(),
        )
        .unwrap();
        SemanticObservationAssembler::new(
            SemanticObservationRequest::initial(
                SemanticObservationId::new(generation).unwrap(),
                context,
                SemanticObservationBudget::INITIAL_FILTERED,
            ),
            snapshot,
        )
        .unwrap()
        .finish()
        .unwrap()
    }

    fn local_action(
        observation: &SemanticObservation,
        value: &str,
        effect: SemanticEffectClass,
    ) -> SemanticPreparedAction {
        let proposal = SemanticActionProposal::try_new(
            SemanticActionIntent::Fill {
                target: SemanticReferenceId::new(2).unwrap(),
                value: SemanticActionText::try_new(value.into()).unwrap(),
            },
            effect,
            SemanticWaitCondition::Immediate,
            SemanticVerification::TargetValueMatchesInput,
            SemanticSettleBudget::try_new(2000).unwrap(),
        )
        .unwrap();
        let snapshot = &observation.frames()[0];
        let batch = SemanticActionBatch::bind(
            SemanticActionBatchId::new(1).unwrap(),
            observation,
            &[snapshot.frame().clone()],
            vec![proposal],
        )
        .unwrap();
        batch.actions()[0].prepare(snapshot).unwrap()
    }

    #[test]
    fn read_click_requires_matching_host_classification_and_current_evidence() {
        let context = context();
        let observation = local_observation(context, 1);
        let snapshot = &observation.frames()[0];
        let proposal = SemanticActionProposal::try_new(
            SemanticActionIntent::Click {
                target: SemanticReferenceId::new(2).unwrap(),
            },
            SemanticEffectClass::Read,
            SemanticWaitCondition::Immediate,
            SemanticVerification::TargetState {
                state: SemanticState::Focused,
                present: true,
            },
            SemanticSettleBudget::try_new(2000).unwrap(),
        )
        .unwrap();
        let batch = SemanticActionBatch::bind(
            SemanticActionBatchId::new(1).unwrap(),
            &observation,
            &[snapshot.frame().clone()],
            vec![proposal],
        )
        .unwrap();
        let action = batch.actions()[0].prepare(snapshot).unwrap();
        let origin = SemanticOrigin::parse("https://example.test/").unwrap();
        for (effect, destination, accepted) in [
            (SemanticEffectClass::Read, origin.clone(), true),
            (SemanticEffectClass::LocalWrite, origin.clone(), false),
            (SemanticEffectClass::ExternalWrite, origin.clone(), false),
            (
                SemanticEffectClass::Read,
                SemanticOrigin::parse("https://foreign.test/").unwrap(),
                false,
            ),
        ] {
            let mut task = AgentWorkDiscoveryTask::try_new(
                context.identity(),
                AgentNavigationDiscovery::try_new(
                    ContextNavigationTarget::parse("https://example.test/start").unwrap(),
                    "/".into(),
                    2,
                )
                .unwrap(),
                vec![SemanticExtractionFieldSchema::try_text("answer".into(), true, 64).unwrap()],
            )
            .unwrap();
            task.evaluate(&observation).unwrap();
            assert!(task.assess_observed(&action, &observation).is_err());
            let mut task = task.with_local_actions(Box::new(LocalPolicy {
                effect,
                destination,
            }));
            assert_eq!(
                task.assess_observed(&action, &observation).is_ok(),
                accepted
            );
            task.evaluate(&local_observation(context, 2)).unwrap();
            assert!(task.assess_observed(&action, &observation).is_err());
        }
    }

    #[test]
    fn local_action_approval_is_current_exact_and_independent_of_model_effect() {
        let context = context();
        let old = local_observation(context, 1);
        let fresh = local_observation(context, 2);
        let old_action = local_action(&old, "approved", SemanticEffectClass::LocalWrite);
        let action = local_action(&fresh, "approved", SemanticEffectClass::LocalWrite);
        let origin = SemanticOrigin::parse("https://example.test/").unwrap();
        for (effect, destination, accepted) in [
            (SemanticEffectClass::LocalWrite, origin.clone(), true),
            (SemanticEffectClass::ExternalWrite, origin.clone(), false),
            (SemanticEffectClass::Read, origin.clone(), false),
            (
                SemanticEffectClass::LocalWrite,
                SemanticOrigin::parse("https://foreign.test/").unwrap(),
                false,
            ),
        ] {
            let mut task = AgentWorkDiscoveryTask::try_new(
                context.identity(),
                AgentNavigationDiscovery::try_new(
                    ContextNavigationTarget::parse("https://example.test/start").unwrap(),
                    "/".into(),
                    2,
                )
                .unwrap(),
                vec![SemanticExtractionFieldSchema::try_text("answer".into(), true, 64).unwrap()],
            )
            .unwrap();
            task.evaluate(&fresh).unwrap();
            assert!(
                task.assess_observed(&action, &fresh).is_err(),
                "read-only task has no action grant"
            );
            let mut task = task.with_local_actions(Box::new(LocalPolicy {
                effect,
                destination,
            }));
            assert_eq!(task.assess_observed(&action, &fresh).is_ok(), accepted);
            assert!(
                task.assess(&action).is_err(),
                "current evidence is mandatory"
            );
            assert!(task.assess_observed(&old_action, &old).is_err());
            assert!(task.assess_observed(&old_action, &fresh).is_err());
            assert!(task
                .assess_observed(
                    &local_action(&fresh, "unapproved", SemanticEffectClass::LocalWrite),
                    &fresh
                )
                .is_err());
            assert!(task
                .assess_observed(
                    &local_action(&fresh, "approved", SemanticEffectClass::ExternalWrite),
                    &fresh
                )
                .is_err());
            assert!(task
                .evaluate(&local_observation(self::context(), 3))
                .is_err());
            assert!(
                task.assess_observed(&action, &fresh).is_err(),
                "failed refresh revokes prior task bindings"
            );
        }
    }

    struct Source(Arc<Mutex<Option<AgentContextAccountBinding>>>);
    impl AgentWorkAccountSource for Source {
        fn sample(&self, _: ContextJoin) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
            self.0.lock().unwrap().ok_or(AgentWorkFailure::Contract)
        }
    }

    fn context() -> ContextJoin {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            1_u128.into(),
            ContextKind::Owned,
        );
        let mut registry = ContextRegistry::new();
        registry
            .reserve(
                identity,
                ContextCapabilities::try_new(ContextKind::Owned, &[]).unwrap(),
            )
            .unwrap();
        registry
            .begin_context(identity.id(), ContextOperationId::new(1).unwrap())
            .unwrap()
            .context()
    }

    #[test]
    fn anonymous_account_attestation_renews_only_from_the_trusted_monotonic_clock() {
        let context = context();
        let task = AgentWorkDiscoveryTask::try_new(
            context.identity(),
            AgentNavigationDiscovery::try_new(
                ContextNavigationTarget::parse("https://example.test/start").unwrap(),
                "/".into(),
                2,
            )
            .unwrap(),
            vec![SemanticExtractionFieldSchema::try_text("summary".into(), true, 64).unwrap()],
        )
        .unwrap();
        let first = task
            .attest_account(context, AgentPolicyInstant::from_millis(100))
            .unwrap();
        assert_eq!(
            task.attest_account(context, AgentPolicyInstant::from_millis(100)),
            Ok(first),
            "one clock instant does not consume another attestation"
        );
        let renewed = task
            .attest_account(context, AgentPolicyInstant::from_millis(30_101))
            .unwrap();
        assert_eq!(renewed.context(), first.context());
        assert_eq!(renewed.account(), AgentAccountScope::Anonymous);
        assert_ne!(renewed.attestation(), first.attestation());
        assert_eq!(
            renewed.observed_at(),
            AgentPolicyInstant::from_millis(30_101)
        );
        assert_eq!(
            task.attest_account(context, AgentPolicyInstant::from_millis(30_100)),
            Err(AgentWorkFailure::Contract),
            "the task cannot rewrite a trusted clock backwards"
        );
    }

    #[test]
    fn account_source_preserves_original_samples_and_refuses_missing_or_substituted_identity() {
        let context = context();
        let account = AgentAccountScope::Authenticated(AgentAccountId::generate());
        let published = Arc::new(Mutex::new(None));
        let task = AgentWorkDiscoveryTask::try_new_with_account_source(
            context.identity(),
            AgentNavigationDiscovery::try_new(
                ContextNavigationTarget::parse("https://example.test/start").unwrap(),
                "/".into(),
                2,
            )
            .unwrap(),
            vec![SemanticExtractionFieldSchema::try_text("summary".into(), true, 64).unwrap()],
            account,
            Box::new(Source(published.clone())),
        )
        .unwrap();
        let now = AgentPolicyInstant::from_millis(100);
        assert_eq!(
            task.attest_account(context, now),
            Err(AgentWorkFailure::Contract)
        );
        let sample = AgentContextAccountBinding::new(
            AgentAccountAttestationId::generate(),
            context,
            account,
            now,
        );
        *published.lock().unwrap() = Some(sample);
        assert_eq!(task.attest_account(context, now), Ok(sample));
        // Later reads cannot synthesize freshness, even after the policy age
        // limit. The session is responsible for rejecting the original age.
        assert_eq!(
            task.attest_account(context, AgentPolicyInstant::from_millis(100_000)),
            Ok(sample)
        );
        for candidate in [
            AgentContextAccountBinding::new(
                AgentAccountAttestationId::generate(),
                context,
                AgentAccountScope::Anonymous,
                now,
            ),
            AgentContextAccountBinding::new(
                AgentAccountAttestationId::generate(),
                self::context(),
                account,
                now,
            ),
        ] {
            *published.lock().unwrap() = Some(candidate);
            assert_eq!(
                task.attest_account(context, now),
                Err(AgentWorkFailure::Contract)
            );
        }
        let renewed = AgentContextAccountBinding::new(
            AgentAccountAttestationId::generate(),
            context,
            account,
            AgentPolicyInstant::from_millis(101),
        );
        *published.lock().unwrap() = Some(renewed);
        assert_eq!(
            task.attest_account(context, AgentPolicyInstant::from_millis(101)),
            Ok(renewed)
        );
    }
}
