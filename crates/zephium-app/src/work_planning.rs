//! One on-demand planning call, followed by one profile-bound Store CAS.
//! There is no approval, execution capability, polling worker, or automatic retry.
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zephium_core::{
    ids::ProfileId,
    work::{planning::*, port::*, *},
};

static ACTIVE: Mutex<Vec<(ProfileId, WorkId)>> = Mutex::new(Vec::new());
struct Permit((ProfileId, WorkId));
impl Permit {
    fn acquire(key: (ProfileId, WorkId)) -> Result<Self, WorkPlanningError> {
        let mut active = ACTIVE.lock().map_err(|_| WorkPlanningError::Unavailable)?;
        if active.len() >= 2 || active.contains(&key) {
            return Err(WorkPlanningError::Capacity);
        }
        active.push(key);
        Ok(Self(key))
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        if let Ok(mut active) = ACTIVE.lock() {
            active.retain(|key| *key != self.0);
        }
    }
}

/// Provider routing/credentials are supplied by trusted Rust composition.
/// Constructing this service starts no tasks, timers, sockets, or Store work.
pub struct WorkPlanningService {
    handle: crate::Handle,
    provider: Arc<dyn WorkPlanningProvider>,
    execution_diagnostic: Option<fn(execution_proposal::WorkExecutionProposalRefusal)>,
}
/// Provider usage remains visible even if the final CAS conflicts or its
/// acknowledgement is lost. Reconcile unknown persistence with a fresh read.
pub struct WorkPlanningCompletion {
    pub usage: WorkPlanningUsage,
    pub persistence: Result<crate::WorkDocumentProjection, WorkError>,
}
impl WorkPlanningService {
    /// Optional closed structural diagnostic; contains no provider/user text.
    pub fn with_execution_diagnostic(
        mut self,
        diagnostic: fn(execution_proposal::WorkExecutionProposalRefusal),
    ) -> Self {
        self.execution_diagnostic = Some(diagnostic);
        self
    }

