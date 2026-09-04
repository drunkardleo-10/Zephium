//! Explicit, release-excluded live Terra semantic-action qualification.

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

    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let result = match arguments.as_slice() {
        [argument] if argument == "--live-fixed-click" => run_fixed_click(),
        [argument] if argument == "--live-public-wikipedia-fill" => run_public_wikipedia_fill(),
        [argument] if argument == "--live-two-action" => run_two_action(),
        _ => std::process::exit(2),
    };
    if let Err(error) = result {
        let _ = writeln!(
            std::io::stderr().lock(),
            "macos-terra-agentic-probe: failed; stage={}; engine_reason={}; encoding_reason={}; verification_reason={}; wait={}; settle_millis={}; protocol_event={}; content=redacted",
            error.label(),
            error.engine_reason_label(),
            error.encoding_reason_label(),
            error.verification_reason_label(),
            error.wait_label(),
            error.settle_millis(),
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
    Engine(&'static str),
    Verification,
    PostClickVerification {
        error: zephium_agent_controller::TerraProbeActionBridgeError,
        wait: zephium_agentic::SemanticWaitCondition,
        settle_millis: u32,
    },
    PostFillVerification {
        error: zephium_agent_controller::TerraProbeActionBridgeError,
        wait: zephium_agentic::SemanticWaitCondition,
        settle_millis: u32,
    },
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
            Self::Provider(TerraProbeProviderError::InitialEncoding(_)) => {
                "provider_initial_encoding"
            }
            Self::Provider(TerraProbeProviderError::DiffEncoding(_)) => "provider_diff_encoding",
            Self::Provider(TerraProbeProviderError::Transport) => "provider_transport",
            Self::Provider(TerraProbeProviderError::PreDispatchTerminal(_)) => {
                "provider_pre_dispatch_terminal"
            }
            Self::Provider(TerraProbeProviderError::CountTerminal(
                AgentProviderFailureClass::Authentication,
            )) => "provider_count_authentication",
            Self::Provider(TerraProbeProviderError::CountTerminal(
                AgentProviderFailureClass::InvalidRequest,
            )) => "provider_count_invalid_request",
            Self::Provider(TerraProbeProviderError::CountTerminal(
                AgentProviderFailureClass::Permission,
            )) => "provider_count_permission",
            Self::Provider(TerraProbeProviderError::CountTerminal(
                AgentProviderFailureClass::NotFound,
            )) => "provider_count_not_found",
            Self::Provider(TerraProbeProviderError::CountTerminal(
                AgentProviderFailureClass::Conflict,
            )) => "provider_count_conflict",
            Self::Provider(TerraProbeProviderError::CountTerminal(
                AgentProviderFailureClass::RateLimited,
            )) => "provider_count_rate_limited",
            Self::Provider(TerraProbeProviderError::CountTerminal(
                AgentProviderFailureClass::Overloaded,
            )) => "provider_count_overloaded",
            Self::Provider(TerraProbeProviderError::CountTerminal(
                AgentProviderFailureClass::Timeout,
            )) => "provider_count_timeout",
            Self::Provider(TerraProbeProviderError::CountTerminal(
                AgentProviderFailureClass::Transport,
            )) => "provider_count_transport",
            Self::Provider(TerraProbeProviderError::CountTerminal(
                AgentProviderFailureClass::Protocol,
            )) => "provider_count_protocol",
            Self::Provider(TerraProbeProviderError::CountTerminal(
                AgentProviderFailureClass::Integration,
            )) => "provider_count_integration",
            Self::Provider(TerraProbeProviderError::CountTerminal(
                AgentProviderFailureClass::Provider,
            )) => "provider_count_provider",
            Self::Provider(TerraProbeProviderError::CountTerminal(
                AgentProviderFailureClass::Cancelled,
            )) => "provider_count_cancelled",
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
            Self::Provider(TerraProbeProviderError::Continuation) => "provider_continuation",
            Self::Provider(TerraProbeProviderError::TurnLimit) => "provider_turn_limit",
            Self::Proposal => "proposal_contract",
            Self::Engine(_) => "native_engine",
            Self::Verification => "fresh_snapshot_verification",
            Self::PostClickVerification { .. } => "post_click_verification",
            Self::PostFillVerification { .. } => "post_fill_verification",
            Self::Metrics => "metrics",
            Self::Output => "output",
        }
    }

    const fn engine_reason_label(self) -> &'static str {
        match self {
            Self::Engine(reason) => reason,
            _ => "not_applicable",
        }
    }

    const fn verification_reason_label(self) -> &'static str {
        use zephium_agent_controller::TerraProbeActionBridgeError;
        use zephium_agentic::SemanticActionQualificationError;

        let error = match self {
            Self::PostClickVerification { error, .. }
            | Self::PostFillVerification { error, .. } => error,
            _ => return "not_applicable",
        };
        match error {
            TerraProbeActionBridgeError::Proposal => "proposal",
            TerraProbeActionBridgeError::StateUpdate => "state_update",
            TerraProbeActionBridgeError::Qualification(
                SemanticActionQualificationError::Identity,
            ) => "identity",
            TerraProbeActionBridgeError::Qualification(
                SemanticActionQualificationError::Preparation,
            ) => "preparation",
            TerraProbeActionBridgeError::Qualification(
                SemanticActionQualificationError::RequestAlreadyTaken,
            ) => "request_already_taken",
            TerraProbeActionBridgeError::Qualification(
                SemanticActionQualificationError::Terminal,
            ) => "terminal",
            TerraProbeActionBridgeError::Qualification(
                SemanticActionQualificationError::Settlement,
            ) => "settlement",
            TerraProbeActionBridgeError::Qualification(
                SemanticActionQualificationError::Verification,
            ) => "verification",
        }
    }

    const fn wait_label(self) -> &'static str {
        use zephium_agentic::SemanticWaitCondition;

        let wait = match self {
            Self::PostClickVerification { wait, .. } | Self::PostFillVerification { wait, .. } => {
                wait
            }
            _ => return "not_applicable",
        };
        match wait {
            SemanticWaitCondition::Immediate => "immediate",
            SemanticWaitCondition::NavigationCommitted => "navigation_committed",
            SemanticWaitCondition::DocumentReady => "document_ready",
            SemanticWaitCondition::TargetState { .. } => "target_state",
            SemanticWaitCondition::UrlChanged => "url_changed",
            SemanticWaitCondition::TitleChanged => "title_changed",
            SemanticWaitCondition::Dialog(_) => "dialog",
            SemanticWaitCondition::SemanticChange => "semantic_change",
            SemanticWaitCondition::MutationQuiet(_) => "mutation_quiet",
            SemanticWaitCondition::ScrollPositionChanged => "scroll_position_changed",
        }
    }

    const fn settle_millis(self) -> u32 {
        match self {
            Self::PostClickVerification { settle_millis, .. }
            | Self::PostFillVerification { settle_millis, .. } => settle_millis,
            _ => 0,
        }
    }

    const fn encoding_reason_label(self) -> &'static str {
        use zephium_agent_controller::TerraProbeProviderError;
        use zephium_agentic::SemanticModelEncodingError;

        let error = match self {
            Self::Provider(TerraProbeProviderError::InitialEncoding(error))
            | Self::Provider(TerraProbeProviderError::DiffEncoding(error)) => error,
            _ => return "not_applicable",
        };
        match error {
            SemanticModelEncodingError::Budget => "budget",
            SemanticModelEncodingError::OutputLimit => "output_limit",
            SemanticModelEncodingError::Invariant => "invariant",
            SemanticModelEncodingError::TokenCounter(_) => "token_counter",
            SemanticModelEncodingError::TokenQuality => "token_quality",
            SemanticModelEncodingError::TokenizerRevisionMismatch => "tokenizer_revision",
            SemanticModelEncodingError::TokenLimit => "token_limit",
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
const fn action_backend_label(
    backend: zephium_agentic::SemanticActionExecutionBackend,
) -> &'static str {
    match backend {
        zephium_agentic::SemanticActionExecutionBackend::FixedSemanticRecipe => {
            "fixed_semantic_recipe"
        }
        zephium_agentic::SemanticActionExecutionBackend::PageWorldCompatibilityFill => {
            "page_world_compatibility_fill"
        }
        zephium_agentic::SemanticActionExecutionBackend::EngineNativeInput => "engine_native_input",
        zephium_agentic::SemanticActionExecutionBackend::InProcessAccessibility => {
            "in_process_accessibility"
        }
    }
}

