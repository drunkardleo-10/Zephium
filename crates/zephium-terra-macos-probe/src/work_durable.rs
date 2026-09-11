//! Opt-in public qualification: model plan, exact approval, native browser,
//! source-backed artifact, resource closure, then a new Store incarnation.
use std::{
    io::Write as _,
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
use zephium_agentic::{
    load_macos_development_openai_credential, AgentProviderTransport, AgentProviderTransportConfig,
    OpenAiWorkPlanner, OpenAiWorkSynthesizer, WorkPlanningConfig,
};
use zephium_app::{
    work_execution::WorkExecutionService, work_planning::WorkPlanningService,
    work_runtime::WorkRuntimeService,
};
use zephium_core::{
    ids::{ProfileId, SpaceId},
    ports::store::Store,
    profiles::ProfileKind,
    session::{PersistedProfile, PersistedSpace, SessionState},
    work::{port::*, runtime::*, *},
};
use zephium_ipc::work::*;
use zephium_work_composition::{durable_runtime::WorkBrowserAdapterSettings, MacosWorkComposition};

const OBJECTIVE: &str = "Find SQLite's official explanation of why WAL mode does not work when clients on different machines share a database over a network filesystem. Produce one concise source-backed note as a single plan responsibility. Use only public documentation at sqlite.org or www.sqlite.org. No account, writes, installations, or external communication are needed. Every factual output needs source-mapped human review.";
const COORDINATED_OBJECTIVE: &str = "Explain SQLite's official reason that WAL mode does not work when clients on different machines share a database over a network filesystem. Use exactly two plan responsibilities: a delegated public-documentation research worker with one source-backed findings output, then a primary agent that depends on those findings and produces one concise source-backed explanation. Both outputs require source_mapped_needs_review. Use only sqlite.org or www.sqlite.org. No accounts, writes, installations or external communication are needed.";

struct WorkflowResult {
    state: WorkRuntimeProjection,
    failure: Option<&'static str>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Mode {
    Public,
    Coordinated,
    CancelCoordinated,
    ProductIntegration,
}

pub(super) fn run() -> Result<(), super::ProbeFailure> {
    run_mode(Mode::Public)
}

pub(super) fn run_coordinated() -> Result<(), super::ProbeFailure> {
    run_mode(Mode::Coordinated)
}

pub(super) fn run_product() -> Result<(), super::ProbeFailure> {
    run_mode(Mode::ProductIntegration)
}

pub(super) fn run_cancelled() -> Result<(), super::ProbeFailure> {
    run_mode(Mode::CancelCoordinated)
}

fn run_mode(mode: Mode) -> Result<(), super::ProbeFailure> {
    use super::ProbeFailure as Error;
    let coordinated = mode != Mode::Public;
    let data = tempfile::Builder::new()
        .prefix("zephium-durable-work-")
        .tempdir()
        .map_err(|_| Error::Runtime)?;
    let store =
        Arc::new(zephium_store::SqliteStore::open(data.path()).map_err(|_| Error::Runtime)?);
    let profile = ProfileId::generate();
    let space = SpaceId::generate();
    store.save_session(SessionState {
        profiles: vec![PersistedProfile {
            id: profile,
            name: "Durable Work qualification".into(),
            kind: ProfileKind::Default,
        }],
        spaces: vec![PersistedSpace {
            id: space,
            profile,
            name: "Public research".into(),
        }],
        active_space: Some(space),
        ..SessionState::default()
    });
    if !store.flush() {
        return Err(Error::Runtime);
    }
    let blocker = zephium_blocker_service::ManagedBlocker::unconfigured(
        zephium_blocker::CompiledArtifactCacheConfig::new(data.path().join("compiled"))
            .map_err(|_| Error::Authority)?,
    )
    .map_err(|_| Error::Runtime)?;
    let extension = zephium_extension_service::prepare_extension_service_boot(
        store
            .claim_extension_service_store_authority()
            .map_err(|_| Error::Authority)?,
        zephium_extension_service::ExtensionRepositoryRoot::from_app_data_directory(
            data.path().to_owned(),
        )
        .map_err(|_| Error::Authority)?,
    )
    .map_err(|_| Error::Authority)?;
    let zephium_extension_service::ExtensionServiceBootPlan::Inert(extension) = extension else {
        return Err(Error::Authority);
    };
    // Credentials never enter Work, model context, diagnostics or serialized reports.
    let planning_key = load_macos_development_openai_credential().map_err(|_| Error::Keychain)?;
    let browser_keys = (0..4)
        .map(|_| load_macos_development_openai_credential())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| Error::Keychain)?;
    let owner_store = store.clone();
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let relay = Arc::new(Mutex::new(None::<zephium_app::CallbackHandle>));
    let events = relay.clone();
    let run = zephium_engine::run_macos_work_application_with_events_probe(
        profile,
        move |event| {
            if let Ok(relay) = events.lock() {
                if let Some(handle) = relay.as_ref() {
                    handle.dispatch(zephium_app::Command::Engine(event));
                }
            }
        },
        move |engine| {
            let shell = zephium_app::spawn_suspended(
                engine.clone(),
                owner_store.clone(),
                blocker,
                extension,
                Box::new(|_| {}),
                Arc::new(super::work_application::NoChrome),
                Box::new(|_| {}),
            )
            .map_err(|_| "durable_shell")?;
            *relay.lock().map_err(|_| "durable_events")? = Some(shell.callback_handle());
            let composition = MacosWorkComposition::new(engine, owner_store);
            if !shell.admit_startup() {
                return Err("durable_startup");
            }
            // The headless host has no chrome to send the ordinary bootstrap
            // command. Admission alone starts the actor, not the session.
            if !shell.dispatch(zephium_app::Command::Bootstrap) {
                return Err("durable_bootstrap");
            }
            let worker_handle = shell.clone();
            let worker = std::thread::Builder::new()
                .name("durable-work-qualification".into())
                .spawn(move || {
                    let result = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|_| "durable_tokio")
                        .and_then(|runtime| {
                            runtime.block_on(async {
                                tokio::time::timeout(
                                    Duration::from_secs(if coordinated { 240 } else { 160 }),
                                    workflow(
                                        &worker_handle,
                                        &composition,
                                        profile,
                                        planning_key,
                                        browser_keys,
                                        mode,
                                    ),
                                )
                                .await
                                .map_err(|_| "durable_deadline")?
                            })
                        });
                    let _ = result_tx.send(result);
                })
                .map_err(|_| "durable_worker")?;
            let mut worker = Some(worker);
            let mut shutdown = None;
            Ok(Box::new(move |native_failed| {
                if native_failed && shutdown.is_none() {
                    let request =
                        shell.shutdown_with_deadline(Instant::now() + Duration::from_secs(8));
                    shutdown = std::thread::Builder::new()
                        .name("durable-work-shutdown".into())
                        .spawn(move || request.recv_until_deadline())
                        .ok();
                }
                if worker.as_ref().is_some_and(|join| join.is_finished()) {
                    let joined = worker.take().is_some_and(|join| join.join().is_ok());
                    if !joined {
                        return Some(Err("durable_worker_panic"));
                    }
                    if shutdown.is_none() {
                        let request =
                            shell.shutdown_with_deadline(Instant::now() + Duration::from_secs(8));
                        shutdown = std::thread::Builder::new()
                            .name("durable-work-shutdown".into())
                            .spawn(move || request.recv_until_deadline())
                            .ok();
                        if shutdown.is_none() {
                            return Some(Err("durable_shutdown_thread"));
                        }
                    }
                }
                if shutdown.as_ref().is_some_and(|join| join.is_finished()) {
                    let clean = shutdown.take().is_some_and(|join| {
                        matches!(join.join(), Ok(Ok(zephium_app::ShutdownOutcome::Clean)))
                    });
                    return Some(if clean && !native_failed {
                        Ok(())
                    } else {
                        Err("durable_shutdown")
                    });
                }
                None
            }))
        },
    );
    run.map_err(|reason| {
        let _ = writeln!(
            std::io::stdout().lock(),
            "durable-work: host_failure={reason}; content=redacted"
        );
        Error::Runtime
    })?;
    let WorkflowResult { state, failure } = result_rx
        .recv_timeout(Duration::from_secs(1))
        .map_err(|_| Error::Runtime)?
        .map_err(|reason| {
            let _ = writeln!(
                std::io::stdout().lock(),
                "durable-work: failure={reason}; content=redacted"
            );
            Error::Runtime
        })?;
    drop(store);
    let reopened = zephium_store::SqliteStore::open(data.path()).map_err(|_| Error::Runtime)?;
    let (tx, rx) = mpsc::sync_channel(1);
    reopened
        .work_document(
            profile,
            WorkRequest::RuntimeRead { id: state.work.id },
            Box::new(move |result| {
                let _ = tx.send(result);
            }),
        )
        .map_err(|_| Error::Runtime)?;
    let WorkReply::Runtime(restored) = rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| Error::Runtime)?
        .map_err(|_| Error::Runtime)?
    else {
        return Err(Error::Runtime);
    };
    if *restored != state {
        return Err(Error::Runtime);
    }
    if mode == Mode::CancelCoordinated {
        let execution = state.executions.first().ok_or(Error::Runtime)?;
        if failure != Some("child_execution")
            || execution.status != WorkExecutionStatus::Cancelled
            || execution.attempts.len() != 2
            || !execution.artifacts.is_empty()
            || execution.attempts.iter().any(|attempt| {
                !matches!(
                    attempt.status,
                    WorkAttemptStatus::Failed | WorkAttemptStatus::Cancelled
                ) || attempt.usage.is_none()
            })
            || !execution
                .attempts
                .iter()
                .any(|attempt| attempt.status == WorkAttemptStatus::Cancelled)
        {
            return Err(Error::Runtime);
        }
        if reopened.shutdown_until(Instant::now() + Duration::from_secs(5))
            != zephium_core::ports::store::StoreShutdownOutcome::Clean
        {
            return Err(Error::Runtime);
        }
        std::fs::write(
            "target/work-runtime-proof/coordinated-cancelled.json",
            serde_json::to_vec_pretty(&serde_json::json!({
                "cancelled_after_native_model_admission": true,
                "host_shutdown_clean": true,
                "reopened": true,
                "projection": state,
            }))
            .map_err(|_| Error::Output)?,
        )
        .map_err(|_| Error::Output)?;
        writeln!(std::io::stdout().lock(), "durable-work: cancellation=true; original_child_settled=true; primary_synthesis=false; resource_closed=true; reopened=true; content=redacted").map_err(|_| Error::Output)?;
        return Ok(());
    }
    if let Some(failure) = failure {
        // Reopening failed facts is useful evidence, but never a successful
        // research report. Native host shutdown has already acknowledged its
        // original owners; unknown attempt outcomes remain unknown in Store.
        if reopened.shutdown_until(Instant::now() + Duration::from_secs(5))
            != zephium_core::ports::store::StoreShutdownOutcome::Clean
        {
            return Err(Error::Runtime);
        }
        std::fs::write(
            "target/work-runtime-proof/coordinated-failure.json",
            serde_json::to_vec_pretty(&serde_json::json!({
                "failure": failure,
                "host_shutdown_clean": true,
                "reopened": true,
                "projection": state,
            }))
            .map_err(|_| Error::Output)?,
        )
        .map_err(|_| Error::Output)?;
        let _ = writeln!(std::io::stdout().lock(), "durable-work: failure={failure}; host_shutdown_clean=true; reopened=true; content=redacted");
        return Err(Error::Runtime);
    }
    let expected_status = if mode == Mode::ProductIntegration {
        WorkExecutionStatus::Completed
    } else {
        WorkExecutionStatus::NeedsReview
    };
    if state.executions[0].status != expected_status || state.executions[0].artifacts.is_empty() {
        return Err(Error::Runtime);
    }
    let mut evidence = Vec::new();
    for link in state.executions[0]
        .artifacts
        .iter()
        .flat_map(|artifact| &artifact.evidence)
    {
        let (tx, rx) = mpsc::sync_channel(1);
        reopened
            .work_document(
                profile,
                WorkRequest::ReadEvidence {
                    id: state.work.id,
                    link: link.clone(),
                },
                Box::new(move |result| {
                    let _ = tx.send(result);
                }),
            )
            .map_err(|_| Error::Runtime)?;
        let WorkReply::Evidence(preview) = rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| Error::Runtime)?
            .map_err(|_| Error::Runtime)?
        else {
            return Err(Error::Runtime);
        };
        evidence.push(preview);
    }
    let output = std::path::Path::new("target/work-runtime-proof");
    std::fs::create_dir_all(output).map_err(|_| Error::Output)?;
    std::fs::write(
        output.join(if mode == Mode::ProductIntegration {
            "product-integration.json"
        } else if coordinated {
            "coordinated-research.json"
        } else {
            "public-research.json"
        }),
        serde_json::to_vec_pretty(&serde_json::json!({
            "projection": state,
            "historical_evidence": evidence,
        }))
        .map_err(|_| Error::Output)?,
    )
    .map_err(|_| Error::Output)?;
    if reopened.shutdown_until(Instant::now() + Duration::from_secs(5))
        != zephium_core::ports::store::StoreShutdownOutcome::Clean
    {
        return Err(Error::Runtime);
    }
    writeln!(std::io::stdout().lock(), "durable-work: model_plan=true; exact_approval=true; native_browser=true; artifacts={}; resource_closed=true; reopened=true; semantic_status={:?}; content=redacted", state.executions[0].artifacts.len(), state.executions[0].status).map_err(|_| Error::Output)?;
    Ok(())
}

