//! Closed public catalog-to-product qualification, never a shipping site adapter.
use std::{cell::Cell, io::Write as _};
use zephium_agent_controller::*;
use zephium_agentic::*;

const ORIGIN: &str = "https://demo.vercel.store";
const DEPARTURE: &str = "https://demo.vercel.store/";
const DESTINATION: &str = "https://demo.vercel.store/product/acme-geometric-circles-t-shirt";
// Reviewed public data and immutable template sources are pinned in the evidence
// ledger. A changed product/price/markup is a refusal, not an adaptive fixture.
const PRODUCT: &str = "Acme Circles T-Shirt";
const PRICE: &str = "$20.00 USD";
const MATERIAL: &str = "60% combed ringspun cotton/40% polyester jersey tee.";
const CATALOG_LINK: &str = "Acme Circles T-Shirt $20.00 USD";
const FIELDS: [(&str, &str, SemanticRole, SemanticReadField); 3] = [
    (
        "product_name",
        PRODUCT,
        SemanticRole::Heading,
        SemanticReadField::AccessibleName,
    ),
    (
        "displayed_price",
        PRICE,
        SemanticRole::Paragraph,
        SemanticReadField::VisibleText,
    ),
    (
        "material_description",
        MATERIAL,
        SemanticRole::Paragraph,
        SemanticReadField::VisibleText,
    ),
];
pub(super) const OBJECTIVE: &str = "Read the public Vercel demo commerce catalog and the exact Acme Circles T-Shirt product page. At the catalog checkpoint, verify the product link includes its displayed price, then navigate exactly once to https://demo.vercel.store/product/acme-geometric-circles-t-shirt. Only at the product checkpoint call extract with initial scope and trusted schema 1. Return three complete exact-copy text fields: product_name from the product heading accessible name, displayed_price from the standalone price paragraph including currency, and material_description from the material-composition paragraph. Cite exactly one distinct current source per field. The host independently verifies departure, fresh arrival and all three values. Use only current delivered evidence. Do not infer availability, compare catalog prices, select a variant, add to cart, search, follow redirects, navigate again, read, expand a subtree, paraphrase or modify anything. Earlier document evidence is retired after navigation.";

pub(super) fn route() -> Result<AgentNavigationRoute, AgentWorkFailure> {
    AgentNavigationRoute::try_new(
        ContextNavigationTarget::parse(DEPARTURE).map_err(|_| AgentWorkFailure::Contract)?,
        vec![ContextNavigationTarget::parse(DESTINATION).map_err(|_| AgentWorkFailure::Contract)?],
    )
    .map_err(|_| AgentWorkFailure::Contract)
}

#[derive(Clone, Copy)]
struct Document {
    context: ContextJoin,
    observation: SemanticObservationId,
}

#[derive(Clone, Copy)]
struct Arrival {
    document: Document,
    references: [SemanticReferenceId; 3],
}

struct CommerceProductTask {
    identity: ContextIdentity,
    origin: SemanticOrigin,
    route: AgentNavigationRoute,
    extraction: AgentWorkExtractionTask,
    departure: Option<Document>,
    arrival: Option<Arrival>,
    samples: Cell<[Option<AgentContextAccountBinding>; 2]>,
    complete: bool,
}

impl CommerceProductTask {
    fn new(identity: ContextIdentity) -> Result<Self, AgentWorkFailure> {
        if identity.kind() != ContextKind::Owned {
            return Err(AgentWorkFailure::Contract);
        }
        Ok(Self {
            identity,
            origin: SemanticOrigin::parse(ORIGIN).map_err(|_| AgentWorkFailure::Contract)?,
            route: route()?,
            extraction: AgentWorkExtractionTask::try_new(
                FIELDS
                    .iter()
                    .map(|(name, value, _, _)| {
                        SemanticExtractionFieldSchema::try_text((*name).into(), true, value.len())
                            .map_err(|_| AgentWorkFailure::Contract)
                    })
                    .collect::<Result<_, _>>()?,
                AgentAccountScope::Anonymous,
            )?
            .with_source_roles(
                SemanticReadRoleSelection::try_new(&[
                    SemanticRole::Heading,
                    SemanticRole::Paragraph,
                ])
                .map_err(|_| AgentWorkFailure::Contract)?,
            ),
            departure: None,
            arrival: None,
            samples: Cell::new([None; 2]),
            complete: false,
        })
    }
}

