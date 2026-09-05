//! Provider-free measurement, not a runtime readiness or scheduling policy.
use super::*;

const SAMPLE_OFFSETS_MS: [u64; 8] = [0, 50, 100, 200, 400, 800, 1_600, 3_200];
const MEASUREMENT_TIMEOUT: Duration = Duration::from_secs(5);

/// Fixed synthetic document-ready marker, never authority for a real page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderingDocumentState {
    /// The fixture's loading marker was visible.
    Loading,
    /// The fixture observed its native DOM-ready transition.
    Interactive,
    /// The fixture observed its load-complete transition.
    Complete,
}

/// Content-free sample from one exact native semantic invocation.
#[derive(Clone, Copy, Debug)]
pub struct RenderingSample {
    /// Milliseconds from the exact committed navigation return.
    pub elapsed_ms: u64,
    /// Exact operation's native-finish fact before capture dispatch.
    pub native_finished_before: bool,
    /// Exact operation's native-finish fact after capture settlement.
    pub native_finished_after: bool,
    /// None means a typed DocumentLoading receipt, not an empty snapshot.
    pub document: Option<RenderingDocumentState>,
    /// Retained bounded nodes; zero only when no snapshot was delivered.
    pub nodes: usize,
    /// Exact synthetic microtask-revealed paragraph was observed.
    pub microtask: bool,
    /// Exact synthetic timer-revealed paragraph was observed.
    pub timer: bool,
    /// Exact synthetic load-event paragraph was observed.
    pub load: bool,
    /// Exact synthetic animation-frame-revealed paragraph was observed.
    pub animation_frame: bool,
}

impl RenderingSample {
    fn controls_ready(self) -> bool {
        self.native_finished_after
            && self.document == Some(RenderingDocumentState::Complete)
            && self.microtask
            && self.timer
            && self.load
    }
}

/// Bounded measurement disposition, not public-site root-cause proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderingDisposition {
    /// Control and animation-frame content were independently observed.
    AnimationFrameObserved,
    /// Controls converged, but no RAF reveal appeared within the sample window.
    AnimationFrameNotObservedWithinWindow,
    /// Load/microtask/timer controls did not all converge; cause remains unknown.
    ControlsIncomplete,
}

/// Returned only after exact original native/profile/listener teardown.
#[derive(Debug)]
pub struct MacosAgenticRenderingProbeReport {
    /// Bounded content-free samples, at most eight; no raw page data.
    pub samples: Vec<RenderingSample>,
    /// Evidence classification; a missing animation is not claimed permanent.
    pub disposition: RenderingDisposition,
}

fn disposition(samples: &[RenderingSample]) -> RenderingDisposition {
    if samples
        .iter()
        .any(|sample| sample.controls_ready() && sample.animation_frame)
    {
        RenderingDisposition::AnimationFrameObserved
    } else if samples
        .last()
        .is_some_and(|sample| sample.elapsed_ms >= SAMPLE_OFFSETS_MS[7])
        && samples.iter().any(|sample| sample.controls_ready())
    {
        RenderingDisposition::AnimationFrameNotObservedWithinWindow
    } else {
        RenderingDisposition::ControlsIncomplete
    }
}

fn marker(
    snapshot: &SemanticSnapshot,
    role: SemanticRole,
    expected: &str,
) -> Result<bool, &'static str> {
    let count = snapshot
        .nodes()
        .iter()
        .filter(|node| {
            let text = if role == SemanticRole::Heading {
                node.name()
            } else {
                node.text()
            };
            node.role() == role
                && node.sensitivity() == SemanticSensitivity::Public
                && text.is_some_and(|text| text.as_str() == expected)
        })
        .count();
    match count {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err("rendering_duplicate_marker"),
    }
}

pub(crate) fn sample_snapshot(
    snapshot: &SemanticSnapshot,
    context: zephium_agentic::ContextJoin,
    origin: &SemanticOrigin,
) -> Result<RenderingSample, &'static str> {
    if snapshot.frame().context() != context
        || snapshot.frame().origin() != origin
        || snapshot.frame().frame() != FrameId::MAIN
        || snapshot.completeness() != SemanticCompleteness::Complete
        || snapshot
            .nodes()
            .iter()
            .any(|node| node.role() == SemanticRole::FrameBoundary)
        || !marker(
            snapshot,
            SemanticRole::Heading,
            "Rendering readiness fixture",
        )?
    {
        return Err("rendering_snapshot_contract");
    }
    let states = [
        (RenderingDocumentState::Loading, "Document loading"),
        (RenderingDocumentState::Interactive, "Document interactive"),
        (RenderingDocumentState::Complete, "Document complete"),
    ];
    let mut document = None;
    for (state, text) in states {
        if marker(snapshot, SemanticRole::Paragraph, text)? && document.replace(state).is_some() {
            return Err("rendering_document_marker");
        }
    }
    if document.is_none() {
        return Err("rendering_document_marker");
    }
    Ok(RenderingSample {
        elapsed_ms: 0,
        native_finished_before: false,
        native_finished_after: false,
        document,
        nodes: snapshot.nodes().len(),
        microtask: marker(snapshot, SemanticRole::Paragraph, "Microtask ready")?,
        timer: marker(snapshot, SemanticRole::Paragraph, "Timer ready")?,
        load: marker(snapshot, SemanticRole::Paragraph, "Load ready")?,
        animation_frame: marker(snapshot, SemanticRole::Paragraph, "Animation frame ready")?,
    })
}

