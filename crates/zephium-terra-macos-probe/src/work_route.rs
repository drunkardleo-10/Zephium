//! One closed public three-document qualifier, never a shipping site adapter.
use std::{cell::Cell, io::Write as _};
use zephium_agent_controller::*;
use zephium_agentic::*;

const ORIGIN: &str = "https://react.dev";
const URLS: [&str; 3] = [
    "https://react.dev/learn",
    "https://react.dev/learn/your-first-component",
    "https://react.dev/learn/importing-and-exporting-components",
];
const HEADINGS: [&str; 3] = [
    "Quick Start",
    "Your First Component",
    "Importing and Exporting Components",
];
const MIDDLE_DEPARTURE: &str = "Components: UI building blocks";
#[cfg(target_os = "macos")]
pub(super) const OBJECTIVE: &str = "Follow exactly this ordered public React guide route using current delivered headings. On Quick Start, navigate to https://react.dev/learn/your-first-component. On Your First Component, verify the distinct Components: UI building blocks heading is also present, then navigate to https://react.dev/learn/importing-and-exporting-components. Only on Importing and Exporting Components call extract with initial scope and trusted schema 1, returning destination_heading as the complete exact Importing and Exporting Components heading with exactly one current heading-text citation. The host independently verifies all three document checkpoints. Never skip or repeat a checkpoint, extract early, follow redirects, act, read, expand a subtree, paraphrase or modify anything. Earlier document evidence is retired after each navigation.";

pub(super) fn route() -> Result<AgentNavigationRoute, AgentWorkFailure> {
    AgentNavigationRoute::try_new(
        ContextNavigationTarget::parse(URLS[0]).map_err(|_| AgentWorkFailure::Contract)?,
        URLS[1..]
            .iter()
            .map(|url| ContextNavigationTarget::parse(url))
            .collect::<Result<_, _>>()
            .map_err(|_| AgentWorkFailure::Contract)?,
    )
    .map_err(|_| AgentWorkFailure::Contract)
}

#[derive(Clone, Copy)]
struct Document {
    context: ContextJoin,
    observation: SemanticObservationId,
}

struct ReactRouteTask {
    identity: ContextIdentity,
    origin: SemanticOrigin,
    route: AgentNavigationRoute,
    extraction: AgentWorkExtractionTask,
    documents: [Option<Document>; 3],
    next_document: usize,
    arrival: Option<Document>,
    samples: Cell<[Option<AgentContextAccountBinding>; 3]>,
    complete: bool,
}

impl ReactRouteTask {
    fn new(identity: ContextIdentity) -> Result<Self, AgentWorkFailure> {
        if identity.kind() != ContextKind::Owned {
            return Err(AgentWorkFailure::Contract);
        }
        Ok(Self {
            identity,
            origin: SemanticOrigin::parse(ORIGIN).map_err(|_| AgentWorkFailure::Contract)?,
            route: route()?,
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
            documents: [None; 3],
            next_document: 0,
            arrival: None,
            samples: Cell::new([None; 3]),
            complete: false,
        })
    }
}

