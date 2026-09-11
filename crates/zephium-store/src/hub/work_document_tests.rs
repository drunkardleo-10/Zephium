use super::*;
use zephium_core::profiles::ProfileKind;
use zephium_core::session::{PersistedProfile, SessionState};
pub(super) fn session() -> SessionState {
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
pub(super) fn create(hub: &mut Hub) -> WorkSnapshot {
    let WorkReply::Snapshot(work) = hub
        .work_document(
            1.into(),
            WorkRequest::Create {
                author: zephium_core::work::WorkAuthor::User,
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
pub(super) fn draft() -> WorkPlanDraft {
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
pub(super) fn edit(
    hub: &mut Hub,
    work: &WorkSnapshot,
    edit: WorkEdit,
) -> Result<WorkSnapshot, WorkError> {
    match hub.work_document(
        work.profile,
        WorkRequest::Edit {
            author: WorkAuthor::User,
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
                author: zephium_core::work::WorkAuthor::User,
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
            author: zephium_core::work::WorkAuthor::User,
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
    conn.execute("UPDATE work_payload_usage SET bytes = 41943040", [])
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
    hub.profile_conn(1.into()).unwrap().execute_batch("UPDATE works SET schema_version = 2, objective = ' '; PRAGMA ignore_check_constraints = OFF;").unwrap();
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

#[test]
fn work_document_archive_delete_and_restore_recover_active_capacity() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&session()).unwrap();
    let initial = create(&mut hub);
    for n in 1..MAX_ACTIVE_WORKS_PER_PROFILE {
        hub.work_document(
            1.into(),
            WorkRequest::Create {
                id: (1000 + n as u128).into(),
                objective: "Another objective".into(),
                author: WorkAuthor::User,
            },
        )
        .unwrap();
    }
    let extra = || WorkRequest::Create {
        id: 9999.into(),
        objective: "New objective".into(),
        author: WorkAuthor::User,
    };
    assert!(matches!(
        hub.work_document(1.into(), extra()),
        Err(WorkError::Capacity)
    ));
    let archived = edit(&mut hub, &initial, WorkEdit::Archive).unwrap();
    hub.work_document(1.into(), extra()).unwrap();
    assert_eq!(
        edit(&mut hub, &archived, WorkEdit::Restore),
        Err(WorkError::Capacity)
    );
    assert!(matches!(
        hub.work_document(
            1.into(),
            WorkRequest::Delete {
                id: archived.id,
                expected: initial.revision
            }
        ),
        Err(WorkError::Conflict)
    ));
    hub.work_document(
        1.into(),
        WorkRequest::Delete {
            id: 9999.into(),
            expected: WorkRevision::INITIAL,
        },
    )
    .unwrap();
    let restored = edit(&mut hub, &archived, WorkEdit::Restore).unwrap();
    assert_eq!(restored.lifecycle, WorkLifecycle::Active);
    let planned = edit(
        &mut hub,
        &restored,
        WorkEdit::ReplaceDraft { draft: draft() },
    )
    .unwrap();
    hub.work_document(
        1.into(),
        WorkRequest::Delete {
            id: planned.id,
            expected: planned.revision,
        },
    )
    .unwrap();
    assert!(matches!(
        hub.work_document(1.into(), WorkRequest::Read { id: planned.id }),
        Err(WorkError::NotFound)
    ));
    let conn = hub.profile_conn(1.into()).unwrap();
    for table in [
        "work_plans",
        "work_plan_nodes",
        "work_questions",
        "work_events",
    ] {
        let count: usize = conn
            .query_row(
                &format!("SELECT count(*) FROM {table} WHERE work_id = ?1"),
                [planned.id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }
}

#[test]
fn work_document_compaction_recovers_plan_question_and_event_limits_without_resetting_cas() {
    let dir = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(dir.path().into()).unwrap();
    hub.save(&session()).unwrap();
    let mut work = create(&mut hub);
    for n in 0..MAX_WORK_QUESTIONS {
        work = edit(
            &mut hub,
            &work,
            WorkEdit::OpenQuestion {
                id: (200 + n as u128).into(),
                prompt: "Old context".into(),
                options: vec![],
            },
        )
        .unwrap();
    }
    work = edit(
        &mut hub,
        &work,
        WorkEdit::SetObjective {
            objective: "Current context".into(),
        },
    )
    .unwrap();
    for _ in 0..MAX_WORK_PLAN_REVISIONS {
        work = edit(&mut hub, &work, WorkEdit::ReplaceDraft { draft: draft() }).unwrap();
    }
    assert_eq!(
        edit(&mut hub, &work, WorkEdit::ReplaceDraft { draft: draft() }),
        Err(WorkError::Capacity)
    );
    // Fill the content-free event budget using legitimate transitions. Identity
    // revisions remain monotonic after explicit retention maintenance.
    while work.revision.get() < MAX_WORK_EVENTS as u64 {
        let action = if work.lifecycle == WorkLifecycle::Active {
            WorkEdit::Archive
        } else {
            WorkEdit::Restore
        };
        work = edit(&mut hub, &work, action).unwrap();
    }
    assert_eq!(
        edit(&mut hub, &work, WorkEdit::Archive),
        Err(WorkError::Capacity)
    );
    let WorkReply::PlanHistory { revisions } = hub
        .work_document(work.profile, WorkRequest::ListPlans { id: work.id })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(revisions.len(), MAX_WORK_PLAN_REVISIONS);
    let current_plan = work.plan.clone();
    let before = work.revision;
    work = edit(&mut hub, &work, WorkEdit::CompactHistory).unwrap();
    assert!(work.revision > before);
    assert_eq!(work.plan, current_plan);
    assert!(work.questions.is_empty());
    let conn = hub.profile_conn(1.into()).unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM work_plans", [], |r| r
            .get::<_, usize>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM work_events", [], |r| r
            .get::<_, usize>(0))
            .unwrap(),
        65
    );
    if work.lifecycle == WorkLifecycle::Archived {
        work = edit(&mut hub, &work, WorkEdit::Restore).unwrap();
    }
    work = edit(&mut hub, &work, WorkEdit::ReplaceDraft { draft: draft() }).unwrap();
    drop(hub);
    let mut hub = Hub::open(dir.path().into()).unwrap();
    let WorkReply::Snapshot(restored) = hub
        .work_document(1.into(), WorkRequest::Read { id: work.id })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(*restored, work);
    assert!(matches!(
        hub.work_document(
            1.into(),
            WorkRequest::Edit {
                id: work.id,
                expected: before,
                edit: WorkEdit::CompactHistory,
                author: WorkAuthor::User
            }
        ),
        Err(WorkError::Conflict)
    ));
}

#[test]
fn work_document_compaction_failure_rolls_back_history_and_accounting() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&session()).unwrap();
    let initial = create(&mut hub);
    let old = edit(
        &mut hub,
        &initial,
        WorkEdit::ReplaceDraft { draft: draft() },
    )
    .unwrap();
    let current = edit(&mut hub, &old, WorkEdit::ReplaceDraft { draft: draft() }).unwrap();
    let conn = hub.profile_conn(1.into()).unwrap();
    let bytes: i64 = conn
        .query_row("SELECT bytes FROM work_payload_usage", [], |r| r.get(0))
        .unwrap();
    conn.execute_batch("CREATE TRIGGER refuse_compaction BEFORE INSERT ON work_events WHEN NEW.kind = 'history_compacted' BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
    assert!(edit(&mut hub, &current, WorkEdit::CompactHistory).is_err());
    let conn = hub.profile_conn(1.into()).unwrap();
    assert_eq!(
        conn.query_row("SELECT bytes FROM work_payload_usage", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        bytes
    );
    assert_eq!(read(conn, initial.profile, initial.id).unwrap(), current);
    assert_eq!(
        read_plan(conn, initial.id, old.revision).unwrap(),
        old.plan.unwrap()
    );
}

#[test]
fn work_document_v15_migration_preserves_history_with_explicit_unknown_legacy_provenance() {
    let mut conn = legacy_work_fixture();
    crate::migrations::apply(&mut conn, crate::migrations::PROFILE).unwrap();
    let migrated = read(&conn, 1.into(), 10.into()).unwrap();
    assert_eq!(migrated.schema_version, 2);
    assert_eq!(migrated.objective, "Legacy objective");
    assert_eq!(migrated.objective_author, WorkAuthor::LegacyUnknown);
    assert_eq!(
        migrated.questions[0].answer.as_deref(),
        Some("Legacy answer")
    );
    assert_eq!(migrated.questions[0].basis_revision, None);
    assert_eq!(migrated.questions[0].state, WorkQuestionState::Superseded);
    assert_eq!(migrated.current_questions().count(), 0);
    assert!(migrated.plan.is_none());
    let old = read_plan(&conn, migrated.id, WorkRevision::new(4).unwrap()).unwrap();
    assert_eq!(old.author, WorkAuthor::LegacyUnknown);
    assert_eq!(old.draft, draft());
    assert!(crate::migrations::apply(&mut conn, &crate::migrations::PROFILE[..14]).is_err());
    let actual: i64 = conn.query_row("SELECT (SELECT sum(length(CAST(objective AS BLOB))) FROM works) + (SELECT sum(length(CAST(body AS BLOB))) FROM work_questions) + (SELECT sum(length(CAST(body AS BLOB))) FROM work_plan_nodes)", [], |r| r.get(0)).unwrap();
    assert_eq!(
        conn.query_row("SELECT bytes FROM work_payload_usage", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        actual
    );
}

#[test]
fn work_document_v15_migration_refuses_corruption_atomically() {
    let mut conn = legacy_work_fixture();
    conn.execute(
        "UPDATE work_questions SET body = '{\"unexpected\":true}'",
        [],
    )
    .unwrap();
    assert!(crate::migrations::apply(&mut conn, crate::migrations::PROFILE).is_err());
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        14
    );
    assert_eq!(
        conn.query_row("SELECT body FROM work_questions", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "{\"unexpected\":true}"
    );
    crate::migrations::validate_current(&conn, &crate::migrations::PROFILE[..14]).unwrap();
}

fn legacy_work_fixture() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    crate::hub::filesystem::configure(&conn).unwrap();
    crate::migrations::apply(&mut conn, &crate::migrations::PROFILE[..14]).unwrap();
    let id = WorkId::from(10).to_string();
    conn.execute("INSERT INTO works(id, schema_version, revision, status, objective, created_unix_ms, updated_unix_ms) VALUES (?1, 1, 4, 'plan_ready', 'Legacy objective', 0, 0)", [&id]).unwrap();
    conn.execute(
        "INSERT INTO work_plans(work_id, revision, plan_id, basis_revision) VALUES (?1, 4, ?2, 3)",
        params![id, draft().id.to_string()],
    )
    .unwrap();
    let node = draft().nodes.remove(0);
    conn.execute("INSERT INTO work_plan_nodes(work_id, plan_revision, node_id, position, body) VALUES (?1, 4, ?2, 0, ?3)", params![id, node.id.to_string(), encode(&node).unwrap()]).unwrap();
    conn.execute("UPDATE works SET current_plan = 4", [])
        .unwrap();
    let question = WorkQuestionId::from(77);
    let body = serde_json::json!({ "id": question, "prompt": "Legacy question", "options": [], "answer": "Legacy answer" }).to_string();
    conn.execute(
        "INSERT INTO work_questions(work_id, question_id, position, body) VALUES (?1, ?2, 0, ?3)",
        params![id, question.to_string(), body],
    )
    .unwrap();
    for (n, kind) in [
        "created",
        "question_opened",
        "question_answered",
        "draft_replaced",
    ]
    .iter()
    .enumerate()
    {
        conn.execute("INSERT INTO work_events(work_id, revision, recorded_unix_ms, kind) VALUES (?1, ?2, 0, ?3)", params![id, n as i64 + 1, kind]).unwrap();
    }
    conn
}

#[test]
fn authoring_commands_survive_lost_replies_restart_and_deletion_without_reminting() {
    use zephium_core::work::authoring::*;
    let dir = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(dir.path().into()).unwrap();
    hub.save(&session()).unwrap();
    let create = WorkRequest::AuthoringCommand {
        command: 700.into(),
        intent: WorkAuthoringIntent::Create {
            objective: "Research an implementation".into(),
        },
    };
    LOSE_COMMIT_ACK.with(|fault| fault.set(true));
    assert!(matches!(
        hub.work_document(1.into(), create.clone()),
        Err(WorkError::OutcomeUnknown)
    ));
    let WorkReply::AuthoringCommand(created) = hub.work_document(1.into(), create.clone()).unwrap()
    else {
        panic!()
    };
    assert_eq!(created.applied_revision, WorkRevision::INITIAL);
    assert!(matches!(
        hub.work_document(
            1.into(),
            WorkRequest::AuthoringCommand {
                command: created.command,
                intent: WorkAuthoringIntent::Create {
                    objective: "Changed replay".into()
                }
            }
        ),
        Err(WorkError::Conflict)
    ));
    let question = WorkRequest::AuthoringCommand {
        command: 701.into(),
        intent: WorkAuthoringIntent::Edit {
            work: created.work,
            expected_revision: created.applied_revision,
            edit: WorkUserEdit::OpenQuestion {
                prompt: "Which deployment?".into(),
                options: vec!["Desktop".into()],
            },
        },
    };
    let WorkReply::AuthoringCommand(questioned) =
        hub.work_document(1.into(), question.clone()).unwrap()
    else {
        panic!()
    };
    let WorkReply::Snapshot(snapshot) = hub
        .work_document(1.into(), WorkRequest::Read { id: created.work })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(snapshot.questions.len(), 1);
    assert_eq!(snapshot.questions[0].author, WorkAuthor::User);
    drop(hub);
    let mut hub = Hub::open(dir.path().into()).unwrap();
    let WorkReply::AuthoringCommand(replayed) =
        hub.work_document(1.into(), question.clone()).unwrap()
    else {
        panic!()
    };
    assert_eq!(replayed, questioned);
    let WorkReply::Snapshot(restored) = hub
        .work_document(1.into(), WorkRequest::Read { id: created.work })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(restored, snapshot);
    assert!(matches!(
        hub.work_document(2.into(), question),
        Err(WorkError::NotFound)
    ));
    let delete = WorkRequest::AuthoringCommand {
        command: 702.into(),
        intent: WorkAuthoringIntent::Delete {
            work: created.work,
            expected_revision: questioned.applied_revision,
        },
    };
    let WorkReply::AuthoringCommand(deleted) = hub.work_document(1.into(), delete.clone()).unwrap()
    else {
        panic!()
    };
    assert!(deleted.deleted);
    let WorkReply::AuthoringCommand(replayed) =
        hub.work_document(1.into(), create.clone()).unwrap()
    else {
        panic!()
    };
    assert_eq!(replayed, created);
    assert!(matches!(
        hub.work_document(1.into(), WorkRequest::Read { id: created.work }),
        Err(WorkError::NotFound)
    ));
    let WorkReply::AuthoringCommand(replayed) = hub.work_document(1.into(), delete).unwrap() else {
        panic!()
    };
    assert_eq!(replayed, deleted);
    let WorkReply::AuthoringCommand(other_profile) = hub.work_document(2.into(), create).unwrap()
    else {
        panic!()
    };
    assert_ne!(other_profile.work, created.work);
}

#[test]
fn authoring_receipt_capacity_refusal_rolls_back_the_edit() {
    use zephium_core::work::authoring::*;
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&session()).unwrap();
    let work = create(&mut hub);
    let conn = hub.profile_conn(work.profile).unwrap();
    conn.execute_batch("CREATE TRIGGER refuse_test_receipt BEFORE INSERT ON work_authoring_commands BEGIN SELECT RAISE(ABORT, 'Work authoring command capacity exceeded'); END;").unwrap();
    let request = WorkRequest::AuthoringCommand {
        command: 700.into(),
        intent: WorkAuthoringIntent::Edit {
            work: work.id,
            expected_revision: work.revision,
            edit: WorkUserEdit::SetObjective {
                objective: "Must roll back".into(),
            },
        },
    };
    assert!(matches!(
        hub.work_document(work.profile, request.clone()),
        Err(WorkError::Capacity)
    ));
    let WorkReply::Snapshot(unchanged) = hub
        .work_document(work.profile, WorkRequest::Read { id: work.id })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(*unchanged, work);
    hub.profile_conn(work.profile)
        .unwrap()
        .execute_batch("DROP TRIGGER refuse_test_receipt")
        .unwrap();
    assert!(matches!(
        hub.work_document(work.profile, request),
        Ok(WorkReply::AuthoringCommand(_))
    ));
}
