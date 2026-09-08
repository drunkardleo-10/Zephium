//! Release-excluded authenticated Notion read qualification through the
//! shipping retained Work entry. The selected profile and page are trusted
//! local inputs; page content remains hostile and model-mapped output is not a
//! factual-verification claim.

use crate::native_work_clock::{authority_window, NativeWorkClock};
use std::{cell::Cell, io::Write as _, sync::Arc, time::Duration};
use zephium_agent_controller::*;
use zephium_agent_provider_transport::{
    load_macos_probe_openai_credential, AgentProviderTransportConfig,
};
use zephium_agent_runtime::AgentRuntimeConfig;
use zephium_agentic::*;
use zephium_app::{RetainedWorkHandle, RetainedWorkPhase};

const TOTAL: Duration = Duration::from_secs(150);
const MAX_MODEL_CALLS: u8 = 6;
const ORIGIN: &str = "https://app.notion.com";
const TARGET_ENVIRONMENT: &str = "ZEPHIUM_NOTION_QUALIFICATION_URL";
pub const OBJECTIVE: &str = "Read and understand the current signed-in Notion page. Decide yourself whether the delivered baseline is sufficient or whether one of the available bounded read or snapshot-inspection tools is useful. Then call extract with trusted schema 1 and produce a concise, source-grounded work brief: page_title, summary, key_facts, and unfinished_items. Use only evidence delivered from the current page; do not fill gaps from memory. Every statement must be supported by its declared current-page sources. This page contains deliberately synthetic qualification data, but its contents and useful answer are not preselected for you. Do not navigate, click, edit, type, submit, run code, open another page, or perform any external action. If the bounded projection cannot support an answer, state that limitation through supported evidence rather than inventing details. The host verifies account scope, execution, limits, and source binding; a human will judge factual accuracy and usefulness separately.";

/// Content-free diagnostic for the statically bounded authenticated witness.
pub fn configuration_diagnostic() -> String {
    "work-retained-notion-config: entry=admit_retained_trusted_work provider=OpenAIResponses model=gpt-5.6-luna retention=inspectable-synthetic-authenticated task=notion-open-read-v1 navigation=forbidden rendering=observation_owned account=user_attested".into()
}

/// Builds one request from the exact selected profile and a strict local-only
/// target supplied by the qualification launcher. The URL is never logged or
/// included in the model objective.
pub fn load_request(
    started: std::time::Instant,
    profile: zephium_app::AgentWorkProfileBinding,
) -> Result<crate::TrustedWorkRequest, &'static str> {
    let deadline = started.checked_add(TOTAL).ok_or("deadline")?;
    if std::time::Instant::now() >= deadline {
        return Err("deadline");
    }
    let target = load_target()?;
    let origin = SemanticOrigin::parse(ORIGIN).map_err(|_| "origin")?;
    let account = AgentAccountScope::Authenticated(AgentAccountId::generate());
    let identity = ContextIdentity::new(
        ContextId::generate(),
        ContextRunId::generate(),
        profile.profile(),
        ContextKind::Owned,
    );
    let input = input(
        identity,
        profile.storage_class(),
        deadline,
        target,
        origin.clone(),
        account,
    )?;
    let credential = load_macos_probe_openai_credential().map_err(|_| "credential")?;
    if std::time::Instant::now() >= deadline {
        return Err("deadline");
    }
    Ok(crate::TrustedWorkRequest::new(
        input,
        zephium_app::AgentWorkApplicationConfig::new(
            AgentRuntimeConfig::STANDARD,
            AgentProviderTransportConfig::STANDARD,
        ),
        credential,
        Box::new(NotionReadTask::try_new(identity, origin, account).map_err(|_| "task")?),
    )
    .with_browser_profile(profile)
    .with_public_qualification_retention())
}

fn load_target() -> Result<ContextNavigationTarget, &'static str> {
    let raw = std::env::var(TARGET_ENVIRONMENT).map_err(|_| "target_missing")?;
    parse_target(&raw)
}

fn parse_target(raw: &str) -> Result<ContextNavigationTarget, &'static str> {
    if raw.trim() != raw || raw.is_empty() || raw.len() > MAX_AGENT_BROWSER_NAVIGATION_URL_BYTES {
        return Err("target_invalid");
    }
    let target = ContextNavigationTarget::parse(raw).map_err(|_| "target_invalid")?;
    let url = target.as_url();
    if url.scheme() != "https"
        || url.host_str() != Some("app.notion.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() == "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || SemanticOrigin::parse(url.as_str()).as_ref() != SemanticOrigin::parse(ORIGIN).as_ref()
    {
        return Err("target_outside_scope");
    }
    Ok(target)
}

