//! One-slot, fail-closed on-device preparation. No native path or activation
//! authority leaves this module. Published output stays private and sealed.

mod implicit_head;
#[cfg(test)]
mod tests;
mod tree;

use std::num::NonZeroU32;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex, MutexGuard, TryLockError,
};

use base64::Engine;
use serde::Serialize;
use sha2::{Digest, Sha256};
use zephium_core::extensions::{
    ExtensionCompatibilityLevel, ExtensionCompatibilityTargetId, ExtensionInstallProvenance,
    ExtensionPackageIdentity, ExtensionProvenanceSource, ExtensionTransformProvenance,
};
use zephium_extension_acquisition::AcquiredExtensionArchive;
use zephium_extension_package::{
    assess_upstream_extension_manifest, parse_bounded_json, AdmittedExtensionManifest,
    BoundedJsonLimits, CanonicalExtensionTreeIndex, ExpectedChromiumIdentity,
    MAX_EXTENSION_MANIFEST_BYTES, MAX_EXTENSION_TREE_INDEX_BYTES,
};
use zephium_private_fs::{
    ByteLimit, LockedPrivateNamespace, PrivateComponent, PrivateDirectory, SealedPrivateDirectory,
};

use super::{ProductAdmittedBetaSource, BETA_SOURCE_POLICY_VERSION};

const TRANSFORM_ID: &str = "beta.manifest-key.v1";
// This is a compiled algorithm descriptor, never downloaded executable data.
// Changing the algorithm requires changing its version and this descriptor.
const TRANSFORM_DESCRIPTOR: &[u8] = b"zephium:beta.manifest-key.v1\0original-crx-developer-spki;insert-only-if-key-absent;canonical-base64;recursive-sorted-json-objects;preserve-array-order;compact-utf8-no-newline;other-file-bytes-exact;existing-key-preserves-all-original-manifest-bytes";
const HISTORY_V2: &str =
    include_str!("../../../zephium-extension-package/assets/macos/webkit-history-v2.js");
const BOUNDED_STORAGE_ID: &str = "local.bounded-storage.v1";
const MANAGED_STORAGE_V2: &str =
    include_str!("../../../zephium-extension-package/assets/macos/webkit-managed-storage-v2.js");
fn bounded_storage_transform_sha256() -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"zephium:local.bounded-storage.v1\0remove-required-and-optional-unlimitedStorage;retain-native-quota;explicit-root-host-grants-for-static-script-paths;preserve-script-filters-and-other-files;canonical-json\0");
    digest.update(TRANSFORM_DESCRIPTOR);
    digest.finalize().into()
}
// Binds the identity insertion and the native adaptation as one transformation.
// Algorithm changes require a new profile revision/descriptor; formatting alone
// must not invalidate installed extensions.
fn brokered_transform_sha256(source: &ProductAdmittedBetaSource) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"zephium:local.webkit-brokered.v1\0");
    digest.update(TRANSFORM_DESCRIPTOR);
    digest.update(zephium_extension_package::macos_compatibility::compiler_sha256());
    if source.uses_history_v2() {
        digest.update(
            b"history-search-v2;word-prefix-query-and-time-before-bounds;readonly;exact-profile\0",
        );
        digest.update(HISTORY_V2.as_bytes());
    }
    if source.uses_history_v3() {
        digest.update(
            b"history-adapter-delivery-v3;replace-generated-history-bridge-with-query-v2-bytes\0",
        );
    }
    digest.finalize().into()
}

fn capability_transform_sha256() -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"zephium:local.webkit-capabilities.v1\0");
    digest.update(TRANSFORM_DESCRIPTOR);
    digest.update(zephium_extension_package::macos_compatibility::capability_compiler_sha256());
    digest.finalize().into()
}

fn capabilities_v2_transform_sha256_legacy() -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"zephium:local.webkit-capabilities.v2\0");
    digest.update(TRANSFORM_DESCRIPTOR);
    digest.update(
        zephium_extension_package::macos_compatibility::capabilities_v2_compiler_sha256_legacy(),
    );
    digest.finalize().into()
}

fn capabilities_v2_transform_sha256_disposal_only() -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"zephium:local.webkit-capabilities.v2;revision2;disposal-symbols\0");
    digest.update(TRANSFORM_DESCRIPTOR);
    digest.update(
        zephium_extension_package::macos_compatibility::capabilities_v2_compiler_sha256_disposal_only(),
    );
    digest.finalize().into()
}

fn capabilities_v2_transform_sha256() -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"zephium:local.webkit-capabilities.v2;revision3;unavailable-permission-readback\0");
    digest.update(TRANSFORM_DESCRIPTOR);
    digest.update(zephium_extension_package::macos_compatibility::capabilities_v2_compiler_sha256());
    digest.finalize().into()
}

#[cfg(feature = "capabilities-v2-qa")]
/// Exact final composed transform digests for the pinned Bitwarden 2026.9.2
/// QA revisions. The first two came from installed authenticated provenance;
/// the current one is asserted against signed-source preparation. None of
/// these digests authenticates publisher bytes on its own.
pub fn signed_bitwarden_qa_transform_refresh() -> ([u8; 32], [u8; 32], [u8; 32]) {
    (
        [
            0x83, 0x4c, 0x2a, 0xb6, 0x8e, 0x24, 0x3c, 0x08, 0x4a, 0xed, 0x77, 0x74, 0x84,
            0x16, 0xc0, 0xea, 0xdb, 0xc7, 0xc4, 0x8c, 0x43, 0x25, 0x9b, 0xa3, 0xc2, 0x38,
            0x9e, 0xa0, 0x4f, 0x8e, 0xbc, 0x05,
        ],
        [
            0x08, 0x30, 0xdc, 0xfa, 0x9c, 0xe2, 0x78, 0xe8, 0xfa, 0x0b, 0x12, 0xfc, 0x67,
            0x4d, 0x1d, 0x19, 0xce, 0x86, 0x2e, 0xaf, 0xba, 0x5a, 0xd6, 0xbe, 0xe2, 0x1f,
            0x52, 0xfe, 0x58, 0xe8, 0xfc, 0x8d,
        ],
        [
            0xa5, 0x6a, 0x60, 0xb2, 0xad, 0x35, 0xcd, 0x33, 0x3e, 0x88, 0x52, 0x93, 0x52,
            0x1a, 0xf2, 0xd3, 0xc9, 0x19, 0x6f, 0x77, 0x71, 0xf5, 0xb7, 0x23, 0x31, 0xd0,
            0x6a, 0x76, 0x28, 0xaa, 0x9a, 0x7c,
        ],
    )
}

fn adapted_transform_sha256(source: &ProductAdmittedBetaSource) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"zephium:local.webkit-adapted.v1\0module-document;classic-worker;publisher-host-or-no-process;exact-capability-policy-v1\0");
    digest.update(TRANSFORM_DESCRIPTOR);
    digest.update(zephium_extension_package::macos_compatibility::compiler_sha256());
    if source.uses_publisher_transform() {
        digest.update([1]);
        digest.update(
            super::native_hosts::policy_digest(source.upstream.publisher().bytes())
                .expect("admitted publisher binding"),
        );
    } else {
        digest.update([0]);
    }
    digest.finalize().into()
}

const MAX_EVIDENCE_BYTES: usize = 4096;
const MAX_ORIGINAL_CRX_BYTES: usize = zephium_core::extensions::MAX_EXTENSION_ARCHIVE_BYTES
    as usize
    + zephium_extension_package::MAX_CRX3_HEADER_BYTES
    + 12;

/// Conservative owned-memory ceiling, excluding the single shared policy
/// owner and temporary caller-owned CRX input.
pub const MAX_PREPARED_BETA_ARTIFACT_RETAINED_BYTES: usize = super::MAX_BETA_SOURCE_RETAINED_BYTES
    + zephium_extension_authority::MAX_PRODUCT_ADMITTED_EXTENSION_MANIFEST_RETAINED_BYTES
    + zephium_extension_package::MAX_EXTENSION_TREE_INDEX_RETAINED_BYTES
    + 2 * (MAX_EXTENSION_MANIFEST_BYTES + MAX_EXTENSION_TREE_INDEX_BYTES)
    + 32 * 1024;

/// Stable preparation failure without local paths or untrusted error text.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum BetaArtifactPreparationError {
    /// The source's signed policy is no longer eligible or current.
    #[error("Beta source policy is no longer current")]
    Policy,
    /// Original CRX, publisher, version, or complete tree differs from admission.
    #[error("Beta source bytes differ from admission")]
    Source,
    /// The compiled transformation cannot preserve the closed source contract.
    #[error("Beta output transformation was rejected")]
    Transform,
    /// A slot is already prepared or owned by a previous operation.
    #[error("Beta preparation slot is already occupied")]
    Occupied,
    /// Private storage is unavailable, corrupt, redirected, or unsettled.
    #[error("Beta artifact storage is unavailable")]
    Storage,
    /// Stored evidence, exact inventory, file bytes, or modes disagree.
    #[error("Beta prepared artifact integrity check failed")]
    Integrity,
    /// The workspace was closed, replaced, or needs reopening after failure.
    #[error("Beta prepared artifact is stale")]
    Stale,
    /// Another operation currently owns the bounded workspace operation.
    #[error("Beta artifact operation is already running")]
    Busy,
}

/// Exclusive bounded scratch workspace. Only `incoming` and `ready` belong
/// here. Opening recovers an interrupted incoming stage; it never promotes
/// residue or treats a ready directory as authenticated without revalidation.
pub struct BetaPreparationWorkspace {
    root: WorkspaceRoot,
    epoch: Arc<AtomicU64>,
    poisoned: bool,
    gate: Arc<Mutex<()>>,
    #[cfg(test)]
    stop_at: Option<TestFrontier>,
}

