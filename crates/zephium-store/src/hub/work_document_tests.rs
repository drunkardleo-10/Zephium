use super::*;
use zephium_core::profiles::ProfileKind;
use zephium_core::session::{PersistedProfile, SessionState};
fn session() -> SessionState {
    SessionState {
        profiles: vec![
            PersistedProfile {
                id: 1.into(),
                name: "Personal".into(),
                kind: ProfileKind::Default,
            },
            PersistedProfile {
                id: 2.into(),
                name: "Other".into(),
                kind: ProfileKind::Named,
            },
        ],
        ..SessionState::default()
    }
}
fn create(hub: &mut Hub) -> WorkSnapshot {
    let WorkReply::Snapshot(work) = hub
        .work_document(
            1.into(),
            WorkRequest::Create {
                id: 10.into(),
                objective: "Compare implementation choices".into(),
            },
        )
        .unwrap()
    else {
        panic!()
    };
    *work
}
fn draft() -> WorkPlanDraft {
    WorkPlanDraft {
        id: 11.into(),
        nodes: vec![WorkPlanNode {
            id: 12.into(),
            objective: "Read sources".into(),
            dependencies: vec![],
            outputs: vec![WorkExpectedOutput {
                name: "sources".into(),
                description: "Cited findings".into(),
                review: WorkOutputReview::SourceMappedNeedsReview,
            }],
        }],
    }
}
fn edit(hub: &mut Hub, work: &WorkSnapshot, edit: WorkEdit) -> Result<WorkSnapshot, WorkError> {
    match hub.work_document(
        work.profile,
        WorkRequest::Edit {
            id: work.id,
            expected: work.revision,
            edit,
        },
    )? {
        WorkReply::Snapshot(w) => Ok(*w),
        _ => panic!(),
    }
}
#[test]
fn work_document_survives_restart_with_questions_and_immutable_plan_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(dir.path().into()).unwrap();
    hub.save(&session()).unwrap();
    let initial = create(&mut hub);
    let planned = edit(
        &mut hub,
        &initial,
        WorkEdit::ReplaceDraft { draft: draft() },
    )
    .unwrap();
    let questioned = edit(
        &mut hub,
        &planned,
        WorkEdit::OpenQuestion {
            id: 30.into(),
            prompt: "Which environment?".into(),
            options: vec!["Local".into()],
        },
    )
    .unwrap();
    let answered = edit(
        &mut hub,
        &questioned,
        WorkEdit::AnswerQuestion {
            id: 30.into(),
            answer: "Local".into(),
        },
    )
    .unwrap();
    let mut next = draft();
    next.nodes[0].objective = "Read local-runtime sources".into();
    let final_work = edit(&mut hub, &answered, WorkEdit::ReplaceDraft { draft: next }).unwrap();
    drop(hub);
    let mut hub = Hub::open(dir.path().into()).unwrap();
    let WorkReply::Snapshot(restored) = hub
        .work_document(1.into(), WorkRequest::Read { id: initial.id })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(*restored, final_work);
    let WorkReply::Plan(old) = hub
        .work_document(
            1.into(),
            WorkRequest::ReadPlan {
                id: initial.id,
                revision: planned.revision,
            },
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(old.draft, draft());
    assert_eq!(restored.status, WorkAuthoringStatus::PlanReady);
    let conn = hub.profile_conn(1.into()).unwrap();
    assert!(conn
        .execute("UPDATE work_plan_nodes SET body = '{}'", [])
        .is_err());
    assert!(conn
        .execute("UPDATE work_events SET kind = 'created'", [])
        .is_err());
}
#[test]
fn work_document_cas_profile_isolation_and_private_refusal() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&session()).unwrap();
    let initial = create(&mut hub);
    let updated = edit(
        &mut hub,
        &initial,
        WorkEdit::SetObjective {
            objective: "Changed".into(),
        },
    )
    .unwrap();
    assert_eq!(
        edit(
            &mut hub,
            &initial,
            WorkEdit::SetObjective {
                objective: "Stale".into()
            }
        ),
        Err(WorkError::Conflict)
    );
    assert!(matches!(
        hub.work_document(2.into(), WorkRequest::Read { id: initial.id }),
        Err(WorkError::NotFound)
    ));
    assert!(matches!(
        hub.work_document(
            999.into(),
            WorkRequest::Create {
                id: 44.into(),
                objective: "Private".into()
            }
        ),
        Err(WorkError::ProfileUnavailable)
    ));
    let WorkReply::Snapshot(read) = hub
        .work_document(1.into(), WorkRequest::Read { id: initial.id })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(*read, updated);
}
#[test]
fn work_document_failed_audit_rolls_back_objective_and_plan_nodes() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&session()).unwrap();
    let initial = create(&mut hub);
    // Fault injection only: fail the last write after all proposed changes.
    hub.profile_conn(1.into()).unwrap().execute_batch("CREATE TRIGGER fail_work_event BEFORE INSERT ON work_events BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
    assert!(edit(
        &mut hub,
        &initial,
        WorkEdit::ReplaceDraft { draft: draft() }
    )
    .is_err());
    let WorkReply::Snapshot(read) = hub
        .work_document(1.into(), WorkRequest::Read { id: initial.id })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(*read, initial);
    assert_eq!(
        hub.profile_conn(1.into())
            .unwrap()
            .query_row("SELECT count(*) FROM work_plans", [], |r| r
                .get::<_, usize>(0))
            .unwrap(),
        0
    );
}
#[test]
fn work_document_read_refuses_corrupt_or_future_payload_without_repair() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&session()).unwrap();
    let initial = create(&mut hub);
    let planned = edit(
        &mut hub,
        &initial,
        WorkEdit::ReplaceDraft { draft: draft() },
    )
    .unwrap();
    let conn = hub.profile_conn(1.into()).unwrap();
    conn.execute_batch("DROP TRIGGER work_plan_nodes_immutable; UPDATE work_plan_nodes SET body = '{\"version\":999}';").unwrap();
    assert!(matches!(
        hub.work_document(1.into(), WorkRequest::Read { id: planned.id }),
        Err(WorkError::Invalid)
    ));
    let body: String = hub
        .profile_conn(1.into())
        .unwrap()
        .query_row("SELECT body FROM work_plan_nodes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(body, "{\"version\":999}");
}
#[test]
fn work_document_pagination_and_plan_capacity_are_bounded() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&session()).unwrap();
    let mut work = create(&mut hub);
    hub.work_document(
        1.into(),
        WorkRequest::Create {
            id: 20.into(),
            objective: "Second".into(),
        },
    )
    .unwrap();
    let WorkReply::Page { works, next } = hub
        .work_document(
            1.into(),
            WorkRequest::List {
                after: None,
                limit: 1,
            },
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(works.len(), 1);
    assert_eq!(next, Some(work.id));
    let WorkReply::Page { works, next } = hub
        .work_document(
            1.into(),
            WorkRequest::List {
                after: next,
                limit: 1,
            },
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(works[0].id, WorkId::from(20));
    assert_eq!(next, None);
    for _ in 0..MAX_WORK_PLAN_REVISIONS {
        work = edit(&mut hub, &work, WorkEdit::ReplaceDraft { draft: draft() }).unwrap();
    }
    assert_eq!(
        edit(&mut hub, &work, WorkEdit::ReplaceDraft { draft: draft() }),
        Err(WorkError::Capacity)
    );
}

thread_local! { pub(super) static LOSE_COMMIT_ACK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
#[test]
fn work_document_uncertain_commit_reconciles_facts_without_blind_replay() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&session()).unwrap();
    let initial = create(&mut hub);
    LOSE_COMMIT_ACK.with(|fault| fault.set(true));
    assert_eq!(
        edit(
            &mut hub,
            &initial,
            WorkEdit::SetObjective {
                objective: "Committed before callback loss".into()
            }
        ),
        Err(WorkError::OutcomeUnknown)
    );
    let WorkReply::Snapshot(read) = hub
        .work_document(1.into(), WorkRequest::Read { id: initial.id })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(read.revision, initial.revision.next().unwrap());
    assert_eq!(read.objective, "Committed before callback loss");
    assert_eq!(
        edit(
            &mut hub,
            &initial,
            WorkEdit::SetObjective {
                objective: "Replay".into()
            }
        ),
        Err(WorkError::Conflict)
    );
}
#[test]
fn work_document_revision_wire_is_exact_and_rejects_untrusted_authority_fields() {
    for value in ["0", "01", "-1", "9223372036854775808"] {
        assert!(serde_json::from_str::<WorkRevision>(&format!("\"{value}\"")).is_err());
    }
    let revision = WorkRevision::new(9_007_199_254_740_993).unwrap();
    let json = serde_json::to_string(&revision).unwrap();
    assert_eq!(
        serde_json::from_str::<WorkRevision>(&json).unwrap(),
        revision
    );
    let mut value =
        serde_json::to_value(WorkSnapshot::create(1.into(), 2.into(), "Objective".into()).unwrap())
            .unwrap();
    value["approved"] = serde_json::json!(true);
    assert!(serde_json::from_value::<WorkSnapshot>(value).is_err());
}

