//! Path-free byte-source boundary for product-bundled extension resources.

use std::fmt;
use std::io::Read;

use thiserror::Error;
use zephium_core::extensions::{
    ExtensionAuthorityId, ExtensionManifestDigest, ExtensionPackageIdentity, ExtensionPackageKey,
    ExtensionPackagePayloadIdentity, ExtensionPackageRevision, ExtensionTreeDigest,
};
use zephium_extension_authority::{BundledCatalogGenerationAnchor, BundledCatalogInventoryDigest};
use zephium_extension_package::{
    ExtensionReleaseCatalogDigest, ExtensionReleaseCatalogRevision, ExtensionTreeIndexDigest,
    PortableRelativePath, MAX_EXTENSION_RELEASE_CATALOG_BYTES,
};

/// Exact structural identity of one product-bundled catalog byte source.
///
/// This value is deliberately copyable metadata, not catalog admission or
/// filesystem authority. Every field must still be matched against a freshly
/// admitted active or rollback catalog before materialized bytes can become
/// authoritative.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BundledReleaseCatalogSourceIdentity {
    authority: ExtensionAuthorityId,
    revision: ExtensionReleaseCatalogRevision,
    catalog_length: u64,
    catalog_digest: ExtensionReleaseCatalogDigest,
    inventory_digest: BundledCatalogInventoryDigest,
}

impl BundledReleaseCatalogSourceIdentity {
    pub(crate) const fn from_generation(
        generation: BundledCatalogGenerationAnchor,
    ) -> Option<Self> {
        if generation.catalog_length() == 0
            || generation.catalog_length() > MAX_EXTENSION_RELEASE_CATALOG_BYTES as u64
        {
            return None;
        }
        Some(Self {
            authority: generation.authority(),
            revision: generation.revision(),
            catalog_length: generation.catalog_length(),
            catalog_digest: generation.catalog_digest(),
            inventory_digest: generation.inventory_digest(),
        })
    }

    /// Returns the catalog trust-domain and epoch identity.
    pub const fn authority(self) -> ExtensionAuthorityId {
        self.authority
    }

    /// Returns the exact release-catalog revision.
    pub const fn revision(self) -> ExtensionReleaseCatalogRevision {
        self.revision
    }

    /// Returns the exact canonical catalog byte length.
    pub const fn catalog_length(self) -> u64 {
        self.catalog_length
    }

    /// Returns SHA-256 of the exact canonical catalog bytes.
    pub const fn catalog_digest(self) -> ExtensionReleaseCatalogDigest {
        self.catalog_digest
    }

    /// Returns the redundant closed package-inventory digest.
    pub const fn inventory_digest(self) -> BundledCatalogInventoryDigest {
        self.inventory_digest
    }
}

/// Exact catalog-bound identity of one product-bundled package byte source.
///
/// The repository package-row digest binds the remaining authenticated row,
/// including tree-index metadata, Chromium identity, provenance, and legal
/// artifacts. It prevents a source adapter from selecting bytes by package key
/// and revision while silently ignoring another catalog-controlled field.
/// This structural value is not package admission, a materialization receipt,
/// a durable pin, or native activation authority.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BundledReleasePackageSourceIdentity {
    catalog: BundledReleaseCatalogSourceIdentity,
    package_key: ExtensionPackageKey,
    package_revision: ExtensionPackageRevision,
    payload: ExtensionPackagePayloadIdentity,
    manifest_digest: ExtensionManifestDigest,
    tree_digest: ExtensionTreeDigest,
    package_row_sha256: [u8; 32],
}

impl BundledReleasePackageSourceIdentity {
    pub(crate) fn from_package(
        catalog: BundledReleaseCatalogSourceIdentity,
        package: &ExtensionPackageIdentity,
        package_row_sha256: [u8; 32],
    ) -> Option<Self> {
        if package.authority() != catalog.authority() {
            return None;
        }
        Some(Self {
            catalog,
            package_key: package.key(),
            package_revision: package.revision(),
            payload: package.payload(),
            manifest_digest: package.manifest_sha256(),
            tree_digest: package.tree_sha256(),
            package_row_sha256,
        })
    }

    /// Returns the exact catalog-generation source identity.
    pub const fn catalog(self) -> BundledReleaseCatalogSourceIdentity {
        self.catalog
    }

