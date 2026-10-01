use super::*;
use rusqlite::Connection;
use zephium_core::ids::ResourceId;

fn database() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    crate::migrations::apply(&mut conn, crate::migrations::PROFILE).unwrap();
    conn
}
fn call(
    conn: &mut Connection,
    profile: ProfileId,
    request: WorkEnvironmentCall,
) -> Result<(WorkEnvironmentReply, bool), WorkError> {
    call_admitted(conn, profile, request, true, true)
}
fn call_admitted(
    conn: &mut Connection,
    profile: ProfileId,
    request: WorkEnvironmentCall,
    space_available: bool,
    browser_available: bool,
) -> Result<(WorkEnvironmentReply, bool), WorkError> {
    let tx = conn.transaction().unwrap();
    let result = super::apply(
        &tx,
        profile,
        request,
        space_available,
        browser_available,
        false,
    )?;
    tx.commit().unwrap();
    Ok(result)
}
fn command(id: u128, intent: WorkEnvironmentIntent) -> WorkEnvironmentCall {
    WorkEnvironmentCall::Command {
        command: id.into(),
        intent,
    }
}
fn take_snapshot(reply: WorkEnvironmentReply) -> WorkEnvironmentSnapshot {
    match reply {
        WorkEnvironmentReply::Snapshot { snapshot }
        | WorkEnvironmentReply::Applied { snapshot, .. }
        | WorkEnvironmentReply::Checkpointed { snapshot, .. } => *snapshot,
        _ => panic!("expected snapshot"),
    }
}
fn create(
    conn: &mut Connection,
    command_id: u128,
    profile: u128,
    space: u128,
) -> WorkEnvironmentSnapshot {
    take_snapshot(
        call(
            conn,
            profile.into(),
            command(
                command_id,
                WorkEnvironmentIntent::Create {
                    space: space.into(),
                    title: "Manual Work".into(),
                },
            ),
        )
        .unwrap()
        .0,
    )
}
fn edit(
    conn: &mut Connection,
    command_id: u128,
    current: &WorkEnvironmentSnapshot,
    edit: WorkEnvironmentEdit,
) -> WorkEnvironmentSnapshot {
    take_snapshot(
        call(
            conn,
            current.profile,
            command(
                command_id,
                WorkEnvironmentIntent::Edit {
                    id: current.id,
                    expected: current.revision,
                    edit,
                },
            ),
        )
        .unwrap()
        .0,
    )
}
fn selected(conn: &mut Connection, space: u128) -> Option<WorkEnvironmentId> {
    let (WorkEnvironmentReply::Page { selected, .. }, changed) = call(
        conn,
        1.into(),
        WorkEnvironmentCall::List {
            space: space.into(),
            after: None,
            limit: 32,
        },
    )
    .unwrap() else {
        panic!()
    };
    assert!(!changed);
    selected
}

#[test]
fn fresh_host_admission_is_required_but_never_reinterprets_retained_receipts() {
    let mut conn = database();
    let create = command(
        1,
        WorkEnvironmentIntent::Create {
            space: 3.into(),
            title: "Manual Work".into(),
        },
    );
    assert!(matches!(
        call_admitted(&mut conn, 1.into(), create.clone(), false, true),
        Err(WorkError::NotFound)
    ));
    assert!(matches!(
        call_admitted(
            &mut conn,
            1.into(),
            WorkEnvironmentCall::List {
                space: 3.into(),
                after: None,
                limit: 32
            },
            false,
            true
        ),
        Err(WorkError::NotFound)
    ));
    let initial = take_snapshot(call(&mut conn, 1.into(), create.clone()).unwrap().0);
    let add = command(
        2,
        WorkEnvironmentIntent::Edit {
            id: initial.id,
            expected: initial.revision,
            edit: WorkEnvironmentEdit::Add {
                reference: WorkEnvironmentReference::Browser { tab: 9.into() },
                area: None,
            },
        },
    );
    assert!(matches!(
        call_admitted(&mut conn, 1.into(), add.clone(), true, false),
        Err(WorkError::NotFound)
    ));
    let attached = take_snapshot(call(&mut conn, 1.into(), add.clone()).unwrap().0);
    for request in [create, add] {
        let (
            WorkEnvironmentReply::Applied {
                replayed, snapshot, ..
            },
            changed,
        ) = call_admitted(&mut conn, 1.into(), request, false, false).unwrap()
        else {
            panic!()
        };
        assert!(replayed && !changed);
        assert_eq!(*snapshot, attached);
    }
}

