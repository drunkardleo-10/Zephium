//! The runtime switch and the lead's run. A profile setting chooses which
//! agent runs a Work request: the lead, or the earlier runtime kept for one
//! release. The lead gets its models per role, the existing search path and
//! the same page machinery the earlier runtime used.
use super::*;
use zephium_app::work_lead::{LeadModel, WorkLeadModels, WorkLeadService};
use zephium_core::work::model::WorkModelRole;

const SETTING_PREFIX: &str = "work.runtime.";
/// Until the lead passes its acceptance runs, a profile without a choice
/// keeps the earlier runtime.
const DEFAULT: WorkRuntimeChoice = WorkRuntimeChoice::Classic;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkRuntimeChoice {
    Lead,
    Classic,
}
impl WorkRuntimeChoice {
    fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "lead" => Some(Self::Lead),
            "classic" => Some(Self::Classic),
            _ => None,
        }
    }
}

/// The runtime for `profile`: the environment's override for QA, else the
/// stored choice, else the default.
pub(crate) async fn runtime(profile: ProfileId) -> WorkRuntimeChoice {
    if let Some(choice) = std::env::var("ZEPHIUM_WORK_RUNTIME")
        .ok()
        .as_deref()
        .and_then(WorkRuntimeChoice::parse)
    {
        return choice;
    }
    let key = format!("{SETTING_PREFIX}{profile}");
    tokio::task::spawn_blocking(move || {
        use zephium_core::ports::store::Store;
        crate::APP_STORE
            .get()
            .and_then(|store| store.app_setting(&key))
            .as_deref()
            .and_then(WorkRuntimeChoice::parse)
    })
    .await
    .ok()
    .flatten()
    .unwrap_or(DEFAULT)
}

/// The models for each role; a helper role without its own model uses the
/// lead's. `None` when no lead model resolves: the earlier runtime runs.
pub(crate) async fn models(profile: ProfileId) -> Option<WorkLeadModels> {
    let resolve = |role| async move {
        zephium_app::work_models::resolve_entry(profile, role)
            .await
            .ok()
            .map(|(entry, client)| LeadModel { entry, client })
    };
    let lead = resolve(WorkModelRole::Lead).await?;
    let page = resolve(WorkModelRole::Page)
        .await
        .unwrap_or_else(|| lead.clone());
    let light = resolve(WorkModelRole::Light)
        .await
        .unwrap_or_else(|| page.clone());
    Some(WorkLeadModels { lead, page, light })
}

impl WorkProviders {
    pub(super) async fn lead(
        &self,
        shell: zephium_app::Handle,
        profile: ProfileId,
        command: zephium_ipc::work::WorkCommandV1,
        context: Option<zephium_core::work::context::WorkContextSelectionV1>,
        models: WorkLeadModels,
    ) -> Result<WorkOperationStateV1, WorkError> {
        use zephium_agentic::{AgentProviderTransport, AgentProviderTransportConfig};
        let zephium_core::work::runtime::WorkRuntimeIntent::BeginAgent { grant, .. } =
            &command.intent
        else {
            return Err(WorkError::Invalid);
        };
        let binding_request = shell.work_profile_binding();
        let binding = tokio::time::timeout(std::time::Duration::from_secs(8), async move {
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
        let search = configure_search_ranking(search, profile, search_transport).await?;
        record_diagnostic(format_args!(
            "work: phase=lead state=models lead={} page={} light={}",
            models.lead.entry.id, models.page.entry.id, models.light.entry.id
        ));
        let callback = shell.callback_handle();
        let work = command.work;
        let started = std::time::Instant::now();
        let service = WorkLeadService::new(shell).with_diagnostic(|event| {
            record_diagnostic(format_args!("work: phase=lead event={event:?}"));
        });
        let projection = service
            .run(
                profile,
                command,
                context,
                models,
                &search,
                |probe, request| {
                    let callback = &callback;
                    async move {
                        let mut settings = zephium_work_composition::durable_runtime::WorkBrowserAdapterSettings::new(
                            binding,
                            zephium_agent_controller::AgentBrowserModel::Gpt6Luna,
                            zephium_app::AgentWorkApplicationConfig::new(
                                zephium_agent_runtime::AgentRuntimeConfig::STANDARD,
                                AgentProviderTransportConfig::STANDARD,
                            ),
                            credential().await?,
                        );
                        settings.decisions = super::super::work_decision::composition_preference(
                            super::super::work_decision::selected_choice(profile).await,
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
                        "work: work={work} phase=lead status={:?} steps={} parts={} artifacts={} elapsed_ms={}",
                        fact.status,
                        fact.steps.len(),
                        fact.parts.len(),
                        fact.artifacts.len(),
                        started.elapsed().as_millis()
                    ));
                }
            }
            Err(error) => record_diagnostic(format_args!(
                "work: work={work} phase=lead failure={error:?} elapsed_ms={}",
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
