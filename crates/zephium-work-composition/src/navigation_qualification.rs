//! One closed public two-document witness shared by actual-app and standalone
//! qualifications. Never a shipping site adapter or alternate controller.

use std::{cell::Cell, io::Write as _};
use zephium_agent_controller::*;
use zephium_agentic::*;

#[path = "navigation_qualification_input.rs"]
mod input;
#[cfg(feature = "discovery-qualification")]
pub(crate) use input::load_configured_request;
pub use input::load_request;
#[path = "navigation_qualification_observer.rs"]
mod observer;
pub use observer::{cancel, ApplicationObserver, ApplicationReport};

const ORIGIN: &str = "https://react.dev";
const MAX_HOPS: u64 = 1;
const DISCOVERY: bool = false;

pub(crate) struct QualificationDefinition {
    pub task_name: &'static str,
    pub retention_name: &'static str,
    pub objective: &'static str,
    pub task: fn(ContextIdentity) -> Result<Box<dyn AgentWorkTask>, AgentWorkFailure>,
    pub authority: fn(AgentPlanNodeAuthority) -> Result<AgentPlanNodeAuthority, &'static str>,
    pub configure_request: fn(crate::TrustedWorkRequest) -> crate::TrustedWorkRequest,
    pub max_hops: u64,
    pub inspection: bool,
    pub verify_owned: fn(&SemanticOwnedExtractionResult) -> bool,
}
const DEFINITION: QualificationDefinition = QualificationDefinition {
    task_name: "react-one-hop-v1",
    retention_name: "stateless",
    objective: OBJECTIVE,
    task,
    authority,
    configure_request,
    max_hops: MAX_HOPS,
    inspection: DISCOVERY,
    verify_owned,
};

impl QualificationDefinition {
    pub(crate) fn configuration_diagnostic(&self) -> String {
        format!("work-application-navigation-config: provider=OpenAIResponses model=gpt-5.6-luna retention={} task={}", self.retention_name, self.task_name)
    }
}

/// Content-free configuration of the exact statically selected witness.
pub fn configuration_diagnostic() -> String {
    DEFINITION.configuration_diagnostic()
}

fn authority(authority: AgentPlanNodeAuthority) -> Result<AgentPlanNodeAuthority, &'static str> {
    Ok(authority)
}
fn configure_request(request: crate::TrustedWorkRequest) -> crate::TrustedWorkRequest {
    request
}
const DESTINATION: &str = "https://react.dev/learn/your-first-component";
const DEPARTURE: &str = "Quick Start";
const ARRIVAL: &str = "Your First Component";
pub const OBJECTIVE: &str = "Read the current React page heading. If the current page is Quick Start, navigate exactly once to https://react.dev/learn/your-first-component. Once the current page is Your First Component, call extract with initial scope and trusted schema 1, returning destination_heading as the complete exact Your First Component heading text with exactly one current heading-text citation. The host independently verifies departure, exact navigation, fresh arrival and citation. Use only current delivered source evidence; do not extract on Quick Start, repeat navigation, follow redirects, act, read, expand a subtree, paraphrase or modify anything.";

#[derive(Clone, Copy)]
struct Document {
    context: ContextJoin,
    observation: SemanticObservationId,
}

struct ReactNavigationTask {
    identity: ContextIdentity,
    origin: SemanticOrigin,
    target: ContextNavigationTarget,
    extraction: AgentWorkExtractionTask,
    departure: Option<Document>,
    arrival: Option<Document>,
    samples: Cell<[Option<AgentContextAccountBinding>; 2]>,
    complete: bool,
}

impl ReactNavigationTask {
    fn new(identity: ContextIdentity) -> Result<Self, AgentWorkFailure> {
        if identity.kind() != ContextKind::Owned {
            return Err(AgentWorkFailure::Contract);
        }
        Ok(Self {
            identity,
            origin: SemanticOrigin::parse(ORIGIN).map_err(|_| AgentWorkFailure::Contract)?,
            target: ContextNavigationTarget::parse(DESTINATION)
                .map_err(|_| AgentWorkFailure::Contract)?,
            extraction: AgentWorkExtractionTask::try_new(
                vec![SemanticExtractionFieldSchema::try_text(
                    "destination_heading".into(),
                    true,
                    64,
                )
                .map_err(|_| AgentWorkFailure::Contract)?],
                AgentAccountScope::Anonymous,
            )?
            .with_source_roles(
                SemanticReadRoleSelection::try_new(&[SemanticRole::Heading])
                    .map_err(|_| AgentWorkFailure::Contract)?,
            ),
            departure: None,
            arrival: None,
            samples: Cell::new([None; 2]),
            complete: false,
        })
    }
}

pub fn task(context: ContextIdentity) -> Result<Box<dyn AgentWorkTask>, AgentWorkFailure> {
    Ok(Box::new(ReactNavigationTask::new(context)?))
}

fn successor(prior: ContextJoin, next: ContextJoin) -> bool {
    prior.identity() == next.identity()
        && prior.context_generation() == next.context_generation()
        && prior.cancellation_generation() == next.cancellation_generation()
        && prior.frame() == FrameId::MAIN
        && next.frame() == FrameId::MAIN
        && prior.navigation_epoch().get().checked_add(1) == Some(next.navigation_epoch().get())
        && prior.frame_generation().get().checked_add(1) == Some(next.frame_generation().get())
}

