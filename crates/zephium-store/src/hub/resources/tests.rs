use super::*;
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
    let first = applied(mutate(&mut conn, create.clone()).unwrap());
    drop(conn);
    let mut conn = connection(&path);
    let replay = applied(mutate(&mut conn, create.clone()).unwrap());
    assert_eq!(first, replay);
    let mut draft = task();
    draft.title = "Changed".into();
    assert!(matches!(
        mutate(
            &mut conn,
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
