//! Trusted capability-provider composition. This module is dormant until an
//! admitted user operation; no model selects credentials, configuration or ports.
use std::sync::Arc;
use zephium_core::{ids::ProfileId, work::WorkError};
use zephium_ipc::work::{WorkOperationStateV1, WorkOperationV1, WorkReplyV1, WorkResponseV1};

pub(crate) struct WorkProviders {
    pub(crate) activity: super::work_activity::WorkActivity,
    #[cfg(target_os = "macos")]
    pub(crate) browser: zephium_work_composition::MacosWorkComposition,
}
impl WorkProviders {
    pub(crate) fn new(
        engine: Arc<zephium_engine::WebviewEngine>,
        store: Arc<zephium_store::SqliteStore>,
        frames: Option<Arc<zephium_store::WorkFrameStore>>,
    ) -> Self {
        #[cfg(not(target_os = "macos"))]
        let _ = (engine, store);
        Self {
            activity: super::work_activity::WorkActivity::new(frames),
            #[cfg(target_os = "macos")]
            browser: zephium_work_composition::MacosWorkComposition::new(engine, store),
        }
    }

    pub(crate) async fn run(
        &self,
        shell: zephium_app::Handle,
        profile: ProfileId,
        input: WorkOperationV1,
    ) -> WorkOperationStateV1 {
        let work = input.work();
        let phase = match &input {
            WorkOperationV1::ReadPublic { .. } => "public_read",
            WorkOperationV1::Run { .. } => "run",
            WorkOperationV1::Plan { .. } => "plan",
            WorkOperationV1::PreparePlan { .. } => "prepare_plan",
            WorkOperationV1::Prepare { .. } => "prepare",
            WorkOperationV1::PrepareAccount { .. } => "prepare_account",
            WorkOperationV1::Start { .. } => "execute",
        };
        let started = std::time::Instant::now();
        record_diagnostic(format_args!(
            "work: work={work} phase={phase} state=admitted"
        ));
        let state = match self.try_run(shell, profile, input).await {
            Ok(state) => state,
            Err(error) => WorkOperationStateV1::Refused {
                error: error.into(),
            },
        };
        let outcome = match &state {
            WorkOperationStateV1::Planned { .. } => "planning_response",
            WorkOperationStateV1::Settled { .. } => "settled_response",
            WorkOperationStateV1::Refused { .. } => "refused",
            WorkOperationStateV1::Pending { .. } => "pending",
            WorkOperationStateV1::Unknown => "unknown",
        };
        record_diagnostic(format_args!(
            "work: work={work} phase={phase} state={outcome} elapsed_ms={}",
            started.elapsed().as_millis()
        ));
        if let WorkOperationStateV1::Planned { response } = &state {
            if let Some(usage) = &response.usage {
                record_diagnostic(format_args!(
                    "work: work={work} phase={phase} input_tokens={} output_tokens={} cost_ceiling_micro_usd={}",
                    usage.input_tokens, usage.output_tokens, usage.cost_ceiling_micro_usd
                ));
            }
        }
        state
    }

