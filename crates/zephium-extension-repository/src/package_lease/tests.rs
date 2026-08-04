use zephium_core::ids::{ExtensionInstallId, ProfileId};

use super::api::{
    ActiveBundledPackageLease, ActiveBundledPackageReleaseRequest, BundledPackageLeaseError,
    BundledPackageLeaseReleaseError, RollbackBundledPackageLease,
    RollbackBundledPackageReleaseRequest,
};
use super::policy::{map_snapshot_object_error, snapshot_object_error_requires_poison};
use super::runtime::{LocalLeaseError, PackageLeaseRuntime};
use crate::materialization::{
    MaterializationTransitionError, PackageLeaseRepositoryIdentity, PackageObjectError,
    SnapshotObjectPhase, MAX_DURABLE_PACKAGE_PINS,
};
use crate::{ExtensionRepository, ExtensionRepositoryError};

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn empty_repository() -> (tempfile::TempDir, ExtensionRepository) {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use zephium_private_fs::LockedPrivateNamespace;

    #[cfg(target_os = "macos")]
    let temporary = tempfile::tempdir_in("/private/tmp").unwrap();
    #[cfg(target_os = "linux")]
    let temporary = tempfile::tempdir_in("/tmp").unwrap();
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let namespace =
        LockedPrivateNamespace::open_or_create(temporary.path().join("repository")).unwrap();
    let repository = ExtensionRepository::open(namespace).unwrap();
    (temporary, repository)
}

#[test]
fn public_lease_values_are_nominal_and_path_free_in_debug() {
    assert_ne!(
        std::any::TypeId::of::<ActiveBundledPackageLease>(),
        std::any::TypeId::of::<RollbackBundledPackageLease>()
    );
    assert_ne!(
        std::any::TypeId::of::<ActiveBundledPackageReleaseRequest>(),
        std::any::TypeId::of::<RollbackBundledPackageReleaseRequest>()
    );
}

