//! Actual Shell queue + original Store + retained native/controller composition.
use super::*;
use crate::work_resources::application::{ActorRequest, StagedActor};
use crate::{Command, PreparedRetainedWork, RetainedWorkPhase};

fn child(name: &str) -> bool {
    const CHILD: &str = "ZEPHIUM_RETAINED_PRODUCT_CHILD";
    if std::env::var_os(CHILD).is_some() {
        return false;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("work_resources::controller::tests::product_tests::{name}"),
            "--nocapture",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "retained product child: {output:?}"
    );
    true
}

fn pump(
    queue: &crate::actor::CommandQueue,
    shell: &mut crate::Shell,
    mut done: impl FnMut() -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "Shell queue deadline");
        if let Some(command) = queue.try_recv() {
            shell.handle(command);
        } else {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

fn selected(
    owner: &crate::Handle,
    queue: &crate::actor::CommandQueue,
    shell: &mut crate::Shell,
) -> crate::AgentWorkProfileBinding {
    let query = owner.work_profile_binding();
    let mut answer = None;
    pump(queue, shell, || {
        answer = query.try_recv();
        answer.is_some()
    });
    let Some(crate::AgentWorkProfileReadiness::Ready(binding)) = answer else {
        panic!("selected ready profile: {answer:?}");
    };
    binding
}

type Servers = Arc<Mutex<Vec<std::thread::JoinHandle<usize>>>>;
fn prepared<S: AgentWorkJournalPort + AgentAuditPort + 'static>(
    profile: crate::AgentWorkProfileBinding,
    engine: crate::SharedEngine,
    store: Arc<S>,
    native: Arc<Native>,
    factories: Arc<AtomicUsize>,
    servers: Servers,
) -> PreparedRetainedWork {
    prepared_until(
        profile,
        engine,
        store,
        native,
        factories,
        servers,
        Instant::now() + Duration::from_secs(600),
    )
}
fn prepared_until<S: AgentWorkJournalPort + AgentAuditPort + 'static>(
    profile: crate::AgentWorkProfileBinding,
    engine: crate::SharedEngine,
    store: Arc<S>,
    native: Arc<Native>,
    factories: Arc<AtomicUsize>,
    servers: Servers,
    deadline: Instant,
) -> PreparedRetainedWork {
    prepared_until_with_document_policy(
        profile,
        engine,
        store,
        native,
        factories,
        servers,
        deadline,
        WorkBrowserDocumentPolicy::Exact,
    )
}
#[allow(clippy::too_many_arguments)]
fn prepared_until_with_document_policy<S: AgentWorkJournalPort + AgentAuditPort + 'static>(
    profile: crate::AgentWorkProfileBinding,
    engine: crate::SharedEngine,
    store: Arc<S>,
    native: Arc<Native>,
    factories: Arc<AtomicUsize>,
    servers: Servers,
    deadline: Instant,
    document_policy: WorkBrowserDocumentPolicy,
) -> PreparedRetainedWork {
    let target = ContextNavigationTarget::parse("https://retained-fixture.invalid/frozen").unwrap();
    let identity = ContextIdentity::new(
        ContextId::generate(),
        ContextRunId::generate(),
        profile.profile(),
        ContextKind::Owned,
    );
    let form_action = native.form_actions.load(Ordering::Acquire);
    let effects = if form_action {
        vec![SemanticEffectClass::Read, SemanticEffectClass::LocalWrite]
    } else {
        vec![SemanticEffectClass::Read]
    };
    let origin = SemanticOrigin::parse(target.as_url().as_ref()).unwrap();
    let input = input_for_context_effects(
        identity,
        origin.clone(),
        Arc::new(Clock(AtomicU64::new(2))),
        profile.storage_class(),
        target,
        AgentRunBudget::try_new(24, 1_000_000, 1_000_000, 1).unwrap(),
        (deadline, None),
        document_policy,
        AgentEffectScope::try_new(&effects).unwrap(),
    );
    let spec = input.retained_resource_spec().unwrap();
    let actor = ActorRequest {
        run: identity.owner(),
        deadline: spec.expires_at,
        prepare: Box::new(move |browser, audit, waiting| {
            if waiting.is_some() {
                return Err(AgentWorkFailure::Contract);
            }
            let responses = if form_action {
                vec![action_tests::act_stream("fixture value")]
            } else {
                vec![response_stream(1), response_stream(2)]
            };
            let task: Box<dyn AgentWorkTask> = if form_action {
                Box::new(
                    AgentWorkFormTask::try_new_local_preparation(
                        identity,
                        origin,
                        AgentAccountScope::Anonymous,
                        vec![AgentWorkFormPhase::try_new(vec![AgentWorkFormGoal::fill(
                            Some("Draft".into()),
                            "fixture value".into(),
                        )
                        .unwrap()])
                        .unwrap()],
                    )
                    .unwrap()
                    .with_extraction(vec![SemanticExtractionFieldSchema::try_text(
                        "label".into(),
                        true,
                        64,
                    )
                    .unwrap()])
                    .unwrap(),
                )
            } else {
                Box::new(task())
            };
            let (transport, server) = fixture_provider_responses(responses);
            servers.lock().unwrap().push(server);
            StagedActor::for_probe(
                input,
                browser,
                transport,
                AgentProviderCredential::try_new(
                    AgentProviderKind::OpenAiResponses,
                    "fixture-not-a-secret".into(),
                )
                .unwrap(),
                audit,
                task,
            )
        }),
    };
    PreparedRetainedWork::for_test(
        spec,
        actor,
        profile,
        engine,
        store.clone(),
        store,
        Box::new(move |sink| {
            factories.fetch_add(1, Ordering::AcqRel);
            *native.resource_sink.lock().unwrap() = Some(sink);
            Some(native)
        }),
    )
    .unwrap_or_else(|_| panic!("approved dormant product input"))
}