    /// Returns the package's trust-domain and epoch identity.
    pub const fn authority(self) -> ExtensionAuthorityId {
        self.catalog.authority()
    }

    /// Returns the stable package update-line key.
    pub const fn package_key(self) -> ExtensionPackageKey {
        self.package_key
    }

    /// Returns the exact package release revision.
    pub const fn package_revision(self) -> ExtensionPackageRevision {
        self.package_revision
    }

    /// Returns the exact tagged bundled-tree or acquired-ZIP identity.
    pub const fn payload(self) -> ExtensionPackagePayloadIdentity {
        self.payload
    }

    /// Returns SHA-256 of the exact manifest bytes.
    pub const fn manifest_digest(self) -> ExtensionManifestDigest {
        self.manifest_digest
    }

    /// Returns SHA-256 of the canonical materialized resource tree.
    pub const fn tree_digest(self) -> ExtensionTreeDigest {
        self.tree_digest
    }

    /// Returns SHA-256 of the complete canonical repository package row.
    pub const fn package_row_sha256(self) -> [u8; 32] {
        self.package_row_sha256
    }
}

impl fmt::Debug for BundledReleasePackageSourceIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BundledReleasePackageSourceIdentity")
            .field("catalog", &self.catalog)
            .field("package_key", &self.package_key)
            .field("package_revision", &self.package_revision)
            .field("payload", &self.payload)
            .field("manifest_digest", &self.manifest_digest)
            .field("tree_digest", &self.tree_digest)
            .field("package_row_sha256", &Sha256Debug(self.package_row_sha256))
            .finish()
    }
}

/// Closed logical role and exact expected bytes of one bundled resource.
///
/// Targets are canonical [`PortableRelativePath`] values used only as logical
/// release or package keys. They are never host filesystem paths, URLs, or
/// permission to resolve an arbitrary path. Source adapters must reject future
/// variants they do not understand with
/// [`BundledReleaseSourceError::UnsupportedResource`].
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum BundledReleaseResourceKind<'resource> {
    /// Exact canonical resource-tree index bytes.
    TreeIndex {
        /// Exact expected byte length.
        length: u64,
        /// SHA-256 of the exact canonical index bytes.
        sha256: ExtensionTreeIndexDigest,
    },
    /// One exact regular file from the canonical package tree.
    TreeFile {
        /// Canonical package-relative logical target.
        target: &'resource PortableRelativePath,
        /// Exact expected byte length.
        length: u64,
        /// SHA-256 of the exact file bytes.
        sha256: [u8; 32],
    },
    /// Exact release-level attribution and license bytes.
    LegalNotice {
        /// Canonical release-relative logical target.
        target: &'resource PortableRelativePath,
        /// Exact expected byte length.
        length: u64,
        /// SHA-256 of the exact legal-notice bytes.
        sha256: [u8; 32],
    },
}

impl<'resource> BundledReleaseResourceKind<'resource> {
    /// Returns the exact expected byte length.
    pub const fn expected_length(self) -> u64 {
        match self {
            Self::TreeIndex { length, .. }
            | Self::TreeFile { length, .. }
            | Self::LegalNotice { length, .. } => length,
        }
    }

    /// Returns SHA-256 of the exact expected bytes.
    pub const fn expected_sha256(self) -> [u8; 32] {
        match self {
            Self::TreeIndex { sha256, .. } => sha256.bytes(),
            Self::TreeFile { sha256, .. } | Self::LegalNotice { sha256, .. } => sha256,
        }
    }

    /// Returns the canonical logical target when this resource names one.
    ///
    /// Tree indexes are selected by catalog-bound package identity and exact
    /// digest, so they do not expose or require a target string.
    pub const fn target(self) -> Option<&'resource PortableRelativePath> {
        match self {
            Self::TreeIndex { .. } => None,
            Self::TreeFile { target, .. } | Self::LegalNotice { target, .. } => Some(target),
        }
    }
}

