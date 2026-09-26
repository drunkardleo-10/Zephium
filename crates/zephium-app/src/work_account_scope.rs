//! Approval drafts for one signed-in page, or for signed-in reads on one
//! origin during the next request. Rust resolves the chosen tab, mints the
//! account identity, and returns the exact draft the user approves. Approval
//! is the user's attestation of the account; Zephium has no independent
//! account collector and never claims one.
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use zephium_core::{
    ids::ProfileId,
    work::{environment::*, port::*, proposal::*, runtime::*, *},
};
use zephium_ipc::work::{
    WorkAccountApprovalRequestV1, WorkAccountEffectV1, WorkAccountModeV1, WorkReplyV1,
    WorkResponseV1,
};

const READ_TIMEOUT: Duration = Duration::from_secs(8);
/// How long a drafted origin grant waits for the request that uses it.
const DRAFT_PATIENCE: Duration = Duration::from_secs(30 * 60);
const MAX_DRAFTED: usize = 16;

/// Origin grants Rust drafted for the person's approval. A request claims
/// each at most once; a later request needs a new approval.
static DRAFTED: Mutex<Vec<Drafted>> = Mutex::new(Vec::new());
struct Drafted {
    profile: ProfileId,
    work: WorkId,
    grant: WorkAccountGrantV1,
    at: Instant,
}
pub(crate) fn draft(
    profile: ProfileId,
    work: WorkId,
    grant: WorkAccountGrantV1,
) -> Result<(), WorkError> {
    let mut drafted = DRAFTED.lock().map_err(|_| WorkError::Unavailable)?;
    drafted.retain(|entry| {
        entry.at.elapsed() < DRAFT_PATIENCE
            && !(entry.profile == profile
                && entry.work == work
                && entry.grant.origin == grant.origin)
    });
    if drafted.len() >= MAX_DRAFTED {
        drafted.remove(0);
    }
    drafted.push(Drafted {
        profile,
        work,
        grant,
        at: Instant::now(),
    });
    Ok(())
}
/// Consumes the drafts a request's grant names, all or none. An account Rust
/// did not draft for this work, or one already used, is refused.
pub fn claim_account_grants(
    profile: ProfileId,
    work: WorkId,
    grants: &[WorkAccountGrantV1],
) -> Result<(), WorkError> {
    if grants.is_empty() {
        return Ok(());
    }
    let mut drafted = DRAFTED.lock().map_err(|_| WorkError::Unavailable)?;
    drafted.retain(|entry| entry.at.elapsed() < DRAFT_PATIENCE);
    let found: Option<Vec<usize>> = grants
        .iter()
        .map(|grant| {
            drafted.iter().position(|entry| {
                entry.profile == profile && entry.work == work && entry.grant == *grant
            })
        })
        .collect();
    let mut found = found.ok_or(WorkError::ReviewRequired)?;
    found.sort_unstable();
    found.dedup();
    if found.len() != grants.len() {
        return Err(WorkError::ReviewRequired);
    }
    for index in found.into_iter().rev() {
        drafted.remove(index);
    }
    Ok(())
}
/// The probe's stand-in for the person approving a drafted origin grant.
#[cfg(feature = "work-execution-probe")]
#[doc(hidden)]
pub fn record_approved_grant_for_probe(
    profile: ProfileId,
    work: WorkId,
    grant: WorkAccountGrantV1,
) -> Result<(), WorkError> {
    grant.validate()?;
    draft(profile, work, grant)
}
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
        let account = serde_json::to_value(zephium_agentic::AgentAccountId::generate())
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or(WorkError::Unavailable)?;
        if request.mode == WorkAccountModeV1::Origin {
            if request.effect != WorkAccountEffectV1::Read {
                return Err(WorkError::Invalid);
            }
            let grant = WorkAccountGrantV1 {
                origin,
                account,
                tab: Some(tab.id),
                pages: request.pages.unwrap_or(MAX_WORK_ACCOUNT_PAGES),
            };
            grant.validate()?;
            draft(profile, request.work, grant.clone())?;
            return Ok(WorkResponseV1 {
                version: 1,
                profile: profile.to_string(),
                reply: WorkReplyV1::AccountGrantDraft {
                    work: request.work,
                    grant,
                },
            });
        }
        if request.pages.is_some() {
            return Err(WorkError::Invalid);
        }
        let scope = WorkAccountScope {
            tab: tab.id,
            url,
            origin,
            account,
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