#[test]
fn selection_corruption_cannot_select_an_environment_from_another_space() {
    let mut conn = database();
    let first = create(&mut conn, 1, 1, 3);
    let other = create(&mut conn, 2, 1, 4);
    conn.execute(
        "UPDATE work_environment_selection SET environment_id=?1 WHERE space_id=?2",
        params![other.id.to_string(), first.space.to_string()],
    )
    .unwrap();
    assert!(matches!(
        call(
            &mut conn,
            first.profile,
            WorkEnvironmentCall::List {
                space: first.space,
                after: None,
                limit: 32
            }
        ),
        Err(WorkError::Unavailable)
    ));
}

#[test]
fn profile18_migration_and_reopen_preserve_objectives_selection_and_exact_receipts() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("profile.sqlite");
    let mut conn = Connection::open(&path).unwrap();
    crate::migrations::apply(&mut conn, &crate::migrations::PROFILE[..28]).unwrap();
    insert_objective(&mut conn, 1.into(), 60.into());
    crate::migrations::apply(&mut conn, crate::migrations::PROFILE).unwrap();
    let initial = create(&mut conn, 1, 1, 3);
    let attached = edit(
        &mut conn,
        2,
        &initial,
        WorkEnvironmentEdit::Add {
            reference: WorkEnvironmentReference::Objective {
                objective: 60.into(),
            },
            area: None,
        },
    );
    drop(conn);
    let mut reopened = Connection::open(&path).unwrap();
    crate::migrations::apply(&mut reopened, crate::migrations::PROFILE).unwrap();
    assert_eq!(selected(&mut reopened, 3), Some(initial.id));
    let (
        WorkEnvironmentReply::Applied {
            replayed,
            applied_revision,
            snapshot,
            ..
        },
        changed,
    ) = call(
        &mut reopened,
        1.into(),
        command(
            1,
            WorkEnvironmentIntent::Create {
                space: 3.into(),
                title: "Manual Work".into(),
            },
        ),
    )
    .unwrap()
    else {
        panic!()
    };
    assert!(replayed && !changed);
    assert_eq!(applied_revision, initial.revision);
    assert_eq!(*snapshot, attached);
    let tx = reopened.transaction().unwrap();
    let objective = super::read_objective(&tx, 1.into(), 60.into()).unwrap();
    assert_eq!(objective.objective, "Existing objective");
    assert_eq!(objective.revision, WorkRevision::INITIAL);
}

