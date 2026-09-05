//! Explicit public-data qualification through the shipping actor and real port.

use std::io::Write as _;
use std::sync::Arc;
use std::time::{Duration, Instant};
use zephium_agent_controller::*;
use zephium_agent_provider_transport::{
    load_macos_probe_openai_credential, AgentProviderTransport, AgentProviderTransportConfig,
};
use zephium_agent_runtime::{AgentRuntimeConfig, PendingAgentRuntime};
use zephium_agentic::*;

struct Clock(Instant);
impl TerraControllerClock for Clock {
    fn now(&self) -> Result<AgentPolicyInstant, TerraControllerClockError> {
        let elapsed = u64::try_from(self.0.elapsed().as_millis())
            .map_err(|_| TerraControllerClockError::Invalid)?;
        Ok(AgentPolicyInstant::from_millis(1_000 + elapsed))
    }
}

struct Task(super::PreparedSearchProgress);
impl AgentWorkTask for Task {
    fn evaluate(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        let (mut initial, mut final_query, mut language) = (false, false, false);
        for node in observation.frames().iter().flat_map(|frame| frame.nodes()) {
            if matches!(node.role(), SemanticRole::Searchbox | SemanticRole::Textbox) {
                if let Some(SemanticValueSummary::Text(value)) = node.value() {
                    let value = value.preview();
                    initial |= !value.truncated()
                        && value.source_bytes() == "Zephium browser".len()
                        && value.text() == "Zephium browser";
                    final_query |= !value.truncated()
                        && value.source_bytes() == "Zephium open source browser".len()
                        && value.text() == "Zephium open source browser";
                }
            }
            language |= node.role() == SemanticRole::Option
                && node.name().is_some_and(|name| name.as_str() == "Deutsch")
                && node.states().contains(SemanticState::Selected);
        }
        Ok(if self.0.observe(initial, final_query, language) {
            AgentWorkTaskProgress::Complete
        } else {
            AgentWorkTaskProgress::Continue
        })
    }
    fn assess(
        &self,
        action: &SemanticPreparedAction,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        if !matches!(
            action.kind(),
            SemanticActionKind::Fill | SemanticActionKind::Select
        ) {
            return Err(AgentWorkFailure::Contract);
        }
        Ok(AgentEffectAssessment::new(
            action,
            action.frame().origin().clone(),
            SemanticEffectClass::LocalWrite,
        ))
    }
    fn attest_account(
        &self,
        context: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        Ok(AgentContextAccountBinding::new(
            AgentAccountAttestationId::generate(),
            context,
            AgentAccountScope::Anonymous,
            now,
        ))
    }
}

pub(super) fn input(
    started: Instant,
) -> Result<
    (
        zephium_core::ids::ProfileId,
        AgentWorkRunInput,
        Box<dyn AgentWorkTask>,
    ),
    super::ProbeFailure,
> {
    use super::ProbeFailure as Error;
    let profile = 1_u128.into();
    let context = ContextIdentity::new(
        ContextId::generate(),
        ContextRunId::generate(),
        profile,
        ContextKind::Owned,
    );
    let origin =
        SemanticOrigin::parse("https://www.wikipedia.org/").map_err(|_| Error::Authority)?;
    let effects =
        AgentEffectScope::try_new(&[SemanticEffectClass::Read, SemanticEffectClass::LocalWrite])
            .map_err(|_| Error::Authority)?;
    let budget =
        AgentRunBudget::try_new(24, 1_000_000, 1_000_000, 1).map_err(|_| Error::Authority)?;
    let node = AgentPlanNodeId::generate();
    let manifest = AgentRunManifest::try_new(
        AgentRunManifestId::generate(),
        context.owner(),
        AgentRunScope::try_new(
            vec![profile],
            vec![AgentAccountScope::Anonymous],
            vec![origin.clone()],
            SemanticSensitivity::Public,
            effects,
            Vec::new(),
        )
        .map_err(|_| Error::Authority)?,
        budget,
        AgentPolicyInstant::from_millis(1_000),
        AgentPolicyInstant::from_millis(200_000),
        vec![AgentPlanNodeScope::new(
            node,
            AgentPlanNodeAuthority::try_new(
                vec![profile],
                vec![AgentAccountScope::Anonymous],
                vec![origin],
                SemanticSensitivity::Public,
                effects,
            )
            .map_err(|_| Error::Authority)?,
            budget,
            AgentPolicyInstant::from_millis(199_999),
        )],
    )
    .map_err(|_| Error::Authority)?;
    let objective = "Prepare a public Wikipedia search without submitting or navigating. Fill the search with exactly Zephium browser and choose Deutsch in the search language selector, in either order. Once both are verified, refine the search text to exactly Zephium open source browser. Use one local_write act action per turn. Locate option references when needed. For each action use mutation_quiet=100 ms, settle_budget=1000 ms, and exact value or exact selected-option verification. Do not click links or submit. The host checks the exact milestones and stops when the final prepared search is verified.";
    let input = AgentWorkRunInput::try_new(
        manifest,
        AgentPlanLeaseBinding::new(AgentPlanLeaseId::generate(), node),
        AgentWorkContextSpec::try_new(
            context,
            ContextProfileStorageClass::Ephemeral,
            ContextNavigationTarget::parse("https://www.wikipedia.org/")
                .map_err(|_| Error::Authority)?,
        )
        .map_err(|_| Error::Authority)?,
        objective.to_owned(),
        AgentWorkRunSettings::new(
            AgentBrowserModel::Luna,
            super::probe_controller_ids()?,
            Arc::new(Clock(started)),
            started + Duration::from_secs(150),
        ),
    )
    .map_err(|_| Error::Authority)?;
    Ok((profile, input, Box::new(Task(Default::default()))))
}

