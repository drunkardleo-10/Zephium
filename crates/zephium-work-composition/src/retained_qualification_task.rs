//! Frozen synthetic public task; never page-authored policy or a success claim.
use super::*;

pub(super) fn capture(retire: RetainedProbeRetire) -> RetainedProbeCapture {
    RetainedProbeCapture::BoundedReadiness(retire)
}
pub(super) fn document_policy() -> zephium_agentic::WorkBrowserDocumentPolicy {
    zephium_agentic::WorkBrowserDocumentPolicy::Exact
}

const MARKERS: [&str; 5] = [
    "Document complete",
    "Load ready",
    "Microtask ready",
    "Timer ready",
    "Animation frame ready",
];

#[derive(Clone)]
pub(super) struct Expected {
    pub observation: SemanticObservationId,
    pub reference: SemanticReferenceId,
    pub frame: SemanticFrameJoin,
}
#[derive(Clone, Copy)]
pub(super) struct Sample {
    pub nodes: u16,
    pub complete: bool,
    pub current: bool,
    pub boundaries: usize,
    pub markers: [bool; 5],
}
impl Sample {
    pub(super) fn trace(self) -> RetainedProbeTrace {
        RetainedProbeTrace::Observation {
            nodes: self.nodes,
            complete: self.complete,
            current_document: self.current,
            frame_boundaries: self.boundaries,
            markers: self.markers,
        }
    }
}
pub(super) fn configured() -> RetainedProbeTrace {
    RetainedProbeTrace::Configured
}
pub(super) fn rendering(
    engine: &WebviewEngine,
    admission: &ForegroundRenderingAdmission,
    resource: WorkBrowserResourceJoin,
) -> Option<WorkResourceRenderingProbe> {
    WorkResourceRenderingProbe::new(engine, admission, resource)
}
pub(super) fn document() -> Result<(Option<FixtureServer>, ContextNavigationTarget), &'static str> {
    let fixture = FixtureServer::start().map_err(|_| "fixture")?;
    let target = ContextNavigationTarget::parse(&fixture.url(FixtureRoute::SemanticRendering))
        .map_err(|_| "fixture_target")?;
    Ok((Some(fixture), target))
}
pub(super) struct Task {
    extraction: AgentWorkExtractionTask,
    context: ContextIdentity,
    expected: Arc<Mutex<Option<Expected>>>,
    sample: Arc<Mutex<Option<Sample>>>,
}
impl Task {
    pub(super) fn new(
        context: ContextIdentity,
        expected: Arc<Mutex<Option<Expected>>>,
        sample: Arc<Mutex<Option<Sample>>>,
    ) -> Result<Self, &'static str> {
        Ok(Self {
            context,
            expected,
            sample,
            extraction: AgentWorkExtractionTask::try_new(
                vec![
                    SemanticExtractionFieldSchema::try_text("animation_status".into(), true, 64)
                        .map_err(|_| "schema")?,
                ],
                AgentAccountScope::Anonymous,
            )
            .map_err(|_| "task")?
            .with_baseline_read(),
        })
    }
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
        let sample = Sample {
            nodes: observation.node_count(),
            complete: snapshot
                .is_some_and(|snapshot| snapshot.completeness() == SemanticCompleteness::Complete),
            current: observation.request().context().identity() == self.context,
            boundaries: observation.frame_boundaries().len(),
            markers: MARKERS.map(|marker| {
                snapshot.is_some_and(|snapshot| {
                    snapshot
                        .nodes()
                        .iter()
                        .filter(|node| {
                            node.role() == SemanticRole::Paragraph
                                && node.text().is_some_and(|text| text.as_str() == marker)
                        })
                        .count()
                        == 1
                })
            }),
        };
        {
            let mut saved = self.sample.lock().map_err(|_| AgentWorkFailure::Contract)?;
            if saved.is_some() {
                return Err(AgentWorkFailure::Contract);
            }
            *saved = Some(sample);
        }
        // Preserve content-free evidence of a refused first native snapshot.
        if !sample.current
            || !sample.complete
            || sample.boundaries != 0
            || sample.markers.into_iter().any(|present| !present)
        {
            return Err(AgentWorkFailure::Contract);
        }
        let snapshot = snapshot.ok_or(AgentWorkFailure::Contract)?;
        let node = snapshot
            .nodes()
            .iter()
            .find(|node| {
                node.role() == SemanticRole::Paragraph
                    && node.text().is_some_and(|text| text.as_str() == MARKERS[4])
            })
            .ok_or(AgentWorkFailure::Contract)?;
        let mut expected = self
            .expected
            .lock()
            .map_err(|_| AgentWorkFailure::Contract)?;
        if expected.is_some() {
            return Err(AgentWorkFailure::Contract);
        }
        *expected = Some(Expected {
            observation: observation.request().id(),
            reference: node.reference(),
            frame: snapshot.frame().clone(),
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
        if result.observation() != expected.observation {
            return Err(AgentWorkFailure::Contract);
        }
        let [field] = result.fields() else {
            return Err(AgentWorkFailure::Contract);
        };
        if field.name() != "animation_status" {
            return Err(AgentWorkFailure::Contract);
        }
        let SemanticExtractedValue::Text(value) = field.value() else {
            return Err(AgentWorkFailure::Contract);
        };
        let Some([source]) = result.sources(value.source_span()) else {
            return Err(AgentWorkFailure::Contract);
        };
        let fragment = source.fragment();
        if value.as_str() != MARKERS[4]
            || fragment.role() != SemanticRole::Paragraph
            || fragment.provenance().reference() != expected.reference
            || fragment.provenance().frame() != &expected.frame
            || !matches!(fragment.content(), SemanticReadContent::Text(text) if text.as_str() == MARKERS[4])
        {
            return Err(AgentWorkFailure::Contract);
        }
        self.extraction.accept_extraction(result)
    }
}
pub(super) fn verify_owned(result: &SemanticOwnedExtractionResult, expected: &Expected) -> bool {
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
    field.name() == "animation_status"
        && sources.next().is_none()
        && result.observation() == expected.observation
        && value.as_str() == MARKERS[4]
        && source.reference == expected.reference
        && source.frame == expected.frame
        && source.role == SemanticRole::Paragraph
        && matches!(&source.content, SemanticOwnedReadContent::Text(text) if text == MARKERS[4])
}