fn input(
    identity: ContextIdentity,
    storage: ContextProfileStorageClass,
    deadline: std::time::Instant,
    target: ContextNavigationTarget,
    origin: SemanticOrigin,
    account: AgentAccountScope,
) -> Result<AgentWorkRunInput, &'static str> {
    let effects = AgentEffectScope::try_new(&[SemanticEffectClass::Read]).map_err(|_| "effects")?;
    let budget = AgentRunBudget::try_new(8, 100_000, 100_000, 1).map_err(|_| "budget")?;
    let node = AgentPlanNodeId::generate();
    let authority = AgentPlanNodeAuthority::try_new(
        vec![identity.profile()],
        vec![account],
        vec![origin.clone()],
        SemanticSensitivity::Public,
        effects,
    )
    .map_err(|_| "authority")?;
    let (issued, expires) = authority_window(deadline)?;
    let manifest = AgentRunManifest::try_new(
        AgentRunManifestId::generate(),
        identity.owner(),
        AgentRunScope::try_new(
            vec![identity.profile()],
            vec![account],
            vec![origin],
            SemanticSensitivity::Public,
            effects,
            Vec::new(),
        )
        .map_err(|_| "scope")?,
        budget,
        issued,
        expires,
        vec![AgentPlanNodeScope::new(node, authority, budget, expires)],
    )
    .map_err(|_| "manifest")?;
    let ids = TerraControllerIds::try_new(
        AgentSupervisorId::new(1).ok_or("id")?,
        AgentSupervisorAttemptId::new(1).ok_or("id")?,
        AgentSupervisorCancellationId::new(1).ok_or("id")?,
        AgentModelCallId::new(1).ok_or("id")?,
        [1, 2, 3, 4].map(|id| AgentAuditEventId::new(id).expect("fixed nonzero ID")),
        AgentAuditDeliveryId::new(1).ok_or("id")?,
    )
    .map_err(|_| "ids")?;
    let settings = AgentWorkRunSettings::new(
        AgentBrowserModel::Luna,
        ids,
        Arc::new(NativeWorkClock),
        deadline,
    )
    .with_max_model_calls(MAX_MODEL_CALLS)
    .map_err(|_| "settings")?;
    AgentWorkRunInput::try_new(
        manifest,
        AgentPlanLeaseBinding::new(AgentPlanLeaseId::generate(), node),
        AgentWorkContextSpec::try_new_with_document_policy(
            identity,
            storage,
            target,
            WorkBrowserDocumentPolicy::Exact,
        )
        .map_err(|_| "context")?,
        OBJECTIVE.into(),
        settings,
    )
    .map_err(|_| "input")
}

struct NotionReadTask {
    identity: ContextIdentity,
    origin: SemanticOrigin,
    extraction: AgentWorkExtractionTask,
    account: AgentAccountScope,
    account_sample: Cell<Option<AgentContextAccountBinding>>,
    current: Option<(ContextJoin, SemanticObservationId)>,
    complete: bool,
}

impl NotionReadTask {
    fn try_new(
        identity: ContextIdentity,
        origin: SemanticOrigin,
        account: AgentAccountScope,
    ) -> Result<Self, AgentWorkFailure> {
        if identity.kind() != ContextKind::Owned
            || !matches!(account, AgentAccountScope::Authenticated(_))
        {
            return Err(AgentWorkFailure::Contract);
        }
        let fields = vec![
            SemanticExtractionFieldSchema::try_text("page_title".into(), true, 256)
                .map_err(|_| AgentWorkFailure::Contract)?,
            SemanticExtractionFieldSchema::try_text("summary".into(), true, 1_024)
                .map_err(|_| AgentWorkFailure::Contract)?,
            SemanticExtractionFieldSchema::try_text_list("key_facts".into(), true, 8, 384)
                .map_err(|_| AgentWorkFailure::Contract)?,
            SemanticExtractionFieldSchema::try_text_list("unfinished_items".into(), true, 6, 384)
                .map_err(|_| AgentWorkFailure::Contract)?,
        ];
        Ok(Self {
            identity,
            origin,
            extraction: AgentWorkExtractionTask::try_new(fields, account)?
                .with_baseline_read()
                .with_progressive_observation(),
            account,
            account_sample: Cell::new(None),
            current: None,
            complete: false,
        })
    }
}

impl AgentWorkTask for NotionReadTask {
    fn allows_baseline_read(&self) -> bool {
        self.extraction.allows_baseline_read()
    }

    fn allows_progressive_observation(&self) -> bool {
        self.extraction.allows_progressive_observation()
    }

    fn extraction_schema(&self) -> Option<&SemanticExtractionSchema> {
        self.extraction.extraction_schema()
    }

