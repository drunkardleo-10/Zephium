use super::*;

fn wait_for_actor_condition(timeout: std::time::Duration, condition: impl Fn() -> bool) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    condition()
}

fn extension_management_catalog(
    profile: ProfileId,
    install: zephium_core::ids::ExtensionInstallId,
    catalog_revision: zephium_core::extensions::ExtensionInstallCatalogRevision,
    install_revision: zephium_core::extensions::ExtensionInstallRevision,
    runtime: zephium_core::ports::extensions::ExtensionManagementRuntimeState,
) -> zephium_core::ports::extensions::ExtensionManagementCatalog {
    let selector = zephium_core::ports::extensions::ExtensionInstallSelector::new(
        profile,
        install,
        catalog_revision,
        install_revision,
    );
    let entry = zephium_core::ports::extensions::ExtensionManagementEntry::new(
        selector,
        "Fixture extension",
        Some("Authenticated fixture metadata".into()),
        Some("Zephium tests".into()),
        "1.0.0",
        true,
        zephium_core::ports::extensions::ExtensionManagementSource::ZephiumVerified,
        Some(1),
        Some(
            zephium_core::ports::extensions::ExtensionManagementProvenance::new(
                "https://example.com/releases/fixture",
                "1.0.0",
                "MIT",
                "Example contributors",
            )
            .unwrap(),
        ),
        runtime,
        zephium_core::ports::extensions::ExtensionManagementGrantState::Uninitialized,
        zephium_core::ports::extensions::ExtensionManagementCompatibility::Compatible,
        Vec::new(),
    )
    .expect("fixture management row must be valid");
    zephium_core::ports::extensions::ExtensionManagementCatalog::new(
        profile,
        catalog_revision,
        vec![entry],
    )
    .expect("fixture management catalog must be valid")
}

fn empty_extension_management_catalog(
    profile: ProfileId,
    catalog_revision: zephium_core::extensions::ExtensionInstallCatalogRevision,
) -> zephium_core::ports::extensions::ExtensionManagementCatalog {
    zephium_core::ports::extensions::ExtensionManagementCatalog::new(
        profile,
        catalog_revision,
        Vec::new(),
    )
    .expect("empty fixture management catalog must be valid")
}

fn extension_install_candidate_catalog(
    profile: ProfileId,
    catalog_revision: zephium_core::extensions::ExtensionInstallCatalogRevision,
    file_access_available: bool,
    private_access_available: bool,
) -> (
    zephium_core::ports::extensions::ExtensionManagementCatalog,
    zephium_core::extensions::ExtensionPackageIdentity,
) {
    use zephium_core::extensions::{
        ExtensionAuthorityId, ExtensionCatalogSetDigest, ExtensionManifestDigest,
        ExtensionPackageIdentity, ExtensionPackageKey, ExtensionPackagePayloadIdentity,
        ExtensionPackageRevision, ExtensionTreeDigest,
    };
    let package = ExtensionPackageIdentity::new(
        ExtensionAuthorityId::from_bytes([11; 32]),
        ExtensionPackageKey::from_bytes([12; 32]),
        ExtensionPackageRevision::INITIAL,
        ExtensionPackagePayloadIdentity::BundledTree,
        ExtensionManifestDigest::from_bytes([13; 32]),
        ExtensionTreeDigest::from_bytes([14; 32]),
    );
    let selector = zephium_core::ports::extensions::ExtensionInstallCandidateSelector::new(
        profile,
        catalog_revision,
        ExtensionCatalogSetDigest::from_bytes([15; 32]),
        package.clone(),
    );
    let candidate = zephium_core::ports::extensions::ExtensionInstallCandidateEntry::new(
        selector,
        "Fixture candidate",
        Some("Authenticated candidate metadata".into()),
        Some("Zephium tests".into()),
        "1.0.0",
        zephium_core::ports::extensions::ExtensionManagementSource::ExternalCompatibility,
        None,
        Some(
            zephium_core::ports::extensions::ExtensionManagementProvenance::new(
                "https://example.com/store/fixture",
                "1.0.0",
                "MIT",
                "External publisher",
            )
            .unwrap(),
        ),
        vec!["storage".into(), "webRequest".into()],
        vec!["<all_urls>".into()],
        vec!["notifications".into(), "tabs".into()],
        vec!["https://optional.example/*".into()],
        file_access_available,
        private_access_available,
        zephium_core::ports::extensions::ExtensionManagementCompatibility::Degraded,
        vec![
            zephium_core::ports::extensions::ExtensionManagementLimitation::api_permission(
                "webRequest",
            )
            .expect("fixture limitation must be valid"),
        ],
    )
    .expect("fixture candidate must be valid");
    (
        zephium_core::ports::extensions::ExtensionManagementCatalog::with_candidates(
            profile,
            catalog_revision,
            Vec::new(),
            vec![candidate],
        )
        .expect("fixture candidate catalog must be valid"),
        package,
    )
}

fn settle_next_management_catalog(
    state: &FakeExtensionLifecycleState,
    catalog: zephium_core::ports::extensions::ExtensionManagementCatalog,
) {
    assert!(wait_for_actor_condition(
        std::time::Duration::from_secs(2),
        || state.management_catalog_callbacks.lock().unwrap().len() == 1
    ));
    state
        .management_catalog_callbacks
        .lock()
        .unwrap()
        .pop()
        .expect("accepted catalog read must retain its callback")(
        zephium_core::ports::extensions::ExtensionManagementCatalogOutcome::Loaded(catalog),
    );
}

fn wait_for_ready_management_catalog(
    rx: &std::sync::mpsc::Receiver<Projection>,
    expected_profile: ProfileId,
    expected_catalog: zephium_core::extensions::ExtensionInstallCatalogRevision,
    expected_entries: usize,
) -> zephium_ipc::ExtensionManagementView {
    let ready = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
        .find_map(|projection| match projection {
            Projection::ExtensionManagement(view)
                if view.phase == zephium_ipc::ExtensionManagementPhase::Ready =>
            {
                Some(view)
            }
            _ => None,
        })
        .expect("catalog settlement must publish a ready management projection");
    assert_eq!(ready.profile_id, expected_profile.to_string());
    assert_eq!(
        ready.catalog_revision.as_deref(),
        Some(format!("{:016x}", expected_catalog.get()).as_str())
    );
    assert_eq!(ready.entries.len(), expected_entries);
    ready
}

