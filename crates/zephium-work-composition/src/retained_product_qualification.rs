//! One public, model-decided page brief through the shipping retained entry.
use crate::navigation_qualification::{self as navigation, QualificationDefinition};
pub use navigation::ApplicationReport;
use std::io::Write as _;
use zephium_agent_controller::*;
use zephium_agentic::*;
use zephium_app::{RetainedWorkHandle, RetainedWorkPhase};

const ORIGIN: &str = "https://agent-browser.dev";
pub const OBJECTIVE: &str = "Read only the current https://agent-browser.dev/ homepage and prepare a concise source-backed technical brief useful to the team building Zephium, a Rust-owned agentic layer over native WebViews. Decide which architecture, capabilities, tradeoffs and product claims matter most. Distinguish what this page claims from what it actually establishes; explain relevant uncertainties and missing details rather than guessing. Use read with initial scope if useful. Finish by calling extract with initial scope and trusted schema 1, putting your brief, important claims and caveats in the answer field with current-page citations. Do not navigate, follow links, click, change values, sign in, submit, install or run anything. The host checks execution and source binding, not the factual correctness or usefulness of your answer; a human will judge those.";
pub(crate) const DEFINITION: QualificationDefinition = QualificationDefinition {
    initial: "https://agent-browser.dev/",
    origin: ORIGIN,
    task_name: "retained-agent-browser-brief-v1",
    retention_name: "inspectable-public",
    objective: OBJECTIVE,
    task,
    authority: Ok,
    configure_request: crate::TrustedWorkRequest::with_public_qualification_retention,
    max_hops: 0,
    inspection: true,
    verify_owned,
};
pub fn configuration_diagnostic() -> String {
    "work-retained-product-config: entry=admit_retained_trusted_work provider=OpenAIResponses model=gpt-5.6-luna retention=inspectable-public task=retained-agent-browser-brief-v1 navigation=false rendering_lease=false".into()
}
pub fn load_request(
    started: std::time::Instant,
    profile: zephium_app::AgentWorkProfileBinding,
) -> Result<crate::TrustedWorkRequest, &'static str> {
    navigation::load_configured_request(started, profile, &DEFINITION)
}
fn task(_: ContextIdentity) -> Result<Box<dyn AgentWorkTask>, AgentWorkFailure> {
    Ok(Box::new(PublicTask(
        AgentWorkExtractionTask::try_new(
            vec![
                SemanticExtractionFieldSchema::try_text("answer".into(), true, 4096)
                    .map_err(|_| AgentWorkFailure::Contract)?,
            ],
            AgentAccountScope::Anonymous,
        )?
        .with_baseline_read(),
    )))
}
struct PublicTask(AgentWorkExtractionTask);
impl AgentWorkTask for PublicTask {
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
        let completeness: Vec<_> = observation
            .frames()
            .iter()
            .map(|frame| frame.completeness())
            .collect();
        let text_bytes: u64 = observation
            .frames()
            .iter()
            .map(|frame| u64::from(frame.total_text_bytes()))
            .sum();
        writeln!(std::io::stdout().lock(), "work-retained-product-observation: nodes={} captured_frames={} boundaries={} completeness={completeness:?} text_bytes={text_bytes} content=redacted", observation.node_count(), observation.frames().len(), observation.frame_boundaries().len()).map_err(|_| AgentWorkFailure::Contract)?;
        self.0.evaluate(observation)
    }
}
fn verify_owned(result: &SemanticOwnedExtractionResult) -> bool {
    let [field] = result.fields() else {
        return false;
    };
    let SemanticExtractedValue::Text(value) = field.value() else {
        return false;
    };
    let Some(sources) = result.sources(value.source_span()) else {
        return false;
    };
    let sources: Vec<_> = sources.collect();
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
pub fn cancel(view: &RetainedWorkHandle) -> bool {
    view.stop()
}
#[derive(Default)]
pub struct ApplicationObserver {
    report: ApplicationReport,
    sequence: u64,
    failed: bool,
}
impl ApplicationObserver {
    pub fn report(&self) -> ApplicationReport {
        self.report
    }
    pub fn healthy(&self) -> bool {
        !self.failed
    }
    pub fn poll(&mut self, view: &RetainedWorkHandle) -> Option<ApplicationReport> {
        let snapshot = view.snapshot();
        for _ in 0..256 {
            let Some(event) = view.take_event() else {
                break;
            };
            self.failed |= event.run() != snapshot.run
                || self.sequence.checked_add(1) != Some(event.sequence());
            self.sequence = event.sequence();
            match event.kind() {
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
                AgentWorkEventKind::ToolProposed(
                    AgentBrowserToolKind::Read | AgentBrowserToolKind::Extract,
                ) => {}
                AgentWorkEventKind::ToolProposed(_)
                | AgentWorkEventKind::ActionActive
                | AgentWorkEventKind::NeedsHuman(_)
                | AgentWorkEventKind::Recovery => self.failed = true,
                _ => {}
            }
            self.failed |= writeln!(
                std::io::stdout().lock(),
                "work-retained-product-event: sequence={} phase={:?} wall_ms={} content=redacted",
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
        self.report.durable_terminal_verified = snapshot.record.is_some_and(|record| {
            record.disposition() == AgentWorkDisposition::Succeeded
                && record.debt() == AgentWorkDebt::NONE
                && record.key()[16..] == snapshot.run.bytes()
        });
        self.report.source_mapping_verified = view
            .take_extraction()
            .is_some_and(|result| verify_owned(&result) && view.take_extraction().is_none());
        self.report.accepted = !self.failed
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
        self.report.accepted &= writeln!(std::io::stdout().lock(), "work-retained-product-terminal: phase={:?} failure={:?} persistence_failure={:?} answer=dashboard_inspectable_public factual_validation=false content=redacted", snapshot.phase, snapshot.failure, snapshot.persistence_failure).is_ok();
        Some(self.report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn static_task_is_read_only_and_not_an_answer_validator() {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            zephium_core::ids::ProfileId::generate(),
            ContextKind::Owned,
        );
        let task = task(identity).unwrap();
        assert!(task.navigation_target().is_none());
        assert!(task.navigation_discovery().is_none());
        assert!(task.allows_baseline_read());
        assert!(!task.allows_subtree_extraction());
        assert!(configuration_diagnostic().contains("retention=inspectable-public"));
        assert_eq!(DEFINITION.initial, "https://agent-browser.dev/");
    }
}
