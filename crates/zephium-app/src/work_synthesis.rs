//! Model production consumes the original admitted attempt. Provider output is
//! data; Rust joins evidence, enforces completion, and persists actual outcomes.
use crate::work_runtime::*;
use std::time::{Duration, Instant};
use zephium_core::work::{artifact::*, runtime::*, synthesis::*, *};

impl WorkNodeAttempt {
    /// Produce semantic artifacts from this attempt's approved dependencies.
    /// No provider retry or history-based reconstruction of an attempt exists.
    pub async fn synthesize(
        self,
        provider: &dyn WorkSynthesisProvider,
    ) -> Result<WorkRuntimeProjection, WorkError> {
        self.synthesize_owned(provider)
            .await
            .map(WorkNodeSettlement::into_projection)
    }

    /// Keep the fresh publication receipt for a parent that owns this worker.
    pub async fn synthesize_owned(
        self,
        provider: &dyn WorkSynthesisProvider,
    ) -> Result<WorkNodeSettlement, WorkError> {
        self.produce_artifacts(provider, false, &[]).await
    }

    /// `context` must be the bodies re-admitted for this execution's manifest.
    pub async fn synthesize_owned_with_context(
        self,
        provider: &dyn WorkSynthesisProvider,
        context: &[context::WorkContextBody],
    ) -> Result<WorkNodeSettlement, WorkError> {
        self.produce_artifacts(provider, false, context).await
    }

    pub(crate) async fn synthesize_primary_owned(
        self,
        provider: &dyn WorkSynthesisProvider,
    ) -> Result<WorkNodeSettlement, WorkError> {
        self.produce_artifacts(provider, true, &[]).await
    }

    async fn produce_artifacts(
        self,
        provider: &dyn WorkSynthesisProvider,
        coordinated: bool,
        context: &[context::WorkContextBody],
    ) -> Result<WorkNodeSettlement, WorkError> {
        if !(matches!(self.specification().capability, WorkCapability::Synthesize)
            || coordinated
                && matches!(
                    self.specification().capability,
                    WorkCapability::Coordinate { .. }
                        | WorkCapability::CoordinatePublicDiscovery { .. }
                        | WorkCapability::CoordinatePublicResearch { .. }
                ))
        {
            return self
                .settle_owned(empty(WorkAttemptStatus::Failed, Some(WorkUsage::default())))
                .await;
        }
        // Preflight can fetch bounded historical quotes but cannot dispatch a
        // model. Its entire lifetime is inside the original node deadline.
        let input = match tokio::time::timeout_at(
            self.deadline().into(),
            self.synthesis_disclosure(context),
        )
        .await
        {
            Ok(Ok(input)) => input,
            error => {
                provider.diagnostic(WorkSynthesisDiagnostic::DisclosureFailed {
                    attempt: self.attempt(),
                    error: match error {
                        Ok(Err(error)) => error,
                        _ => WorkError::Unavailable,
                    },
                });
                return self
                    .settle_owned(empty(WorkAttemptStatus::Failed, Some(WorkUsage::default())))
                    .await;
            }
        };
        provider.diagnostic(WorkSynthesisDiagnostic::DisclosureReady {
            attempt: self.attempt(),
            bytes: serde_json::to_vec(input.context())
                .map_err(|_| WorkError::Invalid)?
                .len(),
            sources: input.context().sources.len(),
            evidence: input.context().evidence.len(),
        });
        if self.cancellation_requested().await? || Instant::now() >= self.deadline() {
            return self
                .settle_owned(empty(
                    WorkAttemptStatus::Cancelled,
                    Some(WorkUsage::default()),
                ))
                .await;
        }
        self.record_activity(zephium_ipc::work::WorkActivityV1::ProducingArtifact);
        let result = {
            let cancelled = async {
                loop {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    if self.cancellation_requested().await.unwrap_or(true) {
                        break;
                    }
                }
            };
            tokio::select! {
                biased;
                _ = tokio::time::sleep_until(self.deadline().into()) => Err(WorkSynthesisError::OutcomeUnknown),
                _ = cancelled => Err(WorkSynthesisError::OutcomeUnknown),
                result = provider.produce_owned(&input, WorkSynthesisTrace { work: self.work(), execution: self.execution(), attempt: self.attempt() }) => result,
            }
        };
        if let Err(error) = &result {
            provider.diagnostic(WorkSynthesisDiagnostic::ProviderRefused {
                attempt: self.attempt(),
                error: *error,
            });
        }
        let result = match result {
            Ok(result) => {
                if !result.usage.within(self.specification().limits) {
                    empty(WorkAttemptStatus::OutcomeUnknown, None)
                } else {
                    match input.resolve(result.outputs) {
                        Ok(artifacts) => WorkAdapterResult {
                            status: WorkAttemptStatus::Succeeded,
                            intervention: None,
                            usage: Some(result.usage),
                            artifacts: artifacts
                                .into_iter()
                                .map(|artifact| WorkArtifactDraft {
                                    output: artifact.output,
                                    title: artifact.title,
                                    data: artifact.data,
                                    evidence: artifact.evidence,
                                })
                                .collect(),
                        },
                        Err(_) => empty(WorkAttemptStatus::Failed, Some(result.usage)),
                    }
                }
            }
            Err(WorkSynthesisError::NotDispatched(_)) => {
                empty(WorkAttemptStatus::Failed, Some(WorkUsage::default()))
            }
            Err(WorkSynthesisError::Rejected(usage) | WorkSynthesisError::Stalled(usage))
                if usage.within(self.specification().limits) =>
            {
                empty(WorkAttemptStatus::Failed, Some(usage))
            }
            Err(_) => empty(WorkAttemptStatus::OutcomeUnknown, None),
        };
        self.settle_owned(result).await
    }
    async fn synthesis_disclosure(
        &self,
        context: &[context::WorkContextBody],
    ) -> Result<WorkSynthesisDisclosure, WorkError> {
        let mut previews = Vec::<WorkEvidencePreviewV1>::new();
        for source in self.dependency_artifacts() {
            for link in &source.evidence {
                if previews.iter().any(|p| p.link == *link) {
                    continue;
                }
                if previews.len() >= 64 {
                    return Err(WorkError::Capacity);
                }
                let preview = self.read_evidence(link.clone()).await?;
                previews.push(preview);
            }
        }
        let mut disclosure_node = self.node().clone();
        disclosure_node.objective = self.disclosure_objective()?;
        WorkSynthesisDisclosure::try_new(
            &disclosure_node,
            self.dependency_artifacts(),
            &previews,
            self.specification().limits,
        )?
        .with_context(context.to_vec())
    }
}
fn empty(status: WorkAttemptStatus, usage: Option<WorkUsage>) -> WorkAdapterResult {
    WorkAdapterResult {
        status,
        usage,
        artifacts: vec![],
        intervention: None,
    }
}