struct StoreAlias(Arc<zephium_store::SqliteStore>);
impl AgentAuditPort for StoreAlias {
    fn append(
        &self,
        delivery: AgentAuditDelivery,
        completion: AgentAuditCompletion,
    ) -> AgentAuditDispatch {
        self.0.append(delivery, completion)
    }
}
impl AgentWorkJournalPort for StoreAlias {
    fn dispatch(
        &self,
        request: AgentWorkJournalRequest,
        completion: AgentWorkJournalCompletion,
    ) -> Result<(), AgentWorkJournalError> {
        self.0.dispatch(request, completion)
    }
}

struct DormantStore;
impl AgentAuditPort for DormantStore {
    fn append(&self, _: AgentAuditDelivery, _: AgentAuditCompletion) -> AgentAuditDispatch {
        panic!("dormant preparation must not append audit");
    }
}
impl AgentWorkJournalPort for DormantStore {
    fn dispatch(
        &self,
        _: AgentWorkJournalRequest,
        _: AgentWorkJournalCompletion,
    ) -> Result<(), AgentWorkJournalError> {
        panic!("dormant preparation must not claim Store");
    }
}

#[test]
fn shipping_preparation_checks_profile_storage_original_audit_and_result_contract_without_effects()
{
    let profile = zephium_core::profiles::Profile {
        id: ProfileId::generate(),
        name: "fixture".into(),
        kind: zephium_core::profiles::ProfileKind::Named,
    };
    let store = Arc::new(DormantStore);
    for mode in 0..5 {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            profile.id,
            ContextKind::Owned,
        );
        let target =
            ContextNavigationTarget::parse("https://retained-fixture.invalid/frozen").unwrap();
        let input = input_for_context(
            identity,
            SemanticOrigin::parse(target.as_url().as_ref()).unwrap(),
            Arc::new(Clock(AtomicU64::new(2))),
            ContextProfileStorageClass::Durable,
            target,
            AgentRunBudget::try_new(24, 1_000_000, 1_000_000, 1).unwrap(),
        );
        let input = if mode == 4 {
            input.persist_extraction_result().unwrap()
        } else {
            input
        };
        let mut selected = profile.clone();
        if mode == 1 {
            selected.id = ProfileId::generate();
        }
        if mode == 2 {
            selected.kind = zephium_core::profiles::ProfileKind::Incognito;
        }
        let audit = if mode == 3 {
            Arc::new(DormantStore)
        } else {
            store.clone()
        };
        let result = PreparedRetainedWork::try_new(
            input,
            crate::AgentWorkProfileBinding::from_profile(&selected),
            crate::AgentWorkApplicationConfig::new(
                AgentRuntimeConfig::STANDARD,
                AgentProviderTransportConfig::STANDARD,
            ),
            AgentProviderCredential::try_new(
                AgentProviderKind::OpenAiResponses,
                "fixture-not-a-secret".into(),
            )
            .unwrap(),
            Box::new(task()),
            crate::RetainedWorkPorts::new(
                Arc::new(crate::shell::tests::FakeEngine::default()),
                store.clone(),
                audit,
                Box::new(|_| panic!("dormant preparation must not take native factory")),
            ),
        );
        // Durable result intent is supported without relaxing the selected
        // profile/storage or original audit-port admission checks.
        assert_eq!(result.is_ok(), matches!(mode, 0 | 4));
    }
}