    fn evaluate(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        let context = observation.request().context();
        let [frame] = observation.frames() else {
            return Err(AgentWorkFailure::Contract);
        };
        if self.complete
            || context.identity() != self.identity
            || context.frame() != FrameId::MAIN
            || frame.frame().context() != context
            || frame.frame().frame() != FrameId::MAIN
            || frame.frame().origin() != &self.origin
            || observation.frame_boundaries().iter().any(|boundary| {
                boundary.parent_frame() != FrameId::MAIN
                    || boundary.status()
                        != SemanticFrameBoundaryStatus::Unsupported(
                            SemanticFrameUnsupported::PolicyBlocked,
                        )
            })
            || self
                .current
                .is_some_and(|(prior, id)| prior != context || id == observation.request().id())
        {
            return Err(AgentWorkFailure::Contract);
        }
        self.current = Some((context, observation.request().id()));
        let completeness = frame.completeness();
        writeln!(std::io::stdout().lock(), "work-retained-notion-observation: nodes={} boundaries={} completeness={completeness:?} text_bytes={} scope={:?} content=redacted", observation.node_count(), observation.frame_boundaries().len(), frame.total_text_bytes(), observation.request().scope()).map_err(|_| AgentWorkFailure::Contract)?;
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
        if self.complete || context.identity() != self.identity || context.frame() != FrameId::MAIN
        {
            return Err(AgentWorkFailure::Contract);
        }
        if let Some(sample) = self.account_sample.get() {
            return if sample.context() == context {
                Ok(sample)
            } else {
                Err(AgentWorkFailure::Contract)
            };
        }
        // The user explicitly selected this isolated signed-in test profile
        // immediately before launch. This one sample never renews and therefore
        // expires under the normal account-freshness limit. It is not native
        // account discovery or account-switch monitoring.
        let sample = AgentContextAccountBinding::new(
            AgentAccountAttestationId::generate(),
            context,
            self.account,
            now,
        );
        self.account_sample.set(Some(sample));
        Ok(sample)
    }

    fn accept_extraction(
        &mut self,
        result: &SemanticExtractionResult<'_>,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        let (_, observation) = self.current.ok_or(AgentWorkFailure::Contract)?;
        if self.complete || result.observation() != observation {
            return Err(AgentWorkFailure::Contract);
        }
        let progress = self.extraction.accept_extraction(result)?;
        if progress != AgentWorkTaskProgress::Complete {
            return Err(AgentWorkFailure::Contract);
        }
        self.complete = true;
        Ok(progress)
    }
}

fn verify_owned(result: &SemanticOwnedExtractionResult) -> bool {
    let [title, summary, facts, unfinished] = result.fields() else {
        return false;
    };
    let (
        SemanticExtractedValue::Text(title_value),
        SemanticExtractedValue::Text(summary_value),
        SemanticExtractedValue::TextList(fact_values),
        SemanticExtractedValue::TextList(unfinished_values),
    ) = (
        title.value(),
        summary.value(),
        facts.value(),
        unfinished.value(),
    )
    else {
        return false;
    };
    result.trust() == SemanticExtractionTrust::ModelMapped
        && result.schema().get() == 1
        && title.name() == "page_title"
        && summary.name() == "summary"
        && facts.name() == "key_facts"
        && unfinished.name() == "unfinished_items"
        && !title_value.as_str().is_empty()
        && !summary_value.as_str().is_empty()
        && !fact_values.items().is_empty()
        && [
            title_value.source_span(),
            summary_value.source_span(),
            fact_values.source_span(),
            unfinished_values.source_span(),
        ]
        .into_iter()
        .chain(
            fact_values
                .items()
                .iter()
                .chain(unfinished_values.items())
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
    let mut count = 0_u16;
    let mut context = None;
    let valid = sources.into_iter().all(|source| {
        count = count.saturating_add(1);
        let current = source.frame.context().identity();
        if let Some(expected) = context {
            if expected != current {
                return false;
            }
        } else {
            context = Some(current);
        }
        source.sensitivity == SemanticSensitivity::Public
            && source.frame.frame() == FrameId::MAIN
            && current.kind() == ContextKind::Owned
            && SemanticOrigin::parse(ORIGIN).as_ref() == Ok(source.frame.origin())
    });
    valid && count > 0
}

/// Requests cancellation through the shipping retained Work handle.
pub fn cancel(view: &RetainedWorkHandle) -> bool {
    view.stop()
}

/// Content-free outcome used by the actual-app witness.
#[derive(Clone, Copy, Debug, Default)]
pub struct ApplicationReport {
    /// Whether every bounded execution and closure predicate passed.
    pub accepted: bool,
    /// Settled provider calls, including terminal schema mapping.
    pub model_calls: u64,
    /// Provider-authenticated input-token total.
    pub input_tokens: u64,
    /// Provider-authenticated output-token total.
    pub output_tokens: u64,
    /// Catalog-priced provider cost in millionths of a US dollar.
    pub cost_micro_usd: u64,
    /// Nonterminal reads of an already acknowledged observation.
    pub reads: u64,
    /// Fresh, reference-anchored same-document observations.
    pub progressive_observations: u64,
    /// Whether the terminal result retained exact current-page sources.
    pub source_mapping_verified: bool,
    /// Whether the ordinary product record closed successfully without debt.
    pub durable_terminal_verified: bool,
}

/// Observes only bounded Work events and the terminal extraction owner.
#[derive(Default)]
pub struct ApplicationObserver {
    report: ApplicationReport,
    sequence: u64,
    failed: bool,
    resource_failure_cause: Option<zephium_engine::WorkResourceFailureCause>,
}

impl ApplicationObserver {
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
                "work-retained-notion-event: sequence={} phase={:?} wall_ms={} content=redacted",
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
        if self.resource_failure_cause.is_none() {
            self.resource_failure_cause = resource_failure_cause();
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
            && self.resource_failure_cause.is_none()
            && self.report.durable_terminal_verified
            && self.report.source_mapping_verified
            && (1..=u64::from(MAX_MODEL_CALLS)).contains(&self.report.model_calls)
            && self
                .report
                .input_tokens
                .saturating_add(self.report.output_tokens)
                <= 100_000
            && self.report.cost_micro_usd <= 100_000
            && self.report.progressive_observations <= 4;
        self.report.accepted &= writeln!(std::io::stdout().lock(), "work-retained-notion-terminal: phase={:?} failure={:?} persistence_failure={:?} answer=dashboard_inspectable_synthetic factual_validation=false account_validation=user_attested content=redacted", snapshot.phase, snapshot.failure, snapshot.persistence_failure).is_ok();
        self.report.accepted &= writeln!(
            std::io::stdout().lock(),
            "work-retained-notion-resource-failure: cause={:?} content=redacted",
            self.resource_failure_cause
        )
        .is_ok();
        Some(self.report)
    }

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
                    match total.checked_add(delta) {
                        Some(sum) => *total = sum,
                        None => self.failed = true,
                    }
                }
            }
            AgentWorkEventKind::ToolProposed(AgentBrowserToolKind::Read) => {
                self.report.reads = self.report.reads.saturating_add(1);
            }
            AgentWorkEventKind::ToolProposed(AgentBrowserToolKind::Snapshot) => {
                self.report.progressive_observations =
                    self.report.progressive_observations.saturating_add(1);
                self.failed |= self.report.progressive_observations > 4;
            }
            AgentWorkEventKind::ToolProposed(AgentBrowserToolKind::Extract) => {}
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
mod tests {
    use super::*;