struct ActorExitBlocker {
    order: Arc<Mutex<Vec<&'static str>>>,
    maintain_calls: Arc<std::sync::atomic::AtomicUsize>,
    shutdown_calls: Arc<std::sync::atomic::AtomicUsize>,
    panic_on_maintain: bool,
}

impl BlockerCompiler for ActorExitBlocker {
    fn compile(
        &self,
        profile: ProfileId,
        generation: ContentPolicyGeneration,
        config: BlockerConfig,
        done: Box<dyn FnOnce(BlockerCompileOutcome) + Send>,
    ) -> BlockerDispatch {
        ImmediateAllowAllCompiler.compile(profile, generation, config, done)
    }

    fn retire_profile(
        &self,
        profile: ProfileId,
        done: Box<dyn FnOnce() + Send>,
    ) -> BlockerRetirementDispatch {
        ImmediateAllowAllCompiler.retire_profile(profile, done)
    }

    fn shutdown_until(&self, _deadline: std::time::Instant) -> BlockerShutdownOutcome {
        self.shutdown_calls
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.order.lock().unwrap().push("blocker");
        BlockerShutdownOutcome::Clean
    }
}

impl BlockerCatalog for ActorExitBlocker {
    fn maintain(&self) -> BlockerCatalogSnapshot {
        self.maintain_calls
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        assert!(
            !self.panic_on_maintain,
            "injected initial blocker-catalog panic"
        );
        test_catalog_snapshot()
    }

    fn request_refresh(&self) -> BlockerCatalogRefreshDispatch {
        BlockerCatalogRefreshDispatch::Busy
    }
}

