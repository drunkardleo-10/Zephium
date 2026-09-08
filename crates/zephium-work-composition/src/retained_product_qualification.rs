//! Public observed-link discovery through the shipping retained product entry.
use crate::navigation_qualification::{self as navigation, QualificationDefinition};
pub use navigation::ApplicationReport;
use std::io::Write as _;
use zephium_agent_controller::*;
use zephium_agentic::*;
use zephium_app::{RetainedWorkHandle, RetainedWorkPhase};

// Static qualification selection only. No URL, prompt, expected answer or
// authority is accepted from an environment variable, page or IPC request.
#[cfg(feature = "retained-commerce-qualification")]
#[path = "retained_commerce_objective.rs"]
mod objective;
#[cfg(not(feature = "retained-commerce-qualification"))]
#[path = "retained_svelte_objective.rs"]
mod objective;
pub use objective::OBJECTIVE;
use objective::{INITIAL, ORIGIN, PATH_PREFIX, TASK_NAME};
pub(crate) const DEFINITION: QualificationDefinition = QualificationDefinition {
    initial: INITIAL,
    origin: ORIGIN,
    task_name: TASK_NAME,
    retention_name: "inspectable-public",
    objective: OBJECTIVE,
    task,
    authority,
    configure_request: crate::TrustedWorkRequest::with_public_qualification_retention,
    max_hops: 2,
    inspection: true,
    verify_owned,
};
pub fn configuration_diagnostic() -> String {
    format!(
        "work-retained-product-config: entry=admit_retained_trusted_work provider=OpenAIResponses model=gpt-5.6-luna retention={} task={} navigation=observed_link_discovery rendering=observation_owned",
        DEFINITION.retention_name, DEFINITION.task_name
    )
}
pub fn load_request(
    started: std::time::Instant,
    profile: zephium_app::AgentWorkProfileBinding,
) -> Result<crate::TrustedWorkRequest, &'static str> {
    navigation::load_configured_request(started, profile, &DEFINITION)
}
fn discovery() -> Result<AgentNavigationDiscovery, AgentWorkFailure> {
    AgentNavigationDiscovery::try_new(
        ContextNavigationTarget::parse(INITIAL).map_err(|_| AgentWorkFailure::Contract)?,
        PATH_PREFIX.into(),
        2,
    )
    .map_err(|_| AgentWorkFailure::Contract)
}
fn authority(authority: AgentPlanNodeAuthority) -> Result<AgentPlanNodeAuthority, &'static str> {
    authority
        .with_navigation_discovery(discovery().map_err(|_| "discovery")?)
        .map_err(|_| "authority")
}
fn task(identity: ContextIdentity) -> Result<Box<dyn AgentWorkTask>, AgentWorkFailure> {
    Ok(Box::new(PublicTask(AgentWorkDiscoveryTask::try_new(
        identity,
        discovery()?,
        vec![
            // Total value ceiling remains 4,096 bytes: 640 + 8*320 + 4*224.
            SemanticExtractionFieldSchema::try_text("summary".into(), true, 640)
                .map_err(|_| AgentWorkFailure::Contract)?,
            SemanticExtractionFieldSchema::try_text_list("important_claims".into(), true, 8, 320)
                .map_err(|_| AgentWorkFailure::Contract)?,
            SemanticExtractionFieldSchema::try_text_list("caveats".into(), true, 4, 224)
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
    fn allows_progressive_observation(&self) -> bool {
        self.0.allows_progressive_observation()
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
    let [summary, claims, caveats] = result.fields() else {
        return false;
    };
    let (
        SemanticExtractedValue::Text(summary_value),
        SemanticExtractedValue::TextList(claim_values),
        SemanticExtractedValue::TextList(caveat_values),
    ) = (summary.value(), claims.value(), caveats.value())
    else {
        return false;
    };
    result.trust() == SemanticExtractionTrust::ModelMapped
        && result.schema().get() == 1
        && summary.name() == "summary"
        && claims.name() == "important_claims"
        && caveats.name() == "caveats"
        && !summary_value.as_str().is_empty()
        && [
            summary_value.source_span(),
            claim_values.source_span(),
            caveat_values.source_span(),
        ]
        .into_iter()
        .chain(
            claim_values
                .items()
                .iter()
                .chain(caveat_values.items())
                .map(|item| item.source_span()),
        )
        .all(|span| verify_sources(result, span))
}
fn verify_sources(
    result: &SemanticOwnedExtractionResult,
    span: SemanticExtractionSourceSpan,
) -> bool {
    let Some(sources) = result.sources(span) else {
        return false;
    };
    let mut count = 0;
    let valid = sources.into_iter().all(|source| {
        count += 1;
        source.sensitivity == SemanticSensitivity::Public
            && source.frame.frame() == FrameId::MAIN
            && SemanticOrigin::parse(ORIGIN).as_ref() == Ok(source.frame.origin())
    });
    valid && count > 0
}
pub fn cancel(view: &RetainedWorkHandle) -> bool {
    view.stop()
}
#[derive(Default)]
pub struct ApplicationObserver {
    report: ApplicationReport,
    sequence: u64,
    failed: bool,
    resource_failure_cause: Option<zephium_engine::WorkResourceFailureCause>,
}
impl ApplicationObserver {
    /// Descriptive first cause only, read from the original resource before
    /// shutdown removes it. Does not affect native or controller decisions.
    fn observe_resource_failure_cause(
        &mut self,
        failure: Option<zephium_engine::WorkResourceFailureCause>,
    ) {
        if self.resource_failure_cause.is_none() {
            self.resource_failure_cause = failure;
        }
    }
    fn resource_failure_diagnostic(&self) -> String {
        format!(
            "work-retained-product-resource-failure: cause={:?} content=redacted",
            self.resource_failure_cause
        )
    }
    pub fn report(&self) -> ApplicationReport {
        self.report
    }
    pub fn healthy(&self) -> bool {
        !self.failed
    }
    pub fn poll(
        &mut self,
        view: &RetainedWorkHandle,
        resource_failure_cause: impl FnOnce() -> Option<zephium_engine::WorkResourceFailureCause>,
    ) -> Option<ApplicationReport> {
        let snapshot = view.snapshot();
        for _ in 0..256 {
            let Some(event) = view.take_event() else {
                break;
            };
            self.failed |= event.run() != snapshot.run
                || self.sequence.checked_add(1) != Some(event.sequence());
            self.sequence = event.sequence();
            self.observe_kind(event.kind());
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
        // Sample after the terminal projection, not before it: a native failure
        // racing this poll must publish its cause before its resource terminal.
        // The ordinary shutdown owner has not yet removed the original guard.
        self.observe_resource_failure_cause(resource_failure_cause());
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
            && (1..=DEFINITION.max_hops).contains(&self.report.navigation_proposals)
            && (1..=8).contains(&self.report.model_calls)
            && self
                .report
                .input_tokens
                .saturating_add(self.report.output_tokens)
                <= 100_000
            && self.report.cost_micro_usd <= 100_000;
        self.report.accepted &= writeln!(std::io::stdout().lock(), "work-retained-product-terminal: phase={:?} failure={:?} persistence_failure={:?} answer=dashboard_inspectable_public factual_validation=false content=redacted", snapshot.phase, snapshot.failure, snapshot.persistence_failure).is_ok();
        self.report.accepted &= writeln!(
            std::io::stdout().lock(),
            "{}",
            self.resource_failure_diagnostic()
        )
        .is_ok();
        Some(self.report)
    }

    // The same handler is exercised by deterministic composition tests. It
    // observes only content-free events; admission and lifecycle stay elsewhere.
    fn observe_kind(&mut self, kind: AgentWorkEventKind) {
        match kind {
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
            AgentWorkEventKind::ToolProposed(AgentBrowserToolKind::Extract) => {}
            AgentWorkEventKind::ToolProposed(
                AgentBrowserToolKind::Read
                | AgentBrowserToolKind::Locate
                | AgentBrowserToolKind::Snapshot,
            ) if DEFINITION.inspection => {}
            AgentWorkEventKind::ToolProposed(AgentBrowserToolKind::Navigate) => {
                self.report.navigation_proposals =
                    self.report.navigation_proposals.saturating_add(1);
                self.failed |= self.report.navigation_proposals > DEFINITION.max_hops;
            }
            AgentWorkEventKind::ToolProposed(_)
            | AgentWorkEventKind::ActionActive
            | AgentWorkEventKind::Verified
            | AgentWorkEventKind::NeedsHuman(_)
            | AgentWorkEventKind::Recovery => self.failed = true,
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "retained_objective_tests.rs"]
mod objective_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resource_failure_cause_survives_observer_to_closed_content_free_log() {
        use zephium_engine::{
            WorkResourceDeadlineStage as Stage, WorkResourceFailureCause as Failure,
        };
        for (cause, label) in [
            (Failure::NavigationEventRefused, "NavigationEventRefused"),
            (Failure::UrlObservationRefused, "UrlObservationRefused"),
            (
                Failure::DocumentFinalizationRefused,
                "DocumentFinalizationRefused",
            ),
            (Failure::RendererLost, "RendererLost"),
            (Failure::SemanticNativeInvariant, "SemanticNativeInvariant"),
            (
                Failure::LifecycleDeadline(Stage::ConstructionTargetProvisional),
                "LifecycleDeadline(ConstructionTargetProvisional)",
            ),
            (
                Failure::UnattributedResourceFailure,
                "UnattributedResourceFailure",
            ),
            (
                Failure::NativeAdmission(ContextPortFailure::NativeRefused),
                "NativeAdmission(NativeRefused)",
            ),
        ] {
            let mut observer = ApplicationObserver::default();
            observer.observe_resource_failure_cause(None);
            observer.observe_resource_failure_cause(Some(cause));
            observer.observe_resource_failure_cause(None);
            observer.observe_resource_failure_cause(Some(Failure::LifecycleDeadline(
                Stage::DestructionDrain,
            )));
            assert_eq!(
                observer.resource_failure_diagnostic(),
                format!(
                    "work-retained-product-resource-failure: cause=Some({label}) content=redacted"
                )
            );
            assert!(observer.healthy());
            assert_eq!(observer.report().model_calls, 0);
            assert!(!observer.report().accepted);
        }
    }
    #[test]
    fn discovery_task_has_frozen_public_scope_but_no_route_or_answer_validator() {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            zephium_core::ids::ProfileId::generate(),
            ContextKind::Owned,
        );
        let task = (DEFINITION.task)(identity).unwrap();
        assert!(task.navigation_target().is_none());
        assert_eq!(task.navigation_discovery(), Some(&discovery().unwrap()));
        assert!(task.navigation_route().is_none());
        assert!(task.allows_baseline_read());
        assert!(task.allows_progressive_observation());
        assert!(!task.allows_subtree_extraction());
        assert!(configuration_diagnostic().contains("retention=inspectable-public"));
        assert!(configuration_diagnostic().contains(DEFINITION.task_name));
        assert_eq!(DEFINITION.initial, INITIAL);
        assert_eq!(DEFINITION.max_hops, 2);
        let fields = task.extraction_schema().unwrap().fields();
        assert_eq!(
            fields.iter().map(|field| field.name()).collect::<Vec<_>>(),
            ["summary", "important_claims", "caveats"]
        );
        assert_eq!(fields[0].max_text_bytes(), Some(640));
        assert_eq!(fields[1].max_list_items(), Some(8));
        assert_eq!(fields[1].max_list_item_bytes(), Some(320));
        assert_eq!(fields[2].max_list_items(), Some(4));
        assert_eq!(fields[2].max_list_item_bytes(), Some(224));
    }

    #[test]
    fn retained_definition_and_observer_preserve_progressive_inspection_without_effects() {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            zephium_core::ids::ProfileId::generate(),
            ContextKind::Owned,
        );
        let wrapped = (DEFINITION.task)(identity).unwrap();
        assert!(wrapped.allows_progressive_observation());
        assert!(wrapped.allows_baseline_read());
        assert!(!wrapped.allows_actions_before_extraction());
        assert!(!wrapped.allows_subtree_extraction());
        let mut observer = ApplicationObserver::default();
        for kind in [
            AgentBrowserToolKind::Snapshot,
            AgentBrowserToolKind::Read,
            AgentBrowserToolKind::Locate,
            AgentBrowserToolKind::Navigate,
            AgentBrowserToolKind::Snapshot,
            AgentBrowserToolKind::Navigate,
            AgentBrowserToolKind::Extract,
        ] {
            observer.observe_kind(AgentWorkEventKind::ToolProposed(kind));
            assert!(observer.healthy(), "{kind:?}");
        }
        assert_eq!(observer.report().navigation_proposals, 2);
        assert!(
            !observer.report().accepted,
            "tool events never prove durable/source closure"
        );
        observer.observe_kind(AgentWorkEventKind::ToolProposed(
            AgentBrowserToolKind::Navigate,
        ));
        assert!(!observer.healthy());
        observer.observe_kind(AgentWorkEventKind::ToolProposed(
            AgentBrowserToolKind::Snapshot,
        ));
        assert!(
            !observer.healthy(),
            "inspection cannot clear a previous failure"
        );
        for kind in [
            AgentBrowserToolKind::Act,
            AgentBrowserToolKind::Back,
            AgentBrowserToolKind::Forward,
            AgentBrowserToolKind::Reload,
            AgentBrowserToolKind::Wait,
            AgentBrowserToolKind::Screenshot,
            AgentBrowserToolKind::ShowForHuman,
            AgentBrowserToolKind::ResumeAfterHuman,
        ] {
            let mut observer = ApplicationObserver::default();
            observer.observe_kind(AgentWorkEventKind::ToolProposed(kind));
            assert!(!observer.healthy(), "{kind:?}");
            assert!(!observer.report().accepted);
        }
        for kind in [
            AgentWorkEventKind::ActionActive,
            AgentWorkEventKind::Verified,
            AgentWorkEventKind::Recovery,
        ] {
            let mut observer = ApplicationObserver::default();
            observer.observe_kind(kind);
            assert!(!observer.healthy());
        }
    }

    #[test]
    fn retained_observer_keeps_exact_usage_and_sticky_failure_across_inspection() {
        let settled =
            |input_tokens, output_tokens, cost_micro_usd| AgentWorkEventKind::ModelSettled {
                call: AgentModelCallId::new(1).unwrap(),
                input_tokens,
                output_tokens,
                cost_micro_usd,
                request_bytes: 100,
                semantic_bytes: 50,
                accounting: AgentModelUsageAccounting::Exact,
                elapsed_millis: 1,
            };
        let mut observer = ApplicationObserver::default();
        observer.observe_kind(settled(720, 58, 214));
        observer.observe_kind(AgentWorkEventKind::ToolProposed(
            AgentBrowserToolKind::Snapshot,
        ));
        observer.observe_kind(settled(888, 88, 284));
        let report = observer.report();
        assert_eq!(
            (
                report.model_calls,
                report.input_tokens,
                report.output_tokens,
                report.cost_micro_usd
            ),
            (2, 1608, 146, 498)
        );
        assert!(observer.healthy());
        assert!(!report.accepted);
        observer.observe_kind(settled(u64::MAX, 0, 0));
        observer.observe_kind(AgentWorkEventKind::ToolProposed(
            AgentBrowserToolKind::Snapshot,
        ));
        assert!(!observer.healthy());
        assert!(!observer.report().accepted);
    }
}
