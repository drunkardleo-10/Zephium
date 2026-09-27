//! Durable custody of Beta package objects, separate from Verified catalogs.
//!
//! Directory names are structural inventory only. A package receipt requires
//! fresh source admission, original CRX authentication, deterministic transform
//! reconstruction and sealed-tree readback. This layer does not install into a
//! profile or mint native-runtime authority. Store owns install/grant/high-water
//! transactions; native ownership must be joined separately by the service.

#[path = "beta/native.rs"]
mod native;
#[path = "beta/storage.rs"]
mod storage;
pub use native::{
    BetaNativeAdmissionError, BetaNativeOwnershipAdmission, BetaNativePackagePin,
    BetaNativePlanningRefusal, BetaRuntimeBuildRefusal, BetaRuntimeHostActivation,
    BetaRuntimeHostRefusal, BetaRuntimePackageAccess, BetaRuntimeRecoveryRefusal,
    BetaRuntimeRecoveryToken,
};
pub use storage::{
    BetaStorageCollection, BetaStorageUsage, MAX_BETA_REPOSITORY_BYTES, MAX_BETA_REPOSITORY_ENTRIES,
};

use crate::operation::RepositoryRuntime;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use zephium_core::extensions::{
    ExtensionInstallProvenance, ExtensionPackageIdentity, ExtensionProvenanceSource,
    ExtensionTransformProvenance, ExtensionUpstreamCheckpoint,
};
use zephium_extension_distribution::beta::{
    BetaArtifactPreparationError, BetaCompatibilityLimitation, BetaPreparationWorkspace,
    PreparedBetaArtifact, ProductAdmittedBetaSource,
};
use zephium_extension_package::AdmittedExtensionManifest;
use zephium_private_fs::{
    LockedPrivateNamespace, PrivateChildKind, PrivateComponent, PrivateEntryName,
};

/// Hard bound covering ready, orphaned and interrupted package slots together.
pub const MAX_BETA_REPOSITORY_SLOTS: usize = 256;

/// Non-authorizing immutable object address. Policy refresh does not rename an
/// object; runtime domain, local revision, original payload and output do.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct BetaPackageObjectId([u8; 32]);

impl BetaPackageObjectId {
    /// Reconstructs a structural address from independently persisted provenance.
    /// The corresponding disk tree still requires complete authentication.
    pub fn from_provenance(provenance: &ExtensionInstallProvenance) -> Self {
        Self::for_package(
            provenance.package(),
            provenance.output_index(),
            provenance.upstream(),
            provenance.transform(),
            provenance.compatibility_digest(),
        )
    }

    /// Stable bytes suitable for a local Store binding, not install authority.
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }

    fn for_package(
        package: &ExtensionPackageIdentity,
        index: [u8; 32],
        upstream: ExtensionUpstreamCheckpoint,
        transform: &ExtensionTransformProvenance,
        compatibility: [u8; 32],
    ) -> Self {
        Self(
            zephium_core::extensions::ExtensionBetaObjectDigest::from_parts(
                package,
                index,
                upstream,
                transform,
                compatibility,
            )
            .bytes(),
        )
    }

    fn component(self) -> PrivateComponent {
        let mut name = String::from("beta-");
        for byte in self.0 {
            use std::fmt::Write;
            let _ = write!(name, "{byte:02x}");
        }
        PrivateComponent::new(name).expect("compiled object address is portable")
    }

    fn parse(name: &str) -> Option<Self> {
        let text = name.strip_prefix("beta-")?;
        if text.len() != 64
            || !text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return None;
        }
        let mut bytes = [0; 32];
        for (out, pair) in bytes.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
            *out = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
        }
        Some(Self(bytes))
    }
}