    async fn try_run(
        &self,
        shell: zephium_app::Handle,
        profile: ProfileId,
        input: WorkOperationV1,
    ) -> Result<WorkOperationStateV1, WorkError> {
        use zephium_agentic::{
            AgentProviderTransport, AgentProviderTransportConfig, OpenAiWorkPlanner,
            OpenAiWorkSynthesizer,
        };
        use zephium_app::{
            work_execution::WorkExecutionService, work_planning::WorkPlanningService,
        };
        match input {
            WorkOperationV1::ReadPublic { command, context } => {
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = (command, context);
                    Err(WorkError::Unavailable)
                }
                #[cfg(target_os = "macos")]
                {
                    let zephium_core::work::runtime::WorkRuntimeIntent::ReadPublic {
                        scope,
                        limits,
                    } = &command.intent
                    else {
                        return Err(WorkError::Invalid);
                    };
                    if command.version != 1 {
                        return Err(WorkError::Invalid);
                    }
                    zephium_core::work::search::validate_direct_public_read(scope, *limits)?;
                    let transport =
                        AgentProviderTransport::try_new(AgentProviderTransportConfig::STANDARD)
                            .map_err(|_| WorkError::Unavailable)?;
                    let config = zephium_agentic::OpenAiPublicSearchConfig::try_new(
                        zephium_agent_model_catalog::try_public_search_provider_exact_call_config(
                            &scope.model,
                            4096,
                        )
                        .map_err(|_| WorkError::Unavailable)?,
                    )?;
                    let provider = zephium_agentic::OpenAiPublicSearch::try_new(
                        transport,
                        credential().await?,
                        config,
                    )?;
                    let projection = WorkExecutionService::new(shell).read_public_with_context(profile, command, context, |attempt, bodies| async move {
                        #[cfg(feature = "work-development-traces")]
                        let provider = provider.with_public_response_retention().with_public_work_trace(attempt.work(),attempt.execution(),attempt.attempt());
                        let attempt_id = attempt.attempt();
                        let started = std::time::Instant::now();
                        let result = attempt.search_public_owned_with_context(&provider, &bodies).await;
                        match &result {
                            Ok(settlement) => {
                                if let Some(fact) = settlement.projection().executions.iter().find(|fact| fact.id == settlement.execution()).and_then(|fact| fact.attempts.iter().find(|fact| fact.id == attempt_id)) {
                                    record_diagnostic(format_args!("work: attempt={attempt_id} phase=public_search status={:?} usage={:?} elapsed_ms={} native_resources=0", fact.status, fact.usage, started.elapsed().as_millis()));
                                }
                            }
                            Err(error) => record_diagnostic(format_args!("work: attempt={attempt_id} phase=public_search failure={error:?} elapsed_ms={}", started.elapsed().as_millis())),
                        }
                        result
                    }, |observer| self.activity.track(observer)).await?;
                    Ok(WorkOperationStateV1::Settled {
                        response: WorkResponseV1 {
                            version: 1,
                            profile: profile.to_string(),
                            reply: WorkReplyV1::Projection {
                                projection: Box::new(projection),
                            },
                        },
                    })
                }
            }
            WorkOperationV1::Run { command, context } => {
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = (command, context);
                    Err(WorkError::Unavailable)
                }
                #[cfg(target_os = "macos")]
                {
                    use zephium_app::work_agent::{WorkAgentProviders, WorkAgentService};
                    let zephium_core::work::runtime::WorkRuntimeIntent::BeginAgent {
                        grant, ..
                    } = &command.intent
                    else {
                        return Err(WorkError::Invalid);
                    };
                    if command.version != 1 {
                        return Err(WorkError::Invalid);
                    }
                    grant.validate()?;
                    let binding_request = shell.work_profile_binding();
                    let binding =
                        tokio::time::timeout(std::time::Duration::from_secs(8), async move {
                            loop {
                                if let Some(binding) = binding_request.try_recv() {
                                    break binding;
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                            }
                        })
                        .await
                        .map_err(|_| WorkError::Unavailable)?;
                    let zephium_app::AgentWorkProfileReadiness::Ready(binding) = binding else {
                        return Err(WorkError::ProfileUnavailable);
                    };
                    if binding.profile() != profile {
                        return Err(WorkError::ProfileUnavailable);
                    }
                    let transport =
                        AgentProviderTransport::try_new(AgentProviderTransportConfig::STANDARD)
                            .map_err(|_| WorkError::Unavailable)?;
                    let agent = zephium_agentic::OpenAiWorkAgent::try_new(
                        transport.clone(),
                        credential().await?,
                        agent_model_config()?,
                    )
                    .map_err(|_| WorkError::Unavailable)?;
                    let decisions = public_decision_settings(transport).await?;
                    let agent = agent
                        .with_link_decisions(
                            decisions.primary,
                            decisions.emulation,
                            decisions.diagnostic,
                        )
                        .map_err(|_| WorkError::Unavailable)?;
                    #[cfg(feature = "work-development-traces")]
                    let agent = agent.with_public_response_retention().with_diagnostic(|event| {
                        use zephium_core::work::synthesis::WorkSynthesisDiagnostic;
                        match event {
                            WorkSynthesisDiagnostic::InputCounted { tokens, maximum, request_bytes } => record_diagnostic(format_args!("work: phase=agent_turn input_counted tokens={tokens} maximum={maximum} request_bytes={request_bytes}")),
                            WorkSynthesisDiagnostic::ProviderTransport { http_status, body_bytes, decoded, elapsed_millis } => record_diagnostic(format_args!("work: phase=agent_turn provider_transport http_status={http_status:?} body_bytes={body_bytes} decoded={decoded} elapsed_ms={elapsed_millis}")),
                            WorkSynthesisDiagnostic::TurnRejected { reason } => record_diagnostic(format_args!("work: phase=agent_turn rejected reason={reason:?}")),
                            _ => {}
                        }
                    });
                    let search_transport =
                        AgentProviderTransport::try_new(AgentProviderTransportConfig::STANDARD)
                            .map_err(|_| WorkError::Unavailable)?;
                    let search_config = zephium_agentic::OpenAiPublicSearchConfig::try_new(
                        zephium_agent_model_catalog::try_public_search_provider_exact_call_config(
                            &grant.model,
                            4096,
                        )
                        .map_err(|_| WorkError::Unavailable)?,
                    )?;
                    let search = zephium_agentic::OpenAiPublicSearch::try_new(
                        search_transport.clone(),
                        credential().await?,
                        search_config,
                    )?;
                    let search = configure_search_ranking(search, search_transport).await?;
                    #[cfg(feature = "work-development-traces")]
                    let search = search.with_public_response_retention().with_diagnostic(|event| {
                        if let zephium_core::work::synthesis::WorkSynthesisDiagnostic::ProviderTransport { http_status, body_bytes, decoded, elapsed_millis } = event {
                            record_diagnostic(format_args!("work: phase=search provider_transport http_status={http_status:?} body_bytes={body_bytes} decoded={decoded} elapsed_ms={elapsed_millis}"));
                        }
                    });
                    let callback = shell.callback_handle();
                    let work = command.work;
                    let started = std::time::Instant::now();
                    let service = WorkAgentService::new(shell);
                    #[cfg(feature = "work-development-traces")]
                    let service = service.with_diagnostic(|event| {
                        record_diagnostic(format_args!("work: phase=agent loop={event:?}"));
                    });
                    let projection = service
                        .run(
                            profile,
                            command,
                            context,
                            WorkAgentProviders {
                                turn: &agent,
                                search: &search,
                            },
                            |probe, request| {
                                let callback = &callback;
                                async move {
                                    let settings = zephium_work_composition::durable_runtime::WorkBrowserAdapterSettings::new(
                                        binding,
                                        zephium_agent_controller::AgentBrowserModel::Luna,
                                        zephium_app::AgentWorkApplicationConfig::new(
                                            zephium_agent_runtime::AgentRuntimeConfig::STANDARD,
                                            AgentProviderTransportConfig::STANDARD,
                                        ),
                                        credential().await?,
                                    );
                                    #[cfg(feature = "work-development-traces")]
                                    let settings = {
                                        let mut settings = settings;
                                        settings.retain_public_responses = true;
                                        settings.diagnostic = Some(|attempt, snapshot| {
                                            record_diagnostic(format_args!("work: attempt={attempt} phase=agent_browser state={:?} failure={:?} persistence_failure={:?}", snapshot.phase, snapshot.failure, snapshot.persistence_failure));
                                        });
                                        settings.model_diagnostic = Some(record_browser_diagnostic);
                                        settings.stage_diagnostic = Some(|stage| {
                                            record_diagnostic(format_args!("work: phase=agent_browser stage={stage}"));
                                        });
                                        settings.resource_diagnostic = Some(|cause| {
                                            record_diagnostic(format_args!("work: phase=agent_browser resource_failure={cause:?}"));
                                        });
                                        settings
                                    };
                                    let result = self.browser.run_agent_step(callback, &probe, request, settings).await;
                                    if let Err(error) = &result {
                                        record_diagnostic(format_args!("work: attempt={} phase=agent_browser failure={error:?}", probe.attempt()));
                                    }
                                    result
                                }
                            },
                            |observer| self.activity.track(observer),
                        )
                        .await;
                    match &projection {
                        Ok(projection) => {
                            if let Some(fact) = projection.executions.last() {
                                record_diagnostic(format_args!(
                                    "work: work={work} phase=agent status={:?} steps={} artifacts={} elapsed_ms={}",
                                    fact.status,
                                    fact.steps.len(),
                                    fact.artifacts.len(),
                                    started.elapsed().as_millis()
                                ));
                            }
                        }
                        Err(error) => record_diagnostic(format_args!(
                            "work: work={work} phase=agent failure={error:?} elapsed_ms={}",
                            started.elapsed().as_millis()
                        )),
                    }
                    Ok(WorkOperationStateV1::Settled {
                        response: WorkResponseV1 {
                            version: 1,
                            profile: profile.to_string(),
                            reply: WorkReplyV1::Projection {
                                projection: Box::new(projection?),
                            },
                        },
                    })
                }
            }
            input @ (WorkOperationV1::Plan { .. } | WorkOperationV1::PreparePlan { .. }) => {
                let (request, prepare) = match input {
                    WorkOperationV1::Plan { request } => (request, false),
                    WorkOperationV1::PreparePlan { request } => (request, true),
                    _ => unreachable!(),
                };
                let transport =
                    AgentProviderTransport::try_new(AgentProviderTransportConfig::STANDARD)
                        .map_err(|_| WorkError::Unavailable)?;
                let provider = OpenAiWorkPlanner::try_new(
                    transport,
                    credential().await?,
                    planning_model_config()?,
                )
                .map_err(|_| WorkError::Unavailable)?;
                #[cfg(feature = "work-development-traces")]
                let provider = provider.with_public_response_retention();
                let service = WorkPlanningService::new(shell, Arc::new(provider));
                #[cfg(feature = "work-development-traces")]
                let service = service.with_execution_diagnostic(|reason| {
                    record_diagnostic(format_args!("work: phase=prepare_plan refusal={reason:?}"));
                });
                let response = if prepare {
                    service.prepare_request(profile, request).await
                } else {
                    service.plan_request(profile, request).await
                };
                Ok(WorkOperationStateV1::Planned { response })
            }
            WorkOperationV1::Prepare { request } => {
                let response = WorkExecutionService::new(shell)
                    .prepare_public_approval(profile, request)
                    .await?;
                Ok(WorkOperationStateV1::Settled { response })
            }
            WorkOperationV1::PrepareAccount { request } => {
                let response = zephium_app::work_account_scope::WorkAccountApproval::new(shell)
                    .prepare(profile, request)
                    .await?;
                Ok(WorkOperationStateV1::Settled { response })
            }
            WorkOperationV1::Start { request } => {
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = request;
                    Err(WorkError::Unavailable)
                }
                #[cfg(target_os = "macos")]
                {
                    let binding_request = shell.work_profile_binding();
                    let binding =
                        tokio::time::timeout(std::time::Duration::from_secs(8), async move {
                            loop {
                                if let Some(binding) = binding_request.try_recv() {
                                    break binding;
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                            }
                        })
                        .await
                        .map_err(|_| WorkError::Unavailable)?;
                    let zephium_app::AgentWorkProfileReadiness::Ready(binding) = binding else {
                        return Err(WorkError::ProfileUnavailable);
                    };
                    if binding.profile() != profile {
                        return Err(WorkError::ProfileUnavailable);
                    }
                    let transport =
                        AgentProviderTransport::try_new(AgentProviderTransportConfig::STANDARD)
                            .map_err(|_| WorkError::Unavailable)?;
                    let synthesis = OpenAiWorkSynthesizer::try_new(
                        transport,
                        credential().await?,
                        synthesis_model_config()?,
                    )
                    .map_err(|_| WorkError::Unavailable)?;
                    #[cfg(feature = "work-development-traces")]
                    let synthesis =
                        synthesis
                            .with_public_response_retention()
                            .with_diagnostic(|event| {
                                use zephium_core::work::synthesis::WorkSynthesisDiagnostic;
                                match event {
                                    WorkSynthesisDiagnostic::DisclosureReady { attempt, bytes, sources, evidence } => record_diagnostic(format_args!("work: attempt={attempt} phase=synthesis disclosure_ready bytes={bytes} sources={sources} evidence={evidence}")),
                                    WorkSynthesisDiagnostic::DisclosureFailed { attempt, error } => record_diagnostic(format_args!("work: attempt={attempt} phase=synthesis disclosure_failed error={error:?}")),
                                    WorkSynthesisDiagnostic::ProviderRefused { attempt, error } => record_diagnostic(format_args!("work: attempt={attempt} phase=synthesis provider_refused error={error:?}")),
                                    WorkSynthesisDiagnostic::InputCounted { tokens, maximum, request_bytes } => record_diagnostic(format_args!("work: phase=synthesis input_counted tokens={tokens} maximum={maximum} request_bytes={request_bytes}")),
                                    WorkSynthesisDiagnostic::ProviderTransport { http_status, body_bytes, decoded, elapsed_millis } => record_diagnostic(format_args!("work: phase=synthesis provider_transport http_status={http_status:?} body_bytes={body_bytes} decoded={decoded} elapsed_ms={elapsed_millis}")),
                                    WorkSynthesisDiagnostic::TurnRejected { .. } => {}
                                }
                            });
                    let callback = shell.callback_handle();
                    let projection = WorkExecutionService::new(shell).execute_request(profile, request, &synthesis, |attempt| {
                        let callback = &callback;
                        async move {
                            if let zephium_core::work::runtime::WorkCapability::PublicSearch { scope } = &attempt.specification().capability {
                                let transport = AgentProviderTransport::try_new(AgentProviderTransportConfig::STANDARD).map_err(|_| WorkError::Unavailable)?;
                                let config = zephium_agentic::OpenAiPublicSearchConfig::try_new(
                                    zephium_agent_model_catalog::try_public_search_provider_exact_call_config(&scope.model, 4096).map_err(|_| WorkError::Unavailable)?
                                )?;
                                let provider = zephium_agentic::OpenAiPublicSearch::try_new(transport, credential().await?, config)?;
                                #[cfg(feature = "work-development-traces")]
                                let provider = provider.with_public_response_retention().with_public_work_trace(attempt.work(), attempt.execution(), attempt.attempt());
                                let attempt_id = attempt.attempt();
                                let started = std::time::Instant::now();
                                let result = attempt.search_public_owned(&provider).await;
                                match &result {
                                    Ok(settlement) => {
                                        if let Some(fact) = settlement.projection().executions.iter().find(|fact| fact.id == settlement.execution()).and_then(|fact| fact.attempts.iter().find(|fact| fact.id == attempt_id)) {
                                            record_diagnostic(format_args!("work: attempt={attempt_id} phase=public_search status={:?} usage={:?} elapsed_ms={} native_resources=0", fact.status, fact.usage, started.elapsed().as_millis()));
                                        }
                                    }
                                    Err(error) => record_diagnostic(format_args!("work: attempt={attempt_id} phase=public_search failure={error:?} elapsed_ms={}", started.elapsed().as_millis())),
                                }
                                return result;
                            }
                            let settings = zephium_work_composition::durable_runtime::WorkBrowserAdapterSettings::new(
                                binding,
                                zephium_agent_controller::AgentBrowserModel::Luna,
                                zephium_app::AgentWorkApplicationConfig::new(
                                    zephium_agent_runtime::AgentRuntimeConfig::STANDARD,
                                    AgentProviderTransportConfig::STANDARD,
                                ),
                                credential().await?,
                            );
                            #[cfg(feature = "work-development-traces")]
                            let settings = {
                                let mut settings = settings;
                                settings.retain_public_responses = true;
                                settings.diagnostic = Some(|attempt, snapshot| {
                                    record_diagnostic(format_args!("work: attempt={attempt} phase=browser state={:?} failure={:?} persistence_failure={:?}", snapshot.phase, snapshot.failure, snapshot.persistence_failure));
                                });
                                settings
                            };
                            let attempt_id = attempt.attempt();
                            let result = self.browser.execute_public_node_owned(callback, attempt, settings).await;
                            if let Err(error) = &result {
                                record_diagnostic(format_args!("work: attempt={attempt_id} phase=browser_adapter failure={error:?}"));
                            }
                            result
                        }
                    }, |observer| self.activity.track(observer)).await?;
                    Ok(WorkOperationStateV1::Settled {
                        response: WorkResponseV1 {
                            version: 1,
                            profile: profile.to_string(),
                            reply: WorkReplyV1::Projection {
                                projection: Box::new(projection),
                            },
                        },
                    })
                }
            }
        }
    }
}

