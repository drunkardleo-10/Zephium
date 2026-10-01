//! Windows execution of the same linear recovery contract exercised on Unix.
use super::*;
use crate::lease::{CommittedMutationFault, LifecycleFault, StreamingFault};
use std::io::Cursor;
use tempfile::TempDir;

fn test_namespace(name: &str) -> (TempDir, LockedPrivateNamespace) {
    let temp = tempfile::tempdir().unwrap();
    let namespace = LockedPrivateNamespace::open_or_create(temp.path().join(name)).unwrap();
    (temp, namespace)
}

fn assert_terminal_identity_ambiguity<T>(error: PrivateFsTransitionError<T>) {
    assert_eq!(error.error(), PrivateFsError::IdentityAmbiguous);
    assert!(!error.is_recoverable());
}

#[test]
fn tree_cleanup_faults_preserve_the_linear_settlement_contract() {
    for fault in 0..3 {
        let (_temp, namespace) = test_namespace("cleanup-fault");
        let name = PrivateComponent::new("tree").unwrap();
        let child = namespace.directory.create_new_private_child(&name).unwrap();
        let lease = &namespace.directory.lease;
        match fault {
            0 => lease.inject_tree_removal_preexecution_identity_fault(),
            1 => lease.inject_tree_removal_mutation_fault(1),
            2 => lease.inject_tree_removal_final_parent_sync_fault(),
            _ => unreachable!(),
        }
        let error = OpenedPrivateDirectory::Writable(child)
            .remove_tree_bounded(TreeRemovalLimits::new(0, 0).unwrap())
            .unwrap_err();
        assert!(!error.is_recoverable());
        assert_eq!(
            error.error(),
            if fault == 0 {
                PrivateFsError::IdentityAmbiguous
            } else {
                PrivateFsError::SettlementUnknown
            }
        );
        assert_eq!(namespace.directory.sync(), Err(PrivateFsError::Quarantined));
    }
}

#[test]
fn same_parent_publication_flushes_once() {
    let (_temp, namespace) = test_namespace("publish-flush");
    let child = namespace
        .directory
        .create_new_private_child(&PrivateComponent::new("source").unwrap())
        .unwrap();
    let sealed = child.seal().unwrap();
    let before = namespace.directory.lease.same_parent_publish_sync_count();
    let published = sealed
        .publish_noreplace(
            &namespace.directory,
            &PrivateComponent::new("ready").unwrap(),
        )
        .unwrap();
    assert_eq!(
        namespace.directory.lease.same_parent_publish_sync_count(),
        before + 1
    );
    published.unseal().unwrap().remove_empty().unwrap();
}

#[test]
fn committed_unlink_reported_as_error_has_unknown_sticky_settlement() {
    let (parent, namespace) = test_namespace("unlink-fault-test");
    let file = PrivateComponent::new("value.bin").unwrap();
    namespace
        .directory
        .write_new_synced(&file, b"value", ByteLimit::new(16).unwrap())
        .unwrap();
    namespace
        .directory
        .lease
        .inject_committed_mutation_fault(CommittedMutationFault::Remove);

    assert_eq!(
        namespace.directory.remove_verified_regular(&file),
        Err(PrivateFsError::SettlementUnknown)
    );
    assert!(!parent.path().join("unlink-fault-test/value.bin").exists());
    assert_eq!(
        namespace.directory.list_components(8),
        Err(PrivateFsError::Quarantined)
    );
}

#[test]
fn reported_stream_write_and_sync_failures_cleanly_remove_the_created_entry() {
    let (parent, namespace) = test_namespace("stream-io-cleanup-test");

    for (index, fault) in [
        StreamingFault::Write,
        StreamingFault::FileSync,
        StreamingFault::DirectorySync,
    ]
    .into_iter()
    .enumerate()
    {
        let name = PrivateEntryName::new(format!("Payload {index}.bin")).unwrap();
        namespace.directory.lease.inject_streaming_fault(fault);
        let mut source = Cursor::new(b"value");
        assert_eq!(
            namespace.directory.write_new_entry_from_reader(
                &name,
                &mut source,
                StreamingFileLength::new(5).unwrap(),
            ),
            Err(StreamingWriteError::Filesystem(PrivateFsError::Io))
        );
        assert!(!parent
            .path()
            .join("stream-io-cleanup-test")
            .join(name.as_str())
            .exists());
        assert!(namespace.directory.list_entry_names(8).unwrap().is_empty());

        let mut retry = Cursor::new(b"value");
        namespace
            .directory
            .write_new_entry_from_reader(&name, &mut retry, StreamingFileLength::new(5).unwrap())
            .unwrap();
        assert!(namespace
            .directory
            .remove_verified_entry_regular(&name)
            .unwrap());
    }
}

#[test]
fn empty_removal_race_returns_directory_not_empty_with_capability() {
    let (parent, namespace) = test_namespace("remove-empty-race-test");
    let name = PrivateComponent::new("child").unwrap();
    let child = namespace.directory.create_new_private_child(&name).unwrap();
    child.lease.inject_remove_empty_race();

    let error = child.remove_empty().err().unwrap();
    let (kind, child) = error.into_parts();
    assert_eq!(kind, PrivateFsError::DirectoryNotEmpty);
    let child = child.expect("rmdir race did not commit removal");
    assert_eq!(
        child.list_components(4).unwrap(),
        vec![PrivateComponent::new("remove-race-entry").unwrap()]
    );
    assert!(parent
        .path()
        .join("remove-empty-race-test/child/remove-race-entry")
        .is_file());
}

#[test]
fn remove_child_identity_observation_stickily_quarantines() {
    let (_parent, namespace) = test_namespace("remove-open-identity-test");
    let name = PrivateComponent::new("child").unwrap();
    let child = namespace.directory.create_new_private_child(&name).unwrap();
    namespace
        .directory
        .lease
        .inject_lifecycle_fault(LifecycleFault::RemoveChildOpenIdentity);

    assert_eq!(
        namespace.directory.remove_empty_private_child(&name),
        Err(PrivateFsError::IdentityAmbiguous)
    );
    assert_eq!(
        namespace.directory.list_components(4),
        Err(PrivateFsError::Quarantined)
    );
    drop(child);
}

#[test]
fn sealed_frontier_identity_ambiguity_never_returns_writable_authority() {
    let (_parent, namespace) = test_namespace("seal-frontier-identity-test");
    let child = namespace
        .directory
        .create_new_private_child(&PrivateComponent::new("child").unwrap())
        .unwrap();
    let payload = PrivateComponent::new("payload.bin").unwrap();
    child
        .write_new_synced(&payload, b"payload", ByteLimit::new(16).unwrap())
        .unwrap();
    child.seal_verified_regular(&payload).unwrap().unwrap();
    child.lease.inject_seal_child_identity_fault();

    let error = child.seal().err().unwrap();
    assert_terminal_identity_ambiguity(error);
    assert_eq!(
        namespace.directory.list_components(4),
        Err(PrivateFsError::Quarantined)
    );
}
