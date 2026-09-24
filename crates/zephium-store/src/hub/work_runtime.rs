//! Same actor, same profile transaction, same Work revision as authoring.
use super::*;
use sha2::{Digest, Sha256};
use zephium_core::work::{artifact::WorkArtifactV1, runtime::*};

const MAX_EXECUTION_BYTES: usize = 524288;
const MAX_RUNTIME_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(super) struct RuntimeClock {
    pub(super) session: WorkRuntimeSessionId,
    pub(super) tick_ms: i64,
}

struct Row {
    fact: WorkExecutionFact,
    owner: WorkRuntimeSessionId,
    approved_tick: i64,
    expires_tick: i64,
}
fn descendant(
    spec: &WorkExecutionSpec,
    mut node: WorkPlanNodeId,
    ancestor: WorkPlanNodeId,
) -> bool {
    for _ in 0..spec.nodes.len() {
        let Some(parent) = spec
            .nodes
            .iter()
            .find(|entry| entry.node == node)
            .and_then(|entry| entry.parent)
        else {
            return false;
        };
        if parent == ancestor {
            return true;
        }
        node = parent;
    }
    false
}
fn rows(conn: &Connection, id: WorkId) -> Result<Vec<Row>, WorkError> {
    let mut statement = conn.prepare("SELECT execution_id, plan_revision, owner_session, approved_unix_ms, expires_unix_ms, CASE WHEN length(CAST(body AS BLOB)) <= 524288 THEN body END, approved_tick_ms, expires_tick_ms FROM work_executions WHERE work_id = ?1 ORDER BY execution_id LIMIT 17").map_err(db)?;
    let rows = statement
        .query_map([id.to_string()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
            ))
        })
        .map_err(db)?;
    let mut result = Vec::new();
    let mut bytes = 0;
    for row in rows {
        let (key, plan, owner, approved_ms, expires_ms, body, approved_tick, expires_tick) =
            row.map_err(db)?;
        bytes += body.len();
        if result.len() >= MAX_WORK_EXECUTIONS || bytes > MAX_RUNTIME_BYTES {
            return Err(WorkError::Capacity);
        }
        crate::bounded_json::preflight_work_execution(&body).map_err(|_| WorkError::Invalid)?;
        let fact: WorkExecutionFact =
            serde_json::from_str(&body).map_err(|_| WorkError::Invalid)?;
        if fact.id.to_string() != key
            || fact.spec.plan_revision != revision(plan)?
            || approved_ms < 0
            || expires_ms <= approved_ms
            || approved_tick < 0
            || expires_tick.checked_sub(approved_tick)
                != Some(i64::from(fact.spec.limits.timeout_seconds) * 1000)
            || expires_ms.checked_sub(approved_ms)
                != Some(i64::from(fact.spec.limits.timeout_seconds) * 1000)
        {
            return Err(WorkError::Invalid);
        }
        result.push(Row {
            fact,
            owner: WorkRuntimeSessionId::parse(&owner).ok_or(WorkError::Invalid)?,
            approved_tick,
            expires_tick,
        });
    }
    Ok(result)
}
/// Joins the original result without inventing an observation session or worker.
pub(super) fn validate_artifact_reference(
    conn: &Connection,
    profile: ProfileId,
    objective: WorkId,
    execution: WorkExecutionId,
    artifact: WorkArtifactId,
) -> Result<(), WorkError> {
    read_artifact(conn, profile, objective, execution, artifact).map(|_| ())
}
pub(super) fn read_artifact(
    conn: &Connection,
    profile: ProfileId,
    objective: WorkId,
    execution: WorkExecutionId,
    artifact: WorkArtifactId,
) -> Result<WorkArtifactV1, WorkError> {
    read_artifact_fact(conn, profile, objective, execution, artifact).map(|(_, value)| value)
}
pub(crate) fn read_artifact_fact(
    conn: &Connection,
    profile: ProfileId,
    objective: WorkId,
    execution: WorkExecutionId,
    artifact: WorkArtifactId,
) -> Result<(WorkExecutionFact, WorkArtifactV1), WorkError> {
    let work = read(conn, profile, objective)?;
    let row = rows(conn, objective)?
        .into_iter()
        .find(|row| row.fact.id == execution)
        .ok_or(WorkError::NotFound)?;
    row.fact.validate(
        &read_plan(conn, objective, row.fact.spec.plan_revision)?,
        work.revision,
    )?;
    let value = row
        .fact
        .artifacts
        .iter()
        .find(|value| value.id == artifact && value.execution == execution)
        .cloned()
        .ok_or(WorkError::NotFound)?;
    Ok((row.fact, value))
}

