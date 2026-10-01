//! Closed local preparation witness through ordinary retained Work admission.
//! This module is release-excluded. The task's local effect attestation applies
//! only to the script-free fixture owned here, never an arbitrary website.
pub use crate::navigation_qualification::ApplicationReport;
use std::{io::Write as _, time::Instant};
use zephium_agent_controller::*;
use zephium_agentic::*;
use zephium_app::{RetainedWorkHandle, RetainedWorkPhase};

#[path = "retained_action_qualification_input.rs"]
mod input;
pub use input::load_request;

const INITIAL_VALUE: &str = "Unprepared";
const VALUE: &str = "Ready for review";
const LABEL: &str = "Draft";
pub const OBJECTIVE: &str = "Prepare the local Draft field with the exact value Ready for review. This explicitly approved fixture has no submission, scripts, autosave or external effects. Use the currently observed field reference and the bounded fill action with local_write, immediate wait, target_value_matches_input verification and a 2000 ms settle ceiling. After the host verifies the change from fresh browser state, extract schema 1 with draft_value as the exact resulting field value, citing only that field's current value-preview evidence. Do not cite its label or the instruction, navigate, click, submit, run code, or change any other field.";

pub fn configuration_diagnostic() -> String {
    "work-retained-action-config: entry=admit_retained_trusted_work provider=OpenAIResponses model=gpt-5.6-luna retention=inspectable-public task=local-draft-v1 effect=approved_local_write".into()
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
struct LocalTask {
    inner: AgentWorkFormExtractionTask,
    fixture: Option<FixtureServer>,
    initial: Option<Checkpoint>,
    completed: Option<Checkpoint>,
}
impl LocalTask {
    fn new(
        identity: ContextIdentity,
        origin: SemanticOrigin,
        fixture: FixtureServer,
    ) -> Result<Self, AgentWorkFailure> {
        Ok(Self {
            inner: AgentWorkFormTask::try_new_local_preparation(
                identity,
                origin,
                AgentAccountScope::Anonymous,
                vec![AgentWorkFormPhase::try_new(vec![AgentWorkFormGoal::fill(
                    Some(LABEL.into()),
                    VALUE.into(),
                )?])?],
            )?
            .with_extraction(vec![SemanticExtractionFieldSchema::try_text(
                "draft_value".into(),
                true,
                64,
            )
            .map_err(|_| AgentWorkFailure::Contract)?])?,
            fixture: Some(fixture),
            initial: None,
            completed: None,
        })
    }
}
impl AgentWorkTask for LocalTask {
    fn allows_actions_before_extraction(&self) -> bool {
        true
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
    fn evaluate(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        if !self.fixture.as_ref().is_some_and(FixtureServer::is_healthy) || self.completed.is_some()
        {
            return Err(AgentWorkFailure::Contract);
        }
        let [frame] = observation.frames() else {
            return Err(AgentWorkFailure::Contract);
        };
        let mut fields = frame.nodes().iter().filter(|node| {
            node.role() == SemanticRole::Textbox
                && node.name().is_some_and(|name| name.as_str() == LABEL)
        });
        let node = fields.next().ok_or(AgentWorkFailure::Contract)?;
        if fields.next().is_some()
            || !observation.frame_boundaries().is_empty()
            || frame.completeness() != SemanticCompleteness::Complete
        {
            return Err(AgentWorkFailure::Contract);
        }
        let checkpoint = Checkpoint {
            observation: observation.request().id(),
            generation: observation.request().generation(),
            frame: frame.frame().clone(),
            invocation: frame.invocation(),
            snapshot: frame.generation(),
            reference: node.reference(),
        };
        let expected = if self.initial.is_none() {
            INITIAL_VALUE
        } else {
            VALUE
        };
        if !matches!(node.value(), Some(SemanticValueSummary::Text(value)) if value.preview().text() == expected && value.len() == expected.len())
        {
            return Err(AgentWorkFailure::Contract);
        }
        let progress = self.inner.evaluate(observation)?;
        if let Some(initial) = &self.initial {
            if checkpoint.observation == initial.observation
                || checkpoint.frame != initial.frame
                || checkpoint.invocation == initial.invocation
                || checkpoint.snapshot == initial.snapshot
                || progress != AgentWorkTaskProgress::ReadyForExtraction
            {
                return Err(AgentWorkFailure::Contract);
            }
            self.completed = Some(checkpoint);
        } else {
            if progress != AgentWorkTaskProgress::Continue {
                return Err(AgentWorkFailure::Contract);
            }
            self.initial = Some(checkpoint);
        }
        Ok(progress)
    }
    fn accept_extraction(
        &mut self,
        result: &SemanticExtractionResult<'_>,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        let expected = self.completed.as_ref().ok_or(AgentWorkFailure::Contract)?;
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
        if field.name() != "draft_value"
            || value.as_str() != VALUE
            || result.observation() != expected.observation
            || provenance.observation() != expected.observation
            || provenance.observation_generation() != expected.generation
            || provenance.frame() != &expected.frame
            || provenance.invocation() != expected.invocation
            || provenance.snapshot() != expected.snapshot
            || provenance.reference() != expected.reference
            || fragment.role() != SemanticRole::Textbox
            || fragment.field() != SemanticReadField::TextValue
            || !matches!(fragment.content(), SemanticReadContent::ValuePreview(preview) if preview.text() == VALUE && !preview.truncated() && preview.source_bytes() == VALUE.len())
        {
            return Err(AgentWorkFailure::Contract);
        }
        let progress = self.inner.accept_extraction(result)?;
        self.fixture
            .take()
            .ok_or(AgentWorkFailure::Contract)?
            .shutdown()
            .map_err(|_| AgentWorkFailure::Contract)?;
        Ok(progress)
    }
}

pub fn cancel(view: &RetainedWorkHandle) -> bool {
    view.stop()
}

/// Observes the production event and durable terminal projections. Physical
/// callback-return proof belongs to the native/app owners, not this observer.
#[derive(Default)]
pub struct ApplicationObserver {
    report: ApplicationReport,
    sequence: u64,
    failed: bool,
    proposed: u8,
    active: u8,
    verified: u8,
}
impl ApplicationObserver {
    pub fn report(&self) -> ApplicationReport {
        self.report
    }
    pub fn healthy(&self) -> bool {
        !self.failed
    }
    fn observe_kind(&mut self, kind: AgentWorkEventKind) {
        match kind {
            AgentWorkEventKind::ModelSettled {
                input_tokens,
                output_tokens,
                cost_micro_usd,
                ..
            } => {
                for (total, delta) in [
                    (&mut self.report.model_calls, 1),
                    (&mut self.report.input_tokens, input_tokens),
                    (&mut self.report.output_tokens, output_tokens),
                    (&mut self.report.cost_micro_usd, cost_micro_usd),
                ] {
                    if let Some(sum) = total.checked_add(delta) {
                        *total = sum;
                    } else {
                        self.failed = true;
                    }
                }
            }
            AgentWorkEventKind::ToolProposed(AgentBrowserToolKind::Act) => {
                self.proposed = self.proposed.saturating_add(1);
                self.failed |= self.proposed != 1;
            }
            AgentWorkEventKind::ActionActive => {
                self.active = self.active.saturating_add(1);
                self.failed |= self.proposed != 1 || self.active != 1 || self.verified != 0;
            }
            AgentWorkEventKind::Verified => {
                self.verified = self.verified.saturating_add(1);
                self.failed |= self.active != 1 || self.verified != 1;
            }
            AgentWorkEventKind::ToolProposed(AgentBrowserToolKind::Extract) => {
                self.failed |= self.verified != 1;
            }
            AgentWorkEventKind::ToolProposed(_)
            | AgentWorkEventKind::NeedsHuman(_)
            | AgentWorkEventKind::Recovery => self.failed = true,
            _ => {}
        }
    }
    fn action_complete(&self) -> bool {
        !self.failed && self.proposed == 1 && self.active == 1 && self.verified == 1
    }
    pub fn poll(
        &mut self,
        view: &RetainedWorkHandle,
        resource_failure: impl FnOnce() -> Option<zephium_engine::WorkResourceFailureCause>,
    ) -> Option<ApplicationReport> {
        let snapshot = view.snapshot();
        for _ in 0..256 {
            let Some(event) = view.take_event() else {
                break;
            };
            self.failed |= event.run() != snapshot.run
                || self.sequence.checked_add(1) != Some(event.sequence());
            self.sequence = event.sequence();
            self.observe_kind(event.kind());
            self.failed |= writeln!(
                std::io::stdout().lock(),
                "work-retained-action-event: sequence={} phase={:?} wall_ms={} content=redacted",
                event.sequence(),
                event.kind(),
                event.elapsed_millis()
            )
            .is_err();
        }
        if !matches!(
            snapshot.phase,
            RetainedWorkPhase::Terminal | RetainedWorkPhase::Uncertain | RetainedWorkPhase::Refused
        ) {
            return None;
        }
        let resource_failure = resource_failure();
        self.report.durable_terminal_verified = snapshot.record.is_some_and(|record| {
            record.disposition() == AgentWorkDisposition::Succeeded
                && record.debt() == AgentWorkDebt::NONE
                && record.key()[16..] == snapshot.run.bytes()
        });
        self.report.source_mapping_verified = view
            .take_extraction()
            .is_some_and(|result| verify_owned(&result) && view.take_extraction().is_none());
        self.report.accepted = self.action_complete()
            && resource_failure.is_none()
            && snapshot.phase == RetainedWorkPhase::Terminal
            && snapshot.failure.is_none()
            && snapshot.persistence_failure.is_none()
            && self.report.durable_terminal_verified
            && self.report.source_mapping_verified
            && (1..=8).contains(&self.report.model_calls)
            && self
                .report
                .input_tokens
                .saturating_add(self.report.output_tokens)
                <= 100_000
            && self.report.cost_micro_usd <= 100_000;
        self.report.accepted &= writeln!(std::io::stdout().lock(), "work-retained-action-terminal: phase={:?} failure={:?} persistence_failure={:?} resource_failure={resource_failure:?} proposed={} active={} verified={} source_mapping={} durable={} accepted={} content=redacted", snapshot.phase, snapshot.failure, snapshot.persistence_failure, self.proposed, self.active, self.verified, self.report.source_mapping_verified, self.report.durable_terminal_verified, self.report.accepted).is_ok();
        Some(self.report)
    }
}

fn verify_owned(result: &SemanticOwnedExtractionResult) -> bool {
    let [field] = result.fields() else {
        return false;
    };
    let SemanticExtractedValue::Text(value) = field.value() else {
        return false;
    };
    let Some(mut sources) = result.sources(value.source_span()) else {
        return false;
    };
    let Some(source) = sources.next() else {
        return false;
    };
    result.trust() == SemanticExtractionTrust::ModelMapped
        && result.schema().get() == 1
        && field.name() == "draft_value"
        && value.as_str() == VALUE
        && sources.next().is_none()
        && source.observation == result.observation()
        && source.observation_generation == result.observation_generation()
        && source.frame.frame() == FrameId::MAIN
        && source.role == SemanticRole::Textbox
        && source.field == SemanticReadField::TextValue
        && source.sensitivity == SemanticSensitivity::Public
        && matches!(&source.content, SemanticOwnedReadContent::ValuePreview { text, source_bytes, truncated } if text == VALUE && *source_bytes == VALUE.len() && !truncated)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context() -> ContextJoin {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            1_u128.into(),
            ContextKind::Owned,
        );
        let mut registry = ContextRegistry::new();
        registry
            .reserve(
                identity,
                ContextCapabilities::try_new(
                    ContextKind::Owned,
                    &[ContextCapability::Observe, ContextCapability::Act],
                )
                .unwrap(),
            )
            .unwrap();
        let operation = registry
            .begin_context(identity.id(), ContextOperationId::new(1).unwrap())
            .unwrap();
        registry
            .settle_construction(identity.id(), operation, ContextSettlement::Applied)
            .unwrap();
        registry.join(identity.id()).unwrap()
    }
    fn observation(
        context: ContextJoin,
        origin: &SemanticOrigin,
        id: u64,
        value: &str,
    ) -> SemanticObservation {
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            origin.clone(),
            SemanticFrameTrust::SameOrigin,
        )
        .unwrap();
        let wire = format!(
            r#"{{"v":1,"i":{id},"g":{id},"c":"complete","n":[{{"k":1,"r":"textbox","n":"Draft","s":64,"o":2,"v":{{"k":"text","value":"{value}"}},"b":{{"x":10,"y":20,"w":120,"h":30}}}},{{"k":2,"r":"paragraph","t":"Ready for review"}}]}}"#
        );
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(id).unwrap(),
                frame,
                SemanticSnapshotGeneration::new(id).unwrap(),
            ),
            wire.as_bytes(),
        )
        .unwrap();
        SemanticObservationAssembler::new(
            SemanticObservationRequest::initial(
                SemanticObservationId::new(id).unwrap(),
                context,
                SemanticObservationBudget::INITIAL_FILTERED,
            ),
            snapshot,
        )
        .unwrap()
        .finish()
        .unwrap()
    }
    fn task(context: ContextJoin) -> (LocalTask, SemanticOrigin) {
        let fixture = FixtureServer::start().unwrap();
        let origin = SemanticOrigin::parse(&fixture.url(FixtureRoute::RetainedLocalForm)).unwrap();
        let task = LocalTask::new(context.identity(), origin.clone(), fixture).unwrap();
        task.attest_account(context, AgentPolicyInstant::from_millis(1))
            .unwrap();
        (task, origin)
    }
    fn mapped<'a>(
        schema: &SemanticExtractionSchema,
        observation: &'a SemanticObservation,
        field: SemanticReadField,
    ) -> SemanticExtractionResult<'a> {
        let read = read_selected_semantic_observation(
            observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(30),
            SemanticReadSensitivityLimit::PublicOnly,
            SemanticReadBudget::STANDARD,
            schema.source_roles(),
        )
        .unwrap();
        let source = read
            .fragments()
            .iter()
            .find(|fragment| {
                fragment.field() == field
                    && (field != SemanticReadField::VisibleText
                        || fragment.role() == SemanticRole::Paragraph)
            })
            .unwrap()
            .id()
            .get();
        let delivery = encode_semantic_read(
            &read,
            SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
        )
        .unwrap()
        .admit_conservative_utf8(
            &SemanticTokenizerRevision::try_new("local-action-test-v1".into()).unwrap(),
        )
        .unwrap()
        .settle_delivery(SemanticModelDeliverySettlement::Committed)
        .unwrap();
        let output = format!(
            r#"{{"v":1,"schema":1,"fields":[{{"name":"draft_value","value":{{"k":"text","value":"Ready for review","sources":["@r{source}"]}}}}]}}"#
        );
        extract_semantic_read(
            schema,
            &read,
            &delivery,
            SemanticReadSensitivityLimit::PublicOnly,
            output.as_bytes(),
        )
        .unwrap()
    }
    #[test]
    fn exact_fresh_field_value_is_required_even_when_other_text_matches() {
        let context = context();
        let (mut task, origin) = task(context);
        let initial = observation(context, &origin, 1, INITIAL_VALUE);
        let fresh = observation(context, &origin, 2, VALUE);
        assert_eq!(
            task.evaluate(&initial).unwrap(),
            AgentWorkTaskProgress::Continue
        );
        assert_eq!(
            task.evaluate(&fresh).unwrap(),
            AgentWorkTaskProgress::ReadyForExtraction
        );
        let schema = task.extraction_schema().unwrap().clone();
        assert!(!verify_owned(
            &mapped(&schema, &fresh, SemanticReadField::VisibleText)
                .into_owned()
                .unwrap()
        ));
        assert!(verify_owned(
            &mapped(&schema, &fresh, SemanticReadField::TextValue)
                .into_owned()
                .unwrap()
        ));
        assert!(task
            .accept_extraction(&mapped(&schema, &fresh, SemanticReadField::VisibleText))
            .is_err());
        assert!(task
            .accept_extraction(&mapped(&schema, &initial, SemanticReadField::TextValue))
            .is_err());
        assert_eq!(
            task.accept_extraction(&mapped(&schema, &fresh, SemanticReadField::TextValue))
                .unwrap(),
            AgentWorkTaskProgress::Complete
        );
    }
    #[test]
    fn already_satisfied_initial_state_and_reused_checkpoint_are_refused() {
        let context = context();
        let (mut already, origin) = task(context);
        assert!(already
            .evaluate(&observation(context, &origin, 1, VALUE))
            .is_err());
        let (mut reused, origin) = task(context);
        reused
            .evaluate(&observation(context, &origin, 1, INITIAL_VALUE))
            .unwrap();
        assert!(reused
            .evaluate(&observation(context, &origin, 1, VALUE))
            .is_err());
    }
    #[test]
    fn success_requires_one_executed_verified_action_in_order() {
        for sequence in [
            vec![],
            vec![AgentWorkEventKind::Verified],
            vec![AgentWorkEventKind::ActionActive],
            vec![AgentWorkEventKind::ToolProposed(
                AgentBrowserToolKind::Extract,
            )],
        ] {
            let mut observer = ApplicationObserver::default();
            for event in sequence {
                observer.observe_kind(event);
            }
            assert!(!observer.action_complete());
        }
        let mut observer = ApplicationObserver::default();
        for event in [
            AgentWorkEventKind::ToolProposed(AgentBrowserToolKind::Act),
            AgentWorkEventKind::ActionActive,
            AgentWorkEventKind::Verified,
            AgentWorkEventKind::ToolProposed(AgentBrowserToolKind::Extract),
        ] {
            observer.observe_kind(event);
        }
        assert!(observer.action_complete());
        observer.observe_kind(AgentWorkEventKind::ActionActive);
        assert!(!observer.action_complete());
    }
}