pub(super) fn task(identity: ContextIdentity) -> Result<Box<dyn AgentWorkTask>, AgentWorkFailure> {
    Ok(Box::new(CommerceProductTask::new(identity)?))
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

fn unique_source(
    snapshot: &SemanticSnapshot,
    role: SemanticRole,
    field: SemanticReadField,
    expected: &str,
) -> Option<SemanticReferenceId> {
    let mut matches = snapshot.nodes().iter().filter(|node| {
        let text = match field {
            SemanticReadField::AccessibleName => node.name(),
            SemanticReadField::VisibleText => node.text(),
            _ => None,
        };
        node.role() == role
            && node.sensitivity() == SemanticSensitivity::Public
            && text.is_some_and(|text| text.as_str() == expected)
    });
    let reference = matches.next()?.reference();
    matches.next().is_none().then_some(reference)
}

impl AgentWorkTask for CommerceProductTask {
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
        // A rejected replacement observation revokes previously verified arrival.
        self.arrival = None;
        let context = observation.request().context();
        let [frame] = observation.frames() else {
            return Err(AgentWorkFailure::Contract);
        };
        let catalog = unique_source(
            frame,
            SemanticRole::Link,
            SemanticReadField::AccessibleName,
            CATALOG_LINK,
        );
        let facts = FIELDS.map(|(_, value, role, field)| unique_source(frame, role, field, value));
        let phase = if self.departure.is_none() {
            "catalog"
        } else {
            "product"
        };
        // Report closed predicates even on refusal, not page text or guesses
        // about why a public deployment changed. This grants no task progress.
        writeln!(std::io::stdout().lock(), "work-commerce-observation: phase={phase}; catalog_match={}; product_matches={:?}; nodes={}; completeness={:?}; frame_boundaries={}; content=redacted", catalog.is_some(), facts.map(|reference| reference.is_some()), frame.nodes().len(), frame.completeness(), observation.frame_boundaries().len()).map_err(|_| AgentWorkFailure::Contract)?;
        if self.complete
            || context.identity() != self.identity
            || context.frame() != FrameId::MAIN
            || !matches!(observation.request().scope(), SemanticScope::Initial)
            || frame.frame().origin() != &self.origin
            || frame.frame().context() != context
            || frame.frame().frame() != FrameId::MAIN
            || frame.completeness() != SemanticCompleteness::Complete
            || !observation.frame_boundaries().is_empty()
        {
            return Err(AgentWorkFailure::Contract);
        }
        let document = Document {
            context,
            observation: observation.request().id(),
        };
        if let Some(departure) = self.departure {
            if !successor(departure.context, context)
                || departure.observation == document.observation
            {
                return Err(AgentWorkFailure::Contract);
            }
            let [Some(name), Some(price), Some(material)] = facts else {
                return Err(AgentWorkFailure::Contract);
            };
            self.arrival = Some(Arrival {
                document,
                references: [name, price, material],
            });
            Ok(AgentWorkTaskProgress::ReadyForExtraction)
        } else {
            if self.samples.get()[0].map(AgentContextAccountBinding::context) != Some(context)
                || catalog.is_none()
            {
                return Err(AgentWorkFailure::Contract);
            }
            self.departure = Some(document);
            Ok(AgentWorkTaskProgress::ReadyForNavigation)
        }
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
                .arrival
                .is_some_and(|arrival| arrival.document.context != context)
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
                || self.arrival.map(|arrival| arrival.document.context) != Some(context)
                || !successor(prior.context(), context)
                || now < prior.observed_at()
            {
                return Err(AgentWorkFailure::Contract);
            }
            1
        } else {
            0
        };
        // Only the runner's fresh isolated ephemeral profile with no imported
        // session/auth/actions is eligible. This is not account/login detection.
        // Samples are minted once per independently verified document, not renewed.
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
            || result.observation() != arrival.document.observation
            || result.schema().get() != 1
            || result.fields().len() != FIELDS.len()
            || self.samples.get()[1].map(AgentContextAccountBinding::context)
                != Some(arrival.document.context)
        {
            return Err(AgentWorkFailure::Contract);
        }
        for (index, (name, expected, role, source_field)) in FIELDS.into_iter().enumerate() {
            let field = &result.fields()[index];
            let SemanticExtractedValue::Text(value) = field.value() else {
                return Err(AgentWorkFailure::Contract);
            };
            let Some([source]) = result.sources(value.source_span()) else {
                return Err(AgentWorkFailure::Contract);
            };
            let fragment = source.fragment();
            let provenance = fragment.provenance();
            if field.name() != name
                || value.as_str() != expected
                || fragment.role() != role
                || fragment.field() != source_field
                || provenance.context() != arrival.document.context
                || provenance.observation() != arrival.document.observation
                || provenance.origin() != &self.origin
                || provenance.reference() != arrival.references[index]
                || provenance.sensitivity() != SemanticSensitivity::Public
                || provenance.trust() != SemanticTrust::UntrustedPage
                || !matches!(fragment.content(), SemanticReadContent::Text(text) if text.as_str() == expected)
            {
                return Err(AgentWorkFailure::Contract);
            }
        }
        self.extraction.accept_extraction(result)?;
        self.complete = true;
        Ok(AgentWorkTaskProgress::Complete)
    }
}

pub(super) fn verify_owned(result: &SemanticOwnedExtractionResult) -> bool {
    if result.trust() != SemanticExtractionTrust::ModelMapped
        || result.schema().get() != 1
        || result.fields().len() != FIELDS.len()
    {
        return false;
    }
    let mut frame = None;
    let mut references = Vec::with_capacity(3);
    for (index, (name, expected, role, source_field)) in FIELDS.into_iter().enumerate() {
        let field = &result.fields()[index];
        let SemanticExtractedValue::Text(value) = field.value() else {
            return false;
        };
        let Some(mut sources) = result.sources(value.source_span()) else {
            return false;
        };
        let Some(source) = sources.next() else {
            return false;
        };
        if field.name() != name
            || value.as_str() != expected
            || sources.next().is_some()
            || source.role != role
            || source.field != source_field
            || source.sensitivity != SemanticSensitivity::Public
            || source.trust != SemanticTrust::UntrustedPage
            || source.frame.frame() != FrameId::MAIN
            || source.frame.context().identity().kind() != ContextKind::Owned
            || SemanticOrigin::parse(ORIGIN).as_ref() != Ok(source.frame.origin())
            || frame.is_some_and(|frame| frame != &source.frame)
            || references.contains(&source.reference)
            || !matches!(&source.content, SemanticOwnedReadContent::Text(text) if text == expected)
        {
            return false;
        }
        frame = Some(&source.frame);
        references.push(source.reference);
    }
    true
}

#[cfg(test)]
#[path = "work_commerce_tests.rs"]
mod tests;
