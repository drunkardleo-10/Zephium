//! Original retained worker + original delivery + real fenced Store; no native-zero substitute.
use super::*;

fn journal(
    store: &zephium_store::SqliteStore,
    request: AgentWorkJournalRequest,
) -> Result<AgentWorkJournalReply, AgentWorkJournalError> {
    let (tx, rx) = mpsc::sync_channel(1);
    store.dispatch(request, Box::new(move |reply| tx.send(reply).unwrap()))?;
    rx.recv_timeout(Duration::from_secs(5)).unwrap()
}

fn acknowledge(
    store: &zephium_store::SqliteStore,
    mutation: AgentWorkJournalMutation,
) -> AgentWorkRecord {
    let AgentWorkJournalReply::Record(Some(record)) =
        journal(store, AgentWorkJournalRequest::CompareAndSet(mutation)).unwrap()
    else {
        panic!("missing original durable acknowledgement");
    };
    assert_eq!(record, mutation.next());
    record
}

#[test]
fn scoped_terminals_persist_without_destroying_retained_page_or_reopening_old_actor() {
    // The real Store process fence outlives its connection. A child process
    // isolates this test from other real-Store tests without resetting the fence.
    const CHILD: &str = "ZEPHIUM_SCOPED_DURABLE_FIXTURE";
    if std::env::var_os(CHILD).is_none() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "work_resources::controller::tests::durable_tests::scoped_terminals_persist_without_destroying_retained_page_or_reopening_old_actor", "--nocapture"])
            .env(CHILD, "1").output().unwrap();
        assert!(child.status.success(), "scoped durable child: {child:?}");
        return;
    }
    let _serial = crate::WORK_RUNTIME_TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(zephium_store::SqliteStore::open(directory.path()).unwrap());
    let AgentWorkJournalReply::Claimed {
        owner: incarnation,
        records,
    } = journal(&store, AgentWorkJournalRequest::Claim).unwrap()
    else {
        panic!("original process fence");
    };
    assert!(records.is_empty());
    let (owner, native, resource, browser) = setup();
    let mut browser = Some(browser);
    let mut prior: Option<AgentWorkRecord> = None;
    let mut original: Option<(AgentRuntimeHandle, AgentRuntimeScopedDrained)> = None;
    for cancelled in [false, true] {
        if cancelled {
            let mut acquire = owner
                .acquire(
                    &resource,
                    ContextRunId::generate(),
                    now(),
                    AgentPolicyInstant::from_millis(600_002),
                )
                .unwrap();
            let Some(LifecycleResult::Event(WorkBrowserResourceEvent::Acquired(lease))) =
                acquire.poll(now()).unwrap()
            else {
                panic!("fresh explicit lease");
            };
            browser = Some(owner.retained_browser(lease, now()).unwrap());
            native.hold_read.store(true, Ordering::Release);
        }
        let browser = browser.take().unwrap();
        let lease = browser.binding().lease().clone();
        let (controller, mut result, scope, server) = prepared_with_audit(
            Box::new(browser),
            if cancelled {
                vec![]
            } else {
                vec![response_stream(1), response_stream(2)]
            },
            store.clone(),
        );
        let admitted = acknowledge(&store, controller.journal_admission(incarnation).unwrap());
        let running = acknowledge(
            &store,
            AgentWorkJournalMutation::transition(admitted, AgentWorkDisposition::Running).unwrap(),
        );
        // Neither the worker nor a read/provider operation precedes Running ACK.
        assert_eq!(native.reads.load(Ordering::Acquire), usize::from(cancelled));
        let (handle, lifecycle) = start(controller, scope);
        if cancelled {
            wait_until(|| native.read.lock().unwrap().is_some());
            handle.stop_and_seal(AgentRuntimeStopReason::Cancelled);
            let (request, callback) = native.read.lock().unwrap().take().unwrap();
            Native::read_result(request, callback);
        }
        let mut outcome = None;
        wait_until(|| {
            while result.take_event().is_some() {}
            outcome = result.take_outcome();
            outcome.is_some()
        });
        let policy = match outcome.unwrap() {
            AgentWorkRetainedOutcome::Accepted {
                settlement,
                extraction,
            } if !cancelled => {
                assert_eq!(extraction.stats().source_edges(), 1);
                settlement
            }
            AgentWorkRetainedOutcome::ClosedUnsuccessfully(closed) if cancelled => {
                closed.policy_settlement()
            }
            _ => panic!("original scoped outcome"),
        };
        let AgentRuntimeScopedDrain::Drained(drained) =
            lifecycle.drain_until(Instant::now() + Duration::from_secs(2))
        else {
            panic!("original scoped worker drain");
        };
        assert_eq!(drained.lease(), &lease);
        assert_eq!(drained.policy(), policy);
        assert!(drained.work_terminal(&handle, admitted).is_err());
        for offset in [32, 48, 64] {
            let mut bytes = *running.as_bytes();
            bytes[offset] ^= 1;
            assert!(drained
                .work_terminal(&handle, AgentWorkRecord::decode(bytes).unwrap())
                .is_err());
        }
        if let Some((old_handle, old_drained)) = &original {
            assert!(drained.work_terminal(old_handle, running).is_err());
            assert!(old_drained.work_terminal(&handle, running).is_err());
        }
        let mutation = drained.work_terminal(&handle, running).unwrap();
        assert_eq!(mutation.next().debt(), AgentWorkDebt::NONE);
        assert_eq!(
            mutation.next().disposition(),
            if cancelled {
                AgentWorkDisposition::Cancelled
            } else {
                AgentWorkDisposition::Succeeded
            }
        );
        // A prepared mutation has not changed the Store. Resource retention is
        // deliberately not mistaken for global-zero or fresh execution permission.
        assert!(
            matches!(journal(&store, AgentWorkJournalRequest::Read { owner: incarnation, key: running.key() }).unwrap(), AgentWorkJournalReply::Record(Some(record)) if record == running)
        );
        let terminal = acknowledge(&store, mutation);
        assert_eq!(
            acknowledge(&store, mutation),
            terminal,
            "exact write reconciliation is idempotent"
        );
        assert!(drained.work_terminal(&handle, terminal).is_err());
        assert!(
            AgentWorkJournalMutation::transition(terminal, AgentWorkDisposition::Running).is_err()
        );
        assert_eq!(native.destructions.load(Ordering::Acquire), 0);
        assert!(!owner.locally_retired());
        assert!(matches!(
            native.seal_for_shutdown(ContextResourceAuditId::new(1).unwrap()),
            ContextShutdownDispatch::SealedWithoutAudit(_)
        ));
        native.join();
        if let Some(prior) = prior {
            assert!(
                matches!(journal(&store, AgentWorkJournalRequest::Read { owner: incarnation, key: prior.key() }).unwrap(), AgentWorkJournalReply::Record(Some(record)) if record == prior)
            );
        }
        prior = Some(terminal);
        if original.is_none() {
            original = Some((handle, drained));
        }
        assert_eq!(server.join().unwrap(), if cancelled { 0 } else { 2 });
    }
    assert_eq!(native.acquisitions.load(Ordering::Acquire), 2);
    assert_eq!(
        native.reads.load(Ordering::Acquire),
        2,
        "fresh second-lease observation, no old snapshot replay"
    );
    let mut destroy = owner.destroy(&resource).unwrap();
    assert!(matches!(
        destroy.poll(now()).unwrap(),
        Some(LifecycleResult::Event(WorkBrowserResourceEvent::Destroyed(
            _
        )))
    ));
    owner.reap_absent(&resource).unwrap();
    owner.seal_resources().unwrap();
    assert!(owner.locally_retired());
    assert_eq!(native.destructions.load(Ordering::Acquire), 1);
}
