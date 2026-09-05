//! Public-only qualification through the real shell actor and desktop adapter.

use std::{
    io::Write as _,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use zephium_agent_controller::AgentWorkEventKind;
use zephium_agent_provider_transport::{
    load_macos_probe_openai_credential, AgentProviderTransportConfig,
};
use zephium_agent_runtime::{AgentRuntimeConfig, AgentRuntimeStopReason};
use zephium_app::{AgentWorkApplicationConfig, AgentWorkApplicationPhase, ShutdownOutcome};
use zephium_work_composition::{MacosWorkComposition, TrustedWorkRequest};

struct NoChrome;
impl zephium_core::ports::chrome::Chrome for NoChrome {
    fn position(&self, _: zephium_core::ports::chrome::ChromeFrame) -> bool {
        false
    }
}
impl zephium_app::PresentationChrome for NoChrome {
    fn apply_tab_for_presentation(
        &self,
        _: zephium_app::ChromePresentation,
        _: zephium_app::ChromePresentationCallback,
    ) -> zephium_app::ChromePresentationDispatch {
        zephium_app::ChromePresentationDispatch::Rejected
    }
}

pub(super) fn run() -> Result<(), super::ProbeFailure> {
    run_mode(Qualification::Actions)
}

pub(super) fn run_extraction() -> Result<(), super::ProbeFailure> {
    run_mode(Qualification::Extraction)
}

pub(super) fn run_artifact() -> Result<(), super::ProbeFailure> {
    run_mode(Qualification::Artifact)
}

pub(super) fn run_cancellation() -> Result<(), super::ProbeFailure> {
    run_mode(Qualification::CancelExtraction)
}

#[derive(Clone, Copy)]
enum Qualification {
    Actions,
    Extraction,
    Artifact,
    CancelExtraction,
}

fn run_mode(mode: Qualification) -> Result<(), super::ProbeFailure> {
    use super::ProbeFailure as Error;
    let extraction = !matches!(mode, Qualification::Actions);
    let durable = matches!(mode, Qualification::Artifact);
    let cancel_after_turn = matches!(mode, Qualification::CancelExtraction);
    let started = Instant::now();
    let (profile, input, task) = if durable {
        super::work_actor::artifact_input(started)?
    } else if extraction {
        super::work_actor::extraction_input(started)?
    } else {
        super::work_actor::input(started)?
    };
    let data = tempfile::Builder::new()
        .prefix("zephium-public-work-")
        .tempdir()
        .map_err(|_| Error::Runtime)?;
    let store =
        Arc::new(zephium_store::SqliteStore::open(data.path()).map_err(|_| Error::Runtime)?);
    if durable {
        register_public_profile(&store, profile)?;
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
    let request = TrustedWorkRequest::new(
        input,
        AgentWorkApplicationConfig::new(
            AgentRuntimeConfig::STANDARD,
            AgentProviderTransportConfig::STANDARD,
        ),
        load_macos_probe_openai_credential().map_err(|_| Error::Keychain)?,
        task,
    );
    let result = zephium_engine::run_macos_agentic_work_application_probe(profile, move |engine| {
        let failed = Arc::new(AtomicBool::new(false));
        let fail_sink = failed.clone();
        let shell = zephium_app::spawn_suspended(
            engine.clone(),
            store.clone(),
            blocker,
            extension,
            Box::new(move |_| {
                fail_sink.store(true, Ordering::Release);
            }),
            Arc::new(NoChrome),
            Box::new(|_| {}),
        )
        .map_err(|_| "application_shell_spawn")?;
        let composition = MacosWorkComposition::new(engine.clone(), store.clone());
        let prepared = composition
            .prepare_public_qualification(request)
            .map_err(|_| "application_prepare")?;
        let view = composition
            .attach(&shell.callback_handle())
            .ok_or("application_attach")?;
        view.admit(prepared).map_err(|_| "application_admit")?;
        if !shell.admit_startup() {
            return Err("application_startup");
        }
        let mut shutdown: Option<
            std::thread::JoinHandle<Result<ShutdownOutcome, std::sync::mpsc::RecvTimeoutError>>,
        > = None;
        let mut terminal_success = false;
        let mut terminal_cancelled = false;
        let mut cancellation_requested = false;
        let mut native_actions = 0_u32;
        let mut terminal_observed = false;
        let mut archive_requested = false;
        let mut archive_verified = false;
        let mut cleanup = None;
        let (mut turns, mut effects, mut tokens_in, mut tokens_out, mut cost) =
            (0_u32, 0_u32, 0_u64, 0_u64, 0_u64);
        Ok(Box::new(move |native_failed| {
            let snapshot = view.snapshot();
            if native_failed || failed.load(Ordering::Acquire) {
                if let Some(run) = snapshot.run {
                    let _ = view.stop(run, AgentRuntimeStopReason::Cancelled);
                }
            }
            while let Some(event) = view.take_event() {
                let mut output = std::io::stdout().lock();
                let written = match event.kind() {
                    AgentWorkEventKind::ModelSettled {
                        call,
                        input_tokens,
                        output_tokens,
                        request_bytes,
                        semantic_bytes,
                        cost_micro_usd,
                        accounting,
                        elapsed_millis,
                    } => {
                        turns += 1;
                        tokens_in += input_tokens;
                        tokens_out += output_tokens;
                        cost += cost_micro_usd;
                        writeln!(output, "work-application-turn: call={}; input_tokens={input_tokens}; output_tokens={output_tokens}; request_bytes={request_bytes}; semantic_bytes={semantic_bytes}; cost_micro_usd={cost_micro_usd}; accounting={accounting:?}; turn_ms={elapsed_millis}; wall_ms={}; content=redacted", call.get(), event.elapsed_millis())
                    }
                    kind => {
                        if matches!(kind, AgentWorkEventKind::ActionActive) {
                            native_actions += 1;
                        }
                        if matches!(kind, AgentWorkEventKind::Verified) {
                            effects += 1;
                        }
                        writeln!(output, "work-application-event: sequence={}; phase={kind:?}; wall_ms={}; content=redacted", event.sequence(), event.elapsed_millis())
                    }
                };
                if written.is_err() {
                    failed.store(true, Ordering::Release);
                }
            }
            // Explicit trusted test control after one real settled Luna turn.
            // This is the same application stop port as a human takeover; no
            // provider or native fault is synthesized and no request is retried.
            if cancel_after_turn && turns > 0 && !cancellation_requested {
                cancellation_requested = snapshot
                    .run
                    .is_some_and(|run| view.stop(run, AgentRuntimeStopReason::HumanTakeover));
                if !cancellation_requested {
                    failed.store(true, Ordering::Release);
                }
            }
            if shutdown.is_none()
                && (matches!(
                    snapshot.phase,
                    AgentWorkApplicationPhase::Succeeded
                        | AgentWorkApplicationPhase::Failed
                        | AgentWorkApplicationPhase::Cancelled
                        | AgentWorkApplicationPhase::Recovery
                        | AgentWorkApplicationPhase::NeedsReview
                        | AgentWorkApplicationPhase::PersistenceUncertain
                ) || native_failed
                    || failed.load(Ordering::Acquire))
            {
                if !terminal_observed {
                    terminal_observed = true;
                    terminal_success = snapshot.phase == AgentWorkApplicationPhase::Succeeded
                        && snapshot.failure.is_none()
                        && view.records().iter().any(|record| {
                            record.disposition() == zephium_agentic::AgentWorkDisposition::Succeeded
                        });
                    terminal_cancelled = cancel_after_turn
                        && cancellation_requested
                        && snapshot.phase == AgentWorkApplicationPhase::Cancelled
                        && snapshot.failure
                            == Some(zephium_agent_controller::AgentWorkFailure::HumanTakeover)
                        && snapshot.persistence_failure.is_none()
                        && turns > 0
                        && native_actions == 0
                        && effects == 0
                        && view.take_extraction().is_none()
                        && view.records().iter().any(|record| {
                            record.disposition() == zephium_agentic::AgentWorkDisposition::Cancelled
                                && record.debt() == zephium_agentic::AgentWorkDebt::NONE
                        });
                    if extraction && terminal_success {
                        terminal_success = view.take_extraction().is_some_and(|result| {
                        let verified = verify_extraction(&result);
                        let stats = result.stats();
                        let _ = writeln!(std::io::stdout().lock(), "work-application-result: fields={}; values={}; source_edges={}; independently_verified={verified}; artifact_promised={durable}; content=redacted", stats.fields(), stats.values(), stats.source_edges());
                        verified && view.take_extraction().is_none()
                    });
                    }
                }
                if durable && terminal_success && !archive_verified {
                    if !archive_requested {
                        archive_requested = view
                            .records()
                            .into_iter()
                            .find(|record| {
                                record.disposition()
                                    == zephium_agentic::AgentWorkDisposition::Succeeded
                            })
                            .is_some_and(|record| view.read_artifact(record, profile));
                        if archive_requested {
                            return None;
                        }
                        terminal_success = false;
                    } else if let Some(archive) = view.take_archived_extraction() {
                        archive_verified = verify_archive(&archive);
                        terminal_success &= archive_verified;
                        let _ = writeln!(std::io::stdout().lock(), "work-application-artifact: body_bytes={}; fields={}; independently_verified={archive_verified}; publication=atomic; content=redacted", archive.descriptor().bytes(), archive.fields().len());
                    } else if snapshot.artifact_read.is_some() {
                        terminal_success = false;
                    } else {
                        return None;
                    }
                }
                if durable {
                    if cleanup.is_none() {
                        cleanup = Some(super::work_artifact_cleanup::Cleanup::start(
                            store.clone(),
                            engine.clone(),
                            profile,
                        ));
                    }
                    let erased = cleanup.as_mut().and_then(|cleanup| cleanup.poll())?;
                    terminal_success &= archive_verified && erased;
                }
                let _ = writeln!(std::io::stdout().lock(), "work-application-terminal: phase={:?}; failure={:?}; persistence={:?}; durable_success={terminal_success}; cancelled_closed={terminal_cancelled}; content=redacted", snapshot.phase, snapshot.failure, snapshot.persistence_failure);
                let request = shell.shutdown_with_deadline(Instant::now() + Duration::from_secs(8));
                shutdown = std::thread::Builder::new()
                    .name("work-qualifier-join".into())
                    .spawn(move || request.recv_until_deadline())
                    .ok();
                if shutdown.is_none() {
                    return Some(Err("application_join_spawn"));
                }
            }
            if shutdown.as_ref().is_some_and(|join| join.is_finished()) {
                let clean = shutdown
                    .take()
                    .is_some_and(|join| matches!(join.join(), Ok(Ok(ShutdownOutcome::Clean))));
                let _ = writeln!(std::io::stdout().lock(), "work-application-closure: model=gpt-5.6-luna; durable_success={terminal_success}; cancelled_closed={terminal_cancelled}; shell_clean={clean}; turns={turns}; native_actions={native_actions}; verified_effects={effects}; input_tokens={tokens_in}; output_tokens={tokens_out}; cost_micro_usd={cost}; elapsed_ms={}; content=redacted", started.elapsed().as_millis());
                return Some(
                    if clean
                        && (if cancel_after_turn {
                            terminal_cancelled
                        } else {
                            terminal_success
                        })
                        && !failed.load(Ordering::Acquire)
                    {
                        Ok(())
                    } else {
                        Err("application_closure")
                    },
                );
            }
            None
        }))
    });
    if durable && result.is_err() {
        // Preserve exact Store recovery metadata if native deletion was not
        // qualified. Never discard its only durable obligation on failure.
        let _ = data.keep();
        let _ = writeln!(
            std::io::stdout().lock(),
            "work-artifact-recovery: private_test_directory_retained=true; content=redacted"
        );
    }
    result.map_err(Error::Engine)?;
    writeln!(std::io::stdout().lock(), "work-application-qualified: focus_isolation=passed; teardown=application_owned; elapsed_ms={}; content=redacted", started.elapsed().as_millis()).map_err(|_| Error::Output)
}

fn register_public_profile(
    store: &zephium_store::SqliteStore,
    profile: zephium_core::ids::ProfileId,
) -> Result<(), super::ProbeFailure> {
    use zephium_core::{
        ids::{ProfileId, SpaceId},
        ports::store::Store,
        profiles::ProfileKind,
        session::{PersistedProfile, PersistedSpace, SessionState},
    };
    let default = ProfileId::generate();
    let default_space = SpaceId::generate();
    store.save_session(SessionState {
        profiles: vec![
            PersistedProfile {
                id: default,
                name: "Public test default".into(),
                kind: ProfileKind::Default,
            },
            PersistedProfile {
                id: profile,
                name: "Public Work test".into(),
                kind: ProfileKind::Named,
            },
        ],
        spaces: vec![
            PersistedSpace {
                id: default_space,
                profile: default,
                name: "Public test default".into(),
            },
            PersistedSpace {
                id: SpaceId::generate(),
                profile,
                name: "Public Work test".into(),
            },
        ],
        items: vec![],
        active_space: Some(default_space),
        active_item: None,
        splits: None,
        recently_closed: vec![],
    });
    if store.flush() {
        Ok(())
    } else {
        Err(super::ProbeFailure::Runtime)
    }
}

fn verify_archive(archive: &zephium_agentic::AgentWorkArchivedExtraction) -> bool {
    use zephium_agentic::*;
    if archive.trust() != SemanticExtractionTrust::ModelMapped || archive.fields().len() != 1 {
        return false;
    }
    let ArchivedValue::TextList { items, .. } = archive.fields()[0].value() else {
        return false;
    };
    if items.len() != 10 {
        return false;
    }
    let mut ids = std::collections::BTreeSet::new();
    let (mut english, mut german) = (false, false);
    for item in items {
        let [id] = item.source_ids() else {
            return false;
        };
        let Some(source) = archive.source(*id) else {
            return false;
        };
        let ArchivedSourceContent::Text { value } = source.content() else {
            return false;
        };
        if !ids.insert(id) || source.role() != "link" || value != item.value() {
            return false;
        }
        english |= value.contains("English");
        german |= value.contains("Deutsch");
    }
    english && german
}

fn verify_extraction(result: &zephium_agentic::SemanticOwnedExtractionResult) -> bool {
    use zephium_agentic::*;
    if result.trust() != SemanticExtractionTrust::ModelMapped || result.fields().len() != 1 {
        return false;
    }
    let SemanticExtractedValue::TextList(list) = result.fields()[0].value() else {
        return false;
    };
    if list.items().len() != 10 {
        return false;
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut english = false;
    let mut german = false;
    for item in list.items() {
        let Some(mut sources) = result.sources(item.source_span()) else {
            return false;
        };
        let Some(source) = sources.next() else {
            return false;
        };
        if sources.next().is_some() || source.role != SemanticRole::Link || !ids.insert(source.id) {
            return false;
        }
        let SemanticOwnedReadContent::Text(text) = &source.content else {
            return false;
        };
        if text != item.as_str() {
            return false;
        }
        english |= text.contains("English");
        german |= text.contains("Deutsch");
    }
    english && german
}
