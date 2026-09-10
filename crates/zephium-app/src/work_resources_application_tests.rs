//! Real Store acknowledgements held independently of committed durable facts.
use super::*;
use crate::work_resources::application::*;

type Held = (
    Result<AgentWorkJournalReply, AgentWorkJournalError>,
    AgentWorkJournalCompletion,
);
type HeldArtifact = (
    Result<AgentWorkArtifactReply, AgentWorkJournalError>,
    AgentWorkArtifactCompletion,
);
struct GatedStore {
    store: Arc<zephium_store::SqliteStore>,
    hold: Arc<AtomicU8>,
    pending: Arc<Mutex<Option<Held>>>,
    pending_artifact: Arc<Mutex<Option<HeldArtifact>>>,
    refuse_artifact_read: AtomicBool,
    wakes: Arc<AtomicUsize>,
    held_signal: Arc<Mutex<Option<mpsc::SyncSender<()>>>>,
}
impl AgentAuditPort for GatedStore {
    fn append(
        &self,
        delivery: AgentAuditDelivery,
        completion: AgentAuditCompletion,
    ) -> AgentAuditDispatch {
        self.store.append(delivery, completion)
    }
}
impl AgentWorkJournalPort for GatedStore {
    fn artifact(
        &self,
        request: AgentWorkArtifactRequest,
        completion: AgentWorkArtifactCompletion,
    ) -> Result<(), AgentWorkJournalError> {
        if matches!(request, AgentWorkArtifactRequest::Read { .. })
            && self.refuse_artifact_read.load(Ordering::Acquire)
        {
            return Err(AgentWorkJournalError::Unavailable);
        }
        let held = matches!(request, AgentWorkArtifactRequest::Publish(_))
            && self.hold.load(Ordering::Acquire) == AgentWorkDisposition::Succeeded as u8;
        let pending = self.pending_artifact.clone();
        self.store.artifact(
            request,
            Box::new(move |reply| {
                if held {
                    assert!(pending
                        .lock()
                        .unwrap()
                        .replace((reply, completion))
                        .is_none());
                } else {
                    completion(reply);
                }
            }),
        )
    }
    fn dispatch(
        &self,
        request: AgentWorkJournalRequest,
        completion: AgentWorkJournalCompletion,
    ) -> Result<(), AgentWorkJournalError> {
        let held = match &request {
            AgentWorkJournalRequest::CompareAndSet(mutation) => {
                self.hold.load(Ordering::Acquire) == mutation.next().disposition() as u8
            }
            _ => false,
        };
        let pending = self.pending.clone();
        let held_signal = self.held_signal.clone();
        self.store.dispatch(
            request,
            Box::new(move |reply| {
                if held {
                    assert!(pending
                        .lock()
                        .unwrap()
                        .replace((reply, completion))
                        .is_none());
                    if let Some(signal) = held_signal.lock().unwrap().as_ref() {
                        signal.send(()).unwrap();
                    }
                } else {
                    completion(reply);
                }
            }),
        )
    }
}
impl GatedStore {
    fn release(&self, next: u8) {
        self.hold.store(next, Ordering::Release);
        let (reply, completion) = self.pending.lock().unwrap().take().unwrap();
        completion(reply);
    }
}
fn child(test: &str) -> bool {
    const CHILD: &str = "ZEPHIUM_RETAINED_ADMISSION_CHILD";
    if std::env::var_os(CHILD).is_some() {
        return false;
    }
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("work_resources::controller::tests::application_tests::{test}"),
            "--nocapture",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "retained admission child: {result:?}"
    );
    true
}

fn coordinator(directory: &std::path::Path) -> (RetainedWork, Arc<Native>, Arc<GatedStore>) {
    coordinator_with_wake(directory, Arc::new(|| true))
}

fn coordinator_with_wake(
    directory: &std::path::Path,
    wake: WakeApplication,
) -> (RetainedWork, Arc<Native>, Arc<GatedStore>) {
    coordinator_with_storage(directory, wake, ContextProfileStorageClass::Ephemeral)
}

fn coordinator_with_storage(
    directory: &std::path::Path,
    wake: WakeApplication,
    storage: ContextProfileStorageClass,
) -> (RetainedWork, Arc<Native>, Arc<GatedStore>) {
    let store = Arc::new(GatedStore {
        store: Arc::new(zephium_store::SqliteStore::open(directory).unwrap()),
        hold: Arc::new(AtomicU8::new(0)),
        pending: Arc::new(Mutex::new(None)),
        pending_artifact: Arc::new(Mutex::new(None)),
        refuse_artifact_read: AtomicBool::new(false),
        wakes: Arc::new(AtomicUsize::new(0)),
        held_signal: Arc::new(Mutex::new(None)),
    });
    let wakes = store.wakes.clone();
    let (owner, native, resource) = construct_fixture_with_wake(
        storage,
        WorkBrowserDocumentPolicy::Exact,
        None,
        Arc::new(move || {
            wakes.fetch_add(1, Ordering::AcqRel);
            wake()
        }),
    );
    native.allow_global_shutdown.store(true, Ordering::Release);
    if storage == ContextProfileStorageClass::Durable {
        use zephium_core::{
            ports::store::Store,
            profiles::ProfileKind,
            session::{PersistedProfile, PersistedSpace, SessionState},
        };
        let profile = resource.identity().profile();
        let space = zephium_core::ids::SpaceId::generate();
        store.store.save_session(SessionState {
            profiles: vec![PersistedProfile {
                id: profile,
                name: "Fixture".into(),
                kind: ProfileKind::Default,
            }],
            spaces: vec![PersistedSpace {
                id: space,
                profile,
                name: "Fixture".into(),
            }],
            items: vec![],
            active_space: Some(space),
            active_item: None,
            splits: None,
            recently_closed: vec![],
        });
        assert!(store.store.flush());
    }
    let work = RetainedWork::new(owner, resource, store.clone(), store.clone())
        .unwrap_or_else(|_| panic!("original owner and Store join"));
    (work, native, store)
}

struct TaskExitGate {
    task: AgentWorkExtractionTask,
    entered: mpsc::SyncSender<()>,
    release: mpsc::Receiver<()>,
}
impl AgentWorkTask for TaskExitGate {
    fn extraction_schema(&self) -> Option<&SemanticExtractionSchema> {
        self.task.extraction_schema()
    }
    fn accept_extraction(
        &mut self,
        result: &SemanticExtractionResult<'_>,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        self.task.accept_extraction(result)
    }
    fn evaluate(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        self.task.evaluate(observation)
    }
    fn assess(
        &self,
        action: &SemanticPreparedAction,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        self.task.assess(action)
    }
    fn attest_account(
        &self,
        context: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        self.task.attest_account(context, now)
    }
}
impl Drop for TaskExitGate {
    fn drop(&mut self) {
        // The common controller publishes Terminal, then drops its task, and
        // only afterwards can the original worker publish completion.
        self.entered.send(()).unwrap();
        self.release.recv_timeout(Duration::from_secs(5)).unwrap();
    }
}

