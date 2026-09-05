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
    use super::ProbeFailure as Error;
    let started = Instant::now();
    let (profile, input, task) = super::work_actor::input(started)?;
    let data = tempfile::tempdir().map_err(|_| Error::Runtime)?;
    let store =
        Arc::new(zephium_store::SqliteStore::open(data.path()).map_err(|_| Error::Runtime)?);
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
        let composition = MacosWorkComposition::new(engine, store);
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
            if shutdown.is_none()
                && (matches!(
                    snapshot.phase,
                    AgentWorkApplicationPhase::Succeeded
                        | AgentWorkApplicationPhase::Recovery
                        | AgentWorkApplicationPhase::NeedsReview
                        | AgentWorkApplicationPhase::PersistenceUncertain
                ) || native_failed
                    || failed.load(Ordering::Acquire))
            {
                terminal_success = snapshot.phase == AgentWorkApplicationPhase::Succeeded
                    && snapshot.failure.is_none()
                    && view.records().iter().any(|record| {
                        record.disposition() == zephium_agentic::AgentWorkDisposition::Succeeded
                    });
                let _ = writeln!(std::io::stdout().lock(), "work-application-terminal: phase={:?}; failure={:?}; persistence={:?}; durable_success={terminal_success}; content=redacted", snapshot.phase, snapshot.failure, snapshot.persistence_failure);
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
                let _ = writeln!(std::io::stdout().lock(), "work-application-closure: model=gpt-5.6-luna; durable_success={terminal_success}; shell_clean={clean}; turns={turns}; verified_effects={effects}; input_tokens={tokens_in}; output_tokens={tokens_out}; cost_micro_usd={cost}; elapsed_ms={}; content=redacted", started.elapsed().as_millis());
                return Some(
                    if clean && terminal_success && !failed.load(Ordering::Acquire) {
                        Ok(())
                    } else {
                        Err("application_closure")
                    },
                );
            }
            None
        }))
    });
    result.map_err(Error::Engine)?;
    writeln!(std::io::stdout().lock(), "work-application-qualified: focus_isolation=passed; teardown=application_owned; elapsed_ms={}; content=redacted", started.elapsed().as_millis()).map_err(|_| Error::Output)
}
