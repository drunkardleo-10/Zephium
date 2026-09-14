//! Provider search runs under the original admitted attempt, without a browser
//! lease or dependency disclosure. Provider citations remain provider evidence.
use crate::work_runtime::*;
use std::time::{Duration, Instant};
use zephium_core::work::{artifact::*, runtime::*, search::*, *};

/// One provider search outcome before publication. `record` is present only
/// for a validated, fully attributed success.
pub(crate) struct WorkSearchOutcome {
    pub(crate) status: WorkAttemptStatus,
    pub(crate) usage: Option<WorkUsage>,
    pub(crate) record: Option<WorkProviderSearchRecordV1>,
}

impl WorkAttemptProbe {
    /// Dispatches exactly one bounded search and reads back the settled
    /// outcome. Cancellation and the attempt deadline yield unknown outcomes,
    /// never a refund.
    pub(crate) async fn run_search(
        &self,
        provider: &dyn WorkPublicSearchProvider,
        scope: &WorkPublicSearchScope,
        context: &[context::WorkContextBody],
        limits: WorkExecutionLimits,
    ) -> Result<WorkSearchOutcome, WorkError> {
        if self.cancellation_requested().await? || Instant::now() >= self.deadline() {
            return Ok(WorkSearchOutcome {
                status: WorkAttemptStatus::Cancelled,
                usage: Some(WorkUsage::default()),
                record: None,
            });
        }
        self.record_activity(zephium_ipc::work::WorkActivityV1::Searching);
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
                _ = tokio::time::sleep_until(self.deadline().into()) => Err(WorkPublicSearchError::OutcomeUnknown),
                _ = cancelled => Err(WorkPublicSearchError::OutcomeUnknown),
                result = provider.search(scope, context, limits) => result,
            }
        };
        Ok(match result {
            Ok(result)
                if result.usage.within(limits)
                    && result.evidence.validate().is_ok()
                    && result.evidence.provider == scope.provider
                    && result.evidence.model == scope.model
                    && u64::from(result.usage.model_tokens)
                        == u64::from(result.evidence.actual_input_tokens)
                            + u64::from(result.evidence.actual_output_tokens) =>
            {
                WorkSearchOutcome {
                    status: WorkAttemptStatus::Succeeded,
                    usage: Some(result.usage),
                    record: Some(WorkProviderSearchRecordV1 {
                        id: WorkArtifactId::generate(),
                        node: self.node(),
                        attempt: self.attempt(),
                        evidence: result.evidence,
                    }),
                }
            }
            Err(WorkPublicSearchError::NotDispatched(_)) => WorkSearchOutcome {
                status: WorkAttemptStatus::Failed,
                usage: Some(WorkUsage::default()),
                record: None,
            },
            Err(WorkPublicSearchError::Rejected(usage)) if usage.within(limits) => {
                WorkSearchOutcome {
                    status: WorkAttemptStatus::Failed,
                    usage: Some(usage),
                    record: None,
                }
            }
            _ => WorkSearchOutcome {
                status: WorkAttemptStatus::OutcomeUnknown,
                usage: None,
                record: None,
            },
        })
    }
}

impl WorkNodeAttempt {
    pub async fn search_public_owned(
        self,
        provider: &dyn WorkPublicSearchProvider,
    ) -> Result<WorkNodeSettlement, WorkError> {
        self.search_public_owned_with_context(provider, &[]).await
    }

    /// `context` must be the bodies admitted for this exact execution; the
    /// persisted spec carries their manifest.
    pub async fn search_public_owned_with_context(
        self,
        provider: &dyn WorkPublicSearchProvider,
        context: &[zephium_core::work::context::WorkContextBody],
    ) -> Result<WorkNodeSettlement, WorkError> {
        let WorkCapability::PublicSearch { scope } = &self.specification().capability else {
            return self
                .settle_owned(empty(WorkAttemptStatus::Failed, Some(WorkUsage::default())))
                .await;
        };
        let limits = self.specification().limits;
        let outcome = self
            .probe()
            .run_search(provider, scope, context, limits)
            .await?;
        let Some(record) = outcome.record else {
            return self
                .settle_owned(empty(outcome.status, outcome.usage))
                .await;
        };
        let links: Vec<_> = (1..=record.evidence.citations.len())
            .map(|index| WorkEvidenceLink {
                extraction_id: record.id,
                source_id: index as u16,
            })
            .collect();
        let artifacts = self
            .node()
            .outputs
            .iter()
            .map(|output| WorkArtifactDraft {
                output: output.name.clone(),
                title: output.name.clone(),
                data: WorkArtifactDataV1::EvidenceCollection {
                    summary: record.evidence.answer.clone(),
                    subjects: vec![],
                    entries: vec![],
                },
                evidence: links.clone(),
            })
            .collect();
        self.settle_with_provider_evidence(
            WorkAdapterResult {
                status: WorkAttemptStatus::Succeeded,
                intervention: None,
                usage: outcome.usage,
                artifacts,
            },
            Some(record),
        )
        .await
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