pub(super) const OBJECTIVE: &str = "Read the current rendering-readiness fixture. Extract the exact current animation-frame status into animation_status, using trusted schema 1 and initial scope. You may inspect the acknowledged baseline with read if useful. Copy the complete paragraph and cite that exact source. Do not navigate, modify the page, or infer the answer from these instructions.";

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(markers: &[&str], role: &str, completeness: &str) -> SemanticObservation {
        let mut rows =
            WorkBrowserResources::new(WorkId::generate(), AgentWorkProfileId::generate());
        let construct = rows
            .construct_document(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                ContextProfileStorageClass::Ephemeral,
                ContextNavigationTarget::parse("http://127.0.0.1:12345/semantic-rendering-v1.html")
                    .unwrap(),
                AgentPolicyInstant::from_millis(1),
            )
            .unwrap();
        let WorkBrowserResourceEvent::Retained(resource) = rows
            .settle_at(
                construct.complete(WorkBrowserResourceNativeOutcome::Constructed),
                AgentPolicyInstant::from_millis(2),
            )
            .unwrap()
        else {
            panic!("construct");
        };
        let acquire = rows
            .acquire(
                &resource,
                ContextRunId::generate(),
                AgentPolicyInstant::from_millis(3),
                AgentPolicyInstant::from_millis(100_000),
            )
            .unwrap();
        let WorkBrowserResourceEvent::Acquired(lease) = rows
            .settle_at(
                acquire.complete(WorkBrowserResourceNativeOutcome::Acquired),
                AgentPolicyInstant::from_millis(4),
            )
            .unwrap()
        else {
            panic!("acquire");
        };
        let request = rows
            .observe_initial(&lease, AgentPolicyInstant::from_millis(5))
            .unwrap();
        let correlation = request.invocation().correlation();
        let (invocation, _completion) = request.into_parts();
        let nodes = markers
            .iter()
            .enumerate()
            .map(|(index, text)| {
                let level = if role == "heading" { r#", "l":2"# } else { "" };
                format!(r#"{{"k":{},"r":"{role}","t":"{text}"{level}}}"#, index + 1)
            })
            .collect::<Vec<_>>()
            .join(",");
        let wire = format!(
            r#"{{"v":1,"i":{},"g":{},"c":"{completeness}","n":[{nodes}]}}"#,
            invocation.invocation().get(),
            invocation.snapshot_generation().get()
        );
        let snapshot = invocation.decode_result(wire.as_bytes()).unwrap();
        let observation = SemanticObservationRequest::initial(
            SemanticObservationId::new(correlation.invocation().get()).unwrap(),
            correlation.frame().context(),
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        SemanticObservationAssembler::new(observation, snapshot)
            .unwrap()
            .finish()
            .unwrap()
    }
    #[test]
    fn fixture_task_requires_complete_current_unique_paragraph_markers() {
        assert_eq!(
            document_policy(),
            zephium_agentic::WorkBrowserDocumentPolicy::Exact
        );
        assert!(matches!(
            capture(Box::new(|_| false)),
            RetainedProbeCapture::BoundedReadiness(_)
        ));
        let good = observation(&MARKERS, "paragraph", "complete");
        let expected = Arc::new(Mutex::new(None));
        let mut task = Task::new(
            good.request().context().identity(),
            expected.clone(),
            Arc::new(Mutex::new(None)),
        )
        .unwrap();
        assert_eq!(task.evaluate(&good), Ok(AgentWorkTaskProgress::Continue));
        let evidence = expected.lock().unwrap().clone().unwrap();
        assert_eq!(evidence.observation, good.request().id());
        assert_eq!(evidence.reference, good.frames()[0].nodes()[4].reference());
        assert!(task.evaluate(&good).is_err());
        for (markers, role, completeness) in [
            (MARKERS[..4].to_vec(), "paragraph", "complete"),
            (
                [MARKERS.as_slice(), &[MARKERS[4]]].concat(),
                "paragraph",
                "complete",
            ),
            (MARKERS.to_vec(), "heading", "complete"),
            (MARKERS.to_vec(), "paragraph", "node_limit"),
        ] {
            let observation = observation(&markers, role, completeness);
            let sample = Arc::new(Mutex::new(None));
            let mut task = Task::new(
                observation.request().context().identity(),
                Arc::new(Mutex::new(None)),
                sample.clone(),
            )
            .unwrap();
            assert!(task.evaluate(&observation).is_err());
            let sample = sample
                .lock()
                .unwrap()
                .expect("refused native sample remains observable");
            assert!(sample.current);
            assert!(!sample.complete || sample.markers.into_iter().any(|present| !present));
        }
        let foreign = observation(&MARKERS, "paragraph", "complete");
        let mut task = Task::new(
            good.request().context().identity(),
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(None)),
        )
        .unwrap();
        assert!(task.evaluate(&foreign).is_err());
        assert!(task
            .attest_account(
                foreign.request().context(),
                AgentPolicyInstant::from_millis(6)
            )
            .is_err());
    }
}
