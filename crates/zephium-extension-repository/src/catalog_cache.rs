//! Bounded in-process cache of product-authenticated catalog projections.

use std::mem::size_of;

#[cfg(feature = "acquired-packages")]
use zephium_extension_authority::AdmittedAcquiredCatalog;
use zephium_extension_authority::{
    AdmittedActiveCatalog, AdmittedBundledCatalog, AdmittedRollbackCatalog,
    BundledPackageAuthority, ProductBundledCatalogGenerationRole,
    MAX_BUNDLED_PACKAGE_AUTHORITY_RETAINED_BYTES, MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS,
};
use zephium_extension_package::{
    ExtensionReleaseCatalog, MAX_EXTENSION_RELEASE_CATALOG_RETAINED_BYTES,
};
use zephium_private_fs::{FileIdentity, PrivateDirectory};

use crate::codec;
use crate::materialization::CatalogAnchor;
use crate::state::Digest32;
use crate::ExtensionRepositoryError;

const CACHE_ENTRY_ACCOUNTING_OVERHEAD: usize = size_of::<CatalogAnchor>()
    + size_of::<ProductBundledCatalogGenerationRole>()
    + size_of::<FileIdentity>()
    + size_of::<usize>()
    + 64;
const MAX_PRODUCT_CATALOG_CACHE_ENTRY_BYTES: usize =
    MAX_EXTENSION_RELEASE_CATALOG_RETAINED_BYTES + CACHE_ENTRY_ACCOUNTING_OVERHEAD;
pub(crate) const MAX_PRODUCT_CATALOG_ADMISSION_CACHE_RETAINED_BYTES: usize =
    MAX_BUNDLED_PACKAGE_AUTHORITY_RETAINED_BYTES
        + MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS * MAX_PRODUCT_CATALOG_CACHE_ENTRY_BYTES;

const _: () = assert!(MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS == 3);
const _: () = assert!(
    size_of::<CachedProductCatalog>()
        <= size_of::<ExtensionReleaseCatalog>() + CACHE_ENTRY_ACCOUNTING_OVERHEAD
);

struct CachedProductCatalog {
    anchor: CatalogAnchor,
    role: ProductBundledCatalogGenerationRole,
    identity: FileIdentity,
    catalog: ExtensionReleaseCatalog,
    retained_bytes: usize,
}

/// Borrowed proof that one cached catalog still matches its exact anchor,
/// file identity, product role, and current durable high-water.
pub(crate) struct CachedProductCatalogAdmission<'a> {
    catalog: &'a ExtensionReleaseCatalog,
}

impl<'a> CachedProductCatalogAdmission<'a> {
    pub(crate) const fn catalog(&self) -> &'a ExtensionReleaseCatalog {
        self.catalog
    }
}

/// Move-independent evidence that exact bounded bytes were revalidated for
/// one cached file identity during the current recovery pass.
///
/// Fields are private so callers cannot synthesize a warm-cache authority
/// proof from an anchor and identity alone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CachedProductCatalogAuthentication {
    anchor: CatalogAnchor,
    identity: FileIdentity,
    epoch: u64,
}

/// One cache hit joined to freshly authenticated exact bytes.
struct CachedProductCatalogHit {
    authentication: CachedProductCatalogAuthentication,
}

impl CachedProductCatalogHit {
    const fn authentication(&self) -> CachedProductCatalogAuthentication {
        self.authentication
    }
}

/// Result of authenticating one exact catalog object for a recovery pass.
pub(crate) struct ProductCatalogObjectAuthentication {
    authentication: CachedProductCatalogAuthentication,
    _bytes_read: usize,
    _cache_hit: bool,
}

impl ProductCatalogObjectAuthentication {
    pub(crate) const fn authentication(&self) -> CachedProductCatalogAuthentication {
        self.authentication
    }

    #[cfg(all(
        test,
        zephium_internal_repository_e2e,
        any(target_os = "macos", target_os = "linux")
    ))]
    pub(crate) const fn bytes_read(&self) -> usize {
        self._bytes_read
    }

    #[cfg(all(
        test,
        zephium_internal_repository_e2e,
        any(target_os = "macos", target_os = "linux")
    ))]
    pub(crate) const fn cache_hit(&self) -> bool {
        self._cache_hit
    }
}