#[test]
fn work_document_payload_budget_is_atomic_and_does_not_scan_history() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&session()).unwrap();
    let initial = create(&mut hub);
    let conn = hub.profile_conn(1.into()).unwrap();
    let bytes: usize = conn
        .query_row("SELECT bytes FROM work_payload_usage", [], |r| r.get(0))
        .unwrap();
    assert_eq!(bytes, initial.objective.len());
    conn.execute("UPDATE work_payload_usage SET bytes = 33554432", [])
        .unwrap();
    assert_eq!(
        edit(
            &mut hub,
            &initial,
            WorkEdit::ReplaceDraft { draft: draft() }
        ),
        Err(WorkError::Capacity)
    );
    let conn = hub.profile_conn(1.into()).unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM work_plans", [], |r| r
            .get::<_, usize>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("SELECT revision FROM works", [], |r| r.get::<_, usize>(0))
            .unwrap(),
        1
    );
}

#[test]
fn work_document_profile_deletion_erases_work_and_blocks_late_edits() {
    let directory = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(directory.path().into()).unwrap();
    hub.save(&session()).unwrap();
    let initial = create(&mut hub);
    let planned = edit(
        &mut hub,
        &initial,
        WorkEdit::ReplaceDraft { draft: draft() },
    )
    .unwrap();
    let mut survivor = session();
    survivor.profiles.remove(0);
    assert_eq!(
        hub.authorize_profile_deletion(1.into(), &survivor).unwrap(),
        zephium_core::ports::store::ProfileDeletionAuthorizeOutcome::Authorized
    );
    assert!(matches!(
        hub.work_document(1.into(), WorkRequest::Read { id: planned.id }),
        Err(WorkError::ProfileUnavailable)
    ));
    assert!(hub.finalize_profile_deletion(1.into()).unwrap());
    assert!(!directory
        .path()
        .join(format!("profile-{}.sqlite", ProfileId::from(1)))
        .exists());
}

