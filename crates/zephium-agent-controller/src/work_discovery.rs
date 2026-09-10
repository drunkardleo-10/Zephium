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
    fn extraction_schema(&self) -> Option<&SemanticExtractionSchema> {
        self.extraction.extraction_schema()
    }
    fn evaluate(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
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
            || observation.frames()[0].frame().origin() != self.discovery.origin()
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