fn planning_model_config() -> Result<zephium_agentic::WorkPlanningConfig, WorkError> {
    zephium_agentic::WorkPlanningConfig::try_new(
        zephium_agent_model_catalog::try_luna_provider_exact_call_config(4096)
            .map_err(|_| WorkError::Unavailable)?,
        8192,
        100_000,
    )
    .map_err(|_| WorkError::Unavailable)
}
// An agent turn discloses a bounded 48KiB context: objective, steps, sources
// and canvas objects. Each turn is separately capped by the run's remaining
// limits; this ceiling only bounds one call.
fn agent_model_config() -> Result<zephium_agentic::WorkPlanningConfig, WorkError> {
    const MAX_TURN_INPUT_TOKENS: u32 = 32_768;
    zephium_agentic::WorkPlanningConfig::try_new(
        zephium_agent_model_catalog::try_luna_provider_exact_call_config(8192)
            .map_err(|_| WorkError::Unavailable)?,
        MAX_TURN_INPUT_TOKENS,
        300_000,
    )
    .map_err(|_| WorkError::Unavailable)
}
// Synthesis consumes a bounded 32KiB semantic context plus schema/instructions.
// Its token ceiling is separate from planning; exact provider counting and the
// original attempt's token/cost limits still decide whether generation starts.
fn synthesis_model_config() -> Result<zephium_agentic::WorkPlanningConfig, WorkError> {
    const MAX_SYNTHESIS_INPUT_TOKENS: u32 = 32_768;
    zephium_agentic::WorkPlanningConfig::try_new(
        zephium_agent_model_catalog::try_luna_provider_exact_call_config(4096)
            .map_err(|_| WorkError::Unavailable)?,
        MAX_SYNTHESIS_INPUT_TOKENS,
        100_000,
    )
    .map_err(|_| WorkError::Unavailable)
}
async fn configure_search_ranking(
    search: zephium_agentic::OpenAiPublicSearch,
    transport: zephium_agentic::AgentProviderTransport,
) -> Result<zephium_agentic::OpenAiPublicSearch, WorkError> {
    let settings = public_decision_settings(transport).await?;
    search.with_decision_ranking(settings.primary, settings.emulation, settings.diagnostic)
}

