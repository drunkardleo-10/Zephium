use super::*;
use zephium_agentic::{AgentWorkDebt, AgentWorkJournalMutation};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Fault {
    BeforeWrite,
    AfterWrite,
    AfterCommit,
    RestartPartial,
}
thread_local! { static FAULT: std::cell::Cell<Option<Fault>> = const { std::cell::Cell::new(None) }; }
pub(super) fn fault(expected: Fault) -> bool {
    FAULT.with(|fault| {
        if fault.get() == Some(expected) {
            fault.set(None);
            true
        } else {
            false
        }
    })
}

fn initial(owner: AgentWorkIncarnation, key: u16) -> AgentWorkRecord {
    let mut bytes = [0; AGENT_WORK_RECORD_BYTES];
    bytes[0] = 1;
    bytes[1] = 1;
    bytes[2] = 63;
    bytes[15] = 1;
    bytes[16..32].copy_from_slice(&owner.bytes());
    bytes[46..48].copy_from_slice(&key.to_be_bytes());
    bytes[62..64].copy_from_slice(&key.to_be_bytes());
    AgentWorkRecord::decode(bytes).unwrap()
}

fn open() -> (tempfile::TempDir, Hub, AgentWorkIncarnation) {
    let directory = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(directory.path().into()).unwrap();
    let Reply::Claimed { owner, records } = hub.agent_work(Request::Claim).unwrap() else {
        panic!()
    };
    assert!(records.is_empty());
    (directory, hub, owner)
}

fn put(hub: &mut Hub, record: AgentWorkRecord) {
    compare_and_set(&mut hub.meta, None, record).unwrap();
}

fn transition(
    hub: &mut Hub,
    record: AgentWorkRecord,
    to: AgentWorkDisposition,
) -> Result<AgentWorkRecord, Error> {
    let mutation = AgentWorkJournalMutation::transition(record, to)?;
    match hub.agent_work(Request::CompareAndSet(mutation))? {
        Reply::Record(Some(next)) => Ok(next),
        _ => panic!("wrong reply"),
    }
}

#[test]
fn namespace_fences_two_live_owners_and_releases_only_on_drop() {
    let (directory, first, owner) = open();
    let mut second = Hub::open(directory.path().into()).unwrap();
    assert!(matches!(
        second.agent_work(Request::Claim),
        Err(Error::Unavailable)
    ));
    drop(first);
    let Reply::Claimed {
        owner: second_owner,
        ..
    } = second.agent_work(Request::Claim).unwrap()
    else {
        panic!()
    };
    assert_ne!(owner, second_owner);
    assert!(matches!(
        second.agent_work(Request::Read {
            owner,
            key: [0; 32]
        }),
        Err(Error::Fenced)
    ));
}

#[test]
fn crash_restart_classifies_every_incomplete_stage_without_replay() {
    let (directory, mut hub, owner) = open();
    let mut old = Vec::new();
    for (key, disposition) in [
        (1, AgentWorkDisposition::Admitted),
        (2, AgentWorkDisposition::Running),
        (3, AgentWorkDisposition::NeedsApproval),
        (4, AgentWorkDisposition::RecoveryRequired),
    ] {
        let record = initial(owner, key);
        put(&mut hub, record);
        let record = if disposition == AgentWorkDisposition::Admitted {
            record
        } else {
            let running = transition(&mut hub, record, AgentWorkDisposition::Running).unwrap();
            if disposition == AgentWorkDisposition::Running {
                running
            } else {
                transition(&mut hub, running, disposition).unwrap()
            }
        };
        old.push(record);
    }
    drop(hub);
    let mut reopened = Hub::open(directory.path().into()).unwrap();
    let Reply::Claimed {
        owner: new_owner,
        records,
    } = reopened.agent_work(Request::Claim).unwrap()
    else {
        panic!()
    };
    assert_eq!(records.len(), 4);
    for (current, old) in records.iter().zip(old) {
        assert_eq!(current.disposition(), AgentWorkDisposition::Interrupted);
        assert_eq!(current.incarnation(), new_owner);
        assert_eq!(current.debt(), AgentWorkDebt::UNKNOWN);
        assert_eq!(current.revision(), old.revision() + 1);
        assert!(matches!(
            reopened.agent_work(Request::Read {
                owner,
                key: old.key()
            }),
            Err(Error::Fenced)
        ));
        assert!(matches!(
            transition(&mut reopened, old, AgentWorkDisposition::FailedClosed),
            Err(Error::Fenced)
        ));
    }
}

#[test]
fn partial_restart_transaction_rolls_back_every_record_and_fence() {
    let (directory, mut hub, owner) = open();
    let first = initial(owner, 1);
    let second = initial(owner, 2);
    put(&mut hub, first);
    put(&mut hub, second);
    drop(hub);
    let mut reopened = Hub::open(directory.path().into()).unwrap();
    FAULT.with(|fault| fault.set(Some(Fault::RestartPartial)));
    assert!(matches!(
        reopened.agent_work(Request::Claim),
        Err(Error::Uncertain)
    ));
    assert_eq!(inventory(&reopened.meta).unwrap(), vec![first, second]);
    assert_eq!(verify_owner(&reopened.meta, owner), Ok(()));
    assert!(
        matches!(reopened.agent_work(Request::Claim), Ok(Reply::Claimed { owner: new_owner, records }) if new_owner != owner && records.iter().all(|record| record.incarnation() == new_owner))
    );
}

