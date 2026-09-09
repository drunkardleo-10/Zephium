//! Trusted, bounded form transitions; not a plan interpreter or submit tool.
use zephium_agentic::*;

use crate::{AgentWorkExtractionTask, AgentWorkFailure, AgentWorkTask, AgentWorkTaskProgress};

#[cfg(test)]
#[path = "work_form_tests.rs"]
mod tests;

const MAX_GOALS: usize = 8;

/// One explicit trusted field/value requirement. Content is private and never
/// diagnostic data. An absent name requires a unique field of the given kind;
/// a name is an exact accessible name, never HTML or a selector expression.
#[derive(Clone)]
pub struct AgentWorkFormGoal {
    name: Option<SemanticActionText>,
    prior_value: Option<SemanticActionText>,
    value: SemanticActionText,
    kind: SemanticActionKind,
}
impl AgentWorkFormGoal {
    /// Requires the exact complete value of a unique textbox or searchbox.
    /// Empty text explicitly means clearing the field. Values are bounded to
    /// the independently observable preview ceiling, not silently truncated.
    pub fn fill(name: Option<String>, value: String) -> Result<Self, AgentWorkFailure> {
        Self::new(name, None, value, SemanticActionKind::Fill)
    }
    /// Requires an exact, complete observed transition on a textbox or
    /// searchbox. The prior value is both a target discriminator and a
    /// fail-closed precondition: a field containing any third value grants no
    /// action. Observing the destination value is already satisfied.
    pub fn fill_transition(
        name: Option<String>,
        prior_value: String,
        value: String,
    ) -> Result<Self, AgentWorkFailure> {
        Self::new(name, Some(prior_value), value, SemanticActionKind::Fill)
    }
    /// Requires the uniquely named descendant option of a unique combobox.
    /// Listboxes/multiselect, submission and navigation are not this contract.
    pub fn select(name: Option<String>, option: String) -> Result<Self, AgentWorkFailure> {
        Self::new(name, None, option, SemanticActionKind::Select)
    }
    fn new(
        name: Option<String>,
        prior_value: Option<String>,
        value: String,
        kind: SemanticActionKind,
    ) -> Result<Self, AgentWorkFailure> {
        if name
            .as_ref()
            .is_some_and(|name| name.is_empty() || name.len() > MAX_SEMANTIC_NAME_BYTES)
            || prior_value
                .as_ref()
                .is_some_and(|value| value.len() > MAX_SEMANTIC_VALUE_PREVIEW_BYTES)
            || prior_value.as_ref().is_some_and(|prior| prior == &value)
            || value.len() > MAX_SEMANTIC_VALUE_PREVIEW_BYTES
            || (kind == SemanticActionKind::Select
                && (prior_value.is_some()
                    || value.is_empty()
                    || value.len() > MAX_SEMANTIC_NAME_BYTES))
        {
            return Err(AgentWorkFailure::Contract);
        }
        Ok(Self {
            name: name
                .map(SemanticActionText::try_new)
                .transpose()
                .map_err(|_| AgentWorkFailure::Contract)?,
            prior_value: prior_value
                .map(SemanticActionText::try_new)
                .transpose()
                .map_err(|_| AgentWorkFailure::Contract)?,
            value: SemanticActionText::try_new(value).map_err(|_| AgentWorkFailure::Contract)?,
            kind,
        })
    }
    fn matches(&self, node: &SemanticNode) -> bool {
        let role = match self.kind {
            SemanticActionKind::Fill => {
                matches!(node.role(), SemanticRole::Textbox | SemanticRole::Searchbox)
            }
            SemanticActionKind::Select => node.role() == SemanticRole::Combobox,
            _ => false,
        };
        role && self.name.as_ref().is_none_or(|name| {
            node.name()
                .is_some_and(|actual| actual.as_str() == name.as_str())
        }) && self.prior_value.as_ref().is_none_or(|prior| {
            exact_text_value(node, prior) || exact_text_value(node, &self.value)
        })
    }
}

/// A conjunction of explicit field requirements. The model may satisfy them
/// in either order; a later phase is unavailable until every current goal is
/// independently observed. Repeat a field in the next phase to preserve it.
#[derive(Clone)]
pub struct AgentWorkFormPhase {
    goals: Vec<AgentWorkFormGoal>,
}
impl AgentWorkFormPhase {
    /// Rejects empty, oversized or overlapping field descriptions. Runtime
    /// resolution independently refuses duplicate/missing semantic matches.
    pub fn try_new(goals: Vec<AgentWorkFormGoal>) -> Result<Self, AgentWorkFailure> {
        if goals.is_empty() || goals.len() > MAX_GOALS {
            return Err(AgentWorkFailure::Contract);
        }
        for (index, goal) in goals.iter().enumerate() {
            if goals[..index].iter().any(|prior| {
                prior.kind == goal.kind
                    && (prior.name.is_none() || goal.name.is_none() || prior.name == goal.name)
            }) {
                return Err(AgentWorkFailure::Contract);
            }
        }
        Ok(Self { goals })
    }
}

