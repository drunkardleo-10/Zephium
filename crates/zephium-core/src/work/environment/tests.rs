use super::*;

fn empty() -> WorkEnvironmentSnapshot {
    WorkEnvironmentSnapshot::create(1.into(), 2.into(), 3.into(), "Manual workspace".into())
        .unwrap()
}
fn edit(
    snapshot: &WorkEnvironmentSnapshot,
    edit: WorkEnvironmentEdit,
) -> Result<WorkEnvironmentSnapshot, WorkError> {
    snapshot.edit(edit, 10.into(), 20.into())
}
fn placement(element: WorkElementId) -> WorkElementPlacement {
    WorkElementPlacement {
        element,
        x: 0,
        y: 0,
        width: 300,
        height: 200,
    }
}

#[test]
fn manual_environment_has_no_objective_or_runtime_and_round_trips() {
    let initial = empty();
    assert!(initial.elements.is_empty());
    assert!(initial.areas.is_empty());
    assert_eq!(initial.revision, WorkRevision::INITIAL);
    assert_eq!(initial.view.revision, WorkRevision::INITIAL);
    initial.validate().unwrap();
    let bytes = serde_json::to_vec(&initial).unwrap();
    let restored: WorkEnvironmentSnapshot = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(initial, restored);
    let renamed = edit(
        &initial,
        WorkEnvironmentEdit::Rename {
            title: "Notes for later".into(),
        },
    )
    .unwrap();
    assert!(renamed.elements.is_empty());
    assert_eq!(renamed.revision, initial.revision.next().unwrap());
    assert_eq!(renamed.view.revision, initial.view.revision);
    let archived = edit(
        &renamed,
        WorkEnvironmentEdit::SetLifecycle {
            lifecycle: WorkLifecycle::Archived,
        },
    )
    .unwrap();
    assert!(matches!(
        edit(
            &archived,
            WorkEnvironmentEdit::Rename { title: "No".into() }
        ),
        Err(WorkError::Conflict)
    ));
    edit(
        &archived,
        WorkEnvironmentEdit::SetLifecycle {
            lifecycle: WorkLifecycle::Active,
        },
    )
    .unwrap()
    .validate()
    .unwrap();
}

#[test]
fn area_membership_is_exact_and_removal_preserves_elements() {
    let initial = empty();
    let reference = WorkEnvironmentReference::Browser { tab: 99.into() };
    assert!(edit(
        &initial,
        WorkEnvironmentEdit::Add {
            reference: reference.clone(),
            area: Some(20.into())
        }
    )
    .is_err());
    let area = edit(
        &initial,
        WorkEnvironmentEdit::CreateArea {
            title: "Sources".into(),
        },
    )
    .unwrap();
    let attached = edit(
        &area,
        WorkEnvironmentEdit::Add {
            reference: reference.clone(),
            area: Some(20.into()),
        },
    )
    .unwrap();
    assert!(matches!(
        attached.edit(
            WorkEnvironmentEdit::Add {
                reference,
                area: None
            },
            11.into(),
            21.into()
        ),
        Err(WorkError::Conflict)
    ));
    assert!(edit(
        &attached,
        WorkEnvironmentEdit::AssignArea {
            element: 10.into(),
            area: Some(404.into())
        }
    )
    .is_err());
    let removed = edit(
        &attached,
        WorkEnvironmentEdit::RemoveArea { area: 20.into() },
    )
    .unwrap();
    assert_eq!(removed.elements.len(), 1);
    assert!(removed.elements[0].area.is_none());
    assert!(removed.areas.is_empty());
    assert!(matches!(
        edit(
            &removed,
            WorkEnvironmentEdit::RemoveArea { area: 20.into() }
        ),
        Err(WorkError::NotFound)
    ));
}

#[test]
fn snapshot_validation_rejects_duplicate_references_and_dangling_placement() {
    let mut snapshot = edit(
        &empty(),
        WorkEnvironmentEdit::Add {
            reference: WorkEnvironmentReference::Browser { tab: 99.into() },
            area: None,
        },
    )
    .unwrap();
    snapshot.view.placements.push(placement(404.into()));
    assert!(snapshot.validate().is_err());
    snapshot.view.placements.clear();
    let mut duplicate = snapshot.elements[0].clone();
    duplicate.id = 11.into();
    snapshot.elements.push(duplicate);
    assert!(
        snapshot.validate().is_err(),
        "persisted snapshots must preserve Add's reference uniqueness invariant"
    );
}

#[test]
fn removal_invalidates_view_revision_even_without_a_placement() {
    let attached = edit(
        &empty(),
        WorkEnvironmentEdit::Add {
            reference: WorkEnvironmentReference::Browser { tab: 99.into() },
            area: None,
        },
    )
    .unwrap();
    for with_placement in [false, true] {
        let mut source = attached.clone();
        if with_placement {
            source.view.placements.push(placement(10.into()));
        }
        let removed = edit(&source, WorkEnvironmentEdit::Remove { element: 10.into() }).unwrap();
        assert!(removed.elements.is_empty());
        assert!(removed.view.placements.is_empty());
        assert_eq!(removed.view.revision, source.view.revision.next().unwrap());
        assert_eq!(removed.revision, source.revision.next().unwrap());
    }
}

