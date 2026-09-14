use super::*;
use zephium_core::ids::ProfileId;
fn task() -> ResourceDraft {
    ResourceDraft {
        title: "Research a topic".into(),
        pinned: false,
        content: ResourceContent::Task {
            description: "Keep the evidence".into(),
            completed: false,
            due_date: Some("2028-02-29".into()),
        },
        related: vec![],
    }
}
fn command(key: &str, intent: ResourceIntent) -> ResourceCommand {
    ResourceCommand {
        version: 1,
        request_id: format!("request-{key:0>16}"),
        intent,
    }
}
fn connection(path: &std::path::Path) -> Connection {
    let mut conn = Connection::open(path).unwrap();
    configure(&conn).unwrap();
    migrations::apply(&mut conn, migrations::PROFILE).unwrap();
    conn
}
fn applied(response: ResourceResponse) -> ResourceRecord {
    match response {
        ResourceResponse::Applied { record, .. } => record,
        other => panic!("unexpected {other:?}"),
    }
}
#[test]
fn durable_create_retry_conflict_trash_and_restore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resources.sqlite");
    let mut conn = connection(&path);
    let create = command("create", ResourceIntent::Create { draft: task() });
    let first = applied(mutate(&mut conn, ProfileId::from(1), create.clone()).unwrap());
    drop(conn);
    let mut conn = connection(&path);
    let replay = applied(mutate(&mut conn, ProfileId::from(1), create.clone()).unwrap());
    assert_eq!(first, replay);
    let mut draft = task();
    draft.title = "Changed".into();
    assert!(matches!(
        mutate(
            &mut conn,
            ProfileId::from(1),
            ResourceCommand {
                intent: ResourceIntent::Create {
                    draft: draft.clone()
                },
                ..create
            }
        )
        .unwrap(),
        ResourceResponse::Error {
            error: ResourceError::Conflict
        }
    ));
    let updated = applied(
        mutate(
            &mut conn,
            ProfileId::from(1),
            command(
                "edit",
                ResourceIntent::Replace {
                    id: first.id.clone(),
                    expected_revision: first.revision.clone(),
                    draft,
                },
            ),
        )
        .unwrap(),
    );
    assert_eq!(updated.revision, "2");
    assert!(matches!(
        mutate(
            &mut conn,
            ProfileId::from(1),
            command(
                "stale",
                ResourceIntent::Trash {
                    id: first.id.clone(),
                    expected_revision: first.revision
                }
            )
        )
        .unwrap(),
        ResourceResponse::Error {
            error: ResourceError::Conflict
        }
    ));
    let deleted = applied(
        mutate(
            &mut conn,
            ProfileId::from(1),
            command(
                "trash",
                ResourceIntent::Trash {
                    id: first.id,
                    expected_revision: updated.revision,
                },
            ),
        )
        .unwrap(),
    );
    assert!(deleted.trashed);
    let restored = applied(
        mutate(
            &mut conn,
            ProfileId::from(1),
            command(
                "restore",
                ResourceIntent::Restore {
                    id: deleted.id,
                    expected_revision: deleted.revision,
                },
            ),
        )
        .unwrap(),
    );
    assert!(!restored.trashed);
    assert_eq!(restored.draft.title, "Changed");
}
#[test]
fn unknown_profiles_and_cross_profile_links_fail_closed() {
    let mut hub = Hub::in_memory().unwrap();
    let a = ProfileId::from(1);
    let b = ProfileId::from(2);
    let call = ResourceCall::Mutate {
        command: Box::new(command("a", ResourceIntent::Create { draft: task() })),
    };
    assert!(matches!(
        hub.resource_call(a, call.clone()),
        ResourceResponse::Error {
            error: ResourceError::Unavailable
        }
    ));
    hub.registry.insert(a);
    hub.registry.insert(b);
    let record = applied(hub.resource_call(a, call));
    assert!(matches!(
        hub.resource_call(
            b,
            ResourceCall::Get {
                id: record.id.clone()
            }
        ),
        ResourceResponse::Error {
            error: ResourceError::NotFound
        }
    ));
    let mut draft = task();
    draft.related.push(record.id);
    assert!(matches!(
        hub.resource_call(
            b,
            ResourceCall::Mutate {
                command: Box::new(command("b", ResourceIntent::Create { draft }))
            }
        ),
        ResourceResponse::Error {
            error: ResourceError::NotFound
        }
    ));
}
#[test]
fn acknowledged_updates_still_cannot_reapply_an_old_revision() {
    let mut hub = Hub::in_memory().unwrap();
    let profile = ProfileId::from(1);
    hub.registry.insert(profile);
    let create = command("create", ResourceIntent::Create { draft: task() });
    let first = applied(hub.resource_call(
        profile,
        ResourceCall::Mutate {
            command: Box::new(create.clone()),
        },
    ));
    hub.resource_call(
        profile,
        ResourceCall::Acknowledge {
            request_id: create.request_id.clone(),
        },
    );
    assert_eq!(
        applied(hub.resource_call(
            profile,
            ResourceCall::Mutate {
                command: Box::new(create)
            }
        ))
        .id,
        first.id
    );
    let update = command(
        "update",
        ResourceIntent::Replace {
            id: first.id,
            expected_revision: first.revision,
            draft: task(),
        },
    );
    hub.resource_call(
        profile,
        ResourceCall::Mutate {
            command: Box::new(update.clone()),
        },
    );
    hub.resource_call(
        profile,
        ResourceCall::Acknowledge {
            request_id: update.request_id.clone(),
        },
    );
    assert!(matches!(
        hub.resource_call(
            profile,
            ResourceCall::Mutate {
                command: Box::new(update)
            }
        ),
        ResourceResponse::Error {
            error: ResourceError::Conflict
        }
    ));
}