struct Binding {
    goal: usize,
    target: SemanticReferenceId,
    option: Option<SemanticReferenceId>,
}
struct Baseline {
    observation: SemanticObservationId,
    generation: SemanticObservationGeneration,
    frame: SemanticFrameJoin,
    snapshot: SemanticSnapshotGeneration,
    invocation: SemanticInvocationId,
}

/// Production task predicate for explicit bounded form-state transitions.
/// The caller supplies trusted context, account, origin and goals, separately
/// from the model objective and approved manifest. Neither page nor model
/// content can create goals. This object grants no native or policy authority.
/// It keeps at most eight goals and current opaque bindings, no page snapshots,
/// worker, timer, queue, persistence or hidden replay path.
pub struct AgentWorkFormTask {
    baseline_read: bool,
    identity: ContextIdentity,
    origin: SemanticOrigin,
    account: AgentAccountScope,
    account_sample: std::cell::Cell<Option<AgentContextAccountBinding>>,
    effect: SemanticEffectClass,
    phases: Vec<AgentWorkFormPhase>,
    phase: usize,
    baseline: Option<Baseline>,
    bindings: Vec<Binding>,
    refused: bool,
}
impl AgentWorkFormTask {
    /// Follow explicitly trusted form transitions with a source-bound result
    /// from the fresh completed page. This retains the constructor-selected
    /// goals, account and effect; it does not add submission or navigation.
    pub fn with_extraction(
        self,
        fields: Vec<SemanticExtractionFieldSchema>,
    ) -> Result<AgentWorkFormExtractionTask, AgentWorkFailure> {
        let extraction = AgentWorkExtractionTask::try_new(fields, self.account)?;
        Ok(AgentWorkFormExtractionTask {
            form: self,
            extraction,
            ready: None,
            complete: false,
        })
    }
    /// Attests local-only preparation for these exact trusted fields and page.
    /// The product must independently establish that changing these fields is
    /// local, non-submitting work; neither an action kind nor an origin allowlist
    /// proves that. Do not use this for autosave, checkout, messaging, or other
    /// remotely effective controls. Such tasks need a different trusted assessor.
    ///
    /// Admits one root-page contract with at most eight total field goals over
    /// at most eight ordered phases. This adds no budget to the run's existing
    /// turn/effect ceilings. Already-satisfied phases need no synthetic action.
    pub fn try_new_local_preparation(
        identity: ContextIdentity,
        origin: SemanticOrigin,
        account: AgentAccountScope,
        phases: Vec<AgentWorkFormPhase>,
    ) -> Result<Self, AgentWorkFailure> {
        Self::try_new(
            identity,
            origin,
            account,
            SemanticEffectClass::LocalWrite,
            phases,
        )
    }

    /// Attests an exact remotely effective field update supplied by trusted
    /// product state. The product and approved plan must independently know
    /// that these precise transitions have `ExternalWrite` semantics. A page,
    /// model, field role or origin allowlist can never select this constructor.
    /// This remains fill/select only and grants no click, submit or navigation.
    pub fn try_new_external_update(
        identity: ContextIdentity,
        origin: SemanticOrigin,
        account: AgentAccountScope,
        phases: Vec<AgentWorkFormPhase>,
    ) -> Result<Self, AgentWorkFailure> {
        Self::try_new(
            identity,
            origin,
            account,
            SemanticEffectClass::ExternalWrite,
            phases,
        )
    }

    fn try_new(
        identity: ContextIdentity,
        origin: SemanticOrigin,
        account: AgentAccountScope,
        effect: SemanticEffectClass,
        phases: Vec<AgentWorkFormPhase>,
    ) -> Result<Self, AgentWorkFailure> {
        if identity.kind() != ContextKind::Owned
            || phases.is_empty()
            || phases.len() > MAX_GOALS
            || phases.iter().map(|phase| phase.goals.len()).sum::<usize>() > MAX_GOALS
        {
            return Err(AgentWorkFailure::Contract);
        }
        Ok(Self {
            baseline_read: false,
            identity,
            origin,
            account,
            account_sample: std::cell::Cell::new(None),
            effect,
            phases,
            phase: 0,
            baseline: None,
            bindings: Vec::with_capacity(MAX_GOALS),
            refused: false,
        })
    }