#[test]
fn view_admits_boundary_geometry_and_rejects_overflow_duplicates_and_capacity() {
    let valid = WorkEnvironmentView {
        revision: WorkRevision::INITIAL,
        x: -1_000_000,
        y: 1_000_000,
        zoom_milli: 100,
        placements: vec![WorkElementPlacement {
            element: 10.into(),
            x: 1_000_000,
            y: -1_000_000,
            width: 120,
            height: 80,
        }],
    };
    valid.validate().unwrap();
    for mutate in [
        |v: &mut WorkEnvironmentView| v.x = -1_000_001,
        |v: &mut WorkEnvironmentView| v.y = 1_000_001,
        |v: &mut WorkEnvironmentView| v.zoom_milli = 99,
        |v: &mut WorkEnvironmentView| v.zoom_milli = 4001,
        |v: &mut WorkEnvironmentView| v.placements[0].x = i32::MAX,
        |v: &mut WorkEnvironmentView| v.placements[0].y = i32::MIN,
        |v: &mut WorkEnvironmentView| v.placements[0].width = 119,
        |v: &mut WorkEnvironmentView| v.placements[0].width = 4097,
        |v: &mut WorkEnvironmentView| v.placements[0].height = 79,
        |v: &mut WorkEnvironmentView| v.placements[0].height = 4097,
    ] {
        let mut invalid = valid.clone();
        mutate(&mut invalid);
        assert!(invalid.validate().is_err());
    }
    let mut duplicate = valid.clone();
    duplicate.placements.push(duplicate.placements[0].clone());
    assert!(duplicate.validate().is_err());
    let mut full = valid;
    full.zoom_milli = 4000;
    full.placements = (0..MAX_ENVIRONMENT_ELEMENTS)
        .map(|id| placement((id as u128).into()))
        .collect();
    full.validate().unwrap();
    full.placements.push(placement(9999.into()));
    assert!(full.validate().is_err());
}

#[test]
fn semantic_counts_titles_and_revision_overflow_are_bounded() {
    assert!(WorkEnvironmentSnapshot::create(
        1.into(),
        2.into(),
        3.into(),
        "x".repeat(MAX_ENVIRONMENT_TITLE_BYTES + 1)
    )
    .is_err());
    assert!(WorkEnvironmentSnapshot::create(1.into(), 2.into(), 3.into(), "\n".into()).is_err());
    let mut snapshot = empty();
    snapshot.areas = (0..MAX_ENVIRONMENT_AREAS)
        .map(|id| WorkArea {
            id: (id as u128).into(),
            title: "Area".into(),
        })
        .collect();
    snapshot.validate().unwrap();
    assert!(matches!(
        edit(
            &snapshot,
            WorkEnvironmentEdit::CreateArea {
                title: "One too many".into()
            }
        ),
        Err(WorkError::Capacity)
    ));
    snapshot.elements = (0..MAX_ENVIRONMENT_ELEMENTS)
        .map(|id| WorkEnvironmentElement {
            id: (id as u128).into(),
            reference: WorkEnvironmentReference::Browser {
                tab: (id as u128).into(),
            },
            area: None,
        })
        .collect();
    snapshot.validate().unwrap();
    assert!(matches!(
        edit(
            &snapshot,
            WorkEnvironmentEdit::Add {
                reference: WorkEnvironmentReference::Browser { tab: 9999.into() },
                area: None
            }
        ),
        Err(WorkError::Capacity)
    ));
    snapshot.revision = WorkRevision::new(i64::MAX as u64).unwrap();
    assert!(matches!(
        edit(
            &snapshot,
            WorkEnvironmentEdit::Rename {
                title: "Overflow".into()
            }
        ),
        Err(WorkError::Capacity)
    ));
}

#[test]
fn checkpoint_wire_identity_is_scoped_to_view_revision_not_global_command_id() {
    let view = WorkEnvironmentView::default();
    let call = WorkEnvironmentCall::Checkpoint {
        id: 1.into(),
        expected: view.revision,
        view: view.clone(),
    };
    assert!(call.validate().is_ok());
    let encoded = serde_json::to_value(&call).unwrap();
    assert_eq!(encoded["kind"], "checkpoint");
    assert!(encoded.get("command").is_none());
    let mut changed = encoded.clone();
    changed["expected"] = serde_json::json!("2");
    let changed: WorkEnvironmentCall = serde_json::from_value(changed).unwrap();
    assert_eq!(changed.validate(), Err(WorkError::Conflict));
    let mut old_intent = encoded;
    old_intent.as_object_mut().unwrap().remove("kind");
    old_intent["kind"] = serde_json::json!("checkpoint");
    assert!(serde_json::from_value::<WorkEnvironmentIntent>(old_intent).is_err());
}