#[test]
fn manual_creation_replays_after_later_revisions_without_creating_objectives() {
    let mut conn = database();
    let create = command(
        10,
        WorkEnvironmentIntent::Create {
            space: 3.into(),
            title: "Manual Work".into(),
        },
    );
    let (first, changed) = call(&mut conn, 1.into(), create.clone()).unwrap();
    assert!(changed);
    let initial = take_snapshot(first);
    assert!(initial.elements.is_empty());
    let renamed = edit(
        &mut conn,
        11,
        &initial,
        WorkEnvironmentEdit::Rename {
            title: "Renamed later".into(),
        },
    );
    let (
        WorkEnvironmentReply::Applied {
            command: receipt,
            applied_revision,
            applied_view_revision,
            replayed,
            snapshot,
        },
        changed,
    ) = call(&mut conn, 1.into(), create).unwrap()
    else {
        panic!()
    };
    assert_eq!(receipt, 10.into());
    assert!(replayed);
    assert!(!changed);
    assert_eq!(applied_revision, initial.revision);
    assert_eq!(applied_view_revision, initial.view.revision);
    assert_eq!(*snapshot, renamed);
    for table in ["works", "work_executions"] {
        let count: u32 = conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
    assert_eq!(
        conn.query_row("SELECT count(*) FROM work_environments", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert!(matches!(
        call(
            &mut conn,
            1.into(),
            command(
                10,
                WorkEnvironmentIntent::Create {
                    space: 3.into(),
                    title: "Different operand".into()
                }
            )
        ),
        Err(WorkError::Conflict)
    ));
}

#[test]
fn edits_use_semantic_cas_and_exact_receipts_survive_later_changes() {
    let mut conn = database();
    let initial = create(&mut conn, 1, 1, 3);
    let request = command(
        2,
        WorkEnvironmentIntent::Edit {
            id: initial.id,
            expected: initial.revision,
            edit: WorkEnvironmentEdit::Rename {
                title: "First edit".into(),
            },
        },
    );
    let first = take_snapshot(call(&mut conn, initial.profile, request.clone()).unwrap().0);
    let later = edit(
        &mut conn,
        3,
        &first,
        WorkEnvironmentEdit::Rename {
            title: "Later edit".into(),
        },
    );
    let (
        WorkEnvironmentReply::Applied {
            applied_revision,
            replayed,
            snapshot,
            ..
        },
        changed,
    ) = call(&mut conn, initial.profile, request).unwrap()
    else {
        panic!()
    };
    assert!(replayed && !changed);
    assert_eq!(applied_revision, first.revision);
    assert_eq!(*snapshot, later);
    for request in [
        command(
            4,
            WorkEnvironmentIntent::Edit {
                id: initial.id,
                expected: initial.revision,
                edit: WorkEnvironmentEdit::Rename {
                    title: "Stale".into(),
                },
            },
        ),
        command(
            2,
            WorkEnvironmentIntent::Edit {
                id: initial.id,
                expected: later.revision,
                edit: WorkEnvironmentEdit::Rename {
                    title: "First edit".into(),
                },
            },
        ),
        command(
            2,
            WorkEnvironmentIntent::Edit {
                id: initial.id,
                expected: initial.revision,
                edit: WorkEnvironmentEdit::Rename {
                    title: "Changed operand".into(),
                },
            },
        ),
    ] {
        assert!(matches!(
            call(&mut conn, initial.profile, request),
            Err(WorkError::Conflict)
        ));
    }
    let read = take_snapshot(
        call(
            &mut conn,
            initial.profile,
            WorkEnvironmentCall::Read { id: initial.id },
        )
        .unwrap()
        .0,
    );
    assert_eq!(read, later);
}

#[test]
fn checkpoint_revision_is_separate_and_cannot_resurrect_removed_elements() {
    let mut conn = database();
    let initial = create(&mut conn, 1, 1, 3);
    let attached = edit(
        &mut conn,
        2,
        &initial,
        WorkEnvironmentEdit::Add {
            reference: WorkEnvironmentReference::Browser { tab: 9.into() },
            area: None,
        },
    );
    let mut view = attached.view.clone();
    view.x = 45;
    view.placements.push(WorkElementPlacement {
        element: attached.elements[0].id,
        x: -80,
        y: 140,
        width: 320,
        height: 200,
        revision: 2,
    });
    let request = WorkEnvironmentCall::Checkpoint {
        id: attached.id,
        expected: view.revision,
        view: view.clone(),
    };
    let checkpoint = take_snapshot(
        call(&mut conn, attached.profile, request.clone())
            .unwrap()
            .0,
    );
    assert_eq!(checkpoint.revision, attached.revision);
    assert_eq!(
        checkpoint.view.revision,
        attached.view.revision.next().unwrap()
    );
    assert_eq!(checkpoint.view.x, 45);
    assert_eq!(checkpoint.view.placements[0].revision, 2);
    // An unrelated semantic edit does not make presentation stale.
    let renamed = edit(
        &mut conn,
        4,
        &checkpoint,
        WorkEnvironmentEdit::Rename {
            title: "New semantic title".into(),
        },
    );
    let mut changed_view = checkpoint.view.clone();
    changed_view.y = 84;
    let second = take_snapshot(
        call(
            &mut conn,
            attached.profile,
            WorkEnvironmentCall::Checkpoint {
                id: attached.id,
                expected: changed_view.revision,
                view: changed_view,
            },
        )
        .unwrap()
        .0,
    );
    assert_eq!(second.revision, renamed.revision);
    let removed = edit(
        &mut conn,
        6,
        &second,
        WorkEnvironmentEdit::Remove {
            element: attached.elements[0].id,
        },
    );
    assert!(removed.view.placements.is_empty());
    assert_eq!(removed.view.revision, second.view.revision.next().unwrap());
    assert!(matches!(
        call(
            &mut conn,
            attached.profile,
            WorkEnvironmentCall::Checkpoint {
                id: attached.id,
                expected: view.revision,
                view: {
                    let mut changed = view.clone();
                    changed.x += 1;
                    changed
                }
            }
        ),
        Err(WorkError::Conflict)
    ));
    // Even a freshly echoed revision cannot make a dangling placement valid.
    view.revision = removed.view.revision;
    assert!(matches!(
        call(
            &mut conn,
            attached.profile,
            WorkEnvironmentCall::Checkpoint {
                id: attached.id,
                expected: view.revision,
                view
            }
        ),
        Err(WorkError::Invalid)
    ));
    let (
        WorkEnvironmentReply::Checkpointed {
            replayed,
            applied_view_revision,
            snapshot,
            ..
        },
        changed,
    ) = call(&mut conn, attached.profile, request).unwrap()
    else {
        panic!()
    };
    assert!(replayed && !changed);
    assert_eq!(applied_view_revision, checkpoint.view.revision);
    assert_eq!(*snapshot, removed);
}

#[test]
fn invalid_area_and_view_writes_leave_no_receipts_or_partial_changes() {
    let mut conn = database();
    let initial = create(&mut conn, 1, 1, 3);
    let invalid = command(
        2,
        WorkEnvironmentIntent::Edit {
            id: initial.id,
            expected: initial.revision,
            edit: WorkEnvironmentEdit::Add {
                reference: WorkEnvironmentReference::Browser { tab: 9.into() },
                area: Some(404.into()),
            },
        },
    );
    assert!(matches!(
        call(&mut conn, initial.profile, invalid),
        Err(WorkError::Invalid)
    ));
    let area = edit(
        &mut conn,
        2,
        &initial,
        WorkEnvironmentEdit::CreateArea {
            title: "Sources".into(),
        },
    );
    let attached = edit(
        &mut conn,
        3,
        &area,
        WorkEnvironmentEdit::Add {
            reference: WorkEnvironmentReference::Browser { tab: 9.into() },
            area: Some(area.areas[0].id),
        },
    );
    let removed = edit(
        &mut conn,
        4,
        &attached,
        WorkEnvironmentEdit::RemoveArea {
            area: area.areas[0].id,
        },
    );
    assert_eq!(removed.elements.len(), 1);
    assert!(removed.elements[0].area.is_none());
    let mut invalid_view = removed.view.clone();
    invalid_view.zoom_milli = 0;
    assert!(matches!(
        call(
            &mut conn,
            initial.profile,
            WorkEnvironmentCall::Checkpoint {
                id: initial.id,
                expected: invalid_view.revision,
                view: invalid_view
            }
        ),
        Err(WorkError::Invalid)
    ));
    assert_eq!(
        take_snapshot(
            call(
                &mut conn,
                initial.profile,
                WorkEnvironmentCall::Read { id: initial.id }
            )
            .unwrap()
            .0
        ),
        removed
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM work_environment_commands", [], |r| {
            r.get::<_, u32>(0)
        })
        .unwrap(),
        4
    );
}

fn insert_resource(conn: &Connection, id: ResourceId, trashed: bool) {
    conn.execute("INSERT INTO user_resources(id,kind,revision,title,pinned,trashed,created_at,updated_at,body,search_text) VALUES (?1,'note',1,'Note',0,?2,1,1,'{}','Note')", params![id.to_string(), trashed]).unwrap();
}
fn insert_objective(conn: &mut Connection, profile: ProfileId, id: WorkId) {
    let tx = conn.transaction().unwrap();
    super::super::apply(
        &tx,
        profile,
        super::super::runtime_store::RuntimeClock {
            session: 1.into(),
            tick_ms: 0,
        },
        zephium_core::work::port::WorkRequest::Create {
            id,
            objective: "Existing objective".into(),
            author: WorkAuthor::User,
        },
    )
    .unwrap();
    tx.commit().unwrap();
}

#[test]
fn references_resolve_only_in_the_exact_profile_database_and_refuse_trash() {
    let mut first = database();
    let mut other = database();
    let initial = create(&mut first, 1, 1, 3);
    let peer = create(&mut other, 1, 2, 3);
    insert_resource(&first, 50.into(), false);
    insert_resource(&first, 51.into(), true);
    insert_objective(&mut first, 1.into(), 60.into());
    for reference in [
        WorkEnvironmentReference::Resource {
            resource: 50.into(),
        },
        WorkEnvironmentReference::Objective {
            objective: 60.into(),
        },
    ] {
        assert!(matches!(
            call(
                &mut other,
                peer.profile,
                command(
                    2,
                    WorkEnvironmentIntent::Edit {
                        id: peer.id,
                        expected: peer.revision,
                        edit: WorkEnvironmentEdit::Add {
                            reference,
                            area: None
                        }
                    }
                )
            ),
            Err(WorkError::NotFound)
        ));
    }
    assert!(matches!(
        call(
            &mut first,
            initial.profile,
            command(
                2,
                WorkEnvironmentIntent::Edit {
                    id: initial.id,
                    expected: initial.revision,
                    edit: WorkEnvironmentEdit::Add {
                        reference: WorkEnvironmentReference::Resource {
                            resource: 51.into()
                        },
                        area: None
                    }
                }
            )
        ),
        Err(WorkError::NotFound)
    ));
    let resource = edit(
        &mut first,
        2,
        &initial,
        WorkEnvironmentEdit::Add {
            reference: WorkEnvironmentReference::Resource {
                resource: 50.into(),
            },
            area: None,
        },
    );
    let objective = edit(
        &mut first,
        3,
        &resource,
        WorkEnvironmentEdit::Add {
            reference: WorkEnvironmentReference::Objective {
                objective: 60.into(),
            },
            area: None,
        },
    );
    assert_eq!(objective.elements.len(), 2);
    assert!(matches!(
        call(
            &mut first,
            2.into(),
            WorkEnvironmentCall::Read { id: initial.id }
        ),
        Err(WorkError::Unavailable)
    ));
    assert!(matches!(
        call(
            &mut other,
            2.into(),
            WorkEnvironmentCall::Read { id: initial.id }
        ),
        Err(WorkError::NotFound)
    ));
}

#[test]
fn selection_is_space_scoped_and_persists_across_reads_replay_and_archival() {
    let mut conn = database();
    let first = create(&mut conn, 1, 1, 3);
    let second = create(&mut conn, 2, 1, 3);
    let other_space = create(&mut conn, 3, 1, 4);
    assert_eq!(selected(&mut conn, 3), Some(second.id));
    assert_eq!(selected(&mut conn, 4), Some(other_space.id));
    call(
        &mut conn,
        1.into(),
        WorkEnvironmentCall::Open { id: first.id },
    )
    .unwrap();
    let reopen = take_snapshot(
        call(
            &mut conn,
            1.into(),
            WorkEnvironmentCall::Open { id: first.id },
        )
        .unwrap()
        .0,
    );
    assert_eq!(reopen, first);
    // Migration preflight and unrelated reads preserve the durable selection.
    crate::migrations::apply(&mut conn, crate::migrations::PROFILE).unwrap();
    call(
        &mut conn,
        1.into(),
        WorkEnvironmentCall::Read { id: second.id },
    )
    .unwrap();
    assert_eq!(selected(&mut conn, 3), Some(first.id));
    let archived = edit(
        &mut conn,
        4,
        &first,
        WorkEnvironmentEdit::SetLifecycle {
            lifecycle: WorkLifecycle::Archived,
        },
    );
    assert_eq!(selected(&mut conn, 3), None);
    assert_eq!(selected(&mut conn, 4), Some(other_space.id));
    assert!(matches!(
        call(
            &mut conn,
            1.into(),
            WorkEnvironmentCall::Open { id: archived.id }
        ),
        Err(WorkError::Conflict)
    ));
    call(
        &mut conn,
        1.into(),
        command(
            1,
            WorkEnvironmentIntent::Create {
                space: 3.into(),
                title: "Manual Work".into(),
            },
        ),
    )
    .unwrap();
    assert_eq!(
        selected(&mut conn, 3),
        None,
        "replayed create must not select an archived environment"
    );
}

fn checkpoint_call(snapshot: &WorkEnvironmentSnapshot, x: i32) -> WorkEnvironmentCall {
    let mut view = snapshot.view.clone();
    view.x = x;
    WorkEnvironmentCall::Checkpoint {
        id: snapshot.id,
        expected: view.revision,
        view,
    }
}

#[test]
fn checkpoints_outlive_global_command_capacity_with_bounded_exact_replay() {
    let mut conn = database();
    let mut snapshot = create(&mut conn, 1, 1, 3);
    let first = checkpoint_call(&snapshot, 1);
    let mut retained = first.clone();
    for index in 1..=8205 {
        let request = checkpoint_call(&snapshot, index);
        if index == 8200 {
            retained = request.clone();
        }
        snapshot = take_snapshot(call(&mut conn, snapshot.profile, request).unwrap().0);
    }
    assert_eq!(snapshot.view.revision.get(), 8206);
    assert_eq!(snapshot.revision, WorkRevision::INITIAL);
    assert_eq!(
        conn.query_row("SELECT count(*) FROM work_environment_commands", [], |r| {
            r.get::<_, u32>(0)
        })
        .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM work_environment_checkpoints",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        64
    );
    let before = snapshot.clone();
    assert!(
        matches!(
            call(&mut conn, snapshot.profile, first),
            Err(WorkError::Conflict)
        ),
        "evicted expected revision never applies again"
    );
    let (
        WorkEnvironmentReply::Checkpointed {
            expected,
            applied_view_revision,
            replayed,
            snapshot: replay,
        },
        changed,
    ) = call_admitted(&mut conn, snapshot.profile, retained.clone(), false, false).unwrap()
    else {
        panic!()
    };
    assert!(replayed && !changed);
    assert_eq!(expected.get(), 8200);
    assert_eq!(applied_view_revision.get(), 8201);
    assert_eq!(*replay, before);
    let WorkEnvironmentCall::Checkpoint { view, .. } = &mut retained else {
        panic!()
    };
    view.x += 1;
    assert!(matches!(
        call(&mut conn, snapshot.profile, retained),
        Err(WorkError::Conflict)
    ));
    // Frequent layout writes do not prevent later semantic edits or creation.
    let renamed = edit(
        &mut conn,
        2,
        &snapshot,
        WorkEnvironmentEdit::Rename {
            title: "Still editable".into(),
        },
    );
    assert_eq!(renamed.title, "Still editable");
    let another = create(&mut conn, 3, 1, 3);
    assert_eq!(another.view.revision, WorkRevision::INITIAL);
}

#[test]
fn archived_checkpoint_replay_is_exact_but_fresh_writes_are_refused() {
    let mut conn = database();
    let initial = create(&mut conn, 1, 1, 3);
    let original = checkpoint_call(&initial, 17);
    let applied = take_snapshot(
        call(&mut conn, initial.profile, original.clone())
            .unwrap()
            .0,
    );
    let archived = edit(
        &mut conn,
        2,
        &applied,
        WorkEnvironmentEdit::SetLifecycle {
            lifecycle: WorkLifecycle::Archived,
        },
    );
    let (
        WorkEnvironmentReply::Checkpointed {
            expected,
            applied_view_revision,
            replayed,
            snapshot,
        },
        changed,
    ) = call(&mut conn, initial.profile, original.clone()).unwrap()
    else {
        panic!()
    };
    assert!(replayed && !changed);
    assert_eq!(expected, initial.view.revision);
    assert_eq!(applied_view_revision, applied.view.revision);
    assert_eq!(*snapshot, archived);
    assert!(matches!(
        call(&mut conn, initial.profile, checkpoint_call(&archived, 18)),
        Err(WorkError::Conflict)
    ));
    let restored = edit(
        &mut conn,
        3,
        &archived,
        WorkEnvironmentEdit::SetLifecycle {
            lifecycle: WorkLifecycle::Active,
        },
    );
    let newer = take_snapshot(
        call(&mut conn, initial.profile, checkpoint_call(&restored, 19))
            .unwrap()
            .0,
    );
    assert_eq!(newer.view.revision, applied.view.revision.next().unwrap());
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM work_environment_checkpoints",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        2
    );
}

