//! Closed, read-only real-storefront brief. Page/model text never grants authority.
use super::*;

const TARGET: &str = "https://shop.pimoroni.com/products/raspberry-pi-pico-2";
const ORIGIN: &str = "https://shop.pimoroni.com";
const PRODUCT: &str = "Raspberry Pi Pico 2";
const FIELDS: [(&str, SemanticRole, SemanticReadField, usize); 2] = [
    (
        "product_name",
        SemanticRole::Heading,
        SemanticReadField::AccessibleName,
        64,
    ),
    (
        "technical_summary",
        SemanticRole::Paragraph,
        SemanticReadField::VisibleText,
        512,
    ),
];
pub(super) const OBJECTIVE: &str = "Prepare a source-backed product brief for Raspberry Pi Pico 2 from the current Pimoroni listing. Extract the exact product heading into product_name and the complete short introductory technical-description paragraph about the board and RP2350 into technical_summary. Use trusted schema 1 and initial scope. You may inspect the acknowledged baseline if useful. Copy each complete source without paraphrase and cite its exact source. Ignore shop navigation, reviews, cart, variants and recommendations. Do not navigate, change page state, sign in, subscribe, add to cart or buy anything. If the required evidence is absent, do not invent it.";

pub(super) fn document() -> Result<(Option<FixtureServer>, ContextNavigationTarget), &'static str> {
    Ok((
        None,
        ContextNavigationTarget::parse(TARGET).map_err(|_| "public_target")?,
    ))
}
pub(super) fn configured() -> RetainedProbeTrace {
    RetainedProbeTrace::ConfiguredPublic
}
#[derive(Clone)]
struct Source {
    reference: SemanticReferenceId,
    value: String,
}
#[derive(Clone)]
pub(super) struct Expected {
    observation: SemanticObservationId,
    frame: SemanticFrameJoin,
    invocation: SemanticInvocationId,
    snapshot: SemanticSnapshotGeneration,
    sources: [Source; 2],
}
#[derive(Clone, Copy)]
pub(super) struct Sample {
    nodes: u16,
    complete: bool,
    current: bool,
    boundaries: usize,
    matched: [bool; 2],
}
impl Sample {
    pub(super) fn trace(self) -> RetainedProbeTrace {
        RetainedProbeTrace::PublicObservation {
            nodes: self.nodes,
            complete: self.complete,
            current_document: self.current,
            frame_boundaries: self.boundaries,
            product_title: self.matched[0],
            product_summary: self.matched[1],
        }
    }
}
pub(super) struct Task {
    extraction: AgentWorkExtractionTask,
    context: ContextIdentity,
    expected: Arc<Mutex<Option<Expected>>>,
    sample: Arc<Mutex<Option<Sample>>>,
    accepted: bool,
}
impl Task {
    pub(super) fn new(
        context: ContextIdentity,
        expected: Arc<Mutex<Option<Expected>>>,
        sample: Arc<Mutex<Option<Sample>>>,
    ) -> Result<Self, &'static str> {
        if context.kind() != ContextKind::Owned {
            return Err("public_context");
        }
        let fields = FIELDS
            .into_iter()
            .map(|(name, _, _, limit)| {
                SemanticExtractionFieldSchema::try_text(name.into(), true, limit)
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "schema")?;
        let roles =
            SemanticReadRoleSelection::try_new(&[SemanticRole::Heading, SemanticRole::Paragraph])
                .map_err(|_| "source_roles")?;
        Ok(Self {
            extraction: AgentWorkExtractionTask::try_new(fields, AgentAccountScope::Anonymous)
                .map_err(|_| "task")?
                .with_baseline_read()
                .with_source_roles(roles),
            context,
            expected,
            sample,
            accepted: false,
        })
    }
}
fn matches_source(index: usize, node: &SemanticNode) -> Option<&str> {
    let (_, role, _, limit) = FIELDS[index];
    if node.role() != role {
        return None;
    }
    let value = if index == 0 {
        node.name()?
    } else {
        node.text()?
    }
    .as_str();
    let relevant = if index == 0 {
        value == PRODUCT
    } else {
        value.starts_with("A low cost,") && value.contains("RP2350")
    };
    (relevant && !value.is_empty() && value.len() <= limit).then_some(value)
}
impl AgentWorkTask for Task {
    fn allows_baseline_read(&self) -> bool {
        true
    }
    fn extraction_schema(&self) -> Option<&SemanticExtractionSchema> {
        self.extraction.extraction_schema()
    }
    fn evaluate(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        let snapshot = (observation.frames().len() == 1).then(|| &observation.frames()[0]);
        let matched = [0, 1].map(|index| {
            snapshot.is_some_and(|snapshot| {
                snapshot
                    .nodes()
                    .iter()
                    .filter(|node| matches_source(index, node).is_some())
                    .count()
                    == 1
            })
        });
        let sample = Sample {
            nodes: observation.node_count(),
            complete: snapshot
                .is_some_and(|snapshot| snapshot.completeness() == SemanticCompleteness::Complete),
            current: observation.request().context().identity() == self.context
                && snapshot.is_some_and(|snapshot| {
                    snapshot
                        .frame()
                        .origin()
                        .as_url()
                        .origin()
                        .ascii_serialization()
                        == ORIGIN
                }),
            boundaries: observation.frame_boundaries().len(),
            matched,
        };
        {
            let mut saved = self.sample.lock().map_err(|_| AgentWorkFailure::Contract)?;
            if saved.is_some() {
                return Err(AgentWorkFailure::Contract);
            }
            *saved = Some(sample);
        }
        if !sample.current || !sample.complete || sample.boundaries != 0 || matched != [true; 2] {
            return Err(AgentWorkFailure::Contract);
        }
        let snapshot = snapshot.ok_or(AgentWorkFailure::Contract)?;
        let sources = [0, 1].map(|index| {
            let node = snapshot
                .nodes()
                .iter()
                .find(|node| matches_source(index, node).is_some())
                .ok_or(AgentWorkFailure::Contract)?;
            Ok(Source {
                reference: node.reference(),
                value: matches_source(index, node)
                    .ok_or(AgentWorkFailure::Contract)?
                    .into(),
            })
        });
        let [first, second] = sources;
        let mut expected = self
            .expected
            .lock()
            .map_err(|_| AgentWorkFailure::Contract)?;
        if expected.is_some() {
            return Err(AgentWorkFailure::Contract);
        }
        *expected = Some(Expected {
            observation: observation.request().id(),
            frame: snapshot.frame().clone(),
            invocation: snapshot.invocation(),
            snapshot: snapshot.generation(),
            sources: [first?, second?],
        });
        Ok(AgentWorkTaskProgress::Continue)
    }
    fn assess(
        &self,
        _: &SemanticPreparedAction,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        Err(AgentWorkFailure::Contract)
    }
    fn attest_account(
        &self,
        context: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        if context.identity() != self.context {
            return Err(AgentWorkFailure::Contract);
        }
        // The original anonymous sample retains its age; no timestamp renewal.
        self.extraction.attest_account(context, now)
    }
    fn accept_extraction(
        &mut self,
        result: &SemanticExtractionResult<'_>,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        let expected = self
            .expected
            .lock()
            .map_err(|_| AgentWorkFailure::Contract)?;
        let expected = expected.as_ref().ok_or(AgentWorkFailure::Contract)?;
        if self.accepted
            || result.observation() != expected.observation
            || result.fields().len() != FIELDS.len()
        {
            return Err(AgentWorkFailure::Contract);
        }
        for (index, field) in result.fields().iter().enumerate() {
            let SemanticExtractedValue::Text(value) = field.value() else {
                return Err(AgentWorkFailure::Contract);
            };
            let Some([source]) = result.sources(value.source_span()) else {
                return Err(AgentWorkFailure::Contract);
            };
            let fragment = source.fragment();
            let (name, role, source_field, _) = FIELDS[index];
            if field.name() != name
                || value.as_str() != expected.sources[index].value
                || fragment.role() != role
                || fragment.field() != source_field
                || fragment.provenance().reference() != expected.sources[index].reference
                || fragment.provenance().frame() != &expected.frame
                || fragment.provenance().invocation() != expected.invocation
                || fragment.provenance().snapshot() != expected.snapshot
                || !matches!(fragment.content(), SemanticReadContent::Text(text) if text.as_str() == value.as_str())
            {
                return Err(AgentWorkFailure::Contract);
            }
        }
        let result = self.extraction.accept_extraction(result)?;
        self.accepted = true;
        Ok(result)
    }
}
pub(super) fn verify_owned(result: &SemanticOwnedExtractionResult, expected: &Expected) -> bool {
    if result.observation() != expected.observation || result.fields().len() != FIELDS.len() {
        return false;
    }
    result.fields().iter().enumerate().all(|(index, field)| {
        let SemanticExtractedValue::Text(value) = field.value() else { return false; };
        let Some(mut sources) = result.sources(value.source_span()) else { return false; };
        let Some(source) = sources.next() else { return false; };
        let (name, role, source_field, _) = FIELDS[index];
        field.name() == name && sources.next().is_none()
            && value.as_str() == expected.sources[index].value
            && source.reference == expected.sources[index].reference
            && source.frame == expected.frame && source.role == role && source.field == source_field
            && source.invocation == expected.invocation && source.snapshot == expected.snapshot
            && matches!(&source.content, SemanticOwnedReadContent::Text(text) if text == value.as_str())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const SUMMARY: &str = "A low cost, test-only technical summary about RP2350.";
    fn context() -> ContextJoin {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            AgentWorkProfileId::generate(),
            ContextKind::Owned,
        );
        let mut registry = ContextRegistry::new();
        registry
            .reserve(
                identity,
                ContextCapabilities::try_new(ContextKind::Owned, &[ContextCapability::Observe])
                    .unwrap(),
            )
            .unwrap();
        let op = registry
            .begin_context(identity.id(), ContextOperationId::new(1).unwrap())
            .unwrap();
        registry
            .settle_construction(identity.id(), op, ContextSettlement::Applied)
            .unwrap();
        registry.join(identity.id()).unwrap()
    }
    fn nodes() -> String {
        format!(
            r#"{{"k":1,"r":"heading","l":1,"n":"{PRODUCT}"}},{{"k":2,"r":"paragraph","t":"{SUMMARY}"}},{{"k":3,"r":"button","n":"Add to cart"}}"#
        )
    }
    fn observation(
        context: ContextJoin,
        id: u64,
        origin: &str,
        completeness: &str,
        nodes: &str,
    ) -> SemanticObservation {
        observation_with_stamp(context, id, origin, completeness, nodes, (1, 1))
    }
    fn observation_with_stamp(
        context: ContextJoin,
        id: u64,
        origin: &str,
        completeness: &str,
        nodes: &str,
        stamp: (u64, u64),
    ) -> SemanticObservation {
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            SemanticOrigin::parse(origin).unwrap(),
            SemanticFrameTrust::SameOrigin,
        )
        .unwrap();
        let wire = format!(
            r#"{{"v":1,"i":{},"g":{},"c":"{completeness}","n":[{nodes}]}}"#,
            stamp.0, stamp.1
        );
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                SemanticInvocationId::new(stamp.0).unwrap(),
                frame,
                SemanticSnapshotGeneration::new(stamp.1).unwrap(),
            ),
            wire.as_bytes(),
        )
        .unwrap();
        let boundaries = snapshot
            .nodes()
            .iter()
            .filter(|node| node.role() == SemanticRole::FrameBoundary)
            .map(|node| node.reference())
            .collect::<Vec<_>>();
        let mut assembler = SemanticObservationAssembler::new(
            SemanticObservationRequest::initial(
                SemanticObservationId::new(id).unwrap(),
                context,
                SemanticObservationBudget::INITIAL_FILTERED,
            ),
            snapshot,
        )
        .unwrap();
        for boundary in boundaries {
            assembler
                .mark_frame_unsupported(
                    FrameId::MAIN,
                    boundary,
                    SemanticFrameUnsupported::PolicyBlocked,
                )
                .unwrap();
        }
        assembler.finish().unwrap()
    }
    fn task(context: ContextJoin) -> Task {
        Task::new(
            context.identity(),
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(None)),
        )
        .unwrap()
    }
    fn mapped<'a>(
        schema: &SemanticExtractionSchema,
        observation: &'a SemanticObservation,
        values: [&str; 2],
        citations: [&str; 2],
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
        let delivery = encode_semantic_read(
            &read,
            SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE,
        )
        .unwrap()
        .admit_conservative_utf8(
            &SemanticTokenizerRevision::try_new("public-brief-test-v1".into()).unwrap(),
        )
        .unwrap()
        .settle_delivery(SemanticModelDeliverySettlement::Committed)
        .unwrap();
        let fields = FIELDS
            .iter()
            .enumerate()
            .map(|(index, (name, _, _, _))| {
                format!(
                    r#"{{"name":"{name}","value":{{"k":"text","value":"{}","sources":[{}]}}}}"#,
                    values[index], citations[index]
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let wire = format!(r#"{{"v":1,"schema":1,"fields":[{fields}]}}"#);
        extract_semantic_read(
            schema,
            &read,
            &delivery,
            SemanticReadSensitivityLimit::PublicOnly,
            wire.as_bytes(),
        )
        .unwrap()
    }
    #[test]
    fn public_brief_has_one_fixed_target_and_no_effect_or_expansion_authority() {
        let (fixture, target) = document().unwrap();
        assert!(fixture.is_none());
        assert_eq!(target.as_url().as_str(), TARGET);
        let task = task(context());
        assert!(task.allows_baseline_read());
        assert!(!task.allows_actions_before_extraction());
        assert!(!task.allows_subtree_extraction());
        assert!(task.navigation_target().is_none());
        assert!(task.navigation_route().is_none());
        assert_eq!(task.extraction_schema().unwrap().fields().len(), 2);
        let borrowed = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            AgentWorkProfileId::generate(),
            ContextKind::BorrowedTab,
        );
        assert!(Task::new(
            borrowed,
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(None))
        )
        .is_err());
    }
    #[test]
    fn public_brief_requires_unique_complete_current_exact_role_sources() {
        let context = context();
        let good = observation(context, 1, ORIGIN, "complete", &nodes());
        let mut ready = task(context);
        assert_eq!(ready.evaluate(&good), Ok(AgentWorkTaskProgress::Continue));
        assert!(ready.evaluate(&good).is_err());
        for (origin, completeness, nodes) in [
            (ORIGIN, "node_limit", nodes()),
            ("https://example.test", "complete", nodes()),
            (
                ORIGIN,
                "complete",
                nodes().replace(PRODUCT, "Other product"),
            ),
            (
                ORIGIN,
                "complete",
                nodes().replace("RP2350", "Different chip"),
            ),
            (
                ORIGIN,
                "complete",
                nodes()
                    .replace("paragraph", "link")
                    .replace("\"t\":", "\"n\":"),
            ),
            (
                ORIGIN,
                "complete",
                format!(r#"{},{{"k":4,"r":"paragraph","t":"{SUMMARY}"}}"#, nodes()),
            ),
            (
                ORIGIN,
                "complete",
                format!(
                    r#"{},{{"k":4,"r":"heading","l":1,"n":"{PRODUCT}"}}"#,
                    nodes()
                ),
            ),
            (
                ORIGIN,
                "complete",
                format!(r#"{},{{"k":4,"r":"frame_boundary"}}"#, nodes()),
            ),
        ] {
            let mut refused = task(context);
            assert!(refused
                .evaluate(&observation(context, 1, origin, completeness, &nodes))
                .is_err());
            assert!(refused.sample.lock().unwrap().is_some());
            assert!(refused.expected.lock().unwrap().is_none());
        }
        let mut foreign = task(self::context());
        assert!(foreign.evaluate(&good).is_err());
        assert!(foreign
            .attest_account(context, AgentPolicyInstant::from_millis(10))
            .is_err());
    }
    #[test]
    fn public_brief_accepts_exact_sources_once_and_owned_consumer_rechecks_them() {
        let context = context();
        let observation = observation(context, 1, ORIGIN, "complete", &nodes());
        let mut task = task(context);
        let schema = task.extraction_schema().unwrap().clone();
        let result = mapped(
            &schema,
            &observation,
            [PRODUCT, SUMMARY],
            [r#""@r1""#, r#""@r2""#],
        );
        assert!(task.accept_extraction(&result).is_err());
        task.evaluate(&observation).unwrap();
        assert_eq!(
            task.accept_extraction(&result),
            Ok(AgentWorkTaskProgress::Complete)
        );
        assert!(task.accept_extraction(&result).is_err());
        let expected = task.expected.lock().unwrap().clone().unwrap();
        assert!(verify_owned(&result.into_owned().unwrap(), &expected));
    }
    #[test]
    fn public_brief_refuses_invention_multiple_sources_and_stale_or_substituted_citations() {
        let context = context();
        let initial = observation(context, 1, ORIGIN, "complete", &nodes());
        let mut task = task(context);
        task.evaluate(&initial).unwrap();
        let schema = task.extraction_schema().unwrap().clone();
        let expected = task.expected.lock().unwrap().clone().unwrap();
        for (values, citations) in [
            (
                [PRODUCT, "Invented technical summary"],
                [r#""@r1""#, r#""@r2""#],
            ),
            ([PRODUCT, SUMMARY], [r#""@r1","@r2""#, r#""@r2""#]),
            ([PRODUCT, SUMMARY], [r#""@r2""#, r#""@r1""#]),
        ] {
            let result = mapped(&schema, &initial, values, citations);
            assert!(task.accept_extraction(&result).is_err());
            assert!(!verify_owned(&result.into_owned().unwrap(), &expected));
        }
        for (context, id, origin, nodes) in [
            (context, 2, ORIGIN, nodes()),
            (self::context(), 1, ORIGIN, nodes()),
            (context, 1, "https://example.test", nodes()),
            (
                context,
                1,
                ORIGIN,
                format!(r#"{{"k":5,"r":"button","n":"unrelated"}},{}"#, nodes()),
            ),
            (context, 1, ORIGIN, nodes().replace("\"t\":", "\"n\":")),
        ] {
            let observation = observation(context, id, origin, "complete", &nodes);
            let result = mapped(
                &schema,
                &observation,
                [PRODUCT, SUMMARY],
                [r#""@r1""#, r#""@r2""#],
            );
            assert!(task.accept_extraction(&result).is_err());
            assert!(!verify_owned(&result.into_owned().unwrap(), &expected));
        }
        for stamp in [(2, 1), (1, 2)] {
            let observation =
                observation_with_stamp(context, 1, ORIGIN, "complete", &nodes(), stamp);
            let result = mapped(
                &schema,
                &observation,
                [PRODUCT, SUMMARY],
                [r#""@r1""#, r#""@r2""#],
            );
            assert!(task.accept_extraction(&result).is_err());
            assert!(!verify_owned(&result.into_owned().unwrap(), &expected));
        }
        assert!(!task.accepted);
    }
}