struct PublicDecisionSettings {
    primary: Option<zephium_agentic::JevDecisionClient>,
    emulation: zephium_agentic::WorkPlanningConfig,
    diagnostic: Option<fn(zephium_agentic::DecisionCallDiagnostic)>,
}

async fn public_decision_settings(
    transport: zephium_agentic::AgentProviderTransport,
) -> Result<PublicDecisionSettings, WorkError> {
    #[cfg(target_os = "macos")]
    let primary =
        tokio::task::spawn_blocking(zephium_agentic::load_macos_development_typesafe_credential)
            .await
            .ok()
            .and_then(Result::ok)
            .and_then(|key| zephium_agentic::JevDecisionClient::direct(transport, key).ok());
    #[cfg(not(target_os = "macos"))]
    let primary = {
        let _ = transport;
        None
    };
    let emulation = zephium_agentic::WorkPlanningConfig::try_new(
        zephium_agent_model_catalog::try_luna_provider_exact_call_config(4096)
            .map_err(|_| WorkError::Unavailable)?,
        32_768,
        100_000,
    )
    .map_err(|_| WorkError::Unavailable)?;
    #[cfg(feature = "work-development-traces")]
    let diagnostic = Some(
        (|fact| {
            record_browser_diagnostic(
                zephium_agent_controller::AgentWorkEventKind::DecisionSettled(fact),
            )
        }) as fn(zephium_agentic::DecisionCallDiagnostic),
    );
    #[cfg(not(feature = "work-development-traces"))]
    let diagnostic = None;
    Ok(PublicDecisionSettings {
        primary,
        emulation,
        diagnostic,
    })
}