impl AgentWorkTask for ReactNavigationTask {
    fn navigation_target(&self) -> Option<&ContextNavigationTarget> {
        Some(&self.target)
    }
    fn extraction_schema(&self) -> Option<&SemanticExtractionSchema> {
        self.extraction.extraction_schema()
    }
    fn assess(
        &self,
        _: &SemanticPreparedAction,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        Err(AgentWorkFailure::Contract)
    }

    fn evaluate(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        self.arrival = None;
        let context = observation.request().context();
        let [frame] = observation.frames() else {
            return Err(AgentWorkFailure::Contract);
        };
        if self.complete
            || context.identity() != self.identity
            || context.frame() != FrameId::MAIN
            || !matches!(observation.request().scope(), SemanticScope::Initial)
            || frame.frame().origin() != &self.origin
            || frame.frame().context() != context
            || frame.frame().frame() != FrameId::MAIN
        {
            return Err(AgentWorkFailure::Contract);
        }
        let expected = if let Some(departure) = self.departure {
            if !successor(departure.context, context)
                || departure.observation == observation.request().id()
            {
                return Err(AgentWorkFailure::Contract);
            }
            ARRIVAL
        } else {
            if self.samples.get()[0].map(AgentContextAccountBinding::context) != Some(context) {
                return Err(AgentWorkFailure::Contract);
            }
            DEPARTURE
        };
        let matches = frame
            .nodes()
            .iter()
            .filter(|node| {
                node.role() == SemanticRole::Heading
                    && node.sensitivity() == SemanticSensitivity::Public
                    && node.name().is_some_and(|name| name.as_str() == expected)
            })
            .count();
        if matches != 1 {
            return Err(AgentWorkFailure::Contract);
        }
        let document = Document {
            context,
            observation: observation.request().id(),
        };
        let (phase, progress) = if self.departure.is_none() {
            self.departure = Some(document);
            ("departure", AgentWorkTaskProgress::ReadyForNavigation)
        } else {
            self.arrival = Some(document);
            ("arrival", AgentWorkTaskProgress::ReadyForExtraction)
        };
        writeln!(std::io::stdout().lock(), "work-navigation-observation: phase={phase}; exact_heading=true; nodes={}; completeness={:?}; content=redacted", frame.nodes().len(), frame.completeness())
            .map_err(|_| AgentWorkFailure::Contract)?;
        Ok(progress)
    }

    fn attest_account(
        &self,
        context: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        if self.complete || context.identity() != self.identity || context.frame() != FrameId::MAIN
        {
            return Err(AgentWorkFailure::Contract);
        }
        if self
            .arrival
            .is_some_and(|arrival| arrival.context != context)
        {
            return Err(AgentWorkFailure::Contract);
        }
        let mut samples = self.samples.get();
        if samples[1].is_some_and(|sample| sample.context() != context) {
            return Err(AgentWorkFailure::Contract);
        }
        if let Some(sample) = samples
            .into_iter()
            .flatten()
            .find(|sample| sample.context() == context)
        {
            return Ok(sample);
        }
        let index = if let Some(prior) = samples[0] {
            if samples[1].is_some()
                || self.arrival.map(|arrival| arrival.context) != Some(context)
                || !successor(prior.context(), context)
                || now < prior.observed_at()
            {
                return Err(AgentWorkFailure::Contract);
            }
            1
        } else {
            0
        };
        // Only this closed qualifier's fresh isolated application profile (or
        // the standalone host's new ephemeral profile) is in scope: no imported
        // session, credential input, action or auth step.
        // Its isolated anonymous basis is sampled once per verified document;
        // cached samples never renew. This is not login/account detection.
        let sample = AgentContextAccountBinding::new(
            AgentAccountAttestationId::generate(),
            context,
            AgentAccountScope::Anonymous,
            now,
        );
        samples[index] = Some(sample);
        self.samples.set(samples);
        Ok(sample)
    }

    fn accept_extraction(
        &mut self,
        result: &SemanticExtractionResult<'_>,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        let arrival = self.arrival.ok_or(AgentWorkFailure::Contract)?;
        if self.complete
            || result.observation() != arrival.observation
            || result.schema().get() != 1
            || self.samples.get()[1].map(AgentContextAccountBinding::context)
                != Some(arrival.context)
        {
            return Err(AgentWorkFailure::Contract);
        }
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
        if field.name() != "destination_heading"
            || value.as_str() != ARRIVAL
            || fragment.role() != SemanticRole::Heading
            || fragment.provenance().context() != arrival.context
            || fragment.provenance().observation() != arrival.observation
            || fragment.provenance().origin() != &self.origin
            || !matches!(fragment.content(), SemanticReadContent::Text(text) if text.as_str() == ARRIVAL)
        {
            return Err(AgentWorkFailure::Contract);
        }
        self.extraction.accept_extraction(result)?;
        self.complete = true;
        Ok(AgentWorkTaskProgress::Complete)
    }
}

pub fn verify_owned(result: &SemanticOwnedExtractionResult) -> bool {
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
        && field.name() == "destination_heading"
        && result.schema().get() == 1
        && sources.next().is_none()
        && value.as_str() == ARRIVAL
        && source.role == SemanticRole::Heading
        && source.sensitivity == SemanticSensitivity::Public
        && source.frame.frame() == FrameId::MAIN
        && SemanticOrigin::parse(ORIGIN).as_ref() == Ok(source.frame.origin())
        && matches!(&source.content, SemanticOwnedReadContent::Text(text) if text == ARRIVAL)
}

#[cfg(test)]
#[path = "navigation_qualification_tests.rs"]
mod tests;
