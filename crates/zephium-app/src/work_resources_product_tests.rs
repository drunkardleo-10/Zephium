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
    prepared_until_isolated(
        profile,
        engine,
        store,
        native,
        factories,
        servers,
        deadline,
        document_policy,
        false,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn prepared_until_isolated<S: AgentWorkJournalPort + AgentAuditPort + 'static>(
    profile: crate::AgentWorkProfileBinding,
    engine: crate::SharedEngine,
    store: Arc<S>,
    native: Arc<Native>,
    factories: Arc<AtomicUsize>,
    servers: Servers,
    deadline: Instant,
    document_policy: WorkBrowserDocumentPolicy,
    isolated: bool,
    provider_gate: Option<Arc<AtomicBool>>,
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
    let input = if isolated {
        input.with_isolated_website_data()
    } else {
        input
    };
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
            let (transport, server) =
                crate::work_provider_fixture::fixture_provider_inspect(responses, move |_, _| {
                    if let Some(gate) = &provider_gate {
                        let deadline = Instant::now() + Duration::from_secs(5);
                        while !gate.load(Ordering::Acquire) {
                            assert!(Instant::now() < deadline, "provider test gate");
                            std::thread::sleep(Duration::from_millis(1));
                        }
                    }
                });
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
fn work_page_group_retains_three_distinct_pages_and_rejects_unrelated_admission() {
    if child("work_page_group_retains_three_distinct_pages_and_rejects_unrelated_admission") {
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
    let factories = Arc::new(AtomicUsize::new(0));
    let servers = Servers::default();
    let work = WorkId::generate();
    let execution = zephium_core::work::WorkExecutionId::generate();
    let attempt = zephium_core::work::WorkAttemptId::generate();
    let deadline = Instant::now() + Duration::from_secs(30);
    let steps: Vec<_> = (0..4)
        .map(|_| zephium_core::work::WorkStepId::generate())
        .collect();
    let mut native_pages = Vec::new();
    let mut views = Vec::new();
    let provider_gate = Arc::new(AtomicBool::new(false));
    for (index, step) in steps[..2].iter().enumerate() {
        let native = Arc::new(Native::default());
        native.allow_global_shutdown.store(true, Ordering::Release);
        let request = prepared_until_isolated(
            profile,
            engine.clone(),
            store.clone(),
            native.clone(),
            factories.clone(),
            servers.clone(),
            deadline,
            WorkBrowserDocumentPolicy::Exact,
            true,
            (index == 0).then(|| provider_gate.clone()),
        )
        .with_work_identity(work)
        .with_page_admission(crate::RetainedPageAdmission {
            profile: profile.profile(),
            work,
            execution,
            attempt,
            step: *step,
            workers: 3,
            deadline,
        })
        .unwrap();
        views.push(callback.attach_retained_work(request).unwrap());
        native_pages.push(native);
        if index == 0 {
            pump(&queue, &mut shell, || {
                views[0]
                    .snapshot()
                    .record
                    .is_some_and(|record| record.disposition() == AgentWorkDisposition::Running)
            });
        }
    }
    pump(&queue, &mut shell, || {
        while views[1].take_event().is_some() {}
        views[1].snapshot().phase == RetainedWorkPhase::Terminal
    });
    provider_gate.store(true, Ordering::Release);
    pump(&queue, &mut shell, || {
        for view in &views {
            while view.take_event().is_some() {}
        }
        views
            .iter()
            .all(|view| view.snapshot().phase == RetainedWorkPhase::Terminal)
    });
    assert_eq!(factories.load(Ordering::Acquire), 2);
    assert!(views.iter().all(|view| view
        .snapshot()
        .record
        .is_some_and(|record| record.disposition() == AgentWorkDisposition::Succeeded)));
    for (step, candidate_work, candidate_attempt) in [
        (steps[0], work, attempt),
        (steps[3], WorkId::generate(), attempt),
        (
            steps[3],
            work,
            zephium_core::work::WorkAttemptId::generate(),
        ),
    ] {
        let request = prepared_until_isolated(
            profile,
            engine.clone(),
            store.clone(),
            Arc::new(Native::default()),
            factories.clone(),
            servers.clone(),
            deadline,
            WorkBrowserDocumentPolicy::Exact,
            true,
            None,
        )
        .with_work_identity(candidate_work)
        .with_page_admission(crate::RetainedPageAdmission {
            profile: profile.profile(),
            work: candidate_work,
            execution,
            attempt: candidate_attempt,
            step,
            workers: 3,
            deadline,
        })
        .unwrap();
        let refused = callback.attach_retained_work(request).unwrap();
        pump(&queue, &mut shell, || {
            refused.snapshot().phase == RetainedWorkPhase::Refused
        });
    }
    assert_eq!(factories.load(Ordering::Acquire), 2);
    let native = Arc::new(Native::default());
    native.allow_global_shutdown.store(true, Ordering::Release);
    let request = prepared_until_isolated(
        profile,
        engine.clone(),
        store.clone(),
        native.clone(),
        factories.clone(),
        servers.clone(),
        deadline,
        WorkBrowserDocumentPolicy::Exact,
        true,
        None,
    )
    .with_work_identity(work)
    .with_page_admission(crate::RetainedPageAdmission {
        profile: profile.profile(),
        work,
        execution,
        attempt,
        step: steps[2],
        workers: 3,
        deadline,
    })
    .unwrap();
    views.push(callback.attach_retained_work(request).unwrap());
    native_pages.push(native);
    pump(&queue, &mut shell, || {
        while views[2].take_event().is_some() {}
        views[2].snapshot().phase == RetainedWorkPhase::Terminal
    });
    let request = prepared_until_isolated(
        profile,
        engine.clone(),
        store.clone(),
        Arc::new(Native::default()),
        factories.clone(),
        servers.clone(),
        deadline,
        WorkBrowserDocumentPolicy::Exact,
        true,
        None,
    )
    .with_work_identity(work)
    .with_page_admission(crate::RetainedPageAdmission {
        profile: profile.profile(),
        work,
        execution,
        attempt,
        step: steps[3],
        workers: 3,
        deadline,
    })
    .unwrap();
    let refused = callback.attach_retained_work(request).unwrap();
    pump(&queue, &mut shell, || {
        refused.snapshot().phase == RetainedWorkPhase::Refused
    });
    assert_eq!(factories.load(Ordering::Acquire), 3);
    assert!(views[0].close());
    pump(&queue, &mut shell, || {
        native_pages[0].destructions.load(Ordering::Acquire) == 1
    });
    pump(&queue, &mut shell, || views[0].is_group_locally_retired());
    assert!(!views[0].is_closed());
    assert!(!views[1].is_group_locally_retired() && !views[2].is_group_locally_retired());
    assert!(!views[1].is_closed() && !views[2].is_closed());
    assert_eq!(native_pages[0].destructions.load(Ordering::Acquire), 1);
    assert_eq!(native_pages[1].destructions.load(Ordering::Acquire), 0);
    native_pages[0]
        .hold_global_audit
        .store(true, Ordering::Release);
    native_pages[1]
        .hold_global_audit
        .store(true, Ordering::Release);
    let auditing = native_pages.clone();
    let audits = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(4);
        for index in 0..2 {
            while auditing[index]
                .pending_global_audit
                .lock()
                .unwrap()
                .is_none()
            {
                assert!(Instant::now() < deadline, "group audit wait");
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(auditing[index + 1..]
                .iter()
                .all(|native| !native.global_sealed.load(Ordering::Acquire)));
            auditing[index].release_global_audit();
        }
    });
    for server in servers.lock().unwrap().drain(..) {
        assert_eq!(server.join().unwrap(), 2);
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
    audits.join().unwrap();
    for native in native_pages {
        native.join();
        assert_eq!(native.destructions.load(Ordering::Acquire), 1);
    }
}

#[test]
fn work_page_group_retires_a_settled_uncertain_page_and_admits_a_later_read() {
    if child("work_page_group_retires_a_settled_uncertain_page_and_admits_a_later_read") {
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
    let factories = Arc::new(AtomicUsize::new(0));
    let servers = Servers::default();
    let work = WorkId::generate();
    let execution = zephium_core::work::WorkExecutionId::generate();
    let attempt = zephium_core::work::WorkAttemptId::generate();
    let deadline = Instant::now() + Duration::from_secs(30);
    let attach = |native: Arc<Native>| {
        let request = prepared_until_isolated(
            profile,
            engine.clone(),
            store.clone(),
            native,
            factories.clone(),
            servers.clone(),
            deadline,
            WorkBrowserDocumentPolicy::Exact,
            true,
            None,
        )
        .with_work_identity(work)
        .with_page_admission(crate::RetainedPageAdmission {
            profile: profile.profile(),
            work,
            execution,
            attempt,
            step: zephium_core::work::WorkStepId::generate(),
            workers: 3,
            deadline,
        })
        .unwrap();
        callback.attach_retained_work(request).unwrap()
    };
    // The stuck page's native construction never settles while its read ends.
    let stuck_native = Arc::new(Native::default());
    stuck_native
        .allow_global_shutdown
        .store(true, Ordering::Release);
    stuck_native.hold_construct.store(true, Ordering::Release);
    let stuck = attach(stuck_native.clone());
    pump(&queue, &mut shell, || {
        stuck.snapshot().phase == RetainedWorkPhase::Constructing
    });
    assert!(stuck.close());
    pump(&queue, &mut shell, || {
        stuck.snapshot().phase == RetainedWorkPhase::Uncertain
    });
    let native = Arc::new(Native::default());
    native.allow_global_shutdown.store(true, Ordering::Release);
    let later = attach(native.clone());
    pump(&queue, &mut shell, || {
        while later.take_event().is_some() {}
        matches!(
            later.snapshot().phase,
            RetainedWorkPhase::Terminal | RetainedWorkPhase::Refused | RetainedWorkPhase::Uncertain
        )
    });
    assert_eq!(later.snapshot().phase, RetainedWorkPhase::Terminal);
    assert!(later
        .snapshot()
        .record
        .is_some_and(|record| record.disposition() == AgentWorkDisposition::Succeeded));
    assert!(later.close());
    pump(&queue, &mut shell, || later.is_group_locally_retired());
    // The native audit counts the whole browser: none starts while the
    // graveyarded page still holds its resource.
    let settle = Instant::now() + Duration::from_millis(200);
    pump(&queue, &mut shell, || Instant::now() >= settle);
    assert!(!later.is_closed() && !stuck.is_closed());
    assert!(!native.global_sealed.load(Ordering::Acquire));
    assert!(!stuck_native.global_sealed.load(Ordering::Acquire));
    stuck_native.release_construction();
    pump(&queue, &mut shell, || {
        later.is_closed() && stuck.is_closed()
    });
    assert!(native.global_sealed.load(Ordering::Acquire));
    assert_eq!(factories.load(Ordering::Acquire), 2);
    for server in servers.lock().unwrap().drain(..) {
        server.join().unwrap();
    }
    let started = Instant::now();
    let shutdown = owner.shutdown_with_deadline(started + Duration::from_secs(5));
    while let Some(command) = queue.try_recv() {
        let terminal = matches!(command, Command::Shutdown { .. });
        shell.handle(command);
        if terminal {
            break;
        }
    }
    assert_eq!(shutdown.recv(), Ok(crate::ShutdownOutcome::Clean));
    assert!(started.elapsed() < Duration::from_secs(5));
    for native in [stuck_native, native] {
        native.join();
        assert_eq!(native.destructions.load(Ordering::Acquire), 1);
        assert!(native.global_sealed.load(Ordering::Acquire));
    }
}

#[test]
fn work_page_group_keeps_a_sibling_read_when_a_page_fails_before_admission() {
    if child("work_page_group_keeps_a_sibling_read_when_a_page_fails_before_admission") {
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
    let factories = Arc::new(AtomicUsize::new(0));
    let servers = Servers::default();
    let work = WorkId::generate();
    let execution = zephium_core::work::WorkExecutionId::generate();
    let attempt = zephium_core::work::WorkAttemptId::generate();
    let deadline = Instant::now() + Duration::from_secs(30);
    let attach = |native: Arc<Native>| {
        native.allow_global_shutdown.store(true, Ordering::Release);
        native.hold_construct.store(true, Ordering::Release);
        let request = prepared_until_isolated(
            profile,
            engine.clone(),
            store.clone(),
            native,
            factories.clone(),
            servers.clone(),
            deadline,
            WorkBrowserDocumentPolicy::Exact,
            true,
            None,
        )
        .with_work_identity(work)
        .with_page_admission(crate::RetainedPageAdmission {
            profile: profile.profile(),
            work,
            execution,
            attempt,
            step: zephium_core::work::WorkStepId::generate(),
            workers: 2,
            deadline,
        })
        .unwrap();
        callback.attach_retained_work(request).unwrap()
    };
    let failing_native = Arc::new(Native::default());
    let sibling_native = Arc::new(Native::default());
    let failing = attach(failing_native.clone());
    let sibling = attach(sibling_native.clone());
    pump(&queue, &mut shell, || {
        [&failing_native, &sibling_native]
            .iter()
            .all(|native| native.construction.lock().unwrap().is_some())
    });
    // The first page's load fails natively before it enters the journal.
    let (request, completion) = failing_native.construction.lock().unwrap().take().unwrap();
    completion(request.complete(WorkBrowserResourceNativeOutcome::Refused));
    pump(&queue, &mut shell, || {
        failing.snapshot().phase == RetainedWorkPhase::Uncertain
    });
    assert_eq!(
        failing.snapshot().failure,
        Some(AgentWorkFailure::ContextLost)
    );
    sibling_native.release_construction();
    pump(&queue, &mut shell, || {
        while sibling.take_event().is_some() {}
        matches!(
            sibling.snapshot().phase,
            RetainedWorkPhase::Terminal | RetainedWorkPhase::Refused | RetainedWorkPhase::Uncertain
        )
    });
    let snapshot = sibling.snapshot();
    assert_eq!(
        (snapshot.phase, snapshot.failure),
        (RetainedWorkPhase::Terminal, None)
    );
    assert!(snapshot
        .record
        .is_some_and(|record| record.disposition() == AgentWorkDisposition::Succeeded));
    assert!(failing.close() && sibling.close());
    pump(&queue, &mut shell, || {
        failing.is_closed() && sibling.is_closed()
    });
    assert_eq!(factories.load(Ordering::Acquire), 2);
    for server in servers.lock().unwrap().drain(..) {
        server.join().unwrap();
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
    for native in [failing_native, sibling_native] {
        native.join();
        assert!(native.global_sealed.load(Ordering::Acquire));
    }
}

#[test]
fn work_page_group_admits_a_later_read_after_its_reads_close_cleanly() {
    if child("work_page_group_admits_a_later_read_after_its_reads_close_cleanly") {
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
    let factories = Arc::new(AtomicUsize::new(0));
    let servers = Servers::default();
    let work = WorkId::generate();
    let execution = zephium_core::work::WorkExecutionId::generate();
    let attempt = zephium_core::work::WorkAttemptId::generate();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut natives = Vec::new();
    let mut attach = || {
        let native = Arc::new(Native::default());
        native.allow_global_shutdown.store(true, Ordering::Release);
        natives.push(native.clone());
        let request = prepared_until_isolated(
            profile,
            engine.clone(),
            store.clone(),
            native,
            factories.clone(),
            servers.clone(),
            deadline,
            WorkBrowserDocumentPolicy::Exact,
            true,
            None,
        )
        .with_work_identity(work)
        .with_page_admission(crate::RetainedPageAdmission {
            profile: profile.profile(),
            work,
            execution,
            attempt,
            step: zephium_core::work::WorkStepId::generate(),
            workers: 3,
            deadline,
        })
        .unwrap();
        callback.attach_retained_work(request).unwrap()
    };
    let settle = |views: &[crate::RetainedWorkHandle],
                  queue: &crate::actor::CommandQueue,
                  shell: &mut crate::Shell| {
        pump(queue, shell, || {
            views.iter().all(|view| {
                while view.take_event().is_some() {}
                matches!(
                    view.snapshot().phase,
                    RetainedWorkPhase::Terminal
                        | RetainedWorkPhase::Refused
                        | RetainedWorkPhase::Uncertain
                )
            })
        });
        for view in views {
            let snapshot = view.snapshot();
            assert_eq!(
                (snapshot.phase, snapshot.failure),
                (RetainedWorkPhase::Terminal, None)
            );
            assert!(snapshot
                .record
                .is_some_and(|record| record.disposition() == AgentWorkDisposition::Succeeded));
        }
        for view in views {
            assert!(view.close());
        }
        pump(queue, shell, || views.iter().all(|view| view.is_closed()));
    };
    // Two reads close cleanly, then the same work reads once more.
    let first = [attach(), attach()];
    settle(&first, &queue, &mut shell);
    let later = [attach()];
    settle(&later, &queue, &mut shell);
    assert_eq!(factories.load(Ordering::Acquire), 3);
    for server in servers.lock().unwrap().drain(..) {
        server.join().unwrap();
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
    for native in natives {
        native.join();
        assert_eq!(native.destructions.load(Ordering::Acquire), 1);
        assert!(native.global_sealed.load(Ordering::Acquire));
    }
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