async fn credential() -> Result<zephium_agentic::AgentProviderCredential, WorkError> {
    #[cfg(target_os = "macos")]
    {
        let started = std::time::Instant::now();
        record_diagnostic(format_args!("work: phase=credential state=requested"));
        let result =
            tokio::task::spawn_blocking(zephium_agentic::load_macos_development_openai_credential)
                .await
                .map_err(|_| WorkError::Unavailable)
                .and_then(|result| result.map_err(|_| WorkError::Unavailable));
        record_diagnostic(format_args!(
            "work: phase=credential available={} elapsed_ms={}",
            result.is_ok(),
            started.elapsed().as_millis()
        ));
        result
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(WorkError::Unavailable)
    }
}

// Only this module's closed facts enter the development log; generic application
// diagnostics may contain page/provider text and are deliberately excluded.
#[cfg(feature = "work-development-traces")]
fn record_browser_diagnostic(event: zephium_agent_controller::AgentWorkEventKind) {
    use zephium_agent_controller::AgentWorkEventKind;
    match event {
        AgentWorkEventKind::DecisionSettled(fact) => {
            record_diagnostic(format_args!(
                "work: phase=decision backend={:?} model={} questions={} state_bytes={} input_tokens={:?} elapsed_ms={} http_status={:?} attempts={:?} failure={:?} envelope={:?}",
                fact.backend, fact.model, fact.question_count, fact.state_bytes,
                fact.input_tokens, fact.elapsed_millis, fact.http_status, fact.attempts,
                fact.failure, fact.envelope_failure,
            ));
            if let Some(facts) = fact.rejected_envelope {
                record_diagnostic(format_args!(
                    "work: phase=decision_rejected_envelope facts={facts:?}"
                ));
            }
            for (kind, counts) in ["noul", "choice", "score"]
                .into_iter()
                .zip(fact.confidence_buckets)
            {
                record_diagnostic(format_args!(
                    "work: phase=decision_confidence kind={kind} below_080={} below_095={} below_098={} at_least_098={}",
                    counts[0], counts[1], counts[2], counts[3],
                ));
            }
        }
        AgentWorkEventKind::DecisionFallback { counts, capacity } => {
            for (kind, counts) in ["noul", "choice", "score"].into_iter().zip(counts) {
                record_diagnostic(format_args!(
                    "work: phase=decision_fallback kind={kind} capacity={capacity} unavailable={} rate_limited={} invalid_answer={} low_confidence={}",
                    counts[0], counts[1], counts[2], counts[3],
                ));
            }
        }
        event => record_diagnostic(format_args!("work: phase=agent_browser event={event:?}")),
    }
}

pub(crate) fn record_diagnostic(arguments: std::fmt::Arguments<'_>) {
    #[cfg(feature = "work-development-traces")]
    super::work_diagnostics::record(arguments);
    #[cfg(not(feature = "work-development-traces"))]
    super::write_diagnostic(arguments);
}
