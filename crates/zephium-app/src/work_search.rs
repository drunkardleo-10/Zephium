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
    /// Why a decoded answer was not admitted; closed words for the step.
    pub(crate) note: Option<&'static str>,
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
                note: None,
                record: None,
            });
        }
        self.record_activity(zephium_ipc::work::WorkActivityV1::Searching);
        let result = self
            .bounded_search(provider.search(scope, context, limits))
            .await;
        // A response that arrived is a known outcome: what it lacks is a
        // failed step with a reason. Only a lost outcome stays unknown.
        let failed = |usage: WorkUsage, note: &'static str| WorkSearchOutcome {
            status: WorkAttemptStatus::Failed,
            usage: Some(clamp(usage, limits)),
            note: Some(note),
            record: None,
        };
        Ok(match result {
            Ok(result) => {
                let refused = if !result.usage.within(limits) {
                    Some("The search exceeded the step budget")
                } else if result.evidence.validate().is_err() {
                    Some("The search results could not be admitted")
                } else if result.evidence.provider != scope.provider
                    || result.evidence.model != scope.model
                {
                    Some("The search came from another provider")
                } else if u64::from(result.usage.model_tokens)
                    != u64::from(result.evidence.actual_input_tokens)
                        + u64::from(result.evidence.actual_output_tokens)
                {
                    Some("The search usage did not reconcile")
                } else {
                    None
                };
                match refused {
                    None => {
                        let ranked = self
                            .rank_search(provider, scope, &result.evidence, result.usage, limits)
                            .await?;
                        match ranked {
                            Some((usage, ranking)) => WorkSearchOutcome {
                                status: WorkAttemptStatus::Succeeded,
                                usage: Some(usage),
                                note: None,
                                record: Some(WorkProviderSearchRecordV1 {
                                    id: WorkArtifactId::generate(),
                                    node: self.node(),
                                    attempt: self.attempt(),
                                    evidence: result.evidence,
                                    ranking,
                                }),
                            },
                            None => WorkSearchOutcome {
                                status: WorkAttemptStatus::OutcomeUnknown,
                                usage: None,
                                note: None,
                                record: None,
                            },
                        }
                    }
                    Some(note) => failed(result.usage, note),
                }
            }
            Err(WorkPublicSearchError::NotDispatched(cause)) => {
                crate::work_trace::record(format_args!(
                    "work: phase=search refused=not_sent cause={cause:?}"
                ));
                failed(WorkUsage::default(), "The search could not be sent")
            }
            Err(WorkPublicSearchError::Rejected(usage)) => {
                failed(usage, "The search gave no usable sources")
            }
            Err(WorkPublicSearchError::OutcomeUnknown) => WorkSearchOutcome {
                status: WorkAttemptStatus::OutcomeUnknown,
                usage: None,
                note: None,
                record: None,
            },
        })
    }

    async fn bounded_search<T>(
        &self,
        future: impl std::future::Future<Output = Result<T, WorkPublicSearchError>>,
    ) -> Result<T, WorkPublicSearchError> {
        // Each look reads the run's whole projection: once a second keeps a
        // run's parallel searches from crowding the store queue.
        let cancelled = async {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if self.cancellation_requested().await.unwrap_or(false) {
                    break;
                }
            }
        };
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(self.deadline().into()) => Err(WorkPublicSearchError::OutcomeUnknown),
            _ = cancelled => Err(WorkPublicSearchError::OutcomeUnknown),
            result = future => result,
        }
    }

    async fn rank_search(
        &self,
        provider: &dyn WorkPublicSearchProvider,
        scope: &WorkPublicSearchScope,
        evidence: &WorkProviderSearchEvidenceV1,
        used: WorkUsage,
        limits: WorkExecutionLimits,
    ) -> Result<Option<(WorkUsage, Option<WorkPublicSearchRanking>)>, WorkError> {
        let remaining = WorkExecutionLimits {
            model_tokens: limits.model_tokens - used.model_tokens,
            cost_micro_usd: limits.cost_micro_usd - used.cost_micro_usd,
            operations: limits.operations - used.operations,
            ..limits
        };
        if evidence.citations.len() < 2
            || remaining.validate().is_err()
            || Instant::now() >= self.deadline()
            || self.cancellation_requested().await?
        {
            return Ok(Some((used, None)));
        }
        let result = self
            .bounded_search(provider.rerank(scope, evidence, remaining, self.deadline()))
            .await;
        let (extra, preferred) = match result {
            Ok(ranking) => {
                let preferred = if ranking.validate(evidence).is_ok() {
                    ranking.preferred
                } else {
                    vec![]
                };
                (ranking.usage, preferred)
            }
            Err(WorkPublicSearchError::NotDispatched(_)) => return Ok(Some((used, None))),
            Err(WorkPublicSearchError::Rejected(usage)) => (usage, vec![]),
            Err(WorkPublicSearchError::OutcomeUnknown) => return Ok(None),
        };
        if !extra.within(remaining) || (extra != WorkUsage::default() && extra.operations == 0) {
            return Ok(None);
        }
        Ok(Some((
            WorkUsage {
                model_tokens: used.model_tokens + extra.model_tokens,
                cost_micro_usd: used.cost_micro_usd + extra.cost_micro_usd,
                operations: used.operations + extra.operations,
                accounting: if used.accounting == WorkUsageAccounting::ConservativeReservation
                    || extra.accounting == WorkUsageAccounting::ConservativeReservation
                {
                    WorkUsageAccounting::ConservativeReservation
                } else {
                    WorkUsageAccounting::Exact
                },
            },
            (extra != WorkUsage::default()).then_some(WorkPublicSearchRanking {
                preferred,
                usage: extra,
            }),
        )))
    }
}

/// A charge never exceeds what the step was allowed to spend.
fn clamp(usage: WorkUsage, limits: WorkExecutionLimits) -> WorkUsage {
    WorkUsage {
        model_tokens: usage.model_tokens.min(limits.model_tokens),
        cost_micro_usd: usage.cost_micro_usd.min(limits.cost_micro_usd),
        operations: usage.operations.min(limits.operations),
        accounting: usage.accounting,
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
