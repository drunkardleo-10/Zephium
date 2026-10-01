//! SQLite fault fixtures exercise persisted facts, not successful execution.
//! The application suite separately supplies real controller terminal owners.
use super::tests::{initial, open, simulate_process_exit, work_test_guard, Fault, FAULT};
use super::*;

const EMPTY_ARCHIVE: &[u8] = br#"{"version":1,"id":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],"profile":"00000000000000000000000001","key":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1],"schema":1,"observation":1,"generation":1,"captured_millis":1,"fields":[],"sources":[]}"#;
const EMPTY_DIGEST: [u8; 32] = [
    0x45, 0xc4, 0xfa, 0x1f, 0x7d, 0xec, 0xe9, 0xb3, 0x4e, 0xdd, 0xbf, 0x73, 0x6a, 0x08, 0xf0, 0xab,
    0x1c, 0x2f, 0xf2, 0x7a, 0x4e, 0x19, 0xfc, 0x17, 0x95, 0x75, 0xaa, 0x3c, 0x77, 0x66, 0x77, 0x90,
];

#[test]
fn historical_evidence_requires_exact_profile_source_and_verified_archive_bytes() {
    use sha2::{Digest, Sha256};
    use zephium_core::work::{artifact::WorkEvidenceLink, WorkError};
    let _process = work_test_guard();
    let (_directory, mut hub, owner) = open();
    let (running, succeeded, original) = fixture(&mut hub, owner);
    let source = r#""sources":[{"id":1,"origin":"https://fixture.invalid/","role":"paragraph","field":1,"context":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],"context_generation":1,"navigation_epoch":1,"frame":1,"frame_generation":1,"invocation":1,"snapshot":1,"reference":1,"browser_derived":true,"content":{"kind":"text","value":"Historical source quotation"}}]"#;
    let body = std::str::from_utf8(EMPTY_ARCHIVE)
        .unwrap()
        .replace("\"sources\":[]", source);
    let descriptor = AgentWorkArtifactDescriptor::decode(
        original.descriptor.id(),
        1_u128.into(),
        running.key(),
        Sha256::digest(body.as_bytes()).into(),
        body.len() as u32,
    )
    .unwrap();
    AgentWorkArchivedExtraction::decode(descriptor, body.as_bytes()).unwrap();
    compare_and_set_records(
        &mut hub.meta,
        Some(running),
        succeeded,
        None,
        Some(ArtifactStorageValue {
            descriptor,
            body: body.as_bytes(),
        }),
    )
    .unwrap();
    let link = WorkEvidenceLink {
        extraction_id: u128::from_be_bytes(descriptor.id()).into(),
        source_id: 1,
    };
    let preview = read_work_evidence(&hub.meta, 1_u128.into(), link.clone()).unwrap();
    assert_eq!(preview.text, "Historical source quotation");
    assert!(!preview.truncated);
    assert_eq!(preview.source_bytes, preview.text.len().to_string());
    assert!(!format!("{preview:?}").contains("quotation"));
    assert!(matches!(
        read_work_evidence(&hub.meta, 2_u128.into(), link.clone()),
        Err(WorkError::NotFound)
    ));
    let mut missing = link.clone();
    missing.source_id = 2;
    assert!(matches!(
        read_work_evidence(&hub.meta, 1_u128.into(), missing),
        Err(WorkError::NotFound)
    ));
    // Corrupt persisted bytes are not promoted to source content.
    hub.meta
        .execute_batch("DROP TRIGGER agent_work_artifact_immutable")
        .unwrap();
    let tampered = body.replace("Historical", "Tampered!!");
    hub.meta
        .execute(
            "UPDATE agent_work_artifacts SET body = ?1",
            [tampered.as_bytes()],
        )
        .unwrap();
    assert!(matches!(
        read_work_evidence(&hub.meta, 1_u128.into(), link),
        Err(WorkError::Invalid)
    ));
}

#[test]
fn artifact_migration_limits_and_profile_erasure_match_the_closed_contract() {
    let source = include_str!("../migrations.rs");
    assert!(source.contains(&format!(
        "length(body) BETWEEN 1 AND {}",
        MAX_AGENT_WORK_ARTIFACT_BYTES
    )));
    assert!(source.contains(&format!(
        "length(NEW.body) > {}",
        MAX_AGENT_WORK_ARTIFACT_TOTAL_BYTES
    )));
    assert!(source.contains("profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE"));
    assert!(source.contains("WHEN NEW.result_profile IS NOT OLD.result_profile"));
}