pub(super) fn projection(
    conn: &Connection,
    profile: ProfileId,
    id: WorkId,
    session: RuntimeClock,
) -> Result<WorkRuntimeProjection, WorkError> {
    let work = read(conn, profile, id)?;
    let mut executions = Vec::new();
    let mut interrupted = Vec::new();
    let mut owners = Vec::new();
    for row in rows(conn, id)? {
        row.fact.validate(
            &read_plan(conn, id, row.fact.spec.plan_revision)?,
            work.revision,
        )?;
        if !row.fact.status.terminal() && row.owner != session.session {
            interrupted.push(row.fact.id);
        }
        owners.push(WorkExecutionOwnership {
            execution: row.fact.id,
            owner: row.owner,
        });
        executions.push(row.fact);
    }
    Ok(WorkRuntimeProjection {
        version: 1,
        work,
        executions,
        interrupted,
        owners,
    })
}
pub(super) fn require_idle(
    conn: &Connection,
    id: WorkId,
    revision: WorkRevision,
) -> Result<(), WorkError> {
    for row in rows(conn, id)? {
        row.fact
            .validate(&read_plan(conn, id, row.fact.spec.plan_revision)?, revision)?;
        if !row.fact.status.terminal() {
            return Err(WorkError::Conflict);
        }
    }
    Ok(())
}
fn bump(
    tx: &Transaction<'_>,
    id: WorkId,
    expected: WorkRevision,
    author: WorkAuthor,
) -> Result<WorkRevision, WorkError> {
    let next = expected.next()?;
    if tx.execute("UPDATE works SET revision = ?3, updated_unix_ms = max(updated_unix_ms, ?4) WHERE id = ?1 AND revision = ?2", params![id.to_string(), expected.get() as i64, next.get() as i64, timestamp()?]).map_err(db)? != 1 {
        return Err(WorkError::Conflict);
    }
    append_event(tx, id, next, WorkEventKind::RuntimeChanged, author)?;
    Ok(next)
}
fn body(fact: &WorkExecutionFact) -> Result<String, WorkError> {
    let body = serde_json::to_string(fact).map_err(|_| WorkError::Invalid)?;
    if body.len() > MAX_EXECUTION_BYTES {
        return Err(WorkError::Capacity);
    }
    Ok(body)
}
fn write(tx: &Transaction<'_>, id: WorkId, fact: &WorkExecutionFact) -> Result<(), WorkError> {
    if tx
        .execute(
            "UPDATE work_executions SET body = ?3 WHERE work_id = ?1 AND execution_id = ?2",
            params![id.to_string(), fact.id.to_string(), body(fact)?],
        )
        .map_err(db)?
        != 1
    {
        return Err(WorkError::Conflict);
    }
    Ok(())
}

/// A runtime intent with the Rust-admitted context manifest it may carry.
pub(super) struct RuntimeIntentInput {
    pub(super) intent: WorkRuntimeIntent,
    pub(super) context: Option<zephium_core::work::context::WorkContextDisclosureV1>,
}

