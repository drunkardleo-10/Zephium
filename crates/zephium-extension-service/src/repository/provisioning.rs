//! Service-owned acquired-package materialization and catalog activation.

use std::io::{Cursor, Read};

use zephium_core::extensions::{ExtensionCatalogSetDigest, ExtensionPackageKey};
use zephium_core::ports::extensions::{
    ExtensionAcquiredCatalogActivationOutcome, ExtensionAcquiredCatalogActivationRequest,
    ExtensionAcquiredPackageProvisioningOutcome, ExtensionAcquiredPackageProvisioningRequest,
    ExtensionAcquiredRuntimeProfile, ExtensionAcquiredRuntimeSelection,
    MAX_EXTENSION_ACQUIRED_CATALOG_BYTES, MAX_EXTENSION_ACQUIRED_CRX_BYTES,
    MAX_EXTENSION_ACQUIRED_LEGAL_NOTICE_BYTES,
};
use zephium_extension_authority::{BundledPackageAuthority, ProductExtensionRuntimeTarget};
use zephium_extension_package::{
    MAX_CRX3_HEADER_BYTES, MAX_EXTENSION_ARCHIVE_BYTES, MAX_EXTENSION_LEGAL_NOTICE_BYTES,
    MAX_EXTENSION_PACKAGE_LINES, MAX_EXTENSION_RELEASE_CATALOG_BYTES,
};
use zephium_extension_repository::{
    AcquiredPackageMaterializationError, AcquiredPackageMaterializationOutcome,
    AcquiredReleaseLegalResource, AcquiredReleaseLegalSource, AcquiredReleaseLegalSourceError,
    BundledCatalogSetError, BundledCatalogSetPromotionOutcome, BundledCatalogSetStageOutcome,
    BundledPackageMaterializationError, BundledPackageRuntimeSelection, ExtensionRepositoryError,
};

use super::ServiceRepository;

const _: () = assert!(MAX_EXTENSION_ACQUIRED_CATALOG_BYTES == MAX_EXTENSION_RELEASE_CATALOG_BYTES);
const _: () = assert!(
    MAX_EXTENSION_ACQUIRED_CRX_BYTES
        >= MAX_EXTENSION_ARCHIVE_BYTES as usize + MAX_CRX3_HEADER_BYTES + 12
);
const _: () =
    assert!(MAX_EXTENSION_ACQUIRED_LEGAL_NOTICE_BYTES == MAX_EXTENSION_LEGAL_NOTICE_BYTES as usize);
const _: () = assert!(MAX_EXTENSION_PACKAGE_LINES == 8);

impl ServiceRepository {
    pub(crate) fn provision_acquired_package(
        &mut self,
        request: ExtensionAcquiredPackageProvisioningRequest,
    ) -> ExtensionAcquiredPackageProvisioningOutcome {
        let Some(repository) = self.repository.as_mut() else {
            return ExtensionAcquiredPackageProvisioningOutcome::Unavailable;
        };
        let (catalog_bytes, package_key, runtime_profile, crx3_bytes, legal_notice_bytes) =
            request.into_parts();
        let authority = match BundledPackageAuthority::product() {
            Ok(authority) => authority,
            Err(_) => return ExtensionAcquiredPackageProvisioningOutcome::Unavailable,
        };
        let catalog = match authority.admit_acquired_catalog(&catalog_bytes) {
            Ok(catalog) => catalog,
            Err(_) => return ExtensionAcquiredPackageProvisioningOutcome::Rejected,
        };
        let mut legal = ExactLegalNoticeSource::new(package_key, legal_notice_bytes);
        match repository.materialize_active_acquired_package(
            &catalog,
            &catalog_bytes,
            product_target(runtime_profile),
            package_key,
            &crx3_bytes,
            &mut legal,
        ) {
            Ok(AcquiredPackageMaterializationOutcome::Materialized) => {
                ExtensionAcquiredPackageProvisioningOutcome::Materialized
            }
            Ok(AcquiredPackageMaterializationOutcome::IdempotentReplay) => {
                ExtensionAcquiredPackageProvisioningOutcome::AlreadyMaterialized
            }
            Ok(_) => ExtensionAcquiredPackageProvisioningOutcome::FailedClosed,
            Err(error) => classify_materialization_error(&error),
        }
    }