async fn workflow(
    handle: &zephium_app::Handle,
    composition: &MacosWorkComposition,
    profile: ProfileId,
    planning_key: zephium_agentic::AgentProviderCredential,
    mut browser_keys: Vec<zephium_agentic::AgentProviderCredential>,
    mode: Mode,
) -> Result<WorkflowResult, &'static str> {
    let coordinated = mode != Mode::Public;
    let binding = loop {
        let selected = handle.work_profile_binding();
        let answer = loop {
            if let Some(answer) = selected.try_recv() {
                break answer;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        match answer {
            zephium_app::AgentWorkProfileReadiness::Ready(binding)
                if binding.profile() == profile =>
            {
                break binding
            }
            zephium_app::AgentWorkProfileReadiness::PolicyPending(_)
            | zephium_app::AgentWorkProfileReadiness::PolicyMissing => {
                tokio::time::sleep(Duration::from_millis(10)).await
            }
            zephium_app::AgentWorkProfileReadiness::ProfileMissing => {
                tokio::time::sleep(Duration::from_millis(10)).await
            }
            zephium_app::AgentWorkProfileReadiness::PolicyFailed => {
                return Err("profile_policy_failed")
            }
            _ => return Err("profile_not_ready"),
        }
    };
    let created = handle
        .work_authoring_command(
            profile,
            WorkAuthoringCommandV1 {
                version: 1,
                command: WorkCommandId::generate(),
                intent: WorkAuthoringIntent::Create {
                    objective: if coordinated {
                        COORDINATED_OBJECTIVE
                    } else {
                        OBJECTIVE
                    }
                    .into(),
                },
            },
        )
        .map_err(|_| "create_admission")?
        .response(profile)
        .await;
    let WorkReplyV1::AuthoringApplied { receipt: created } = created.reply else {
        return Err("create_persistence");
    };
    let work = created.work;
    let transport = AgentProviderTransport::try_new(AgentProviderTransportConfig::STANDARD)
        .map_err(|_| "planning_transport")?;
    let synthesis = if coordinated {
        Some(
            OpenAiWorkSynthesizer::try_new(
                transport.clone(),
                browser_keys.pop().ok_or("synthesis_key")?,
                WorkPlanningConfig::try_new(
                    zephium_agent_model_catalog::try_luna_provider_exact_call_config(4096)
                        .map_err(|_| "synthesis_model")?,
                    8192,
                    10_000,
                )
                .map_err(|_| "synthesis_limits")?,
            )
            .map_err(|_| "synthesis_provider")?
            .with_public_response_retention(),
        )
    } else {
        None
    };
    let planner = OpenAiWorkPlanner::try_new(
        transport,
        planning_key,
        WorkPlanningConfig::try_new(
            zephium_agent_model_catalog::try_luna_provider_exact_call_config(4096)
                .map_err(|_| "model_config")?,
            8192,
            100_000,
        )
        .map_err(|_| "planning_limits")?,
    )
    .map_err(|_| "planner")?;
    let result = WorkPlanningService::new(handle.clone(), Arc::new(planner))
        .plan_request(
            profile,
            WorkPlanRequestV1 {
                version: 1,
                work,
                expected_revision: created.applied_revision,
            },
        )
        .await;
    let WorkPlanningOutcomeV1::Settled { response } = result.outcome else {
        return Err("planning");
    };
    let WorkReplyV1::Snapshot { snapshot: planned } = response.reply else {
        return Err("plan_persistence");
    };
    let plan = planned.plan.as_ref().ok_or("clarification_required")?;
    std::fs::create_dir_all("target/work-runtime-proof").map_err(|_| "plan_report")?;
    std::fs::write(
        if coordinated {
            "target/work-runtime-proof/coordinated-plan.json"
        } else {
            "target/work-runtime-proof/public-plan.json"
        },
        serde_json::to_vec_pretty(&planned).map_err(|_| "plan_report")?,
    )
    .map_err(|_| "plan_report")?;
    if plan.draft.nodes.len() > browser_keys.len() {
        return Err("qualification_node_limit");
    }
    let primary = if coordinated {
        if plan.draft.nodes.len() != 2
            || plan.draft.nodes.iter().any(|n| {
                n.outputs.len() != 1
                    || n.outputs[0].review != WorkOutputReview::SourceMappedNeedsReview
            })
        {
            return Err("qualification_primary_shape");
        }
        Some(
            plan.draft
                .nodes
                .iter()
                .find(|n| n.dependencies.len() == 1)
                .ok_or("qualification_primary_dependency")?
                .id,
        )
    } else {
        None
    };
    let limits = WorkExecutionLimits {
        model_tokens: 400_000,
        cost_micro_usd: 500_000,
        operations: 48,
        timeout_seconds: 120,
        max_workers: 1,
    };
    let limits = if coordinated {
        WorkExecutionLimits {
            model_tokens: 800_000,
            cost_micro_usd: 1_000_000,
            operations: 96,
            timeout_seconds: 180,
            max_workers: 2,
        }
    } else {
        limits
    };
    let driver = WorkExecutionService::new(handle.clone());
    let preview = driver
        .prepare_public_approval(
            profile,
            WorkApprovalRequestV1 {
                version: 1,
                work,
                expected_revision: planned.revision,
                limits,
                primary,
                scope: WorkBrowseScope {
                    start_url: "https://sqlite.org/docs.html".into(),
                    routes: ["https://sqlite.org", "https://www.sqlite.org"]
                        .into_iter()
                        .map(|origin| WorkBrowseRoute {
                            origin: origin.into(),
                            path_prefix: "/".into(),
                        })
                        .collect(),
                    max_hops: 8,
                },
            },
        )
        .await
        .map_err(|_| "approval_preview")?;
    let WorkReplyV1::ApprovalDraft { spec, .. } = preview.reply else {
        return Err("approval_preview_reply");
    };
    // Explicit qualification approval of this exact model-produced revision,
    // limited to the public scope and budget requested above. No remote writes.
    let request = handle
        .work_command(
            profile,
            zephium_ipc::work::WorkCommandV1 {
                version: 1,
                work,
                expected_revision: planned.revision,
                command: WorkCommandId::generate(),
                intent: WorkRuntimeIntent::Approve { spec },
            },
        )
        .map_err(|_| "approval_admission")?;
    let WorkReply::RuntimeCommand {
        projection: approved,
        receipt,
    } = request.await.map_err(|_| "approval_persistence")?.reply
    else {
        return Err("approval_reply");
    };
    let mut state = *approved;
    let mut completed = Vec::new();
    let mut browser_keys = browser_keys.into_iter();
    if matches!(mode, Mode::Coordinated | Mode::ProductIntegration) {
        state = driver
            .execute_request(
                profile,
                WorkStartRequestV1 {
                    version: 1,
                    work,
                    expected_revision: state.work.revision,
                    execution: receipt.execution,
                },
                synthesis.as_ref().ok_or("primary_model")?,
                |attempt| {
                    let key = browser_keys.next();
                    async move {
                        let key = key.ok_or(WorkError::Capacity)?;
                        composition
                            .execute_public_node_owned(
                                &handle.callback_handle(),
                                attempt,
                                browser_settings(binding, key),
                            )
                            .await
                    }
                },
                |_| {},
            )
            .await
            .map_err(|_| "product_execution")?;
        std::fs::write(
            "target/work-runtime-proof/coordinated-attempt.json",
            serde_json::to_vec_pretty(&state).map_err(|_| "primary_report")?,
        )
        .map_err(|_| "primary_report")?;
        if state.executions[0].status != WorkExecutionStatus::NeedsReview
            || state.executions[0].artifacts.len() != 2
        {
            return Ok(WorkflowResult {
                state,
                failure: Some("product_execution"),
            });
        }
        if mode == Mode::ProductIntegration {
            state = review_product_results(handle, profile, state).await?;
        }
        writeln!(std::io::stdout().lock(), "durable-work: product_driver=true; original_parent=true; original_child=true; structured_handoff=true; primary_synthesis=true; content=redacted").map_err(|_| "primary_progress")?;
        return Ok(WorkflowResult {
            state,
            failure: None,
        });
    }
    if let Some(primary) = primary {
        let attempt = WorkRuntimeService::new(handle.clone())
            .begin_node(
                profile,
                work,
                state.work.revision,
                receipt.execution,
                primary,
            )
            .await
            .map_err(|_| "primary_admission")?;
        let mut coordinator = attempt
            .coordinate()
            .await
            .map_err(|_| "primary_ownership")?;
        let child = plan
            .draft
            .nodes
            .iter()
            .find(|n| n.id != primary)
            .ok_or("child_node")?
            .id;
        let credential = browser_keys.next().ok_or("child_key")?;
        let child_result = coordinator
            .execute_child(child, |attempt| async {
                let observer = attempt.observer();
                let callback = handle.callback_handle();
                let operation = composition.execute_public_node_owned(
                    &callback,
                    attempt,
                    browser_settings(binding, credential),
                );
                if mode != Mode::CancelCoordinated {
                    return operation.await;
                }
                tokio::pin!(operation);
                loop {
                    tokio::select! {
                        result = &mut operation => return result,
                        _ = tokio::time::sleep(Duration::from_millis(10)) => {}
                    }
                    if observer.latest().is_some_and(|signal| {
                        signal.activity == zephium_ipc::work::WorkActivityV1::Planning
                    }) {
                        let WorkReply::Runtime(current) =
                            handle.work_projection(profile, work)?.await?.reply
                        else {
                            return Err(WorkError::Invalid);
                        };
                        handle
                            .work_command(
                                profile,
                                zephium_ipc::work::WorkCommandV1 {
                                    version: 1,
                                    work,
                                    expected_revision: current.work.revision,
                                    command: WorkCommandId::generate(),
                                    intent: WorkRuntimeIntent::Cancel {
                                        execution: receipt.execution,
                                    },
                                },
                            )?
                            .await?;
                        // Keep polling the original adapter to settle native,
                        // provider and Store ownership after durable stop intent.
                        return operation.await;
                    }
                }
            })
            .await
            .map_err(|error| {
                let _ = writeln!(
                    std::io::stdout().lock(),
                    "durable-work: child_error={error:?}; content=redacted"
                );
                "child_execution"
            });
        let state = coordinator
            .finish(synthesis.as_ref().ok_or("primary_model")?)
            .await
            .map_err(|_| "primary_synthesis")?
            .into_projection();
        // Keep failed qualification facts too. This snapshot does not claim
        // native closure or Store reopen; only the final report below does.
        // Finish consumes the poisoned original coordinator without another
        // model call when a child fails, retaining any unknown child charge.
        std::fs::write(
            "target/work-runtime-proof/coordinated-attempt.json",
            serde_json::to_vec_pretty(&state).map_err(|_| "primary_report")?,
        )
        .map_err(|_| "primary_report")?;
        if let Err(failure) = child_result {
            return Ok(WorkflowResult {
                state,
                failure: Some(failure),
            });
        }
        if state.executions[0].attempts.len() != 2 || state.executions[0].artifacts.len() != 2 {
            return Ok(WorkflowResult {
                state,
                failure: Some("primary_publication"),
            });
        }
        writeln!(std::io::stdout().lock(), "durable-work: original_parent=true; original_child=true; structured_handoff=true; primary_synthesis=true; content=redacted").map_err(|_| "primary_progress")?;
        return Ok(WorkflowResult {
            state,
            failure: None,
        });
    }
    // Bounded sequential qualification dispatch of the model's exact DAG.
    // This does not claim primary/child model delegation or a production scheduler.
    while completed.len() < plan.draft.nodes.len() {
        let node = plan
            .draft
            .nodes
            .iter()
            .find(|node| {
                !completed.contains(&node.id)
                    && node.dependencies.iter().all(|id| completed.contains(id))
            })
            .ok_or("dependency_readiness")?
            .id;
        let attempt = WorkRuntimeService::new(handle.clone())
            .begin_node(profile, work, state.work.revision, receipt.execution, node)
            .await
            .map_err(|_| "attempt_admission")?;
        writeln!(std::io::stdout().lock(), "durable-work: model_plan=true; exact_approval=true; starting_native=true; content=redacted").map_err(|_| "progress_output")?;
        state = composition
            .execute_public_node(
                &handle.callback_handle(),
                attempt,
                browser_settings(
                    binding,
                    browser_keys.next().ok_or("qualification_key_limit")?,
                ),
            )
            .await
            .map_err(|error| {
                let _ = writeln!(
                    std::io::stdout().lock(),
                    "durable-work: execution_error={error:?}; content=redacted"
                );
                "native_execution"
            })?;
        if !state.executions[0]
            .attempts
            .iter()
            .any(|attempt| attempt.node == node && attempt.status == WorkAttemptStatus::Succeeded)
        {
            return Err("node_not_successful");
        }
        completed.push(node);
    }
    Ok(WorkflowResult {
        state,
        failure: None,
    })
}

async fn review_product_results(
    handle: &zephium_app::Handle,
    profile: ProfileId,
    mut state: WorkRuntimeProjection,
) -> Result<WorkRuntimeProjection, &'static str> {
    let original = state.executions[0].artifacts.clone();
    let execution = state.executions[0].id;
    let work = state.work.id;
    let apply = |command| async move {
        let reply = handle
            .work_command(profile, command)
            .map_err(|_| "review_admission")?
            .response(profile)
            .await;
        let WorkReplyV1::ExecutionApplied { projection, .. } = reply.reply else {
            return Err("review_persistence");
        };
        Ok::<_, &'static str>(*projection)
    };
    for artifact in &original {
        let command = WorkCommandV1 {
            version: 1,
            work,
            expected_revision: state.work.revision,
            command: WorkCommandId::generate(),
            intent: WorkRuntimeIntent::ReviewArtifact {
                execution,
                artifact: artifact.id,
                decision: WorkArtifactDecision::Accepted,
            },
        };
        state = apply(command.clone()).await?;
        if apply(command).await? != state {
            return Err("review_replay");
        }
    }
    if state.executions[0].status != WorkExecutionStatus::Completed {
        return Err("review_completion");
    }
    let primary = original.last().ok_or("review_artifact")?;
    let WorkArtifactDataV1::Document { paragraphs } = &primary.data else {
        return Err("review_document");
    };
    let mut paragraphs = paragraphs.clone();
    paragraphs.push("Review note: preserve the cited sources and any stated evidence limitations when using this result.".into());
    let previous_revision = state.work.revision;
    state = apply(WorkCommandV1 {
        version: 1,
        work,
        expected_revision: previous_revision,
        command: WorkCommandId::generate(),
        intent: WorkRuntimeIntent::EditArtifact {
            execution,
            artifact: primary.id,
            data: WorkArtifactDataV1::Document { paragraphs },
            evidence: primary.evidence.clone(),
        },
    })
    .await?;
    if state.executions[0].status != WorkExecutionStatus::NeedsReview {
        return Err("edit_review_reset");
    }
    let review = WorkRuntimeIntent::ReviewArtifact {
        execution,
        artifact: primary.id,
        decision: WorkArtifactDecision::Accepted,
    };
    let stale = handle
        .work_command(
            profile,
            WorkCommandV1 {
                version: 1,
                work,
                expected_revision: previous_revision,
                command: WorkCommandId::generate(),
                intent: review.clone(),
            },
        )
        .map_err(|_| "stale_review_admission")?
        .response(profile)
        .await;
    if !matches!(
        stale.reply,
        WorkReplyV1::Error {
            error: WorkFailureV1::Conflict
        }
    ) {
        return Err("stale_review_not_refused");
    }
    state = apply(WorkCommandV1 {
        version: 1,
        work,
        expected_revision: state.work.revision,
        command: WorkCommandId::generate(),
        intent: review,
    })
    .await?;
    if state.executions[0].status != WorkExecutionStatus::Completed
        || state.executions[0].artifacts != original
    {
        return Err("review_original_changed");
    }
    writeln!(std::io::stdout().lock(), "durable-work: product_commands=true; accepted=true; edited=true; original_artifact_immutable=true; stale_review_refused=true; replay_idempotent=true; content=redacted").map_err(|_| "review_report")?;
    Ok(state)
}

fn browser_settings(
    profile: zephium_app::AgentWorkProfileBinding,
    credential: zephium_agentic::AgentProviderCredential,
) -> WorkBrowserAdapterSettings {
    WorkBrowserAdapterSettings {
        retain_public_responses: true,
        resource_diagnostic: Some(|cause| {
            let _ = writeln!(
                std::io::stdout().lock(),
                "durable-work: resource_failure={cause:?}; content=redacted"
            );
        }),
        diagnostic: Some(|snapshot| {
            let _ = writeln!(
                std::io::stdout().lock(),
                "durable-work: native_phase={:?}; failure={:?}; persistence={:?}; content=redacted",
                snapshot.phase,
                snapshot.failure,
                snapshot.persistence_failure
            );
        }),
        profile,
        model: zephium_agent_controller::AgentBrowserModel::Luna,
        config: zephium_app::AgentWorkApplicationConfig::new(
            zephium_agent_runtime::AgentRuntimeConfig::STANDARD,
            AgentProviderTransportConfig::STANDARD,
        ),
        credential,
    }
}