// A child retains its parent's exact namespace lease; there is no nested lock
// or ambient path reopening when a repository owns the workspace.
enum WorkspaceRoot {
    Namespace(LockedPrivateNamespace),
    Child(PrivateDirectory),
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TestFrontier {
    Original,
    Tree,
    Index,
    Evidence,
    Sealed,
    Published,
}

/// Exact, re-read, sealed output of a compiled transformation. This is neither
/// provider eligibility, user consent, installation, nor native lease authority.
/// It exposes no path and cannot substitute for a Verified manifest witness.
///
/// ```compile_fail
/// fn require_verified(_: &zephium_extension_authority::ProductAdmittedExtensionManifest) {}
/// fn cannot_promote(prepared: &zephium_extension_distribution::beta::PreparedBetaArtifact) {
///     require_verified(prepared);
/// }
/// ```
pub struct PreparedBetaArtifact {
    directory: SealedPrivateDirectory,
    source: ProductAdmittedBetaSource,
    blueprint: Blueprint,
    epoch: Arc<AtomicU64>,
    generation: u64,
    gate: Arc<Mutex<()>>,
}

impl std::fmt::Debug for PreparedBetaArtifact {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedBetaArtifact")
            .finish_non_exhaustive()
    }
}

impl PreparedBetaArtifact {
    /// Optional APIs removed by the compiled local adaptation, never grantable.
    pub fn withheld_optional_permissions(&self) -> &[zephium_core::extensions::ApiPermissionName] {
        &self.source.withheld.optional_api
    }
    /// Optional site access removed rather than being offered as a user grant.
    pub fn withheld_optional_hosts(&self) -> &[Box<str>] {
        &self.source.withheld.optional_hosts
    }
    /// External messaging is limited to this same extension, with no webpages.
    pub fn external_messaging_withheld(&self) -> bool {
        self.source.withheld.external_messaging
    }
    /// Exact local publisher binding rederived from authenticated source and
    /// persisted transformation evidence. User grants and native code-signature
    /// validation are still required; this never supplies Verified authority.
    pub fn publisher_native_host(
        &self,
    ) -> Option<&zephium_core::extensions::ExtensionPublisherNativeHostRequirement> {
        self.blueprint.publisher_native_host.as_ref()
    }
    /// Exact logical bytes retained on disk by this sealed artifact, including
    /// original CRX and control files. Does not count filesystem allocation units.
    pub fn storage_bytes(&self) -> Result<u64, BetaArtifactPreparationError> {
        self.current()?;
        [
            self.source.source.original_crx_length() as u64,
            self.blueprint.index.total_bytes(),
            self.blueprint.index_bytes.len() as u64,
            self.blueprint.evidence.len() as u64,
        ]
        .into_iter()
        .try_fold(0u64, |sum, bytes| {
            sum.checked_add(bytes)
                .ok_or(BetaArtifactPreparationError::Integrity)
        })
    }
    /// Artifact tree entries plus its slot, ready directory and control files.
    pub fn storage_entries(&self) -> usize {
        self.blueprint.index.total_entry_count().saturating_add(6)
    }

    /// Resolves bounded display metadata from the exact authenticated default
    /// locale resource. This reads no executable package resource.
    pub fn resolve_metadata(
        &self,
    ) -> Result<
        zephium_extension_package::ResolvedExtensionManifestMetadata,
        BetaArtifactPreparationError,
    > {
        let _operation = lock(&self.gate)?;
        self.current()?;
        let metadata = self.blueprint.manifest.metadata();
        let bytes = if let Some(resource) = metadata.locale_messages() {
            let mut directory = self
                .directory
                .open_sealed_private_child(&component("extension")?)
                .map_err(storage)?;
            let mut names = resource.path().as_str().split('/').peekable();
            let final_name = loop {
                let name = names
                    .next()
                    .ok_or(BetaArtifactPreparationError::Integrity)?;
                if names.peek().is_none() {
                    break entry(name)?;
                }
                directory = directory
                    .open_sealed_entry_child(&entry(name)?)
                    .map_err(storage)?;
            };
            Some(
                directory
                    .with_bounded_entry_regular_reader(
                        &final_name,
                        bound(resource.length() as usize)?,
                        |reader| {
                            let mut bytes = Vec::new();
                            reader.read_to_end(&mut bytes).map_err(storage)?;
                            Ok::<_, BetaArtifactPreparationError>(bytes)
                        },
                    )
                    .map_err(storage)?
                    .ok_or(BetaArtifactPreparationError::Integrity)??,
            )
        } else {
            None
        };
        let resolved = zephium_extension_package::resolve_extension_metadata_default_locale(
            metadata,
            bytes.as_deref(),
        )
        .map_err(|_| BetaArtifactPreparationError::Integrity)?;
        self.current()?;
        Ok(resolved)
    }
    /// Conservative charge for owned source, manifest, index and evidence.
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.source.retained_bytes()
            + self.blueprint.manifest_bytes.capacity()
            + self.blueprint.manifest.retained_bytes()
            + self.blueprint.index_bytes.capacity()
            + self.blueprint.index.retained_bytes()
            + self.blueprint.evidence.capacity()
            + self
                .blueprint
                .publisher_native_host
                .as_ref()
                .map_or(0, |host| host.retained_bytes())
    }
    fn current(&self) -> Result<(), BetaArtifactPreparationError> {
        if self.epoch.load(Ordering::Acquire) != self.generation {
            return Err(BetaArtifactPreparationError::Stale);
        }
        self.source
            .revalidate()
            .map_err(|_| BetaArtifactPreparationError::Policy)
    }

    /// Checks live source policy and workspace ownership without package I/O.
    /// This is a freshness fence, not a substitute for complete `verify`.
    pub fn revalidate(&self) -> Result<(), BetaArtifactPreparationError> {
        self.current()
    }

    /// Re-hashes the entire sealed output and original archive and checks the
    /// exact evidence/inventory. Consumers must do this before durable use.
    pub fn verify(&self) -> Result<(), BetaArtifactPreparationError> {
        let _operation = lock(&self.gate)?;
        self.verify_locked()
    }

    fn verify_locked(&self) -> Result<(), BetaArtifactPreparationError> {
        if self.retained_bytes() > MAX_PREPARED_BETA_ARTIFACT_RETAINED_BYTES {
            return Err(BetaArtifactPreparationError::Integrity);
        }
        self.current()?;
        verify_artifact(&self.directory, &self.source, &self.blueprint)?;
        self.current()
    }

    /// Moves verified source custody into a repository-owned workspace. It
    /// reauthenticates the saved original and recomputes the compiled output;
    /// neither serialized evidence nor a caller-provided tree becomes authority.
    /// Existing ready output is revalidated, never replaced. The returned
    /// receipt belongs to the destination owner and survives scratch teardown.
    /// Failure can leave inert destination residue and must be recovered by
    /// reopening that workspace. No install, grant or native authority is issued.
    pub fn materialize_into(
        self,
        destination: &mut BetaPreparationWorkspace,
    ) -> Result<PreparedBetaArtifact, BetaArtifactPreparationError> {
        let gate = Arc::clone(&self.gate);
        let _operation = lock(&gate)?;
        self.verify_locked()?;
        if Arc::ptr_eq(&gate, &destination.gate) {
            return Err(BetaArtifactPreparationError::Busy);
        }
        destination.available()?;
        let bytes = read_control(
            &self.directory,
            "original.crx",
            self.source.source.original_crx_length(),
        )?;
        self.current()?;
        let expected_package = self.blueprint.manifest.descriptor().package().clone();
        let expected_index = self.blueprint.index.index_sha256();
        let expected_evidence: [u8; 32] = Sha256::digest(&self.blueprint.evidence).into();
        let epoch = Arc::clone(&self.epoch);
        let generation = self.generation;
        let Self {
            source,
            blueprint,
            directory,
            ..
        } = self;
        drop(blueprint);
        drop(directory);
        let materialized = if destination.exists("ready")? {
            destination.reopen(source)?
        } else {
            destination.prepare(source, &bytes)?
        };
        if epoch.load(Ordering::Acquire) != generation {
            return Err(BetaArtifactPreparationError::Stale);
        }
        materialized.verify()?;
        if materialized.blueprint.manifest.descriptor().package() != &expected_package
            || materialized.blueprint.index.index_sha256() != expected_index
            || <[u8; 32]>::from(Sha256::digest(&materialized.blueprint.evidence))
                != expected_evidence
        {
            return Err(BetaArtifactPreparationError::Integrity);
        }
        Ok(materialized)
    }

    /// Complete explicit limitations for the consent projection. A stale
    /// policy cannot produce a new consent proposal.
    pub fn limitations(
        &self,
    ) -> Result<&[super::BetaCompatibilityLimitation], BetaArtifactPreparationError> {
        self.current()?;
        Ok(self.source.limitations())
    }

    /// Original authenticated publisher, version, CRX and ZIP identity.
    pub fn upstream(
        &self,
    ) -> Result<zephium_core::extensions::ExtensionUpstreamCheckpoint, BetaArtifactPreparationError>
    {
        self.current()?;
        Ok(self.source.upstream())
    }

    /// Expected Store high-water state used when this source was admitted.
    pub fn previous_upstream(
        &self,
    ) -> Result<
        Option<zephium_core::extensions::ExtensionUpstreamCheckpoint>,
        BetaArtifactPreparationError,
    > {
        self.current()?;
        Ok(self.source.previous_upstream())
    }

    /// Fresh structural output manifest, not filesystem or native authority.
    pub fn manifest(&self) -> Result<&AdmittedExtensionManifest, BetaArtifactPreparationError> {
        self.current()?;
        Ok(&self.blueprint.manifest)
    }

    /// Exact output tree index derived by the compiled transformation.
    pub fn index(&self) -> &CanonicalExtensionTreeIndex {
        &self.blueprint.index
    }

    /// Exact identity operation or versioned compiled transform provenance.
    pub fn transformation(&self) -> &ExtensionTransformProvenance {
        &self.blueprint.transform
    }

    /// Constructs complete structural Store provenance after revalidation.
    /// `provider` must come from the install coordinator's separately approved
    /// acquisition receipt; supplying this enum does NOT prove provider eligibility.
    /// User grants and native incarnation remain independently bound by Store.
    pub fn provenance(
        &self,
        provider: ExtensionProvenanceSource,
    ) -> Result<ExtensionInstallProvenance, BetaArtifactPreparationError> {
        let _operation = lock(&self.gate)?;
        self.verify_locked()?;
        ExtensionInstallProvenance::new(
            provider,
            self.source.upstream(),
            self.source.original_tree(),
            self.blueprint.transform.clone(),
            self.blueprint.manifest.descriptor(),
            self.blueprint.index.index_sha256().bytes(),
            self.source
                .policy_evidence()
                .map_err(|_| BetaArtifactPreparationError::Policy)?,
        )
        .ok_or(BetaArtifactPreparationError::Integrity)
    }
}