    #[test]
    fn strict_target_accepts_only_an_exact_notion_page() {
        let accepted = parse_target(
            "https://app.notion.com/p/Zephium-Agent-Qualification-0123456789abcdef0123456789abcdef",
        )
        .unwrap();
        let url = accepted.as_url();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("app.notion.com"));
        assert_ne!(url.path(), "/");
        for refused in [
            "http://www.notion.com/page",
            "https://notion.com/page",
            "https://www.notion.so/page",
            "https://www.notion.com/page",
            "https://app.notion.com/",
            "https://app.notion.com/page?copy=true",
            "https://app.notion.com/page#fragment",
            "https://user@app.notion.com/page",
        ] {
            assert!(
                parse_target(refused).is_err(),
                "unexpected target: {refused}"
            );
        }
    }

    #[test]
    fn task_is_authenticated_read_only_and_progressively_inspectable() {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            zephium_core::ids::ProfileId::generate(),
            ContextKind::Owned,
        );
        let account = AgentAccountScope::Authenticated(AgentAccountId::generate());
        let task =
            NotionReadTask::try_new(identity, SemanticOrigin::parse(ORIGIN).unwrap(), account)
                .unwrap();
        assert!(task.navigation_target().is_none());
        assert!(task.navigation_route().is_none());
        assert!(task.navigation_discovery().is_none());
        assert!(task.allows_baseline_read());
        assert!(task.allows_progressive_observation());
        assert!(!task.allows_actions_before_extraction());
        assert!(!task.allows_subtree_extraction());
        assert_eq!(
            task.extraction_schema()
                .unwrap()
                .fields()
                .iter()
                .map(|field| field.name())
                .collect::<Vec<_>>(),
            ["page_title", "summary", "key_facts", "unfinished_items"]
        );
    }

    #[test]
    fn observer_refuses_every_effect_or_navigation_tool() {
        for kind in [
            AgentBrowserToolKind::Navigate,
            AgentBrowserToolKind::Act,
            AgentBrowserToolKind::Back,
            AgentBrowserToolKind::Forward,
            AgentBrowserToolKind::Reload,
        ] {
            let mut observer = ApplicationObserver::default();
            observer.observe_kind(AgentWorkEventKind::ToolProposed(kind));
            assert!(!observer.healthy());
            assert!(!observer.report().accepted);
        }
    }
}