#[test]
fn actor_panic_terminalizes_pending_and_later_shutdown_requests() {
    let store = Arc::new(FakeStore::default());
    let order = Arc::new(Mutex::new(Vec::new()));
    *store.barrier_order.lock().unwrap() = Some(Arc::clone(&order));
    *store.shutdown_outcome.lock().unwrap() = Some(StoreShutdownOutcome::Clean);
    store
        .panic_on_load
        .store(true, std::sync::atomic::Ordering::Release);
    let engine = Arc::new(FakeEngine::default());
    *engine.shutdown_order.lock().unwrap() = Some(Arc::clone(&order));
    let blocker_shutdown_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let blocker_maintain_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let blocker = Arc::new(ActorExitBlocker {
        order: Arc::clone(&order),
        maintain_calls: blocker_maintain_calls,
        shutdown_calls: Arc::clone(&blocker_shutdown_calls),
        panic_on_maintain: false,
    });
    let (extension_service, extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    *extension_state.shutdown_order.lock().unwrap() = Some(Arc::clone(&order));
    let (terminal_tx, terminal_rx) = sync_channel(1);
    let handle = spawn(
        engine.clone(),
        store.clone(),
        blocker,
        extension_service,
        Box::new(move |failure| {
            let _ = terminal_tx.send(failure);
        }),
        Arc::new(FakeChrome),
        Box::new(|_| {}),
    )
    .expect("spawn test shell");

    assert!(handle.dispatch(Command::Bootstrap));
    let pending = handle.shutdown();
    assert_eq!(
        pending
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap(),
        ShutdownOutcome::Unclean
    );
    assert_eq!(handle.shutdown().recv().unwrap(), ShutdownOutcome::Unclean);
    assert_eq!(
        terminal_rx.recv().unwrap(),
        ShellTerminalFailure::ActorExitedUnexpectedly
    );
    assert_eq!(
        extension_state
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1,
        "actor unwind must explicitly consume the extension-service owner"
    );
    assert!(!extension_state
        .dropped_without_shutdown
        .load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(
        store
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        engine
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        blocker_shutdown_calls.load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        order.lock().unwrap().as_slice(),
        &["extensions", "store-final", "engine", "blocker"]
    );
}

#[test]
fn initial_blocker_catalog_panic_keeps_full_shell_under_terminal_cleanup() {
    let store = Arc::new(FakeStore::default());
    let order = Arc::new(Mutex::new(Vec::new()));
    *store.barrier_order.lock().unwrap() = Some(Arc::clone(&order));
    *store.shutdown_outcome.lock().unwrap() = Some(StoreShutdownOutcome::Clean);
    let engine = Arc::new(FakeEngine::default());
    *engine.shutdown_order.lock().unwrap() = Some(Arc::clone(&order));
    let blocker_shutdown_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let blocker_maintain_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let blocker = Arc::new(ActorExitBlocker {
        order: Arc::clone(&order),
        maintain_calls: blocker_maintain_calls,
        shutdown_calls: Arc::clone(&blocker_shutdown_calls),
        panic_on_maintain: true,
    });
    let (extension_service, extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    *extension_state.shutdown_order.lock().unwrap() = Some(Arc::clone(&order));
    let callback_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let callback_calls_for_actor = Arc::clone(&callback_calls);
    let (terminal_tx, terminal_rx) = sync_channel(1);

    let handle = spawn(
        engine.clone(),
        store.clone(),
        blocker,
        extension_service,
        Box::new(move |failure| {
            callback_calls_for_actor.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            let _ = terminal_tx.send(failure);
        }),
        Arc::new(FakeChrome),
        Box::new(|_| {}),
    )
    .expect("worker construction and ownership handoff must succeed");

    assert_eq!(
        terminal_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("guarded construction panic must reach the terminal callback"),
        ShellTerminalFailure::ActorExitedUnexpectedly
    );
    assert_eq!(handle.shutdown().recv().unwrap(), ShutdownOutcome::Unclean);
    assert_eq!(callback_calls.load(std::sync::atomic::Ordering::Acquire), 1);
    assert!(terminal_rx.try_recv().is_err());
    assert_eq!(
        extension_state
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert!(!extension_state
        .dropped_without_shutdown
        .load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(
        store
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        engine
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        blocker_shutdown_calls.load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        order.lock().unwrap().as_slice(),
        &["extensions", "store-final", "engine", "blocker"]
    );
}

#[test]
fn suspended_actor_enters_no_external_port_before_composition_admission() {
    let store = Arc::new(FakeStore::default());
    *store.shutdown_outcome.lock().unwrap() = Some(StoreShutdownOutcome::Clean);
    let engine = Arc::new(FakeEngine::default());
    let order = Arc::new(Mutex::new(Vec::new()));
    *store.barrier_order.lock().unwrap() = Some(Arc::clone(&order));
    *engine.shutdown_order.lock().unwrap() = Some(Arc::clone(&order));
    let maintain_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let blocker_shutdown_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let blocker = Arc::new(ActorExitBlocker {
        order: Arc::clone(&order),
        maintain_calls: Arc::clone(&maintain_calls),
        shutdown_calls: Arc::clone(&blocker_shutdown_calls),
        panic_on_maintain: true,
    });
    let (extension_service, extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    *extension_state.shutdown_order.lock().unwrap() = Some(Arc::clone(&order));
    let published = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let callback_observed_publication = Arc::clone(&published);
    let callback_order = Arc::clone(&order);
    let (terminal_tx, terminal_rx) = sync_channel(1);

    let handle = crate::spawn_suspended(
        engine.clone(),
        store.clone(),
        blocker,
        extension_service,
        Box::new(move |failure| {
            assert!(
                callback_observed_publication.load(std::sync::atomic::Ordering::Acquire),
                "terminal callback must not run before desktop publishes ownership"
            );
            callback_order.lock().unwrap().push("terminal");
            let _ = terminal_tx.send(failure);
        }),
        Arc::new(FakeChrome),
        Box::new(|_| {}),
    )
    .expect("suspended Shell construction");

    assert!(handle.wait_until_startup_suspended(
        std::time::Instant::now() + std::time::Duration::from_secs(2)
    ));
    assert_eq!(maintain_calls.load(std::sync::atomic::Ordering::Acquire), 0);
    assert!(order.lock().unwrap().is_empty());

    // Models desktop publishing the Handle and clearing all temporary owners.
    published.store(true, std::sync::atomic::Ordering::Release);
    assert!(handle.admit_startup());
    assert!(!handle.admit_startup(), "admission is exactly once");
    assert_eq!(
        terminal_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("guarded startup panic reaches the published owner"),
        ShellTerminalFailure::ActorExitedUnexpectedly
    );
    assert_eq!(handle.shutdown().recv().unwrap(), ShutdownOutcome::Unclean);

    assert_eq!(maintain_calls.load(std::sync::atomic::Ordering::Acquire), 1);
    assert_eq!(
        extension_state
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        store
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        engine
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        blocker_shutdown_calls.load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        order.lock().unwrap().as_slice(),
        &["terminal", "extensions", "store-final", "engine", "blocker"]
    );
}

#[test]
fn terminal_barrier_cancels_suspended_startup_without_external_admission() {
    let store = Arc::new(FakeStore::default());
    *store.shutdown_outcome.lock().unwrap() = Some(StoreShutdownOutcome::Clean);
    let engine = Arc::new(FakeEngine::default());
    let order = Arc::new(Mutex::new(Vec::new()));
    *store.barrier_order.lock().unwrap() = Some(Arc::clone(&order));
    *engine.shutdown_order.lock().unwrap() = Some(Arc::clone(&order));
    let maintain_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let blocker_shutdown_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let blocker = Arc::new(ActorExitBlocker {
        order: Arc::clone(&order),
        maintain_calls: Arc::clone(&maintain_calls),
        shutdown_calls: Arc::clone(&blocker_shutdown_calls),
        panic_on_maintain: false,
    });
    let (extension_service, extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    *extension_state.shutdown_order.lock().unwrap() = Some(Arc::clone(&order));
    let terminal_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let terminal_calls_for_actor = Arc::clone(&terminal_calls);
    let handle = crate::spawn_suspended(
        engine.clone(),
        store.clone(),
        blocker,
        extension_service,
        Box::new(move |_| {
            terminal_calls_for_actor.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }),
        Arc::new(FakeChrome),
        Box::new(|_| {}),
    )
    .expect("suspended Shell construction");

    assert!(handle.wait_until_startup_suspended(
        std::time::Instant::now() + std::time::Duration::from_secs(2)
    ));
    assert_eq!(handle.shutdown().recv().unwrap(), ShutdownOutcome::Clean);
    assert_eq!(maintain_calls.load(std::sync::atomic::Ordering::Acquire), 0);
    assert_eq!(terminal_calls.load(std::sync::atomic::Ordering::Acquire), 0);
    assert_eq!(
        extension_state
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        store
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        engine
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        blocker_shutdown_calls.load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        order.lock().unwrap().as_slice(),
        &[
            "store-preflight",
            "extensions",
            "store-final",
            "engine",
            "blocker",
        ]
    );
}

#[test]
fn spawned_actor_processes_dispatched_commands() {
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = spawn(
        Arc::new(FakeEngine::default()),
        Arc::new(FakeStore::default()),
        Arc::new(ImmediateAllowAllCompiler),
        clean_extension_lifecycle(),
        Box::new(|_| {}),
        Arc::new(FakeChrome),
        Box::new(move |s| {
            let _ = tx.send(s);
        }),
    )
    .expect("spawn test shell");
    handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0)));
    handle.dispatch(Command::Bootstrap);
    let projection = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
        .find(|p| matches!(p, Projection::Items(_)))
        .expect("bootstrap must project an items snapshot");
    let Projection::Items(s) = projection else {
        unreachable!()
    };
    assert!(s.active.is_some());
}

#[test]
fn slow_history_sqlite_read_never_blocks_shell_coordination() {
    let store = Arc::new(FakeStore::default());
    store
        .history_delay_ms
        .store(500, std::sync::atomic::Ordering::Release);
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = spawn(
        Arc::new(FakeEngine::default()),
        store,
        Arc::new(ImmediateAllowAllCompiler),
        clean_extension_lifecycle(),
        Box::new(|_| {}),
        Arc::new(FakeChrome),
        Box::new(move |projection| {
            let _ = tx.send(projection);
        }),
    )
    .expect("spawn test shell");
    assert!(handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0))));
    assert!(handle.dispatch(Command::Bootstrap));
    assert!(
        std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
            .any(|projection| matches!(projection, Projection::Items(_)))
    );

    let started = std::time::Instant::now();
    assert!(handle.dispatch(Command::Search("slow".into())));
    assert!(handle.dispatch(Command::Open));
    let opened = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_millis(250)).ok())
        .find_map(|projection| match projection {
            Projection::Items(items) if items.tabs.len() == 2 => Some(items),
            _ => None,
        });
    assert!(
        opened.is_some(),
        "shell actor stalled behind SQLite history"
    );
    assert!(started.elapsed() < std::time::Duration::from_millis(400));
}