impl BetaPreparationWorkspace {
    /// Proves process-local custody without exposing a path or transferring
    /// authority. A different owner or invalidated generation cannot match.
    pub fn owns_artifact(&self, artifact: &PreparedBetaArtifact) -> bool {
        !self.poisoned
            && Arc::ptr_eq(&self.epoch, &artifact.epoch)
            && Arc::ptr_eq(&self.gate, &artifact.gate)
            && self.epoch.load(Ordering::Acquire) == artifact.generation
    }
    /// Admits a dedicated browser-owned namespace and removes only a known
    /// prepublication stage. Unsupported filesystem adapters fail closed.
    pub fn open(namespace: LockedPrivateNamespace) -> Result<Self, BetaArtifactPreparationError> {
        Self::open_root(WorkspaceRoot::Namespace(namespace))
    }

    /// Opens a dedicated child owned by a managed repository. Every operation
    /// retains the original parent namespace lock and identity boundary.
    pub fn open_in(directory: PrivateDirectory) -> Result<Self, BetaArtifactPreparationError> {
        Self::open_root(WorkspaceRoot::Child(directory))
    }

    fn open_root(root: WorkspaceRoot) -> Result<Self, BetaArtifactPreparationError> {
        let owner = Self {
            root,
            epoch: Arc::new(AtomicU64::new(0)),
            poisoned: false,
            gate: Arc::new(Mutex::new(())),
            #[cfg(test)]
            stop_at: None,
        };
        owner.inventory()?;
        if owner.exists("incoming")? {
            tree::remove_artifact(owner.root(), "incoming")?;
        }
        Ok(owner)
    }

    /// Prepares a locally admitted external package using the same bounded
    /// extractor and sealed publication protocol. No remote approval is used.
    pub fn prepare_external(
        &mut self,
        source: super::ProductAdmittedExternalSource,
        original_crx: &[u8],
    ) -> Result<PreparedBetaArtifact, BetaArtifactPreparationError> {
        self.prepare(source.inner, original_crx)
    }

    /// Reauthenticates the exact original CRX, performs the compiled identity
    /// adaptation, streams every file to private storage, verifies readback,
    /// seals the closure and publishes it with an atomic no-replace rename.
    /// Any write failure requires reopening before another mutation.
    pub fn prepare(
        &mut self,
        source: ProductAdmittedBetaSource,
        original_crx: &[u8],
    ) -> Result<PreparedBetaArtifact, BetaArtifactPreparationError> {
        let gate = Arc::clone(&self.gate);
        let _operation = lock(&gate)?;
        self.available()?;
        source
            .revalidate()
            .map_err(|_| BetaArtifactPreparationError::Policy)?;
        if self.exists("ready")? || self.exists("incoming")? {
            return Err(BetaArtifactPreparationError::Occupied);
        }
        let mut archive = authenticate(&source, original_crx)?;
        let mut blueprint = Blueprint::derive(&source, &mut archive, V2Recipe::Current)?;
        self.poisoned = true;
        let generation = self.advance()?;
        let incoming = self
            .root()
            .create_new_private_child(&component("incoming")?)
            .map_err(storage)?;
        write_control(
            &incoming,
            "original.crx",
            original_crx,
            MAX_ORIGINAL_CRX_BYTES,
        )?;
        #[cfg(test)]
        if self.stop_at == Some(TestFrontier::Original) {
            return Err(BetaArtifactPreparationError::Storage);
        }
        let output = incoming
            .create_new_private_child(&component("extension")?)
            .map_err(storage)?;
        tree::write(output, &source, &mut archive, &blueprint)?;
        #[cfg(test)]
        if self.stop_at == Some(TestFrontier::Tree) {
            return Err(BetaArtifactPreparationError::Storage);
        }
        write_control(
            &incoming,
            "tree-index.json",
            &blueprint.index_bytes,
            MAX_EXTENSION_TREE_INDEX_BYTES,
        )?;
        #[cfg(test)]
        if self.stop_at == Some(TestFrontier::Index) {
            return Err(BetaArtifactPreparationError::Storage);
        }
        write_control(
            &incoming,
            "evidence.json",
            &blueprint.evidence,
            MAX_EVIDENCE_BYTES,
        )?;
        #[cfg(test)]
        if self.stop_at == Some(TestFrontier::Evidence) {
            return Err(BetaArtifactPreparationError::Storage);
        }
        let sealed = incoming
            .seal()
            .map_err(|_| BetaArtifactPreparationError::Storage)?;
        #[cfg(test)]
        if self.stop_at == Some(TestFrontier::Sealed) {
            return Err(BetaArtifactPreparationError::Storage);
        }
        verify_artifact(&sealed, &source, &blueprint)?;
        source
            .revalidate()
            .map_err(|_| BetaArtifactPreparationError::Policy)?;
        let ready = sealed
            .publish_noreplace(self.root(), &component("ready")?)
            .map_err(|_| BetaArtifactPreparationError::Storage)?;
        #[cfg(test)]
        if self.stop_at == Some(TestFrontier::Published) {
            return Err(BetaArtifactPreparationError::Storage);
        }
        // Compiled replacement bytes are needed only during preparation.
        // The sealed index binds them for subsequent native resource reads.
        blueprint.replacements.clear();
        let artifact = PreparedBetaArtifact {
            directory: ready,
            source,
            blueprint,
            epoch: Arc::clone(&self.epoch),
            generation,
            gate: Arc::clone(&self.gate),
        };
        artifact.verify_locked()?;
        self.poisoned = false;
        Ok(artifact)
    }

    /// Reopens a ready closure using freshly admitted original source. Disk
    /// evidence is never authority: the saved CRX is authenticated again and
    /// the transformation is recomputed before the entire output is re-hashed.
    pub fn reopen(
        &self,
        source: ProductAdmittedBetaSource,
    ) -> Result<PreparedBetaArtifact, BetaArtifactPreparationError> {
        let _operation = lock(&self.gate)?;
        self.available()?;
        source
            .revalidate()
            .map_err(|_| BetaArtifactPreparationError::Policy)?;
        let directory = self
            .root()
            .open_sealed_private_child(&component("ready")?)
            .map_err(storage)?;
        let bytes = read_control(
            &directory,
            "original.crx",
            source.source.original_crx_length(),
        )?;
        let mut archive = authenticate(&source, &bytes)?;
        let mut blueprint = Blueprint::derive(&source, &mut archive, V2Recipe::Current)?;
        // Compiled replacement bytes are needed only during preparation.
        // The sealed index binds them for subsequent native resource reads.
        blueprint.replacements.clear();
        let artifact = PreparedBetaArtifact {
            directory,
            source,
            blueprint,
            epoch: Arc::clone(&self.epoch),
            generation: self.epoch.load(Ordering::Acquire),
            gate: Arc::clone(&self.gate),
        };
        artifact.verify_locked()?;
        Ok(artifact)
    }

    /// Reconstructs admission from saved original bytes and an exact Store
    /// provenance binding. No package download or surviving scratch receipt is
    /// needed. A fresh accepted policy is still required; historical metadata
    /// does not enable new admissions or grant an offline runtime exception.
    /// The returned artifact carries current policy evidence. The caller must
    /// preserve the separately matched historical Store provenance for joins.
    pub fn reopen_bound(
        &self,
        policy: Arc<crate::public_policy::AcceptedExtensionPolicy>,
        expected: &ExtensionInstallProvenance,
        high_water: zephium_core::extensions::ExtensionUpstreamCheckpoint,
        runtime: super::BetaRuntimeTarget,
    ) -> Result<PreparedBetaArtifact, BetaArtifactPreparationError> {
        self.reopen_with_policy(
            super::SourcePolicy::Signed(policy),
            expected,
            high_water,
            runtime,
        )
    }

    /// Restores an installed local external package offline from its original
    /// authenticated CRX, exact provenance and publisher high-water record.
    pub fn reopen_external_bound(
        &self,
        expected: &ExtensionInstallProvenance,
        high_water: zephium_core::extensions::ExtensionUpstreamCheckpoint,
        runtime: super::BetaRuntimeTarget,
    ) -> Result<PreparedBetaArtifact, BetaArtifactPreparationError> {
        let policy = match expected.runtime_target().as_str() {
            zephium_core::extensions::LOCAL_MACOS_HISTORY_V3_COMPATIBILITY_TARGET => {
                super::SourcePolicy::LocalHistoryV3
            }
            zephium_core::extensions::LOCAL_MACOS_CAPABILITIES_V1_COMPATIBILITY_TARGET => {
                super::SourcePolicy::Local
            }
            zephium_core::extensions::LOCAL_MACOS_CAPABILITIES_V2_COMPATIBILITY_TARGET => {
                super::SourcePolicy::LocalCapabilitiesV2Candidate
            }
            zephium_core::extensions::LOCAL_MACOS_IDENTITY_V1_COMPATIBILITY_TARGET => {
                super::SourcePolicy::Local
            }
            zephium_core::extensions::LOCAL_MACOS_MAIN_DOCUMENT_GLOBS_V1_COMPATIBILITY_TARGET => {
                super::SourcePolicy::Local
            }
            zephium_core::extensions::LOCAL_MACOS_HISTORY_V2_COMPATIBILITY_TARGET => {
                super::SourcePolicy::LocalHistoryV2
            }
            _ => super::SourcePolicy::LocalLegacyHistory,
        };
        self.reopen_with_policy(policy, expected, high_water, runtime)
    }