#[test]
fn worker_exit_wakes_application_after_last_progress_wake_was_consumed() {
    if child("worker_exit_wakes_application_after_last_progress_wake_was_consumed") {
        return;
    }
    worker_exit_wake(false, false);
}

#[test]
fn blocking_shutdown_joins_original_worker_store_and_native_wakes_without_shell_commands() {
    if child(
        "blocking_shutdown_joins_original_worker_store_and_native_wakes_without_shell_commands",
    ) {
        return;
    }
    worker_exit_wake(true, false);
}

#[test]
fn shutdown_passes_its_deadline_through_post_completion_scoped_thread_join() {
    if child("shutdown_passes_its_deadline_through_post_completion_scoped_thread_join") {
        return;
    }
    worker_exit_wake(false, true);
}

fn worker_exit_wake(blocking_shutdown: bool, post_completion: bool) {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let (wake, wakes) = mpsc::sync_channel(1);
    let hold_completion = Arc::new(AtomicBool::new(false));
    let holding_completion = hold_completion.clone();
    let (completion_entered, completion_entering) = mpsc::sync_channel(1);
    let (completion_release, completion_releasing) = mpsc::sync_channel(1);
    let completion_releasing = Mutex::new(completion_releasing);
    let (mut work, native, store) = coordinator_with_wake(
        directory.path(),
        Arc::new(move || {
            if holding_completion.swap(false, Ordering::AcqRel) {
                completion_entered.send(()).unwrap();
                completion_releasing
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap();
            }
            match wake.try_send(()) {
                Ok(()) | Err(mpsc::TrySendError::Full(())) => true,
                Err(mpsc::TrySendError::Disconnected(())) => false,
            }
        }),
    );
    // No periodic/blind polling. Every poll below is driven by the exact
    // original application wake, with timeout used only as test failure.
    while !work.ready() {
        wakes
            .recv_timeout(Duration::from_secs(2))
            .expect("Store claim wake");
        work.poll(now());
    }
    let (entered, entering) = mpsc::sync_channel(1);
    let (release, releasing) = mpsc::sync_channel(1);
    let (server_tx, server_rx) = mpsc::sync_channel(1);
    let request = ActorRequest {
        run: ContextRunId::generate(),
        deadline: AgentPolicyInstant::from_millis(600_002),
        prepare: Box::new(move |browser, audit| {
            let input = input(browser.binding(), Arc::new(Clock(AtomicU64::new(2))));
            let (transport, server) =
                fixture_provider_responses(vec![response_stream(1), response_stream(2)]);
            server_tx.send(server).unwrap();
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
                Box::new(TaskExitGate {
                    task: task(),
                    entered,
                    release: releasing,
                }),
            )
        }),
    };
    assert!(work.submit(request, now()).is_ok());
    let mut terminal = false;
    while !terminal {
        wakes
            .recv_timeout(Duration::from_secs(2))
            .expect("original progress wake");
        work.poll(now());
        while let Some(event) = work.take_event() {
            terminal |= matches!(event.kind(), AgentWorkEventKind::Terminal);
        }
    }
    entering.recv_timeout(Duration::from_secs(2)).unwrap();
    native.join();
    assert_eq!(
        server_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .join()
            .unwrap(),
        2
    );
    while wakes.try_recv().is_ok() {
        work.poll(now());
        drain_events(&mut work);
    }
    assert_eq!(work.phase(), AdmissionPhase::Running);
    assert_eq!(
        work.record().unwrap().disposition(),
        AgentWorkDisposition::Running
    );
    assert!(
        work.next_deadline().is_none(),
        "no timer may hide the missing completion wake"
    );
    if post_completion {
        // All progress/native wakes have been consumed. The next notification
        // is completion.mark_stopped's own Waker, invoked after stopped=true
        // but before worker_main/Tokio/thread-local teardown can return.
        hold_completion.store(true, Ordering::Release);
        release.send(()).unwrap();
        completion_entering
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        let result = std::thread::scope(|scope| {
            let (finished, finishing) = mpsc::channel();
            let work = &mut work;
            scope.spawn(move || {
                let result = work.shutdown_until(
                    &Clock(AtomicU64::new(2)),
                    Instant::now() + Duration::from_millis(10),
                );
                let _ = finished.send(result);
            });
            let result = finishing.recv_timeout(Duration::from_millis(70));
            completion_release.send(()).unwrap();
            result
        });
        assert!(
            matches!(result, Ok(false)),
            "caller deadline must cap scoped join, not a fresh 100 ms window: {result:?}"
        );
        assert_eq!(work.phase(), AdmissionPhase::Uncertain);
        return;
    }
    if blocking_shutdown {
        assert!(!work.shutdown_until(
            &Clock(AtomicU64::new(2)),
            Instant::now() + Duration::from_millis(20)
        ));
        assert!(
            !work.resource_destroyed(),
            "unfinished original worker stays owned"
        );
        assert_eq!(native.destructions.load(Ordering::Acquire), 0);
        let (held, holding) = mpsc::sync_channel(1);
        *store.held_signal.lock().unwrap() = Some(held);
        store
            .hold
            .store(AgentWorkDisposition::Succeeded as u8, Ordering::Release);
        let deliver = std::thread::spawn(move || {
            release.send(()).unwrap();
            holding
                .recv_timeout(Duration::from_secs(2))
                .expect("original terminal CAS callback");
            store.release(0);
        });
        assert!(work.shutdown_until(
            &Clock(AtomicU64::new(2)),
            Instant::now() + Duration::from_secs(3)
        ));
        deliver.join().unwrap();
        assert!(work.resource_destroyed());
        assert_eq!(native.global_audits.load(Ordering::Acquire), 1);
        assert_eq!(
            work.record().unwrap().disposition(),
            AgentWorkDisposition::Succeeded
        );
        assert!(work.take_extraction().is_some());
        return;
    }
    release.send(()).unwrap();
    while work.phase() != AdmissionPhase::Terminal {
        wakes
            .recv_timeout(Duration::from_secs(2))
            .expect("worker completion or terminal Store ACK wake");
        work.poll(now());
        drain_events(&mut work);
    }
    assert!(work.take_extraction().is_some());
    work.begin_shutdown();
    while !work.poll_shutdown(now()).unwrap() {
        wakes
            .recv_timeout(Duration::from_secs(2))
            .expect("original native seal/audit wake");
    }
}