#[test]
fn tracked_operation_has_exact_admission_and_actor_disposition_id() {
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = spawn(
        Arc::new(FakeEngine::default()),
        Arc::new(FakeStore::default()),
        Arc::new(ImmediateAllowAllCompiler),
        clean_extension_lifecycle(),
        Box::new(|_| {}),
        Arc::new(FakeChrome),
        Box::new(move |projection| {
            let _ = tx.send(projection);
        }),
    )
    .expect("spawn test shell");
    assert!(handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0))));
    assert!(handle.dispatch(Command::Bootstrap));
    assert!(handle.dispatch_operation("operation-7".into(), Command::Open));

    let completion = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
        .find_map(|projection| match projection {
            Projection::OperationProcessed(completion) => Some(completion),
            _ => None,
        })
        .expect("admitted operation must complete in actor order");
    assert_eq!(completion.operation_id, "operation-7");
    assert_eq!(completion.outcome, OperationOutcome::Deferred);
    assert_eq!(completion.reason, OperationReason::NativeWorkPending);
    assert!(!handle.dispatch_operation(String::new(), Command::Open));
    assert!(!handle.dispatch_operation(
        "nested".into(),
        Command::Operation {
            operation_id: "inner".into(),
            command: Box::new(Command::Open),
        },
    ));
}

#[test]
fn extension_install_uses_only_the_retained_authenticated_candidate() {
    let (tx, rx) = std::sync::mpsc::channel();
    let engine = Arc::new(FakeEngine::default());
    let (extension_service, extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    let handle = spawn(
        engine.clone(),
        Arc::new(FakeStore::default()),
        Arc::new(ImmediateAllowAllCompiler),
        extension_service,
        Box::new(|_| {}),
        Arc::new(FakeChrome),
        Box::new(move |projection| {
            let _ = tx.send(projection);
        }),
    )
    .expect("spawn test shell");
    assert!(handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0))));
    assert!(handle.dispatch(Command::Bootstrap));
    let profile = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
        .find_map(|projection| match projection {
            Projection::Items(items) => items.profile.map(|profile| profile.id),
            _ => None,
        })
        .and_then(|profile| ProfileId::parse(&profile))
        .expect("bootstrap must publish the focused profile");
    let _ = rx.try_iter().count();

    let catalog_revision = zephium_core::extensions::ExtensionInstallCatalogRevision::INITIAL;
    let (catalog, package) =
        extension_install_candidate_catalog(profile, catalog_revision, true, true);
    assert!(handle.dispatch(Command::SetExtensionManagementVisible(true)));
    settle_next_management_catalog(&extension_state, catalog);
    let ready = wait_for_ready_management_catalog(&rx, profile, catalog_revision, 0);
    assert_eq!(ready.candidates.len(), 1);
    assert_eq!(ready.candidates[0].candidate_index, 0);
    assert_eq!(
        ready.candidates[0].source,
        zephium_ipc::ExtensionManagementSourceView::ExternalCompatibility
    );
    assert_eq!(ready.candidates[0].verified_catalog_unix, None);
    assert_eq!(
        ready.candidates[0]
            .provenance
            .as_ref()
            .map(|value| value.source_url.as_str()),
        Some("https://example.com/store/fixture")
    );
    assert!(ready.candidates[0].supports_file_access);
    assert!(ready.candidates[0].file_access_available);
    assert!(ready.candidates[0].private_access_available);
    assert_eq!(ready.candidates[0].required_api, ["storage", "webRequest"]);
    assert_eq!(ready.candidates[0].required_hosts, ["<all_urls>"]);
    assert_eq!(ready.candidates[0].optional_api, ["notifications", "tabs"]);
    assert_eq!(
        ready.candidates[0].limitations,
        [
            zephium_ipc::ExtensionManagementLimitationView::ApiPermission {
                name: "webRequest".to_owned(),
            }
        ]
    );
    assert_eq!(
        ready.candidates[0].optional_hosts,
        ["https://optional.example/*"]
    );

    assert!(handle.dispatch_operation(
        "stale-extension-install".into(),
        Command::InstallFocusedExtension {
            candidate_index: 0,
            expected_catalog: catalog_revision.next().unwrap(),
            optional_api_indices: Vec::new(),
            optional_host_indices: Vec::new(),
            file_access: false,
            private_access: false,
        },
    ));
    let stale = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
        .find_map(|projection| match projection {
            Projection::OperationProcessed(completion)
                if completion.operation_id == "stale-extension-install" =>
            {
                Some(completion)
            }
            _ => None,
        })
        .expect("stale candidate must settle immediately");
    assert_eq!(stale.outcome, OperationOutcome::Rejected);
    assert_eq!(stale.reason, OperationReason::StoreConflict);
    assert!(extension_state.install_calls.lock().unwrap().is_empty());

    assert!(handle.dispatch_operation(
        "invalid-extension-grants".into(),
        Command::InstallFocusedExtension {
            candidate_index: ready.candidates[0].candidate_index,
            expected_catalog: catalog_revision,
            optional_api_indices: vec![2],
            optional_host_indices: vec![0, 0],
            file_access: false,
            private_access: false,
        },
    ));
    let invalid = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
        .find_map(|projection| match projection {
            Projection::OperationProcessed(completion)
                if completion.operation_id == "invalid-extension-grants" =>
            {
                Some(completion)
            }
            _ => None,
        })
        .expect("invalid optional selection must settle immediately");
    assert_eq!(invalid.outcome, OperationOutcome::Rejected);
    assert_eq!(invalid.reason, OperationReason::StoreConflict);
    assert!(extension_state.install_calls.lock().unwrap().is_empty());

    assert!(handle.dispatch_operation(
        "extension-install".into(),
        Command::InstallFocusedExtension {
            candidate_index: ready.candidates[0].candidate_index,
            expected_catalog: catalog_revision,
            optional_api_indices: vec![1, 0],
            optional_host_indices: vec![0],
            file_access: true,
            private_access: true,
        },
    ));
    assert!(wait_for_actor_condition(
        std::time::Duration::from_secs(2),
        || extension_state.install_callbacks.lock().unwrap().len() == 1
    ));
    assert!(rx
        .try_iter()
        .all(|projection| !matches!(projection, Projection::OperationProcessed(_))));
    let install_calls = extension_state.install_calls.lock().unwrap();
    let (selector, selection, deadline) = &install_calls[0];
    assert_eq!(selector.profile(), profile);
    assert_eq!(selector.expected_catalog_revision(), catalog_revision);
    assert_eq!(selector.package(), &package);
    assert_eq!(selection.optional_api_indices(), [0, 1]);
    assert_eq!(selection.optional_host_indices(), [0]);
    assert!(selection.file_access());
    assert!(selection.private_access());
    assert!(*deadline > std::time::Instant::now());
    drop(install_calls);

    let install = zephium_core::ids::ExtensionInstallId::from(93_000);
    let mut active_profiles = zephium_core::ports::extensions::ExtensionActiveProfiles::EMPTY;
    assert!(active_profiles.try_insert(profile));
    extension_state
        .install_callbacks
        .lock()
        .unwrap()
        .pop()
        .unwrap()(
        zephium_core::ports::extensions::ExtensionManagementSettlement::new(
            zephium_core::ports::extensions::ExtensionInstallOutcome::Installed {
                install,
                runtime: zephium_core::ports::extensions::ExtensionInstalledRuntimeState::Active(
                    zephium_core::extensions::ExtensionRuntimeGeneration::INITIAL,
                ),
            },
            Some(active_profiles),
        ),
    );
    let installed = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
        .find_map(|projection| match projection {
            Projection::OperationProcessed(completion)
                if completion.operation_id == "extension-install" =>
            {
                Some(completion)
            }
            _ => None,
        })
        .expect("install settlement must complete the original operation");
    assert_eq!(installed.outcome, OperationOutcome::Applied);
    assert_eq!(installed.reason, OperationReason::MutationApplied);
    assert!(engine
        .extension_browser_surfaces()
        .last()
        .is_some_and(|surface| !surface.windows().is_empty()));

    settle_next_management_catalog(
        &extension_state,
        extension_management_catalog(
            profile,
            install,
            catalog_revision.next().unwrap().next().unwrap(),
            zephium_core::extensions::ExtensionInstallRevision::INITIAL
                .next()
                .unwrap(),
            zephium_core::ports::extensions::ExtensionManagementRuntimeState::Active(
                zephium_core::extensions::ExtensionRuntimeGeneration::INITIAL,
            ),
        ),
    );
    let _ = wait_for_ready_management_catalog(
        &rx,
        profile,
        catalog_revision.next().unwrap().next().unwrap(),
        1,
    );
    let options_catalog = catalog_revision.next().unwrap().next().unwrap();
    let options_install = zephium_core::extensions::ExtensionInstallRevision::INITIAL
        .next()
        .unwrap();
    assert!(handle.dispatch(Command::OpenFocusedExtensionOptions {
        install,
        expected_catalog: options_catalog,
        expected_install: options_install,
    }));
    assert!(wait_for_actor_condition(
        std::time::Duration::from_secs(2),
        || engine.extension_options_requests().len() == 1
    ));
    assert_eq!(
        engine.extension_options_requests(),
        [zephium_core::extensions::ExtensionRuntimeInstance::new(
            profile,
            install,
            zephium_core::extensions::ExtensionRuntimeGeneration::INITIAL,
        )]
    );

    assert_eq!(
        handle
            .shutdown()
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap(),
        ShutdownOutcome::Clean
    );
}