    fn reopen_with_policy(
        &self,
        policy: super::SourcePolicy,
        expected: &ExtensionInstallProvenance,
        high_water: zephium_core::extensions::ExtensionUpstreamCheckpoint,
        runtime: super::BetaRuntimeTarget,
    ) -> Result<PreparedBetaArtifact, BetaArtifactPreparationError> {
        let _operation = lock(&self.gate)?;
        self.available()?;
        if let super::SourcePolicy::Signed(signed) = &policy {
            signed
                .policy()
                .map_err(|_| BetaArtifactPreparationError::Policy)?;
        }
        let directory = self
            .root()
            .open_sealed_private_child(&component("ready")?)
            .map_err(storage)?;
        // This bounded length hint is not authentication. Wrong lengths fail
        // exact EOF checks; the complete evidence is recomputed below.
        let evidence = directory
            .with_bounded_entry_regular_reader(
                &entry("evidence.json")?,
                bound(MAX_EVIDENCE_BYTES)?,
                |reader| {
                    let mut bytes = Vec::new();
                    reader
                        .read_to_end(&mut bytes)
                        .map_err(|_| BetaArtifactPreparationError::Integrity)?;
                    Ok(bytes)
                },
            )
            .map_err(storage)?
            .ok_or(BetaArtifactPreparationError::Integrity)??;
        let document = parse_bounded_json(&evidence, BoundedJsonLimits::extension_manifest())
            .map_err(|_| BetaArtifactPreparationError::Integrity)?
            .into_value();
        let length = document
            .get("original_crx_length")
            .and_then(serde_json::Value::as_u64)
            .and_then(|length| usize::try_from(length).ok())
            .filter(|length| *length > 0 && *length <= MAX_ORIGINAL_CRX_BYTES)
            .ok_or(BetaArtifactPreparationError::Integrity)?;
        let original = read_control(&directory, "original.crx", length)?;
        if <[u8; 32]>::from(Sha256::digest(&original)) != expected.upstream().original_crx_sha256()
        {
            return Err(BetaArtifactPreparationError::Source);
        }
        let identity = ExpectedChromiumIdentity::from_manifest_key_digest(
            zephium_extension_package::ChromiumManifestKeyDigest::from_bytes(
                expected.upstream().publisher().bytes(),
            ),
        );
        let mut archive = AcquiredExtensionArchive::authenticate_upstream_crx3(
            &original,
            identity.extension_id(),
            None,
        )
        .map_err(|_| BetaArtifactPreparationError::Source)?;
        let mut manifest = Vec::new();
        let mut receipts = Vec::with_capacity(archive.files().len());
        for index in 0..archive.files().len() {
            let file = &archive.files()[index];
            let receipt = if file.path().as_str() == "manifest.json" {
                if file.length() > MAX_EXTENSION_MANIFEST_BYTES as u64 {
                    return Err(BetaArtifactPreparationError::Source);
                }
                archive.copy_file(index, &mut manifest)
            } else {
                archive.copy_file(index, &mut std::io::sink())
            }
            .map_err(|_| BetaArtifactPreparationError::Source)?;
            receipts.push(receipt);
        }
        let receipt = archive
            .finish_tree(receipts)
            .map_err(|_| BetaArtifactPreparationError::Source)?;
        let source = super::admit_source(
            policy,
            receipt,
            &manifest,
            runtime,
            expected.package().revision(),
            Some(high_water),
        )
        .map_err(|error| match error {
            super::BetaSourceAdmissionError::PolicyUnavailable
            | super::BetaSourceAdmissionError::TargetNotEnabled
            | super::BetaSourceAdmissionError::Revoked => BetaArtifactPreparationError::Policy,
            super::BetaSourceAdmissionError::SourceMismatch
            | super::BetaSourceAdmissionError::UpstreamRollback => {
                BetaArtifactPreparationError::Source
            }
            super::BetaSourceAdmissionError::Unsupported(_) => {
                BetaArtifactPreparationError::Transform
            }
            super::BetaSourceAdmissionError::Capacity => BetaArtifactPreparationError::Integrity,
        })?;
        if source.upstream() != expected.upstream() || source.original_tree() != expected.original()
        {
            return Err(BetaArtifactPreparationError::Source);
        }
        // The source receipt above came from this same authenticated archive
        // over immutably borrowed original bytes. Reuse it instead of repeating
        // CRX verification and ZIP preflight before deterministic compilation.
        let recipe = pinned_bitwarden_v2_reopen_recipe(expected);
        let mut blueprint = Blueprint::derive(&source, &mut archive, recipe)?;
        // Compiled replacement bytes are needed only during preparation.
        // The sealed index binds them for subsequent native resource reads.
        blueprint.replacements.clear();
        let artifact = PreparedBetaArtifact {
            directory,
            source,
            blueprint,
            epoch: Arc::clone(&self.epoch),
            generation: self.epoch.load(Ordering::Acquire),
            gate: Arc::clone(&self.gate),
        };
        artifact.verify_locked()?;
        let policy = artifact
            .source
            .policy_evidence()
            .map_err(|_| BetaArtifactPreparationError::Policy)?;
        if !expected.matches_manifest(artifact.blueprint.manifest.descriptor())
            || expected.output_index() != artifact.blueprint.index.index_sha256().bytes()
            || expected.transform() != &artifact.blueprint.transform
            || policy.revision < expected.policy().revision
            || (policy.revision == expected.policy().revision
                && policy.sha256 != expected.policy().sha256)
        {
            return Err(BetaArtifactPreparationError::Integrity);
        }
        Ok(artifact)
    }

    /// Explicitly discards an uninstalled prepared artifact, invalidating old
    /// receipts before the first mutation. This is not native uninstall: no
    /// native path or lease is issued by this workspace.
    pub fn discard(&mut self) -> Result<(), BetaArtifactPreparationError> {
        let gate = Arc::clone(&self.gate);
        let _operation = lock(&gate)?;
        self.available()?;
        self.poisoned = true;
        self.advance()?;
        if self.exists("ready")? {
            tree::remove_artifact(self.root(), "ready")?;
        }
        self.poisoned = false;
        Ok(())
    }

    fn root(&self) -> &PrivateDirectory {
        match &self.root {
            WorkspaceRoot::Namespace(namespace) => namespace.directory(),
            WorkspaceRoot::Child(directory) => directory,
        }
    }
    fn exists(&self, name: &str) -> Result<bool, BetaArtifactPreparationError> {
        Ok(self
            .root()
            .inspect_entry(&entry(name)?)
            .map_err(storage)?
            .is_some())
    }
    fn advance(&self) -> Result<u64, BetaArtifactPreparationError> {
        let next = self
            .epoch
            .load(Ordering::Acquire)
            .checked_add(1)
            .filter(|n| *n < u64::MAX)
            .ok_or(BetaArtifactPreparationError::Stale)?;
        self.epoch.store(next, Ordering::Release);
        Ok(next)
    }
    fn inventory(&self) -> Result<(), BetaArtifactPreparationError> {
        let entries = self.root().list_components(2).map_err(storage)?;
        if entries.len() > 1
            || entries
                .iter()
                .any(|name| !matches!(name.as_str(), "incoming" | "ready"))
        {
            return Err(BetaArtifactPreparationError::Integrity);
        }
        Ok(())
    }
    fn available(&self) -> Result<(), BetaArtifactPreparationError> {
        if self.poisoned {
            return Err(BetaArtifactPreparationError::Stale);
        }
        self.inventory()
    }
}