    /// Allows the model to inspect bounded public details already captured in
    /// the acknowledged baseline before proposing a field action. This adds
    /// no turns, native operations, task goals or approval authority.
    pub fn with_baseline_read(mut self) -> Self {
        self.baseline_read = true;
        self
    }

    fn observe(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        let Some(snapshot) = observation.frames().first() else {
            return Err(AgentWorkFailure::Contract);
        };
        let frame = snapshot.frame();
        if observation.request().context().identity() != self.identity
            || !matches!(observation.request().scope(), SemanticScope::Initial)
            || observation.request().generation() != SemanticObservationGeneration::INITIAL
            || frame.context() != observation.request().context()
            || frame.frame() != FrameId::MAIN
            || frame.origin() != &self.origin
            || snapshot.completeness() != SemanticCompleteness::Complete
            || self.baseline.as_ref().is_some_and(|last| {
                last.frame != *frame
                    || snapshot.generation() <= last.snapshot
                    || snapshot.invocation() == last.invocation
                    || observation.request().id() == last.observation
            })
        {
            return Err(AgentWorkFailure::Contract);
        }
        // All current-phase fields must resolve before any grant is retained.
        // A complete phase can advance using this fresh state; it does not
        // certify that an action occurred or manufacture a verification proof.
        while let Some(phase) = self.phases.get(self.phase) {
            for (index, goal) in phase.goals.iter().enumerate() {
                let mut matches = snapshot
                    .nodes()
                    .iter()
                    .enumerate()
                    .filter(|(_, node)| goal.matches(node));
                let Some((target_index, target)) = matches.next() else {
                    return Err(AgentWorkFailure::Contract);
                };
                if matches.next().is_some()
                    || target.sensitivity() == SemanticSensitivity::Secret
                    || target.states().contains(SemanticState::Disabled)
                {
                    return Err(AgentWorkFailure::Contract);
                }
                let (satisfied, option) = match goal.kind {
                    SemanticActionKind::Fill => {
                        if !target.operations().contains(SemanticOperationClass::Fill) {
                            return Err(AgentWorkFailure::Contract);
                        }
                        if !matches!(target.value(), Some(SemanticValueSummary::Text(_)) | None) {
                            return Err(AgentWorkFailure::Contract);
                        }
                        let equal = exact_text_value(target, &goal.value);
                        if !equal
                            && goal
                                .prior_value
                                .as_ref()
                                .is_some_and(|prior| !exact_text_value(target, prior))
                        {
                            return Err(AgentWorkFailure::Contract);
                        }
                        (equal, None)
                    }
                    SemanticActionKind::Select => {
                        if !target.operations().contains(SemanticOperationClass::Select) {
                            return Err(AgentWorkFailure::Contract);
                        }
                        let mut option = None;
                        let mut selected = 0;
                        for (option_index, node) in snapshot.nodes().iter().enumerate() {
                            if node.role() != SemanticRole::Option
                                || !descendant(snapshot, option_index, target_index)
                            {
                                continue;
                            }
                            selected +=
                                usize::from(node.states().contains(SemanticState::Selected));
                            if node
                                .name()
                                .is_some_and(|name| name.as_str() == goal.value.as_str())
                            {
                                if option.is_some()
                                    || node.sensitivity() == SemanticSensitivity::Secret
                                    || node.states().contains(SemanticState::Disabled)
                                {
                                    return Err(AgentWorkFailure::Contract);
                                }
                                option = Some(node);
                            }
                        }
                        let option = option.ok_or(AgentWorkFailure::Contract)?;
                        if selected > 1 {
                            return Err(AgentWorkFailure::Contract);
                        }
                        (
                            option.states().contains(SemanticState::Selected),
                            Some(option.reference()),
                        )
                    }
                    _ => return Err(AgentWorkFailure::Contract),
                };
                if !satisfied {
                    self.bindings.push(Binding {
                        goal: index,
                        target: target.reference(),
                        option,
                    });
                }
            }
            if !self.bindings.is_empty() {
                break;
            }
            self.phase += 1;
        }
        self.baseline = Some(Baseline {
            observation: observation.request().id(),
            generation: observation.request().generation(),
            frame: frame.clone(),
            snapshot: snapshot.generation(),
            invocation: snapshot.invocation(),
        });
        Ok(if self.phase == self.phases.len() {
            AgentWorkTaskProgress::Complete
        } else {
            AgentWorkTaskProgress::Continue
        })
    }
}