pub(super) fn command(
    tx: &Transaction<'_>,
    profile: ProfileId,
    session: RuntimeClock,
    id: WorkId,
    expected: WorkRevision,
    command: WorkCommandId,
    input: RuntimeIntentInput,
) -> Result<(WorkReply, bool), WorkError> {
    let RuntimeIntentInput { intent, context } = input;
    let mut current = read(tx, profile, id)?;
    let direct = matches!(intent, WorkRuntimeIntent::ReadPublic { .. });
    let agent = matches!(intent, WorkRuntimeIntent::BeginAgent { .. });
    if context.is_some() && !direct && !agent {
        return Err(WorkError::Invalid);
    }
    let admitted = |projection: WorkRuntimeProjection, receipt, replayed| {
        let projection = Box::new(projection);
        if direct {
            WorkReply::PublicReadAdmitted {
                projection,
                receipt,
                replayed,
            }
        } else if agent {
            WorkReply::AgentAdmitted {
                projection,
                receipt,
                replayed,
            }
        } else {
            WorkReply::RuntimeCommand {
                projection,
                receipt,
            }
        }
    };
    let input =
        serde_json::to_vec(&(expected, &intent, &context)).map_err(|_| WorkError::Invalid)?;
    if input.len() > MAX_WORK_REQUEST_BYTES {
        return Err(WorkError::Capacity);
    }
    let digest = Sha256::digest(input).to_vec();
    let previous: Option<(Vec<u8>, String)> = tx.query_row("SELECT request_digest, CASE WHEN length(CAST(body AS BLOB)) <= 512 THEN body END FROM work_commands WHERE work_id = ?1 AND command_id = ?2", params![id.to_string(), command.to_string()], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(db)?;
    if let Some((previous_digest, body)) = previous {
        if previous_digest != digest {
            return Err(WorkError::Conflict);
        }
        let receipt: WorkCommandReceipt = decode(&body)?;
        if receipt.command != command || receipt.applied_revision > current.revision {
            return Err(WorkError::Invalid);
        }
        return Ok((
            admitted(projection(tx, profile, id, session)?, receipt, true),
            false,
        ));
    }
    if current.revision != expected || current.lifecycle != WorkLifecycle::Active {
        return Err(WorkError::Conflict);
    }
    let mut expected = expected;
    let intent = if let WorkRuntimeIntent::ReadPublic { scope, limits } = intent {
        zephium_core::work::search::validate_direct_public_read(&scope, limits)?;
        if current.objective != scope.query || current.status == WorkAuthoringStatus::NeedsInput {
            return Err(WorkError::Invalid);
        }
        require_idle(tx, id, expected)?;
        let draft = zephium_core::work::proposal::WorkPlanProposal {
            nodes: vec![zephium_core::work::proposal::WorkNodeProposal {
                key: 0,
                objective: scope.query.clone(),
                dependencies: vec![],
                outputs: vec![WorkExpectedOutput {
                    name: "Research findings".into(),
                    description: "Public findings with attributable sources".into(),
                    review: WorkOutputReview::SourceMappedNeedsReview,
                }],
            }],
        }
        .mint()?;
        let (reply, _) = apply(
            tx,
            profile,
            session,
            WorkRequest::Edit {
                id,
                expected,
                edit: WorkEdit::ReplaceDraft { draft },
                author: WorkAuthor::User,
            },
        )?;
        let WorkReply::Snapshot(snapshot) = reply else {
            return Err(WorkError::Invalid);
        };
        current = *snapshot;
        expected = current.revision;
        let plan = current.plan.as_ref().ok_or(WorkError::Invalid)?;
        WorkRuntimeIntent::Approve {
            spec: WorkExecutionSpec {
                request: None,
                plan_revision: plan.revision,
                limits,
                nodes: vec![WorkNodeExecutionSpec {
                    node: plan.draft.nodes[0].id,
                    parent: None,
                    capability: WorkCapability::PublicSearch { scope },
                    limits,
                }],
                context,
            },
        }
    } else if let WorkRuntimeIntent::BeginAgent { grant, limits } = intent {
        grant.validate()?;
        limits.validate()?;
        if current.status == WorkAuthoringStatus::NeedsInput {
            return Err(WorkError::Invalid);
        }
        require_idle(tx, id, expected)?;
        let draft = zephium_core::work::proposal::WorkPlanProposal {
            nodes: vec![zephium_core::work::proposal::WorkNodeProposal {
                key: 0,
                objective: current.objective.clone(),
                dependencies: vec![],
                outputs: vec![WorkExpectedOutput {
                    name: "Result".into(),
                    description: "Objects and findings with attributable sources".into(),
                    review: WorkOutputReview::SourceMappedNeedsReview,
                }],
            }],
        }
        .mint()?;
        let (reply, _) = apply(
            tx,
            profile,
            session,
            WorkRequest::Edit {
                id,
                expected,
                edit: WorkEdit::ReplaceDraft { draft },
                author: WorkAuthor::User,
            },
        )?;
        let WorkReply::Snapshot(snapshot) = reply else {
            return Err(WorkError::Invalid);
        };
        current = *snapshot;
        expected = current.revision;
        let plan = current.plan.as_ref().ok_or(WorkError::Invalid)?;
        let mut spec = WorkExecutionSpec::agent(plan, limits, grant)?;
        spec.context = context;
        WorkRuntimeIntent::Approve { spec }
    } else {
        intent
    };
    let all = rows(tx, id)?;
    let execution = match intent {
        WorkRuntimeIntent::ReadPublic { .. } | WorkRuntimeIntent::BeginAgent { .. } => {
            return Err(WorkError::Invalid)
        }
        WorkRuntimeIntent::AnswerStep {
            execution,
            step,
            answer,
        } => {
            let mut row = all
                .into_iter()
                .find(|r| r.fact.id == execution)
                .ok_or(WorkError::NotFound)?;
            if row.fact.status != WorkExecutionStatus::Running || !row.fact.is_agent() {
                return Err(WorkError::Conflict);
            }
            let asked = row
                .fact
                .steps
                .iter_mut()
                .find(|s| s.id == step)
                .ok_or(WorkError::NotFound)?;
            let WorkStepKindV1::Ask {
                answer: pending, ..
            } = &mut asked.kind
            else {
                return Err(WorkError::Invalid);
            };
            if asked.status != WorkStepStatus::Running || pending.is_some() {
                return Err(WorkError::Conflict);
            }
            *pending = Some(answer);
            asked.status = WorkStepStatus::Succeeded;
            row.fact.validate(
                &read_plan(tx, id, row.fact.spec.plan_revision)?,
                expected.next()?,
            )?;
            write(tx, id, &row.fact)?;
            execution
        }
        WorkRuntimeIntent::ApproveStep {
            execution,
            step,
            approve,
        } => {
            let mut row = all
                .into_iter()
                .find(|r| r.fact.id == execution)
                .ok_or(WorkError::NotFound)?;
            if row.fact.status != WorkExecutionStatus::Running || !row.fact.is_agent() {
                return Err(WorkError::Conflict);
            }
            let proposed = row
                .fact
                .steps
                .iter_mut()
                .find(|s| s.id == step)
                .ok_or(WorkError::NotFound)?;
            let (WorkStepKindV1::WriteFile {
                decision: pending, ..
            }
            | WorkStepKindV1::EditFile {
                decision: pending, ..
            }
            | WorkStepKindV1::MoveFile {
                decision: pending, ..
            }
            | WorkStepKindV1::DeleteFile {
                decision: pending, ..
            }
            | WorkStepKindV1::RunCommand {
                decision: pending, ..
            }) = &mut proposed.kind
            else {
                return Err(WorkError::Invalid);
            };
            if proposed.status != WorkStepStatus::Running || pending.is_some() {
                return Err(WorkError::Conflict);
            }
            if matches!(proposed.kind, WorkStepKindV1::RunCommand { .. })
                && proposed
                    .local
                    .as_ref()
                    .and_then(|l| l.policy.as_ref())
                    .is_none_or(|p| p.scope == WorkCommandApprovalScopeV1::None)
            {
                return Err(WorkError::Conflict);
            }
            match &mut proposed.kind {
                WorkStepKindV1::WriteFile { decision, .. }
                | WorkStepKindV1::EditFile { decision, .. }
                | WorkStepKindV1::MoveFile { decision, .. }
                | WorkStepKindV1::DeleteFile { decision, .. }
                | WorkStepKindV1::RunCommand { decision, .. } => *decision = Some(approve),
                _ => return Err(WorkError::Invalid),
            }
            if approve {
                if let Some(policy) = proposed
                    .local
                    .as_ref()
                    .and_then(|l| l.policy.as_ref())
                    .filter(|p| p.scope == WorkCommandApprovalScopeV1::Folder)
                {
                    if !row
                        .fact
                        .folder_approvals
                        .iter()
                        .any(|a| a.root == policy.root)
                    {
                        row.fact.folder_approvals.push(WorkFolderApprovalV1 {
                            root: policy.root.clone(),
                            at: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map_err(|_| WorkError::Unavailable)?
                                .as_secs()
                                .to_string(),
                        });
                    }
                }
            }
            row.fact.validate(
                &read_plan(tx, id, row.fact.spec.plan_revision)?,
                expected.next()?,
            )?;
            write(tx, id, &row.fact)?;
            execution
        }
        WorkRuntimeIntent::Steer { execution, text } => {
            let mut row = all
                .into_iter()
                .find(|r| r.fact.id == execution)
                .ok_or(WorkError::NotFound)?;
            if row.fact.status != WorkExecutionStatus::Running || !row.fact.is_agent() {
                return Err(WorkError::Conflict);
            }
            let max_steps = row
                .fact
                .agent_grant()
                .map(|grant| usize::from(grant.max_steps))
                .ok_or(WorkError::Invalid)?;
            // Leave room for the agent's own next turn and finish.
            if row.fact.steps.len() + 2 >= max_steps {
                return Err(WorkError::Capacity);
            }
            let turn = row.fact.steps.last().map_or(1, |step| step.turn);
            row.fact.steps.push(WorkStepFact {
                id: WorkStepId::generate(),
                turn,
                kind: WorkStepKindV1::Steer { text },
                status: WorkStepStatus::Succeeded,
                usage: None,
                artifacts: vec![],
                evidence: None,
                note: None,
                measurements: None,
                local: None,
            });
            row.fact.validate(
                &read_plan(tx, id, row.fact.spec.plan_revision)?,
                expected.next()?,
            )?;
            write(tx, id, &row.fact)?;
            execution
        }
        intent @ (WorkRuntimeIntent::ReviewArtifact { .. }
        | WorkRuntimeIntent::EditArtifact { .. }) => {
            let (execution, artifact) = match &intent {
                WorkRuntimeIntent::ReviewArtifact {
                    execution,
                    artifact,
                    ..
                }
                | WorkRuntimeIntent::EditArtifact {
                    execution,
                    artifact,
                    ..
                } => (*execution, *artifact),
                _ => unreachable!(),
            };
            let mut row = all
                .into_iter()
                .find(|r| r.fact.id == execution)
                .ok_or(WorkError::NotFound)?;
            if !matches!(
                row.fact.status,
                WorkExecutionStatus::NeedsReview | WorkExecutionStatus::Completed
            ) {
                return Err(WorkError::Conflict);
            }
            if !row.fact.artifacts.iter().any(|a| a.id == artifact) {
                return Err(WorkError::NotFound);
            }
            let index = match row
                .fact
                .user_artifacts
                .iter()
                .position(|a| a.artifact == artifact)
            {
                Some(index) => index,
                None => {
                    row.fact.user_artifacts.push(WorkArtifactUserState {
                        artifact,
                        revision: expected.next()?,
                        decision: None,
                        edited_data: None,
                        evidence: vec![],
                    });
                    row.fact.user_artifacts.len() - 1
                }
            };
            let edited = &mut row.fact.user_artifacts[index];
            edited.revision = expected.next()?;
            match intent {
                WorkRuntimeIntent::ReviewArtifact { decision, .. } => {
                    edited.decision = Some(decision)
                }
                WorkRuntimeIntent::EditArtifact { data, evidence, .. } => {
                    edited.edited_data = Some(data);
                    edited.evidence = evidence;
                    edited.decision = None;
                }
                _ => unreachable!(),
            }
            row.fact.status = if row.fact.needs_review() {
                WorkExecutionStatus::NeedsReview
            } else {
                WorkExecutionStatus::Completed
            };
            row.fact.validate(
                &read_plan(tx, id, row.fact.spec.plan_revision)?,
                expected.next()?,
            )?;
            write(tx, id, &row.fact)?;
            execution
        }
        WorkRuntimeIntent::Approve { spec } => {
            require_idle(tx, id, current.revision)?;
            let plan = current.plan.as_ref().ok_or(WorkError::Conflict)?;
            spec.validate(plan)?;
            let id_execution = WorkExecutionId::generate();
            let now = timestamp()?;
            let expires = now
                .checked_add(i64::from(spec.limits.timeout_seconds) * 1000)
                .ok_or(WorkError::Invalid)?;
            let expires_tick = session
                .tick_ms
                .checked_add(i64::from(spec.limits.timeout_seconds) * 1000)
                .ok_or(WorkError::Capacity)?;
            let fact = WorkExecutionFact {
                authorization: if direct {
                    WorkExecutionAuthorization::UserDirectedPublicRead
                } else if agent {
                    WorkExecutionAuthorization::UserDirectedAgent
                } else {
                    WorkExecutionAuthorization::ReviewedPlan
                },
                id: id_execution,
                approved_revision: expected.next()?,
                spec,
                status: WorkExecutionStatus::Approved,
                attempts: vec![],
                intervention: None,
                artifacts: vec![],
                user_artifacts: vec![],
                provider_evidence: vec![],
                file_evidence: vec![],
                command_evidence: vec![],
                folder_approvals: vec![],
                steps: vec![],
            };
            fact.validate(plan, expected.next()?)?;
            tx.execute("INSERT INTO work_executions(work_id, execution_id, plan_revision, owner_session, approved_unix_ms, expires_unix_ms, body, approved_tick_ms, expires_tick_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)", params![id.to_string(), id_execution.to_string(), fact.spec.plan_revision.get() as i64, session.session.to_string(), now, expires, body(&fact)?, session.tick_ms, expires_tick]).map_err(db)?;
            id_execution
        }
        WorkRuntimeIntent::Cancel {
            execution,
            intervention,
        } => {
            let mut row = all
                .into_iter()
                .find(|r| r.fact.id == execution)
                .ok_or(WorkError::NotFound)?;
            if row.fact.status.terminal() {
                return Err(WorkError::Conflict);
            }
            if let Some(intervention) = intervention {
                intervention.validate()?;
                row.fact.intervention = Some(intervention);
            }
            row.fact.status = if row
                .fact
                .attempts
                .iter()
                .any(|a| a.status == WorkAttemptStatus::Running)
            {
                WorkExecutionStatus::CancelRequested
            } else {
                WorkExecutionStatus::Cancelled
            };
            write(tx, id, &row.fact)?;
            execution
        }
        WorkRuntimeIntent::AcknowledgeInterruption { execution } => {
            let mut row = all
                .into_iter()
                .find(|r| r.fact.id == execution)
                .ok_or(WorkError::NotFound)?;
            if row.owner == session.session || row.fact.status.terminal() {
                return Err(WorkError::Conflict);
            }
            row.fact.interrupt();
            write(tx, id, &row.fact)?;
            execution
        }
    };
    let applied_revision = bump(tx, id, expected, WorkAuthor::User)?;
    let receipt = WorkCommandReceipt {
        command,
        applied_revision,
        execution,
    };
    tx.execute("INSERT INTO work_commands(work_id, command_id, request_digest, body) VALUES (?1, ?2, ?3, ?4)", params![id.to_string(), command.to_string(), digest, encode(&receipt)?]).map_err(db)?;
    Ok((
        admitted(projection(tx, profile, id, session)?, receipt, false),
        true,
    ))
}

pub(super) fn update(
    tx: &Transaction<'_>,
    profile: ProfileId,
    session: RuntimeClock,
    id: WorkId,
    expected: WorkRevision,
    update: WorkRuntimeUpdate,
) -> Result<(WorkReply, bool), WorkError> {
    let (update, provider_evidence) = match update {
        WorkRuntimeUpdate::SettleProviderSearch {
            execution,
            attempt,
            status,
            usage,
            artifacts,
            evidence,
        } => (
            WorkRuntimeUpdate::Settle {
                execution,
                attempt,
                status,
                usage,
                artifacts,
                intervention: None,
            },
            Some(evidence),
        ),
        update => (update, None),
    };
    let current = read(tx, profile, id)?;
    if current.revision != expected || current.lifecycle != WorkLifecycle::Active {
        return Err(WorkError::Conflict);
    }
    let execution = match &update {
        WorkRuntimeUpdate::Begin { execution, .. }
        | WorkRuntimeUpdate::BeginChild { execution, .. }
        | WorkRuntimeUpdate::Settle { execution, .. }
        | WorkRuntimeUpdate::BeginStep { execution, .. }
        | WorkRuntimeUpdate::SettleStep { execution, .. }
        | WorkRuntimeUpdate::CommandProgress { execution, .. }
        | WorkRuntimeUpdate::SettleCommand { execution, .. }
        | WorkRuntimeUpdate::FinishCancellation { execution } => *execution,
        WorkRuntimeUpdate::SettleProviderSearch { .. } => return Err(WorkError::Invalid),
    };
    let mut row = rows(tx, id)?
        .into_iter()
        .find(|r| r.fact.id == execution)
        .ok_or(WorkError::NotFound)?;
    if row.owner != session.session || row.fact.status.terminal() {
        return Err(WorkError::Conflict);
    }
    let plan = read_plan(tx, id, row.fact.spec.plan_revision)?;
    row.fact.validate(&plan, current.revision)?;
    let mut remaining_millis = None;
    let parent_attempt = match &update {
        WorkRuntimeUpdate::BeginChild { parent, .. } => Some(*parent),
        _ => None,
    };
    match update {
        WorkRuntimeUpdate::Begin { attempt, node, .. }
        | WorkRuntimeUpdate::BeginChild { attempt, node, .. } => {
            let spec = row
                .fact
                .spec
                .nodes
                .iter()
                .find(|entry| entry.node == node)
                .ok_or(WorkError::Invalid)?;
            match (spec.parent, parent_attempt) {
                (None, None) => {}
                (Some(parent_node), Some(parent))
                    if row.fact.attempts.iter().any(|entry| {
                        entry.id == parent
                            && entry.node == parent_node
                            && entry.status == WorkAttemptStatus::Running
                    }) && row.fact.spec.nodes.iter().any(|entry| {
                        entry.node == parent_node
                            && matches!(
                                entry.capability,
                                WorkCapability::Coordinate { .. }
                                    | WorkCapability::CoordinatePublicDiscovery { .. }
                                    | WorkCapability::CoordinatePublicResearch { .. }
                            )
                    }) => {}
                _ => return Err(WorkError::Unavailable),
            }
            let now = session.tick_ms;
            if !matches!(
                row.fact.status,
                WorkExecutionStatus::Approved | WorkExecutionStatus::Running
            ) || now < row.approved_tick
                || now >= row.expires_tick
                || current.plan.as_ref().map(|p| p.revision) != Some(plan.revision)
                || row
                    .fact
                    .attempts
                    .iter()
                    .any(|a| a.id == attempt || a.node == node)
                || row.fact.attempts.iter().any(|a| {
                    !matches!(
                        a.status,
                        WorkAttemptStatus::Running | WorkAttemptStatus::Succeeded
                    )
                })
                || row.fact.attempts.len() >= MAX_WORK_ATTEMPTS
                || row
                    .fact
                    .attempts
                    .iter()
                    .filter(|a| a.status == WorkAttemptStatus::Running)
                    .count()
                    >= usize::from(row.fact.spec.limits.max_workers)
            {
                return Err(WorkError::Conflict);
            }
            let node_plan = plan
                .draft
                .nodes
                .iter()
                .find(|n| n.id == node)
                .ok_or(WorkError::Invalid)?;
            if !node_plan.dependencies.iter().all(|dependency| {
                row.fact
                    .attempts
                    .iter()
                    .any(|a| a.node == *dependency && a.status == WorkAttemptStatus::Succeeded)
                    || (matches!(
                        spec.capability,
                        WorkCapability::Coordinate { .. }
                            | WorkCapability::CoordinatePublicDiscovery { .. }
                            | WorkCapability::CoordinatePublicResearch { .. }
                    ) && descendant(&row.fact.spec, *dependency, node))
            }) {
                return Err(WorkError::Conflict);
            }
            row.fact.attempts.push(WorkAttemptFact {
                id: attempt,
                node,
                status: WorkAttemptStatus::Running,
                usage: None,
            });
            row.fact.status = WorkExecutionStatus::Running;
            remaining_millis =
                Some(u32::try_from(row.expires_tick - now).map_err(|_| WorkError::Invalid)?);
        }
        WorkRuntimeUpdate::Settle {
            attempt,
            status,
            usage,
            artifacts,
            intervention,
            ..
        } => {
            if status == WorkAttemptStatus::Running
                || (status != WorkAttemptStatus::Succeeded && !artifacts.is_empty())
                || intervention.as_ref().is_some_and(|i| {
                    i.validate().is_err() || status == WorkAttemptStatus::Succeeded
                })
            {
                return Err(WorkError::Invalid);
            }
            let index = row
                .fact
                .attempts
                .iter()
                .position(|a| a.id == attempt)
                .ok_or(WorkError::NotFound)?;
            let fact = &row.fact.attempts[index];
            if fact.status != WorkAttemptStatus::Running {
                return Err(WorkError::Conflict);
            }
            if artifacts
                .iter()
                .any(|a| a.attempt != attempt || a.node != fact.node || a.execution != execution)
            {
                return Err(WorkError::Invalid);
            }
            if status == WorkAttemptStatus::Succeeded {
                let node = plan
                    .draft
                    .nodes
                    .iter()
                    .find(|n| n.id == fact.node)
                    .ok_or(WorkError::Invalid)?;
                // A coordinator can start to delegate its prerequisites, but
                // neither it nor a worker may publish before their success.
                let succeeded = |id| {
                    row.fact
                        .attempts
                        .iter()
                        .any(|a| a.node == id && a.status == WorkAttemptStatus::Succeeded)
                };
                if !node.dependencies.iter().all(|id| succeeded(*id))
                    || row
                        .fact
                        .spec
                        .nodes
                        .iter()
                        .any(|entry| entry.parent == Some(fact.node) && !succeeded(entry.node))
                {
                    return Err(WorkError::Conflict);
                }
                if !row.fact.is_agent()
                    && (artifacts.len() != node.outputs.len()
                        || !node
                            .outputs
                            .iter()
                            .all(|o| artifacts.iter().any(|a| a.output == o.name)))
                {
                    return Err(WorkError::Invalid);
                }
            }
            row.fact.attempts[index].status = status;
            row.fact.attempts[index].usage = usage;
            row.fact.artifacts.extend(artifacts);
            if intervention.is_some() {
                row.fact.intervention = intervention;
            }
            if let Some(evidence) = provider_evidence {
                if evidence.attempt != attempt {
                    return Err(WorkError::Invalid);
                }
                row.fact.provider_evidence.push(*evidence);
            }
            let running = row
                .fact
                .attempts
                .iter()
                .any(|a| a.status == WorkAttemptStatus::Running);
            if !running {
                row.fact.status = if row
                    .fact
                    .attempts
                    .iter()
                    .any(|a| a.status == WorkAttemptStatus::OutcomeUnknown)
                {
                    WorkExecutionStatus::Interrupted
                } else if row.fact.status == WorkExecutionStatus::CancelRequested {
                    WorkExecutionStatus::Cancelled
                } else if row
                    .fact
                    .attempts
                    .iter()
                    .any(|a| a.status != WorkAttemptStatus::Succeeded)
                {
                    WorkExecutionStatus::Failed
                } else if row.fact.attempts.len() == plan.draft.nodes.len() {
                    if row
                        .fact
                        .artifacts
                        .iter()
                        .all(|a| a.review == WorkOutputReview::Mechanical)
                    {
                        WorkExecutionStatus::Completed
                    } else {
                        WorkExecutionStatus::NeedsReview
                    }
                } else {
                    WorkExecutionStatus::Running
                };
            }
        }
        WorkRuntimeUpdate::BeginStep {
            attempt,
            step,
            artifacts,
            evidence,
            file,
            ..
        } => {
            let fact = row
                .fact
                .attempts
                .iter()
                .find(|a| a.id == attempt)
                .ok_or(WorkError::NotFound)?;
            if fact.status != WorkAttemptStatus::Running
                || !row.fact.is_agent()
                || row.fact.steps.iter().any(|s| s.id == step.id)
                || row.fact.steps.len() >= MAX_WORK_STEPS
            {
                return Err(WorkError::Conflict);
            }
            step.validate()?;
            let node = fact.node;
            attach_step_payload(
                &mut row.fact,
                attempt,
                node,
                &step.artifacts,
                step.evidence,
                artifacts,
                evidence,
                file,
            )?;
            row.fact.steps.push(step);
        }
        WorkRuntimeUpdate::CommandProgress {
            attempt,
            step,
            output,
            ..
        } => {
            let node = row
                .fact
                .attempts
                .iter()
                .find(|a| a.id == attempt && a.status == WorkAttemptStatus::Running)
                .ok_or(WorkError::Conflict)?;
            let _ = node;
            let step = row
                .fact
                .steps
                .iter_mut()
                .find(|s| s.id == step && s.status == WorkStepStatus::Running)
                .ok_or(WorkError::Conflict)?;
            if !matches!(step.kind, WorkStepKindV1::RunCommand { .. }) {
                return Err(WorkError::Invalid);
            }
            let local = step.local.as_mut().ok_or(WorkError::Invalid)?;
            if local
                .policy
                .as_ref()
                .is_some_and(|p| p.scope != WorkCommandApprovalScopeV1::None)
                && step.kind.file_decision() != Some(true)
            {
                return Err(WorkError::Conflict);
            }
            validate_local_text(&output.text)?;
            local.output = Some(output);
        }
        WorkRuntimeUpdate::SettleCommand {
            attempt,
            step,
            status,
            record,
            note,
            ..
        } => {
            if !matches!(status, WorkStepStatus::Succeeded | WorkStepStatus::Failed) {
                return Err(WorkError::Invalid);
            }
            let fact = row
                .fact
                .attempts
                .iter()
                .find(|a| a.id == attempt && a.status == WorkAttemptStatus::Running)
                .ok_or(WorkError::Conflict)?;
            if record.attempt != attempt
                || record.node != fact.node
                || row.fact.command_evidence.len() >= MAX_WORK_STEPS
            {
                return Err(WorkError::Invalid);
            }
            record.command.validate()?;
            let step = row
                .fact
                .steps
                .iter_mut()
                .find(|s| s.id == step && s.status == WorkStepStatus::Running)
                .ok_or(WorkError::Conflict)?;
            let WorkStepKindV1::RunCommand { cwd, command, .. } = &step.kind else {
                return Err(WorkError::Invalid);
            };
            if *cwd != record.command.cwd || *command != record.command.command {
                return Err(WorkError::Invalid);
            }
            let policy = step
                .local
                .as_ref()
                .and_then(|l| l.policy.as_ref())
                .ok_or(WorkError::Invalid)?;
            if policy.scope != WorkCommandApprovalScopeV1::None
                && step.kind.file_decision() != Some(true)
            {
                return Err(WorkError::Conflict);
            }
            step.status = status;
            step.evidence = Some(record.id);
            step.note = Some(note);
            if let Some(local) = &mut step.local {
                local.output = None;
            }
            row.fact.command_evidence.push(*record);
        }
        WorkRuntimeUpdate::SettleStep {
            attempt,
            step,
            status,
            usage,
            artifacts,
            evidence,
            file,
            note,
            measurements,
            ..
        } => {
            if status == WorkStepStatus::Running {
                return Err(WorkError::Invalid);
            }
            let fact = row
                .fact
                .attempts
                .iter()
                .find(|a| a.id == attempt)
                .ok_or(WorkError::NotFound)?;
            if fact.status != WorkAttemptStatus::Running || !row.fact.is_agent() {
                return Err(WorkError::Conflict);
            }
            let node = fact.node;
            let index = row
                .fact
                .steps
                .iter()
                .position(|s| s.id == step)
                .ok_or(WorkError::NotFound)?;
            if row.fact.steps[index].status != WorkStepStatus::Running {
                return Err(WorkError::Conflict);
            }
            let ids: Vec<_> = artifacts.iter().map(|a| a.id).collect();
            let record = evidence
                .as_ref()
                .map(|e| e.id)
                .or(file.as_ref().map(|f| f.id));
            attach_step_payload(
                &mut row.fact,
                attempt,
                node,
                &ids,
                record,
                artifacts,
                evidence,
                file,
            )?;
            let settled = &mut row.fact.steps[index];
            settled.status = status;
            settled.usage = usage;
            settled.artifacts = ids;
            settled.evidence = record;
            if note.is_some() {
                settled.note = note;
            }
            if measurements.is_some() {
                settled.measurements = measurements;
            }
        }
        WorkRuntimeUpdate::FinishCancellation { .. } => {
            if row.fact.status != WorkExecutionStatus::CancelRequested
                || row
                    .fact
                    .attempts
                    .iter()
                    .any(|a| a.status == WorkAttemptStatus::Running)
            {
                return Err(WorkError::Conflict);
            }
            row.fact.status = WorkExecutionStatus::Cancelled;
        }
        WorkRuntimeUpdate::SettleProviderSearch { .. } => return Err(WorkError::Invalid),
    }
    row.fact.validate(&plan, expected.next()?)?;
    write(tx, id, &row.fact)?;
    bump(tx, id, expected, WorkAuthor::PrimaryAgent)?;
    let projection = Box::new(projection(tx, profile, id, session)?);
    Ok((
        match remaining_millis {
            Some(remaining_millis) => WorkReply::RuntimeStarted {
                projection,
                remaining_millis,
            },
            None => WorkReply::Runtime(projection),
        },
        true,
    ))
}

/// Artifacts and a provider record are attached once, by exactly the step
/// that claims them, and only to the live attempt that produced them.
#[allow(clippy::too_many_arguments)]
fn attach_step_payload(
    fact: &mut WorkExecutionFact,
    attempt: WorkAttemptId,
    node: WorkPlanNodeId,
    claimed: &[WorkArtifactId],
    claimed_evidence: Option<WorkArtifactId>,
    artifacts: Vec<WorkArtifactV1>,
    evidence: Option<Box<WorkProviderSearchRecordV1>>,
    file: Option<Box<WorkFileRecordV1>>,
) -> Result<(), WorkError> {
    if artifacts.len() != claimed.len()
        || !claimed
            .iter()
            .all(|id| artifacts.iter().any(|a| a.id == *id))
        || (evidence.is_some() && file.is_some())
        || evidence
            .as_ref()
            .map(|e| e.id)
            .or(file.as_ref().map(|f| f.id))
            != claimed_evidence
        || artifacts
            .iter()
            .any(|a| a.attempt != attempt || a.node != node || a.execution != fact.id)
        || artifacts
            .iter()
            .any(|a| fact.artifacts.iter().any(|existing| existing.id == a.id))
    {
        return Err(WorkError::Invalid);
    }
    if fact.artifacts.len() + artifacts.len() > zephium_core::work::artifact::MAX_WORK_ARTIFACTS {
        return Err(WorkError::Capacity);
    }
    if let Some(record) = evidence {
        if record.attempt != attempt
            || record.node != node
            || fact.provider_evidence.iter().any(|e| e.id == record.id)
        {
            return Err(WorkError::Invalid);
        }
        fact.provider_evidence.push(*record);
    }
    if let Some(record) = file {
        if record.attempt != attempt
            || record.node != node
            || fact.file_evidence.iter().any(|e| e.id == record.id)
            || fact.file_evidence.len() >= MAX_WORK_STEPS
        {
            return Err(WorkError::Invalid);
        }
        fact.file_evidence.push(*record);
    }
    fact.artifacts.extend(artifacts);
    Ok(())
}

#[cfg(test)]
#[path = "work_runtime_tests.rs"]
mod tests;
