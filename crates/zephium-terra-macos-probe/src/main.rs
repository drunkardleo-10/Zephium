//! Explicit, release-excluded live Terra click qualification.

#![forbid(unsafe_code)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]
#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unreachable, clippy::unwrap_used)
)]

#[cfg(not(target_os = "macos"))]
fn main() {
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    use std::io::Write as _;

    if std::env::args_os().skip(1).collect::<Vec<_>>() != ["--live-fixed-click"] {
        std::process::exit(2);
    }
    if let Err(error) = run() {
        let _ = writeln!(
            std::io::stderr().lock(),
            "macos-terra-agentic-probe: failed; stage={}; protocol_event={}; content=redacted",
            error.label(),
            error.protocol_event_label()
        );
        std::process::exit(1);
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
enum ProbeFailure {
    Runtime,
    Keychain,
    Authority,
    Provider(zephium_agent_controller::TerraProbeProviderError),
    Proposal,
    Engine,
    Verification,
    Metrics,
    Output,
}

#[cfg(target_os = "macos")]
impl ProbeFailure {
    const fn label(self) -> &'static str {
        use zephium_agent_controller::TerraProbeProviderError;
        use zephium_agentic::{AgentProviderFailureClass, AgentProviderProtocolError};

        match self {
            Self::Runtime => "runtime",
            Self::Keychain => "keychain",
            Self::Authority => "authority",
            Self::Provider(TerraProbeProviderError::Authority) => "provider_authority",
            Self::Provider(TerraProbeProviderError::Catalog) => "provider_catalog",
            Self::Provider(TerraProbeProviderError::Clock) => "provider_clock",
            Self::Provider(TerraProbeProviderError::Deadline) => "provider_deadline",
            Self::Provider(TerraProbeProviderError::Encoding) => "provider_encoding",
            Self::Provider(TerraProbeProviderError::Transport) => "provider_transport",
            Self::Provider(TerraProbeProviderError::PreDispatchTerminal(_)) => {
                "provider_pre_dispatch_terminal"
            }
            Self::Provider(TerraProbeProviderError::CountTerminal(
                AgentProviderFailureClass::Authentication,
            )) => "provider_count_authentication",
            Self::Provider(TerraProbeProviderError::CountTerminal(_)) => "provider_count_terminal",
            Self::Provider(TerraProbeProviderError::ModelTerminal(
                AgentProviderFailureClass::Authentication,
            )) => "provider_model_authentication",
            Self::Provider(TerraProbeProviderError::ModelTerminal(
                AgentProviderFailureClass::InvalidRequest,
            )) => "provider_model_invalid_request",
            Self::Provider(TerraProbeProviderError::ModelTerminal(
                AgentProviderFailureClass::Permission,
            )) => "provider_model_permission",
            Self::Provider(TerraProbeProviderError::ModelTerminal(
                AgentProviderFailureClass::NotFound,
            )) => "provider_model_not_found",
            Self::Provider(TerraProbeProviderError::ModelTerminal(
                AgentProviderFailureClass::Conflict,
            )) => "provider_model_conflict",
            Self::Provider(TerraProbeProviderError::ModelTerminal(
                AgentProviderFailureClass::RateLimited,
            )) => "provider_model_rate_limited",
            Self::Provider(TerraProbeProviderError::ModelTerminal(
                AgentProviderFailureClass::Overloaded,
            )) => "provider_model_overloaded",
            Self::Provider(TerraProbeProviderError::ModelTerminal(
                AgentProviderFailureClass::Timeout,
            )) => "provider_model_timeout",
            Self::Provider(TerraProbeProviderError::ModelTerminal(
                AgentProviderFailureClass::Transport,
            )) => "provider_model_transport",
            Self::Provider(TerraProbeProviderError::ModelTerminal(
                AgentProviderFailureClass::Protocol,
            )) => "provider_model_protocol",
            Self::Provider(TerraProbeProviderError::ModelTerminal(
                AgentProviderFailureClass::Integration,
            )) => "provider_model_integration",
            Self::Provider(TerraProbeProviderError::ModelTerminal(
                AgentProviderFailureClass::Provider,
            )) => "provider_model_provider",
            Self::Provider(TerraProbeProviderError::ModelTerminal(
                AgentProviderFailureClass::Cancelled,
            )) => "provider_model_cancelled",
            Self::Provider(TerraProbeProviderError::ModelProtocol(
                AgentProviderProtocolError::Framing,
                _,
            )) => "provider_model_protocol_framing",
            Self::Provider(TerraProbeProviderError::ModelProtocol(
                AgentProviderProtocolError::Limit,
                _,
            )) => "provider_model_protocol_limit",
            Self::Provider(TerraProbeProviderError::ModelProtocol(
                AgentProviderProtocolError::Event,
                _,
            )) => "provider_model_protocol_event",
            Self::Provider(TerraProbeProviderError::ModelProtocol(
                AgentProviderProtocolError::Sequence,
                _,
            )) => "provider_model_protocol_sequence",
            Self::Provider(TerraProbeProviderError::ModelProtocol(
                AgentProviderProtocolError::UnsupportedOutput,
                _,
            )) => "provider_model_protocol_unsupported_output",
            Self::Provider(TerraProbeProviderError::ModelProtocol(
                AgentProviderProtocolError::ToolCall,
                _,
            )) => "provider_model_protocol_tool_call",
            Self::Provider(TerraProbeProviderError::ModelProtocol(
                AgentProviderProtocolError::Usage,
                _,
            )) => "provider_model_protocol_usage",
            Self::Provider(TerraProbeProviderError::ModelProtocol(
                AgentProviderProtocolError::Terminal,
                _,
            )) => "provider_model_protocol_terminal",
            Self::Provider(TerraProbeProviderError::Settlement) => "provider_settlement",
            Self::Provider(TerraProbeProviderError::Proposal) => "provider_proposal",
            Self::Proposal => "proposal_contract",
            Self::Engine => "native_engine",
            Self::Verification => "fresh_snapshot_verification",
            Self::Metrics => "metrics",
            Self::Output => "output",
        }
    }

    const fn protocol_event_label(self) -> &'static str {
        use zephium_agent_controller::TerraProbeProviderError;
        use zephium_agentic::AgentProviderProtocolEvent;

        let Self::Provider(TerraProbeProviderError::ModelProtocol(_, event)) = self else {
            return "not_applicable";
        };
        match event {
            Some(AgentProviderProtocolEvent::OpenAiCreated) => "openai_created",
            Some(AgentProviderProtocolEvent::OpenAiInProgress) => "openai_in_progress",
            Some(AgentProviderProtocolEvent::OpenAiOutputItemAdded) => "openai_output_item_added",
            Some(AgentProviderProtocolEvent::OpenAiOutputItemDone) => "openai_output_item_done",
            Some(AgentProviderProtocolEvent::OpenAiContentPart) => "openai_content_part",
            Some(AgentProviderProtocolEvent::OpenAiText) => "openai_text",
            Some(AgentProviderProtocolEvent::OpenAiToolArgumentsDelta) => {
                "openai_tool_arguments_delta"
            }
            Some(AgentProviderProtocolEvent::OpenAiToolArgumentsDone) => {
                "openai_tool_arguments_done"
            }
            Some(AgentProviderProtocolEvent::OpenAiTerminal) => "openai_terminal",
            Some(AgentProviderProtocolEvent::OpenAiError) => "openai_error",
            Some(AgentProviderProtocolEvent::OpenAiDone) => "openai_done",
            Some(AgentProviderProtocolEvent::OpenAiUnknown) => "openai_unknown",
            None => "unavailable",
        }
    }
}