#[test]
fn synchronous_action_rejection_persists_failed_terminal_and_shell_shuts_down_cleanly() {
    if child("synchronous_action_rejection_persists_failed_terminal_and_shell_shuts_down_cleanly") {
        return;
    }
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(zephium_store::SqliteStore::open(directory.path()).unwrap());
    let engine = Arc::new(crate::shell::tests::FakeEngine::default());
    let queue = crate::actor::CommandQueue::new();
    let owner = crate::actor::Handle::new(queue.clone());
    let mut shell = crate::Shell::new(
        engine.clone(),
        store.clone(),
        Arc::new(crate::shell::tests::FakeChrome),
        Box::new(|_| {}),
    );
    shell.attach_queue(queue.clone());
    shell.handle(Command::Bootstrap);
    let profile = selected(&owner, &queue, &mut shell);
    let native = Arc::new(Native::default());
    native.allow_global_shutdown.store(true, Ordering::Release);
    native.form_actions.store(true, Ordering::Release);
    native.reject_action.store(true, Ordering::Release);
    let factories = Arc::new(AtomicUsize::new(0));
    let servers = Servers::default();
    let view = owner
        .callback_handle()
        .attach_retained_work(prepared(
            profile,
            engine,
            store.clone(),
            native.clone(),
            factories.clone(),
            servers.clone(),
        ))
        .unwrap();
    let mut events = Vec::new();
    pump(&queue, &mut shell, || {
        while let Some(event) = view.take_event() {
            events.push(event.kind());
        }
        matches!(
            view.snapshot().phase,
            RetainedWorkPhase::Terminal | RetainedWorkPhase::Uncertain
        )
    });
    let snapshot = view.snapshot();
    assert_eq!(snapshot.phase, RetainedWorkPhase::Terminal);
    assert_eq!(snapshot.persistence_failure, None);
    assert_eq!(
        snapshot.failure,
        Some(AgentWorkFailure::Browser(
            AgentBrowserProviderError::Action(AgentBrowserActionError::Failed(
                SemanticActionFailure::BackendRefused
            ),)
        ))
    );
    let record = snapshot.record.unwrap();
    assert_eq!(record.disposition(), AgentWorkDisposition::Failed);
    assert_eq!(record.debt(), AgentWorkDebt::NONE);
    let (tx, rx) = mpsc::sync_channel(1);
    store
        .dispatch(
            AgentWorkJournalRequest::Read {
                owner: record.incarnation(),
                key: record.key(),
            },
            Box::new(move |reply| tx.send(reply).unwrap()),
        )
        .unwrap();
    assert!(matches!(rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        Ok(AgentWorkJournalReply::Record(Some(durable))) if durable == record));
    assert!(view.take_extraction().is_none());
    assert!(!events.contains(&AgentWorkEventKind::Verified));
    assert!(!events.contains(&AgentWorkEventKind::Recovery));
    assert_eq!(
        events
            .iter()
            .filter(|kind| matches!(kind, AgentWorkEventKind::ActionRejected(_)))
            .count(),
        1
    );
    assert_eq!(factories.load(Ordering::Acquire), 1);
    assert_eq!(native.actions.load(Ordering::Acquire), 1);
    assert_eq!(native.reads.load(Ordering::Acquire), 1);
    assert!(!native.form_applied.load(Ordering::Acquire));
    native.join();
    for server in servers.lock().unwrap().drain(..) {
        assert_eq!(server.join().unwrap(), 1);
    }
    let shutdown = owner.shutdown_with_deadline(Instant::now() + Duration::from_secs(5));
    while let Some(command) = queue.try_recv() {
        let terminal = matches!(command, Command::Shutdown { .. });
        shell.handle(command);
        if terminal {
            break;
        }
    }
    assert_eq!(shutdown.recv(), Ok(crate::ShutdownOutcome::Clean));
    assert_eq!(native.destructions.load(Ordering::Acquire), 1);
    assert!(native.global_sealed.load(Ordering::Acquire));
}

