use super::work_planning_tests::{drive, fixture};
use crate::work_context::WorkContextAdmission;
use crate::Command;
use std::time::Duration;
use zephium_core::ids::ResourceId;
use zephium_core::ports::engine::EngineEvent;
use zephium_core::resources::*;
use zephium_core::work::{context::*, environment::*, port::*, *};

async fn environment(
    shell: &mut crate::Shell,
    queue: &crate::actor::CommandQueue,
    handle: &crate::Handle,
    profile: zephium_core::ids::ProfileId,
    call: WorkEnvironmentCall,
) -> WorkEnvironmentSnapshot {
    let request = handle
        .submit_work_document(
            WorkRequest::Environment {
                call,
                space_available: true,
                browser_available: true,
            },
            Some(profile),
        )
        .unwrap();
    let projection = drive(shell, queue, request).await.unwrap();
    match projection.reply {
        WorkReply::Environment(WorkEnvironmentReply::Applied { snapshot, .. })
        | WorkReply::Environment(WorkEnvironmentReply::Snapshot { snapshot }) => *snapshot,
        _ => panic!("expected an environment snapshot"),
    }
}

#[tokio::test]
async fn context_admission_binds_digests_and_refuses_stale_or_private_public_reads() {
    let store = std::sync::Arc::new(zephium_store::SqliteStore::in_memory().unwrap());
    let (mut shell, queue, handle, profile) = fixture(store);
    let space = shell.windows.focused().unwrap().space;
    let tab = shell.windows.focused().unwrap().active.unwrap();
    shell.handle(Command::Navigate {
        id: tab,
        input: "https://docs.example/guide".into(),
    });
    shell.handle(Command::Engine(EngineEvent::UrlChanged {
        id: tab,
        url: "https://docs.example/guide".into(),
    }));
    shell.handle(Command::Engine(EngineEvent::TitleChanged {
        id: tab,
        title: "Guide".into(),
    }));

    let receiver = handle.resource_call(
        profile,
        ResourceCall::Mutate {
            command: Box::new(ResourceCommand {
                version: 1,
                request_id: "context-admission-note-0001".into(),
                intent: ResourceIntent::Create {
                    draft: ResourceDraft {
                        title: "Keyboard notes".into(),
                        pinned: false,
                        content: ResourceContent::Note {
                            document: NoteDocument {
                                version: 1,
                                document: DocumentNode {
                                    kind: "doc".into(),
                                    content: vec![DocumentNode {
                                        kind: "paragraph".into(),
                                        content: vec![DocumentNode {
                                            kind: "text".into(),
                                            content: vec![],
                                            text: Some("Budget 150 EUR, tenkeyless".into()),
                                            attrs: None,
                                            marks: vec![],
                                        }],
                                        text: None,
                                        attrs: None,
                                        marks: vec![],
                                    }],
                                    text: None,
                                    attrs: None,
                                    marks: vec![],
                                },
                            },
                        },
                        related: vec![],
                    },
                },
            }),
        },
    );
    let reply = drive(&mut shell, &queue, async {
        loop {
            if let Ok(reply) = receiver.try_recv() {
                break reply;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await;
    let ResourceResponse::Applied { record, .. } = reply.response else {
        panic!("note must persist");
    };
    let note_revision = record.revision.clone();
    let resource = ResourceId::parse(&record.id).unwrap();

    let created = environment(
        &mut shell,
        &queue,
        &handle,
        profile,
        WorkEnvironmentCall::Command {
            command: WorkCommandId::generate(),
            intent: WorkEnvironmentIntent::Create {
                space,
                title: "Keyboards".into(),
            },
        },
    )
    .await;
    let with_note = environment(
        &mut shell,
        &queue,
        &handle,
        profile,
        WorkEnvironmentCall::Command {
            command: WorkCommandId::generate(),
            intent: WorkEnvironmentIntent::Edit {
                id: created.id,
                expected: created.revision,
                edit: WorkEnvironmentEdit::Add {
                    reference: WorkEnvironmentReference::Resource { resource },
                    area: None,
                },
            },
        },
    )
    .await;
    let snapshot = environment(
        &mut shell,
        &queue,
        &handle,
        profile,
        WorkEnvironmentCall::Command {
            command: WorkCommandId::generate(),
            intent: WorkEnvironmentIntent::Edit {
                id: created.id,
                expected: with_note.revision,
                edit: WorkEnvironmentEdit::Add {
                    reference: WorkEnvironmentReference::Browser { tab },
                    area: None,
                },
            },
        },
    )
    .await;
    let element = |kind: &str| {
        snapshot
            .elements
            .iter()
            .find(|element| {
                matches!(
                    (&element.reference, kind),
                    (WorkEnvironmentReference::Resource { .. }, "note")
                        | (WorkEnvironmentReference::Browser { .. }, "tab")
                )
            })
            .unwrap()
            .id
    };
    let selection = WorkContextSelectionV1 {
        environment: snapshot.id,
        items: vec![
            WorkContextSelectionItem {
                element: element("note"),
                revision: note_revision.clone(),
            },
            WorkContextSelectionItem {
                element: element("tab"),
                revision: "https://docs.example/guide".into(),
            },
        ],
    };

    let admission = WorkContextAdmission::new(handle.clone());
    let admitted = drive(
        &mut shell,
        &queue,
        admission.admit(profile, WorkContextPurpose::Planning, &selection),
    )
    .await
    .unwrap();
    let manifest = &admitted.disclosure;
    assert_eq!(manifest.environment, snapshot.id);
    assert_eq!(manifest.environment_revision, snapshot.revision);
    assert_eq!(manifest.items.len(), 2);
    assert_eq!(manifest.items[0].kind, WorkContextItemKind::Note);
    assert_eq!(manifest.items[0].title, "Keyboard notes");
    assert_eq!(manifest.items[0].visibility, WorkContextVisibility::Private);
    assert_eq!(manifest.items[0].digest.len(), 64);
    assert_eq!(manifest.items[1].kind, WorkContextItemKind::Tab);
    assert_eq!(manifest.items[1].title, "Guide");
    assert!(admitted.bodies[0].text.contains("Budget 150 EUR"));
    assert!(admitted.bodies[1]
        .text
        .contains("https://docs.example/guide"));
    assert_eq!(
        manifest.total_bytes as usize,
        admitted.bodies.iter().map(|b| b.text.len()).sum::<usize>()
    );
    assert!(manifest.requires_review());

    let public = drive(
        &mut shell,
        &queue,
        admission.admit(profile, WorkContextPurpose::PublicRead, &selection),
    )
    .await;
    assert!(matches!(public, Err(WorkError::ReviewRequired)));
    let preview = drive(
        &mut shell,
        &queue,
        admission.preview(profile, WorkContextPurpose::PublicRead, &selection),
    )
    .await
    .unwrap();
    assert_eq!(preview.purpose, WorkContextPurpose::PublicRead);
    assert_eq!(preview.items.len(), 2);

    let mut stale = selection.clone();
    stale.items[0].revision = "999999".into();
    let refused = drive(
        &mut shell,
        &queue,
        admission.admit(profile, WorkContextPurpose::Planning, &stale),
    )
    .await;
    assert!(matches!(refused, Err(WorkError::Conflict)));

    let missing = WorkContextSelectionV1 {
        environment: snapshot.id,
        items: vec![WorkContextSelectionItem {
            element: WorkElementId::generate(),
            revision: "x".into(),
        }],
    };
    let not_found = drive(
        &mut shell,
        &queue,
        admission.admit(profile, WorkContextPurpose::Planning, &missing),
    )
    .await;
    assert!(matches!(not_found, Err(WorkError::NotFound)));
}