#[test]
fn unavailable_file_and_private_grants_are_rejected_before_service_admission() {
    let (tx, rx) = std::sync::mpsc::channel();
    let (extension_service, extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    let handle = spawn(
        Arc::new(FakeEngine::default()),
        Arc::new(FakeStore::default()),
        Arc::new(ImmediateAllowAllCompiler),
        extension_service,
        Box::new(|_| {}),
        Arc::new(FakeChrome),
        Box::new(move |projection| {
            let _ = tx.send(projection);
        }),
    )
    .expect("spawn test shell");
    assert!(handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0))));
    assert!(handle.dispatch(Command::Bootstrap));
    let profile = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
        .find_map(|projection| match projection {
            Projection::Items(items) => items.profile.map(|profile| profile.id),
            _ => None,
        })
        .and_then(|profile| ProfileId::parse(&profile))
        .expect("bootstrap must publish the focused profile");
    let _ = rx.try_iter().count();
    let catalog_revision = zephium_core::extensions::ExtensionInstallCatalogRevision::INITIAL;
    let (catalog, _) = extension_install_candidate_catalog(profile, catalog_revision, false, false);
    assert!(handle.dispatch(Command::SetExtensionManagementVisible(true)));
    settle_next_management_catalog(&extension_state, catalog);
    let ready = wait_for_ready_management_catalog(&rx, profile, catalog_revision, 0);
    assert!(!ready.candidates[0].file_access_available);
    assert!(!ready.candidates[0].private_access_available);

    for (operation_id, file_access, private_access) in [
        ("unavailable-file-access", true, false),
        ("unavailable-private-access", false, true),
    ] {
        assert!(handle.dispatch_operation(
            operation_id.into(),
            Command::InstallFocusedExtension {
                candidate_index: 0,
                expected_catalog: catalog_revision,
                optional_api_indices: Vec::new(),
                optional_host_indices: Vec::new(),
                file_access,
                private_access,
            },
        ));
        let rejected =
            std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
                .find_map(|projection| match projection {
                    Projection::OperationProcessed(completion)
                        if completion.operation_id == operation_id =>
                    {
                        Some(completion)
                    }
                    _ => None,
                })
                .expect("unavailable grant selection must settle immediately");
        assert_eq!(rejected.outcome, OperationOutcome::Rejected);
        assert_eq!(rejected.reason, OperationReason::StoreConflict);
    }
    assert!(extension_state.install_calls.lock().unwrap().is_empty());
    assert_eq!(
        handle
            .shutdown()
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap(),
        ShutdownOutcome::Clean
    );
}