pub(super) fn run() -> Result<(), super::ProbeFailure> {
    use super::ProbeFailure as Error;
    let started = Instant::now();
    let (profile, input, task) = input(started)?;
    let data = tempfile::tempdir().map_err(|_| Error::Runtime)?;
    let store =
        Arc::new(zephium_store::SqliteStore::open(data.path()).map_err(|_| Error::Runtime)?);
    let credential = load_macos_probe_openai_credential().map_err(|_| Error::Keychain)?;
    let transport = AgentProviderTransport::try_new(AgentProviderTransportConfig::STANDARD)
        .map_err(|_| Error::Runtime)?;
    let (controller, mut handle) = AgentWorkController::try_new_for_probe(
        input,
        transport,
        credential,
        store.clone(),
        task,
        AgentBrowserRetention::InspectablePublicData,
    )
    .map_err(|_| Error::Authority)?;
    let pending = PendingAgentRuntime::spawn_suspended_with_controller(
        AgentRuntimeConfig::STANDARD,
        Box::new(controller),
    )
    .map_err(|_| Error::Runtime)?;
    let sink = pending.native_event_sink();
    let result = zephium_engine::run_macos_agentic_work_actor_probe(
        profile,
        move |event| {
            let _ = sink.publish(event);
        },
        move |port| {
            let (runtime, completion, lifecycle) = pending.bind_browser_port(port).into_parts();
            let mut lifecycle = Some(lifecycle);
            runtime.start_run().map_err(|_| "actor_start")?;
            Ok(Box::new(move |native_failed| {
                if native_failed {
                    runtime.cancel_and_seal();
                }
                while let Some(event) = handle.take_event() {
                    let mut output = std::io::stdout().lock();
                    let result = match event.kind() {
                    AgentWorkEventKind::ModelSettled { call, input_tokens, output_tokens, request_bytes, semantic_bytes, cost_micro_usd, accounting, elapsed_millis } => writeln!(output, "work-actor-turn: call={}; input_tokens={input_tokens}; output_tokens={output_tokens}; request_bytes={request_bytes}; semantic_bytes={semantic_bytes}; cost_micro_usd={cost_micro_usd}; accounting={accounting:?}; turn_ms={elapsed_millis}; wall_ms={}; content=redacted", call.get(), event.elapsed_millis()),
                    kind => writeln!(output, "work-actor-event: sequence={}; phase={kind:?}; wall_ms={}; content=redacted", event.sequence(), event.elapsed_millis()),
                };
                    if result.is_err() {
                        runtime.cancel_and_seal();
                        return Some(Err("actor_output"));
                    }
                }
                if !completion.is_stopped() {
                    return None;
                }
                let outcome = handle.take_outcome();
                let native = lifecycle
                    .take()?
                    .shutdown_until(Instant::now() + Duration::from_secs(2));
                Some(match (outcome, native) {
                (Some(AgentWorkOutcome::Succeeded(settlement)), AgentBrowserShutdownOutcome::Clean(_)) => {
                    if writeln!(std::io::stdout().lock(), "work-actor-terminal: state=succeeded; model_calls={}; verified_effects={}; lifecycle=clean; content=redacted", settlement.closure().model_calls(), settlement.closure().effects()).is_err() { Err("actor_output") } else { Ok(()) }
                }
                (Some(AgentWorkOutcome::Recovery(recovery)), _) => {
                    let _ = writeln!(std::io::stdout().lock(), "work-actor-recovery: reason={:?}; retained_callbacks={}; content=redacted", recovery.failure(), recovery.retained_callbacks());
                    Err("actor_recovery")
                }
                _ => Err("actor_terminal_unproven"),
            })
            }))
        },
    );
    let store_closed = store.shutdown_until(Instant::now() + Duration::from_secs(2));
    result.map_err(Error::Engine)?;
    if store_closed != zephium_core::ports::store::StoreShutdownOutcome::Clean {
        return Err(Error::Engine("actor_store_teardown"));
    }
    writeln!(std::io::stdout().lock(), "work-actor-qualified: model=gpt-5.6-luna; store=durable; native=production_port; focus_isolation=passed; elapsed_ms={}; content=redacted", started.elapsed().as_millis()).map_err(|_| Error::Output)?;
    Ok(())
}
