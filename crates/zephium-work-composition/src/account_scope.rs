//! Account-scoped requests for a live durable attempt: one signed-in page read
//! or one reversible field update, both in a Work-owned page that shares the
//! selected profile's cookies. The account is user-attested at approval; this
//! module supplies no detector and never claims independent verification.
use crate::{
    native_work_clock::{authority_window, NativeWorkClock},
    PublicReadWorkAccount, PublicReadWorkInvocation, PublicReadWorkObjective,
    PublicReadWorkSettings, TrustedWorkRequest,
};
use std::{sync::Arc, time::Instant};
use zephium_agent_controller::*;
use zephium_agent_provider_transport::AgentProviderCredential;
use zephium_agentic::*;
use zephium_app::{AgentWorkApplicationConfig, AgentWorkProfileBinding};
use zephium_core::work::{runtime::*, WorkError};

const MAX_MODEL_CALLS: u8 = 16;
const MAX_HOPS: usize = 2;
const EXTRACTION_FIELD: &str = "output_0";

/// The user's approval named this account for this profile and origin. Each
/// sample is minted at request time in the Work clock domain.
struct UserAttestedAccount {
    account: AgentAccountId,
}
impl AgentWorkAccountSource for UserAttestedAccount {
    fn sample(&self, context: ContextJoin) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        let observed_at =
            zephium_engine::work_browser_monotonic_now().ok_or(AgentWorkFailure::Contract)?;
        Ok(AgentContextAccountBinding::new(
            AgentAccountAttestationId::generate(),
            context,
            AgentAccountScope::Authenticated(self.account),
            observed_at,
        ))
    }
}

pub(crate) struct AccountOperands {
    pub profile: AgentWorkProfileBinding,
    pub model: AgentBrowserModel,
    pub config: AgentWorkApplicationConfig,
    pub credential: AgentProviderCredential,
    pub budget: AgentRunBudget,
    pub deadline: Instant,
    pub objective: String,
}

fn account_id(scope: &WorkAccountScope) -> Result<AgentAccountId, WorkError> {
    AgentAccountId::parse(&scope.account).ok_or(WorkError::Invalid)
}

/// Slash-delimited directory of the page path; the whole origin when the
/// path cannot be expressed as a safe prefix.
pub(crate) fn path_prefix(path: &str) -> String {
    let directory = &path[..=path.rfind('/').unwrap_or(0)];
    if directory.contains('%') || directory.split('/').any(|part| matches!(part, "." | "..")) {
        "/".into()
    } else {
        directory.to_owned()
    }
}

pub(crate) fn read_request(
    scope: &WorkAccountScope,
    output_fields: Vec<SemanticExtractionFieldSchema>,
    operands: AccountOperands,
) -> Result<TrustedWorkRequest, WorkError> {
    scope.validate()?;
    let target = ContextNavigationTarget::parse(&scope.url).map_err(|_| WorkError::Invalid)?;
    let origin = SemanticOrigin::parse(&scope.origin).map_err(|_| WorkError::Invalid)?;
    let rule = AgentNavigationOriginRule::try_new(
        origin,
        path_prefix(target.as_url().path()),
        true,
        false,
    )
    .map_err(|_| WorkError::Invalid)?;
    let navigation = AgentNavigationDiscovery::try_new_production(target, vec![rule], MAX_HOPS, 2)
        .map_err(|_| WorkError::Invalid)?;
    let account = account_id(scope)?;
    let mut objective = operands.objective;
    objective.push_str("\nThis page is open with the user's own signed-in session at the approved origin. Read it and stay within the approved scope; you have no permission to write, submit, or leave the origin.\n");
    PublicReadWorkInvocation::new(
        PublicReadWorkObjective {
            objective,
            navigation,
            output_fields,
        },
        PublicReadWorkSettings {
            account: PublicReadWorkAccount::Identified {
                account,
                source: Box::new(UserAttestedAccount { account }),
            },
            model: operands.model,
            budget: operands.budget,
            max_model_calls: MAX_MODEL_CALLS,
            deadline: operands.deadline,
        },
        operands.config,
        operands.credential,
    )
    .with_persistent_result()
    .into_request(operands.profile)
    .map_err(|_| WorkError::Invalid)
}

