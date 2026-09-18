//! Bounded semantic production from explicitly selected dependency artifacts.
//! Model-facing keys are local to one disclosure, never durable identities or
//! permission to retrieve another source. Publication remains host-owned.
use super::{artifact::*, runtime::*, *};
use std::{future::Future, pin::Pin};

pub const MAX_SYNTHESIS_CONTEXT_BYTES: usize = 32 * 1024;
pub const MAX_SYNTHESIS_OUTPUT_BYTES: usize = 128 * 1024;

#[derive(Serialize)]
pub struct WorkSynthesisSource {
    pub key: u16,
    pub title: String,
    pub data: WorkArtifactDataV1,
    pub evidence: Vec<u16>,
}
#[derive(Serialize)]
pub struct WorkSynthesisEvidence {
    pub key: u16,
    pub origin: String,
    pub role: String,
    pub text: String,
    pub truncated: bool,
}
#[derive(Serialize)]
pub struct WorkSynthesisContext {
    pub objective: String,
    pub outputs: Vec<WorkExpectedOutput>,
    pub sources: Vec<WorkSynthesisSource>,
    pub evidence: Vec<WorkSynthesisEvidence>,
    /// User-selected canvas objects admitted for this execution.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<super::context::WorkContextBody>,
}

/// Disclosure data, never an execution token. The application admits the exact
/// dependency set and resolves historical evidence before calling this builder.
pub struct WorkSynthesisDisclosure {
    context: WorkSynthesisContext,
    links: Vec<WorkEvidenceLink>,
    limits: WorkExecutionLimits,
}
impl WorkSynthesisDisclosure {
    pub fn try_new(
        node: &WorkPlanNode,
        sources: &[WorkArtifactV1],
        previews: &[WorkEvidencePreviewV1],
        limits: WorkExecutionLimits,
    ) -> Result<Self, WorkError> {
        limits.validate()?;
        validate_text(&node.objective, MAX_WORK_TEXT_BYTES)?;
        if sources.len() > MAX_WORK_ARTIFACTS || previews.len() > 64 {
            return Err(WorkError::Capacity);
        }
        // Structural validation is not a semantic verification oracle. A model
        // adapter cannot satisfy a mechanically verified completion contract.
        if node.outputs.is_empty()
            || node.outputs.len() > 8
            || node
                .outputs
                .iter()
                .any(|o| o.review == WorkOutputReview::Mechanical)
        {
            return Err(WorkError::Invalid);
        }
        let mut names = BTreeSet::new();
        for output in &node.outputs {
            validate_text(&output.name, 128)?;
            validate_text(&output.description, MAX_WORK_TEXT_BYTES)?;
            if !names.insert(&output.name) {
                return Err(WorkError::Invalid);
            }
        }
        let mut links = Vec::new();
        let mut evidence = Vec::new();
        for preview in previews {
            if preview.version != 1
                || preview.link.source_id == 0
                || links.contains(&preview.link)
                || !sources
                    .iter()
                    .any(|source| source.evidence.contains(&preview.link))
            {
                return Err(WorkError::Invalid);
            }
            validate_text(&preview.origin, 4096)?;
            validate_text(&preview.role, 512)?;
            validate_text(&preview.text, 8192)?;
            links.push(preview.link.clone());
            evidence.push(WorkSynthesisEvidence {
                key: evidence.len() as u16,
                origin: preview.origin.clone(),
                role: preview.role.clone(),
                text: preview.text.clone(),
                truncated: preview.truncated,
            });
        }
        let mut selected = Vec::new();
        let mut identities = BTreeSet::new();
        for source in sources {
            source.validate()?;
            if !identities.insert(source.id) {
                return Err(WorkError::Invalid);
            }
            let citations = source
                .evidence
                .iter()
                .map(|link| {
                    links
                        .iter()
                        .position(|candidate| candidate == link)
                        .map(|index| index as u16)
                        .ok_or(WorkError::Invalid)
                })
                .collect::<Result<Vec<_>, _>>()?;
            selected.push(WorkSynthesisSource {
                key: selected.len() as u16,
                title: source.title.clone(),
                data: source.data.clone(),
                evidence: citations,
            });
        }
        let mut context = WorkSynthesisContext {
            objective: node.objective.clone(),
            outputs: node.outputs.clone(),
            sources: selected,
            evidence,
            context: Vec::new(),
        };
        let fits = |context: &WorkSynthesisContext| -> Result<bool, WorkError> {
            Ok(serde_json::to_vec(context)
                .map_err(|_| WorkError::Invalid)?
                .len()
                <= MAX_SYNTHESIS_CONTEXT_BYTES)
        };
        if !fits(&context)? {
            // Only the model-facing evidence passage is shortened. Exact local
            // citation keys and original persisted evidence remain unchanged.
            let original: Vec<_> = context
                .evidence
                .iter()
                .map(|item| (item.text.clone(), item.truncated))
                .collect();
            let project = |context: &mut WorkSynthesisContext, characters: usize| {
                for (item, (text, truncated)) in context.evidence.iter_mut().zip(&original) {
                    item.text = text.chars().take(characters).collect();
                    item.truncated = *truncated || item.text.len() < text.len();
                }
            };
            project(&mut context, 128);
            if !fits(&context)? {
                return Err(WorkError::Capacity);
            }
            let mut low = 128usize;
            let mut high = 8192;
            while low < high {
                let middle = low + (high - low).div_ceil(2);
                project(&mut context, middle);
                if fits(&context)? {
                    low = middle;
                } else {
                    high = middle - 1;
                }
            }
            project(&mut context, low);
        }
        Ok(Self {
            context,
            links,
            limits,
        })
    }
    pub fn context(&self) -> &WorkSynthesisContext {
        &self.context
    }
    /// Adds re-admitted context bodies within their own ceiling.
    pub fn with_context(
        mut self,
        bodies: Vec<super::context::WorkContextBody>,
    ) -> Result<Self, WorkError> {
        if bodies.len() > super::context::MAX_CONTEXT_ITEMS
            || bodies.iter().map(|body| body.text.len()).sum::<usize>()
                > super::context::MAX_CONTEXT_TOTAL_BYTES
        {
            return Err(WorkError::Capacity);
        }
        self.context.context = bodies;
        Ok(self)
    }
    pub fn limits(&self) -> WorkExecutionLimits {
        self.limits
    }
    /// Resolve only exact local citation keys after the complete output contract
    /// validates. No partial publication or model-chosen review state.
    pub fn resolve(
        &self,
        outputs: Vec<WorkSynthesisOutput>,
    ) -> Result<Vec<WorkSynthesisArtifact>, WorkError> {
        if outputs.len() != self.context.outputs.len() {
            return Err(WorkError::Invalid);
        }
        let mut used = BTreeSet::new();
        let mut resolved = Vec::new();
        let mut bytes = 0;
        for output in outputs {
            let expected = self
                .context
                .outputs
                .get(usize::from(output.output))
                .ok_or(WorkError::Invalid)?;
            if !used.insert(output.output) || output.evidence.len() > 64 {
                return Err(WorkError::Invalid);
            }
            validate_text(&output.title, 512)?;
            output.data.validate(output.evidence.len())?;
            // Prose may link only to sources this attempt was shown; an
            // invented URL is refused with the whole envelope.
            if let WorkArtifactDataV1::Document {
                formatted: Some(document),
                ..
            } = &output.data
            {
                let cited = super::document::document_links(document);
                if cited.iter().any(|href| {
                    !self
                        .context
                        .evidence
                        .iter()
                        .any(|evidence| evidence.origin == *href)
                }) {
                    return Err(WorkError::Invalid);
                }
            }
            bytes += serde_json::to_vec(&output)
                .map_err(|_| WorkError::Invalid)?
                .len();
            if bytes > MAX_SYNTHESIS_OUTPUT_BYTES {
                return Err(WorkError::Capacity);
            }
            let mut cited = BTreeSet::new();
            let evidence = output
                .evidence
                .into_iter()
                .map(|key| {
                    if !cited.insert(key) {
                        return Err(WorkError::Invalid);
                    }
                    self.links
                        .get(usize::from(key))
                        .cloned()
                        .ok_or(WorkError::Invalid)
                })
                .collect::<Result<Vec<_>, _>>()?;
            if expected.review == WorkOutputReview::SourceMappedNeedsReview && evidence.is_empty() {
                return Err(WorkError::Invalid);
            }
            resolved.push(WorkSynthesisArtifact {
                output: expected.name.clone(),
                title: output.title,
                data: output.data,
                evidence,
            });
        }
        Ok(resolved)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkSynthesisOutput {
    pub output: u8,
    pub title: String,
    pub data: WorkArtifactDataV1,
    pub evidence: Vec<u16>,
}
pub struct WorkSynthesisArtifact {
    pub output: String,
    pub title: String,
    pub data: WorkArtifactDataV1,
    pub evidence: Vec<WorkEvidenceLink>,
}
pub struct WorkSynthesisResult {
    pub outputs: Vec<WorkSynthesisOutput>,
    pub usage: WorkUsage,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkSynthesisError {
    /// No generation was dispatched.
    NotDispatched(WorkError),
    /// A provider terminal and its conservative charge were independently read.
    Rejected(WorkUsage),
    /// An accepted call may have been billed; never refund or auto-retry.
    OutcomeUnknown,
    /// A dispatched call produced no readable terminal; its ceiling is
    /// charged and the caller may try again within its budget.
    Stalled(WorkUsage),
}
pub type WorkSynthesisFuture<'a> =
    Pin<Box<dyn Future<Output = Result<WorkSynthesisResult, WorkSynthesisError>> + Send + 'a>>;
/// Content-free facts identifying the boundary of a synthesis refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkSynthesisDiagnostic {
    DisclosureReady {
        attempt: WorkAttemptId,
        bytes: usize,
        sources: usize,
        evidence: usize,
    },
    DisclosureFailed {
        attempt: WorkAttemptId,
        error: WorkError,
    },
    ProviderRefused {
        attempt: WorkAttemptId,
        error: WorkSynthesisError,
    },
    InputCounted {
        tokens: u32,
        maximum: u32,
        request_bytes: usize,
    },
    /// Closed transport facts for one generation call: HTTP status when a
    /// response arrived, body size, whether it decoded, wall time.
    ProviderTransport {
        http_status: Option<u16>,
        body_bytes: usize,
        decoded: bool,
        elapsed_millis: u64,
    },
    /// A decoded agent turn the transport could not admit.
    TurnRejected {
        reason: super::agent::WorkAgentTurnRejection,
    },
}
/// Per-call attribution only, never worker authority or permission to retrieve Work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkSynthesisTrace {
    pub work: WorkId,
    pub execution: WorkExecutionId,
    pub attempt: WorkAttemptId,
}
pub trait WorkSynthesisProvider: Send + Sync {
    fn diagnostic(&self, _event: WorkSynthesisDiagnostic) {}
    fn produce<'a>(&'a self, input: &'a WorkSynthesisDisclosure) -> WorkSynthesisFuture<'a>;
    fn produce_owned<'a>(
        &'a self,
        input: &'a WorkSynthesisDisclosure,
        _trace: WorkSynthesisTrace,
    ) -> WorkSynthesisFuture<'a> {
        self.produce(input)
    }
}
impl std::fmt::Debug for WorkSynthesisDisclosure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkSynthesisDisclosure([redacted])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (
        WorkPlanNode,
        WorkArtifactV1,
        WorkEvidencePreviewV1,
        WorkExecutionLimits,
    ) {
        let link = WorkEvidenceLink {
            extraction_id: 55.into(),
            source_id: 3,
        };
        let node = WorkPlanNode {
            id: 1.into(),
            objective: "Summarize the dependency's evidence".into(),
            dependencies: vec![2.into()],
            outputs: vec![WorkExpectedOutput {
                name: "summary".into(),
                description: "A cited comparison".into(),
                review: WorkOutputReview::SourceMappedNeedsReview,
            }],
        };
        let source = WorkArtifactV1 {
            version: 1,
            id: 4.into(),
            execution: 5.into(),
            node: 2.into(),
            attempt: 6.into(),
            output: "research".into(),
            title: "Original finding".into(),
            data: WorkArtifactDataV1::Document {
                paragraphs: vec!["A database uses shared memory.".into()],
                formatted: None,
            },
            evidence: vec![link.clone()],
            review: WorkOutputReview::SourceMappedNeedsReview,
            presentation: WorkArtifactPresentationV1::Automatic,
        };
        let preview = WorkEvidencePreviewV1 {
            link_destination: None,
            source: super::super::artifact::WorkEvidenceSourceV1::NativeExtraction,
            version: 1,
            link,
            origin: "https://sqlite.org".into(),
            role: "paragraph".into(),
            text: "Processes must be on the same host.".into(),
            truncated: false,
            source_bytes: "35".into(),
        };
        let limits = WorkExecutionLimits {
            model_tokens: 8000,
            cost_micro_usd: 10000,
            operations: 2,
            timeout_seconds: 60,
            max_workers: 1,
        };
        (node, source, preview, limits)
    }
    fn output() -> WorkSynthesisOutput {
        WorkSynthesisOutput {
            output: 0,
            title: "Summary".into(),
            data: WorkArtifactDataV1::Document {
                paragraphs: vec!["Shared memory requires the same host.".into()],
                formatted: None,
            },
            evidence: vec![0],
        }
    }
    #[test]
    fn synthesis_discloses_local_keys_and_resolves_only_original_sources() {
        let (node, source, preview, limits) = fixture();
        let input = WorkSynthesisDisclosure::try_new(
            &node,
            std::slice::from_ref(&source),
            std::slice::from_ref(&preview),
            limits,
        )
        .unwrap();
        let wire = serde_json::to_string(input.context()).unwrap();
        for id in [
            source.id.to_string(),
            source.execution.to_string(),
            source.attempt.to_string(),
            preview.link.extraction_id.to_string(),
        ] {
            assert!(!wire.contains(&id));
        }
        let resolved = input.resolve(vec![output()]).unwrap();
        assert_eq!(resolved[0].output, "summary");
        assert_eq!(resolved[0].evidence, vec![preview.link]);
        for citations in [vec![], vec![1], vec![0, 0]] {
            let mut invalid = output();
            invalid.evidence = citations;
            assert!(matches!(
                input.resolve(vec![invalid]),
                Err(WorkError::Invalid)
            ));
        }
        assert!(matches!(
            input.resolve(vec![output(), output()]),
            Err(WorkError::Invalid)
        ));
        let mut invalid = output();
        invalid.output = 1;
        assert!(matches!(
            input.resolve(vec![invalid]),
            Err(WorkError::Invalid)
        ));
    }
    #[test]
    fn synthesis_rejects_unjoined_evidence_and_mechanical_verification() {
        let (mut node, source, mut preview, limits) = fixture();
        assert!(matches!(
            WorkSynthesisDisclosure::try_new(&node, std::slice::from_ref(&source), &[], limits),
            Err(WorkError::Invalid)
        ));
        preview.link.source_id = 4;
        assert!(matches!(
            WorkSynthesisDisclosure::try_new(&node, &[source], &[preview], limits),
            Err(WorkError::Invalid)
        ));
        node.outputs[0].review = WorkOutputReview::Mechanical;
        assert!(matches!(
            WorkSynthesisDisclosure::try_new(&node, &[], &[], limits),
            Err(WorkError::Invalid)
        ));
    }
    #[test]
    fn synthesis_prose_may_link_only_to_disclosed_source_urls() {
        use crate::work::document::*;
        let (node, source, preview, limits) = fixture();
        let input = WorkSynthesisDisclosure::try_new(
            &node,
            std::slice::from_ref(&source),
            std::slice::from_ref(&preview),
            limits,
        )
        .unwrap();
        let document = |href: &str| {
            let (paragraphs, formatted) = compile_blocks(&[WorkDocumentBlock {
                kind: WorkBlockKind::Paragraph,
                level: None,
                spans: vec![WorkDocumentSpan {
                    text: "Source".into(),
                    style: WorkSpanStyle::Plain,
                    href: Some(href.into()),
                }],
                items: vec![],
            }])
            .unwrap();
            WorkSynthesisOutput {
                output: 0,
                title: "Summary".into(),
                data: WorkArtifactDataV1::Document {
                    paragraphs,
                    formatted: Some(formatted),
                },
                evidence: vec![0],
            }
        };
        assert!(input.resolve(vec![document("https://sqlite.org")]).is_ok());
        assert_eq!(
            input
                .resolve(vec![document("https://invented.example/page")])
                .err(),
            Some(WorkError::Invalid)
        );
    }

    #[test]
    fn synthesis_rejects_renderer_code_as_an_extra_model_field() {
        let mut value = serde_json::to_value(output()).unwrap();
        value["html"] = serde_json::json!("<script>run()</script>");
        assert!(serde_json::from_value::<WorkSynthesisOutput>(value).is_err());
        let (node, mut source, preview, limits) = fixture();
        source.data = WorkArtifactDataV1::Document {
            paragraphs: vec!["\\".repeat(20_000)],
            formatted: None,
        };
        assert!(matches!(
            WorkSynthesisDisclosure::try_new(&node, &[source], &[preview], limits),
            Err(WorkError::Capacity)
        ));
    }
}