#[test]
fn selected_shell_retains_page_after_durable_result_and_owns_global_shutdown() {
    if child("selected_shell_retains_page_after_durable_result_and_owns_global_shutdown") {
        return;
    }
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(zephium_store::SqliteStore::open(directory.path()).unwrap());
    let engine = Arc::new(crate::shell::tests::FakeEngine::default());
    let queue = crate::actor::CommandQueue::new();
    let owner = crate::actor::Handle::new(queue.clone());
    let callback = owner.callback_handle();
    let mut shell = crate::Shell::new(
        engine.clone(),
        store.clone(),
        Arc::new(crate::shell::tests::FakeChrome),
        Box::new(|_| {}),
    );
    shell.attach_queue(queue.clone());
    shell.handle(Command::Bootstrap);
    let profile = selected(&owner, &queue, &mut shell);
    let native = Arc::new(Native::default());
    native.allow_global_shutdown.store(true, Ordering::Release);
    let factories = Arc::new(AtomicUsize::new(0));
    let servers = Servers::default();

    // Foreign Engine and changed selected-profile identity refuse before the
    // original one-shot factory, durable Claim or any provider work.
    let wrong = callback
        .attach_retained_work(prepared(
            profile,
            Arc::new(crate::shell::tests::FakeEngine::default()),
            store.clone(),
            native.clone(),
            factories.clone(),
            servers.clone(),
        ))
        .unwrap();
    pump(&queue, &mut shell, || {
        wrong.snapshot().phase == RetainedWorkPhase::Refused
    });
    let foreign = crate::AgentWorkProfileBinding::from_profile(&zephium_core::profiles::Profile {
        id: ProfileId::generate(),
        name: "fixture".into(),
        kind: zephium_core::profiles::ProfileKind::Named,
    });
    let wrong_profile = callback
        .attach_retained_work(prepared(
            foreign,
            engine.clone(),
            store.clone(),
            native.clone(),
            factories.clone(),
            servers.clone(),
        ))
        .unwrap();
    pump(&queue, &mut shell, || {
        wrong_profile.snapshot().phase == RetainedWorkPhase::Refused
    });
    assert_eq!(factories.load(Ordering::Acquire), 0);
    assert!(servers.lock().unwrap().is_empty());

    let wrong_store = callback
        .attach_retained_work(prepared(
            profile,
            engine.clone(),
            Arc::new(StoreAlias(store.clone())),
            native.clone(),
            factories.clone(),
            servers.clone(),
        ))
        .unwrap();
    pump(&queue, &mut shell, || {
        wrong_store.snapshot().phase == RetainedWorkPhase::Refused
    });
    let stopped = callback
        .attach_retained_work(prepared(
            profile,
            engine.clone(),
            store.clone(),
            native.clone(),
            factories.clone(),
            servers.clone(),
        ))
        .unwrap();
    assert!(stopped.stop());
    pump(&queue, &mut shell, || {
        stopped.snapshot().phase == RetainedWorkPhase::Refused
    });
    assert_eq!(factories.load(Ordering::Acquire), 0);

    let view = callback
        .attach_retained_work(prepared(
            profile,
            engine.clone(),
            store.clone(),
            native.clone(),
            factories.clone(),
            servers.clone(),
        ))
        .unwrap();
    assert!(view.take_extraction().is_none());
    let duplicate = callback
        .attach_retained_work(prepared(
            profile,
            engine.clone(),
            store.clone(),
            native.clone(),
            factories.clone(),
            servers.clone(),
        ))
        .unwrap();
    pump(&queue, &mut shell, || {
        while view.take_event().is_some() {}
        view.snapshot().phase == RetainedWorkPhase::Terminal
            && duplicate.snapshot().phase == RetainedWorkPhase::Refused
    });
    let snapshot = view.snapshot();
    assert_eq!(snapshot.failure, None);
    assert_eq!(snapshot.persistence_failure, None);
    assert_eq!(
        snapshot.record.unwrap().disposition(),
        AgentWorkDisposition::Succeeded
    );
    assert_eq!(snapshot.record.unwrap().debt(), AgentWorkDebt::NONE);
    assert!(view.take_extraction().is_some());
    assert!(view.take_extraction().is_none());
    assert_eq!(factories.load(Ordering::Acquire), 1);
    assert_eq!(native.acquisitions.load(Ordering::Acquire), 1);
    assert_eq!(native.destructions.load(Ordering::Acquire), 0);
    assert_eq!(native.reporters.lock().unwrap().len(), 1);
    assert!(!native.global_sealed.load(Ordering::Acquire));
    native.join();
    for server in servers.lock().unwrap().drain(..) {
        assert_eq!(server.join().unwrap(), 2);
    }
    let legacy = callback.attach_work(store.clone(), engine).unwrap();
    pump(&queue, &mut shell, || {
        legacy.snapshot().phase == crate::AgentWorkApplicationPhase::Recovery
    });
    assert!(!view.is_closed());
    assert!(view.close());
    pump(&queue, &mut shell, || view.is_closed());
    assert_eq!(native.destructions.load(Ordering::Acquire), 1);
    assert!(native.global_sealed.load(Ordering::Acquire));
    assert!(native.reporters.lock().unwrap().is_empty());
    let shutdown = owner.shutdown_with_deadline(Instant::now() + Duration::from_secs(5));
    while let Some(command) = queue.try_recv() {
        let terminal = matches!(command, Command::Shutdown { .. });
        shell.handle(command);
        if terminal {
            break;
        }
    }
    assert_eq!(shutdown.recv(), Ok(crate::ShutdownOutcome::Clean));
    assert_eq!(native.destructions.load(Ordering::Acquire), 1);
    assert!(native.global_sealed.load(Ordering::Acquire));
    assert!(native.reporters.lock().unwrap().is_empty());
}