    pub(crate) fn activate_acquired_catalog(
        &mut self,
        request: ExtensionAcquiredCatalogActivationRequest,
    ) -> ExtensionAcquiredCatalogActivationOutcome {
        let Some(repository) = self.repository.as_mut() else {
            return ExtensionAcquiredCatalogActivationOutcome::Unavailable;
        };
        let (catalog_bytes, selections) = request.into_parts();
        let authority = match BundledPackageAuthority::product() {
            Ok(authority) => authority,
            Err(_) => return ExtensionAcquiredCatalogActivationOutcome::Unavailable,
        };
        let catalog = match authority.admit_acquired_catalog(&catalog_bytes) {
            Ok(catalog) => catalog,
            Err(_) => return ExtensionAcquiredCatalogActivationOutcome::Rejected,
        };
        let selections = selections
            .iter()
            .copied()
            .map(repository_selection)
            .collect::<Vec<_>>();
        let staged = match repository.stage_active_acquired_catalog_set(
            &catalog,
            &catalog_bytes,
            &selections,
        ) {
            Ok(BundledCatalogSetStageOutcome::Staged(identity))
            | Ok(BundledCatalogSetStageOutcome::IdempotentCandidate(identity)) => identity,
            Ok(BundledCatalogSetStageOutcome::AlreadyCurrent(identity)) => {
                return ExtensionAcquiredCatalogActivationOutcome::AlreadyActive(
                    ExtensionCatalogSetDigest::from_bytes(identity.bytes()),
                );
            }
            Ok(BundledCatalogSetStageOutcome::AlreadyPrevious(_)) => {
                return ExtensionAcquiredCatalogActivationOutcome::Rejected;
            }
            Ok(_) => return ExtensionAcquiredCatalogActivationOutcome::FailedClosed,
            Err(error) => return classify_activation_error(&error),
        };
        match repository.promote_active_acquired_catalog_set(
            &catalog,
            &catalog_bytes,
            &selections,
            staged,
        ) {
            Ok(BundledCatalogSetPromotionOutcome::Promoted(identity)) => {
                ExtensionAcquiredCatalogActivationOutcome::Activated(
                    ExtensionCatalogSetDigest::from_bytes(identity.bytes()),
                )
            }
            Ok(BundledCatalogSetPromotionOutcome::IdempotentCurrent(identity)) => {
                ExtensionAcquiredCatalogActivationOutcome::AlreadyActive(
                    ExtensionCatalogSetDigest::from_bytes(identity.bytes()),
                )
            }
            Ok(_) => ExtensionAcquiredCatalogActivationOutcome::FailedClosed,
            Err(error) => classify_activation_error(&error),
        }
    }
}

struct ExactLegalNoticeSource {
    package_key: ExtensionPackageKey,
    bytes: Vec<u8>,
    consumed: bool,
}

impl ExactLegalNoticeSource {
    fn new(package_key: ExtensionPackageKey, bytes: Vec<u8>) -> Self {
        Self {
            package_key,
            bytes,
            consumed: false,
        }
    }
}

impl AcquiredReleaseLegalSource for ExactLegalNoticeSource {
    fn with_legal_notice<T, E, F>(
        &mut self,
        resource: AcquiredReleaseLegalResource<'_>,
        callback: F,
    ) -> Result<Result<T, E>, AcquiredReleaseLegalSourceError>
    where
        F: FnOnce(&mut dyn Read) -> Result<T, E>,
    {
        if self.consumed
            || resource.package().package_key() != self.package_key
            || usize::try_from(resource.expected_length()).ok() != Some(self.bytes.len())
        {
            return Err(AcquiredReleaseLegalSourceError::IdentityAmbiguous);
        }
        self.consumed = true;
        Ok(callback(&mut Cursor::new(&self.bytes)))
    }
}

const fn product_target(profile: ExtensionAcquiredRuntimeProfile) -> ProductExtensionRuntimeTarget {
    match profile {
        ExtensionAcquiredRuntimeProfile::MacosNative => ProductExtensionRuntimeTarget::MacosNative,
        ExtensionAcquiredRuntimeProfile::MacosNativeBrokered => {
            ProductExtensionRuntimeTarget::MacosNativeBrokered
        }
        ExtensionAcquiredRuntimeProfile::MacosCompatibility => {
            ProductExtensionRuntimeTarget::MacosCompatibility
        }
        ExtensionAcquiredRuntimeProfile::LinuxCompatibility => {
            ProductExtensionRuntimeTarget::LinuxCompatibility
        }
        ExtensionAcquiredRuntimeProfile::WindowsNative => {
            ProductExtensionRuntimeTarget::WindowsNative
        }
    }
}