/// Explicit trusted form goals followed by one current-page extraction. The
/// constructor's local/external classification and policy approval remain
/// mandatory. Arbitrary website fields must not be admitted through this task.
pub struct AgentWorkFormExtractionTask {
    form: AgentWorkFormTask,
    extraction: AgentWorkExtractionTask,
    ready: Option<SemanticObservationId>,
    complete: bool,
}
impl AgentWorkTask for AgentWorkFormExtractionTask {
    fn allows_actions_before_extraction(&self) -> bool {
        true
    }
    fn allows_baseline_read(&self) -> bool {
        self.form.allows_baseline_read()
    }
    fn extraction_schema(&self) -> Option<&SemanticExtractionSchema> {
        self.extraction.extraction_schema()
    }
    fn evaluate(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        if self.ready.is_some() || self.complete {
            return Err(AgentWorkFailure::Contract);
        }
        match self.form.evaluate(observation)? {
            AgentWorkTaskProgress::Complete => {
                self.ready = Some(observation.request().id());
                Ok(AgentWorkTaskProgress::ReadyForExtraction)
            }
            progress => Ok(progress),
        }
    }
    fn assess(
        &self,
        action: &SemanticPreparedAction,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        if self.ready.is_some() || self.complete {
            return Err(AgentWorkFailure::Contract);
        }
        self.form.assess(action)
    }
    fn attest_account(
        &self,
        context: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        self.form.attest_account(context, now)
    }
    fn accept_extraction(
        &mut self,
        result: &SemanticExtractionResult<'_>,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        if self.complete || self.ready != Some(result.observation()) {
            return Err(AgentWorkFailure::Contract);
        }
        let progress = self.extraction.accept_extraction(result)?;
        self.complete = true;
        Ok(progress)
    }
}

fn descendant(snapshot: &SemanticSnapshot, mut node: usize, ancestor: usize) -> bool {
    // Decoder-proven depth is bounded. Never walk page-provided links without
    // retaining that bound even if the decoder contract changes later.
    for _ in 0..MAX_SEMANTIC_DEPTH {
        let Some(parent) = snapshot.nodes().get(node).and_then(SemanticNode::parent) else {
            return false;
        };
        node = usize::from(parent);
        if node == ancestor {
            return true;
        }
    }
    false
}

fn exact_text_value(node: &SemanticNode, expected: &SemanticActionText) -> bool {
    match node.value() {
        Some(SemanticValueSummary::Text(value)) => {
            let preview = value.preview();
            !preview.truncated()
                && preview.source_bytes() == expected.len()
                && preview.text() == expected.as_str()
        }
        None => expected.is_empty(),
        _ => false,
    }
}

impl AgentWorkTask for AgentWorkFormTask {
    fn allows_baseline_read(&self) -> bool {
        self.baseline_read
    }
    fn evaluate(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        self.bindings.clear();
        if self.refused || self.phase == self.phases.len() {
            return Err(AgentWorkFailure::Contract);
        }
        let result = self.observe(observation);
        if result.is_err() {
            self.bindings.clear();
            self.refused = true;
        }
        result
    }
    fn assess(
        &self,
        action: &SemanticPreparedAction,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        let baseline = self.baseline.as_ref().ok_or(AgentWorkFailure::Contract)?;
        let phase = self
            .phases
            .get(self.phase)
            .ok_or(AgentWorkFailure::Contract)?;
        if self.refused
            || action.source_observation() != baseline.observation
            || action.source_observation_generation() != baseline.generation
            || action.frame() != &baseline.frame
            || action.bound_action().snapshot_generation() != baseline.snapshot
            || action.effect() != self.effect
        {
            return Err(AgentWorkFailure::Contract);
        }
        let binding = self
            .bindings
            .iter()
            .find(|binding| binding.target == action.target_reference())
            .ok_or(AgentWorkFailure::Contract)?;
        let goal = &phase.goals[binding.goal];
        if action.kind() != goal.kind
            || action.bound_action().option_reference() != binding.option
            || (goal.kind == SemanticActionKind::Fill && action.fill_text() != Some(&goal.value))
        {
            return Err(AgentWorkFailure::Contract);
        }
        Ok(AgentEffectAssessment::new(
            action,
            self.origin.clone(),
            self.effect,
        ))
    }
    fn attest_account(
        &self,
        context: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        if self.refused
            || context.identity() != self.identity
            || self
                .baseline
                .as_ref()
                .is_some_and(|baseline| baseline.frame.context() != context)
        {
            return Err(AgentWorkFailure::Contract);
        }
        if let Some(sample) = self.account_sample.get() {
            return if sample.context() == context {
                Ok(sample)
            } else {
                Err(AgentWorkFailure::Contract)
            };
        }
        // A constructor-supplied scope is not a live account detector. Preserve
        // its original age; only a trusted adapter can supply a new sample.
        let sample = AgentContextAccountBinding::new(
            AgentAccountAttestationId::generate(),
            context,
            self.account,
            now,
        );
        self.account_sample.set(Some(sample));
        Ok(sample)
    }
}