impl fmt::Debug for BundledReleaseResourceKind<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TreeIndex { length, sha256 } => formatter
                .debug_struct("TreeIndex")
                .field("length", length)
                .field("sha256", sha256)
                .finish(),
            Self::TreeFile {
                target,
                length,
                sha256,
            } => formatter
                .debug_struct("TreeFile")
                .field("target", target)
                .field("length", length)
                .field("sha256", &Sha256Debug(*sha256))
                .finish(),
            Self::LegalNotice {
                target,
                length,
                sha256,
            } => formatter
                .debug_struct("LegalNotice")
                .field("target", target)
                .field("length", length)
                .field("sha256", &Sha256Debug(*sha256))
                .finish(),
        }
    }
}

/// One catalog- and package-bound request for exact bundled release bytes.
///
/// Construction is repository-private so a byte-source implementation only
/// receives requests derived from admitted, bounded release metadata. The
/// value contains no host path and grants no authority beyond one synchronous
/// callback invocation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BundledReleaseResource<'resource> {
    package: BundledReleasePackageSourceIdentity,
    kind: BundledReleaseResourceKind<'resource>,
}

impl<'resource> BundledReleaseResource<'resource> {
    pub(crate) const fn tree_index(
        package: BundledReleasePackageSourceIdentity,
        length: u64,
        sha256: ExtensionTreeIndexDigest,
    ) -> Self {
        Self {
            package,
            kind: BundledReleaseResourceKind::TreeIndex { length, sha256 },
        }
    }

    pub(crate) const fn tree_file(
        package: BundledReleasePackageSourceIdentity,
        target: &'resource PortableRelativePath,
        length: u64,
        sha256: [u8; 32],
    ) -> Self {
        Self {
            package,
            kind: BundledReleaseResourceKind::TreeFile {
                target,
                length,
                sha256,
            },
        }
    }

    pub(crate) const fn legal_notice(
        package: BundledReleasePackageSourceIdentity,
        target: &'resource PortableRelativePath,
        length: u64,
        sha256: [u8; 32],
    ) -> Self {
        Self {
            package,
            kind: BundledReleaseResourceKind::LegalNotice {
                target,
                length,
                sha256,
            },
        }
    }

    /// Returns the complete catalog-bound package source identity.
    pub const fn package(self) -> BundledReleasePackageSourceIdentity {
        self.package
    }

    /// Returns the logical role and exact expected byte identity.
    pub const fn kind(self) -> BundledReleaseResourceKind<'resource> {
        self.kind
    }
}

/// Stable failure at the bundled release byte-source boundary.
///
/// The error is deliberately payload-free: source adapters must not leak host
/// paths, URLs, platform error strings, or other ambient resource details into
/// repository control flow or logs.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum BundledReleaseSourceError {
    /// The exact requested resource is absent from this release source.
    #[error("bundled extension release resource is missing")]
    Missing,
    /// The adapter does not understand the requested closed resource role.
    #[error("bundled extension release resource role is unsupported")]
    UnsupportedResource,
    /// Platform policy refused access to the otherwise recognized source.
    #[error("bundled extension release resource access was denied")]
    AccessDenied,
    /// The resolved source failed path, type, ownership, or boundary checks.
    #[error("bundled extension release resource is unsafe")]
    Unsafe,
    /// The same exact source identity could not be proved across the callback.
    #[error("bundled extension release resource identity became ambiguous")]
    IdentityAmbiguous,
    /// The release source or a required platform primitive is unavailable.
    #[error("bundled extension release source is unavailable")]
    Unavailable,
    /// Source I/O failed after request and boundary validation.
    #[error("bundled extension release source I/O failed")]
    Io,
}

