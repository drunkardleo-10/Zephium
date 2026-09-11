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
}
/// Provider usage remains visible even if the final CAS conflicts or its
/// acknowledgement is lost. Reconcile unknown persistence with a fresh read.
pub struct WorkPlanningCompletion {
    pub usage: WorkPlanningUsage,
    pub persistence: Result<crate::WorkDocumentProjection, WorkError>,
}
impl WorkPlanningService {
    pub async fn plan_request(
        &self,
        profile: ProfileId,
        request: zephium_ipc::work::WorkPlanRequestV1,
    ) -> zephium_ipc::work::WorkPlanningResponseV1 {
        use zephium_ipc::work::*;
        let result = if request.version == 1 {
            self.plan(profile, request.work, request.expected_revision)
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
                let reason = match error {
                    WorkPlanningError::Invalid => WorkPlanningFailureV1::Invalid,
                    WorkPlanningError::Capacity => WorkPlanningFailureV1::Capacity,
                    WorkPlanningError::Unavailable => WorkPlanningFailureV1::Unavailable,
                    WorkPlanningError::Cancelled => WorkPlanningFailureV1::Cancelled,
                    WorkPlanningError::Timeout => WorkPlanningFailureV1::Timeout,
                    WorkPlanningError::Stale => WorkPlanningFailureV1::Stale,
                    WorkPlanningError::NeedsInput => WorkPlanningFailureV1::NeedsInput,
                    WorkPlanningError::Privacy => WorkPlanningFailureV1::Privacy,
                    WorkPlanningError::ProviderOutcomeUnknown => {
                        WorkPlanningFailureV1::ProviderOutcomeUnknown
                    }
                    WorkPlanningError::ProviderRefused(_) => WorkPlanningFailureV1::ProviderRefused,
                    WorkPlanningError::Store(error) => WorkPlanningFailureV1::Store {
                        error: error.into(),
                    },
                };
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
        Self { handle, provider }
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
        let disclosure = WorkPlanningDisclosure::from_snapshot(&snapshot)?;
        let result =
            tokio::time::timeout(Duration::from_secs(180), self.provider.propose(disclosure))
                .await
                .map_err(|_| WorkPlanningError::ProviderOutcomeUnknown)??;
        let edit = result.proposal.into_edit()?;
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
