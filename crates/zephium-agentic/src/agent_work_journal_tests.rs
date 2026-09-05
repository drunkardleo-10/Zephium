use super::*;

fn initial() -> AgentWorkRecord {
    let mut bytes = [0; AGENT_WORK_RECORD_BYTES];
    bytes[0] = 1;
    bytes[1] = 1;
    bytes[2] = 63;
    bytes[15] = 1;
    bytes[31] = 1;
    bytes[47] = 2;
    bytes[63] = 3;
    AgentWorkRecord::decode(bytes).unwrap()
}

#[test]
fn every_disposition_pair_has_closed_transition_grammar() {
    use AgentWorkDisposition::*;
    let dispositions = [
        Admitted,
        Running,
        NeedsApproval,
        RecoveryRequired,
        Interrupted,
        Succeeded,
        FreshAdmissionRequired,
        Rejected,
        FailedClosed,
        Failed,
        Cancelled,
    ];
    for from in dispositions {
        let mut bytes = *initial().as_bytes();
        bytes[1] = from as u8;
        bytes[2] = if matches!(from, Succeeded | Failed | Cancelled) {
            0
        } else {
            63
        };
        let record = AgentWorkRecord::decode(bytes).unwrap();
        for to in dispositions {
            let allowed = matches!(
                (from, to),
                (Admitted, Running | RecoveryRequired | FailedClosed)
                    | (Running, NeedsApproval | RecoveryRequired | FailedClosed)
                    | (
                        NeedsApproval | Interrupted,
                        FreshAdmissionRequired | Rejected | FailedClosed
                    )
                    | (RecoveryRequired, FailedClosed)
            );
            assert_eq!(record.transition(to).is_ok(), allowed, "{from:?} -> {to:?}");
        }
    }
}

#[test]
fn review_is_one_use_and_never_clears_execution_debt() {
    let review = initial()
        .transition(AgentWorkDisposition::Running)
        .unwrap()
        .transition(AgentWorkDisposition::NeedsApproval)
        .unwrap();
    for decision in [
        AgentWorkDisposition::FreshAdmissionRequired,
        AgentWorkDisposition::Rejected,
    ] {
        let next = AgentWorkJournalMutation::transition(review, decision)
            .unwrap()
            .next();
        assert_eq!(next.debt(), AgentWorkDebt::UNKNOWN);
        assert!(next.disposition().is_terminal());
        assert!(AgentWorkJournalMutation::transition(next, decision).is_err());
        assert!(AgentWorkJournalMutation::transition(next, AgentWorkDisposition::Running).is_err());
        assert!(next.is_successor_of(review));
    }
}

#[test]
fn unsuccessful_closed_facts_are_immutable_and_cannot_mint_mutation_authority() {
    let running = initial().transition(AgentWorkDisposition::Running).unwrap();
    for disposition in [
        AgentWorkDisposition::Failed,
        AgentWorkDisposition::Cancelled,
    ] {
        let mut bytes = *running.as_bytes();
        bytes[1] = disposition as u8;
        bytes[2] = 0;
        bytes[8..16].copy_from_slice(&(running.revision() + 1).to_be_bytes());
        let terminal = AgentWorkRecord::decode(bytes).unwrap();
        assert!(terminal.is_successor_of(running));
        assert!(!terminal.is_successor_of(initial()));
        assert_eq!(terminal.debt(), AgentWorkDebt::NONE);
        assert_eq!(
            terminal.interrupted(AgentWorkIncarnation::generate()),
            Ok(terminal)
        );
        assert!(AgentWorkJournalMutation::transition(running, disposition).is_err());
        assert!(terminal.transition(AgentWorkDisposition::Running).is_err());
        assert!(terminal
            .transition(AgentWorkDisposition::FailedClosed)
            .is_err());
        bytes[2] = AgentWorkDebt::UNKNOWN.bits();
        assert!(AgentWorkRecord::decode(bytes).is_none());
    }
}

#[test]
fn restart_fences_incomplete_state_without_reopening_terminal_state() {
    let prior = initial().transition(AgentWorkDisposition::Running).unwrap();
    let owner = AgentWorkIncarnation::generate();
    let recovered = prior.interrupted(owner).unwrap();
    assert_eq!(recovered.disposition(), AgentWorkDisposition::Interrupted);
    assert_eq!(recovered.revision(), prior.revision() + 1);
    assert_eq!(recovered.incarnation(), owner);
    assert_eq!(recovered.debt(), AgentWorkDebt::UNKNOWN);
    assert!(!recovered.is_successor_of(prior));
    assert_eq!(recovered.interrupted(owner), Ok(recovered));
    let terminal = recovered
        .transition(AgentWorkDisposition::Rejected)
        .unwrap();
    assert_eq!(
        terminal.interrupted(AgentWorkIncarnation::generate()),
        Ok(terminal)
    );
}

#[test]
fn corrupt_versions_reserved_fields_and_zero_or_contradictory_state_fail_closed() {
    for (offset, value) in [
        (0, 0),
        (0, 2),
        (1, 0),
        (1, 12),
        (2, 64),
        (3, 1),
        (7, 1),
        (15, 0),
        (31, 0),
        (2, 0),
    ] {
        let mut bytes = *initial().as_bytes();
        bytes[offset] = value;
        assert!(AgentWorkRecord::decode(bytes).is_none(), "offset {offset}");
    }
    let mut bytes = *initial().as_bytes();
    bytes[1] = AgentWorkDisposition::Succeeded as u8;
    assert!(AgentWorkRecord::decode(bytes).is_none());
}

#[test]
fn overflow_and_foreign_cas_are_refused() {
    let prior = initial();
    let next = prior.transition(AgentWorkDisposition::Running).unwrap();
    let mut bytes = *next.as_bytes();
    bytes[63] = 4;
    assert!(!AgentWorkRecord::decode(bytes)
        .unwrap()
        .is_successor_of(prior));
    let mut bytes = *prior.as_bytes();
    bytes[8..16].copy_from_slice(&u64::MAX.to_be_bytes());
    assert_eq!(
        AgentWorkRecord::decode(bytes)
            .unwrap()
            .transition(AgentWorkDisposition::Running),
        Err(AgentWorkJournalError::Capacity)
    );
}

#[test]
fn debug_projection_is_content_free_and_omits_durable_identifiers() {
    assert_eq!(
        format!("{:?}", initial()),
        "AgentWorkRecord { revision: 1, disposition: Admitted, debt: AgentWorkDebt(63), .. }"
    );
    assert_eq!(
        format!("{:?}", initial().incarnation()),
        "AgentWorkIncarnation([redacted])"
    );
}