/// Synchronous path-free provider of exact product-bundled release bytes.
///
/// This trait deliberately uses a generic callback and static dispatch. It is
/// not a trait-object plugin boundary and cannot retain the reader after the
/// call. Implementations receive a complete catalog/package/resource identity,
/// must resolve it only inside their fixed signed application-resource root,
/// and must expose at most the requested length plus one EOF-probe byte. The
/// extra byte is essential: truncating at the expected length would let a
/// longer hostile resource appear exact to the repository callback.
///
/// Implementations must establish the source object's type, exact spelling,
/// and stable identity before invoking `callback`, then revalidate the same
/// identity, observed source length, and source boundary after the callback
/// even when it returns `Err(E)`. A post-callback source failure takes
/// precedence and is returned as the outer [`BundledReleaseSourceError`];
/// `Ok(Err(E))` is returned only after source revalidation succeeds. Comparing
/// the observed bytes with the requested exact length, EOF, and SHA-256 remains
/// the repository callback's responsibility and therefore stays in the nested
/// result rather than being conflated with source failures.
///
/// Repository operations retain one shared high-level operation gate while
/// invoking source callbacks. Re-entering any extension repository or package
/// lease on the callback thread is rejected before lock acquisition. An
/// implementation must not delegate repository access to another thread or
/// block on work that can enter a repository.
pub trait BundledReleaseByteSource {
    /// Runs `callback` with a bounded reader for one exact bundled resource.
    ///
    /// The mutable receiver serializes source-local state and prevents ordinary
    /// overlapping calls. The callback must not re-enter the same source by an
    /// ambient alias, repository, or package lease.
    /// The reader and request borrows cannot escape this call.
    fn with_resource<T, E, F>(
        &mut self,
        resource: BundledReleaseResource<'_>,
        callback: F,
    ) -> Result<Result<T, E>, BundledReleaseSourceError>
    where
        F: FnOnce(&mut dyn Read) -> Result<T, E>;
}

struct Sha256Debug([u8; 32]);