pub(super) fn measure(
    view: &AgentOwnedView,
    context: zephium_agentic::ContextJoin,
    operation: zephium_agentic::ContextOperationJoin,
    url: &str,
    runtime: &ProbeRuntime<'_, '_>,
) -> Result<MacosAgenticRenderingProbeReport, &'static str> {
    if operation.context() != context {
        return Err("rendering_operation");
    }
    let origin = SemanticOrigin::parse(url).map_err(|_| "rendering_origin")?;
    let frame = SemanticFrameJoin::try_new(
        context,
        FrameId::MAIN,
        context.frame_generation(),
        origin.clone(),
        SemanticFrameTrust::SameOrigin,
    )
    .map_err(|_| "rendering_frame")?;
    let started = Instant::now();
    let deadline = started
        .checked_add(MEASUREMENT_TIMEOUT)
        .ok_or("rendering_deadline")?;
    let mut samples = Vec::with_capacity(SAMPLE_OFFSETS_MS.len());
    for (index, offset) in SAMPLE_OFFSETS_MS.into_iter().enumerate() {
        let sample_at = started
            .checked_add(Duration::from_millis(offset))
            .ok_or("rendering_deadline")?;
        while !runtime.failed() && Instant::now() < sample_at && Instant::now() < deadline {
            runtime.pump();
        }
        if runtime.failed() || Instant::now() >= deadline {
            return Err("rendering_deadline");
        }
        let finished_before = view
            .navigation()
            .document_finished_for_audit(operation)
            .ok_or("rendering_navigation_join")?;
        let id = u64::try_from(index)
            .ok()
            .and_then(|index| index.checked_add(1))
            .ok_or("rendering_identity")?;
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(id).ok_or("rendering_identity")?,
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        let invocation = encode_semantic_runtime_invocation(
            &request,
            frame.clone(),
            SemanticInvocationId::new(id).ok_or("rendering_identity")?,
            SemanticSnapshotGeneration::new(id).ok_or("rendering_identity")?,
            SemanticRuntimeBudget::INITIAL_FILTERED,
        )
        .map_err(|_| "rendering_encode")?;
        // One invocation per sampling slot, not a readiness retry. The same
        // absolute measurement deadline owns every callback and pump.
        let outcome = dispatch_invocation(view, invocation, runtime, deadline)?;
        let finished_after = view
            .navigation()
            .document_finished_for_audit(operation)
            .ok_or("rendering_navigation_join")?;
        if finished_before && !finished_after {
            return Err("rendering_finish_regressed");
        }
        let mut sample = match outcome {
            Ok(snapshot) => sample_snapshot(&snapshot, context, &origin)?,
            Err(SemanticRuntimePortFailure::Result(SemanticRuntimeResultError::Runtime(
                SemanticRuntimeFault::DocumentLoading,
            ))) => RenderingSample {
                elapsed_ms: 0,
                native_finished_before: false,
                native_finished_after: false,
                document: None,
                nodes: 0,
                microtask: false,
                timer: false,
                load: false,
                animation_frame: false,
            },
            Err(_) => return Err("rendering_snapshot_failure"),
        };
        sample.elapsed_ms =
            u64::try_from(started.elapsed().as_millis()).map_err(|_| "rendering_clock")?;
        sample.native_finished_before = finished_before;
        sample.native_finished_after = finished_after;
        samples.push(sample);
        if sample.controls_ready() && sample.animation_frame {
            break;
        }
    }
    Ok(MacosAgenticRenderingProbeReport {
        disposition: disposition(&samples),
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_animation_evidence_requires_independent_converged_controls() {
        let mut sample = RenderingSample {
            elapsed_ms: 3_200,
            native_finished_before: true,
            native_finished_after: true,
            document: Some(RenderingDocumentState::Complete),
            nodes: 6,
            microtask: true,
            timer: true,
            load: true,
            animation_frame: false,
        };
        assert_eq!(disposition(&[]), RenderingDisposition::ControlsIncomplete);
        assert_eq!(
            disposition(&[RenderingSample {
                elapsed_ms: 1_600,
                ..sample
            }]),
            RenderingDisposition::ControlsIncomplete
        );
        assert_eq!(
            disposition(&[sample]),
            RenderingDisposition::AnimationFrameNotObservedWithinWindow
        );
        for incomplete in [
            RenderingSample {
                native_finished_after: false,
                ..sample
            },
            RenderingSample {
                document: Some(RenderingDocumentState::Interactive),
                ..sample
            },
            RenderingSample {
                document: None,
                ..sample
            },
            RenderingSample {
                microtask: false,
                ..sample
            },
            RenderingSample {
                timer: false,
                ..sample
            },
            RenderingSample {
                load: false,
                ..sample
            },
        ] {
            assert_eq!(
                disposition(&[incomplete]),
                RenderingDisposition::ControlsIncomplete
            );
            assert_eq!(
                disposition(&[RenderingSample {
                    animation_frame: true,
                    ..incomplete
                }]),
                RenderingDisposition::ControlsIncomplete
            );
        }
        sample.animation_frame = true;
        assert_eq!(
            disposition(&[sample]),
            RenderingDisposition::AnimationFrameObserved
        );
        assert_eq!(SAMPLE_OFFSETS_MS.len(), 8);
        assert_eq!(SAMPLE_OFFSETS_MS[7], 3_200);
        assert!(SAMPLE_OFFSETS_MS.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(MEASUREMENT_TIMEOUT > Duration::from_millis(SAMPLE_OFFSETS_MS[7]));
    }

    fn make_context() -> zephium_agentic::ContextJoin {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            ProfileId::generate(),
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
        let operation = registry
            .begin_context(identity.id(), ContextOperationId::new(1).unwrap())
            .unwrap();
        registry
            .settle_construction(identity.id(), operation, ContextSettlement::Applied)
            .unwrap();
        registry.join(identity.id()).unwrap()
    }

    fn snapshot(
        context: zephium_agentic::ContextJoin,
        origin: &str,
        completeness: &str,
        extra: &str,
    ) -> SemanticSnapshot {
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            SemanticOrigin::parse(origin).unwrap(),
            SemanticFrameTrust::SameOrigin,
        )
        .unwrap();
        let wire = format!(
            r#"{{"v":1,"i":1,"g":1,"c":"{completeness}","n":[{{"k":1,"r":"document"}},{{"k":2,"p":0,"r":"heading","l":1,"n":"Rendering readiness fixture"}},{{"k":3,"p":0,"r":"paragraph","t":"Document complete"}}{extra}]}}"#
        );
        zephium_agentic::decode_semantic_snapshot(
            zephium_agentic::SemanticDecodeContext::new(
                SemanticInvocationId::new(1).unwrap(),
                frame,
                SemanticSnapshotGeneration::INITIAL,
            ),
            wire.as_bytes(),
        )
        .unwrap()
    }

    #[test]
    fn semantic_measurements_reject_wrong_document_origin_completeness_and_forged_markers() {
        let context = make_context();
        let url = "http://127.0.0.1:1234";
        let origin = SemanticOrigin::parse(url).unwrap();
        let complete = snapshot(context, url, "complete", "");
        let sample = sample_snapshot(&complete, context, &origin).unwrap();
        assert_eq!(sample.document, Some(RenderingDocumentState::Complete));
        assert!(!sample.controls_ready());
        assert!(!sample.animation_frame);
        for invalid in [
            snapshot(make_context(), url, "complete", ""),
            snapshot(context, "http://127.0.0.1:1235", "complete", ""),
            snapshot(context, url, "node_limit", ""),
            snapshot(
                context,
                url,
                "complete",
                r#",{"k":4,"p":0,"r":"frame_boundary"}"#,
            ),
            snapshot(
                context,
                url,
                "complete",
                r#",{"k":4,"p":0,"r":"paragraph","t":"Document interactive"}"#,
            ),
            snapshot(
                context,
                url,
                "complete",
                r#",{"k":4,"p":0,"r":"paragraph","t":"Timer ready"},{"k":5,"p":0,"r":"paragraph","t":"Timer ready"}"#,
            ),
        ] {
            assert!(sample_snapshot(&invalid, context, &origin).is_err());
        }
        let wrong_role = snapshot(
            context,
            url,
            "complete",
            r#",{"k":4,"p":0,"r":"link","t":"Animation frame ready"}"#,
        );
        assert!(
            !sample_snapshot(&wrong_role, context, &origin)
                .unwrap()
                .animation_frame
        );
        let sensitive = snapshot(
            context,
            url,
            "complete",
            r#",{"k":4,"p":0,"r":"paragraph","q":"sensitive","t":"Animation frame ready"}"#,
        );
        assert!(
            !sample_snapshot(&sensitive, context, &origin)
                .unwrap()
                .animation_frame
        );
    }
}