#[test]
fn work_document_profile_v14_upgrade_preserves_existing_data_and_rejects_rollback() {
    let mut conn = Connection::open_in_memory().unwrap();
    crate::hub::filesystem::configure(&conn).unwrap();
    crate::migrations::apply(&mut conn, &crate::migrations::PROFILE[..13]).unwrap();
    conn.execute(
        "INSERT INTO settings(key, value) VALUES ('existing', 'preserved')",
        [],
    )
    .unwrap();
    crate::migrations::apply(&mut conn, crate::migrations::PROFILE).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT value FROM settings WHERE key = 'existing'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "preserved"
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM works", [], |r| r.get::<_, usize>(0))
            .unwrap(),
        0
    );
    assert!(crate::migrations::apply(&mut conn, &crate::migrations::PROFILE[..13]).is_err());
}

#[test]
fn work_document_listing_refuses_unknown_versions_and_invalid_content() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&session()).unwrap();
    create(&mut hub);
    hub.profile_conn(1.into())
        .unwrap()
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON; UPDATE works SET schema_version = 999;",
        )
        .unwrap();
    assert!(matches!(
        hub.work_document(
            1.into(),
            WorkRequest::List {
                after: None,
                limit: 1
            }
        ),
        Err(WorkError::Invalid)
    ));
    hub.profile_conn(1.into()).unwrap().execute_batch("UPDATE works SET schema_version = 1, objective = ' '; PRAGMA ignore_check_constraints = OFF;").unwrap();
    assert!(matches!(
        hub.work_document(
            1.into(),
            WorkRequest::List {
                after: None,
                limit: 1
            }
        ),
        Err(WorkError::Invalid)
    ));
}