type Servers = Arc<Mutex<Vec<std::thread::JoinHandle<usize>>>>;

#[test]
fn retained_result_requires_atomic_ack_and_remains_readable_after_handoff() {
    if child("retained_result_requires_atomic_ack_and_remains_readable_after_handoff") {
        return;
    }
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let (mut work, native, store) = coordinator_with_storage(
        directory.path(),
        Arc::new(|| true),
        ContextProfileStorageClass::Durable,
    );
    let servers = Servers::default();
    poll_until(&mut work, RetainedWork::ready);
    store
        .hold
        .store(AgentWorkDisposition::Succeeded as u8, Ordering::Release);
    assert!(work
        .submit(
            request_with_result(
                ContextRunId::generate(),
                vec![response_stream(1), response_stream(2)],
                servers.clone(),
                true
            ),
            now()
        )
        .is_ok());
    poll_until(&mut work, |_| {
        store.pending_artifact.lock().unwrap().is_some()
    });
    assert_eq!(work.phase(), AdmissionPhase::Closing);
    assert!(work.take_extraction().is_none());
    assert!(work.artifact().is_none());
    let (reply, completion) = store.pending_artifact.lock().unwrap().take().unwrap();
    let AgentWorkArtifactReply::Published { record, descriptor } = reply.unwrap() else {
        panic!("atomic publication");
    };
    completion(Ok(AgentWorkArtifactReply::Published { record, descriptor }));
    poll_until(&mut work, |work| work.phase() == AdmissionPhase::Terminal);
    assert_eq!(work.artifact(), Some(descriptor));
    assert!(work.take_extraction().is_some());
    assert!(work.take_extraction().is_none());
    assert!(work.read_artifact(record));
    assert!(!work.read_artifact(record), "one archived read at a time");
    poll_until(&mut work, |work| work.artifact_read().is_some());
    let archived = work.take_archived_extraction().unwrap();
    assert_eq!(archived.descriptor(), descriptor);
    assert_eq!(archived.trust(), SemanticExtractionTrust::ModelMapped);
    assert_eq!(archived.fields()[0].name(), "label");
    assert_eq!(work.record(), Some(record));
    store.refuse_artifact_read.store(true, Ordering::Release);
    assert!(work.read_artifact(record));
    poll_until(&mut work, |work| work.artifact_read().is_some());
    assert_eq!(
        work.artifact_read(),
        Some(Err(AgentWorkJournalError::Unavailable))
    );
    assert_eq!(work.phase(), AdmissionPhase::Terminal);
    assert_eq!(
        work.record(),
        Some(record),
        "a failed archive read cannot rewrite a completed run"
    );
    assert_eq!(
        native.reads.load(Ordering::Acquire),
        1,
        "archive read never dispatches a browser read"
    );
    for server in servers.lock().unwrap().drain(..) {
        assert_eq!(server.join().unwrap(), 2);
    }
    native.join();
    assert!(work.shutdown_until(
        &Clock(AtomicU64::new(2)),
        Instant::now() + Duration::from_secs(2)
    ));
}

fn request(run: ContextRunId, responses: Vec<String>, servers: Servers) -> ActorRequest {
    request_with_result(run, responses, servers, false)
}

fn request_with_result(
    run: ContextRunId,
    responses: Vec<String>,
    servers: Servers,
    persist: bool,
) -> ActorRequest {
    ActorRequest {
        run,
        deadline: AgentPolicyInstant::from_millis(600_002),
        prepare: Box::new(move |browser, audit| {
            let input = input(browser.binding(), Arc::new(Clock(AtomicU64::new(2))));
            let input = if persist {
                input.persist_extraction_result().unwrap()
            } else {
                input
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
                Box::new(task()),
            )
        }),
    }
}
fn poll_until(work: &mut RetainedWork, mut predicate: impl FnMut(&RetainedWork) -> bool) {
    wait_until(|| {
        work.poll(now());
        predicate(work)
    });
}
fn drain_events(work: &mut RetainedWork) {
    while work.take_event().is_some() {}
}

fn start_held_read_recovery(work: &mut RetainedWork, native: &Arc<Native>, servers: &Servers) {
    native.hold_read.store(true, Ordering::Release);
    assert!(work
        .submit(
            request(ContextRunId::generate(), Vec::new(), servers.clone()),
            now(),
        )
        .is_ok());
    poll_until(work, |_| native.read.lock().unwrap().is_some());
    native
        .reporters
        .lock()
        .unwrap()
        .values()
        .next()
        .expect("retained resource health reporter")
        .invalidate();
}

#[test]
fn recovery_classification_acknowledgement_gates_global_native_shutdown() {
    if child("recovery_classification_acknowledgement_gates_global_native_shutdown") {
        return;
    }
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let (mut work, native, store) = coordinator(directory.path());
    let servers = Servers::default();
    poll_until(&mut work, RetainedWork::ready);
    store.hold.store(
        AgentWorkDisposition::RecoveryRequired as u8,
        Ordering::Release,
    );
    start_held_read_recovery(&mut work, &native, &servers);
    poll_until(&mut work, |_| store.pending.lock().unwrap().is_some());
    let running = work.record().unwrap();
    assert_eq!(running.disposition(), AgentWorkDisposition::Running);
    assert_eq!(running.debt(), AgentWorkDebt::UNKNOWN);
    assert!(work.take_extraction().is_none());

    let (read, callback) = native.read.lock().unwrap().take().unwrap();
    Native::read_result(read, callback);
    native.join();
    work.begin_shutdown();
    assert!(!work.poll_shutdown(now()).unwrap());
    assert!(work.resource_destroyed());
    assert_eq!(
        native.global_audits.load(Ordering::Acquire),
        0,
        "global native proof cannot precede the original recovery CAS ACK"
    );

    store.release(0);
    let deadline = Instant::now() + Duration::from_secs(2);
    assert!(!work.shutdown_until(&Clock(AtomicU64::new(2)), deadline));
    assert!(
        Instant::now() < deadline,
        "classified recovery has no clean-state notification to wait for"
    );
    assert_eq!(work.phase(), AdmissionPhase::Uncertain);
    assert_eq!(
        work.record().unwrap(),
        AgentWorkJournalMutation::transition(running, AgentWorkDisposition::RecoveryRequired,)
            .unwrap()
            .next()
    );
    assert_eq!(work.record().unwrap().debt(), AgentWorkDebt::UNKNOWN);
    assert!(work.take_extraction().is_none());
    assert_eq!(native.global_audits.load(Ordering::Acquire), 1);
    assert!(native.global_sealed.load(Ordering::Acquire));
    for server in servers.lock().unwrap().drain(..) {
        assert_eq!(server.join().unwrap(), 0);
    }
}