#[test]
fn stop_during_original_construction_never_acquires_or_starts_and_shell_cleans_it() {
    if child("stop_during_original_construction_never_acquires_or_starts_and_shell_cleans_it") {
        return;
    }
    interrupted_construction(false, WorkBrowserDocumentPolicy::Exact);
}

#[test]
fn expiry_during_original_construction_never_acquires_or_starts_and_shell_cleans_it() {
    if child("expiry_during_original_construction_never_acquires_or_starts_and_shell_cleans_it") {
        return;
    }
    interrupted_construction(true, WorkBrowserDocumentPolicy::Exact);
}

#[test]
fn trusted_initial_document_policy_reaches_the_original_native_construction() {
    if child("trusted_initial_document_policy_reaches_the_original_native_construction") {
        return;
    }
    interrupted_construction(false, WorkBrowserDocumentPolicy::InitialQueryFinalization);
}

fn interrupted_construction(expire: bool, document_policy: WorkBrowserDocumentPolicy) {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(zephium_store::SqliteStore::open(directory.path()).unwrap());
    let engine = Arc::new(crate::shell::tests::FakeEngine::default());
    let queue = crate::actor::CommandQueue::new();
    let owner = crate::actor::Handle::new(queue.clone());
    let callback = owner.callback_handle();
    let mut shell = crate::Shell::new(
        engine.clone(),
        store.clone(),
        Arc::new(crate::shell::tests::FakeChrome),
        Box::new(|_| {}),
    );
    shell.attach_queue(queue.clone());
    shell.handle(Command::Bootstrap);
    let profile = selected(&owner, &queue, &mut shell);
    let native = Arc::new(Native::default());
    native.allow_global_shutdown.store(true, Ordering::Release);
    native.hold_construct.store(true, Ordering::Release);
    let factories = Arc::new(AtomicUsize::new(0));
    let servers = Servers::default();
    let deadline = Instant::now()
        + if expire {
            Duration::from_millis(200)
        } else {
            Duration::from_secs(600)
        };
    let view = callback
        .attach_retained_work(prepared_until_with_document_policy(
            profile,
            engine,
            store.clone(),
            native.clone(),
            factories.clone(),
            servers.clone(),
            deadline,
            document_policy,
        ))
        .unwrap();
    pump(&queue, &mut shell, || {
        view.snapshot().phase == RetainedWorkPhase::Constructing
    });
    assert_eq!(
        native
            .construction
            .lock()
            .unwrap()
            .as_ref()
            .map(|(request, _)| request.document_policy()),
        Some(document_policy)
    );
    if expire {
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        assert!(callback.dispatch(Command::WorkWake));
    } else {
        assert!(view.stop());
    }
    pump(&queue, &mut shell, || {
        view.snapshot().phase == RetainedWorkPhase::Uncertain
    });
    assert!(native.construction.lock().unwrap().is_some());
    assert_eq!(native.destructions.load(Ordering::Acquire), 0);
    native.release_construction();
    while let Some(command) = queue.try_recv() {
        shell.handle(command);
    }
    assert_eq!(native.acquisitions.load(Ordering::Acquire), 0);
    assert!(view.snapshot().record.is_none());
    assert!(view.take_extraction().is_none());
    assert!(servers.lock().unwrap().is_empty());
    let shutdown = owner.shutdown_with_deadline(Instant::now() + Duration::from_secs(5));
    while let Some(command) = queue.try_recv() {
        let terminal = matches!(command, Command::Shutdown { .. });
        shell.handle(command);
        if terminal {
            break;
        }
    }
    assert_eq!(shutdown.recv(), Ok(crate::ShutdownOutcome::Clean));
    assert_eq!(native.destructions.load(Ordering::Acquire), 1);
    assert!(native.global_sealed.load(Ordering::Acquire));
}