pub(crate) fn update_request(
    scope: &WorkAccountScope,
    update: &WorkFieldUpdateV1,
    operands: AccountOperands,
) -> Result<TrustedWorkRequest, WorkError> {
    scope.validate()?;
    update.validate()?;
    let target = ContextNavigationTarget::parse(&scope.url).map_err(|_| WorkError::Invalid)?;
    let origin = SemanticOrigin::parse(&scope.origin).map_err(|_| WorkError::Invalid)?;
    let account = AgentAccountScope::Authenticated(account_id(scope)?);
    let identity = ContextIdentity::new(
        ContextId::generate(),
        ContextRunId::generate(),
        operands.profile.profile(),
        ContextKind::Owned,
    );
    let effects = AgentEffectScope::try_new(&[
        SemanticEffectClass::Read,
        SemanticEffectClass::ExternalWrite,
    ])
    .map_err(|_| WorkError::Invalid)?;
    let node = AgentPlanNodeId::generate();
    let authority = AgentPlanNodeAuthority::try_new(
        vec![identity.profile()],
        vec![account],
        vec![origin.clone()],
        SemanticSensitivity::Public,
        effects,
    )
    .map_err(|_| WorkError::Invalid)?;
    let (issued, expires) = authority_window(operands.deadline).map_err(|_| WorkError::Invalid)?;
    let manifest = AgentRunManifest::try_new(
        AgentRunManifestId::generate(),
        identity.owner(),
        AgentRunScope::try_new(
            vec![identity.profile()],
            vec![account],
            vec![origin.clone()],
            SemanticSensitivity::Public,
            effects,
            Vec::new(),
        )
        .map_err(|_| WorkError::Invalid)?,
        operands.budget,
        issued,
        expires,
        vec![AgentPlanNodeScope::new(
            node,
            authority,
            operands.budget,
            expires,
        )],
    )
    .map_err(|_| WorkError::Invalid)?;
    let ids = TerraControllerIds::try_new(
        AgentSupervisorId::new(1).ok_or(WorkError::Invalid)?,
        AgentSupervisorAttemptId::new(1).ok_or(WorkError::Invalid)?,
        AgentSupervisorCancellationId::new(1).ok_or(WorkError::Invalid)?,
        AgentModelCallId::new(1).ok_or(WorkError::Invalid)?,
        [1, 2, 3, 4].map(|id| AgentAuditEventId::new(id).expect("fixed nonzero ID")),
        AgentAuditDeliveryId::new(1).ok_or(WorkError::Invalid)?,
    )
    .map_err(|_| WorkError::Invalid)?;
    let settings = AgentWorkRunSettings::new(
        operands.model,
        ids,
        Arc::new(NativeWorkClock),
        operands.deadline,
    )
    .with_max_model_calls(MAX_MODEL_CALLS)
    .map_err(|_| WorkError::Invalid)?;
    let input = AgentWorkRunInput::try_new(
        manifest,
        AgentPlanLeaseBinding::new(AgentPlanLeaseId::generate(), node),
        AgentWorkContextSpec::try_new_with_document_policy(
            identity,
            operands.profile.storage_class(),
            target,
            WorkBrowserDocumentPolicy::Exact,
        )
        .map_err(|_| WorkError::Invalid)?,
        update_objective(&operands.objective, update),
        settings,
    )
    .map_err(|_| WorkError::Invalid)?
    .persist_extraction_result()
    .map_err(|_| WorkError::Invalid)?;
    let task = FieldUpdateTask::try_new(identity, origin, account, update.clone())
        .map_err(|_| WorkError::Invalid)?;
    Ok(
        TrustedWorkRequest::new(input, operands.config, operands.credential, Box::new(task))
            .with_browser_profile(operands.profile),
    )
}