#[test]
fn extension_management_stays_pending_until_serialized_service_settlement() {
    let (tx, rx) = std::sync::mpsc::channel();
    let engine = Arc::new(FakeEngine::default());
    let (extension_service, extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    let handle = spawn(
        engine.clone(),
        Arc::new(FakeStore::default()),
        Arc::new(ImmediateAllowAllCompiler),
        extension_service,
        Box::new(|_| {}),
        Arc::new(FakeChrome),
        Box::new(move |projection| {
            let _ = tx.send(projection);
        }),
    )
    .expect("spawn test shell");
    assert!(handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0))));
    assert!(handle.dispatch(Command::Bootstrap));
    let profile = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
        .find_map(|projection| match projection {
            Projection::Items(items) => items.profile.map(|profile| profile.id),
            _ => None,
        })
        .and_then(|profile| ProfileId::parse(&profile))
        .expect("bootstrap must publish the focused profile");
    let _ = rx.try_iter().count();

    let install = zephium_core::ids::ExtensionInstallId::from(91_000);
    assert!(handle.dispatch(Command::SetExtensionManagementVisible(true)));
    settle_next_management_catalog(
        &extension_state,
        extension_management_catalog(
            profile,
            install,
            zephium_core::extensions::ExtensionInstallCatalogRevision::INITIAL,
            zephium_core::extensions::ExtensionInstallRevision::INITIAL,
            zephium_core::ports::extensions::ExtensionManagementRuntimeState::Disabled,
        ),
    );
    wait_for_ready_management_catalog(
        &rx,
        profile,
        zephium_core::extensions::ExtensionInstallCatalogRevision::INITIAL,
        1,
    );
    assert!(handle.dispatch_operation(
        "extension-enable".into(),
        Command::SetFocusedExtensionEnabled {
            install,
            expected_catalog: zephium_core::extensions::ExtensionInstallCatalogRevision::INITIAL,
            expected_install: zephium_core::extensions::ExtensionInstallRevision::INITIAL,
            enabled: true,
        },
    ));
    assert!(wait_for_actor_condition(
        std::time::Duration::from_secs(2),
        || extension_state.set_enabled_callbacks.lock().unwrap().len() == 1
    ));
    assert!(rx
        .try_iter()
        .all(|projection| !matches!(projection, Projection::OperationProcessed(_))));
    let (selector, enabled, deadline) = extension_state.set_enabled_calls.lock().unwrap()[0];
    assert_eq!(selector.profile(), profile);
    assert_eq!(selector.install(), install);
    assert!(enabled);
    assert!(deadline > std::time::Instant::now());

    let mut active_profiles = zephium_core::ports::extensions::ExtensionActiveProfiles::EMPTY;
    assert!(active_profiles.try_insert(profile));
    extension_state
        .set_enabled_callbacks
        .lock()
        .unwrap()
        .pop()
        .unwrap()(
        zephium_core::ports::extensions::ExtensionManagementSettlement::new(
            zephium_core::ports::extensions::ExtensionSetEnabledOutcome::Enabled {
                generation: zephium_core::extensions::ExtensionRuntimeGeneration::INITIAL,
                changed: true,
            },
            Some(active_profiles),
        ),
    );
    let enabled = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
        .find_map(|projection| match projection {
            Projection::OperationProcessed(completion)
                if completion.operation_id == "extension-enable" =>
            {
                Some(completion)
            }
            _ => None,
        })
        .expect("service settlement must complete the original operation");
    assert_eq!(enabled.outcome, OperationOutcome::Applied);
    assert_eq!(enabled.reason, OperationReason::MutationApplied);
    assert!(engine
        .extension_browser_surfaces()
        .last()
        .is_some_and(|surface| !surface.windows().is_empty()));

    let expected_catalog = zephium_core::extensions::ExtensionInstallCatalogRevision::INITIAL
        .next()
        .unwrap();
    let expected_install = zephium_core::extensions::ExtensionInstallRevision::INITIAL
        .next()
        .unwrap();
    settle_next_management_catalog(
        &extension_state,
        extension_management_catalog(
            profile,
            install,
            expected_catalog,
            expected_install,
            zephium_core::ports::extensions::ExtensionManagementRuntimeState::Active(
                zephium_core::extensions::ExtensionRuntimeGeneration::INITIAL,
            ),
        ),
    );
    wait_for_ready_management_catalog(&rx, profile, expected_catalog, 1);
    assert!(handle.dispatch_operation(
        "extension-uninstall".into(),
        Command::UninstallFocusedExtension {
            install,
            expected_catalog,
            expected_install,
        },
    ));
    assert!(wait_for_actor_condition(
        std::time::Duration::from_secs(2),
        || extension_state.uninstall_callbacks.lock().unwrap().len() == 1
    ));
    assert!(rx
        .try_iter()
        .all(|projection| !matches!(projection, Projection::OperationProcessed(_))));
    let (selector, deadline) = extension_state.uninstall_calls.lock().unwrap()[0];
    assert_eq!(selector.profile(), profile);
    assert_eq!(selector.install(), install);
    assert_eq!(selector.catalog_revision(), expected_catalog);
    assert_eq!(selector.install_revision(), expected_install);
    assert!(deadline > std::time::Instant::now());

    extension_state
        .uninstall_callbacks
        .lock()
        .unwrap()
        .pop()
        .unwrap()(
        zephium_core::ports::extensions::ExtensionManagementSettlement::new(
            zephium_core::ports::extensions::ExtensionUninstallOutcome::Uninstalled,
            Some(zephium_core::ports::extensions::ExtensionActiveProfiles::EMPTY),
        ),
    );
    let uninstalled =
        std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
            .find_map(|projection| match projection {
                Projection::OperationProcessed(completion)
                    if completion.operation_id == "extension-uninstall" =>
                {
                    Some(completion)
                }
                _ => None,
            })
            .expect("uninstall settlement must complete the original operation");
    assert_eq!(uninstalled.outcome, OperationOutcome::Applied);
    assert_eq!(uninstalled.reason, OperationReason::MutationApplied);
    assert!(engine
        .extension_browser_surfaces()
        .last()
        .is_some_and(|surface| surface.windows().is_empty()));

    let uninstalled_catalog = expected_catalog.next().unwrap();
    settle_next_management_catalog(
        &extension_state,
        empty_extension_management_catalog(profile, uninstalled_catalog),
    );
    wait_for_ready_management_catalog(&rx, profile, uninstalled_catalog, 0);
    assert!(handle.dispatch(Command::SetExtensionManagementVisible(false)));

    assert_eq!(
        handle
            .shutdown()
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap(),
        ShutdownOutcome::Clean
    );
}

