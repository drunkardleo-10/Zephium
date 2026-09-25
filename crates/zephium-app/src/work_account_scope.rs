//! Approval drafts for one signed-in page. Rust resolves the chosen tab, mints
//! the account identity, and returns the exact specification the user
//! approves. Approval is the user's attestation of the account; Zephium has no
//! independent account collector and never claims one.
use std::time::Duration;
use zephium_core::{
    ids::ProfileId,
    work::{environment::*, port::*, proposal::*, runtime::*, *},
};
use zephium_ipc::work::{
    WorkAccountApprovalRequestV1, WorkAccountEffectV1, WorkReplyV1, WorkResponseV1,
};

const READ_TIMEOUT: Duration = Duration::from_secs(8);
pub const ACCOUNT_LIMITS: WorkExecutionLimits = WorkExecutionLimits {
    model_tokens: 128_000,
    cost_micro_usd: 500_000,
    operations: 64,
    timeout_seconds: 600,
    max_workers: 1,
};

pub struct WorkAccountApproval {
    handle: crate::Handle,
}
impl WorkAccountApproval {
    pub fn new(handle: crate::Handle) -> Self {
        Self { handle }
    }

    /// Mints a single-step plan from the objective when needed, then drafts a
    /// specification naming profile, page, origin, account, and effect.
    pub async fn prepare(
        &self,
        profile: ProfileId,
        request: WorkAccountApprovalRequestV1,
    ) -> Result<WorkResponseV1, WorkError> {
        if request.version != 1 {
            return Err(WorkError::Invalid);
        }
        let state = self.projection(profile, request.work).await?;
        if state.work.revision != request.expected_revision
            || state.work.lifecycle != WorkLifecycle::Active
            || state.work.status == WorkAuthoringStatus::NeedsInput
            || state.executions.iter().any(|e| !e.status.terminal())
        {
            return Err(WorkError::Conflict);
        }
        let tab = self
            .attached_tab(profile, request.environment, request.element)
            .await?;
        let url = tab.url.ok_or(WorkError::Invalid)?;
        let origin = url::Url::parse(&url)
            .map_err(|_| WorkError::Invalid)?
            .origin()
            .ascii_serialization();
        let scope = WorkAccountScope {
            tab: tab.id,
            url,
            origin,
            account: serde_json::to_value(zephium_agentic::AgentAccountId::generate())
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .ok_or(WorkError::Unavailable)?,
        };
        let capability = match request.effect {
            WorkAccountEffectV1::Read => WorkCapability::AccountRead { scope },
            WorkAccountEffectV1::Update { update } => {
                WorkCapability::AccountUpdate { scope, update }
            }
        };
        capability.validate()?;
        let (plan, revision) = match state.work.plan.as_ref() {
            Some(plan)
                if state.work.status == WorkAuthoringStatus::PlanReady
                    && plan.draft.nodes.len() == 1 =>
            {
                (plan.clone(), state.work.revision)
            }
            _ => self.mint_plan(profile, &state.work).await?,
        };
        let mut spec = WorkExecutionSpec::account_scoped(&plan, ACCOUNT_LIMITS, capability)?;
        spec.context = plan.context.clone();
        Ok(WorkResponseV1 {
            version: 1,
            profile: profile.to_string(),
            reply: WorkReplyV1::ApprovalDraft {
                work: request.work,
                expected_revision: revision,
                spec,
            },
        })
    }

    async fn mint_plan(
        &self,
        profile: ProfileId,
        work: &WorkSnapshot,
    ) -> Result<(WorkPlanRevision, WorkRevision), WorkError> {
        let draft = WorkPlanProposal {
            nodes: vec![WorkNodeProposal {
                key: 0,
                objective: work.objective.clone(),
                dependencies: vec![],
                outputs: vec![WorkExpectedOutput {
                    name: "Findings".into(),
                    description: "What the signed-in page shows, with sources".into(),
                    review: WorkOutputReview::SourceMappedNeedsReview,
                }],
            }],
        }
        .mint()?;
        let request = self.handle.submit_work_document(
            WorkRequest::Edit {
                id: work.id,
                expected: work.revision,
                edit: WorkEdit::ReplaceDraft { draft },
                author: WorkAuthor::User,
            },
            Some(profile),
        )?;
        let projection = tokio::time::timeout(READ_TIMEOUT, request)
            .await
            .map_err(|_| WorkError::OutcomeUnknown)??;
        let WorkReply::Snapshot(snapshot) = projection.reply else {
            return Err(WorkError::Invalid);
        };
        if projection.profile != profile || snapshot.id != work.id {
            return Err(WorkError::ProfileUnavailable);
        }
        let plan = snapshot.plan.clone().ok_or(WorkError::Invalid)?;
        Ok((plan, snapshot.revision))
    }

    async fn projection(
        &self,
        profile: ProfileId,
        work: WorkId,
    ) -> Result<WorkRuntimeProjection, WorkError> {
        let response =
            tokio::time::timeout(READ_TIMEOUT, self.handle.work_projection(profile, work)?)
                .await
                .map_err(|_| WorkError::Unavailable)??;
        let WorkReply::Runtime(state) = response.reply else {
            return Err(WorkError::Invalid);
        };
        if response.profile != profile || state.work.profile != profile {
            return Err(WorkError::ProfileUnavailable);
        }
        Ok(*state)
    }

    /// The element must be a browser attachment of this environment; the tab
    /// must belong to the profile and currently show an HTTPS page.
    async fn attached_tab(
        &self,
        profile: ProfileId,
        environment: WorkEnvironmentId,
        element: WorkElementId,
    ) -> Result<crate::TabMetadata, WorkError> {
        let request = self.handle.submit_work_document(
            WorkRequest::Environment {
                call: WorkEnvironmentCall::Read { id: environment },
                space_available: false,
                browser_available: false,
                note_available: false,
            },
            Some(profile),
        )?;
        let projection = tokio::time::timeout(READ_TIMEOUT, request)
            .await
            .map_err(|_| WorkError::Unavailable)??;
        let WorkReply::Environment(WorkEnvironmentReply::Snapshot { snapshot }) = projection.reply
        else {
            return Err(WorkError::NotFound);
        };
        if projection.profile != profile || snapshot.profile != profile {
            return Err(WorkError::ProfileUnavailable);
        }
        let tab = snapshot
            .elements
            .iter()
            .find(|entry| entry.id == element)
            .and_then(|entry| match entry.reference {
                WorkEnvironmentReference::Browser { tab } => Some(tab),
                _ => None,
            })
            .ok_or(WorkError::NotFound)?;
        let receiver = self.handle.tab_metadata(profile, vec![tab]);
        let mut tabs = tokio::task::spawn_blocking(move || receiver.recv_timeout(READ_TIMEOUT))
            .await
            .map_err(|_| WorkError::Unavailable)?
            .map_err(|_| WorkError::Unavailable)?;
        tabs.pop()
            .filter(|entry| entry.id == tab)
            .ok_or(WorkError::NotFound)
    }
}