#[cfg(target_os = "macos")]
fn run_fixed_click() -> Result<(), ProbeFailure> {
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use zephium_agent_controller::{
        run_initial_terra_probe, TerraControllerClock, TerraControllerRunInput,
        TerraControllerTurnInput, TerraProbeActionBridge,
    };
    use zephium_agent_provider_transport::{
        load_macos_development_openai_credential, AgentProviderTransportConfig,
    };
    use zephium_agentic::{AgentPolicyInstant, SemanticActionExecutionInstant};

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
            let ids = probe_controller_ids().map_err(|_| {
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
    let terminal =
        terminal_result.map_err(|error| callback_failure.unwrap_or(ProbeFailure::Engine(error)))?;
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

#[cfg(target_os = "macos")]
fn probe_controller_ids() -> Result<zephium_agent_controller::TerraControllerIds, ProbeFailure> {
    use zephium_agent_controller::TerraControllerIds;
    use zephium_agentic::{
        AgentAuditDeliveryId, AgentAuditEventId, AgentModelCallId, AgentSupervisorAttemptId,
        AgentSupervisorCancellationId, AgentSupervisorId,
    };

    TerraControllerIds::try_new(
        AgentSupervisorId::new(1).ok_or(ProbeFailure::Authority)?,
        AgentSupervisorAttemptId::new(1).ok_or(ProbeFailure::Authority)?,
        AgentSupervisorCancellationId::new(1).ok_or(ProbeFailure::Authority)?,
        AgentModelCallId::new(1).ok_or(ProbeFailure::Authority)?,
        [
            AgentAuditEventId::new(1).ok_or(ProbeFailure::Authority)?,
            AgentAuditEventId::new(2).ok_or(ProbeFailure::Authority)?,
            AgentAuditEventId::new(3).ok_or(ProbeFailure::Authority)?,
            AgentAuditEventId::new(4).ok_or(ProbeFailure::Authority)?,
        ],
        AgentAuditDeliveryId::new(1).ok_or(ProbeFailure::Authority)?,
    )
    .map_err(|_| ProbeFailure::Authority)
}

#[cfg(target_os = "macos")]
fn run_public_wikipedia_fill() -> Result<(), ProbeFailure> {
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use zephium_agent_controller::{
        run_initial_terra_probe, TerraControllerClock, TerraControllerRunInput,
        TerraControllerTurnInput, TerraProbeActionBridge,
    };
    use zephium_agent_provider_transport::{
        load_macos_development_openai_credential, AgentProviderTransportConfig,
    };
    use zephium_agentic::{AgentPolicyInstant, SemanticActionExecutionInstant};

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
    let terminal_result = zephium_engine::run_macos_agentic_semantic_model_public_fill_probe(
        |observation, authority| {
            let (manifest, lease, account, request, frame, invocation, generation, now) =
                authority.into_parts();
            let turn = TerraControllerTurnInput::try_new(
                account,
                request,
                frame,
                invocation,
                generation,
                "On the current Wikipedia public discovery page, fill its search input with exactly Zephium browser. Return exactly one act action and do not submit or navigate. Use effect=local_write, wait=mutation_quiet 100 ms, verification=target_value_matches_input, and settle_budget=1000 ms."
                    .to_owned(),
            )
            .map_err(|_| {
                callback_failure = Some(ProbeFailure::Authority);
            })?;
            let input = TerraControllerRunInput::try_new(
                manifest,
                lease,
                turn,
                probe_controller_ids().map_err(|_| {
                    callback_failure = Some(ProbeFailure::Authority);
                })?,
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
    let terminal =
        terminal_result.map_err(|error| callback_failure.unwrap_or(ProbeFailure::Engine(error)))?;
    let (settlement, snapshot, observed_at) = terminal.into_parts();
    let action = bridge.ok_or(ProbeFailure::Proposal)?;
    let wait = action.wait();
    let settle_millis = action.settle_millis();
    let report = action
        .settle_and_verify(settlement, &snapshot, observed_at)
        .map_err(|error| ProbeFailure::PostFillVerification {
            error,
            wait,
            settle_millis,
        })?;
    let backend = action_backend_label(report.applied().backend());
    let (receipt, input, provider_elapsed) = metrics.ok_or(ProbeFailure::Metrics)?;
    let input = input.metrics();
    let provider_elapsed = provider_elapsed.as_millis();
    let elapsed = started.elapsed().as_millis();
    use std::io::Write as _;
    writeln!(
        std::io::stdout().lock(),
        "macos-terra-agentic-probe: passed; workflow=public-wikipedia-search-fill; model=gpt-5.6-terra; turns=1; verified_actions=1; backend={backend}; profile=ephemeral; extensions=absent; presentation=hidden; focus_theft=false; input_tokens={}; output_tokens={}; total_tokens={}; request_bytes={}; semantic_bytes={}; charged_micro_usd={}; provider_elapsed_ms={provider_elapsed}; elapsed_ms={elapsed}; content=redacted",
        receipt.input_tokens(),
        receipt.output_tokens(),
        receipt
            .input_tokens()
            .saturating_add(receipt.output_tokens()),
        input.serialized_request_bytes(),
        input.semantic().disclosed_bytes(),
        receipt.cost_micro_usd(),
    )
    .map_err(|_| ProbeFailure::Output)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn run_two_action() -> Result<(), ProbeFailure> {
    use std::cell::{Cell, RefCell};
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use zephium_agent_controller::{
        TerraControllerClock, TerraControllerRunInput, TerraControllerTurnInput,
        TerraProbeActionBridge, TerraProbeSession,
    };
    use zephium_agent_provider_transport::{
        load_macos_development_openai_credential, AgentProviderCredential,
        AgentProviderTransportConfig,
    };
    use zephium_agentic::{
        AgentModelCallReceipt, AgentPolicyInstant, AgentProviderInputMetricReceipt,
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

    struct TurnMetric {
        receipt: AgentModelCallReceipt,
        input: AgentProviderInputMetricReceipt,
        provider_elapsed: Duration,
    }

    struct WorkflowState {
        credential: Option<AgentProviderCredential>,
        session: Option<TerraProbeSession>,
        bridge: Option<TerraProbeActionBridge>,
        metrics: Vec<TurnMetric>,
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| ProbeFailure::Runtime)?;
    let state = RefCell::new(WorkflowState {
        credential: Some(
            load_macos_development_openai_credential().map_err(|_| ProbeFailure::Keychain)?,
        ),
        session: None,
        bridge: None,
        metrics: Vec::with_capacity(2),
    });
    let callback_failure = Cell::new(None);
    let started = Instant::now();
    let result = zephium_engine::run_macos_agentic_semantic_model_two_action_probe(
        |observation, authority| {
            let mut state = state.try_borrow_mut().map_err(|_| {
                callback_failure.set(Some(ProbeFailure::Authority));
            })?;
            let (manifest, lease, account, request, frame, invocation, generation, now) =
                authority.into_parts();
            let turn = TerraControllerTurnInput::try_new(
                account,
                request,
                frame,
                invocation,
                generation,
                "Complete exactly two local actions in order. First click Primary semantic action with target_state expanded true verification. After the verified tool result, fill Semantic fill text with Zephium model continuation using target_value_matches_input verification. Return exactly one act action per turn and use bounded settle budgets."
                    .to_owned(),
            )
            .map_err(|_| {
                callback_failure.set(Some(ProbeFailure::Authority));
            })?;
            let input = TerraControllerRunInput::try_new(
                manifest,
                lease,
                turn,
                probe_controller_ids().map_err(|_| {
                    callback_failure.set(Some(ProbeFailure::Authority));
                })?,
                Arc::new(Clock(now)),
                Instant::now()
                    .checked_add(Duration::from_secs(120))
                    .ok_or_else(|| {
                        callback_failure.set(Some(ProbeFailure::Authority));
                    })?,
            )
            .map_err(|_| {
                callback_failure.set(Some(ProbeFailure::Authority));
            })?;
            let credential = state.credential.take().ok_or_else(|| {
                callback_failure.set(Some(ProbeFailure::Keychain));
            })?;
            let provider_started = Instant::now();
            let (session, provider_turn) = runtime
                .block_on(TerraProbeSession::start(
                    input,
                    AgentProviderTransportConfig::STANDARD,
                    credential,
                    observation,
                ))
                .map_err(|error| {
                    callback_failure.set(Some(ProbeFailure::Provider(error)));
                })?;
            let provider_elapsed = provider_started.elapsed();
            let receipt = provider_turn.receipt();
            let input = provider_turn.input();
            let mut bridge = TerraProbeActionBridge::try_prepare(
                provider_turn.into_tool_turn(),
                observation,
                1,
                1,
                SemanticActionExecutionInstant::from_millis(10_000),
            )
            .map_err(|_| {
                callback_failure.set(Some(ProbeFailure::Proposal));
            })?;
            let request = bridge.take_native_request().map_err(|_| {
                callback_failure.set(Some(ProbeFailure::Proposal));
            })?;
            state.session = Some(session);
            state.bridge = Some(bridge);
            state.metrics.push(TurnMetric {
                receipt,
                input,
                provider_elapsed,
            });
            Ok(request)
        },
        |baseline, settlement, current, observed_at| {
            let mut state = state.try_borrow_mut().map_err(|_| {
                callback_failure.set(Some(ProbeFailure::Authority));
            })?;
            let bridge = state.bridge.take().ok_or_else(|| {
                callback_failure.set(Some(ProbeFailure::Proposal));
            })?;
            let wait = bridge.wait();
            let settle_millis = bridge.settle_millis();
            let (_, transition) = bridge
                .settle_for_continuation(settlement, baseline, current, observed_at)
                .map_err(|error| {
                    callback_failure.set(Some(ProbeFailure::PostClickVerification {
                        error,
                        wait,
                        settle_millis,
                    }));
                })?;
            let provider_started = Instant::now();
            let provider_turn = runtime
                .block_on(
                    state
                        .session
                        .as_mut()
                        .ok_or_else(|| {
                            callback_failure.set(Some(ProbeFailure::Authority));
                        })?
                        .continue_after_verified_action(transition),
                )
                .map_err(|error| {
                    callback_failure.set(Some(ProbeFailure::Provider(error)));
                })?;
            let provider_elapsed = provider_started.elapsed();
            let receipt = provider_turn.receipt();
            let input = provider_turn.input();
            let mut bridge = TerraProbeActionBridge::try_prepare(
                provider_turn.into_tool_turn(),
                current,
                2,
                2,
                SemanticActionExecutionInstant::from_millis(10_000),
            )
            .map_err(|_| {
                callback_failure.set(Some(ProbeFailure::Proposal));
            })?;
            let request = bridge.take_native_request().map_err(|_| {
                callback_failure.set(Some(ProbeFailure::Proposal));
            })?;
            state.bridge = Some(bridge);
            state.metrics.push(TurnMetric {
                receipt,
                input,
                provider_elapsed,
            });
            Ok(request)
        },
        |_baseline, settlement, current, observed_at| {
            let mut state = state.try_borrow_mut().map_err(|_| {
                callback_failure.set(Some(ProbeFailure::Authority));
            })?;
            let snapshot = current.frames().first().ok_or_else(|| {
                callback_failure.set(Some(ProbeFailure::Verification));
            })?;
            let bridge = state.bridge.take().ok_or_else(|| {
                callback_failure.set(Some(ProbeFailure::Proposal));
            })?;
            let wait = bridge.wait();
            let settle_millis = bridge.settle_millis();
            let report = bridge
                .settle_and_verify(settlement, snapshot, observed_at)
                .map_err(|error| {
                    callback_failure.set(Some(ProbeFailure::PostFillVerification {
                        error,
                        wait,
                        settle_millis,
                    }));
                })?;
            let _ = report.applied();
            state
                .session
                .take()
                .ok_or_else(|| {
                    callback_failure.set(Some(ProbeFailure::Authority));
                })?
                .finish()
                .map_err(|error| {
                    callback_failure.set(Some(ProbeFailure::Provider(error)));
                })?;
            Ok(())
        },
    );
    result.map_err(|error| {
        callback_failure
            .get()
            .unwrap_or(ProbeFailure::Engine(error))
    })?;
    let state = state.into_inner();
    if state.credential.is_some()
        || state.session.is_some()
        || state.bridge.is_some()
        || state.metrics.len() != 2
    {
        return Err(ProbeFailure::Metrics);
    }
    let mut input_tokens = 0_u64;
    let mut output_tokens = 0_u64;
    let mut charged_micro_usd = 0_u64;
    let mut request_bytes = 0_u64;
    let mut semantic_bytes = 0_u64;
    let mut provider_elapsed_ms = 0_u128;
    for metric in state.metrics {
        input_tokens = input_tokens
            .checked_add(metric.receipt.input_tokens())
            .ok_or(ProbeFailure::Metrics)?;
        output_tokens = output_tokens
            .checked_add(metric.receipt.output_tokens())
            .ok_or(ProbeFailure::Metrics)?;
        charged_micro_usd = charged_micro_usd
            .checked_add(metric.receipt.cost_micro_usd())
            .ok_or(ProbeFailure::Metrics)?;
        let input = metric.input.metrics();
        request_bytes = request_bytes
            .checked_add(u64::from(input.serialized_request_bytes()))
            .ok_or(ProbeFailure::Metrics)?;
        semantic_bytes = semantic_bytes
            .checked_add(u64::from(input.semantic().disclosed_bytes()))
            .ok_or(ProbeFailure::Metrics)?;
        provider_elapsed_ms = provider_elapsed_ms
            .checked_add(metric.provider_elapsed.as_millis())
            .ok_or(ProbeFailure::Metrics)?;
    }
    let total_tokens = input_tokens
        .checked_add(output_tokens)
        .ok_or(ProbeFailure::Metrics)?;
    let elapsed_ms = started.elapsed().as_millis();
    use std::io::Write as _;
    writeln!(
        std::io::stdout().lock(),
        "macos-terra-agentic-probe: passed; workflow=verified-click-then-fill; model=gpt-5.6-terra; turns=2; verified_actions=2; input_tokens={input_tokens}; output_tokens={output_tokens}; total_tokens={total_tokens}; request_bytes={request_bytes}; semantic_bytes={semantic_bytes}; charged_micro_usd={charged_micro_usd}; provider_elapsed_ms={provider_elapsed_ms}; elapsed_ms={elapsed_ms}; content=redacted",
    )
    .map_err(|_| ProbeFailure::Output)?;
    Ok(())
}