fn update_objective(intent: &str, update: &WorkFieldUpdateV1) -> String {
    let field = update.field.as_deref().map_or_else(
        || "text field".to_owned(),
        |name| format!("field named `{name}`"),
    );
    format!(
        "User intent: {intent}\n\nOn the current signed-in page, change the {field} whose value is exactly `{from}` to exactly `{to}`, then restore it to exactly `{from}`. Use the current semantic field reference; do not invent selectors. Perform exactly one bounded fill action per turn with effect=external_write, wait=mutation_quiet 1000 ms, verification=target_value_matches_input, and settle_budget=5000 ms. Continue after the first verified change and restore the original value. After the host verifies the restoration from fresh browser state, call extract with trusted schema 1 and return {EXTRACTION_FIELD} as the exact current field value with only its value-preview source. Do not navigate, click, submit, edit any other field, open another page, or run code. Both exact updates were approved by the user; the host independently restricts the target, values, ordering, account, origin, and effects.",
        from = update.from,
        to = update.to,
    )
}

#[derive(Clone)]
struct Checkpoint {
    observation: SemanticObservationId,
    generation: SemanticObservationGeneration,
    frame: SemanticFrameJoin,
    invocation: SemanticInvocationId,
    snapshot: SemanticSnapshotGeneration,
    reference: SemanticReferenceId,
}

/// Exactly two fill transitions on one uniquely identified public text
/// control, each proven from a fresh observation, followed by an extraction
/// bound to the restored value. Ambiguous or changed pages refuse.
struct FieldUpdateTask {
    inner: AgentWorkFormExtractionTask,
    identity: ContextIdentity,
    origin: SemanticOrigin,
    update: WorkFieldUpdateV1,
    initial: Option<Checkpoint>,
    changed: Option<Checkpoint>,
    restored: Option<Checkpoint>,
}
impl FieldUpdateTask {
    fn try_new(
        identity: ContextIdentity,
        origin: SemanticOrigin,
        account: AgentAccountScope,
        update: WorkFieldUpdateV1,
    ) -> Result<Self, AgentWorkFailure> {
        if identity.kind() != ContextKind::Owned
            || !matches!(account, AgentAccountScope::Authenticated(_))
        {
            return Err(AgentWorkFailure::Contract);
        }
        let transition = |from: &str, to: &str| {
            AgentWorkFormPhase::try_new(vec![AgentWorkFormGoal::fill_transition(
                update.field.clone(),
                from.into(),
                to.into(),
            )?])
        };
        let inner = AgentWorkFormTask::try_new_external_update(
            identity,
            origin.clone(),
            account,
            vec![
                transition(&update.from, &update.to)?,
                transition(&update.to, &update.from)?,
            ],
        )?
        .with_baseline_read()
        .with_extraction(vec![SemanticExtractionFieldSchema::try_text(
            EXTRACTION_FIELD.into(),
            true,
            MAX_WORK_FIELD_VALUE_BYTES,
        )
        .map_err(|_| AgentWorkFailure::Contract)?])?;
        Ok(Self {
            inner,
            identity,
            origin,
            update,
            initial: None,
            changed: None,
            restored: None,
        })
    }

    fn expected(&self) -> &str {
        if self.initial.is_none() || self.changed.is_some() {
            &self.update.from
        } else {
            &self.update.to
        }
    }

    fn checkpoint(
        &self,
        observation: &SemanticObservation,
    ) -> Result<Checkpoint, AgentWorkFailure> {
        let (checkpoint, readiness) = self.field_state(observation)?;
        if readiness != AgentWorkInitialReadiness::Ready {
            return Err(AgentWorkFailure::Contract);
        }
        Ok(checkpoint)
    }