#[test]
fn schema19_upgrade_retains_old_receipts_and_checkpoint_cas_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profile.sqlite");
    let mut conn = Connection::open(&path).unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    crate::migrations::apply(&mut conn, &crate::migrations::PROFILE[..29]).unwrap();
    let initial = create(&mut conn, 1, 1, 3);
    // A provisional schema19 checkpoint receipt is historical metadata. Upgrade
    // retains its digest verbatim; no old random ID becomes a new checkpoint.
    let old_digest = vec![7_u8; 32];
    conn.execute("INSERT INTO work_environment_commands(command_id,digest,environment_id,revision,view_revision) VALUES (?1,?2,?3,1,1)",params![WorkCommandId::from(9_u128).to_string(),old_digest,initial.id.to_string()]).unwrap();
    crate::migrations::apply(&mut conn, crate::migrations::PROFILE).unwrap();
    assert!(crate::migrations::apply(&mut conn, &crate::migrations::PROFILE[..29]).is_err());
    assert!(crate::migrations::apply(&mut conn, &crate::migrations::PROFILE[..28]).is_err());
    assert!(crate::migrations::apply(&mut conn, &crate::migrations::PROFILE[..25]).is_err());
    assert_eq!(
        conn.query_row(
            "SELECT digest FROM work_environment_commands WHERE command_id=?1",
            [WorkCommandId::from(9_u128).to_string()],
            |r| r.get::<_, Vec<u8>>(0)
        )
        .unwrap(),
        vec![7_u8; 32]
    );
    let winner = checkpoint_call(&initial, 10);
    let loser = checkpoint_call(&initial, 20);
    let applied = take_snapshot(call(&mut conn, initial.profile, winner.clone()).unwrap().0);
    drop(conn);
    let mut reopened = Connection::open(&path).unwrap();
    crate::migrations::apply(&mut reopened, crate::migrations::PROFILE).unwrap();
    assert!(matches!(
        call(&mut reopened, initial.profile, loser),
        Err(WorkError::Conflict)
    ));
    let (
        WorkEnvironmentReply::Checkpointed {
            replayed, snapshot, ..
        },
        changed,
    ) = call(&mut reopened, initial.profile, winner).unwrap()
    else {
        panic!()
    };
    assert!(replayed && !changed);
    assert_eq!(*snapshot, applied);
    assert!(matches!(
        call(&mut reopened, 2.into(), checkpoint_call(&applied, 30)),
        Err(WorkError::Unavailable)
    ));
}