#[test]
fn recovery_shutdown_waits_for_original_late_read_then_returns_unclean_promptly() {
    if child("recovery_shutdown_waits_for_original_late_read_then_returns_unclean_promptly") {
        return;
    }
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let (mut work, native, _) = coordinator(directory.path());
    let servers = Servers::default();
    poll_until(&mut work, RetainedWork::ready);
    start_held_read_recovery(&mut work, &native, &servers);
    poll_until(&mut work, |work| {
        work.record()
            .is_some_and(|record| record.disposition() == AgentWorkDisposition::RecoveryRequired)
    });
    assert_eq!(work.record().unwrap().debt(), AgentWorkDebt::UNKNOWN);

    work.begin_shutdown();
    assert!(!work.poll_shutdown(now()).unwrap());
    assert!(
        !work.resource_destroyed(),
        "an outstanding original read callback must remain physically owned"
    );
    assert_eq!(native.destructions.load(Ordering::Acquire), 1);
    assert_eq!(native.global_audits.load(Ordering::Acquire), 0);

    let (read, callback) = native.read.lock().unwrap().take().unwrap();
    let deliver = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(40));
        Native::read_result(read, callback);
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    assert!(!work.shutdown_until(&Clock(AtomicU64::new(2)), deadline));
    deliver.join().unwrap();
    native.join();
    assert!(
        Instant::now() < deadline,
        "the returned callback permits cleanup but cannot turn Recovery clean"
    );
    assert!(work.resource_destroyed());
    assert_eq!(native.destructions.load(Ordering::Acquire), 1);
    assert_eq!(native.global_audits.load(Ordering::Acquire), 1);
    assert_eq!(
        work.record().unwrap().disposition(),
        AgentWorkDisposition::RecoveryRequired
    );
    assert_eq!(work.record().unwrap().debt(), AgentWorkDebt::UNKNOWN);
    assert!(work.take_extraction().is_none());
    for server in servers.lock().unwrap().drain(..) {
        assert_eq!(server.join().unwrap(), 0);
    }
}

#[test]
fn drained_failed_mapping_projects_original_typed_failure_through_terminal_ack() {
    if child("drained_failed_mapping_projects_original_typed_failure_through_terminal_ack") {
        return;
    }
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    failed_mapping_projection(
        "sk-fixture-not-a-secret",
        SemanticExtractionError::SecretValue,
    );
}

#[test]
fn multiline_mapping_projects_invalid_text_through_terminal_ack() {
    if child("multiline_mapping_projects_invalid_text_through_terminal_ack") {
        return;
    }
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    // Two levels of JSON string encoding: SSE delta, then mapped value.
    failed_mapping_projection(
        "First.\\\\n\\\\nSecond.",
        SemanticExtractionError::InvalidText,
    );
}

fn failed_mapping_projection(value: &str, error: SemanticExtractionError) {
    use zephium_agent_controller::AgentBrowserProviderError;
    let directory = tempfile::tempdir().unwrap();
    let (mut work, native, store) = coordinator(directory.path());
    let servers = Servers::default();
    poll_until(&mut work, RetainedWork::ready);
    store
        .hold
        .store(AgentWorkDisposition::Failed as u8, Ordering::Release);
    let mapping = response_stream(2).replace("Fixture result", value);
    assert!(work
        .submit(
            request(
                ContextRunId::generate(),
                vec![response_stream(1), mapping],
                servers.clone()
            ),
            now()
        )
        .is_ok());
    poll_until(&mut work, |work| {
        work.phase() == AdmissionPhase::Closing && store.pending.lock().unwrap().is_some()
    });
    let expected = AgentWorkFailure::Browser(AgentBrowserProviderError::Extraction(
        AgentProviderExtractionOutputError::Extraction(error),
    ));
    assert_eq!(work.phase(), AdmissionPhase::Closing);
    assert_eq!(work.failures(), (Some(expected), None));
    assert!(work.take_extraction().is_none());
    store.release(0);
    poll_until(&mut work, |work| work.phase() == AdmissionPhase::Terminal);
    assert_eq!(
        work.record().unwrap().disposition(),
        AgentWorkDisposition::Failed
    );
    assert_eq!(work.record().unwrap().debt(), AgentWorkDebt::NONE);
    assert_eq!(work.failures(), (Some(expected), None));
    assert!(work.take_extraction().is_none());
    assert!(!native.global_sealed.load(Ordering::Acquire));
    assert_eq!(native.destructions.load(Ordering::Acquire), 0);
    for server in servers.lock().unwrap().drain(..) {
        assert_eq!(server.join().unwrap(), 2);
    }
    assert!(work.shutdown_until(
        &Clock(AtomicU64::new(2)),
        Instant::now() + Duration::from_secs(2)
    ));
}