#[test]
fn partial_writes_and_lost_acknowledgement_reconcile_only_exact_cas() {
    for injected in [Fault::BeforeWrite, Fault::AfterWrite, Fault::AfterCommit] {
        let (_directory, mut hub, owner) = open();
        let admitted = initial(owner, 1);
        put(&mut hub, admitted);
        let mutation =
            AgentWorkJournalMutation::transition(admitted, AgentWorkDisposition::Running).unwrap();
        FAULT.with(|fault| fault.set(Some(injected)));
        assert!(matches!(
            hub.agent_work(Request::CompareAndSet(mutation)),
            Err(Error::Uncertain)
        ));
        let expected = if injected == Fault::AfterCommit {
            mutation.next()
        } else {
            admitted
        };
        assert_eq!(read(&hub.meta, admitted.key()).unwrap(), Some(expected));
        assert!(
            matches!(hub.agent_work(Request::CompareAndSet(mutation)), Ok(Reply::Record(Some(record))) if record == mutation.next())
        );
        assert_eq!(inventory(&hub.meta).unwrap(), vec![mutation.next()]);
    }
}

#[test]
fn approval_accept_reject_stale_replay_and_cancellation_races_are_exact() {
    for winner in [
        AgentWorkDisposition::FreshAdmissionRequired,
        AgentWorkDisposition::Rejected,
        AgentWorkDisposition::FailedClosed,
    ] {
        let (_directory, mut hub, owner) = open();
        let admitted = initial(owner, 1);
        put(&mut hub, admitted);
        let running = transition(&mut hub, admitted, AgentWorkDisposition::Running).unwrap();
        let review = transition(&mut hub, running, AgentWorkDisposition::NeedsApproval).unwrap();
        let terminal = transition(&mut hub, review, winner).unwrap();
        assert_eq!(terminal.debt(), AgentWorkDebt::UNKNOWN);
        // Transport-level retry acknowledges exact already persisted decision.
        assert_eq!(transition(&mut hub, review, winner), Ok(terminal));
        // A fresh user decision after acknowledgement cannot reopen the record.
        assert_eq!(
            transition(&mut hub, terminal, winner),
            Err(Error::Transition)
        );
        for loser in [
            AgentWorkDisposition::FreshAdmissionRequired,
            AgentWorkDisposition::Rejected,
            AgentWorkDisposition::FailedClosed,
        ] {
            if loser != winner {
                assert_eq!(transition(&mut hub, review, loser), Err(Error::Conflict));
            }
        }
    }
}

#[test]
fn terminal_persistence_is_immutable_across_restart_and_sql_update() {
    let (directory, mut hub, owner) = open();
    let admitted = initial(owner, 1);
    put(&mut hub, admitted);
    let terminal = transition(&mut hub, admitted, AgentWorkDisposition::FailedClosed).unwrap();
    assert!(hub
        .meta
        .execute("UPDATE agent_work_runs SET terminal = 0", [])
        .is_err());
    assert!(hub.meta.execute("DELETE FROM agent_work_runs", []).is_err());
    drop(hub);
    let mut reopened = Hub::open(directory.path().into()).unwrap();
    assert!(
        matches!(reopened.agent_work(Request::Claim), Ok(Reply::Claimed { records, .. }) if records == vec![terminal])
    );
}

#[test]
fn transient_store_cannot_claim_durable_admission() {
    let mut hub = Hub::in_memory().unwrap();
    assert!(matches!(
        hub.agent_work(Request::Claim),
        Err(Error::Unavailable)
    ));
}

#[test]
fn corrupt_record_fails_closed() {
    let (_directory, mut hub, owner) = open();
    let admitted = initial(owner, 1);
    put(&mut hub, admitted);
    let mut corrupt = *admitted.as_bytes();
    corrupt[3] = 1;
    hub.meta
        .execute(
            "UPDATE agent_work_runs SET record = ?1",
            [corrupt.as_slice()],
        )
        .unwrap();
    assert_eq!(read(&hub.meta, admitted.key()), Err(Error::Uncertain));
    assert!(matches!(
        hub.agent_work(Request::Claim),
        Err(Error::Uncertain)
    ));
}

#[test]
fn retention_capacity_refuses_admission_without_evicting_debt() {
    let (_directory, mut hub, owner) = open();
    let transaction = hub.meta.transaction().unwrap();
    for key in 1..=1024 {
        let record = initial(owner, key);
        transaction
            .execute(
                "INSERT INTO agent_work_runs(run_key, record, terminal) VALUES (?1, ?2, 0)",
                params![record.key().as_slice(), record.as_bytes().as_slice()],
            )
            .unwrap();
    }
    transaction.commit().unwrap();
    assert_eq!(
        compare_and_set(&mut hub.meta, None, initial(owner, 1025)),
        Err(Error::Capacity)
    );
    assert_eq!(inventory(&hub.meta).unwrap().len(), 1024);
    let first = initial(owner, 1);
    assert!(transition(&mut hub, first, AgentWorkDisposition::FailedClosed).is_ok());
}