/// Process-local acceleration for repeated recovery of the same immutable
/// product catalog generations.
///
/// Entries carry no durable or runtime authority. Every lookup remains joined
/// to the exact recovered anchor, catalog-file identity, and current high-water.
/// The cache starts empty after process restart and is cleared if the writer is
/// sealed, so crash recovery always reconstructs authority from exact bytes at
/// least once.
pub(crate) struct ProductCatalogAdmissionCache {
    authority: Option<BundledPackageAuthority>,
    entries: [Option<CachedProductCatalog>; MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS],
    retained_bytes: usize,
    epoch: u64,
}

impl ProductCatalogAdmissionCache {
    pub(crate) fn new() -> Self {
        Self {
            authority: None,
            entries: [const { None }; MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS],
            retained_bytes: 0,
            epoch: 1,
        }
    }

    /// Reads and authenticates one exact content-addressed catalog object.
    ///
    /// This method owns the private-filesystem join so callers cannot pair a
    /// cached identity with bytes obtained from somewhere else. A warm hit
    /// skips only canonical parsing and product admission; it still performs
    /// a bounded read, pre/post identity checks, exact length, and SHA-256.
    pub(crate) fn authenticate_catalog_object(
        &mut self,
        catalog_objects: &PrivateDirectory,
        anchor: CatalogAnchor,
        expected_identity: FileIdentity,
        high_water: Option<CatalogAnchor>,
    ) -> Result<ProductCatalogObjectAuthentication, ExtensionRepositoryError> {
        let name = crate::names::catalog_file(anchor.catalog_sha256);
        if catalog_objects
            .regular_identity(&name)
            .map_err(crate::storage::map_recovery_fs)?
            != Some(expected_identity)
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        let bytes = crate::storage::read_required_recovery(
            catalog_objects,
            &name,
            zephium_extension_package::MAX_EXTENSION_RELEASE_CATALOG_BYTES,
        )?;
        if u64::try_from(bytes.len()).ok() != Some(anchor.catalog_length)
            || catalog_objects
                .regular_identity(&name)
                .map_err(crate::storage::map_recovery_fs)?
                != Some(expected_identity)
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        if let Some(hit) = self.lookup(anchor, expected_identity, high_water, &bytes)? {
            return Ok(ProductCatalogObjectAuthentication {
                authentication: hit.authentication(),
                _bytes_read: bytes.len(),
                _cache_hit: true,
            });
        }
        let authentication =
            self.authenticate_bytes(anchor, expected_identity, high_water, &bytes)?;
        Ok(ProductCatalogObjectAuthentication {
            authentication,
            _bytes_read: bytes.len(),
            _cache_hit: false,
        })
    }