#[test]
fn original_acknowledgements_gate_retained_workers_results_and_successors() {
    if child("original_acknowledgements_gate_retained_workers_results_and_successors") {
        return;
    }
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let (mut work, native, store) = coordinator(directory.path());
    let servers = Servers::default();
    let first = ContextRunId::generate();
    let mut second = Some(request(ContextRunId::generate(), vec![], servers.clone()));
    assert_eq!(work.phase(), AdmissionPhase::Loading);
    assert!(!work.ready());
    poll_until(&mut work, |work| work.ready());
    let wake_before_acquire = store.wakes.load(Ordering::Acquire);
    store
        .hold
        .store(AgentWorkDisposition::Admitted as u8, Ordering::Release);
    assert!(work
        .submit(
            request(
                first,
                vec![response_stream(1), response_stream(2)],
                servers.clone()
            ),
            now()
        )
        .is_ok());
    poll_until(&mut work, |_| store.pending.lock().unwrap().is_some());
    assert_eq!(work.phase(), AdmissionPhase::Admitting);
    assert_eq!(native.acquisitions.load(Ordering::Acquire), 1);
    assert!(
        store.wakes.load(Ordering::Acquire) > wake_before_acquire,
        "original resource lane was rearmed before acquisition"
    );
    assert_eq!(native.reads.load(Ordering::Acquire), 0);
    assert!(work.take_extraction().is_none());
    second = Some(work.submit(second.take().unwrap(), now()).err().unwrap());
    assert_eq!(native.acquisitions.load(Ordering::Acquire), 1);

    let wake_before_ack = store.wakes.load(Ordering::Acquire);
    store.release(AgentWorkDisposition::Running as u8);
    assert!(
        store.wakes.load(Ordering::Acquire) > wake_before_ack,
        "original Store ACK uses the same rearmed application wake"
    );
    poll_until(&mut work, |_| store.pending.lock().unwrap().is_some());
    assert_eq!(work.phase(), AdmissionPhase::Starting);
    assert_eq!(
        native.reads.load(Ordering::Acquire),
        0,
        "durable Running is not its ACK"
    );
    second = Some(work.submit(second.take().unwrap(), now()).err().unwrap());
    store.release(AgentWorkDisposition::Succeeded as u8);
    poll_until(&mut work, |_| store.pending.lock().unwrap().is_some());
    native.join();
    assert_eq!(work.phase(), AdmissionPhase::Closing);
    assert_eq!(native.reads.load(Ordering::Acquire), 1);
    assert_eq!(
        work.record().unwrap().disposition(),
        AgentWorkDisposition::Running
    );
    assert!(
        work.take_extraction().is_none(),
        "result cannot precede terminal ACK"
    );
    second = Some(work.submit(second.take().unwrap(), now()).err().unwrap());
    assert_eq!(
        native.acquisitions.load(Ordering::Acquire),
        1,
        "native reuse is not B admission"
    );
    let (reply, _) = store
        .pending
        .lock()
        .unwrap()
        .as_ref()
        .map(|(reply, _)| {
            let Ok(AgentWorkJournalReply::Record(Some(record))) = reply else {
                panic!("original committed terminal");
            };
            (*record, ())
        })
        .unwrap();
    assert_eq!(reply.disposition(), AgentWorkDisposition::Succeeded);
    store.release(0);
    poll_until(&mut work, |work| work.phase() == AdmissionPhase::Terminal);
    second = Some(work.submit(second.take().unwrap(), now()).err().unwrap());
    drain_events(&mut work);
    assert!(!work.ready(), "unread result stays owned");
    assert_eq!(work.take_extraction().unwrap().stats().source_edges(), 1);
    assert!(work.take_extraction().is_none());
    assert!(work.ready());
    assert!(
        work.submit(request(first, vec![], servers.clone()), now())
            .is_err(),
        "old actor ID is not resumed"
    );

    native.hold_read.store(true, Ordering::Release);
    assert!(work.submit(second.take().unwrap(), now()).is_ok());
    poll_until(&mut work, |_| native.read.lock().unwrap().is_some());
    assert_eq!(
        native.reads.load(Ordering::Acquire),
        2,
        "fresh successor read, no A snapshot replay"
    );
    work.cancel();
    let (read, callback) = native.read.lock().unwrap().take().unwrap();
    Native::read_result(read, callback);
    poll_until(&mut work, |work| work.phase() == AdmissionPhase::Terminal);
    native.join();
    assert_eq!(
        work.record().unwrap().disposition(),
        AgentWorkDisposition::Cancelled
    );
    assert_eq!(work.record().unwrap().debt(), AgentWorkDebt::NONE);
    assert!(work.take_extraction().is_none());
    drain_events(&mut work);
    assert!(work.ready());
    assert_eq!(native.acquisitions.load(Ordering::Acquire), 2);
    assert_eq!(native.destructions.load(Ordering::Acquire), 0);
    assert_eq!(
        native.global_audits.load(Ordering::Acquire),
        0,
        "the retained page cannot enter global shutdown between actors"
    );
    work.begin_shutdown();
    wait_until(|| work.poll_shutdown(now()).unwrap());
    assert!(!work.ready());
    assert_eq!(native.destructions.load(Ordering::Acquire), 1);
    assert_eq!(native.global_audits.load(Ordering::Acquire), 1);
    let calls: Vec<_> = servers
        .lock()
        .unwrap()
        .drain(..)
        .map(|server| server.join().unwrap())
        .collect();
    assert_eq!(calls, [2, 0]);
}

#[test]
fn mismatched_original_acknowledgement_cannot_start_worker_and_cleanup_stays_owned() {
    if child("mismatched_original_acknowledgement_cannot_start_worker_and_cleanup_stays_owned") {
        return;
    }
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let (mut work, native, store) = coordinator(directory.path());
    let servers = Servers::default();
    poll_until(&mut work, |work| work.ready());
    store
        .hold
        .store(AgentWorkDisposition::Running as u8, Ordering::Release);
    assert!(work
        .submit(
            request(ContextRunId::generate(), vec![], servers.clone()),
            now()
        )
        .is_ok());
    poll_until(&mut work, |_| store.pending.lock().unwrap().is_some());
    let (reply, completion) = store.pending.lock().unwrap().take().unwrap();
    let Ok(AgentWorkJournalReply::Record(Some(record))) = reply else {
        panic!("Running ACK");
    };
    let mut wrong = *record.as_bytes();
    wrong[64] ^= 1;
    completion(Ok(AgentWorkJournalReply::Record(AgentWorkRecord::decode(
        wrong,
    ))));
    work.poll(now());
    assert_eq!(work.phase(), AdmissionPhase::Uncertain);
    assert!(!work.ready());
    assert_eq!(native.reads.load(Ordering::Acquire), 0);
    assert!(work
        .submit(
            request(ContextRunId::generate(), vec![], servers.clone()),
            now()
        )
        .is_err());
    assert_eq!(native.acquisitions.load(Ordering::Acquire), 1);
    work.begin_shutdown();
    wait_until(|| {
        let settled = work.poll_shutdown(now()).unwrap();
        assert!(!settled);
        work.resource_destroyed()
    });
    assert_eq!(native.destructions.load(Ordering::Acquire), 1);
    assert!(!work.ready(), "cleanup does not clear durable uncertainty");
    store.hold.store(0, Ordering::Release);
    assert!(work.reconcile());
    poll_until(&mut work, |work| {
        work.record()
            .is_some_and(|record| record.disposition() == AgentWorkDisposition::FailedClosed)
    });
    assert!(
        !work.poll_shutdown(now()).unwrap(),
        "pre-start recovery owner was not a scoped drain"
    );
    assert!(!work.reconcile(), "original exact CAS has now settled");
    for server in servers.lock().unwrap().drain(..) {
        assert_eq!(server.join().unwrap(), 0);
    }
}

