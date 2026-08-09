//! Zero-worker startup planning for builds with no extension authority.

use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::time::Instant;

use zephium_core::ids::ProfileId;
use zephium_core::ports::extensions::{
    ExtensionProfileRetirementDisposition, ExtensionServiceLifecycle,
    ExtensionServiceShutdownOutcome, ExtensionServiceStartupOutcome,
};
use zephium_extension_authority::{
    BundledPackageAuthority, BundledProductAuthorityStatus, ProductExtensionManifestAuthority,
    ProductExtensionManifestAuthorityStatus,
};
use zephium_extension_runtime_api::ExtensionRuntimeHostFactory;
use zephium_store::{ExtensionServiceStoreAuthority, ExtensionServiceStoreStartupRequirement};

use crate::startup::{ExtensionRepositoryRoot, ExtensionServiceLaunchInput};

/// Fail-closed reason a product build cannot select one extension startup
/// topology.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionServiceBootError {
    /// Package and manifest product authorities are invalid or disagree about
    /// whether extensions are provisioned in this exact binary.
    InvalidProductProvisioning,
}

impl fmt::Display for ExtensionServiceBootError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "extension package and manifest product authorities are invalid or inconsistent",
        )
    }
}

impl Error for ExtensionServiceBootError {}

/// Move-only prerequisites for the full extension-service worker.
///
/// The native host factory is deliberately absent until the caller selects
/// this branch. An inert product launch therefore leaves native runtime
/// authority inside the engine and cannot accidentally initialize a host.
pub struct ExtensionServiceWorkerLaunch {
    store_authority: ExtensionServiceStoreAuthority,
    repository_root: ExtensionRepositoryRoot,
}

impl ExtensionServiceWorkerLaunch {
    /// Joins the engine's unique native-host factory only after the full worker
    /// branch has been selected.
    pub fn bind_host_factory(
        self,
        host_factory: ExtensionRuntimeHostFactory,
    ) -> ExtensionServiceLaunchInput {
        ExtensionServiceLaunchInput::new(self.store_authority, self.repository_root, host_factory)
    }
}

/// Exact startup topology selected before any extension repository is opened
/// or service worker is spawned.
pub enum ExtensionServiceBootPlan {
    /// Compile-time product authority is absent, the repository path is
    /// definitely absent, and Store proved no unresolved native owner.
    Inert(Box<dyn ExtensionServiceLifecycle>),
    /// Product authority or possible cleanup state requires the serialized
    /// repository/recovery worker.
    Worker(ExtensionServiceWorkerLaunch),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProductAuthorityAvailability {
    Unprovisioned,
    Configured,
}

/// Selects the zero-worker path only from three exact, independent facts.
///
/// Any filesystem ambiguity, durable native-owner debt, or configured product
/// authority selects the full worker. This function performs no repository
/// construction and no Store actor wait: the Store requirement was captured
/// synchronously during `SqliteStore::open`.
pub fn prepare_extension_service_boot(
    store_authority: ExtensionServiceStoreAuthority,
    repository_root: ExtensionRepositoryRoot,
) -> Result<ExtensionServiceBootPlan, ExtensionServiceBootError> {
    let product = classify_product_authority(
        BundledPackageAuthority::product_status(),
        ProductExtensionManifestAuthority::product_status(),
    )?;
    if product == ProductAuthorityAvailability::Unprovisioned
        && repository_root_is_definitely_absent(&repository_root)
        && store_authority.startup_requirement()
            == ExtensionServiceStoreStartupRequirement::NoNativeOwnershipDebt
    {
        return Ok(ExtensionServiceBootPlan::Inert(Box::new(
            InertExtensionServiceLifecycle {
                _store_authority: store_authority,
            },
        )));
    }
    Ok(ExtensionServiceBootPlan::Worker(
        ExtensionServiceWorkerLaunch {
            store_authority,
            repository_root,
        },
    ))
}

fn classify_product_authority(
    packages: BundledProductAuthorityStatus,
    manifests: ProductExtensionManifestAuthorityStatus,
) -> Result<ProductAuthorityAvailability, ExtensionServiceBootError> {
    match (packages, manifests) {
        (
            BundledProductAuthorityStatus::Unprovisioned,
            ProductExtensionManifestAuthorityStatus::Unprovisioned,
        ) => Ok(ProductAuthorityAvailability::Unprovisioned),
        (
            BundledProductAuthorityStatus::Configured,
            ProductExtensionManifestAuthorityStatus::Configured,
        ) => Ok(ProductAuthorityAvailability::Configured),
        _ => Err(ExtensionServiceBootError::InvalidProductProvisioning),
    }
}

fn repository_root_is_definitely_absent(root: &ExtensionRepositoryRoot) -> bool {
    matches!(
        fs::symlink_metadata(root.path()),
        Err(error) if error.kind() == io::ErrorKind::NotFound
    )
}

/// Zero-thread lifecycle authority for an exact inert product launch.
///
/// Retaining Store's move-only capability prevents a second service topology
/// from being installed later in the process. Product authority is immutable,
/// the repository was absent, and the captured journal was empty, so no
/// extension runtime can appear behind this fence.
struct InertExtensionServiceLifecycle {
    _store_authority: ExtensionServiceStoreAuthority,
}

impl ExtensionServiceLifecycle for InertExtensionServiceLifecycle {
    fn settle_startup_until(&mut self, _deadline: Instant) -> ExtensionServiceStartupOutcome {
        ExtensionServiceStartupOutcome::Ready(
            zephium_core::ports::extensions::ExtensionActiveProfiles::EMPTY,
        )
    }