fn register(hub: &mut Hub) {
    use zephium_core::{
        profiles::ProfileKind,
        session::{PersistedProfile, PersistedSpace, SessionState},
    };
    hub.save(&SessionState {
        profiles: vec![PersistedProfile {
            id: 1_u128.into(),
            name: "Fixture".into(),
            kind: ProfileKind::Default,
        }],
        spaces: vec![PersistedSpace {
            id: 2_u128.into(),
            profile: 1_u128.into(),
            name: "Fixture".into(),
        }],
        items: vec![],
        active_space: Some(2_u128.into()),
        active_item: None,
        splits: None,
        recently_closed: vec![],
    })
    .unwrap();
}

fn fixture(
    hub: &mut Hub,
    owner: AgentWorkIncarnation,
) -> (
    AgentWorkRecord,
    AgentWorkRecord,
    ArtifactStorageValue<'static>,
) {
    register(hub);
    let admitted = initial(owner, 1);
    compare_and_set_records(&mut hub.meta, None, admitted, Some(1_u128.into()), None).unwrap();
    let running = admitted.transition(AgentWorkDisposition::Running).unwrap();
    compare_and_set_records(&mut hub.meta, Some(admitted), running, None, None).unwrap();
    // Raw persisted success is fixture data, never a core publication owner.
    let mut bytes = *running.as_bytes();
    bytes[1] = 6;
    bytes[2] = 0;
    bytes[15] = 3;
    let succeeded = AgentWorkRecord::decode(bytes).unwrap();
    let descriptor = AgentWorkArtifactDescriptor::decode(
        [1; 16],
        1_u128.into(),
        running.key(),
        EMPTY_DIGEST,
        EMPTY_ARCHIVE.len() as u32,
    )
    .unwrap();
    AgentWorkArchivedExtraction::decode(descriptor, EMPTY_ARCHIVE).unwrap();
    (
        running,
        succeeded,
        ArtifactStorageValue {
            descriptor,
            body: EMPTY_ARCHIVE,
        },
    )
}

