//! Public open-objective evidence through ordinary profile-bound application admission.
use std::io::Write as _;
use zephium_agent_controller::*;
use zephium_agentic::*;

use crate::navigation_qualification::{self as navigation, QualificationDefinition};
pub use navigation::{cancel, ApplicationReport};

const ORIGIN: &str = "https://react.dev";
const MAX_HOPS: u64 = 2;
const DISCOVERY: bool = true;
pub(crate) const DEFINITION: QualificationDefinition = QualificationDefinition {
    task_name: "react-open-objective-v1",
    retention_name: "inspectable-public",
    objective: OBJECTIVE,
    task,
    authority,
    configure_request,
    max_hops: MAX_HOPS,
    inspection: DISCOVERY,
    verify_owned,
};

/// Content-free configuration of the exact statically selected witness.
pub fn configuration_diagnostic() -> String {
    DEFINITION.configuration_diagnostic()
}

pub fn load_request(
    started: std::time::Instant,
    profile: zephium_app::AgentWorkProfileBinding,
) -> Result<crate::TrustedWorkRequest, &'static str> {
    navigation::load_configured_request(started, profile, &DEFINITION)
}

pub struct ApplicationObserver(navigation::ApplicationObserver);
impl Default for ApplicationObserver {
    fn default() -> Self {
        Self(navigation::ApplicationObserver::with_definition(
            &DEFINITION,
        ))
    }
}
impl ApplicationObserver {
    pub fn report(&self) -> ApplicationReport {
        self.0.report()
    }
    pub fn healthy(&self) -> bool {
        self.0.healthy()
    }
    pub fn poll(
        &mut self,
        view: &zephium_app::AgentWorkApplicationHandle,
    ) -> Option<ApplicationReport> {
        self.0.poll(view)
    }
}
/// A practical question without an answer, destination URL or scripted route.
pub const OBJECTIVE: &str = "Starting at https://react.dev/learn, find React's guidance for this problem: in one click handler I call setNumber(number + 1) three times, but my counter only increases once. Explain why and what I should change so one click increases it three times. Follow current observed document links under https://react.dev/learn/ if useful; you may take at most two hops. Do not guess destination URLs. Use read with initial scope if needed. When the current page supports the answer, extract with initial scope and trusted schema 1, putting a concise explanation and practical correction in answer, with current source citations. The route and answer are yours to discover; the host checks sources and execution, while a human judges usefulness. Do not click controls, change values, sign in, submit, or buy anything.";

fn scope() -> Result<AgentNavigationDiscovery, AgentWorkFailure> {
    AgentNavigationDiscovery::try_new(
        ContextNavigationTarget::parse("https://react.dev/learn")
            .map_err(|_| AgentWorkFailure::Contract)?,
        "/learn/".into(),
        MAX_HOPS as usize,
    )
    .map_err(|_| AgentWorkFailure::Contract)
}
fn authority(authority: AgentPlanNodeAuthority) -> Result<AgentPlanNodeAuthority, &'static str> {
    authority
        .with_navigation_discovery(scope().map_err(|_| "scope")?)
        .map_err(|_| "authority")
}
fn configure_request(request: crate::TrustedWorkRequest) -> crate::TrustedWorkRequest {
    request.with_public_qualification_retention()
}
pub fn task(context: ContextIdentity) -> Result<Box<dyn AgentWorkTask>, AgentWorkFailure> {
    Ok(Box::new(PublicTask(AgentWorkDiscoveryTask::try_new(
        context,
        scope()?,
        vec![
            SemanticExtractionFieldSchema::try_text("answer".into(), true, 2048)
                .map_err(|_| AgentWorkFailure::Contract)?,
        ],
    )?)))
}

struct PublicTask(AgentWorkDiscoveryTask);
impl AgentWorkTask for PublicTask {
    fn navigation_discovery(&self) -> Option<&AgentNavigationDiscovery> {
        self.0.navigation_discovery()
    }
    fn allows_baseline_read(&self) -> bool {
        self.0.allows_baseline_read()
    }
    fn extraction_schema(&self) -> Option<&SemanticExtractionSchema> {
        self.0.extraction_schema()
    }
    fn assess(
        &self,
        action: &SemanticPreparedAction,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        self.0.assess(action)
    }
    fn attest_account(
        &self,
        context: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        self.0.attest_account(context, now)
    }
    fn accept_extraction(
        &mut self,
        result: &SemanticExtractionResult<'_>,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        self.0.accept_extraction(result)
    }
    fn evaluate(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        let progress = self.0.evaluate(observation);
        let mut links = 0_usize;
        let mut destination_bytes = 0_usize;
        for target in observation
            .frames()
            .iter()
            .flat_map(|frame| frame.nodes())
            .filter_map(SemanticNode::link_destination)
        {
            links += 1;
            destination_bytes += target.as_url().as_str().len();
        }
        // Record closed boundary reasons even when task evaluation refuses,
        // without disclosing child origins, content or identifiers.
        let boundaries = observation.frame_boundaries().len();
        let policy_blocked_frames = observation
            .frame_boundaries()
            .iter()
            .filter(|boundary| {
                boundary.status()
                    == SemanticFrameBoundaryStatus::Unsupported(
                        SemanticFrameUnsupported::PolicyBlocked,
                    )
            })
            .count();
        writeln!(std::io::stdout().lock(), "work-discovery-observation: model=gpt-5.6-luna retention=inspectable-public nodes={} links={links} destination_bytes={destination_bytes} captured_frames={} boundaries={boundaries} policy_blocked_frames={policy_blocked_frames} task_accepted={} content=redacted", observation.node_count(), observation.frames().len(), progress.is_ok()).map_err(|_| AgentWorkFailure::Contract)?;
        progress
    }
}
pub fn verify_owned(result: &SemanticOwnedExtractionResult) -> bool {
    let [field] = result.fields() else {
        return false;
    };
    let SemanticExtractedValue::Text(value) = field.value() else {
        return false;
    };
    let Some(sources) = result.sources(value.source_span()) else {
        return false;
    };
    let sources = sources.collect::<Vec<_>>();
    result.trust() == SemanticExtractionTrust::ModelMapped
        && result.schema().get() == 1
        && field.name() == "answer"
        && !value.as_str().is_empty()
        && !sources.is_empty()
        && sources.iter().all(|source| {
            source.sensitivity == SemanticSensitivity::Public
                && source.frame.frame() == FrameId::MAIN
                && SemanticOrigin::parse(ORIGIN).as_ref() == Ok(source.frame.origin())
        })
}
