//! Exhaustive package-lease error classification and public error mapping.

use super::api::{BundledPackageLeaseError, BundledPackageLeaseReleaseError};
use super::runtime::LocalLeaseError;
use crate::materialization::{PackageObjectError, SnapshotLoadError, SnapshotObjectPhase};
use crate::operation::RepositoryOperationError;
use crate::writer::{completed_error_requires_sealing, preflight_error_requires_sealing};
use crate::{BundledPackageMaterializationError, ExtensionRepositoryError};

pub(super) fn snapshot_error_requires_poison(error: &SnapshotLoadError) -> bool {
    match error {
        SnapshotLoadError::DurableMismatch
        | SnapshotLoadError::Preparation(_)
        | SnapshotLoadError::CatalogAdmission(_)
        | SnapshotLoadError::ManifestAdmission(_)
        | SnapshotLoadError::PackageNotMaterialized => true,
        SnapshotLoadError::Object { phase, error } => {
            snapshot_object_error_requires_poison(*phase, *error)
        }
        SnapshotLoadError::Repository(error) => matches!(
            error,
            ExtensionRepositoryError::StateCorrupt
                | ExtensionRepositoryError::RecoveryAmbiguous
                | ExtensionRepositoryError::SettlementAmbiguous
                | ExtensionRepositoryError::FileSystem(
                    zephium_private_fs::PrivateFsError::NotFound
                        | zephium_private_fs::PrivateFsError::ReservedComponent
                        | zephium_private_fs::PrivateFsError::Unsafe
                        | zephium_private_fs::PrivateFsError::BoundExceeded
                        | zephium_private_fs::PrivateFsError::AlreadyExists
                        | zephium_private_fs::PrivateFsError::NamespaceMismatch
                        | zephium_private_fs::PrivateFsError::DirectoryNotEmpty
                        | zephium_private_fs::PrivateFsError::IdentityAmbiguous
                        | zephium_private_fs::PrivateFsError::SettlementUnknown
                        | zephium_private_fs::PrivateFsError::Quarantined
                )
        ),
        SnapshotLoadError::CatalogAuthority(_)
        | SnapshotLoadError::ManifestAuthority(_)
        | SnapshotLoadError::StaleSelection
        | SnapshotLoadError::PackageNotSelected
        | SnapshotLoadError::WrongRole
        | SnapshotLoadError::EligibilityMismatch
        | SnapshotLoadError::InstallPackageMismatch
        | SnapshotLoadError::AccountingOverflow => false,
    }
}

pub(super) const fn snapshot_object_error_requires_poison(
    phase: SnapshotObjectPhase,
    error: PackageObjectError,
) -> bool {
    match phase {
        SnapshotObjectPhase::Preflight { had_intent } => {
            preflight_error_requires_sealing(error, had_intent)
        }
        SnapshotObjectPhase::Completed => completed_error_requires_sealing(error),
    }
}

pub(super) fn post_pin_error_requires_poison(error: &BundledPackageLeaseError) -> bool {
    !matches!(
        error,
        BundledPackageLeaseError::CatalogAuthority(_)
            | BundledPackageLeaseError::ManifestAuthority(_)
            | BundledPackageLeaseError::Repository(ExtensionRepositoryError::FileSystem(
                zephium_private_fs::PrivateFsError::LockUnavailable
                    | zephium_private_fs::PrivateFsError::InUse
                    | zephium_private_fs::PrivateFsError::PrimitiveUnavailable
                    | zephium_private_fs::PrivateFsError::Io
            ))
    )
}

impl From<ExtensionRepositoryError> for BundledPackageLeaseError {
    fn from(error: ExtensionRepositoryError) -> Self {
        Self::Repository(error)
    }
}

impl From<ExtensionRepositoryError> for BundledPackageLeaseReleaseError {
    fn from(error: ExtensionRepositoryError) -> Self {
        Self::Repository(error)
    }
}