    pub async fn prepare_request(
        &self,
        profile: ProfileId,
        request: zephium_ipc::work::WorkPlanRequestV1,
    ) -> zephium_ipc::work::WorkPlanningResponseV1 {
        use zephium_ipc::work::*;
        let result = self.prepare_execution(profile, &request).await;
        let usage_view = |usage: WorkPlanningUsage| WorkPlanningUsageV1 {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cost_ceiling_micro_usd: usage.cost_ceiling_micro_usd.to_string(),
        };
        let (usage, outcome) = match result {
            Ok((usage, spec)) => (
                Some(usage_view(usage)),
                WorkPlanningOutcomeV1::Settled {
                    response: WorkResponseV1 {
                        version: 1,
                        profile: profile.to_string(),
                        reply: match spec {
                            Ok(spec) => WorkReplyV1::ApprovalDraft {
                                work: request.work,
                                expected_revision: request.expected_revision,
                                spec,
                            },
                            Err(error) => WorkReplyV1::Error {
                                error: error.into(),
                            },
                        },
                    },
                },
            ),
            Err(error) => (
                if let WorkPlanningError::ProviderRefused(usage) = error {
                    Some(usage_view(usage))
                } else {
                    None
                },
                WorkPlanningOutcomeV1::Refused {
                    reason: error.into(),
                },
            ),
        };
        WorkPlanningResponseV1 {
            version: 1,
            profile: profile.to_string(),
            work: request.work,
            basis_revision: request.expected_revision,
            usage,
            outcome,
        }
    }
    async fn prepare_execution(
        &self,
        profile: ProfileId,
        request: &zephium_ipc::work::WorkPlanRequestV1,
    ) -> Result<
        (
            WorkPlanningUsage,
            Result<runtime::WorkExecutionSpec, WorkError>,
        ),
        WorkPlanningError,
    > {
        if request.version != 1 {
            return Err(WorkPlanningError::Invalid);
        }
        let _permit = Permit::acquire((profile, request.work))?;
        let state = self
            .approval_basis(profile, request)
            .await
            .map_err(WorkPlanningError::Store)?;
        let admitted = self
            .admit_context(profile, request.context.as_ref())
            .await?;
        let disclosure =
            WorkPlanningDisclosure::from_snapshot_with_context(&state.work, admitted.as_ref())?;
        let manifest = disclosure.admitted().cloned();
        let result = tokio::time::timeout(
            Duration::from_secs(180),
            self.provider.propose_execution(disclosure),
        )
        .await
        .map_err(|_| WorkPlanningError::ProviderOutcomeUnknown)??;
        // Model completion does not approve anything. Recheck the exact current
        // plan/profile before exposing a draft; the eventual command has its own CAS.
        let spec = match self.approval_basis(profile, request).await {
            Ok(state) => state
                .work
                .plan
                .as_ref()
                .ok_or(WorkError::Invalid)
                .and_then(|plan| {
                    let limits = runtime::WorkExecutionLimits {
                        model_tokens: 256_000,
                        cost_micro_usd: 1_000_000,
                        operations: 256,
                        timeout_seconds: 900,
                        max_workers: 4,
                    };
                    result
                        .proposal
                        .proposed_limits(limits)
                        .and_then(|limits| result.proposal.compile_diagnosed(plan, limits))
                        .map(|mut spec| {
                            spec.context = manifest.clone().or_else(|| plan.context.clone());
                            spec
                        })
                        .map_err(|refusal| {
                            if let Some(diagnostic) = self.execution_diagnostic {
                                diagnostic(refusal);
                            }
                            refusal.work_error()
                        })
                }),
            Err(error) => Err(error),
        };
        Ok((result.usage, spec))
    }
    async fn approval_basis(
        &self,
        profile: ProfileId,
        request: &zephium_ipc::work::WorkPlanRequestV1,
    ) -> Result<runtime::WorkRuntimeProjection, WorkError> {
        let response = tokio::time::timeout(
            Duration::from_secs(10),
            self.handle.work_projection(profile, request.work)?,
        )
        .await
        .map_err(|_| WorkError::Unavailable)??;
        let WorkReply::Runtime(state) = response.reply else {
            return Err(WorkError::Invalid);
        };
        if response.profile != profile
            || state.work.revision != request.expected_revision
            || state.work.lifecycle != WorkLifecycle::Active
            || state.work.status != WorkAuthoringStatus::PlanReady
            || state
                .executions
                .iter()
                .any(|execution| !execution.status.terminal())
        {
            return Err(WorkError::Conflict);
        }
        Ok(*state)
    }
    pub async fn plan_request(
        &self,
        profile: ProfileId,
        request: zephium_ipc::work::WorkPlanRequestV1,
    ) -> zephium_ipc::work::WorkPlanningResponseV1 {
        use zephium_ipc::work::*;
        let result = if request.version == 1 {
            self.plan(
                profile,
                request.work,
                request.expected_revision,
                request.context.as_ref(),
            )
            .await
        } else {
            Err(WorkPlanningError::Invalid)
        };
        let usage_view = |usage: WorkPlanningUsage| WorkPlanningUsageV1 {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cost_ceiling_micro_usd: usage.cost_ceiling_micro_usd.to_string(),
        };
        let (usage, outcome) = match result {
            Ok(completion) => {
                let persistence = completion.persistence.and_then(|p| {
                    if p.profile == profile {
                        Ok(p.reply)
                    } else {
                        Err(WorkError::ProfileUnavailable)
                    }
                });
                (
                    Some(usage_view(completion.usage)),
                    WorkPlanningOutcomeV1::Settled {
                        response: WorkResponseV1::from_result(profile, persistence),
                    },
                )
            }
            Err(error) => {
                let usage = if let WorkPlanningError::ProviderRefused(usage) = error {
                    Some(usage_view(usage))
                } else {
                    None
                };
                let reason = error.into();
                (usage, WorkPlanningOutcomeV1::Refused { reason })
            }
        };
        WorkPlanningResponseV1 {
            version: 1,
            profile: profile.to_string(),
            work: request.work,
            basis_revision: request.expected_revision,
            usage,
            outcome,
        }
    }
    pub fn new(handle: crate::Handle, provider: Arc<dyn WorkPlanningProvider>) -> Self {
        Self {
            handle,
            provider,
            execution_diagnostic: None,
        }
    }
    async fn admit_context(
        &self,
        profile: ProfileId,
        selection: Option<&context::WorkContextSelectionV1>,
    ) -> Result<Option<context::WorkAdmittedContext>, WorkPlanningError> {
        let Some(selection) = selection else {
            return Ok(None);
        };
        crate::work_context::WorkContextAdmission::new(self.handle.clone())
            .admit(profile, context::WorkContextPurpose::Planning, selection)
            .await
            .map(Some)
            .map_err(|error| match error {
                WorkError::Conflict => WorkPlanningError::Stale,
                WorkError::Capacity => WorkPlanningError::Capacity,
                error => WorkPlanningError::Store(error),
            })
    }
    /// Caller identifies the displayed owner and revision; Shell verifies both
    /// owner selections and Store performs the final revision CAS. Dropping this
    /// future cancels local provider I/O. Once the final edit is queued, it may
    /// commit even if the caller disappears; cancellation never rolls it back.
    pub async fn plan(
        &self,
        profile: ProfileId,
        id: WorkId,
        expected: WorkRevision,
        context: Option<&context::WorkContextSelectionV1>,
    ) -> Result<WorkPlanningCompletion, WorkPlanningError> {
        let _permit = Permit::acquire((profile, id))?;
        let read = self
            .handle
            .submit_work_document(WorkRequest::Read { id }, Some(profile))
            .map_err(WorkPlanningError::Store)?;
        let projection = tokio::time::timeout(Duration::from_secs(10), read)
            .await
            .map_err(|_| WorkPlanningError::Timeout)?
            .map_err(WorkPlanningError::Store)?;
        let WorkReply::Snapshot(snapshot) = projection.reply else {
            return Err(WorkPlanningError::Invalid);
        };
        if projection.profile != profile
            || snapshot.profile != profile
            || snapshot.id != id
            || snapshot.revision != expected
        {
            return Err(WorkPlanningError::Stale);
        }
        let admitted = self.admit_context(profile, context).await?;
        let disclosure =
            WorkPlanningDisclosure::from_snapshot_with_context(&snapshot, admitted.as_ref())?;
        let manifest = disclosure.admitted().cloned();
        let result =
            tokio::time::timeout(Duration::from_secs(180), self.provider.propose(disclosure))
                .await
                .map_err(|_| WorkPlanningError::ProviderOutcomeUnknown)??;
        let edit = result.proposal.into_edit_disclosed(manifest)?;
        let request = WorkRequest::Edit {
            id,
            expected,
            edit,
            author: WorkAuthor::PrimaryAgent,
        };
        let persistence = match self.handle.submit_work_document(request, Some(profile)) {
            Ok(receipt) => tokio::time::timeout(Duration::from_secs(10), receipt)
                .await
                .unwrap_or(Err(WorkError::OutcomeUnknown)),
            Err(error) => Err(error),
        };
        Ok(WorkPlanningCompletion {
            usage: result.usage,
            persistence,
        })
    }
}
