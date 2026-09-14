//! Provider search runs under the original admitted attempt, without a browser
//! lease or dependency disclosure. Provider citations remain provider evidence.
use crate::work_runtime::*;
use std::time::{Duration, Instant};
use zephium_core::work::{artifact::*, runtime::*, search::*, *};

impl WorkNodeAttempt {
    pub async fn search_public_owned(
        self,
        provider: &dyn WorkPublicSearchProvider,
    ) -> Result<WorkNodeSettlement, WorkError> {
        let WorkCapability::PublicSearch { scope } = &self.specification().capability else {
            return self
                .settle_owned(empty(WorkAttemptStatus::Failed, Some(WorkUsage::default())))
                .await;
        };
        if self.cancellation_requested().await? || Instant::now() >= self.deadline() {
            return self
                .settle_owned(empty(
                    WorkAttemptStatus::Cancelled,
                    Some(WorkUsage::default()),
                ))
                .await;
        }
        self.record_activity(zephium_ipc::work::WorkActivityV1::Reading);
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
                result = provider.search(scope, self.specification().limits) => result,
            }
        };
        let result = match result {
            Ok(result)
                if result.usage.within(self.specification().limits)
                    && result.evidence.validate().is_ok()
                    && result.evidence.provider == scope.provider
                    && result.evidence.model == scope.model
                    && u64::from(result.usage.model_tokens)
                        == u64::from(result.evidence.actual_input_tokens)
                            + u64::from(result.evidence.actual_output_tokens) =>
            {
                let id = WorkArtifactId::generate();
                let links: Vec<_> = (1..=result.evidence.citations.len())
                    .map(|index| WorkEvidenceLink {
                        extraction_id: id,
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
                            summary: result.evidence.answer.clone(),
                            subjects: vec![],
                            entries: vec![],
                        },
                        evidence: links.clone(),
                    })
                    .collect();
                let evidence = WorkProviderSearchRecordV1 {
                    id,
                    node: self.node().id,
                    attempt: self.attempt(),
                    evidence: result.evidence,
                };
                return self
                    .settle_with_provider_evidence(
                        WorkAdapterResult {
                            status: WorkAttemptStatus::Succeeded,
                            usage: Some(result.usage),
                            artifacts,
                        },
                        Some(evidence),
                    )
                    .await;
            }
            Err(WorkPublicSearchError::NotDispatched(_)) => {
                empty(WorkAttemptStatus::Failed, Some(WorkUsage::default()))
            }
            Err(WorkPublicSearchError::Rejected(usage))
                if usage.within(self.specification().limits) =>
            {
                empty(WorkAttemptStatus::Failed, Some(usage))
            }
            _ => empty(WorkAttemptStatus::OutcomeUnknown, None),
        };
        self.settle_owned(result).await
    }
}

fn empty(status: WorkAttemptStatus, usage: Option<WorkUsage>) -> WorkAdapterResult {
    WorkAdapterResult {
        status,
        usage,
        artifacts: vec![],
    }
}