#[cfg(target_os = "macos")]
fn run() -> Result<(), ProbeFailure> {
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use zephium_agent_controller::{
        run_initial_terra_probe, TerraControllerClock, TerraControllerIds, TerraControllerRunInput,
        TerraControllerTurnInput, TerraProbeActionBridge,
    };
    use zephium_agent_provider_transport::{
        load_macos_development_openai_credential, AgentProviderTransportConfig,
    };
    use zephium_agentic::{
        AgentAuditDeliveryId, AgentAuditEventId, AgentModelCallId, AgentPolicyInstant,
        AgentSupervisorAttemptId, AgentSupervisorCancellationId, AgentSupervisorId,
        SemanticActionExecutionInstant,
    };

    struct Clock(AgentPolicyInstant);
    impl TerraControllerClock for Clock {
        fn now(
            &self,
        ) -> Result<AgentPolicyInstant, zephium_agent_controller::TerraControllerClockError>
        {
            Ok(self.0)
        }
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| ProbeFailure::Runtime)?;
    let mut credential =
        Some(load_macos_development_openai_credential().map_err(|_| ProbeFailure::Keychain)?);
    let mut bridge = None;
    let mut metrics = None;
    let mut callback_failure = None;
    let started = Instant::now();
    let terminal_result = zephium_engine::run_macos_agentic_semantic_model_click_probe(
        |observation, authority| {
            let (manifest, lease, account, request, frame, invocation, generation, now) =
                authority.into_parts();
            let turn = TerraControllerTurnInput::try_new(
                account,
                request,
                frame,
                invocation,
                generation,
                "Return exactly one act action: click the control named Primary semantic action; effect=local_write; wait=target_state expanded true; verification=target_state expanded true; use a bounded settle budget."
                    .to_owned(),
            )
            .map_err(|_| {
                callback_failure = Some(ProbeFailure::Authority);
            })?;
            let ids = TerraControllerIds::try_new(
                AgentSupervisorId::new(1).ok_or_else(|| {
                    callback_failure = Some(ProbeFailure::Authority);
                })?,
                AgentSupervisorAttemptId::new(1).ok_or_else(|| {
                    callback_failure = Some(ProbeFailure::Authority);
                })?,
                AgentSupervisorCancellationId::new(1).ok_or_else(|| {
                    callback_failure = Some(ProbeFailure::Authority);
                })?,
                AgentModelCallId::new(1).ok_or_else(|| {
                    callback_failure = Some(ProbeFailure::Authority);
                })?,
                [
                    AgentAuditEventId::new(1).ok_or_else(|| {
                        callback_failure = Some(ProbeFailure::Authority);
                    })?,
                    AgentAuditEventId::new(2).ok_or_else(|| {
                        callback_failure = Some(ProbeFailure::Authority);
                    })?,
                    AgentAuditEventId::new(3).ok_or_else(|| {
                        callback_failure = Some(ProbeFailure::Authority);
                    })?,
                    AgentAuditEventId::new(4).ok_or_else(|| {
                        callback_failure = Some(ProbeFailure::Authority);
                    })?,
                ],
                AgentAuditDeliveryId::new(1).ok_or_else(|| {
                    callback_failure = Some(ProbeFailure::Authority);
                })?,
            )
            .map_err(|_| {
                callback_failure = Some(ProbeFailure::Authority);
            })?;
            let input = TerraControllerRunInput::try_new(
                manifest,
                lease,
                turn,
                ids,
                Arc::new(Clock(now)),
                Instant::now()
                    .checked_add(Duration::from_secs(120))
                    .ok_or_else(|| {
                        callback_failure = Some(ProbeFailure::Authority);
                    })?,
            )
            .map_err(|_| {
                callback_failure = Some(ProbeFailure::Authority);
            })?;
            let key = credential.take().ok_or_else(|| {
                callback_failure = Some(ProbeFailure::Keychain);
            })?;
            let provider_started = Instant::now();
            let turn = runtime
                .block_on(run_initial_terra_probe(
                    input,
                    AgentProviderTransportConfig::STANDARD,
                    key,
                    observation,
                ))
                .map_err(|error| {
                    callback_failure = Some(ProbeFailure::Provider(error));
                })?;
            let provider_elapsed = provider_started.elapsed();
            let receipt = turn.receipt();
            let input = turn.input();
            let mut action = TerraProbeActionBridge::try_prepare(
                turn.into_tool_turn(),
                observation,
                1,
                1,
                SemanticActionExecutionInstant::from_millis(10_000),
            )
            .map_err(|_| {
                callback_failure = Some(ProbeFailure::Proposal);
            })?;
            let request = action.take_native_request().map_err(|_| {
                callback_failure = Some(ProbeFailure::Proposal);
            })?;
            bridge = Some(action);
            metrics = Some((receipt, input, provider_elapsed));
            Ok(request)
        },
    );
    let terminal = terminal_result.map_err(|_| callback_failure.unwrap_or(ProbeFailure::Engine))?;
    let (settlement, snapshot, observed_at) = terminal.into_parts();
    let report = bridge
        .ok_or(ProbeFailure::Proposal)?
        .settle_and_verify(settlement, &snapshot, observed_at)
        .map_err(|_| ProbeFailure::Verification)?;
    let _ = report.applied();
    let (receipt, input, provider_elapsed) = metrics.ok_or(ProbeFailure::Metrics)?;
    let input = input.metrics();
    let structured = input
        .structured_input_tokens()
        .map_or(0, |count| count.tokens());
    let semantic = input
        .semantic_payload_tokens()
        .map_or(0, |count| count.tokens());
    let provider_elapsed = provider_elapsed.as_millis();
    let elapsed = started.elapsed().as_millis();
    use std::io::Write as _;
    let mut output = std::io::stdout().lock();
    writeln!(
        output,
        "macos-terra-agentic-probe: passed; model=gpt-5.6-terra; tool=act-click; effect=local_write; verified=true; input_tokens={}; output_tokens={}; total_tokens={}; structured_tokens={structured}; semantic_tokens={semantic}; request_bytes={}; semantic_bytes={}; charged_micro_usd={}; provider_elapsed_ms={provider_elapsed}; elapsed_ms={elapsed}; content=redacted",
        receipt.input_tokens(), receipt.output_tokens(), receipt.input_tokens().saturating_add(receipt.output_tokens()), input.serialized_request_bytes(), input.semantic().disclosed_bytes(), receipt.cost_micro_usd(),
    ).map_err(|_| ProbeFailure::Output)?;
    Ok(())
}