impl Drop for BetaPreparationWorkspace {
    fn drop(&mut self) {
        self.epoch.store(u64::MAX, Ordering::Release);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum V2Recipe {
    Current,
    Legacy,
    DisposalOnly,
}

/// Earlier v2 recipes may be reconstructed only while reopening their exact
/// pinned installed QA artifacts. New preparation always selects Current.
#[cfg(feature = "capabilities-v2-qa")]
fn exact_legacy_bitwarden_v2_reopen(expected: &ExtensionInstallProvenance) -> bool {
    const OUTPUT_INDEX: [u8; 32] = [
        0xed, 0xb4, 0xab, 0xee, 0x74, 0x19, 0xc1, 0x4a, 0xa1, 0xac, 0x0f, 0x23, 0x0e, 0xdb,
        0x9d, 0xc9, 0xd8, 0xe9, 0x0e, 0x40, 0x19, 0x84, 0xc5, 0x04, 0xa9, 0x23, 0xe1, 0x19,
        0xf5, 0xa7, 0x0f, 0x3e,
    ];
    const OUTPUT_TREE: [u8; 32] = [
        0xef, 0x3b, 0xfd, 0x62, 0xfd, 0x09, 0xff, 0x29, 0x18, 0xd1, 0x86, 0x41, 0xba, 0x5b,
        0x20, 0xd6, 0x55, 0xa0, 0xea, 0x9d, 0x55, 0x74, 0x1c, 0xbd, 0x3b, 0x9d, 0x02, 0x7d,
        0x7f, 0x6f, 0x37, 0x34,
    ];
    const OUTPUT_MANIFEST: [u8; 32] = [
        0x97, 0xcf, 0xeb, 0xc0, 0x40, 0x77, 0x69, 0xc7, 0x67, 0x9d, 0x96, 0x14, 0x97, 0x01,
        0x83, 0xae, 0x8c, 0x71, 0x29, 0x52, 0x45, 0x58, 0xae, 0xc9, 0x6b, 0x42, 0x7c, 0x63,
        0x12, 0x92, 0xc3, 0xf0,
    ];
    expected.source() == &ExtensionProvenanceSource::ChromeWebStore
        && expected.runtime_target().as_str()
            == zephium_core::extensions::LOCAL_MACOS_CAPABILITIES_V2_COMPATIBILITY_TARGET
        && expected.upstream().original_crx_sha256() == super::SIGNED_BITWARDEN_QA_CRX_SHA256
        && expected.package().revision() == zephium_core::extensions::ExtensionPackageRevision::INITIAL
        && expected.output_index() == OUTPUT_INDEX
        && expected.package().tree_sha256().bytes() == OUTPUT_TREE
        && expected.package().manifest_sha256().bytes() == OUTPUT_MANIFEST
        && matches!(
            expected.transform(),
            ExtensionTransformProvenance::Compiled { target, revision, sha256 }
                if target.as_str() == "local.webkit-capabilities.v2"
                    && revision.get() == 4
                    && *sha256 == signed_bitwarden_qa_transform_refresh().0
        )
}

#[cfg(feature = "capabilities-v2-qa")]
fn exact_disposal_only_bitwarden_v2_reopen(expected: &ExtensionInstallProvenance) -> bool {
    const OUTPUT_INDEX: [u8; 32] = [
        0x75, 0x72, 0x39, 0xba, 0x97, 0xc0, 0xd5, 0x5e, 0x57, 0xda, 0xd9, 0xd8, 0xfe, 0x40,
        0xaf, 0xda, 0xeb, 0xd3, 0x84, 0x80, 0x58, 0x44, 0x48, 0xc4, 0xb4, 0x12, 0xa6, 0xd8,
        0xc5, 0xa7, 0x62, 0x28,
    ];
    const OUTPUT_TREE: [u8; 32] = [
        0x0f, 0x6c, 0xde, 0x83, 0xb3, 0x10, 0x68, 0x82, 0xf5, 0x94, 0xa3, 0x94, 0xf8, 0xc6,
        0x9d, 0x8b, 0x8a, 0xd8, 0x66, 0xee, 0x20, 0xd6, 0x90, 0x39, 0xfa, 0xfc, 0xd1, 0x97,
        0xba, 0x18, 0xd2, 0xeb,
    ];
    const OUTPUT_MANIFEST: [u8; 32] = [
        0x93, 0x7d, 0x91, 0x06, 0x85, 0x75, 0xa9, 0x31, 0x2f, 0x54, 0xcd, 0x8e, 0xc2, 0x3a,
        0x88, 0x96, 0xf2, 0xdc, 0xa4, 0x9e, 0x3b, 0x2b, 0x95, 0x9c, 0xc7, 0x66, 0x7a, 0x41,
        0x2a, 0xb6, 0xdf, 0x65,
    ];
    expected.source() == &ExtensionProvenanceSource::ChromeWebStore
        && expected.runtime_target().as_str()
            == zephium_core::extensions::LOCAL_MACOS_CAPABILITIES_V2_COMPATIBILITY_TARGET
        && expected.upstream().original_crx_sha256() == super::SIGNED_BITWARDEN_QA_CRX_SHA256
        && expected.package().revision().get() == 2
        && expected.output_index() == OUTPUT_INDEX
        && expected.package().tree_sha256().bytes() == OUTPUT_TREE
        && expected.package().manifest_sha256().bytes() == OUTPUT_MANIFEST
        && matches!(
            expected.transform(),
            ExtensionTransformProvenance::Compiled { target, revision, sha256 }
                if target.as_str() == "local.webkit-capabilities.v2"
                    && revision.get() == 5
                    && *sha256 == signed_bitwarden_qa_transform_refresh().1
        )
}

#[cfg(feature = "capabilities-v2-qa")]
fn pinned_bitwarden_v2_reopen_recipe(expected: &ExtensionInstallProvenance) -> V2Recipe {
    if exact_legacy_bitwarden_v2_reopen(expected) {
        V2Recipe::Legacy
    } else if exact_disposal_only_bitwarden_v2_reopen(expected) {
        V2Recipe::DisposalOnly
    } else {
        V2Recipe::Current
    }
}

#[cfg(not(feature = "capabilities-v2-qa"))]
fn pinned_bitwarden_v2_reopen_recipe(_expected: &ExtensionInstallProvenance) -> V2Recipe {
    V2Recipe::Current
}

struct Blueprint {
    replacements: std::collections::BTreeMap<String, Vec<u8>>,
    manifest_bytes: Vec<u8>,
    manifest: AdmittedExtensionManifest,
    index_bytes: Vec<u8>,
    index: CanonicalExtensionTreeIndex,
    transform: ExtensionTransformProvenance,
    evidence: Vec<u8>,
    publisher_native_host:
        Option<zephium_core::extensions::ExtensionPublisherNativeHostRequirement>,
}

impl Blueprint {
    fn derive(
        source: &ProductAdmittedBetaSource,
        archive: &mut AcquiredExtensionArchive<'_>,
        recipe: V2Recipe,
    ) -> Result<Self, BetaArtifactPreparationError> {
        if recipe != V2Recipe::Current && !source.uses_capabilities_v2_transform() {
            return Err(BetaArtifactPreparationError::Transform);
        }
        let manifest_slot = archive
            .files()
            .iter()
            .position(|file| file.path().as_str() == "manifest.json")
            .ok_or(BetaArtifactPreparationError::Source)?;
        let mut original_manifest =
            Vec::with_capacity(archive.files()[manifest_slot].length() as usize);
        let _manifest_receipt = archive
            .copy_file(manifest_slot, &mut original_manifest)
            .map_err(|_| BetaArtifactPreparationError::Source)?;
        if source
            .source
            .upstream_checkpoint(&original_manifest)
            .map_err(|_| BetaArtifactPreparationError::Source)?
            != source.upstream()
        {
            return Err(BetaArtifactPreparationError::Source);
        }
        let (manifest_bytes, transform) = if source.requires_manifest_key_transform() {
            let mut document =
                parse_bounded_json(&original_manifest, BoundedJsonLimits::extension_manifest())
                    .map_err(|_| BetaArtifactPreparationError::Transform)?
                    .into_value();
            let root = document
                .as_object_mut()
                .ok_or(BetaArtifactPreparationError::Transform)?;
            if root.contains_key("key") {
                return Err(BetaArtifactPreparationError::Source);
            }
            root.insert(
                "key".into(),
                serde_json::Value::String(
                    base64::engine::general_purpose::STANDARD
                        .encode(archive.developer_public_key()),
                ),
            );
            document.sort_all_objects();
            let bytes = serde_json::to_vec(&document)
                .map_err(|_| BetaArtifactPreparationError::Transform)?;
            if bytes.len() > MAX_EXTENSION_MANIFEST_BYTES {
                return Err(BetaArtifactPreparationError::Transform);
            }
            (
                bytes,
                ExtensionTransformProvenance::Compiled {
                    target: ExtensionCompatibilityTargetId::parse_exact(TRANSFORM_ID)
                        .map_err(|_| BetaArtifactPreparationError::Transform)?,
                    revision: NonZeroU32::new(1).ok_or(BetaArtifactPreparationError::Transform)?,
                    sha256: Sha256::digest(TRANSFORM_DESCRIPTOR).into(),
                },
            )
        } else {
            (original_manifest, ExtensionTransformProvenance::Identity)
        };
        let manifest_bytes = if source.withheld.is_empty() {
            manifest_bytes
        } else {
            let mut root =
                parse_bounded_json(&manifest_bytes, BoundedJsonLimits::extension_manifest())
                    .map_err(|_| BetaArtifactPreparationError::Transform)?
                    .into_value();
            if let Some(permissions) = root
                .get_mut("optional_permissions")
                .and_then(serde_json::Value::as_array_mut)
            {
                permissions.retain(|value| {
                    !source
                        .withheld
                        .optional_api
                        .iter()
                        .any(|name| value.as_str() == Some(name.as_str()))
                });
            }
            if source.withheld.external_messaging {
                root["externally_connectable"] = serde_json::json!({
                    "ids":[source.source.extension_id().as_str()], "matches":[],
                });
            }
            if source
                .withheld
                .optional_api
                .iter()
                .any(|name| name.as_str() == "sidePanel")
            {
                // A panel controlled by a withheld optional API must not
                // survive as a separately runnable manifest surface.
                root.as_object_mut()
                    .ok_or(BetaArtifactPreparationError::Transform)?
                    .remove("side_panel");
            }
            if let Some(hosts) = root
                .get_mut("optional_host_permissions")
                .and_then(serde_json::Value::as_array_mut)
            {
                hosts.retain(|value| {
                    !source
                        .withheld
                        .optional_hosts
                        .iter()
                        .any(|host| value.as_str() == Some(host.as_ref()))
                });
            }
            root.sort_all_objects();
            serde_json::to_vec(&root).map_err(|_| BetaArtifactPreparationError::Transform)?
        };
        let cross_browser_background = source
            .manifest
            .descriptor()
            .declarations()
            .background()
            .is_some_and(|background| {
                background.environment()
                    == zephium_core::extensions::ExtensionBackgroundEnvironment::CrossBrowser
            });
        let manifest_bytes = if cross_browser_background {
            let mut root =
                parse_bounded_json(&manifest_bytes, BoundedJsonLimits::extension_manifest())
                    .map_err(|_| BetaArtifactPreparationError::Transform)?
                    .into_value();
            let background = root
                .get_mut("background")
                .and_then(serde_json::Value::as_object_mut)
                .ok_or(BetaArtifactPreparationError::Transform)?;
            // The structural parser already proves an exact, single duplicate
            // entrypoint. Select the worker input to the ordinary compiler;
            // macOS module adaptation then chooses its document environment.
            let scripts = background
                .remove("scripts")
                .ok_or(BetaArtifactPreparationError::Transform)?;
            if scripts
                != serde_json::json!([background
                    .get("service_worker")
                    .ok_or(BetaArtifactPreparationError::Transform)?])
                || background.contains_key("preferred_environment")
            {
                return Err(BetaArtifactPreparationError::Transform);
            }
            root.sort_all_objects();
            serde_json::to_vec(&root).map_err(|_| BetaArtifactPreparationError::Transform)?
        } else {
            manifest_bytes
        };
        let (manifest_bytes, transform) = if source.uses_bounded_storage_transform() {
            let mut root =
                parse_bounded_json(&manifest_bytes, BoundedJsonLimits::extension_manifest())
                    .map_err(|_| BetaArtifactPreparationError::Transform)?
                    .into_value();
            for field in ["permissions", "optional_permissions"] {
                if let Some(permissions) = root
                    .get_mut(field)
                    .and_then(serde_json::Value::as_array_mut)
                {
                    permissions.retain(|name| name.as_str() != Some("unlimitedStorage"));
                }
            }
            let mut hosts: std::collections::BTreeSet<String> = root
                .get("host_permissions")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect();
            let original_hosts = hosts.len();
            for pattern in root
                .get("content_scripts")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .flat_map(|script| {
                    script
                        .get("matches")
                        .and_then(serde_json::Value::as_array)
                        .into_iter()
                        .flatten()
                })
                .filter_map(serde_json::Value::as_str)
            {
                if let Some((scheme, rest)) = pattern.split_once("://") {
                    if matches!(scheme, "http" | "https" | "*") {
                        if let Some((host, path)) = rest.split_once('/') {
                            if path != "*" {
                                hosts.insert(format!("{scheme}://{host}/*"));
                            }
                        }
                    }
                }
            }
            if hosts.len() != original_hosts {
                root["host_permissions"] = serde_json::json!(hosts);
            }
            root.sort_all_objects();
            (
                serde_json::to_vec(&root).map_err(|_| BetaArtifactPreparationError::Transform)?,
                ExtensionTransformProvenance::Compiled {
                    target: ExtensionCompatibilityTargetId::parse_exact(BOUNDED_STORAGE_ID)
                        .map_err(|_| BetaArtifactPreparationError::Transform)?,
                    revision: NonZeroU32::MIN,
                    sha256: bounded_storage_transform_sha256(),
                },
            )
        } else {
            (manifest_bytes, transform)
        };
        let mut replacements = std::collections::BTreeMap::new();
        let managed_schema = source
            .manifest
            .descriptor()
            .declarations()
            .additional()
            .managed_storage_schema_resource()
            .is_some();
        let mut normalized_pages = std::collections::BTreeSet::new();
        let (manifest_bytes, transform) = if source.uses_brokered_transform()
            || source.uses_capability_transform()
            || source.uses_capabilities_v2_transform()
            || source.uses_identity_transform()
            || source.uses_main_document_globs_transform()
            || source.uses_adapted_transform()
        {
            use zephium_extension_package::macos_compatibility as compiler;
            compiler::reject_reserved_paths(source.source.index())
                .map_err(|_| BetaArtifactPreparationError::Transform)?;
            let mut read_bytes = 0usize;
            let plan = compiler::build_plan(
                &mut |file| {
                    read_bytes = read_bytes
                        .checked_add(file.length() as usize)
                        .ok_or("transform resource budget")?;
                    if read_bytes > 16 * 1024 * 1024 {
                        return Err("transform resource budget".into());
                    }
                    let slot = archive
                        .files()
                        .iter()
                        .position(|entry| entry.path() == file.path())
                        .ok_or("missing source resource")?;
                    let mut bytes = Vec::with_capacity(file.length() as usize);
                    let _read_receipt = archive
                        .copy_file(slot, &mut bytes)
                        .map_err(|_| "invalid source resource")?;
                    if bytes.len() as u64 != file.length()
                        || <[u8; 32]>::from(Sha256::digest(&bytes)) != file.sha256()
                    {
                        return Err("source resource mismatch".into());
                    }
                    if compiler::is_html_path(file.path().as_str()) {
                        let (bytes, changed) = implicit_head::normalize(bytes)?;
                        if changed {
                            normalized_pages.insert(file.path().as_str().to_owned());
                        }
                        Ok(bytes)
                    } else {
                        Ok(bytes)
                    }
                },
                source.source.index(),
                &manifest_bytes,
                if recipe == V2Recipe::Legacy {
                    compiler::ArtifactTarget::NativeCapabilitiesV2Legacy
                } else if recipe == V2Recipe::DisposalOnly {
                    compiler::ArtifactTarget::NativeCapabilitiesV2DisposalOnly
                } else if source.uses_capabilities_v2_transform() {
                    compiler::ArtifactTarget::NativeCapabilitiesV2
                } else if source.uses_capability_transform() {
                    compiler::ArtifactTarget::NativeCapabilityBrokerV1
                } else if source.uses_main_document_globs_transform() {
                    compiler::ArtifactTarget::NativeMainDocumentGlobsV1
                } else if source.uses_identity_transform() {
                    compiler::ArtifactTarget::NativeIdentityV1
                } else if source.uses_brokered_transform() {
                    compiler::ArtifactTarget::NativeBrokeredV1
                } else if source.uses_publisher_transform() {
                    compiler::ArtifactTarget::NativePublisherV1
                } else {
                    compiler::ArtifactTarget::NativeV3
                },
                if source.uses_adapted_transform()
                    && source
                        .manifest
                        .descriptor()
                        .declarations()
                        .background()
                        .is_some_and(|background| {
                            background.worker_type()
                                == zephium_core::extensions::ExtensionBackgroundWorkerType::Module
                        })
                {
                    compiler::BackgroundEnvironment::Document
                } else {
                    compiler::BackgroundEnvironment::ServiceWorker
                },
            )
            .map_err(|_| BetaArtifactPreparationError::Transform)?;
            compiler::enforce_output_budgets(source.source.index(), &plan)
                .map_err(|_| BetaArtifactPreparationError::Transform)?;
            replacements = plan.into_files();
            // V2 historically hashed the new adapter but retained V1 output.
            // Keep those sealed artifacts reconstructible; only V3 changes bytes.
            if source.uses_history_v3() {
                let bridge = replacements
                    .get_mut(compiler::HISTORY_BRIDGE)
                    .ok_or(BetaArtifactPreparationError::Transform)?;
                *bridge = HISTORY_V2.as_bytes().to_vec();
            }
            let mut manifest = replacements
                .remove("manifest.json")
                .ok_or(BetaArtifactPreparationError::Transform)?;
            if managed_schema {
                validate_managed_schema(archive, &manifest)?;
                replacements.insert(
                    compiler::MANAGED_STORAGE_BRIDGE.to_owned(),
                    MANAGED_STORAGE_V2.as_bytes().to_vec(),
                );
                let mut document =
                    parse_bounded_json(&manifest, BoundedJsonLimits::extension_manifest())
                        .map_err(|_| BetaArtifactPreparationError::Transform)?
                        .into_value();
                if let Some(scripts) = document
                    .get_mut("content_scripts")
                    .and_then(serde_json::Value::as_array_mut)
                {
                    for script in scripts {
                        if script.get("world").and_then(serde_json::Value::as_str) == Some("MAIN") {
                            continue;
                        }
                        if let Some(js) = script
                            .get_mut("js")
                            .and_then(serde_json::Value::as_array_mut)
                        {
                            js.insert(
                                0,
                                serde_json::Value::String(
                                    compiler::MANAGED_STORAGE_BRIDGE.to_owned(),
                                ),
                            );
                        }
                    }
                }
                document.sort_all_objects();
                manifest = serde_json::to_vec(&document)
                    .map_err(|_| BetaArtifactPreparationError::Transform)?;
            }
            (
                manifest,
                ExtensionTransformProvenance::Compiled {
                    target: ExtensionCompatibilityTargetId::parse_exact(
                        if source.uses_capabilities_v2_transform() {
                            "local.webkit-capabilities.v2"
                        } else if source.uses_capability_transform() {
                            "local.webkit-capabilities.v1"
                        } else if source.uses_main_document_globs_transform() {
                            "local.webkit-main-document-globs.v1"
                        } else if source.uses_identity_transform() {
                            "local.webkit-identity.v1"
                        } else if source.uses_history_v3() {
                            "local.webkit-history.v3"
                        } else if source.uses_history_v2() {
                            "local.webkit-history.v2"
                        } else if source.uses_brokered_transform() {
                            "local.webkit-brokered.v1"
                        } else {
                            "local.webkit-adapted.v1"
                        },
                    )
                    .map_err(|_| BetaArtifactPreparationError::Transform)?,
                    revision: if source.uses_capabilities_v2_transform() {
                        NonZeroU32::new(match recipe {
                            V2Recipe::Legacy => 1,
                            V2Recipe::DisposalOnly => 2,
                            V2Recipe::Current => 3,
                        })
                        .ok_or(BetaArtifactPreparationError::Transform)?
                    } else {
                        NonZeroU32::MIN
                    },
                    sha256: if recipe == V2Recipe::Legacy {
                        capabilities_v2_transform_sha256_legacy()
                    } else if recipe == V2Recipe::DisposalOnly {
                        capabilities_v2_transform_sha256_disposal_only()
                    } else if source.uses_capabilities_v2_transform() {
                        capabilities_v2_transform_sha256()
                    } else if source.uses_capability_transform() {
                        capability_transform_sha256()
                    } else if source.uses_main_document_globs_transform() {
                        compiler::main_document_globs_compiler_sha256()
                    } else if source.uses_identity_transform() {
                        compiler::identity_compiler_sha256()
                    } else if source.uses_brokered_transform() {
                        brokered_transform_sha256(source)
                    } else {
                        adapted_transform_sha256(source)
                    },
                },
            )
        } else {
            (manifest_bytes, transform)
        };
        let combined_storage = source.uses_bounded_storage_transform()
            && (source.uses_brokered_transform()
                || source.uses_capability_transform()
                || source.uses_capabilities_v2_transform()
                || source.uses_identity_transform()
                || source.uses_main_document_globs_transform()
                || source.uses_adapted_transform());
        let transform = if combined_storage {
            compose_storage_transform(transform)?
        } else {
            transform
        };
        let transform = if managed_schema {
            compose_managed_storage_transform(transform)?
        } else {
            transform
        };
        let transform = if cross_browser_background {
            compose_cross_browser_transform(transform)?
        } else {
            transform
        };
        let transform = if normalized_pages.is_empty() {
            transform
        } else {
            compose_implicit_head_transform(transform, &normalized_pages)?
        };
        let transform = compose_withheld_transform(transform, &source.withheld)?;
        #[derive(Serialize)]
        struct Row {
            path: String,
            length: u64,
            sha256: String,
        }
        #[derive(Serialize)]
        struct Index {
            schema_version: u32,
            files: Vec<Row>,
        }
        let mut rows = std::collections::BTreeMap::new();
        for file in source.source.index().files() {
            rows.insert(
                file.path().as_str().to_owned(),
                Row {
                    path: file.path().as_str().to_owned(),
                    length: file.length(),
                    sha256: super::hex(file.sha256()),
                },
            );
        }
        for (path, bytes) in std::iter::once(("manifest.json", manifest_bytes.as_slice())).chain(
            replacements
                .iter()
                .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
        ) {
            rows.insert(
                path.to_owned(),
                Row {
                    path: path.to_owned(),
                    length: bytes.len() as u64,
                    sha256: super::hex(Sha256::digest(bytes).into()),
                },
            );
        }
        let files = rows.into_values().collect();
        let index_bytes = serde_json::to_vec(&Index {
            schema_version: 1,
            files,
        })
        .map_err(|_| BetaArtifactPreparationError::Transform)?;
        let index = CanonicalExtensionTreeIndex::parse_canonical(&index_bytes)
            .map_err(|_| BetaArtifactPreparationError::Transform)?;
        let original_package = source
            .manifest()
            .map_err(|_| BetaArtifactPreparationError::Policy)?
            .descriptor()
            .package();
        let package = ExtensionPackageIdentity::new(
            original_package.authority(),
            original_package.key(),
            original_package.revision(),
            original_package.payload(),
            index.manifest_sha256(),
            index.tree_sha256(),
        );
        let manifest = assess_upstream_extension_manifest(
            &package,
            &index,
            &ExpectedChromiumIdentity::from_manifest_key_digest(archive.developer_key_sha256()),
            &manifest_bytes,
            &source.rules().for_output(),
        )
        .map_err(|_| BetaArtifactPreparationError::Transform)?;
        if manifest.chromium_key().is_none()
            || (!(source.uses_brokered_transform()
                || source.uses_capability_transform()
                || source.uses_capabilities_v2_transform()
                || source.uses_identity_transform()
                || source.uses_main_document_globs_transform()
                || source.uses_adapted_transform()
                || source.uses_bounded_storage_transform()
                || cross_browser_background
                || !source.withheld.is_empty())
                && (manifest.descriptor().declarations()
                    != source.manifest.descriptor().declarations()
                    || manifest.descriptor().compatibility()
                        != source.manifest.descriptor().compatibility()))
            || manifest.descriptor().compatibility().iter().any(|row| {
                matches!(
                    row.level(),
                    ExtensionCompatibilityLevel::Unsupported
                        | ExtensionCompatibilityLevel::Unassessed
                )
            })
        {
            return Err(BetaArtifactPreparationError::Transform);
        }
        let mut evidence_document = serde_json::json!({
            "schema_version":1, "source_policy_version":BETA_SOURCE_POLICY_VERSION,
            "authority":super::hex(package.authority().bytes()), "publisher":super::hex(source.upstream().publisher().bytes()),
            "revision":package.revision().get(), "original_crx":super::hex(source.upstream().original_crx_sha256()),
            "original_crx_length":source.source.original_crx_length(),
            "original_zip":super::hex(source.upstream().archive_sha256()), "upstream_version":source.upstream().version().to_string(),
            "original_tree":super::hex(source.original_tree().tree), "original_index":super::hex(source.original_tree().index),
            "original_manifest":super::hex(source.original_tree().manifest),
            "transform":match &transform { ExtensionTransformProvenance::Identity => "identity.v1", ExtensionTransformProvenance::Compiled { target, .. } => target.as_str() },
            "transform_sha256":super::hex(if recipe == V2Recipe::Legacy { capabilities_v2_transform_sha256_legacy() } else if recipe == V2Recipe::DisposalOnly { capabilities_v2_transform_sha256_disposal_only() } else if source.uses_capabilities_v2_transform() { capabilities_v2_transform_sha256() } else if source.uses_capability_transform() { capability_transform_sha256() } else if source.uses_main_document_globs_transform() { zephium_extension_package::macos_compatibility::main_document_globs_compiler_sha256() } else if source.uses_identity_transform() { zephium_extension_package::macos_compatibility::identity_compiler_sha256() } else if source.uses_brokered_transform() { brokered_transform_sha256(source) } else if source.uses_adapted_transform() { adapted_transform_sha256(source) } else if source.uses_bounded_storage_transform() { bounded_storage_transform_sha256() } else { Sha256::digest(TRANSFORM_DESCRIPTOR).into() }),
            "runtime_target":manifest.descriptor().compatibility_target().as_str(), "output_tree":super::hex(index.tree_sha256().bytes()),
            "output_index":super::hex(index.index_sha256().bytes()), "output_manifest":super::hex(index.manifest_sha256().bytes()),
            "compatibility":super::hex(manifest.descriptor().compatibility_digest().bytes())
        });
        if !source.withheld.is_empty() {
            evidence_document["withheld_features"] = source.withheld.evidence();
            if let ExtensionTransformProvenance::Compiled { sha256, .. } = &transform {
                evidence_document["transform_sha256"] = serde_json::json!(super::hex(*sha256));
            }
            evidence_document["source_compatibility"] = serde_json::json!(super::hex(
                source.manifest.descriptor().compatibility_digest().bytes()
            ));
        }
        if cross_browser_background {
            evidence_document["background_selection"] =
                serde_json::json!("same-entrypoint-cross-browser-v1");
            evidence_document["source_compatibility"] = serde_json::json!(super::hex(
                source.manifest.descriptor().compatibility_digest().bytes()
            ));
            if let ExtensionTransformProvenance::Compiled { sha256, .. } = &transform {
                evidence_document["transform_sha256"] = serde_json::json!(super::hex(*sha256));
            }
        }
        if !normalized_pages.is_empty() {
            evidence_document["implicit_head_pages"] = serde_json::json!(normalized_pages);
            if let ExtensionTransformProvenance::Compiled { sha256, .. } = &transform {
                evidence_document["transform_sha256"] = serde_json::json!(super::hex(*sha256));
            }
        }
        if source.uses_bounded_storage_transform() {
            evidence_document["source_compatibility"] = serde_json::json!(super::hex(
                source.manifest.descriptor().compatibility_digest().bytes()
            ));
            evidence_document["bounded_storage"] = serde_json::json!("native-quota");
            if combined_storage {
                if let ExtensionTransformProvenance::Compiled { sha256, .. } = &transform {
                    evidence_document["transform_sha256"] = serde_json::json!(super::hex(*sha256));
                }
            }
        }
        if managed_schema {
            evidence_document["managed_storage"] =
                serde_json::json!("no-administrator-policies-v2");
            if let ExtensionTransformProvenance::Compiled { sha256, .. } = &transform {
                evidence_document["transform_sha256"] = serde_json::json!(super::hex(*sha256));
            }
        }
        let publisher_native_host = if source.uses_publisher_transform() {
            let host = super::native_hosts::requirement(&package)
                .ok_or(BetaArtifactPreparationError::Transform)?;
            evidence_document["publisher_native_host"] = serde_json::json!({
                "extension_id":host.upstream_chromium_extension_id(),
                "host":host.host_name(),
                "team":host.macos_publisher().team_identifier(),
                "signing_id":host.macos_publisher().signing_identifier(),
            });
            Some(host)
        } else {
            None
        };
        evidence_document.sort_all_objects();
        let evidence = serde_json::to_vec(&evidence_document)
            .map_err(|_| BetaArtifactPreparationError::Transform)?;
        if evidence.len() > MAX_EVIDENCE_BYTES {
            return Err(BetaArtifactPreparationError::Transform);
        }
        Ok(Self {
            replacements,
            manifest_bytes,
            manifest,
            index_bytes,
            index,
            transform,
            evidence,
            publisher_native_host,
        })
    }
}

fn validate_managed_schema(
    archive: &mut AcquiredExtensionArchive<'_>,
    manifest: &[u8],
) -> Result<(), BetaArtifactPreparationError> {
    let document = parse_bounded_json(manifest, BoundedJsonLimits::extension_manifest())
        .map_err(|_| BetaArtifactPreparationError::Transform)?
        .into_value();
    let path = document
        .pointer("/storage/managed_schema")
        .and_then(serde_json::Value::as_str)
        .ok_or(BetaArtifactPreparationError::Transform)?;
    let path = path.strip_prefix("./").unwrap_or(path);
    let slot = archive
        .files()
        .iter()
        .position(|file| {
            file.path().as_str() == path && file.length() <= MAX_EXTENSION_MANIFEST_BYTES as u64
        })
        .ok_or(BetaArtifactPreparationError::Transform)?;
    let mut bytes = Vec::new();
    let _schema_receipt = archive
        .copy_file(slot, &mut bytes)
        .map_err(|_| BetaArtifactPreparationError::Transform)?;
    let schema = parse_bounded_json(&bytes, BoundedJsonLimits::extension_manifest())
        .map_err(|_| BetaArtifactPreparationError::Transform)?
        .into_value();
    if schema.get("type").and_then(serde_json::Value::as_str) != Some("object")
        || !schema
            .get("properties")
            .is_some_and(serde_json::Value::is_object)
    {
        return Err(BetaArtifactPreparationError::Transform);
    }
    // No policy values are supplied or validated. Schema metadata never causes
    // a network fetch, and defaults in the schema are not administrator policy.
    Ok(())
}

fn compose_managed_storage_transform(
    base: ExtensionTransformProvenance,
) -> Result<ExtensionTransformProvenance, BetaArtifactPreparationError> {
    let ExtensionTransformProvenance::Compiled {
        target,
        revision,
        sha256,
    } = base
    else {
        return Err(BetaArtifactPreparationError::Transform);
    };
    let mut hash = Sha256::new();
    hash.update(b"zephium:local.managed-storage.v2\0bounded-object-schema;no-configured-policy;isolated-contexts-and-extension-pages;preserve-schema-file-and-manifest\0");
    hash.update(MANAGED_STORAGE_V2.as_bytes());
    hash.update((target.as_str().len() as u32).to_le_bytes());
    hash.update(target.as_str().as_bytes());
    hash.update(revision.get().to_le_bytes());
    hash.update(sha256);
    Ok(ExtensionTransformProvenance::Compiled {
        target,
        revision: revision
            .checked_add(1)
            .ok_or(BetaArtifactPreparationError::Transform)?,
        sha256: hash.finalize().into(),
    })
}

fn compose_storage_transform(
    base: ExtensionTransformProvenance,
) -> Result<ExtensionTransformProvenance, BetaArtifactPreparationError> {
    let ExtensionTransformProvenance::Compiled {
        target,
        revision,
        sha256,
    } = base
    else {
        return Err(BetaArtifactPreparationError::Transform);
    };
    let mut hash = Sha256::new();
    hash.update(b"zephium:local.storage-and-native-adapter.v1\0storage-before-native;preserve-native-target\0");
    hash.update(bounded_storage_transform_sha256());
    hash.update((target.as_str().len() as u32).to_le_bytes());
    hash.update(target.as_str().as_bytes());
    hash.update(revision.get().to_le_bytes());
    hash.update(sha256);
    Ok(ExtensionTransformProvenance::Compiled {
        target,
        revision: revision
            .checked_add(1)
            .ok_or(BetaArtifactPreparationError::Transform)?,
        sha256: hash.finalize().into(),
    })
}

fn compose_implicit_head_transform(
    base: ExtensionTransformProvenance,
    pages: &std::collections::BTreeSet<String>,
) -> Result<ExtensionTransformProvenance, BetaArtifactPreparationError> {
    let ExtensionTransformProvenance::Compiled {
        target,
        revision,
        sha256,
    } = base
    else {
        return Err(BetaArtifactPreparationError::Transform);
    };
    let mut hash = Sha256::new();
    hash.update(implicit_head::DESCRIPTOR);
    hash.update((target.as_str().len() as u32).to_le_bytes());
    hash.update(target.as_str().as_bytes());
    hash.update(revision.get().to_le_bytes());
    hash.update(sha256);
    hash.update(serde_json::to_vec(pages).map_err(|_| BetaArtifactPreparationError::Transform)?);
    Ok(ExtensionTransformProvenance::Compiled {
        target,
        revision: revision
            .checked_add(1)
            .ok_or(BetaArtifactPreparationError::Transform)?,
        sha256: hash.finalize().into(),
    })
}

fn compose_cross_browser_transform(
    base: ExtensionTransformProvenance,
) -> Result<ExtensionTransformProvenance, BetaArtifactPreparationError> {
    let mut hash = Sha256::new();
    hash.update(b"zephium:local.cross-browser-background.v1\0exact-single-scripts-equals-worker;no-preferred-environment;select-worker-before-native-adaptation;preserve-source-files;canonical-json\0");
    let (target, revision) = match base {
        ExtensionTransformProvenance::Identity => {
            hash.update([0]);
            (
                "local.cross-browser-background.v1".to_owned(),
                NonZeroU32::MIN,
            )
        }
        ExtensionTransformProvenance::Compiled {
            target,
            revision,
            sha256,
        } => {
            hash.update([1]);
            hash.update((target.as_str().len() as u32).to_le_bytes());
            hash.update(target.as_str().as_bytes());
            hash.update(revision.get().to_le_bytes());
            hash.update(sha256);
            if target.as_str() == TRANSFORM_ID {
                (
                    "local.cross-browser-background.v1".to_owned(),
                    NonZeroU32::MIN,
                )
            } else {
                (
                    target.as_str().to_owned(),
                    revision
                        .checked_add(1)
                        .ok_or(BetaArtifactPreparationError::Transform)?,
                )
            }
        }
    };
    Ok(ExtensionTransformProvenance::Compiled {
        target: ExtensionCompatibilityTargetId::parse_exact(&target)
            .map_err(|_| BetaArtifactPreparationError::Transform)?,
        revision,
        sha256: hash.finalize().into(),
    })
}

fn compose_withheld_transform(
    base: ExtensionTransformProvenance,
    withheld: &super::withheld::WithheldFeatures,
) -> Result<ExtensionTransformProvenance, BetaArtifactPreparationError> {
    if withheld.is_empty() {
        return Ok(base);
    }
    let mut hash = Sha256::new();
    hash.update(b"zephium:local.withheld-features.v1\0remove-unsupported-optional-api-and-hosts;external-self-only-no-webpages;preserve-other-fields-and-files;canonical-json\0");
    let mut evidence = withheld.evidence();
    evidence.sort_all_objects();
    hash.update(
        serde_json::to_vec(&evidence).map_err(|_| BetaArtifactPreparationError::Transform)?,
    );
    let (target, revision) = match base {
        ExtensionTransformProvenance::Identity => {
            hash.update([0]);
            ("local.withheld-features.v1".to_owned(), NonZeroU32::MIN)
        }
        ExtensionTransformProvenance::Compiled {
            target,
            revision,
            sha256,
        } => {
            hash.update([1]);
            hash.update((target.as_str().len() as u32).to_le_bytes());
            hash.update(target.as_str().as_bytes());
            hash.update(revision.get().to_le_bytes());
            hash.update(sha256);
            if target.as_str() == TRANSFORM_ID {
                ("local.withheld-features.v1".to_owned(), NonZeroU32::MIN)
            } else {
                (
                    target.as_str().to_owned(),
                    revision
                        .checked_add(1)
                        .ok_or(BetaArtifactPreparationError::Transform)?,
                )
            }
        }
    };
    Ok(ExtensionTransformProvenance::Compiled {
        target: ExtensionCompatibilityTargetId::parse_exact(&target)
            .map_err(|_| BetaArtifactPreparationError::Transform)?,
        revision,
        sha256: hash.finalize().into(),
    })
}

fn authenticate<'a>(
    source: &ProductAdmittedBetaSource,
    bytes: &'a [u8],
) -> Result<AcquiredExtensionArchive<'a>, BetaArtifactPreparationError> {
    if bytes.len() > MAX_ORIGINAL_CRX_BYTES || bytes.len() != source.source.original_crx_length() {
        return Err(BetaArtifactPreparationError::Source);
    }
    let archive = AcquiredExtensionArchive::authenticate_upstream_crx3(
        bytes,
        source.source.extension_id(),
        Some(source.source.developer_key_sha256()),
    )
    .map_err(|_| BetaArtifactPreparationError::Source)?;
    if archive.original_crx_sha256() != source.upstream().original_crx_sha256()
        || archive.payload_identity() != source.source.payload_identity()
        || archive.files().len() != source.source.index().files().len()
    {
        return Err(BetaArtifactPreparationError::Source);
    }
    Ok(archive)
}

