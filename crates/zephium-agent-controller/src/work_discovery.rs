//! Generic public read-only objective contract, without a route or answer oracle.
use crate::{AgentWorkExtractionTask, AgentWorkFailure, AgentWorkTask, AgentWorkTaskProgress};
use std::cell::Cell;
use zephium_agentic::*;

/// Bounded observed-link exploration followed by one current-document mapping.
/// Completion proves source-backed shape, not the usefulness or truth of an answer.
pub struct AgentWorkDiscoveryTask {
    identity: ContextIdentity,
    discovery: AgentNavigationDiscovery,
    extraction: AgentWorkExtractionTask,
    account: Cell<Option<AgentContextAccountBinding>>,
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
            current: None,
            complete: false,
        })
    }
}

impl AgentWorkTask for AgentWorkDiscoveryTask {
    fn navigation_discovery(&self) -> Option<&AgentNavigationDiscovery> {
        Some(&self.discovery)
    }
    fn allows_baseline_read(&self) -> bool {
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
            || !observation.frame_boundaries().is_empty()
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
            if sample.context() == context {
                return Ok(sample);
            }
            let prior = sample.context();
            if prior.context_generation() != context.context_generation()
                || prior.cancellation_generation() != context.cancellation_generation()
                || prior.navigation_epoch().get().checked_add(1)
                    != Some(context.navigation_epoch().get())
                || prior.frame_generation().get().checked_add(1)
                    != Some(context.frame_generation().get())
            {
                return Err(AgentWorkFailure::Contract);
            }
        }
        // The trusted caller admitted an isolated anonymous public session.
        // Cache each original document sample; repeated calls cannot renew it.
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