pub(super) fn map_snapshot_error(error: SnapshotLoadError) -> BundledPackageLeaseError {
    match error {
        SnapshotLoadError::Repository(error) => BundledPackageLeaseError::Repository(error),
        SnapshotLoadError::CatalogAuthority(error) => {
            BundledPackageLeaseError::CatalogAuthority(error)
        }
        SnapshotLoadError::CatalogAdmission(error) => {
            BundledPackageLeaseError::CatalogAdmission(error)
        }
        SnapshotLoadError::ManifestAuthority(error) => {
            BundledPackageLeaseError::ManifestAuthority(error)
        }
        SnapshotLoadError::ManifestAdmission(error) => {
            BundledPackageLeaseError::ManifestAdmission(error)
        }
        SnapshotLoadError::StaleSelection => BundledPackageLeaseError::StaleSelection,
        SnapshotLoadError::PackageNotSelected => BundledPackageLeaseError::PackageNotSelected,
        SnapshotLoadError::WrongRole => BundledPackageLeaseError::WrongCatalogRole,
        SnapshotLoadError::EligibilityMismatch | SnapshotLoadError::InstallPackageMismatch => {
            BundledPackageLeaseError::EligibilityMismatch
        }
        SnapshotLoadError::PackageNotMaterialized => {
            BundledPackageLeaseError::PackageNotMaterialized
        }
        SnapshotLoadError::AccountingOverflow => BundledPackageLeaseError::CapacityExhausted,
        SnapshotLoadError::Object { error, .. } => map_snapshot_object_error(error),
        SnapshotLoadError::Preparation(_) | SnapshotLoadError::DurableMismatch => {
            BundledPackageLeaseError::DurableObjectMismatch
        }
    }
}

pub(super) fn map_snapshot_object_error(error: PackageObjectError) -> BundledPackageLeaseError {
    match error {
        PackageObjectError::BuildStateMismatch => {
            BundledPackageLeaseError::Repository(ExtensionRepositoryError::RecoveryAmbiguous)
        }
        PackageObjectError::CapacityExhausted => BundledPackageLeaseError::CapacityExhausted,
        PackageObjectError::GenerationExhausted => {
            BundledPackageLeaseError::Repository(ExtensionRepositoryError::GenerationExhausted)
        }
        PackageObjectError::Collision | PackageObjectError::ExactMismatch => {
            BundledPackageLeaseError::DurableObjectMismatch
        }
        PackageObjectError::Source(_) => BundledPackageLeaseError::DurableObjectMismatch,
        PackageObjectError::Filesystem(error) => {
            BundledPackageLeaseError::Repository(ExtensionRepositoryError::FileSystem(error))
        }
        PackageObjectError::SettlementAmbiguous => {
            BundledPackageLeaseError::Repository(ExtensionRepositoryError::SettlementAmbiguous)
        }
    }
}

pub(super) fn map_local_acquire(error: LocalLeaseError) -> BundledPackageLeaseError {
    match error {
        LocalLeaseError::AlreadyOpen => BundledPackageLeaseError::LeaseAlreadyOpen,
        LocalLeaseError::CapacityExhausted => BundledPackageLeaseError::CapacityExhausted,
        LocalLeaseError::SnapshotMismatch => BundledPackageLeaseError::DurableObjectMismatch,
        LocalLeaseError::ConcurrentLease => BundledPackageLeaseError::LeaseAlreadyOpen,
    }
}

pub(super) fn map_lease_operation_error(
    error: RepositoryOperationError,
) -> BundledPackageLeaseError {
    BundledPackageLeaseError::Repository(error.repository_error())
}

pub(super) fn map_release_operation_error(
    error: RepositoryOperationError,
) -> BundledPackageLeaseReleaseError {
    BundledPackageLeaseReleaseError::Repository(error.repository_error())
}

pub(super) fn map_transition_finish(
    error: BundledPackageMaterializationError,
) -> BundledPackageLeaseError {
    match error {
        BundledPackageMaterializationError::Repository(error) => {
            BundledPackageLeaseError::Repository(error)
        }
        _ => BundledPackageLeaseError::DurableObjectMismatch,
    }
}

pub(super) fn map_release_finish(
    error: BundledPackageMaterializationError,
) -> BundledPackageLeaseReleaseError {
    match error {
        BundledPackageMaterializationError::Repository(error) => {
            BundledPackageLeaseReleaseError::Repository(error)
        }
        _ => {
            BundledPackageLeaseReleaseError::Repository(ExtensionRepositoryError::RecoveryAmbiguous)
        }
    }
}