/// Stable failures never include paths, untrusted package text or native errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum BetaRepositoryError {
    /// A delegated package callback attempted to enter repository management.
    #[error("Beta repository callback reentry is forbidden")]
    CallbackReentry,
    /// Unknown, redirected, corrupted or inconsistent repository contents.
    #[error("Beta repository integrity check failed")]
    Integrity,
    /// Private filesystem operation failed; close/reopen before continuing.
    #[error("Beta repository storage is unavailable")]
    Storage,
    /// A pre-mutation filesystem refusal preserved the object; retry later.
    #[error("Beta repository storage is temporarily busy")]
    StorageUnavailable,
    /// The fixed live-plus-residue slot budget is exhausted.
    #[error("Beta repository capacity is exhausted")]
    Capacity,
    /// A requested object does not exist; no replacement is inferred.
    #[error("Beta repository package is absent")]
    NotFound,
    /// An earlier uncertain operation invalidated this owner and its receipts.
    #[error("Beta repository requires recovery")]
    Quarantined,
    /// The source or output could not establish fresh Beta admission.
    #[error("Beta package authentication failed: {0}")]
    Admission(BetaArtifactPreparationError),
}

/// Live repository-owned output. It survives destruction of the acquisition
/// scratch workspace, but closing or quarantining its repository invalidates
/// it. This is not a Verified witness, user consent, installation or native lease.
///
/// ```compile_fail
/// fn verified(_: &zephium_extension_authority::ProductAdmittedExtensionManifest) {}
/// fn beta_is_not_verified(value: &zephium_extension_repository::beta::StoredBetaPackage) {
///     verified(value);
/// }
/// ```
pub struct StoredBetaPackage {
    id: BetaPackageObjectId,
    artifact: PreparedBetaArtifact,
    persisted: Option<ExtensionInstallProvenance>,
}

impl std::fmt::Debug for StoredBetaPackage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredBetaPackage")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl StoredBetaPackage {
    /// Optional APIs explicitly removed from this authenticated local artifact.
    pub fn withheld_optional_permissions(&self) -> &[zephium_core::extensions::ApiPermissionName] {
        self.artifact.withheld_optional_permissions()
    }
    /// Optional site access removed from the local artifact's grantable surface.
    pub fn withheld_optional_hosts(&self) -> &[Box<str>] {
        self.artifact.withheld_optional_hosts()
    }
    /// Whether incoming external messaging was restricted to the same extension.
    pub fn external_messaging_withheld(&self) -> bool {
        self.artifact.external_messaging_withheld()
    }
    /// Authenticated, localized UI metadata, independent of user grants.
    pub fn resolve_metadata(
        &self,
    ) -> Result<zephium_extension_package::ResolvedExtensionManifestMetadata, BetaRepositoryError>
    {
        self.artifact
            .resolve_metadata()
            .map_err(BetaRepositoryError::Admission)
    }
    /// Non-authorizing immutable object address.
    pub const fn id(&self) -> BetaPackageObjectId {
        self.id
    }
    /// Reproves live policy, repository ownership, complete bytes and sealed modes.
    pub fn verify(&self) -> Result<(), BetaRepositoryError> {
        self.artifact
            .verify()
            .map_err(BetaRepositoryError::Admission)
    }
    /// Fresh structural manifest used to prepare consent and exact Store joins.
    pub fn manifest(&self) -> Result<&AdmittedExtensionManifest, BetaRepositoryError> {
        self.artifact
            .manifest()
            .map_err(BetaRepositoryError::Admission)
    }
    /// Exact compatibility limitations that must be bound to user consent.
    pub fn limitations(&self) -> Result<&[BetaCompatibilityLimitation], BetaRepositoryError> {
        self.artifact
            .limitations()
            .map_err(BetaRepositoryError::Admission)
    }
    /// Expected prior publisher high-water state, to be compared by Store.
    pub fn previous_upstream(
        &self,
    ) -> Result<Option<ExtensionUpstreamCheckpoint>, BetaRepositoryError> {
        self.artifact
            .previous_upstream()
            .map_err(BetaRepositoryError::Admission)
    }
    /// Constructs structural provenance, not acquisition-provider eligibility.
    /// The service must supply a separately approved provider receipt.
    pub fn provenance(
        &self,
        provider: ExtensionProvenanceSource,
    ) -> Result<ExtensionInstallProvenance, BetaRepositoryError> {
        self.artifact
            .provenance(provider)
            .map_err(BetaRepositoryError::Admission)
    }
    /// Conservative owned source/manifest/index charge for the serialized service.
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.artifact.retained_bytes()
            + self
                .persisted
                .as_ref()
                .map_or(0, ExtensionInstallProvenance::retained_bytes)
    }

    /// Exact historical provenance matched during Store-bound recovery. New
    /// admission uses `provenance` above, which always carries current policy.
    pub fn persisted_provenance(
        &self,
    ) -> Result<Option<&ExtensionInstallProvenance>, BetaRepositoryError> {
        self.verify()?;
        Ok(self.persisted.as_ref())
    }
}