pub(super) fn task(context: ContextIdentity) -> Result<Box<dyn AgentWorkTask>, AgentWorkFailure> {
    Ok(Box::new(ReactRouteTask::new(context)?))
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

impl AgentWorkTask for ReactRouteTask {
    fn navigation_route(&self) -> Option<&AgentNavigationRoute> {
        Some(&self.route)
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
        let index = self.next_document;
        let expected = HEADINGS.get(index).ok_or(AgentWorkFailure::Contract)?;
        if index == 0 {
            if self.samples.get()[0].map(AgentContextAccountBinding::context) != Some(context) {
                return Err(AgentWorkFailure::Contract);
            }
        } else {
            let prior = self.documents[index - 1].ok_or(AgentWorkFailure::Contract)?;
            if !successor(prior.context, context)
                || self
                    .documents
                    .iter()
                    .flatten()
                    .any(|document| document.observation == observation.request().id())
            {
                return Err(AgentWorkFailure::Contract);
            }
        }
        let count = |heading: &str| {
            frame
                .nodes()
                .iter()
                .filter(|node| {
                    node.role() == SemanticRole::Heading
                        && node.sensitivity() == SemanticSensitivity::Public
                        && node.name().is_some_and(|name| name.as_str() == heading)
                })
                .count()
        };
        // Arrival alone is insufficient at the intermediate checkpoint. A
        // distinct task-authored section must independently permit departure.
        if count(expected) != 1 || (index == 1 && count(MIDDLE_DEPARTURE) != 1) {
            return Err(AgentWorkFailure::Contract);
        }
        let document = Document {
            context,
            observation: observation.request().id(),
        };
        self.documents[index] = Some(document);
        self.next_document += 1;
        let progress = if index == 2 {
            self.arrival = Some(document);
            AgentWorkTaskProgress::ReadyForExtraction
        } else {
            AgentWorkTaskProgress::ReadyForNavigation
        };
        writeln!(std::io::stdout().lock(), "work-route-observation: checkpoint={index}; exact_heading=true; departure_predicate={}; nodes={}; completeness={:?}; content=redacted",
            index < 2, frame.nodes().len(), frame.completeness()).map_err(|_| AgentWorkFailure::Contract)?;
        Ok(progress)
    }
    fn attest_account(
        &self,
        context: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        if self.complete
            || context.identity() != self.identity
            || context.frame() != FrameId::MAIN
            || self
                .documents
                .iter()
                .flatten()
                .last()
                .is_some_and(|document| document.context != context)
        {
            return Err(AgentWorkFailure::Contract);
        }
        let mut samples = self.samples.get();
        if let Some(sample) = samples
            .iter()
            .flatten()
            .find(|sample| sample.context() == context)
        {
            return Ok(*sample);
        }
        let index = samples.iter().flatten().count();
        if index >= samples.len() {
            return Err(AgentWorkFailure::Contract);
        }
        if index > 0 {
            let prior = samples[index - 1].ok_or(AgentWorkFailure::Contract)?;
            if self.documents[index].map(|document| document.context) != Some(context)
                || !successor(prior.context(), context)
                || now < prior.observed_at()
            {
                return Err(AgentWorkFailure::Contract);
            }
        } else if self.next_document != 0 {
            return Err(AgentWorkFailure::Contract);
        }
        // This closed runner constructs one fresh isolated ephemeral profile,
        // imports no session and performs no auth/actions. Sample that anonymous
        // basis once per verified document; never renew a cached timestamp.
        // This is neither login detection nor authenticated-session authority.
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
            || self.next_document != 3
            || result.observation() != arrival.observation
            || result.schema().get() != 1
            || self.samples.get()[2].map(AgentContextAccountBinding::context)
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
            || value.as_str() != HEADINGS[2]
            || fragment.role() != SemanticRole::Heading
            || fragment.provenance().context() != arrival.context
            || fragment.provenance().observation() != arrival.observation
            || fragment.provenance().origin() != &self.origin
            || !matches!(fragment.content(), SemanticReadContent::Text(text) if text.as_str() == HEADINGS[2])
        {
            return Err(AgentWorkFailure::Contract);
        }
        self.extraction.accept_extraction(result)?;
        self.complete = true;
        Ok(AgentWorkTaskProgress::Complete)
    }
}

pub(super) fn verify_owned(result: &SemanticOwnedExtractionResult) -> bool {
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
        && value.as_str() == HEADINGS[2]
        && source.role == SemanticRole::Heading
        && source.sensitivity == SemanticSensitivity::Public
        && source.frame.frame() == FrameId::MAIN
        && SemanticOrigin::parse(ORIGIN).as_ref() == Ok(source.frame.origin())
        && matches!(&source.content, SemanticOwnedReadContent::Text(text) if text == HEADINGS[2])
}

#[cfg(test)]
#[path = "work_route_tests.rs"]
mod tests;