    fn field_state(
        &self,
        observation: &SemanticObservation,
    ) -> Result<(Checkpoint, AgentWorkInitialReadiness), AgentWorkFailure> {
        let expected = self.expected();
        let context = observation.request().context();
        let [frame] = observation.frames() else {
            return Err(AgentWorkFailure::Contract);
        };
        if context.identity() != self.identity
            || context.frame() != FrameId::MAIN
            || !matches!(observation.request().scope(), SemanticScope::Initial)
            || observation.request().generation() != SemanticObservationGeneration::INITIAL
            || frame.frame().context() != context
            || frame.frame().frame() != FrameId::MAIN
            || frame.frame().origin() != &self.origin
            || frame.completeness() != SemanticCompleteness::Complete
            || observation.frame_boundaries().iter().any(|boundary| {
                boundary.parent_frame() != FrameId::MAIN
                    || boundary.status()
                        != SemanticFrameBoundaryStatus::Unsupported(
                            SemanticFrameUnsupported::PolicyBlocked,
                        )
            })
        {
            return Err(AgentWorkFailure::Contract);
        }
        let mut exact_values = 0_u16;
        let mut candidates = 0_u16;
        let mut candidate = None;
        for node in frame.nodes() {
            if node.role() != SemanticRole::Textbox {
                continue;
            }
            let exact = matches!(node.value(), Some(SemanticValueSummary::Text(value)) if !value.preview().truncated() && value.preview().source_bytes() == expected.len() && value.preview().text() == expected);
            if !exact
                || self
                    .update
                    .field
                    .as_deref()
                    .is_some_and(|name| node.name().map(SemanticText::as_str) != Some(name))
            {
                continue;
            }
            exact_values = exact_values.saturating_add(1);
            if node.sensitivity() != SemanticSensitivity::Public {
                return Err(AgentWorkFailure::Contract);
            }
            candidate = Some(node);
            if !node.states().contains(SemanticState::Disabled)
                && node.operations().contains(SemanticOperationClass::Fill)
            {
                candidates = candidates.saturating_add(1);
            }
        }
        if exact_values != 1 {
            return Err(AgentWorkFailure::Contract);
        }
        let node = candidate.ok_or(AgentWorkFailure::Contract)?;
        let index = frame
            .nodes()
            .iter()
            .position(|entry| entry.reference() == node.reference())
            .and_then(|index| u16::try_from(index).ok())
            .ok_or(AgentWorkFailure::Contract)?;
        if frame
            .nodes()
            .iter()
            .any(|entry| entry.parent() == Some(index))
        {
            return Err(AgentWorkFailure::Contract);
        }
        Ok((
            Checkpoint {
                observation: observation.request().id(),
                generation: observation.request().generation(),
                frame: frame.frame().clone(),
                invocation: frame.invocation(),
                snapshot: frame.generation(),
                reference: node.reference(),
            },
            if candidates == 1 {
                AgentWorkInitialReadiness::Ready
            } else {
                AgentWorkInitialReadiness::Pending
            },
        ))
    }

    fn fresh_successor(prior: &Checkpoint, next: &Checkpoint) -> bool {
        prior.frame == next.frame
            && prior.observation != next.observation
            && prior.invocation != next.invocation
            && next.snapshot > prior.snapshot
    }
}