#[test]
fn timed_out_terminal_acknowledgement_never_reopens_retained_execution() {
    if child("timed_out_terminal_acknowledgement_never_reopens_retained_execution") {
        return;
    }
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let (mut work, native, store) = coordinator(directory.path());
    let servers = Servers::default();
    poll_until(&mut work, |work| work.ready());
    store
        .hold
        .store(AgentWorkDisposition::Succeeded as u8, Ordering::Release);
    assert!(work
        .submit(
            request(
                ContextRunId::generate(),
                vec![response_stream(1), response_stream(2)],
                servers.clone()
            ),
            now()
        )
        .is_ok());
    poll_until(&mut work, |_| store.pending.lock().unwrap().is_some());
    native.join();
    poll_until(&mut work, |work| work.phase() == AdmissionPhase::Uncertain);
    assert!(!work.ready());
    assert!(work.take_extraction().is_none());
    assert!(work
        .submit(
            request(ContextRunId::generate(), vec![], servers.clone()),
            now()
        )
        .is_err());
    assert_eq!(native.acquisitions.load(Ordering::Acquire), 1);
    work.begin_shutdown();
    wait_until(|| {
        let settled = work.poll_shutdown(now()).unwrap();
        assert!(!settled);
        work.resource_destroyed()
    });
    assert_eq!(native.destructions.load(Ordering::Acquire), 1);
    store.hold.store(0, Ordering::Release);
    assert_eq!(
        native.global_audits.load(Ordering::Acquire),
        0,
        "native global seal cannot precede the unresolved original terminal ACK"
    );
    assert!(work.reconcile());
    poll_until(&mut work, |work| work.phase() == AdmissionPhase::Terminal);
    assert!(
        !work.ready(),
        "reconciliation never clears the sticky execution stop"
    );
    assert!(work.take_extraction().is_none());
    wait_until(|| work.poll_shutdown(now()).unwrap());
    store.release(0); // The old callback cannot select/rewrite the reconciled lane.
    work.poll(now());
    assert!(work.poll_shutdown(now()).unwrap());
    for server in servers.lock().unwrap().drain(..) {
        assert_eq!(server.join().unwrap(), 2);
    }
}

#[test]
fn shutdown_deadline_caps_native_retry_without_consuming_the_original_coordinator() {
    if child("shutdown_deadline_caps_native_retry_without_consuming_the_original_coordinator") {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let (mut work, native, _) = coordinator(directory.path());
    poll_until(&mut work, |work| work.ready());
    native.global_queued_debt.store(1, Ordering::Release);
    let clock = Clock(AtomicU64::new(2));
    let deadline = Instant::now() + Duration::from_millis(40);
    assert!(!work.shutdown_until(&clock, deadline));
    assert!(Instant::now() >= deadline);
    assert!(Instant::now() < deadline + Duration::from_secs(1));
    assert!(work.resource_destroyed());
    assert!(!work.ready());
    assert_eq!(native.global_audits.load(Ordering::Acquire), 1);
    assert!(work.next_deadline().unwrap() > deadline);
    native.global_queued_debt.store(0, Ordering::Release);
    assert!(work.shutdown_until(&clock, Instant::now() + Duration::from_secs(2)));
    assert_eq!(native.global_audits.load(Ordering::Acquire), 2);
}

#[test]
fn shutdown_timeout_retains_the_exact_pending_native_terminal() {
    if child("shutdown_timeout_retains_the_exact_pending_native_terminal") {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let (mut work, native, _) = coordinator(directory.path());
    poll_until(&mut work, |work| work.ready());
    native.hold_global_audit.store(true, Ordering::Release);
    let clock = Clock(AtomicU64::new(2));
    assert!(!work.shutdown_until(&clock, Instant::now() + Duration::from_millis(30)));
    assert!(native.pending_global_audit.lock().unwrap().is_some());
    assert!(work.resource_destroyed());
    assert_eq!(native.global_audits.load(Ordering::Acquire), 1);
    native.release_global_audit();
    assert!(work.shutdown_until(&clock, Instant::now() + Duration::from_secs(1)));
    assert_eq!(native.global_audits.load(Ordering::Acquire), 1);
}

#[test]
fn shutdown_timeout_retains_the_original_terminal_store_acknowledgement() {
    if child("shutdown_timeout_retains_the_original_terminal_store_acknowledgement") {
        return;
    }
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let (mut work, native, store) = coordinator(directory.path());
    let servers = Servers::default();
    poll_until(&mut work, |work| work.ready());
    store
        .hold
        .store(AgentWorkDisposition::Succeeded as u8, Ordering::Release);
    assert!(work
        .submit(
            request(
                ContextRunId::generate(),
                vec![response_stream(1), response_stream(2)],
                servers.clone()
            ),
            now()
        )
        .is_ok());
    poll_until(&mut work, |_| store.pending.lock().unwrap().is_some());
    native.join();
    let clock = Clock(AtomicU64::new(2));
    assert!(!work.shutdown_until(&clock, Instant::now() + Duration::from_millis(30)));
    assert!(store.pending.lock().unwrap().is_some());
    assert!(work.resource_destroyed());
    assert_eq!(native.global_audits.load(Ordering::Acquire), 0);
    assert_eq!(
        work.record().unwrap().disposition(),
        AgentWorkDisposition::Running
    );
    assert!(!work.ready());
    assert!(work.take_extraction().is_none());
    native.hold_global_audit.store(true, Ordering::Release);
    let deliver = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        store.release(0);
    });
    let deadline = Instant::now() + Duration::from_millis(120);
    assert!(!work.shutdown_until(&clock, deadline));
    deliver.join().unwrap();
    assert!(Instant::now() >= deadline);
    assert!(Instant::now() < deadline + Duration::from_secs(1));
    assert_eq!(native.global_audits.load(Ordering::Acquire), 1);
    assert_eq!(
        work.record().unwrap().disposition(),
        AgentWorkDisposition::Succeeded
    );
    native.release_global_audit();
    assert!(work.shutdown_until(&clock, Instant::now() + Duration::from_secs(1)));
    assert_eq!(
        work.record().unwrap().disposition(),
        AgentWorkDisposition::Succeeded
    );
    assert!(work.take_extraction().is_some());
    assert!(!work.ready());
    for server in servers.lock().unwrap().drain(..) {
        assert_eq!(server.join().unwrap(), 2);
    }
}