/// Exclusive, bounded Beta object repository. It deliberately uses a dedicated
/// namespace rather than interpreting or extending a Verified catalog journal.
/// Slots are opened lazily, so startup cannot multiply tree recovery by the
/// number of installed packages. No object is implicitly promoted or deleted.
pub struct BetaPackageRepository {
    namespace: LockedPrivateNamespace,
    slots: BTreeMap<BetaPackageObjectId, Option<BetaPreparationWorkspace>>,
    quarantined: bool,
    runtime: RepositoryRuntime,
    reservations: Arc<
        Mutex<
            BTreeMap<
                zephium_core::extensions::ExtensionNativeOwnershipKey,
                Arc<native::Reservation>,
            >,
        >,
    >,
}

impl BetaPackageRepository {
    /// Validates structural inventory only; disk contents cannot mint a receipt.
    pub fn open(namespace: LockedPrivateNamespace) -> Result<Self, BetaRepositoryError> {
        let slots = inspect_inventory(&namespace)?
            .into_iter()
            .map(|id| (id, None))
            .collect();
        Ok(Self {
            namespace,
            slots,
            quarantined: false,
            runtime: RepositoryRuntime::new(),
            reservations: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }

    /// Returns bounded structural inventory for later Store/repository joins.
    pub fn inventory(&mut self) -> Result<Vec<BetaPackageObjectId>, BetaRepositoryError> {
        let operation_runtime = self.runtime.clone();
        let _operation = operation_runtime
            .enter()
            .map_err(native::repository_operation_error)?;
        self.check()?;
        Ok(self.slots.keys().copied().collect())
    }

    /// Transfers an exact prepared source into durable repository custody.
    /// Replay reauthenticates existing output without replacing it. Scratch
    /// teardown cannot invalidate the returned repository receipt.
    pub fn materialize(
        &mut self,
        prepared: PreparedBetaArtifact,
    ) -> Result<StoredBetaPackage, BetaRepositoryError> {
        let operation_runtime = self.runtime.clone();
        let _operation = operation_runtime
            .enter()
            .map_err(native::repository_operation_error)?;
        self.check()?;
        prepared.verify().map_err(BetaRepositoryError::Admission)?;
        let id = BetaPackageObjectId::for_package(
            prepared
                .manifest()
                .map_err(BetaRepositoryError::Admission)?
                .descriptor()
                .package(),
            prepared.index().index_sha256().bytes(),
            prepared
                .upstream()
                .map_err(BetaRepositoryError::Admission)?,
            prepared.transformation(),
            prepared
                .manifest()
                .map_err(BetaRepositoryError::Admission)?
                .descriptor()
                .compatibility_digest()
                .bytes(),
        );
        if !self.slots.contains_key(&id) && self.slots.len() >= MAX_BETA_REPOSITORY_SLOTS {
            return Err(BetaRepositoryError::Capacity);
        }
        let usage = storage::measure(self)?;
        let added_bytes = if self.slots.contains_key(&id) {
            0
        } else {
            prepared
                .storage_bytes()
                .map_err(BetaRepositoryError::Admission)?
        };
        let added_entries = if self.slots.contains_key(&id) {
            0
        } else {
            prepared.storage_entries()
        };
        if usage.bytes().saturating_add(added_bytes) > MAX_BETA_REPOSITORY_BYTES
            || usage.entries().saturating_add(added_entries) > MAX_BETA_REPOSITORY_ENTRIES
        {
            return Err(BetaRepositoryError::Capacity);
        }
        let result = (|| {
            if !self.slots.contains_key(&id) {
                let directory = self
                    .namespace
                    .directory()
                    .create_new_private_child(&id.component())
                    .map_err(|_| BetaRepositoryError::Storage)?;
                self.slots.insert(id, None);
                let workspace = BetaPreparationWorkspace::open_in(directory)
                    .map_err(BetaRepositoryError::Admission)?;
                self.slots.insert(id, Some(workspace));
            }
            let workspace = self.workspace(id)?;
            let artifact = prepared
                .materialize_into(workspace)
                .map_err(BetaRepositoryError::Admission)?;
            self.bind_fresh_artifact(id, artifact)
        })();
        self.settle(result)
    }

    /// Reauthenticates saved original CRX and deterministic output with a new
    /// live source witness. This currently requires fresh policy; it is not an
    /// offline installed-runtime admission shortcut.
    pub fn reopen(
        &mut self,
        id: BetaPackageObjectId,
        source: ProductAdmittedBetaSource,
    ) -> Result<StoredBetaPackage, BetaRepositoryError> {
        let operation_runtime = self.runtime.clone();
        let _operation = operation_runtime
            .enter()
            .map_err(native::repository_operation_error)?;
        self.check()?;
        source
            .revalidate()
            .map_err(|_| BetaRepositoryError::Admission(BetaArtifactPreparationError::Policy))?;
        if !self.slots.contains_key(&id) {
            return Err(BetaRepositoryError::NotFound);
        }
        let result = (|| {
            let artifact = self
                .workspace(id)?
                .reopen(source)
                .map_err(BetaRepositoryError::Admission)?;
            self.bind_fresh_artifact(id, artifact)
        })();
        self.settle(result)
    }

    /// Reconstructs custody directly from the saved original CRX and exact
    /// persisted Store provenance, without downloading the package again.
    /// Current signed policy and a Store high-water snapshot remain required.
    pub fn reopen_bound(
        &mut self,
        policy: std::sync::Arc<
            zephium_extension_distribution::public_policy::AcceptedExtensionPolicy,
        >,
        expected: &ExtensionInstallProvenance,
        high_water: ExtensionUpstreamCheckpoint,
        runtime: zephium_extension_distribution::beta::BetaRuntimeTarget,
    ) -> Result<StoredBetaPackage, BetaRepositoryError> {
        let operation_runtime = self.runtime.clone();
        let _operation = operation_runtime
            .enter()
            .map_err(native::repository_operation_error)?;
        self.check()?;
        policy
            .policy()
            .map_err(|_| BetaRepositoryError::Admission(BetaArtifactPreparationError::Policy))?;
        let id = BetaPackageObjectId::from_provenance(expected);
        if !self.slots.contains_key(&id) {
            return Err(BetaRepositoryError::NotFound);
        }
        let result = (|| {
            let artifact = self
                .workspace(id)?
                .reopen_bound(policy, expected, high_water, runtime)
                .map_err(BetaRepositoryError::Admission)?;
            let mut stored = self.bind_fresh_artifact(id, artifact)?;
            stored.persisted = Some(expected.clone());
            Ok(stored)
        })();
        self.settle(result)
    }

    /// Restores locally admitted external bytes and grants without a backend
    /// connection. Signed-policy objects cannot cross into this namespace.
    pub fn reopen_external_bound(
        &mut self,
        expected: &ExtensionInstallProvenance,
        high_water: ExtensionUpstreamCheckpoint,
        runtime: zephium_extension_distribution::beta::BetaRuntimeTarget,
    ) -> Result<StoredBetaPackage, BetaRepositoryError> {
        let operation_runtime = self.runtime.clone();
        let _operation = operation_runtime
            .enter()
            .map_err(native::repository_operation_error)?;
        self.check()?;
        let id = BetaPackageObjectId::from_provenance(expected);
        if !self.slots.contains_key(&id) {
            return Err(BetaRepositoryError::NotFound);
        }
        let result = (|| {
            let artifact = self
                .workspace(id)?
                .reopen_external_bound(expected, high_water, runtime)
                .map_err(BetaRepositoryError::Admission)?;
            let mut stored = self.bind_fresh_artifact(id, artifact)?;
            stored.persisted = Some(expected.clone());
            Ok(stored)
        })();
        self.settle(result)
    }

    // All callers immediately consume a fully verified result from prepare,
    // reopen, or materialize_into, with no intervening callback or mutation.
    // Bind its structural object identity without rehashing the whole tree a
    // second time. This issues no native authority: native planning, custody
    // transitions and delegated resource reads retain their own verification.
    fn bind_fresh_artifact(
        &self,
        id: BetaPackageObjectId,
        artifact: PreparedBetaArtifact,
    ) -> Result<StoredBetaPackage, BetaRepositoryError> {
        artifact
            .revalidate()
            .map_err(BetaRepositoryError::Admission)?;
        let observed = BetaPackageObjectId::for_package(
            artifact
                .manifest()
                .map_err(BetaRepositoryError::Admission)?
                .descriptor()
                .package(),
            artifact.index().index_sha256().bytes(),
            artifact
                .upstream()
                .map_err(BetaRepositoryError::Admission)?,
            artifact.transformation(),
            artifact
                .manifest()
                .map_err(BetaRepositoryError::Admission)?
                .descriptor()
                .compatibility_digest()
                .bytes(),
        );
        if observed != id {
            return Err(BetaRepositoryError::Integrity);
        }
        Ok(StoredBetaPackage {
            id,
            artifact,
            persisted: None,
        })
    }

    fn workspace(
        &mut self,
        id: BetaPackageObjectId,
    ) -> Result<&mut BetaPreparationWorkspace, BetaRepositoryError> {
        let slot = self
            .slots
            .get_mut(&id)
            .ok_or(BetaRepositoryError::NotFound)?;
        if slot.is_none() {
            let directory = self
                .namespace
                .directory()
                .open_private_child(&id.component())
                .map_err(|_| BetaRepositoryError::Storage)?;
            *slot = Some(
                BetaPreparationWorkspace::open_in(directory)
                    .map_err(BetaRepositoryError::Admission)?,
            );
        }
        slot.as_mut().ok_or(BetaRepositoryError::Integrity)
    }

    fn check(&mut self) -> Result<(), BetaRepositoryError> {
        if self.quarantined {
            return Err(BetaRepositoryError::Quarantined);
        }
        let result = inspect_inventory(&self.namespace).and_then(|current| {
            if current.iter().copied().ne(self.slots.keys().copied()) {
                Err(BetaRepositoryError::Integrity)
            } else {
                Ok(())
            }
        });
        self.settle(result)
    }

    fn settle<T>(
        &mut self,
        result: Result<T, BetaRepositoryError>,
    ) -> Result<T, BetaRepositoryError> {
        if result.is_err() {
            self.runtime.poison();
            self.quarantined = true;
            // Dropping workspace owners invalidates every outstanding receipt.
            self.slots.clear();
        }
        result
    }
}

impl Drop for BetaPackageRepository {
    fn drop(&mut self) {
        // Passive invalidation only. Durable rows and native owners still need
        // explicit reconciliation; immutable object data is not deleted.
        self.runtime.poison();
    }
}

fn inspect_inventory(
    namespace: &LockedPrivateNamespace,
) -> Result<Vec<BetaPackageObjectId>, BetaRepositoryError> {
    let names = namespace
        .directory()
        .list_components(MAX_BETA_REPOSITORY_SLOTS)
        .map_err(|_| BetaRepositoryError::Integrity)?;
    let mut ids = Vec::with_capacity(names.len());
    for name in names {
        let id = BetaPackageObjectId::parse(name.as_str()).ok_or(BetaRepositoryError::Integrity)?;
        let entry =
            PrivateEntryName::new(name.as_str()).map_err(|_| BetaRepositoryError::Integrity)?;
        if !matches!(
            namespace.directory().inspect_entry(&entry),
            Ok(Some(PrivateChildKind::Directory(_)))
        ) {
            return Err(BetaRepositoryError::Integrity);
        }
        ids.push(id);
    }
    ids.sort_unstable();
    Ok(ids)
}
