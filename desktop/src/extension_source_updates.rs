//! Transport composition only. Shell's existing heartbeat schedules checks;
//! this module starts no polling task and retains no strong Shell/App handle.
use super::*;
use zephium_app::{StoreExtensionUpdateDispatch, StoreExtensionUpdateResult as ResultKind};

pub(super) fn configure(shell: &Handle, store: &Arc<SqliteStore>, shutdown: &ShutdownCoordinator) {
    let mut seed = [0u8; 8];
    if getrandom::fill(&mut seed).is_err() {
        write_diagnostic(format_args!(
            "extensions: automatic update jitter unavailable"
        ));
        return;
    }
    let callback = shell.callback_handle();
    let store = Arc::downgrade(store);
    let shutdown = Arc::clone(&shutdown.terminal_started);
    let registration = callback.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let saved = store
            .upgrade()
            .and_then(|store| store.app_setting("extensions.source-update-schedule.v1"));
        let dispatch = StoreExtensionUpdateDispatch::new(
            u64::from_le_bytes(seed),
            saved,
            move |context, done| {
                if shutdown.load(Ordering::Acquire)
                    || STORE_EXTENSION_INSTALL_IN_FLIGHT
                        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                        .is_err()
                {
                    done(ResultKind::RetryLater);
                    return;
                }
                let busy = AtomicFlagReset(&STORE_EXTENSION_INSTALL_IN_FLIGHT);
                let callback = callback.clone();
                let store = store.clone();
                let shutdown = Arc::clone(&shutdown);
                tauri::async_runtime::spawn(async move {
                    let _busy = busy;
                    // Crash/restart cadence is durable before making any request. The
                    // Store barrier runs off Shell/UI; failure means no network work.
                    let flushed = tauri::async_runtime::spawn_blocking(move || {
                        store.upgrade().is_some_and(|store| {
                            store.flush_until(
                                std::time::Instant::now() + std::time::Duration::from_secs(2),
                            )
                        })
                    })
                    .await
                    .unwrap_or(false);
                    if !flushed || shutdown.load(Ordering::Acquire) {
                        done(ResultKind::RetryLater);
                        return;
                    }
                    let result = download_store_selection(&shutdown, &callback, context).await;
                    done(classify(result));
                });
            },
        );
        let _ = registration.dispatch(Command::ConfigureStoreExtensionUpdates(dispatch));
    });
}
fn classify(result: Result<StoreSelectionDownload, String>) -> ResultKind {
    use zephium_core::ports::extensions::{
        ExtensionStorePackagePreparationOutcome as Outcome, ExtensionUpdateOutcome as Update,
    };
    match result {
        Ok(StoreSelectionDownload::NoNewerVersionOffered) => ResultKind::NoChange,
        Ok(StoreSelectionDownload::Prepared { outcome, .. }) => match outcome {
            Outcome::UpToDate => ResultKind::NoChange,
            Outcome::UpdateAvailable => ResultKind::ReviewRequired,
            Outcome::UpdateSettled(settlement) => match settlement.outcome() {
                Update::Updated { .. } => ResultKind::Updated,
                Update::Rejected => ResultKind::Skipped,
                Update::Conflict | Update::Unavailable => ResultKind::RetryLater,
                Update::OutcomeUnknown | Update::FailedClosed => ResultKind::FailedClosed,
            },
            Outcome::Unsupported(_) | Outcome::InvalidPackage | Outcome::StorageLimit => {
                ResultKind::Skipped
            }
            Outcome::FailedClosed => ResultKind::FailedClosed,
            Outcome::Unavailable | Outcome::Prepared(_) | Outcome::AlreadyInstalled => {
                ResultKind::RetryLater
            }
        },
        Err(_) => ResultKind::RetryLater,
    }
}