#[test]
fn expired_shutdown_deadline_stops_admission_without_dispatching_cleanup() {
    if child("expired_shutdown_deadline_stops_admission_without_dispatching_cleanup") {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let (mut work, native, _) = coordinator(directory.path());
    poll_until(&mut work, |work| work.ready());
    let clock = Clock(AtomicU64::new(2));
    assert!(!work.shutdown_until(&clock, Instant::now()));
    assert!(!work.ready());
    assert_eq!(native.destructions.load(Ordering::Acquire), 0);
    assert_eq!(native.global_audits.load(Ordering::Acquire), 0);
    assert!(work.shutdown_until(&clock, Instant::now() + Duration::from_secs(1)));
}

#[test]
fn shutdown_deadline_is_rechecked_after_the_policy_clock_returns() {
    if child("shutdown_deadline_is_rechecked_after_the_policy_clock_returns") {
        return;
    }
    struct DelayedClock;
    impl TerraControllerClock for DelayedClock {
        fn now(&self) -> Result<AgentPolicyInstant, TerraControllerClockError> {
            std::thread::sleep(Duration::from_millis(30));
            Ok(now())
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let (mut work, native, _) = coordinator(directory.path());
    poll_until(&mut work, |work| work.ready());
    assert!(!work.shutdown_until(&DelayedClock, Instant::now() + Duration::from_millis(10)));
    assert_eq!(native.destructions.load(Ordering::Acquire), 0);
    assert_eq!(native.global_audits.load(Ordering::Acquire), 0);
    assert!(work.shutdown_until(
        &Clock(AtomicU64::new(2)),
        Instant::now() + Duration::from_secs(1)
    ));
}

#[test]
fn local_resource_destruction_waits_for_original_global_native_zero_audit() {
    if child("local_resource_destruction_waits_for_original_global_native_zero_audit") {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let (mut work, native, _) = coordinator(directory.path());
    poll_until(&mut work, |work| work.ready());
    native.hold_global_audit.store(true, Ordering::Release);
    work.begin_shutdown();
    assert!(!work.poll_shutdown(now()).unwrap());
    assert!(work.resource_destroyed());
    assert_eq!(native.destructions.load(Ordering::Acquire), 1);
    assert_eq!(native.global_audits.load(Ordering::Acquire), 1);
    assert!(
        !work.poll_shutdown(now()).unwrap(),
        "scheduled/held native audit is not proof"
    );
    native.release_global_audit();
    assert!(work.poll_shutdown(now()).unwrap());
    assert!(work.poll_shutdown(now()).unwrap());
    assert_eq!(
        native.global_audits.load(Ordering::Acquire),
        1,
        "seal/proof are consumed once"
    );
}

#[test]
fn duplicate_native_zero_terminal_is_retained_before_shutdown_can_complete() {
    if child("duplicate_native_zero_terminal_is_retained_before_shutdown_can_complete") {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let (mut work, native, _) = coordinator(directory.path());
    poll_until(&mut work, |work| work.ready());
    native.hold_global_audit.store(true, Ordering::Release);
    work.begin_shutdown();
    assert!(!work.poll_shutdown(now()).unwrap());
    native.hold_global_audit.store(false, Ordering::Release);
    native.release_global_audit();
    native.global_audit(ContextResourceAuditId::new(1).unwrap(), true);
    assert!(!work.poll_shutdown(now()).unwrap());
    assert_eq!(work.phase(), AdmissionPhase::Uncertain);
    assert!(!work.poll_shutdown(now()).unwrap());
}

#[test]
fn synchronous_native_refusals_arm_retry_without_an_extra_poll_or_callback() {
    if child("synchronous_native_refusals_arm_retry_without_an_extra_poll_or_callback") {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let (wake, wakes) = mpsc::channel();
    let (mut work, native, _) =
        coordinator_with_wake(directory.path(), Arc::new(move || wake.send(()).is_ok()));
    poll_until(&mut work, |work| work.ready());
    native
        .synchronous_global_seal
        .store(true, Ordering::Release);
    native.reject_global_audit.store(true, Ordering::Release);
    work.begin_shutdown();
    assert!(!work.poll_shutdown(now()).unwrap());
    while wakes.try_recv().is_ok() {}
    let first = work
        .next_deadline()
        .expect("synchronous seal must arm retry immediately");
    std::thread::sleep(first.saturating_duration_since(Instant::now()));
    assert!(!work.poll_shutdown(now()).unwrap());
    assert!(
        wakes.try_recv().is_err(),
        "unsupported audit has no callback wake"
    );
    let second = work
        .next_deadline()
        .expect("synchronous audit refusal must rearm retry immediately");
    assert!(second > first);
    native.reject_global_audit.store(false, Ordering::Release);
    std::thread::sleep(second.saturating_duration_since(Instant::now()));
    assert!(!work.poll_shutdown(now()).unwrap());
    wakes
        .recv_timeout(Duration::from_secs(2))
        .expect("original audit terminal wakes owner");
    assert!(work.poll_shutdown(now()).unwrap());
    assert_eq!(native.global_audits.load(Ordering::Acquire), 1);
}

#[test]
fn synchronous_native_exhaustion_returns_without_an_unavailable_final_wake() {
    if child("synchronous_native_exhaustion_returns_without_an_unavailable_final_wake") {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let (mut work, native, _) = coordinator(directory.path());
    poll_until(&mut work, |work| work.ready());
    native
        .synchronous_global_seal
        .store(true, Ordering::Release);
    native.reject_global_audit.store(true, Ordering::Release);
    work.begin_shutdown();
    for attempt in 1..=8 {
        let result = work.poll_shutdown(now());
        if attempt == 8 {
            assert!(
                result.is_err(),
                "last synchronous refusal must fail in its dispatch poll"
            );
        } else {
            assert!(!result.unwrap());
            let retry = work
                .next_deadline()
                .expect("no callbacks exist; retry must be armed");
            std::thread::sleep(retry.saturating_duration_since(Instant::now()));
        }
    }
    assert!(work.next_deadline().is_none());
    assert_eq!(native.global_audits.load(Ordering::Acquire), 0);
    assert!(work.poll_shutdown(now()).is_err());
}

#[test]
fn retained_global_shutdown_reuses_nonzero_retry_cadence_and_exact_zero_predicate() {
    if child("retained_global_shutdown_reuses_nonzero_retry_cadence_and_exact_zero_predicate") {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let (mut work, native, _) = coordinator(directory.path());
    poll_until(&mut work, |work| work.ready());
    native.global_queued_debt.store(1, Ordering::Release);
    work.begin_shutdown();
    assert!(!work.poll_shutdown(now()).unwrap());
    assert!(!work.poll_shutdown(now()).unwrap());
    assert!(work.resource_destroyed());
    assert_eq!(native.global_audits.load(Ordering::Acquire), 1);
    let retry = work
        .next_deadline()
        .expect("existing native retry cadence joins application timer");
    assert!(!work.poll_shutdown(now()).unwrap());
    assert_eq!(
        native.global_audits.load(Ordering::Acquire),
        1,
        "no tight native audit loop"
    );
    native.global_queued_debt.store(0, Ordering::Release);
    std::thread::sleep(retry.saturating_duration_since(Instant::now()));
    assert!(!work.poll_shutdown(now()).unwrap());
    assert!(work.poll_shutdown(now()).unwrap());
    assert_eq!(native.global_audits.load(Ordering::Acquire), 2);
}

fn wrong_native_audit(identity: bool) {
    let directory = tempfile::tempdir().unwrap();
    let (mut work, native, _) = coordinator(directory.path());
    poll_until(&mut work, |work| work.ready());
    if identity {
        native
            .wrong_global_audit_identity
            .store(true, Ordering::Release);
    } else {
        native
            .wrong_global_audit_kind
            .store(true, Ordering::Release);
    }
    work.begin_shutdown();
    assert!(!work.poll_shutdown(now()).unwrap());
    assert!(!work.poll_shutdown(now()).unwrap());
    assert!(work.resource_destroyed());
    assert_eq!(work.phase(), AdmissionPhase::Uncertain);
    assert!(!work.poll_shutdown(now()).unwrap());
    assert_eq!(
        native.global_audits.load(Ordering::Acquire),
        1,
        "mismatched original terminal stays retained"
    );
}

#[test]
fn ordinary_native_zero_cannot_replace_the_original_seal_barrier() {
    if child("ordinary_native_zero_cannot_replace_the_original_seal_barrier") {
        return;
    }
    wrong_native_audit(false);
}

#[test]
fn wrong_native_zero_identity_cannot_settle_the_original_seal_barrier() {
    if child("wrong_native_zero_identity_cannot_settle_the_original_seal_barrier") {
        return;
    }
    wrong_native_audit(true);
}

#[test]
fn retained_native_shutdown_never_exceeds_the_existing_eight_audit_ceiling() {
    if child("retained_native_shutdown_never_exceeds_the_existing_eight_audit_ceiling") {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let (mut work, native, _) = coordinator(directory.path());
    poll_until(&mut work, |work| work.ready());
    native.global_queued_debt.store(1, Ordering::Release);
    work.begin_shutdown();
    loop {
        match work.poll_shutdown(now()) {
            Ok(false) => {}
            Ok(true) => panic!("nonzero native tasks cannot prove shutdown"),
            Err(Refusal::Uncertain) => break,
            Err(error) => panic!("unexpected native refusal: {error:?}"),
        }
        if let Some(deadline) = work.next_deadline() {
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        }
    }
    assert_eq!(
        native.global_audits.load(Ordering::Acquire),
        usize::from(MAX_AGENT_NATIVE_SHUTDOWN_AUDITS)
    );
    assert!(work.poll_shutdown(now()).is_err());
    assert_eq!(native.global_audits.load(Ordering::Acquire), 8);
}

fn stopped_before_start(expired: bool) {
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let (mut work, native, store) = coordinator(directory.path());
    let servers = Servers::default();
    poll_until(&mut work, |work| work.ready());
    store.hold.store(
        if expired {
            AgentWorkDisposition::Running
        } else {
            AgentWorkDisposition::Admitted
        } as u8,
        Ordering::Release,
    );
    assert!(work
        .submit(
            request(ContextRunId::generate(), vec![], servers.clone()),
            now()
        )
        .is_ok());
    poll_until(&mut work, |_| store.pending.lock().unwrap().is_some());
    assert_eq!(native.acquisitions.load(Ordering::Acquire), 1);
    if expired {
        work.poll(AgentPolicyInstant::from_millis(600_003));
    } else {
        work.cancel();
    }
    assert_eq!(work.phase(), AdmissionPhase::Uncertain);
    assert_eq!(native.reads.load(Ordering::Acquire), 0);
    assert_eq!(
        native.destructions.load(Ordering::Acquire),
        0,
        "ordinary stop is not whole-resource destruction or proven revocation"
    );
    assert!(work
        .submit(
            request(ContextRunId::generate(), vec![], servers.clone()),
            now()
        )
        .is_err());
    store.release(0);
    poll_until(&mut work, |work| {
        work.record()
            .is_some_and(|record| record.disposition() == AgentWorkDisposition::FailedClosed)
    });
    assert_eq!(
        native.reads.load(Ordering::Acquire),
        0,
        "raced ACK cannot start expired/stopped staged actor"
    );
    assert_ne!(work.record().unwrap().debt(), AgentWorkDebt::NONE);
    work.begin_shutdown();
    wait_until(|| {
        assert!(!work.poll_shutdown(now()).unwrap());
        work.resource_destroyed()
    });
    assert_eq!(native.destructions.load(Ordering::Acquire), 1);
    assert!(!work.ready());
    for server in servers.lock().unwrap().drain(..) {
        assert_eq!(server.join().unwrap(), 0);
    }
}

#[test]
fn cancellation_after_acquisition_does_not_fake_revocation_or_start_on_late_ack() {
    if child("cancellation_after_acquisition_does_not_fake_revocation_or_start_on_late_ack") {
        return;
    }
    stopped_before_start(false);
}

#[test]
fn expired_lease_is_checked_before_raced_running_acknowledgement() {
    if child("expired_lease_is_checked_before_raced_running_acknowledgement") {
        return;
    }
    stopped_before_start(true);
}

#[test]
fn refused_scoped_worker_persists_failed_closed_after_running_ack() {
    if child("refused_scoped_worker_persists_failed_closed_after_running_ack") {
        return;
    }
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    // Hold the real process worker permit. No native port is bound and this
    // unrelated suspended worker is never allowed to run a controller.
    let occupied = PendingAgentRuntime::spawn_suspended(AgentRuntimeConfig::STANDARD).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let (mut work, native, _) = coordinator(directory.path());
    let servers = Servers::default();
    poll_until(&mut work, |work| work.ready());
    assert!(work
        .submit(
            request(ContextRunId::generate(), vec![], servers.clone()),
            now()
        )
        .is_ok());
    poll_until(&mut work, |work| {
        work.record()
            .is_some_and(|record| record.disposition() == AgentWorkDisposition::FailedClosed)
    });
    assert_eq!(native.reads.load(Ordering::Acquire), 0);
    assert_eq!(native.acquisitions.load(Ordering::Acquire), 1);
    assert!(!work.ready());
    work.begin_shutdown();
    wait_until(|| {
        assert!(!work.poll_shutdown(now()).unwrap());
        work.resource_destroyed()
    });
    assert_ne!(work.record().unwrap().debt(), AgentWorkDebt::NONE);
    drop(occupied);
    for server in servers.lock().unwrap().drain(..) {
        assert_eq!(server.join().unwrap(), 0);
    }
}