#[test]
fn partial_result_or_terminal_writes_rollback_together_and_commit_loss_reconciles_once() {
    for fault in [
        Fault::BeforeWrite,
        Fault::AfterArtifactWrite,
        Fault::AfterWrite,
        Fault::AfterCommit,
    ] {
        let _process = work_test_guard();
        let (_directory, mut hub, owner) = open();
        let (running, succeeded, artifact) = fixture(&mut hub, owner);
        FAULT.with(|slot| slot.set(Some(fault)));
        assert_eq!(
            compare_and_set_records(
                &mut hub.meta,
                Some(running),
                succeeded,
                None,
                Some(artifact)
            ),
            Err(Error::Uncertain)
        );
        let committed = fault == Fault::AfterCommit;
        assert_eq!(
            read(&hub.meta, running.key()).unwrap(),
            Some(if committed { succeeded } else { running })
        );
        assert_eq!(
            read_artifact(&hub.meta, running.key(), 1_u128.into())
                .unwrap()
                .is_some(),
            committed
        );
        compare_and_set_records(
            &mut hub.meta,
            Some(running),
            succeeded,
            None,
            Some(artifact),
        )
        .unwrap();
        compare_and_set_records(
            &mut hub.meta,
            Some(running),
            succeeded,
            None,
            Some(artifact),
        )
        .unwrap();
        assert_eq!(
            hub.meta
                .query_row("SELECT count(*) FROM agent_work_artifacts", [], |row| row
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
            1
        );
        assert_eq!(read(&hub.meta, running.key()).unwrap(), Some(succeeded));
        drop(hub);
    }
}

#[test]
fn promised_result_cannot_be_downgraded_or_replaced_by_conflicting_publication() {
    let _process = work_test_guard();
    let (_directory, mut hub, owner) = open();
    let (running, succeeded, artifact) = fixture(&mut hub, owner);
    assert_eq!(
        compare_and_set_records(&mut hub.meta, Some(running), succeeded, None, None),
        Err(Error::Transition)
    );
    compare_and_set_records(
        &mut hub.meta,
        Some(running),
        succeeded,
        None,
        Some(artifact),
    )
    .unwrap();
    let changed = ArtifactStorageValue {
        descriptor: AgentWorkArtifactDescriptor::decode(
            [2; 16],
            1_u128.into(),
            running.key(),
            EMPTY_DIGEST,
            EMPTY_ARCHIVE.len() as u32,
        )
        .unwrap(),
        ..artifact
    };
    assert_eq!(
        compare_and_set_records(&mut hub.meta, Some(running), succeeded, None, Some(changed)),
        Err(Error::Conflict)
    );
    assert_eq!(
        compare_and_set_records(&mut hub.meta, Some(running), succeeded, None, None),
        Err(Error::Transition)
    );
    assert!(hub
        .meta
        .execute(
            "UPDATE agent_work_artifacts SET body = body WHERE run_key = ?1",
            [running.key().as_slice()]
        )
        .is_err());
    assert!(hub
        .meta
        .execute(
            "UPDATE agent_work_runs SET result_profile = NULL WHERE run_key = ?1",
            [running.key().as_slice()]
        )
        .is_err());
    drop(hub);
}

#[test]
fn restart_reads_archived_data_with_new_fence_but_never_reopens_the_terminal() {
    let _process = work_test_guard();
    let (directory, mut hub, owner) = open();
    let (running, succeeded, artifact) = fixture(&mut hub, owner);
    compare_and_set_records(
        &mut hub.meta,
        Some(running),
        succeeded,
        None,
        Some(artifact),
    )
    .unwrap();
    drop(hub);
    simulate_process_exit();
    let mut hub = Hub::open(directory.path().into()).unwrap();
    let Reply::Claimed {
        owner: fresh,
        records,
    } = hub.agent_work(Request::Claim).unwrap()
    else {
        panic!()
    };
    assert_ne!(fresh, owner);
    assert_eq!(records, [succeeded]);
    assert!(matches!(
        hub.agent_work_artifact(AgentWorkArtifactRequest::Read {
            owner,
            record: succeeded,
            profile: 1_u128.into()
        }),
        Err(Error::Fenced)
    ));
    assert!(matches!(
        hub.agent_work_artifact(AgentWorkArtifactRequest::Read {
            owner: fresh,
            record: running,
            profile: 1_u128.into()
        }),
        Err(Error::Conflict)
    ));
    assert!(matches!(
        hub.agent_work_artifact(AgentWorkArtifactRequest::Read {
            owner: fresh,
            record: succeeded,
            profile: 99_u128.into()
        }),
        Err(Error::Fenced)
    ));
    let AgentWorkArtifactReply::Read(Some(result)) = hub
        .agent_work_artifact(AgentWorkArtifactRequest::Read {
            owner: fresh,
            record: succeeded,
            profile: 1_u128.into(),
        })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(result.descriptor(), artifact.descriptor);
    assert_eq!(
        result.trust(),
        zephium_agentic::SemanticExtractionTrust::ModelMapped
    );
    assert_eq!(read(&hub.meta, running.key()).unwrap(), Some(succeeded));
    drop(hub);
}

#[test]
fn profile_erasure_removes_private_body_without_rewriting_terminal_or_restoring_intent() {
    let _process = work_test_guard();
    let (_directory, mut hub, owner) = open();
    let (running, succeeded, artifact) = fixture(&mut hub, owner);
    compare_and_set_records(
        &mut hub.meta,
        Some(running),
        succeeded,
        None,
        Some(artifact),
    )
    .unwrap();
    // Exercise the same FK used by the existing authorized profile deletion;
    // this raw fixture deletion is not an application/native erasure proof.
    hub.meta
        .execute(
            "DELETE FROM profiles WHERE id = ?1",
            [ProfileId::from(1).to_string()],
        )
        .unwrap();
    hub.registry.remove(&1_u128.into());
    assert!(read_artifact(&hub.meta, running.key(), 1_u128.into())
        .unwrap()
        .is_none());
    assert_eq!(read(&hub.meta, running.key()).unwrap(), Some(succeeded));
    assert_eq!(
        result_profile(&hub.meta, running.key()).unwrap(),
        Some(1_u128.into())
    );
    assert!(matches!(
        hub.agent_work_artifact(AgentWorkArtifactRequest::Read {
            owner,
            record: succeeded,
            profile: 1_u128.into()
        }),
        Err(Error::Fenced)
    ));
    drop(hub);
}

#[test]
fn result_retention_pressure_never_evicts_and_corrupt_bodies_fail_closed() {
    let _process = work_test_guard();
    let (_directory, mut hub, owner) = open();
    let (running, succeeded, artifact) = fixture(&mut hub, owner);
    for key in 2..=129_u16 {
        let record = initial(owner, key);
        compare_and_set_records(&mut hub.meta, None, record, None, None).unwrap();
        hub.meta.execute("INSERT INTO agent_work_artifacts(run_key, profile_id, artifact_id, digest, body) VALUES (?1, ?2, ?3, ?4, zeroblob(?5))",
            params![record.key().as_slice(), ProfileId::from(1).to_string(), (key as u128).to_be_bytes().as_slice(), [0_u8;32].as_slice(), MAX_AGENT_WORK_ARTIFACT_BYTES]).unwrap();
    }
    assert_eq!(
        compare_and_set_records(
            &mut hub.meta,
            Some(running),
            succeeded,
            None,
            Some(artifact)
        ),
        Err(Error::Capacity)
    );
    assert_eq!(read(&hub.meta, running.key()).unwrap(), Some(running));
    assert!(matches!(
        read_artifact(&hub.meta, initial(owner, 2).key(), 1_u128.into()),
        Err(Error::Uncertain)
    ));
    assert_eq!(
        hub.meta
            .query_row("SELECT count(*) FROM agent_work_artifacts", [], |row| row
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        128
    );
    drop(hub);
}