fn verify_artifact(
    directory: &SealedPrivateDirectory,
    source: &ProductAdmittedBetaSource,
    blueprint: &Blueprint,
) -> Result<(), BetaArtifactPreparationError> {
    let entries = directory.list_entry_names(4).map_err(storage)?;
    if entries.iter().map(|name| name.as_str()).collect::<Vec<_>>()
        != [
            "evidence.json",
            "extension",
            "original.crx",
            "tree-index.json",
        ]
    {
        return Err(BetaArtifactPreparationError::Integrity);
    }
    if read_control(directory, "tree-index.json", blueprint.index_bytes.len())?
        != blueprint.index_bytes
        || read_control(directory, "evidence.json", blueprint.evidence.len())? != blueprint.evidence
    {
        return Err(BetaArtifactPreparationError::Integrity);
    }
    let (length, digest) = directory
        .with_bounded_entry_regular_reader(
            &entry("original.crx")?,
            bound(MAX_ORIGINAL_CRX_BYTES)?,
            tree::hash_reader,
        )
        .map_err(storage)?
        .ok_or(BetaArtifactPreparationError::Integrity)??;
    if length != source.source.original_crx_length() as u64
        || digest != source.upstream().original_crx_sha256()
    {
        return Err(BetaArtifactPreparationError::Integrity);
    }
    let output = directory
        .open_sealed_entry_child(&entry("extension")?)
        .map_err(storage)?;
    tree::verify(&output, &blueprint.index)
}