impl AgentWorkTask for FieldUpdateTask {
    fn initial_readiness(
        &self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkInitialReadiness, AgentWorkFailure> {
        if self.initial.is_some() || self.changed.is_some() || self.restored.is_some() {
            return Err(AgentWorkFailure::Contract);
        }
        self.field_state(observation)
            .map(|(_, readiness)| readiness)
    }
    fn allows_actions_before_extraction(&self) -> bool {
        true
    }
    fn allows_baseline_read(&self) -> bool {
        self.inner.allows_baseline_read()
    }
    fn extraction_schema(&self) -> Option<&SemanticExtractionSchema> {
        self.inner.extraction_schema()
    }
    fn model_action_operations(
        &self,
        node: &SemanticNode,
        observation: &SemanticObservation,
    ) -> Result<SemanticOperations, AgentWorkFailure> {
        self.inner.model_action_operations(node, observation)
    }
    fn evaluate(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        if self.restored.is_some() {
            return Err(AgentWorkFailure::Contract);
        }
        let checkpoint = self.checkpoint(observation)?;
        let progress = self.inner.evaluate(observation)?;
        if self.initial.is_none() {
            if progress != AgentWorkTaskProgress::Continue {
                return Err(AgentWorkFailure::Contract);
            }
            self.initial = Some(checkpoint);
        } else if self.changed.is_none() {
            if progress != AgentWorkTaskProgress::Continue
                || !Self::fresh_successor(
                    self.initial.as_ref().ok_or(AgentWorkFailure::Contract)?,
                    &checkpoint,
                )
            {
                return Err(AgentWorkFailure::Contract);
            }
            self.changed = Some(checkpoint);
        } else {
            if progress != AgentWorkTaskProgress::ReadyForExtraction
                || !Self::fresh_successor(
                    self.changed.as_ref().ok_or(AgentWorkFailure::Contract)?,
                    &checkpoint,
                )
            {
                return Err(AgentWorkFailure::Contract);
            }
            self.restored = Some(checkpoint);
        }
        Ok(progress)
    }
    fn assess(
        &self,
        action: &SemanticPreparedAction,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        self.inner.assess(action)
    }
    fn attest_account(
        &self,
        context: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        self.inner.attest_account(context, now)
    }
    fn accept_extraction(
        &mut self,
        result: &SemanticExtractionResult<'_>,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        let expected = self.restored.as_ref().ok_or(AgentWorkFailure::Contract)?;
        let original = self.update.from.as_str();
        let [field] = result.fields() else {
            return Err(AgentWorkFailure::Contract);
        };
        let SemanticExtractedValue::Text(value) = field.value() else {
            return Err(AgentWorkFailure::Contract);
        };
        let Some([source]) = result.sources(value.source_span()) else {
            return Err(AgentWorkFailure::Contract);
        };
        let fragment = source.fragment();
        let provenance = fragment.provenance();
        if field.name() != EXTRACTION_FIELD
            || value.as_str() != original
            || result.observation() != expected.observation
            || provenance.observation() != expected.observation
            || provenance.observation_generation() != expected.generation
            || provenance.frame() != &expected.frame
            || provenance.invocation() != expected.invocation
            || provenance.snapshot() != expected.snapshot
            || provenance.reference() != expected.reference
            || fragment.role() != SemanticRole::Textbox
            || fragment.field() != SemanticReadField::TextValue
            || !matches!(fragment.content(), SemanticReadContent::ValuePreview(preview) if preview.text() == original && !preview.truncated() && preview.source_bytes() == original.len())
        {
            return Err(AgentWorkFailure::Contract);
        }
        self.inner.accept_extraction(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_directory_becomes_the_navigation_prefix() {
        assert_eq!(path_prefix("/p/Page-abc"), "/p/");
        assert_eq!(path_prefix("/"), "/");
        assert_eq!(path_prefix("/a/b/c/"), "/a/b/c/");
        assert_eq!(path_prefix("/%2e%2e/x"), "/");
        assert_eq!(path_prefix("/../x"), "/");
    }

    #[test]
    fn field_update_task_requires_an_authenticated_owned_context() {
        let origin = SemanticOrigin::parse("https://example.test").unwrap();
        let update = WorkFieldUpdateV1 {
            field: None,
            from: "Before".into(),
            to: "After".into(),
        };
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            1_u128.into(),
            ContextKind::Owned,
        );
        assert!(FieldUpdateTask::try_new(
            identity,
            origin.clone(),
            AgentAccountScope::Anonymous,
            update.clone()
        )
        .is_err());
        let task = FieldUpdateTask::try_new(
            identity,
            origin,
            AgentAccountScope::Authenticated(AgentAccountId::generate()),
            update,
        )
        .unwrap();
        assert_eq!(task.expected(), "Before");
        assert!(task.extraction_schema().is_some());
    }
}