#[test]
fn repository_health_is_shared_and_sticky() {
    let runtime = crate::operation::RepositoryRuntime::new();
    let shared = runtime.clone();
    assert!(shared.is_healthy());
    runtime.poison();
    assert!(!shared.is_healthy());
    assert!(!runtime.is_healthy());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn must_seal_planning_errors_seal_both_public_boundaries() {
    let (_temporary, mut repository) = empty_repository();
    assert!(matches!(
        repository.finish_lease_planning_error(MaterializationTransitionError::MustSeal(
            ExtensionRepositoryError::InjectedCrash,
        )),
        BundledPackageLeaseError::Repository(ExtensionRepositoryError::InjectedCrash)
    ));
    assert!(repository.writer_is_sealed());

    let (_temporary, mut repository) = empty_repository();
    assert_eq!(
        repository.finish_release_planning_error(MaterializationTransitionError::MustSeal(
            ExtensionRepositoryError::InjectedCrash,
        )),
        BundledPackageLeaseReleaseError::Repository(ExtensionRepositoryError::InjectedCrash)
    );
    assert!(repository.writer_is_sealed());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn clean_planning_errors_recover_before_both_public_boundaries_return() {
    let (_temporary, mut repository) = empty_repository();
    assert!(matches!(
        repository.finish_lease_planning_error(MaterializationTransitionError::Clean(
            ExtensionRepositoryError::FileSystem(zephium_private_fs::PrivateFsError::Io),
        )),
        BundledPackageLeaseError::Repository(ExtensionRepositoryError::FileSystem(
            zephium_private_fs::PrivateFsError::Io
        ))
    ));
    assert!(!repository.writer_is_sealed());
    assert!(repository.writer_materialization().is_ok());

    let (_temporary, mut repository) = empty_repository();
    assert_eq!(
        repository.finish_release_planning_error(MaterializationTransitionError::Clean(
            ExtensionRepositoryError::FileSystem(zephium_private_fs::PrivateFsError::Io),
        )),
        BundledPackageLeaseReleaseError::Repository(ExtensionRepositoryError::FileSystem(
            zephium_private_fs::PrivateFsError::Io
        ))
    );
    assert!(!repository.writer_is_sealed());
    assert!(repository.writer_materialization().is_ok());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn failed_clean_planning_recovery_seals_both_public_boundaries() {
    let (temporary, mut repository) = empty_repository();
    std::fs::remove_file(
        temporary
            .path()
            .join("repository/materialization/state.json"),
    )
    .unwrap();
    let acquisition =
        repository.finish_lease_planning_error(MaterializationTransitionError::Clean(
            ExtensionRepositoryError::FileSystem(zephium_private_fs::PrivateFsError::Io),
        ));
    assert!(!matches!(
        acquisition,
        BundledPackageLeaseError::Repository(ExtensionRepositoryError::FileSystem(
            zephium_private_fs::PrivateFsError::Io
        ))
    ));
    assert!(repository.writer_is_sealed());

    let (temporary, mut repository) = empty_repository();
    std::fs::remove_file(
        temporary
            .path()
            .join("repository/materialization/state.json"),
    )
    .unwrap();
    let release = repository.finish_release_planning_error(MaterializationTransitionError::Clean(
        ExtensionRepositoryError::FileSystem(zephium_private_fs::PrivateFsError::Io),
    ));
    assert_ne!(
        release,
        BundledPackageLeaseReleaseError::Repository(ExtensionRepositoryError::FileSystem(
            zephium_private_fs::PrivateFsError::Io
        ))
    );
    assert!(repository.writer_is_sealed());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn post_pin_drift_seals_but_transient_io_remains_retryable() {
    let (_temporary, mut repository) = empty_repository();
    assert!(matches!(
        repository.finish_post_pin_error(BundledPackageLeaseError::EligibilityMismatch),
        BundledPackageLeaseError::EligibilityMismatch
    ));
    assert!(repository.writer_is_sealed());

    let (_temporary, mut repository) = empty_repository();
    assert!(matches!(
        repository.finish_post_pin_error(BundledPackageLeaseError::Repository(
            ExtensionRepositoryError::FileSystem(zephium_private_fs::PrivateFsError::Io),
        )),
        BundledPackageLeaseError::Repository(ExtensionRepositoryError::FileSystem(
            zephium_private_fs::PrivateFsError::Io
        ))
    ));
    assert!(!repository.writer_is_sealed());
}

#[test]
fn snapshot_object_error_policy_is_exhaustive_by_phase() {
    #[derive(Clone, Copy)]
    enum ExpectedMap {
        Recovery,
        Capacity,
        Generation,
        Durable,
        FileSystem(zephium_private_fs::PrivateFsError),
        Settlement,
    }

    let cases = [
        (
            PackageObjectError::BuildStateMismatch,
            true,
            true,
            true,
            ExpectedMap::Recovery,
        ),
        (
            PackageObjectError::CapacityExhausted,
            false,
            false,
            true,
            ExpectedMap::Capacity,
        ),
        (
            PackageObjectError::GenerationExhausted,
            false,
            true,
            true,
            ExpectedMap::Generation,
        ),
        (
            PackageObjectError::Collision,
            true,
            true,
            true,
            ExpectedMap::Durable,
        ),
        (
            PackageObjectError::ExactMismatch,
            true,
            true,
            true,
            ExpectedMap::Durable,
        ),
        (
            PackageObjectError::Source(crate::materialization::BundledReleaseSourceError::Io),
            false,
            false,
            false,
            ExpectedMap::Durable,
        ),
        (
            PackageObjectError::Filesystem(zephium_private_fs::PrivateFsError::Io),
            false,
            false,
            false,
            ExpectedMap::FileSystem(zephium_private_fs::PrivateFsError::Io),
        ),
        (
            PackageObjectError::Filesystem(zephium_private_fs::PrivateFsError::NotFound),
            true,
            true,
            true,
            ExpectedMap::FileSystem(zephium_private_fs::PrivateFsError::NotFound),
        ),
        (
            PackageObjectError::Filesystem(zephium_private_fs::PrivateFsError::Quarantined),
            true,
            true,
            true,
            ExpectedMap::FileSystem(zephium_private_fs::PrivateFsError::Quarantined),
        ),
        (
            PackageObjectError::SettlementAmbiguous,
            true,
            true,
            true,
            ExpectedMap::Settlement,
        ),
    ];

    for (error, preflight, preflight_with_intent, completed, expected) in cases {
        assert_eq!(
            snapshot_object_error_requires_poison(
                SnapshotObjectPhase::Preflight { had_intent: false },
                error,
            ),
            preflight
        );
        assert_eq!(
            snapshot_object_error_requires_poison(
                SnapshotObjectPhase::Preflight { had_intent: true },
                error,
            ),
            preflight_with_intent
        );
        assert_eq!(
            snapshot_object_error_requires_poison(SnapshotObjectPhase::Completed, error),
            completed
        );
        let mapped = map_snapshot_object_error(error);
        match expected {
            ExpectedMap::Recovery => assert!(matches!(
                mapped,
                BundledPackageLeaseError::Repository(ExtensionRepositoryError::RecoveryAmbiguous)
            )),
            ExpectedMap::Capacity => {
                assert!(matches!(
                    mapped,
                    BundledPackageLeaseError::CapacityExhausted
                ))
            }
            ExpectedMap::Generation => assert!(matches!(
                mapped,
                BundledPackageLeaseError::Repository(ExtensionRepositoryError::GenerationExhausted)
            )),
            ExpectedMap::Durable => assert!(matches!(
                mapped,
                BundledPackageLeaseError::DurableObjectMismatch
            )),
            ExpectedMap::FileSystem(expected) => assert!(matches!(
                mapped,
                BundledPackageLeaseError::Repository(ExtensionRepositoryError::FileSystem(
                    observed
                )) if observed == expected
            )),
            ExpectedMap::Settlement => assert!(matches!(
                mapped,
                BundledPackageLeaseError::Repository(ExtensionRepositoryError::SettlementAmbiguous)
            )),
        }
    }

    let filesystem_cases = [
        (zephium_private_fs::PrivateFsError::NotFound, true),
        (zephium_private_fs::PrivateFsError::ReservedComponent, true),
        (zephium_private_fs::PrivateFsError::Unsafe, true),
        (zephium_private_fs::PrivateFsError::LockUnavailable, false),
        (zephium_private_fs::PrivateFsError::BoundExceeded, true),
        (zephium_private_fs::PrivateFsError::AlreadyExists, true),
        (zephium_private_fs::PrivateFsError::NamespaceMismatch, true),
        (zephium_private_fs::PrivateFsError::DirectoryNotEmpty, true),
        (zephium_private_fs::PrivateFsError::InUse, false),
        (zephium_private_fs::PrivateFsError::IdentityAmbiguous, true),
        (zephium_private_fs::PrivateFsError::SettlementUnknown, true),
        (zephium_private_fs::PrivateFsError::Quarantined, true),
        (
            zephium_private_fs::PrivateFsError::PrimitiveUnavailable,
            false,
        ),
        (zephium_private_fs::PrivateFsError::Io, false),
    ];
    for (filesystem, poison) in filesystem_cases {
        let error = PackageObjectError::Filesystem(filesystem);
        assert_eq!(
            snapshot_object_error_requires_poison(
                SnapshotObjectPhase::Preflight { had_intent: false },
                error,
            ),
            poison
        );
        assert_eq!(
            snapshot_object_error_requires_poison(
                SnapshotObjectPhase::Preflight { had_intent: true },
                error,
            ),
            poison
        );
        assert_eq!(
            snapshot_object_error_requires_poison(SnapshotObjectPhase::Completed, error),
            poison
        );
        assert!(matches!(
            map_snapshot_object_error(error),
            BundledPackageLeaseError::Repository(ExtensionRepositoryError::FileSystem(
                observed
            )) if observed == filesystem
        ));
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn repository_identity() -> (
    tempfile::TempDir,
    zephium_private_fs::LockedPrivateNamespace,
    PackageLeaseRepositoryIdentity,
) {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use zephium_private_fs::{LockedPrivateNamespace, PrivateComponent};

    #[cfg(target_os = "macos")]
    let temporary = tempfile::tempdir_in("/private/tmp").unwrap();
    #[cfg(target_os = "linux")]
    let temporary = tempfile::tempdir_in("/tmp").unwrap();
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let namespace =
        LockedPrivateNamespace::open_or_create(temporary.path().join("repository")).unwrap();
    let records = namespace
        .directory()
        .create_new_private_child(&PrivateComponent::new("records").unwrap())
        .unwrap();
    let trees = namespace
        .directory()
        .create_new_private_child(&PrivateComponent::new("trees").unwrap())
        .unwrap();
    let identity = PackageLeaseRepositoryIdentity {
        root: namespace.directory().identity(),
        records: records.identity(),
        trees: trees.identity(),
    };
    (temporary, namespace, identity)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn live_presence_registry_prunes_dead_owners() {
    let (_temporary, _namespace, repository) = repository_identity();
    let mut runtime = PackageLeaseRuntime::new();
    let expired = runtime
        .reserve(repository, ProfileId::from(1), ExtensionInstallId::from(1))
        .unwrap();
    assert_eq!(runtime.live.len(), 1);
    drop(expired);
    let live = runtime
        .reserve(repository, ProfileId::from(2), ExtensionInstallId::from(2))
        .unwrap();
    assert_eq!(runtime.live.len(), 1);
    assert_eq!(live.profile, ProfileId::from(2));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn live_presence_registry_enforces_the_durable_pin_ceiling() {
    let (_temporary, _namespace, repository) = repository_identity();
    let mut runtime = PackageLeaseRuntime::new();
    let mut retained = Vec::with_capacity(MAX_DURABLE_PACKAGE_PINS);
    for owner in 0..MAX_DURABLE_PACKAGE_PINS {
        retained.push(
            runtime
                .reserve(
                    repository,
                    ProfileId::from(owner as u128 + 1),
                    ExtensionInstallId::from(owner as u128 + 1),
                )
                .unwrap(),
        );
    }
    assert!(matches!(
        runtime.reserve(
            repository,
            ProfileId::from(u128::MAX),
            ExtensionInstallId::from(u128::MAX),
        ),
        Err(LocalLeaseError::CapacityExhausted)
    ));
    assert_eq!(retained.len(), MAX_DURABLE_PACKAGE_PINS);
}