impl fmt::Debug for Sha256Debug {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:02x}{:02x}{:02x}{:02x}…",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use zephium_core::extensions::{
        ExtensionManifestDigest, ExtensionPackageIdentity, ExtensionTreeDigest,
    };

    fn catalog(authority_byte: u8) -> BundledReleaseCatalogSourceIdentity {
        let generation = BundledCatalogGenerationAnchor::from_parts(
            ExtensionAuthorityId::from_bytes([authority_byte; 32]),
            ExtensionReleaseCatalogRevision::INITIAL,
            123,
            ExtensionReleaseCatalogDigest::from_bytes([2; 32]),
            BundledCatalogInventoryDigest::from_bytes([3; 32]),
        );
        BundledReleaseCatalogSourceIdentity::from_generation(generation).unwrap()
    }

    fn package(authority_byte: u8) -> BundledReleasePackageSourceIdentity {
        let catalog = catalog(authority_byte);
        let package = ExtensionPackageIdentity::new(
            ExtensionAuthorityId::from_bytes([authority_byte; 32]),
            ExtensionPackageKey::from_bytes([4; 32]),
            ExtensionPackageRevision::INITIAL,
            ExtensionPackagePayloadIdentity::BundledTree,
            ExtensionManifestDigest::from_bytes([5; 32]),
            ExtensionTreeDigest::from_bytes([6; 32]),
        );
        BundledReleasePackageSourceIdentity::from_package(catalog, &package, [7; 32]).unwrap()
    }

    #[test]
    fn catalog_identity_retains_every_generation_field() {
        let identity = catalog(1);
        assert_eq!(
            identity.authority(),
            ExtensionAuthorityId::from_bytes([1; 32])
        );
        assert_eq!(
            identity.revision(),
            ExtensionReleaseCatalogRevision::INITIAL
        );
        assert_eq!(identity.catalog_length(), 123);
        assert_eq!(identity.catalog_digest().bytes(), [2; 32]);
        assert_eq!(identity.inventory_digest().bytes(), [3; 32]);
    }

    #[test]
    fn catalog_identity_rejects_an_invalid_exact_length() {
        let generation = BundledCatalogGenerationAnchor::from_parts(
            ExtensionAuthorityId::from_bytes([1; 32]),
            ExtensionReleaseCatalogRevision::INITIAL,
            0,
            ExtensionReleaseCatalogDigest::from_bytes([2; 32]),
            BundledCatalogInventoryDigest::from_bytes([3; 32]),
        );
        assert_eq!(
            BundledReleaseCatalogSourceIdentity::from_generation(generation),
            None
        );
    }

    #[test]
    fn package_identity_rejects_cross_authority_composition() {
        let package = ExtensionPackageIdentity::new(
            ExtensionAuthorityId::from_bytes([9; 32]),
            ExtensionPackageKey::from_bytes([4; 32]),
            ExtensionPackageRevision::INITIAL,
            ExtensionPackagePayloadIdentity::BundledTree,
            ExtensionManifestDigest::from_bytes([5; 32]),
            ExtensionTreeDigest::from_bytes([6; 32]),
        );
        assert_eq!(
            BundledReleasePackageSourceIdentity::from_package(catalog(1), &package, [7; 32]),
            None
        );
    }

    #[test]
    fn package_identity_retains_every_package_projection() {
        let identity = package(1);
        assert_eq!(identity.catalog(), catalog(1));
        assert_eq!(
            identity.authority(),
            ExtensionAuthorityId::from_bytes([1; 32])
        );
        assert_eq!(
            identity.package_key(),
            ExtensionPackageKey::from_bytes([4; 32])
        );
        assert_eq!(
            identity.package_revision(),
            ExtensionPackageRevision::INITIAL
        );
        assert_eq!(
            identity.payload(),
            ExtensionPackagePayloadIdentity::BundledTree
        );
        assert_eq!(identity.manifest_digest().bytes(), [5; 32]);
        assert_eq!(identity.tree_digest().bytes(), [6; 32]);
        assert_eq!(identity.package_row_sha256(), [7; 32]);
    }

    #[test]
    fn resource_kinds_carry_only_logical_targets_and_exact_byte_identity() {
        let target = PortableRelativePath::parse("scripts/content.js").unwrap();
        let resource = BundledReleaseResource::tree_file(package(1), &target, 17, [8; 32]);
        assert_eq!(resource.package(), package(1));
        assert_eq!(resource.kind().target(), Some(&target));
        assert_eq!(resource.kind().expected_length(), 17);
        assert_eq!(resource.kind().expected_sha256(), [8; 32]);

        let index = BundledReleaseResource::tree_index(
            package(1),
            91,
            ExtensionTreeIndexDigest::from_bytes([9; 32]),
        );
        assert_eq!(index.kind().target(), None);
        assert_eq!(index.kind().expected_length(), 91);
        assert_eq!(index.kind().expected_sha256(), [9; 32]);
    }

    struct TestSource {
        bytes: &'static [u8],
        post_callback_error: Option<BundledReleaseSourceError>,
        callback_count: usize,
    }

    impl BundledReleaseByteSource for TestSource {
        fn with_resource<T, E, F>(
            &mut self,
            _resource: BundledReleaseResource<'_>,
            callback: F,
        ) -> Result<Result<T, E>, BundledReleaseSourceError>
        where
            F: FnOnce(&mut dyn Read) -> Result<T, E>,
        {
            self.callback_count += 1;
            let mut reader = Cursor::new(self.bytes);
            let result = callback(&mut reader);
            match self.post_callback_error {
                Some(error) => Err(error),
                None => Ok(result),
            }
        }
    }

    fn test_resource(target: &PortableRelativePath) -> BundledReleaseResource<'_> {
        BundledReleaseResource::tree_file(package(1), target, 4, [10; 32])
    }

    #[test]
    fn callback_validation_result_remains_nested() {
        let mut source = TestSource {
            bytes: b"data",
            post_callback_error: None,
            callback_count: 0,
        };
        let target = PortableRelativePath::parse("manifest.json").unwrap();
        let result = source
            .with_resource(test_resource(&target), |reader| {
                let mut bytes = Vec::new();
                reader.read_to_end(&mut bytes).unwrap();
                Err::<(), _>("caller validation failed")
            })
            .unwrap();
        assert_eq!(result, Err("caller validation failed"));
        assert_eq!(source.callback_count, 1);
    }

    #[test]
    fn post_callback_source_failure_takes_precedence() {
        let mut source = TestSource {
            bytes: b"data",
            post_callback_error: Some(BundledReleaseSourceError::IdentityAmbiguous),
            callback_count: 0,
        };
        let target = PortableRelativePath::parse("manifest.json").unwrap();
        let result = source.with_resource(test_resource(&target), |_reader| {
            Err::<(), _>("caller validation failed")
        });
        assert_eq!(result, Err(BundledReleaseSourceError::IdentityAmbiguous));
        assert_eq!(source.callback_count, 1);
    }

    #[test]
    fn public_boundary_values_are_copy() {
        fn require_copy<T: Copy>() {}

        require_copy::<BundledReleaseCatalogSourceIdentity>();
        require_copy::<BundledReleasePackageSourceIdentity>();
        require_copy::<BundledReleaseResourceKind<'static>>();
        require_copy::<BundledReleaseResource<'static>>();
        require_copy::<BundledReleaseSourceError>();
    }
}