#[test]
fn unprovisioned_management_is_truthful_and_never_probes_the_inert_service() {
    let (tx, rx) = std::sync::mpsc::channel();
    let (extension_service, extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    extension_state
        .management_not_configured
        .store(true, std::sync::atomic::Ordering::Release);
    let handle = spawn(
        Arc::new(FakeEngine::default()),
        Arc::new(FakeStore::default()),
        Arc::new(ImmediateAllowAllCompiler),
        extension_service,
        Box::new(|_| {}),
        Arc::new(FakeChrome),
        Box::new(move |projection| {
            let _ = tx.send(projection);
        }),
    )
    .expect("spawn test shell");
    assert!(handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0))));
    assert!(handle.dispatch(Command::Bootstrap));
    let mut startup_availability = None;
    let profile = loop {
        match rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("bootstrap must publish extension availability and focused items")
        {
            Projection::ExtensionManagementAvailability(view) => {
                startup_availability = Some(view.availability);
            }
            Projection::Items(items) => {
                if let Some(profile) = items
                    .profile
                    .and_then(|profile| ProfileId::parse(&profile.id))
                {
                    break profile;
                }
            }
            _ => {}
        }
    };
    assert_eq!(
        startup_availability,
        Some(zephium_ipc::ExtensionManagementAvailabilityView::NotConfigured)
    );
    let _ = rx.try_iter().count();

    assert!(handle.dispatch(Command::SetExtensionManagementVisible(true)));
    let unavailable =
        std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
            .find_map(|projection| match projection {
                Projection::ExtensionManagement(view)
                    if view.phase == zephium_ipc::ExtensionManagementPhase::NotConfigured =>
                {
                    Some(view)
                }
                _ => None,
            })
            .expect("inert product must publish an explicit not-configured phase");
    assert_eq!(unavailable.profile_id, profile.to_string());
    assert!(extension_state
        .management_catalog_calls
        .lock()
        .unwrap()
        .is_empty());
    assert!(extension_state
        .management_catalog_callbacks
        .lock()
        .unwrap()
        .is_empty());

    assert_eq!(
        handle
            .shutdown()
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap(),
        ShutdownOutcome::Clean
    );
}

#[test]
fn ambiguous_extension_management_write_fences_later_writes_until_restart() {
    let (tx, rx) = std::sync::mpsc::channel();
    let (extension_service, extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    let handle = spawn(
        Arc::new(FakeEngine::default()),
        Arc::new(FakeStore::default()),
        Arc::new(ImmediateAllowAllCompiler),
        extension_service,
        Box::new(|_| {}),
        Arc::new(FakeChrome),
        Box::new(move |projection| {
            let _ = tx.send(projection);
        }),
    )
    .expect("spawn test shell");
    assert!(handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0))));
    assert!(handle.dispatch(Command::Bootstrap));
    let profile = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
        .find_map(|projection| match projection {
            Projection::Items(items) => items.profile.map(|profile| profile.id),
            _ => None,
        })
        .and_then(|profile| ProfileId::parse(&profile))
        .expect("bootstrap must publish the focused profile");
    let _ = rx.try_iter().count();

    let install = zephium_core::ids::ExtensionInstallId::from(92_000);
    assert!(handle.dispatch(Command::SetExtensionManagementVisible(true)));
    settle_next_management_catalog(
        &extension_state,
        extension_management_catalog(
            profile,
            install,
            zephium_core::extensions::ExtensionInstallCatalogRevision::INITIAL,
            zephium_core::extensions::ExtensionInstallRevision::INITIAL,
            zephium_core::ports::extensions::ExtensionManagementRuntimeState::PendingActivation,
        ),
    );
    wait_for_ready_management_catalog(
        &rx,
        profile,
        zephium_core::extensions::ExtensionInstallCatalogRevision::INITIAL,
        1,
    );

    assert!(handle.dispatch_operation(
        "extension-unknown".into(),
        Command::SetFocusedExtensionEnabled {
            install,
            expected_catalog: zephium_core::extensions::ExtensionInstallCatalogRevision::INITIAL,
            expected_install: zephium_core::extensions::ExtensionInstallRevision::INITIAL,
            enabled: false,
        },
    ));
    assert!(wait_for_actor_condition(
        std::time::Duration::from_secs(2),
        || extension_state.set_enabled_callbacks.lock().unwrap().len() == 1
    ));
    extension_state
        .set_enabled_callbacks
        .lock()
        .unwrap()
        .pop()
        .unwrap()(
        zephium_core::ports::extensions::ExtensionManagementSettlement::new(
            zephium_core::ports::extensions::ExtensionSetEnabledOutcome::OutcomeUnknown,
            None,
        ),
    );
    let ambiguous = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
        .find_map(|projection| match projection {
            Projection::OperationProcessed(completion)
                if completion.operation_id == "extension-unknown" =>
            {
                Some(completion)
            }
            _ => None,
        })
        .expect("ambiguous write must terminalize its admitted operation");
    assert_eq!(ambiguous.outcome, OperationOutcome::Deferred);
    assert_eq!(ambiguous.reason, OperationReason::StoreOutcomeUnknown);

    assert!(handle.dispatch_operation(
        "extension-fenced".into(),
        Command::UninstallFocusedExtension {
            install: zephium_core::ids::ExtensionInstallId::from(92_001),
            expected_catalog: zephium_core::extensions::ExtensionInstallCatalogRevision::INITIAL,
            expected_install: zephium_core::extensions::ExtensionInstallRevision::INITIAL,
        },
    ));
    let fenced = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
        .find_map(|projection| match projection {
            Projection::OperationProcessed(completion)
                if completion.operation_id == "extension-fenced" =>
            {
                Some(completion)
            }
            _ => None,
        })
        .expect("post-ambiguity operation must be rejected in actor order");
    assert_eq!(fenced.outcome, OperationOutcome::Rejected);
    assert_eq!(fenced.reason, OperationReason::StoreReconciliationFailed);
    assert!(extension_state.uninstall_calls.lock().unwrap().is_empty());

    assert_eq!(
        handle
            .shutdown()
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap(),
        ShutdownOutcome::Clean
    );
}

#[test]
fn spawned_shutdown_is_ordered_behind_prior_commands() {
    let store = Arc::new(FakeStore::default());
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = spawn(
        Arc::new(FakeEngine::default()),
        store.clone(),
        Arc::new(ImmediateAllowAllCompiler),
        clean_extension_lifecycle(),
        Box::new(|_| {}),
        Arc::new(FakeChrome),
        Box::new(move |projection| {
            let _ = tx.send(projection);
        }),
    )
    .expect("spawn test shell");
    handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0)));
    handle.dispatch(Command::Bootstrap);
    let state = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
        .find_map(|projection| match projection {
            Projection::Items(state) => Some(state),
            _ => None,
        })
        .expect("bootstrap snapshot");
    let id = ItemId::parse(state.active.as_deref().unwrap()).unwrap();

    handle.dispatch(Command::Navigate {
        id,
        input: "example.com".into(),
    });
    handle.dispatch(Command::Engine(EngineEvent::UrlChanged {
        id,
        url: "https://example.com/".into(),
    }));
    handle.dispatch(Command::Engine(EngineEvent::TitleChanged {
        id,
        title: "Queued before close".into(),
    }));
    let completion = handle.shutdown();
    let workers = completion.workers.clone();
    assert_eq!(
        completion
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap(),
        ShutdownOutcome::Clean
    );
    assert!(workers.actor_and_timer_stopped());

    let saved = store.saved.lock().unwrap().clone().unwrap();
    let tab = saved.items.iter().find(|item| item.id == id).unwrap();
    let PersistedKind::Tab { title, .. } = &tab.kind else {
        panic!("active item must remain a tab");
    };
    assert_eq!(title, "Queued before close");
    assert_eq!(
        store
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| **event == "flush")
            .count(),
        2
    );
}