fn write_control(
    directory: &PrivateDirectory,
    name: &str,
    bytes: &[u8],
    max: usize,
) -> Result<(), BetaArtifactPreparationError> {
    directory
        .write_new_synced(&component(name)?, bytes, bound(max)?)
        .map_err(storage)?;
    directory
        .seal_verified_regular(&component(name)?)
        .map_err(storage)?
        .ok_or(BetaArtifactPreparationError::Integrity)?;
    Ok(())
}
fn read_control(
    directory: &SealedPrivateDirectory,
    name: &str,
    exact: usize,
) -> Result<Vec<u8>, BetaArtifactPreparationError> {
    if exact == 0 || exact > MAX_ORIGINAL_CRX_BYTES.max(MAX_EXTENSION_TREE_INDEX_BYTES) {
        return Err(BetaArtifactPreparationError::Integrity);
    }
    directory
        .with_bounded_entry_regular_reader(&entry(name)?, bound(exact)?, |reader| {
            let mut bytes = vec![0; exact];
            std::io::Read::read_exact(reader, &mut bytes)
                .map_err(|_| BetaArtifactPreparationError::Integrity)?;
            if reader
                .read(&mut [0])
                .map_err(|_| BetaArtifactPreparationError::Integrity)?
                != 0
            {
                return Err(BetaArtifactPreparationError::Integrity);
            }
            Ok(bytes)
        })
        .map_err(storage)?
        .ok_or(BetaArtifactPreparationError::Integrity)?
}
fn component(name: &str) -> Result<PrivateComponent, BetaArtifactPreparationError> {
    PrivateComponent::new(name).map_err(storage)
}
fn entry(name: &str) -> Result<zephium_private_fs::PrivateEntryName, BetaArtifactPreparationError> {
    zephium_private_fs::PrivateEntryName::new(name)
        .map_err(|_| BetaArtifactPreparationError::Integrity)
}
fn bound(bytes: usize) -> Result<ByteLimit, BetaArtifactPreparationError> {
    ByteLimit::new(bytes).map_err(storage)
}
fn storage(_error: impl std::fmt::Debug) -> BetaArtifactPreparationError {
    BetaArtifactPreparationError::Storage
}

fn lock(gate: &Mutex<()>) -> Result<MutexGuard<'_, ()>, BetaArtifactPreparationError> {
    gate.try_lock().map_err(|error| match error {
        TryLockError::WouldBlock => BetaArtifactPreparationError::Busy,
        TryLockError::Poisoned(_) => BetaArtifactPreparationError::Storage,
    })
}