    fn with_profile_retired_until(
        &mut self,
        _profile: ProfileId,
        deadline: Instant,
        continuation: Box<dyn FnOnce() + '_>,
    ) -> ExtensionProfileRetirementDisposition {
        if Instant::now() >= deadline {
            drop(continuation);
            return ExtensionProfileRetirementDisposition::Unavailable;
        }
        continuation();
        ExtensionProfileRetirementDisposition::Continued
    }

    fn shutdown_until(self: Box<Self>, _deadline: Instant) -> ExtensionServiceShutdownOutcome {
        ExtensionServiceShutdownOutcome::Clean
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use tempfile::tempdir;
    use zephium_core::ports::store::StoreShutdownOutcome;
    use zephium_store::SqliteStore;

    use super::*;

    #[test]
    fn product_authorities_must_be_both_absent_or_both_configured() {
        assert_eq!(
            classify_product_authority(
                BundledProductAuthorityStatus::Unprovisioned,
                ProductExtensionManifestAuthorityStatus::Unprovisioned,
            ),
            Ok(ProductAuthorityAvailability::Unprovisioned)
        );
        assert_eq!(
            classify_product_authority(
                BundledProductAuthorityStatus::Configured,
                ProductExtensionManifestAuthorityStatus::Configured,
            ),
            Ok(ProductAuthorityAvailability::Configured)
        );
        for (packages, manifests) in [
            (
                BundledProductAuthorityStatus::InvalidProvisioning,
                ProductExtensionManifestAuthorityStatus::Unprovisioned,
            ),
            (
                BundledProductAuthorityStatus::Unprovisioned,
                ProductExtensionManifestAuthorityStatus::InvalidProvisioning,
            ),
            (
                BundledProductAuthorityStatus::Configured,
                ProductExtensionManifestAuthorityStatus::Unprovisioned,
            ),
            (
                BundledProductAuthorityStatus::Unprovisioned,
                ProductExtensionManifestAuthorityStatus::Configured,
            ),
        ] {
            assert_eq!(
                classify_product_authority(packages, manifests),
                Err(ExtensionServiceBootError::InvalidProductProvisioning)
            );
        }
    }

    #[cfg(not(zephium_internal_repository_e2e))]
    #[test]
    fn exact_empty_unprovisioned_launch_is_inert_and_never_creates_repository() {
        let temporary = tempdir().unwrap();
        let repository_path = temporary
            .path()
            .join(crate::EXTENSION_REPOSITORY_DIRECTORY_NAME);
        let store = Arc::new(SqliteStore::open(temporary.path()).unwrap());
        let authority = store.claim_extension_service_store_authority().unwrap();
        let root = ExtensionRepositoryRoot::from_app_data_directory(temporary.path()).unwrap();
        let ExtensionServiceBootPlan::Inert(mut lifecycle) =
            prepare_extension_service_boot(authority, root).unwrap()
        else {
            panic!("empty unprovisioned launch unexpectedly selected a worker");
        };
        assert!(!repository_path.exists());
        assert_eq!(
            lifecycle.settle_startup_until(Instant::now()),
            ExtensionServiceStartupOutcome::Ready(
                zephium_core::ports::extensions::ExtensionActiveProfiles::EMPTY,
            )
        );
        let continued = std::cell::Cell::new(false);
        assert_eq!(
            lifecycle.with_profile_retired_until(
                ProfileId::from(7),
                Instant::now() + Duration::from_secs(1),
                Box::new(|| continued.set(true)),
            ),
            ExtensionProfileRetirementDisposition::Continued
        );
        assert!(continued.get());
        assert_eq!(
            lifecycle.shutdown_until(Instant::now()),
            ExtensionServiceShutdownOutcome::Clean
        );
        assert!(!repository_path.exists());
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(1)),
            StoreShutdownOutcome::Clean
        );
    }

    #[test]
    fn any_existing_repository_node_selects_worker_without_opening_it() {
        let temporary = tempdir().unwrap();
        let repository_path = temporary
            .path()
            .join(crate::EXTENSION_REPOSITORY_DIRECTORY_NAME);
        fs::create_dir(&repository_path).unwrap();
        let store = Arc::new(SqliteStore::open(temporary.path()).unwrap());
        let authority = store.claim_extension_service_store_authority().unwrap();
        let root = ExtensionRepositoryRoot::from_app_data_directory(temporary.path()).unwrap();
        let plan = prepare_extension_service_boot(authority, root).unwrap();
        assert!(matches!(plan, ExtensionServiceBootPlan::Worker(_)));
        assert!(repository_path.is_dir());
        drop(plan);
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(1)),
            StoreShutdownOutcome::Clean
        );
    }

    #[cfg(not(zephium_internal_repository_e2e))]
    #[test]
    fn expired_inert_retirement_never_invokes_its_continuation() {
        let temporary = tempdir().unwrap();
        let store = Arc::new(SqliteStore::open(temporary.path()).unwrap());
        let authority = store.claim_extension_service_store_authority().unwrap();
        let root = ExtensionRepositoryRoot::from_app_data_directory(temporary.path()).unwrap();
        let ExtensionServiceBootPlan::Inert(mut lifecycle) =
            prepare_extension_service_boot(authority, root).unwrap()
        else {
            panic!("empty unprovisioned launch unexpectedly selected a worker");
        };
        let continued = std::cell::Cell::new(false);
        assert_eq!(
            lifecycle.with_profile_retired_until(
                ProfileId::from(8),
                Instant::now(),
                Box::new(|| continued.set(true)),
            ),
            ExtensionProfileRetirementDisposition::Unavailable
        );
        assert!(!continued.get());
        assert_eq!(
            lifecycle.shutdown_until(Instant::now()),
            ExtensionServiceShutdownOutcome::Clean
        );
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(1)),
            StoreShutdownOutcome::Clean
        );
    }
}