    fn product_authority(&mut self) -> Result<&BundledPackageAuthority, ExtensionRepositoryError> {
        if self.epoch == 0 {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        if self.authority.is_none() {
            let authority = BundledPackageAuthority::product()
                .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
            let retained_bytes = self
                .retained_bytes
                .checked_add(authority.retained_bytes())
                .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
            if retained_bytes > MAX_PRODUCT_CATALOG_ADMISSION_CACHE_RETAINED_BYTES {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
            self.retained_bytes = retained_bytes;
            self.authority = Some(authority);
        }
        self.authority
            .as_ref()
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)
    }

    fn lookup(
        &self,
        anchor: CatalogAnchor,
        identity: FileIdentity,
        high_water: Option<CatalogAnchor>,
        exact_catalog_bytes: &[u8],
    ) -> Result<Option<CachedProductCatalogHit>, ExtensionRepositoryError> {
        anchor.validate()?;
        let Some(entry) = self
            .entries
            .iter()
            .flatten()
            .find(|entry| entry.anchor == anchor)
        else {
            if self
                .entries
                .iter()
                .flatten()
                .any(|entry| entry.anchor.catalog_sha256 == anchor.catalog_sha256)
            {
                return Err(ExtensionRepositoryError::RecoveryAmbiguous);
            }
            return Ok(None);
        };
        if entry.identity != identity {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        validate_exact_catalog_bytes(exact_catalog_bytes, anchor)?;
        validate_role_against_high_water(entry.role, anchor, high_water)?;
        validate_catalog_against_anchor(&entry.catalog, anchor)?;
        Ok(Some(CachedProductCatalogHit {
            authentication: CachedProductCatalogAuthentication {
                anchor,
                identity,
                epoch: self.epoch,
            },
        }))
    }

    pub(crate) fn lookup_authenticated(
        &self,
        authentication: CachedProductCatalogAuthentication,
        high_water: Option<CatalogAnchor>,
    ) -> Result<CachedProductCatalogAdmission<'_>, ExtensionRepositoryError> {
        let entry = self
            .entries
            .iter()
            .flatten()
            .find(|entry| entry.anchor == authentication.anchor)
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        if self.epoch == 0
            || authentication.epoch != self.epoch
            || entry.identity != authentication.identity
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        validate_role_against_high_water(entry.role, entry.anchor, high_water)?;
        validate_catalog_against_anchor(&entry.catalog, entry.anchor)?;
        Ok(CachedProductCatalogAdmission {
            catalog: &entry.catalog,
        })
    }

    fn authenticate_bytes(
        &mut self,
        anchor: CatalogAnchor,
        identity: FileIdentity,
        high_water: Option<CatalogAnchor>,
        exact_catalog_bytes: &[u8],
    ) -> Result<CachedProductCatalogAuthentication, ExtensionRepositoryError> {
        let (role, admitted_anchor, catalog) = {
            let authority = self.product_authority()?;
            let role = anchor
                .generation_anchor()
                .ok()
                .and_then(|generation| authority.recognize_generation(&generation))
                .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
            validate_role_against_high_water(role, anchor, high_water)?;
            match role {
                ProductBundledCatalogGenerationRole::Active => {
                    let admitted = authority
                        .admit_active_catalog(exact_catalog_bytes)
                        .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
                    (role, active_anchor(&admitted), admitted.catalog().clone())
                }
                ProductBundledCatalogGenerationRole::Rollback => {
                    let admitted = authority
                        .admit_rollback_catalog(exact_catalog_bytes)
                        .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
                    (role, rollback_anchor(&admitted), admitted.catalog().clone())
                }
            }
        };
        if admitted_anchor != anchor {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        self.insert(anchor, role, identity, catalog)?;
        Ok(CachedProductCatalogAuthentication {
            anchor,
            identity,
            epoch: self.epoch,
        })
    }

    fn insert(
        &mut self,
        anchor: CatalogAnchor,
        role: ProductBundledCatalogGenerationRole,
        identity: FileIdentity,
        catalog: ExtensionReleaseCatalog,
    ) -> Result<(), ExtensionRepositoryError> {
        if self.epoch == 0 {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        anchor.validate()?;
        validate_catalog_against_anchor(&catalog, anchor)?;
        if let Some(existing) = self
            .entries
            .iter()
            .flatten()
            .find(|entry| entry.anchor == anchor)
        {
            return if existing.role == role
                && existing.identity == identity
                && existing.catalog == catalog
            {
                Ok(())
            } else {
                Err(ExtensionRepositoryError::RecoveryAmbiguous)
            };
        }
        if self
            .entries
            .iter()
            .flatten()
            .any(|entry| entry.anchor.catalog_sha256 == anchor.catalog_sha256)
            || self.entry_count() >= MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        let retained_bytes = catalog
            .retained_bytes()
            .checked_add(CACHE_ENTRY_ACCOUNTING_OVERHEAD)
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        if retained_bytes > MAX_PRODUCT_CATALOG_CACHE_ENTRY_BYTES {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        let total = self
            .retained_bytes
            .checked_add(retained_bytes)
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        if total > MAX_PRODUCT_CATALOG_ADMISSION_CACHE_RETAINED_BYTES {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        let slot = self
            .entries
            .iter_mut()
            .find(|entry| entry.is_none())
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        *slot = Some(CachedProductCatalog {
            anchor,
            role,
            identity,
            catalog,
            retained_bytes,
        });
        self.retained_bytes = total;
        Ok(())
    }

    pub(crate) fn seed_active(
        &mut self,
        identity: FileIdentity,
        admitted: &AdmittedBundledCatalog,
    ) -> Result<(), ExtensionRepositoryError> {
        self.insert(
            bundled_active_anchor(admitted),
            ProductBundledCatalogGenerationRole::Active,
            identity,
            admitted.catalog().clone(),
        )
    }

    #[cfg(feature = "acquired-packages")]
    pub(crate) fn seed_active_acquired(
        &mut self,
        identity: FileIdentity,
        admitted: &AdmittedAcquiredCatalog,
    ) -> Result<(), ExtensionRepositoryError> {
        self.insert(
            acquired_active_anchor(admitted),
            ProductBundledCatalogGenerationRole::Active,
            identity,
            admitted.catalog().clone(),
        )
    }

    pub(crate) fn seed_rollback(
        &mut self,
        identity: FileIdentity,
        admitted: &AdmittedRollbackCatalog,
    ) -> Result<(), ExtensionRepositoryError> {
        self.insert(
            rollback_anchor(admitted),
            ProductBundledCatalogGenerationRole::Rollback,
            identity,
            admitted.catalog().clone(),
        )
    }

    pub(crate) fn forget_digest(
        &mut self,
        digest: Digest32,
    ) -> Result<(), ExtensionRepositoryError> {
        let has_entry = self
            .entries
            .iter()
            .flatten()
            .any(|entry| entry.anchor.catalog_sha256 == digest);
        if !has_entry {
            return Ok(());
        }
        let next_epoch = self
            .epoch
            .checked_add(1)
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        for slot in &mut self.entries {
            if slot
                .as_ref()
                .is_none_or(|entry| entry.anchor.catalog_sha256 != digest)
            {
                continue;
            }
            let entry = slot
                .take()
                .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
            self.retained_bytes = self
                .retained_bytes
                .checked_sub(entry.retained_bytes)
                .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        }
        self.epoch = next_epoch;
        Ok(())
    }

    pub(crate) fn clear(&mut self) {
        self.authority = None;
        self.entries = [const { None }; MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS];
        self.retained_bytes = 0;
        // Clearing is terminal for this cache instance: it occurs only while
        // sealing the owning repository and must invalidate every outstanding
        // recovery-pass token even if a file identity is later reused.
        self.epoch = 0;
    }

    fn entry_count(&self) -> usize {
        self.entries.iter().flatten().count()
    }

    #[cfg(test)]
    fn entries_are_empty(&self) -> bool {
        self.entries.iter().all(Option::is_none)
    }
}

fn validate_exact_catalog_bytes(
    bytes: &[u8],
    anchor: CatalogAnchor,
) -> Result<(), ExtensionRepositoryError> {
    if u64::try_from(bytes.len()).ok() != Some(anchor.catalog_length)
        || codec::digest(bytes) != anchor.catalog_sha256
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(())
}

fn active_anchor(admitted: &AdmittedActiveCatalog) -> CatalogAnchor {
    let anchor = admitted.generation_anchor();
    CatalogAnchor {
        authority_id: Digest32::from_bytes(anchor.authority().bytes()),
        revision: anchor.revision().get(),
        catalog_length: anchor.catalog_length(),
        catalog_sha256: Digest32::from_bytes(anchor.catalog_digest().bytes()),
        inventory_sha256: Digest32::from_bytes(anchor.inventory_digest().bytes()),
    }
}

fn bundled_active_anchor(admitted: &AdmittedBundledCatalog) -> CatalogAnchor {
    CatalogAnchor {
        authority_id: Digest32::from_bytes(admitted.authority().bytes()),
        revision: admitted.revision().get(),
        catalog_length: admitted.catalog_length(),
        catalog_sha256: Digest32::from_bytes(admitted.catalog_digest().bytes()),
        inventory_sha256: Digest32::from_bytes(admitted.inventory_digest().bytes()),
    }
}

#[cfg(feature = "acquired-packages")]
fn acquired_active_anchor(admitted: &AdmittedAcquiredCatalog) -> CatalogAnchor {
    CatalogAnchor {
        authority_id: Digest32::from_bytes(admitted.authority().bytes()),
        revision: admitted.revision().get(),
        catalog_length: admitted.catalog_length(),
        catalog_sha256: Digest32::from_bytes(admitted.catalog_digest().bytes()),
        inventory_sha256: Digest32::from_bytes(admitted.inventory_digest().bytes()),
    }
}

fn rollback_anchor(admitted: &AdmittedRollbackCatalog) -> CatalogAnchor {
    CatalogAnchor {
        authority_id: Digest32::from_bytes(admitted.authority().bytes()),
        revision: admitted.revision().get(),
        catalog_length: admitted.catalog_length(),
        catalog_sha256: Digest32::from_bytes(admitted.catalog_digest().bytes()),
        inventory_sha256: Digest32::from_bytes(admitted.inventory_digest().bytes()),
    }
}

fn validate_catalog_against_anchor(
    catalog: &ExtensionReleaseCatalog,
    anchor: CatalogAnchor,
) -> Result<(), ExtensionRepositoryError> {
    if catalog.authority().bytes() != anchor.authority_id.bytes()
        || catalog.revision().get() != anchor.revision
        || catalog.digest().bytes() != anchor.catalog_sha256.bytes()
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(())
}

pub(crate) fn validate_role_against_high_water(
    role: ProductBundledCatalogGenerationRole,
    catalog: CatalogAnchor,
    high_water: Option<CatalogAnchor>,
) -> Result<(), ExtensionRepositoryError> {
    let high_water = high_water.ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
    let valid = match role {
        ProductBundledCatalogGenerationRole::Active => catalog == high_water,
        ProductBundledCatalogGenerationRole::Rollback => {
            catalog.authority_id == high_water.authority_id
                && (catalog.revision < high_water.revision || catalog == high_water)
        }
    };
    if !valid {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(())
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use std::fs;
    use std::os::unix::fs::MetadataExt as _;
    use std::os::unix::fs::PermissionsExt as _;

    use tempfile::TempDir;
    use zephium_private_fs::{ByteLimit, LockedPrivateNamespace, PrivateComponent};

    use super::*;

    struct StructuralGeneration {
        anchor: CatalogAnchor,
        catalog: ExtensionReleaseCatalog,
        bytes: Vec<u8>,
    }

    fn structural_generation(revision: u64, inventory_byte: u8) -> StructuralGeneration {
        const ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";
        let bytes = format!(
            concat!(
                r#"{{"schema_version":1,"catalog_revision":{revision},"created_unix":{revision},"authority_id":"{zero}","admission_policy_sha256":"{zero}","packages":[{{"package_key":"{zero}","revision":1,"payload":{{"kind":"bundled_tree"}},"manifest_sha256":"{zero}","tree_sha256":"{zero}","tree_index_sha256":"{zero}","tree_index_length":1,"tree_file_count":1,"tree_bytes":1,"chromium":null,"provenance":{{"source_url":"https://example.com/releases/v1/source","upstream_version":"1","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Example","redistribution":"Reviewed","legal_notice":{{"target":"licenses/example.txt","kind":"notice_bundle","length":1,"sha256":"{zero}"}},"corresponding_source":null}}}}]}}"#,
            ),
            revision = revision,
            zero = ZERO,
        )
        .into_bytes();
        let catalog = ExtensionReleaseCatalog::parse_canonical(&bytes).unwrap();
        let anchor = CatalogAnchor {
            authority_id: Digest32::from_bytes(catalog.authority().bytes()),
            revision: catalog.revision().get(),
            catalog_length: u64::try_from(bytes.len()).unwrap(),
            catalog_sha256: Digest32::from_bytes(catalog.digest().bytes()),
            inventory_sha256: Digest32::from_bytes([inventory_byte; 32]),
        };
        StructuralGeneration {
            anchor,
            catalog,
            bytes,
        }
    }

    fn test_identities(count: usize) -> (TempDir, Vec<FileIdentity>) {
        #[cfg(target_os = "macos")]
        let temporary = tempfile::tempdir_in("/private/tmp").unwrap();
        #[cfg(target_os = "linux")]
        let temporary = tempfile::tempdir_in("/tmp").unwrap();
        fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let namespace =
            LockedPrivateNamespace::open_or_create(temporary.path().join("repository")).unwrap();
        let identities = (0..count)
            .map(|index| {
                let name = PrivateComponent::new(format!("identity-{index}")).unwrap();
                let byte = [u8::try_from(index).unwrap()];
                namespace
                    .directory()
                    .write_new_synced(&name, &byte, ByteLimit::new(1).unwrap())
                    .unwrap()
            })
            .collect();
        (temporary, identities)
    }

    fn assert_ambiguous<T>(result: Result<T, ExtensionRepositoryError>) {
        assert!(matches!(
            result,
            Err(ExtensionRepositoryError::RecoveryAmbiguous)
        ));
    }

    fn cached_catalog<'a>(
        cache: &'a ProductCatalogAdmissionCache,
        anchor: CatalogAnchor,
        identity: FileIdentity,
        high_water: Option<CatalogAnchor>,
        bytes: &[u8],
    ) -> &'a ExtensionReleaseCatalog {
        let authentication = cache
            .lookup(anchor, identity, high_water, bytes)
            .unwrap()
            .unwrap()
            .authentication();
        cache
            .lookup_authenticated(authentication, high_water)
            .unwrap()
            .catalog()
    }

    #[test]
    fn exact_anchor_with_wrong_file_identity_fails_closed_instead_of_missing() {
        let (_temporary, identities) = test_identities(2);
        let generation = structural_generation(1, 1);
        let missing = structural_generation(2, 2);
        let mut cache = ProductCatalogAdmissionCache::new();
        cache
            .insert(
                generation.anchor,
                ProductBundledCatalogGenerationRole::Active,
                identities[0],
                generation.catalog.clone(),
            )
            .unwrap();

        assert_ambiguous(cache.lookup(
            generation.anchor,
            identities[1],
            Some(generation.anchor),
            &generation.bytes,
        ));
        let mut mutated_bytes = generation.bytes.clone();
        mutated_bytes[0] ^= 1;
        assert_ambiguous(cache.lookup(
            generation.anchor,
            identities[0],
            Some(generation.anchor),
            &mutated_bytes,
        ));
        assert!(cache
            .lookup(
                missing.anchor,
                identities[1],
                Some(generation.anchor),
                &missing.bytes,
            )
            .unwrap()
            .is_none());
    }

    #[test]
    fn warm_live_catalog_same_inode_mutation_is_not_a_cache_hit() {
        #[cfg(target_os = "macos")]
        let temporary = tempfile::tempdir_in("/private/tmp").unwrap();
        #[cfg(target_os = "linux")]
        let temporary = tempfile::tempdir_in("/tmp").unwrap();
        fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let repository_path = temporary.path().join("repository");
        let namespace = LockedPrivateNamespace::open_or_create(&repository_path).unwrap();
        let generation = structural_generation(1, 1);
        let name = crate::names::catalog_file(generation.anchor.catalog_sha256);
        let identity = namespace
            .directory()
            .write_new_synced(
                &name,
                &generation.bytes,
                ByteLimit::new(generation.bytes.len()).unwrap(),
            )
            .unwrap();
        let mut cache = ProductCatalogAdmissionCache::new();
        cache
            .insert(
                generation.anchor,
                ProductBundledCatalogGenerationRole::Active,
                identity,
                generation.catalog,
            )
            .unwrap();
        let authentication = cache
            .authenticate_catalog_object(
                namespace.directory(),
                generation.anchor,
                identity,
                Some(generation.anchor),
            )
            .unwrap();
        assert!(authentication._cache_hit);

        let catalog_path = repository_path.join(name.as_str());
        let inode = fs::metadata(&catalog_path).unwrap().ino();
        let mut mutated = generation.bytes;
        mutated[0] ^= 1;
        fs::write(&catalog_path, mutated).unwrap();
        assert_eq!(fs::metadata(&catalog_path).unwrap().ino(), inode);
        assert_ambiguous(cache.authenticate_catalog_object(
            namespace.directory(),
            generation.anchor,
            identity,
            Some(generation.anchor),
        ));
    }

    #[test]
    fn lookup_validates_anchor_role_and_current_high_water() {
        let (_temporary, identities) = test_identities(2);
        let rollback = structural_generation(2, 2);
        let active = structural_generation(3, 3);
        let older = structural_generation(1, 1);
        let mut cache = ProductCatalogAdmissionCache::new();
        cache
            .insert(
                rollback.anchor,
                ProductBundledCatalogGenerationRole::Rollback,
                identities[0],
                rollback.catalog.clone(),
            )
            .unwrap();
        cache
            .insert(
                active.anchor,
                ProductBundledCatalogGenerationRole::Active,
                identities[1],
                active.catalog.clone(),
            )
            .unwrap();

        assert_eq!(
            cached_catalog(
                &cache,
                active.anchor,
                identities[1],
                Some(active.anchor),
                &active.bytes,
            ),
            &active.catalog
        );
        assert_eq!(
            cached_catalog(
                &cache,
                rollback.anchor,
                identities[0],
                Some(active.anchor),
                &rollback.bytes,
            ),
            &rollback.catalog
        );
        assert_eq!(
            cached_catalog(
                &cache,
                rollback.anchor,
                identities[0],
                Some(rollback.anchor),
                &rollback.bytes,
            ),
            &rollback.catalog
        );
        assert_ambiguous(cache.lookup(active.anchor, identities[1], None, &active.bytes));
        assert_ambiguous(cache.lookup(
            active.anchor,
            identities[1],
            Some(rollback.anchor),
            &active.bytes,
        ));
        assert_ambiguous(cache.lookup(
            rollback.anchor,
            identities[0],
            Some(older.anchor),
            &rollback.bytes,
        ));

        let mut foreign_high_water = active.anchor;
        foreign_high_water.authority_id = Digest32::from_bytes([9; 32]);
        assert_ambiguous(cache.lookup(
            rollback.anchor,
            identities[0],
            Some(foreign_high_water),
            &rollback.bytes,
        ));

        let mut invalid_anchor = active.anchor;
        invalid_anchor.revision = 0;
        assert_ambiguous(cache.lookup(
            invalid_anchor,
            identities[1],
            Some(active.anchor),
            &active.bytes,
        ));
        assert_ambiguous(cache.insert(
            rollback.anchor,
            ProductBundledCatalogGenerationRole::Rollback,
            identities[0],
            active.catalog,
        ));
    }

    #[test]
    fn exact_reinsertion_is_idempotent_but_conflicting_evidence_is_rejected() {
        let (_temporary, identities) = test_identities(2);
        let generation = structural_generation(1, 1);
        let mut cache = ProductCatalogAdmissionCache::new();
        cache
            .insert(
                generation.anchor,
                ProductBundledCatalogGenerationRole::Active,
                identities[0],
                generation.catalog.clone(),
            )
            .unwrap();
        let retained_bytes = cache.retained_bytes;

        cache
            .insert(
                generation.anchor,
                ProductBundledCatalogGenerationRole::Active,
                identities[0],
                generation.catalog.clone(),
            )
            .unwrap();
        assert_eq!(cache.entry_count(), 1);
        assert_eq!(cache.retained_bytes, retained_bytes);

        assert_ambiguous(cache.insert(
            generation.anchor,
            ProductBundledCatalogGenerationRole::Rollback,
            identities[0],
            generation.catalog.clone(),
        ));
        assert_ambiguous(cache.insert(
            generation.anchor,
            ProductBundledCatalogGenerationRole::Active,
            identities[1],
            generation.catalog,
        ));
        assert_eq!(cache.entry_count(), 1);
        assert_eq!(cache.retained_bytes, retained_bytes);
    }

    #[test]
    fn one_digest_cannot_be_rebound_to_a_different_anchor() {
        let (_temporary, identities) = test_identities(1);
        let generation = structural_generation(1, 1);
        let mut rebound = generation.anchor;
        rebound.inventory_sha256 = Digest32::from_bytes([2; 32]);
        let mut cache = ProductCatalogAdmissionCache::new();
        cache
            .insert(
                generation.anchor,
                ProductBundledCatalogGenerationRole::Active,
                identities[0],
                generation.catalog.clone(),
            )
            .unwrap();

        assert_ambiguous(cache.insert(
            rebound,
            ProductBundledCatalogGenerationRole::Active,
            identities[0],
            generation.catalog,
        ));
        assert_ambiguous(cache.lookup(rebound, identities[0], Some(rebound), &generation.bytes));
        assert_eq!(cache.entry_count(), 1);
    }

    #[test]
    fn generation_bound_and_retained_byte_accounting_are_exact() {
        let (_temporary, identities) = test_identities(1);
        let generations = (1..=MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS + 1)
            .map(|revision| structural_generation(u64::try_from(revision).unwrap(), revision as u8))
            .collect::<Vec<_>>();
        let mut cache = ProductCatalogAdmissionCache::new();
        let mut expected_retained_bytes = 0;
        for generation in &generations[..MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS] {
            cache
                .insert(
                    generation.anchor,
                    ProductBundledCatalogGenerationRole::Rollback,
                    identities[0],
                    generation.catalog.clone(),
                )
                .unwrap();
            expected_retained_bytes +=
                generation.catalog.retained_bytes() + CACHE_ENTRY_ACCOUNTING_OVERHEAD;
        }
        assert_eq!(cache.entry_count(), MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS);
        assert_eq!(cache.retained_bytes, expected_retained_bytes);
        assert!(cache.retained_bytes <= MAX_PRODUCT_CATALOG_ADMISSION_CACHE_RETAINED_BYTES);

        let over_limit = &generations[MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS];
        assert_ambiguous(cache.insert(
            over_limit.anchor,
            ProductBundledCatalogGenerationRole::Rollback,
            identities[0],
            over_limit.catalog.clone(),
        ));
        assert_eq!(cache.retained_bytes, expected_retained_bytes);

        let forgotten = &generations[1];
        let authentication = cache
            .lookup(
                forgotten.anchor,
                identities[0],
                Some(generations[2].anchor),
                &forgotten.bytes,
            )
            .unwrap()
            .unwrap()
            .authentication();
        let forgotten_bytes = forgotten.catalog.retained_bytes() + CACHE_ENTRY_ACCOUNTING_OVERHEAD;
        cache
            .forget_digest(forgotten.anchor.catalog_sha256)
            .unwrap();
        assert_eq!(
            cache.entry_count(),
            MAX_PRODUCT_BUNDLED_CATALOG_GENERATIONS - 1
        );
        assert_eq!(
            cache.retained_bytes,
            expected_retained_bytes - forgotten_bytes
        );
        assert!(cache
            .lookup(
                forgotten.anchor,
                identities[0],
                Some(generations[2].anchor),
                &forgotten.bytes,
            )
            .unwrap()
            .is_none());
        assert_ambiguous(cache.lookup_authenticated(authentication, Some(generations[2].anchor)));
        cache
            .insert(
                forgotten.anchor,
                ProductBundledCatalogGenerationRole::Rollback,
                identities[0],
                forgotten.catalog.clone(),
            )
            .unwrap();
        assert_ambiguous(cache.lookup_authenticated(authentication, Some(generations[2].anchor)));
        let replacement_authentication = cache
            .lookup(
                forgotten.anchor,
                identities[0],
                Some(generations[2].anchor),
                &forgotten.bytes,
            )
            .unwrap()
            .unwrap()
            .authentication();
        cache
            .lookup_authenticated(replacement_authentication, Some(generations[2].anchor))
            .unwrap();

        let retained_after_forget = cache.retained_bytes;
        cache.forget_digest(Digest32::from_bytes([99; 32])).unwrap();
        assert_eq!(cache.retained_bytes, retained_after_forget);
    }

    #[test]
    fn forget_digest_detects_accounting_underflow_without_wrapping() {
        let (_temporary, identities) = test_identities(1);
        let generation = structural_generation(1, 1);
        let mut cache = ProductCatalogAdmissionCache::new();
        cache
            .insert(
                generation.anchor,
                ProductBundledCatalogGenerationRole::Active,
                identities[0],
                generation.catalog,
            )
            .unwrap();
        cache.retained_bytes = 0;

        assert_ambiguous(cache.forget_digest(generation.anchor.catalog_sha256));
        assert_eq!(cache.retained_bytes, 0);
    }

    #[test]
    fn clear_discards_every_entry_and_all_accounting() {
        let (_temporary, identities) = test_identities(1);
        let first = structural_generation(1, 1);
        let second = structural_generation(2, 2);
        let mut cache = ProductCatalogAdmissionCache::new();
        for generation in [&first, &second] {
            cache
                .insert(
                    generation.anchor,
                    ProductBundledCatalogGenerationRole::Rollback,
                    identities[0],
                    generation.catalog.clone(),
                )
                .unwrap();
        }
        assert_eq!(cache.entry_count(), 2);
        assert!(cache.retained_bytes > 0);

        cache.clear();
        assert!(cache.authority.is_none());
        assert!(cache.entries_are_empty());
        assert_eq!(cache.retained_bytes, 0);
        assert!(cache
            .lookup(
                first.anchor,
                identities[0],
                Some(first.anchor),
                &first.bytes,
            )
            .unwrap()
            .is_none());
    }

    #[cfg(zephium_internal_repository_e2e)]
    #[test]
    fn nonforgeable_seed_paths_and_authority_charge_clear_together() {
        let (_temporary, identities) = test_identities(2);
        let mut cache = ProductCatalogAdmissionCache::new();
        let active = cache
            .product_authority()
            .unwrap()
            .admit_catalog(crate::repository_e2e_fixture::ACTIVE_CATALOG_BYTES)
            .unwrap();
        let rollback = cache
            .product_authority()
            .unwrap()
            .admit_rollback_catalog(crate::repository_e2e_fixture::ROLLBACK_CATALOG_BYTES)
            .unwrap();
        let authority_bytes = cache.authority.as_ref().unwrap().retained_bytes();
        assert_eq!(cache.retained_bytes, authority_bytes);

        cache.seed_active(identities[0], &active).unwrap();
        cache.seed_rollback(identities[1], &rollback).unwrap();
        assert_eq!(cache.entry_count(), 2);
        assert!(cache.retained_bytes > authority_bytes);
        assert!(cache.retained_bytes <= MAX_PRODUCT_CATALOG_ADMISSION_CACHE_RETAINED_BYTES);

        cache.clear();
        assert!(cache.authority.is_none());
        assert!(cache.entries_are_empty());
        assert_eq!(cache.retained_bytes, 0);
    }

    #[cfg(not(zephium_internal_repository_e2e))]
    #[test]
    fn regular_build_reports_unprovisioned_authority_without_mutating_accounting() {
        let mut cache = ProductCatalogAdmissionCache::new();
        assert!(cache.product_authority().is_err());
        assert!(cache.authority.is_none());
        assert_eq!(cache.retained_bytes, 0);
        assert!(cache.entries_are_empty());
    }
}