#[test]
fn schema21_widens_resource_kinds_and_keeps_receipts_joined() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resources.sqlite");
    let mut conn = Connection::open(&path).unwrap();
    configure(&conn).unwrap();
    migrations::apply(&mut conn, &migrations::PROFILE[..20]).unwrap();
    let create = command("create", ResourceIntent::Create { draft: task() });
    let first = applied(mutate(&mut conn, ProfileId::from(1), create.clone()).unwrap());
    let bytes_before: i64 = conn
        .query_row(
            "SELECT bytes FROM user_resource_usage WHERE id=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    migrations::apply(&mut conn, migrations::PROFILE).unwrap();
    assert!(migrations::apply(&mut conn, &migrations::PROFILE[..20]).is_err());
    let violations: i64 = conn
        .query_row(
            "SELECT count(*) FROM pragma_foreign_key_check('user_resource_receipts')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(violations, 0);
    let bytes_after: i64 = conn
        .query_row(
            "SELECT bytes FROM user_resource_usage WHERE id=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(bytes_before, bytes_after);
    let joined: i64 = conn
        .query_row(
            "SELECT count(*) FROM user_resource_receipts r JOIN user_resources u ON u.id=r.resource_id WHERE r.request_id=?1",
            [&create.request_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(joined, 1);
    let replay = applied(mutate(&mut conn, ProfileId::from(1), create).unwrap());
    assert_eq!(replay.id, first.id);
    let object = ResourceDraft {
        title: "Shortlist".into(),
        pinned: false,
        content: ResourceContent::Object {
            object: WorkObjectV1 {
                version: 1,
                data: zephium_core::work::artifact::WorkArtifactDataV1::Checklist {
                    items: vec![zephium_core::work::artifact::WorkChecklistItem {
                        text: "Compare pricing".into(),
                        completed: false,
                    }],
                },
                evidence: vec![],
                provenance: None,
            },
        },
        related: vec![],
    };
    let created = applied(
        mutate(
            &mut conn,
            ProfileId::from(1),
            command("object", ResourceIntent::Create { draft: object }),
        )
        .unwrap(),
    );
    assert_eq!(created.draft.kind(), ResourceKind::Object);
    let page = list(
        &conn,
        ResourceQuery {
            completed: None,
            kind: ResourceKind::Object,
            search: "pricing".into(),
            trashed: false,
            after: None,
            limit: 10,
        },
    )
    .unwrap();
    let ResourceResponse::Page { items, .. } = page else {
        panic!("expected page");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title, "Shortlist");
}
