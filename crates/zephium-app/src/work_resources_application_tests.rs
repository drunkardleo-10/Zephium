//! Real Store acknowledgements held independently of committed durable facts.
use super::*;
use crate::work_resources::application::*;

type Held = (
    Result<AgentWorkJournalReply, AgentWorkJournalError>,
    AgentWorkJournalCompletion,
);
struct GatedStore {
    store: Arc<zephium_store::SqliteStore>,
    hold: Arc<AtomicU8>,
    pending: Arc<Mutex<Option<Held>>>,
    wakes: Arc<AtomicUsize>,
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
        self.store.dispatch(
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
    let store = Arc::new(GatedStore {
        store: Arc::new(zephium_store::SqliteStore::open(directory).unwrap()),
        hold: Arc::new(AtomicU8::new(0)),
        pending: Arc::new(Mutex::new(None)),
        wakes: Arc::new(AtomicUsize::new(0)),
    });
    let wakes = store.wakes.clone();
    let (owner, native, resource) = construct_fixture_with_wake(
        ContextProfileStorageClass::Ephemeral,
        WorkBrowserDocumentPolicy::Exact,
        None,
        Arc::new(move || {
            wakes.fetch_add(1, Ordering::AcqRel);
            true
        }),
    );
    let work = RetainedWork::new(owner, resource, store.clone(), store.clone())
        .unwrap_or_else(|_| panic!("original owner and Store join"));
    (work, native, store)
}

type Servers = Arc<Mutex<Vec<std::thread::JoinHandle<usize>>>>;
fn request(run: ContextRunId, responses: Vec<String>, servers: Servers) -> ActorRequest {
    ActorRequest {
        run,
        deadline: AgentPolicyInstant::from_millis(600_002),
        prepare: Box::new(move |browser, audit| {
            let input = input(browser.binding(), Arc::new(Clock(AtomicU64::new(2))));
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
    work.begin_shutdown();
    wait_until(|| work.poll_shutdown(now()).unwrap());
    assert!(!work.ready());
    assert_eq!(native.destructions.load(Ordering::Acquire), 1);
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
    assert!(work.reconcile());
    poll_until(&mut work, |work| work.phase() == AdmissionPhase::Terminal);
    assert!(
        !work.ready(),
        "reconciliation never clears the sticky execution stop"
    );
    assert!(work.take_extraction().is_none());
    assert!(work.poll_shutdown(now()).unwrap());
    store.release(0); // The old callback cannot select/rewrite the reconciled lane.
    work.poll(now());
    assert!(work.poll_shutdown(now()).unwrap());
    for server in servers.lock().unwrap().drain(..) {
        assert_eq!(server.join().unwrap(), 2);
    }
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