const fn repository_selection(
    selection: ExtensionAcquiredRuntimeSelection,
) -> BundledPackageRuntimeSelection {
    BundledPackageRuntimeSelection::new(
        selection.package_key(),
        product_target(selection.runtime_profile()),
    )
}

fn classify_materialization_error(
    error: &AcquiredPackageMaterializationError,
) -> ExtensionAcquiredPackageProvisioningOutcome {
    match error {
        AcquiredPackageMaterializationError::Repository(error) => classify_repository_error(*error),
        AcquiredPackageMaterializationError::InterruptedBuildSettlement(_) => {
            ExtensionAcquiredPackageProvisioningOutcome::FailedClosed
        }
        _ => ExtensionAcquiredPackageProvisioningOutcome::Rejected,
    }
}

fn classify_repository_error(
    error: ExtensionRepositoryError,
) -> ExtensionAcquiredPackageProvisioningOutcome {
    match error {
        ExtensionRepositoryError::SettlementAmbiguous => {
            ExtensionAcquiredPackageProvisioningOutcome::OutcomeUnknown
        }
        ExtensionRepositoryError::GarbageCollectionInProgress
        | ExtensionRepositoryError::CatalogAdvanceBlockedByBuild
        | ExtensionRepositoryError::CatalogAdvanceBlockedByLiveGeneration => {
            ExtensionAcquiredPackageProvisioningOutcome::Unavailable
        }
        ExtensionRepositoryError::StateCorrupt
        | ExtensionRepositoryError::RecoveryAmbiguous
        | ExtensionRepositoryError::Sealed
        | ExtensionRepositoryError::FileSystem(_)
        | ExtensionRepositoryError::CallbackReentry => {
            ExtensionAcquiredPackageProvisioningOutcome::FailedClosed
        }
        _ => ExtensionAcquiredPackageProvisioningOutcome::Rejected,
    }
}

fn classify_activation_error(
    error: &BundledCatalogSetError,
) -> ExtensionAcquiredCatalogActivationOutcome {
    match error {
        BundledCatalogSetError::Package(BundledPackageMaterializationError::Repository(error))
        | BundledCatalogSetError::AcquiredPackage(
            AcquiredPackageMaterializationError::Repository(error),
        ) => match classify_repository_error(*error) {
            ExtensionAcquiredPackageProvisioningOutcome::Unavailable => {
                ExtensionAcquiredCatalogActivationOutcome::Unavailable
            }
            ExtensionAcquiredPackageProvisioningOutcome::OutcomeUnknown => {
                ExtensionAcquiredCatalogActivationOutcome::OutcomeUnknown
            }
            ExtensionAcquiredPackageProvisioningOutcome::FailedClosed => {
                ExtensionAcquiredCatalogActivationOutcome::FailedClosed
            }
            _ => ExtensionAcquiredCatalogActivationOutcome::Rejected,
        },
        BundledCatalogSetError::Package(
            BundledPackageMaterializationError::InterruptedBuildSettlement(_),
        )
        | BundledCatalogSetError::AcquiredPackage(
            AcquiredPackageMaterializationError::InterruptedBuildSettlement(_),
        ) => ExtensionAcquiredCatalogActivationOutcome::FailedClosed,
        _ => ExtensionAcquiredCatalogActivationOutcome::Rejected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquired_profile_mapping_preserves_broker_policy_without_splitting_native_ownership() {
        assert_eq!(
            product_target(ExtensionAcquiredRuntimeProfile::MacosNative),
            ProductExtensionRuntimeTarget::MacosNative
        );
        assert_eq!(
            product_target(ExtensionAcquiredRuntimeProfile::MacosNativeBrokered),
            ProductExtensionRuntimeTarget::MacosNativeBrokered
        );
        assert_eq!(
            ExtensionAcquiredRuntimeProfile::MacosNativeBrokered.runtime_backend(),
            zephium_core::extensions::ExtensionRuntimeBackendTarget::MacosNative
        );
    }
}