#[test]
fn dropping_last_handle_does_not_cancel_an_accepted_shutdown_barrier() {
    let handle = spawn(
        Arc::new(FakeEngine::default()),
        Arc::new(FakeStore::default()),
        Arc::new(ImmediateAllowAllCompiler),
        clean_extension_lifecycle(),
        Box::new(|_| {}),
        Arc::new(FakeChrome),
        Box::new(|_| {}),
    )
    .expect("spawn test shell");
    assert!(handle.dispatch(Command::Bootstrap));
    let completion = handle.shutdown();
    drop(handle);

    assert_eq!(
        completion
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap(),
        ShutdownOutcome::Clean
    );
}

#[test]
fn bounded_startup_observation_cannot_starve_a_queued_shutdown() {
    let (extension_service, extension_state) = extension_lifecycle_with_startup_outcomes([
        zephium_core::ports::extensions::ExtensionServiceStartupOutcome::Unavailable,
    ]);
    extension_state
        .wait_until_startup_deadline
        .store(true, std::sync::atomic::Ordering::Release);
    let handle = spawn(
        Arc::new(FakeEngine::default()),
        Arc::new(FakeStore::default()),
        Arc::new(ImmediateAllowAllCompiler),
        extension_service,
        Box::new(|_| {}),
        Arc::new(FakeChrome),
        Box::new(|_| {}),
    )
    .expect("spawn test shell");

    assert!(handle.dispatch(Command::Bootstrap));
    let completion = handle.shutdown();

    assert_eq!(completion.recv().unwrap(), ShutdownOutcome::Clean);
    assert_eq!(
        extension_state
            .startup_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        extension_state
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
}

#[test]
fn real_timer_thread_retries_transient_extension_startup_once() {
    use zephium_core::ports::extensions::ExtensionServiceStartupOutcome::{Ready, Unavailable};

    let store = Arc::new(FakeStore::default());
    let (extension_service, extension_state) = extension_lifecycle_with_startup_outcomes([
        Unavailable,
        Ready(zephium_core::ports::extensions::ExtensionActiveProfiles::EMPTY),
    ]);
    let handle = spawn(
        Arc::new(FakeEngine::default()),
        store.clone(),
        Arc::new(ImmediateAllowAllCompiler),
        extension_service,
        Box::new(|_| {}),
        Arc::new(FakeChrome),
        Box::new(|_| {}),
    )
    .expect("spawn test shell");

    assert!(handle.dispatch(Command::Bootstrap));
    assert!(wait_for_actor_condition(
        std::time::Duration::from_secs(2),
        || extension_state
            .startup_calls
            .load(std::sync::atomic::Ordering::Acquire)
            == 2
            && store
                .load_session_calls
                .load(std::sync::atomic::Ordering::Acquire)
                == 1
    ));
    assert_eq!(
        extension_state
            .startup_calls
            .load(std::sync::atomic::Ordering::Acquire),
        2,
        "one actual timer wake must settle the retained startup attempt"
    );
    assert_eq!(
        store
            .load_session_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(handle.shutdown().recv().unwrap(), ShutdownOutcome::Clean);
}

#[test]
fn activated_distribution_reloads_visible_management_and_ignores_the_overtaken_read() {
    let (tx, rx) = std::sync::mpsc::channel();
    let (extension_service, extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    let handle = spawn(
        Arc::new(FakeEngine::default()),
        Arc::new(FakeStore::default()),
        Arc::new(ImmediateAllowAllCompiler),
        extension_service,
        Box::new(|_| {}),
        Arc::new(FakeChrome),
        Box::new(move |projection| {
            let _ = tx.send(projection);
        }),
    )
    .expect("spawn test shell");
    assert!(handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0))));
    assert!(handle.dispatch(Command::Bootstrap));
    let profile = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
        .find_map(|projection| match projection {
            Projection::Items(items) => items.profile.map(|profile| profile.id),
            _ => None,
        })
        .and_then(|profile| ProfileId::parse(&profile))
        .expect("bootstrap must publish the focused profile");
    let _ = rx.try_iter().count();

    assert!(handle.dispatch(Command::SetExtensionManagementVisible(true)));
    assert!(wait_for_actor_condition(
        std::time::Duration::from_secs(2),
        || extension_state
            .management_catalog_calls
            .lock()
            .unwrap()
            .len()
            == 1
    ));
    let completion = zephium_core::ports::extensions::ExtensionDistributionCompletionStatus::new(
        zephium_core::extensions::ExtensionCatalogSetDigest::from_bytes([7; 32]),
        1,
        1,
        0,
        0,
        true,
    )
    .unwrap();
    assert!(handle.dispatch(Command::ExtensionDistributionStatusChanged(
        zephium_core::ports::extensions::ExtensionDistributionStatus::new(
            2,
            zephium_core::ports::extensions::ExtensionDistributionState::Ready(completion),
        )
        .unwrap(),
    )));
    assert!(wait_for_actor_condition(
        std::time::Duration::from_secs(2),
        || extension_state
            .management_catalog_calls
            .lock()
            .unwrap()
            .len()
            == 2
    ));

    let overtaken = extension_state
        .management_catalog_callbacks
        .lock()
        .unwrap()
        .remove(0);
    overtaken(zephium_core::ports::extensions::ExtensionManagementCatalogOutcome::Rejected);
    settle_next_management_catalog(
        &extension_state,
        empty_extension_management_catalog(
            profile,
            zephium_core::extensions::ExtensionInstallCatalogRevision::INITIAL,
        ),
    );
    let ready = wait_for_ready_management_catalog(
        &rx,
        profile,
        zephium_core::extensions::ExtensionInstallCatalogRevision::INITIAL,
        0,
    );
    assert_eq!(ready.phase, zephium_ipc::ExtensionManagementPhase::Ready);

    assert_eq!(
        handle
            .shutdown()
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap(),
        ShutdownOutcome::Clean
    );
}