fn listed(conn: &mut Connection, space: u128) -> Vec<WorkEnvironmentSummary> {
    match call(
        conn,
        1.into(),
        WorkEnvironmentCall::List {
            space: space.into(),
            after: None,
            limit: 32,
        },
    )
    .unwrap()
    .0
    {
        WorkEnvironmentReply::Page { works, .. } => works,
        _ => panic!("expected a page"),
    }
}

#[test]
fn the_list_says_what_each_work_asked_and_whether_it_holds_anything() {
    let mut conn = database();
    let draft = create(&mut conn, 1, 1, 3);
    let used = create(&mut conn, 2, 1, 3);
    insert_objective(&mut conn, 1.into(), 60.into());
    edit(
        &mut conn,
        3,
        &used,
        WorkEnvironmentEdit::Add {
            reference: WorkEnvironmentReference::Objective {
                objective: 60.into(),
            },
            area: None,
        },
    );
    let works = listed(&mut conn, 3);
    let draft = works.iter().find(|work| work.id == draft.id).unwrap();
    assert!(draft.empty);
    assert!(draft.requests.is_empty());
    assert_eq!(draft.name, None);
    let used = works.iter().find(|work| work.id == used.id).unwrap();
    assert!(!used.empty);
    assert_eq!(used.requests, vec!["Existing objective".to_owned()]);
    assert_ne!(used.touched_ms, "0");
}
