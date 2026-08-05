//! Durable, path-free native extension ownership reconciliation state.
//!
//! This journal is the ordering barrier around every backend call that could
//! create, retain, or destroy a native extension owner. A coordinator must
//! durably publish `NativeMayOwn` before entering such a call and may clear an
//! entry only after native absence is definite and every subordinate package
//! or resource owner has been released. On restart, `NativeOwned` is
//! intentionally no stronger than `NativeMayOwn`; persistence preserves the
//! exact phase while the service applies that conservative interpretation.

use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::mem::size_of;

use crate::ids::{ExtensionInstallId, ProfileId};
use crate::session::MAX_SESSION_PROFILES;

use super::{
    ExtensionGrantBrowsingContext, ExtensionGrantDigest, ExtensionGrantRevision,
    ExtensionInstallCatalogRevision, ExtensionInstallRevision, ExtensionPackageIdentity,
    MAX_EXTENSION_INSTALLS_PER_PROFILE,
};

/// Maximum number of unresolved native owners retained across all profiles.
///
/// One install can own at most one regular and one private runtime. The
/// durable ceiling follows the complete bounded profile/install authority
/// space; tighter live-native limits remain the engine resource ledger's job.
pub const MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES: usize =
    MAX_SESSION_PROFILES * MAX_EXTENSION_INSTALLS_PER_PROFILE * 2;

/// Maximum vector allocation retained by a valid complete journal cohort.
pub const MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_RETAINED_BYTES: usize =
    MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES * size_of::<ExtensionNativeOwnershipEntry>();

/// Conservative retained size of one actor mutation payload.
pub const MAX_EXTENSION_NATIVE_OWNERSHIP_MUTATION_RETAINED_BYTES: usize =
    size_of::<ExtensionNativeOwnershipJournalMutation>()
        + size_of::<ExtensionNativeOwnershipPreparation>()
        + 128;

/// Exact byte length of every platform-native extension owner identifier.
///
/// Zephium admits only the canonical Chromium identifier alphabet at this
/// boundary. WebView2 exposes that identifier natively, while WKWebExtension
/// contexts are assigned the same bounded grammar by Zephium before loading.
/// A fixed representation keeps the complete durable cohort allocation-free
/// and prevents backend strings from becoming an unbounded persistence input.
pub const EXTENSION_NATIVE_OWNERSHIP_ID_BYTES: usize = 32;

const MAX_DURABLE_COUNTER: u64 = i64::MAX as u64;

macro_rules! durable_counter {
    ($name:ident, $initial_doc:literal) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u64);

        impl $name {
            #[doc = $initial_doc]
            pub const INITIAL: Self = Self(1);

            /// Constructs one nonzero value representable by SQLite INTEGER.
            pub const fn new(value: u64) -> Option<Self> {
                if value == 0 || value > MAX_DURABLE_COUNTER {
                    None
                } else {
                    Some(Self(value))
                }
            }

            /// Returns the exact durable integer value.
            pub const fn get(self) -> u64 {
                self.0
            }

            /// Advances without wrapping or crossing SQLite's signed range.
            pub const fn next(self) -> Option<Self> {
                match self.0.checked_add(1) {
                    Some(value) => Self::new(value),
                    None => None,
                }
            }
        }
    };
}

durable_counter!(
    ExtensionNativeOwnershipJournalRevision,
    "Initial complete-journal compare-and-swap revision."
);
durable_counter!(
    ExtensionNativeOwnershipEntryRevision,
    "Initial revision of one unresolved native ownership operation."
);
durable_counter!(
    ExtensionNativeOwnershipOperation,
    "First durable native ownership operation identity."
);
durable_counter!(
    ExtensionNativeIncarnation,
    "First persistent native owner incarnation."
);

/// Path-free content identity of one exact active or rollback catalog set.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExtensionCatalogSetDigest([u8; 32]);

impl ExtensionCatalogSetDigest {
    /// Constructs an exact digest. Authentication remains repository-owned.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the exact digest bytes.
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }
}

impl fmt::Debug for ExtensionCatalogSetDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "ExtensionCatalogSetDigest({:02x}{:02x}{:02x}{:02x}…)",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}

/// Product-authorized role of the exact catalog set used by this owner.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ExtensionCatalogGenerationRole {
    /// Ordinary active product generation.
    Active,
    /// Explicitly authorized rollback generation.
    Rollback,
}

impl ExtensionCatalogGenerationRole {
    /// Stable durable encoding.
    pub const fn as_persisted(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Rollback => "rollback",
        }
    }

    /// Decodes only the closed durable vocabulary.
    pub fn from_persisted(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "rollback" => Some(Self::Rollback),
            _ => None,
        }
    }
}

/// Exact reviewed runtime backend selected for one native owner.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ExtensionRuntimeBackendTarget {
    /// Apple's native WKWebExtension runtime.
    MacosNative,
    /// Zephium compatibility runtime hosted by WKWebView.
    MacosCompatibility,
    /// Zephium compatibility runtime hosted by WebKitGTK.
    LinuxCompatibility,
    /// WebView2's native browser-extension runtime.
    WindowsNative,
}

impl ExtensionRuntimeBackendTarget {
    /// Stable durable encoding.
    pub const fn as_persisted(self) -> &'static str {
        match self {
            Self::MacosNative => "macos_native",
            Self::MacosCompatibility => "macos_compatibility",
            Self::LinuxCompatibility => "linux_compatibility",
            Self::WindowsNative => "windows_native",
        }
    }

    /// Decodes only the closed durable vocabulary.
    pub fn from_persisted(value: &str) -> Option<Self> {
        match value {
            "macos_native" => Some(Self::MacosNative),
            "macos_compatibility" => Some(Self::MacosCompatibility),
            "linux_compatibility" => Some(Self::LinuxCompatibility),
            "windows_native" => Some(Self::WindowsNative),
            _ => None,
        }
    }
}

/// Exact backend-bound identifier of one platform-native extension owner.
///
/// This is deliberately not an extension install identity. It identifies the
/// concrete owner surfaced by one native backend and is useful only while the
/// containing journal row and incarnation remain authoritative.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ExtensionNativeOwnershipIdentity {
    /// `WKWebExtensionContext.uniqueIdentifier`, assigned by Zephium.
    MacosWebExtension([u8; EXTENSION_NATIVE_OWNERSHIP_ID_BYTES]),
    /// `CoreWebView2BrowserExtension.Id`, returned by WebView2.
    WindowsWebView2Extension([u8; EXTENSION_NATIVE_OWNERSHIP_ID_BYTES]),
}

impl ExtensionNativeOwnershipIdentity {
    /// Parses one exact native identifier for the selected backend.
    ///
    /// Compatibility runtimes do not expose a platform-native extension
    /// owner and therefore cannot mint this authority.
    pub fn parse(
        backend: ExtensionRuntimeBackendTarget,
        value: &str,
    ) -> Result<Self, ExtensionNativeOwnershipIdentityError> {
        let bytes: [u8; EXTENSION_NATIVE_OWNERSHIP_ID_BYTES] = value
            .as_bytes()
            .try_into()
            .map_err(|_| ExtensionNativeOwnershipIdentityError::InvalidIdentifier)?;
        if !bytes.iter().all(|byte| matches!(byte, b'a'..=b'p')) {
            return Err(ExtensionNativeOwnershipIdentityError::InvalidIdentifier);
        }
        match backend {
            ExtensionRuntimeBackendTarget::MacosNative => Ok(Self::MacosWebExtension(bytes)),
            ExtensionRuntimeBackendTarget::WindowsNative => {
                Ok(Self::WindowsWebView2Extension(bytes))
            }
            ExtensionRuntimeBackendTarget::MacosCompatibility
            | ExtensionRuntimeBackendTarget::LinuxCompatibility => {
                Err(ExtensionNativeOwnershipIdentityError::UnsupportedBackend)
            }
        }
    }

    /// Reconstructs only the closed durable kind vocabulary.
    pub fn from_persisted(
        kind: u8,
        bytes: [u8; EXTENSION_NATIVE_OWNERSHIP_ID_BYTES],
    ) -> Result<Self, ExtensionNativeOwnershipIdentityError> {
        let backend = match kind {
            1 => ExtensionRuntimeBackendTarget::MacosNative,
            2 => ExtensionRuntimeBackendTarget::WindowsNative,
            _ => return Err(ExtensionNativeOwnershipIdentityError::UnknownKind),
        };
        let value = std::str::from_utf8(&bytes)
            .map_err(|_| ExtensionNativeOwnershipIdentityError::InvalidIdentifier)?;
        Self::parse(backend, value)
    }

    /// Stable compact durable kind.
    pub const fn persisted_kind(self) -> u8 {
        match self {
            Self::MacosWebExtension(_) => 1,
            Self::WindowsWebView2Extension(_) => 2,
        }
    }

    /// Backend that owns this identifier.
    pub const fn backend(self) -> ExtensionRuntimeBackendTarget {
        match self {
            Self::MacosWebExtension(_) => ExtensionRuntimeBackendTarget::MacosNative,
            Self::WindowsWebView2Extension(_) => ExtensionRuntimeBackendTarget::WindowsNative,
        }
    }

    /// Returns the exact bounded native identifier bytes.
    pub const fn bytes(self) -> [u8; EXTENSION_NATIVE_OWNERSHIP_ID_BYTES] {
        match self {
            Self::MacosWebExtension(bytes) | Self::WindowsWebView2Extension(bytes) => bytes,
        }
    }
}

impl fmt::Debug for ExtensionNativeOwnershipIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionNativeOwnershipIdentity")
            .field("backend", &self.backend())
            .field("identifier", &"<redacted>")
            .finish()
    }
}

/// Structural native-owner identifier refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionNativeOwnershipIdentityError {
    UnsupportedBackend,
    UnknownKind,
    InvalidIdentifier,
}

impl fmt::Display for ExtensionNativeOwnershipIdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid extension native-owner identifier: {self:?}"
        )
    }
}

impl Error for ExtensionNativeOwnershipIdentityError {}

/// Desired direction of one unresolved native ownership operation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ExtensionNativeOwnershipIntent {
    /// Establish and retain the exact native owner.
    Acquire,
    /// Prove the native owner absent and release its subordinate resources.
    Release,
}

impl ExtensionNativeOwnershipIntent {
    /// Stable durable encoding.
    pub const fn as_persisted(self) -> &'static str {
        match self {
            Self::Acquire => "acquire",
            Self::Release => "release",
        }
    }

    /// Decodes only the closed durable vocabulary.
    pub fn from_persisted(value: &str) -> Option<Self> {
        match value {
            "acquire" => Some(Self::Acquire),
            "release" => Some(Self::Release),
            _ => None,
        }
    }
}

/// Conservative durable native-ownership phase.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ExtensionNativeOwnershipPhase {
    /// Native absence is definite; preparation may occur but no native call
    /// capable of creating an owner has begun.
    NativeAbsentPreparing,
    /// A native owner may exist. This must commit before entering an
    /// activation, removal, or otherwise ownership-changing native call.
    NativeMayOwn,
    /// The prior process observed definite ownership. Restart reconciliation
    /// must conservatively treat this as `NativeMayOwn`.
    NativeOwned,
    /// Native absence is definite, but package/resource release and final
    /// journal clearing remain outstanding.
    NativeAbsentReleasePending,
}

impl ExtensionNativeOwnershipPhase {
    /// Stable durable encoding.
    pub const fn as_persisted(self) -> &'static str {
        match self {
            Self::NativeAbsentPreparing => "native_absent_preparing",
            Self::NativeMayOwn => "native_may_own",
            Self::NativeOwned => "native_owned",
            Self::NativeAbsentReleasePending => "native_absent_release_pending",
        }
    }

    /// Decodes only the closed durable vocabulary.
    pub fn from_persisted(value: &str) -> Option<Self> {
        match value {
            "native_absent_preparing" => Some(Self::NativeAbsentPreparing),
            "native_may_own" => Some(Self::NativeMayOwn),
            "native_owned" => Some(Self::NativeOwned),
            "native_absent_release_pending" => Some(Self::NativeAbsentReleasePending),
            _ => None,
        }
    }
}

/// Stable unique key for one profile/install/browsing-partition owner.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExtensionNativeOwnershipKey {
    profile: ProfileId,
    install_id: ExtensionInstallId,
    browsing_context: ExtensionGrantBrowsingContext,
}

impl ExtensionNativeOwnershipKey {
    /// Constructs the complete owner key.
    pub const fn new(
        profile: ProfileId,
        install_id: ExtensionInstallId,
        browsing_context: ExtensionGrantBrowsingContext,
    ) -> Self {
        Self {
            profile,
            install_id,
            browsing_context,
        }
    }

    pub const fn profile(self) -> ProfileId {
        self.profile
    }

    pub const fn install_id(self) -> ExtensionInstallId {
        self.install_id
    }

    pub const fn browsing_context(self) -> ExtensionGrantBrowsingContext {
        self.browsing_context
    }
}

/// Complete path-free input for beginning one ownership operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionNativeOwnershipPreparation {
    key: ExtensionNativeOwnershipKey,
    package: ExtensionPackageIdentity,
    catalog_set_digest: ExtensionCatalogSetDigest,
    catalog_role: ExtensionCatalogGenerationRole,
    store_catalog_revision: ExtensionInstallCatalogRevision,
    store_install_revision: ExtensionInstallRevision,
    store_grant_revision: ExtensionGrantRevision,
    grant_digest: ExtensionGrantDigest,
    runtime_backend: ExtensionRuntimeBackendTarget,
}

impl ExtensionNativeOwnershipPreparation {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        key: ExtensionNativeOwnershipKey,
        package: ExtensionPackageIdentity,
        catalog_set_digest: ExtensionCatalogSetDigest,
        catalog_role: ExtensionCatalogGenerationRole,
        store_catalog_revision: ExtensionInstallCatalogRevision,
        store_install_revision: ExtensionInstallRevision,
        store_grant_revision: ExtensionGrantRevision,
        grant_digest: ExtensionGrantDigest,
        runtime_backend: ExtensionRuntimeBackendTarget,
    ) -> Self {
        Self {
            key,
            package,
            catalog_set_digest,
            catalog_role,
            store_catalog_revision,
            store_install_revision,
            store_grant_revision,
            grant_digest,
            runtime_backend,
        }
    }

    pub const fn key(&self) -> ExtensionNativeOwnershipKey {
        self.key
    }

    pub const fn profile(&self) -> ProfileId {
        self.key.profile()
    }

    pub fn package(&self) -> &ExtensionPackageIdentity {
        &self.package
    }

    pub const fn catalog_set_digest(&self) -> ExtensionCatalogSetDigest {
        self.catalog_set_digest
    }

    pub const fn catalog_role(&self) -> ExtensionCatalogGenerationRole {
        self.catalog_role
    }

    pub const fn store_catalog_revision(&self) -> ExtensionInstallCatalogRevision {
        self.store_catalog_revision
    }

    pub const fn store_install_revision(&self) -> ExtensionInstallRevision {
        self.store_install_revision
    }

    pub const fn store_grant_revision(&self) -> ExtensionGrantRevision {
        self.store_grant_revision
    }

    pub const fn grant_digest(&self) -> ExtensionGrantDigest {
        self.grant_digest
    }

    pub const fn runtime_backend(&self) -> ExtensionRuntimeBackendTarget {
        self.runtime_backend
    }
}

/// One exact unresolved native ownership operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionNativeOwnershipEntry {
    key: ExtensionNativeOwnershipKey,
    operation: ExtensionNativeOwnershipOperation,
    revision: ExtensionNativeOwnershipEntryRevision,
    package: ExtensionPackageIdentity,
    catalog_set_digest: ExtensionCatalogSetDigest,
    catalog_role: ExtensionCatalogGenerationRole,
    store_catalog_revision: ExtensionInstallCatalogRevision,
    store_install_revision: ExtensionInstallRevision,
    store_grant_revision: ExtensionGrantRevision,
    grant_digest: ExtensionGrantDigest,
    runtime_backend: ExtensionRuntimeBackendTarget,
    native_identity: Option<ExtensionNativeOwnershipIdentity>,
    native_incarnation: ExtensionNativeIncarnation,
    intent: ExtensionNativeOwnershipIntent,
    phase: ExtensionNativeOwnershipPhase,
}

impl ExtensionNativeOwnershipEntry {
    #[allow(clippy::too_many_arguments)]
    pub fn from_persisted(
        key: ExtensionNativeOwnershipKey,
        operation: ExtensionNativeOwnershipOperation,
        revision: ExtensionNativeOwnershipEntryRevision,
        package: ExtensionPackageIdentity,
        catalog_set_digest: ExtensionCatalogSetDigest,
        catalog_role: ExtensionCatalogGenerationRole,
        store_catalog_revision: ExtensionInstallCatalogRevision,
        store_install_revision: ExtensionInstallRevision,
        store_grant_revision: ExtensionGrantRevision,
        grant_digest: ExtensionGrantDigest,
        runtime_backend: ExtensionRuntimeBackendTarget,
        native_incarnation: ExtensionNativeIncarnation,
        intent: ExtensionNativeOwnershipIntent,
        phase: ExtensionNativeOwnershipPhase,
    ) -> Result<Self, ExtensionNativeOwnershipJournalError> {
        Self::from_persisted_with_native_identity(
            key,
            operation,
            revision,
            package,
            catalog_set_digest,
            catalog_role,
            store_catalog_revision,
            store_install_revision,
            store_grant_revision,
            grant_digest,
            runtime_backend,
            None,
            native_incarnation,
            intent,
            phase,
        )
    }

    /// Reconstructs a persisted row with an optional exact native identity.
    #[allow(clippy::too_many_arguments)]
    pub fn from_persisted_with_native_identity(
        key: ExtensionNativeOwnershipKey,
        operation: ExtensionNativeOwnershipOperation,
        revision: ExtensionNativeOwnershipEntryRevision,
        package: ExtensionPackageIdentity,
        catalog_set_digest: ExtensionCatalogSetDigest,
        catalog_role: ExtensionCatalogGenerationRole,
        store_catalog_revision: ExtensionInstallCatalogRevision,
        store_install_revision: ExtensionInstallRevision,
        store_grant_revision: ExtensionGrantRevision,
        grant_digest: ExtensionGrantDigest,
        runtime_backend: ExtensionRuntimeBackendTarget,
        native_identity: Option<ExtensionNativeOwnershipIdentity>,
        native_incarnation: ExtensionNativeIncarnation,
        intent: ExtensionNativeOwnershipIntent,
        phase: ExtensionNativeOwnershipPhase,
    ) -> Result<Self, ExtensionNativeOwnershipJournalError> {
        if operation.get() != native_incarnation.get() {
            return Err(ExtensionNativeOwnershipJournalError::OperationIncarnationMismatch);
        }
        if !valid_state(intent, phase) {
            return Err(ExtensionNativeOwnershipJournalError::InvalidState { intent, phase });
        }
        if !valid_native_identity(runtime_backend, native_identity, intent, phase) {
            return Err(ExtensionNativeOwnershipJournalError::InvalidNativeIdentity);
        }
        if !valid_entry_revision(runtime_backend, native_identity, intent, phase, revision) {
            return Err(ExtensionNativeOwnershipJournalError::InvalidEntryRevision {
                intent,
                phase,
                revision,
            });
        }
        Ok(Self {
            key,
            operation,
            revision,
            package,
            catalog_set_digest,
            catalog_role,
            store_catalog_revision,
            store_install_revision,
            store_grant_revision,
            grant_digest,
            runtime_backend,
            native_identity,
            native_incarnation,
            intent,
            phase,
        })
    }

    fn preparing(
        preparation: ExtensionNativeOwnershipPreparation,
        operation: ExtensionNativeOwnershipOperation,
        native_incarnation: ExtensionNativeIncarnation,
    ) -> Self {
        Self {
            key: preparation.key,
            operation,
            revision: ExtensionNativeOwnershipEntryRevision::INITIAL,
            package: preparation.package,
            catalog_set_digest: preparation.catalog_set_digest,
            catalog_role: preparation.catalog_role,
            store_catalog_revision: preparation.store_catalog_revision,
            store_install_revision: preparation.store_install_revision,
            store_grant_revision: preparation.store_grant_revision,
            grant_digest: preparation.grant_digest,
            runtime_backend: preparation.runtime_backend,
            native_identity: None,
            native_incarnation,
            intent: ExtensionNativeOwnershipIntent::Acquire,
            phase: ExtensionNativeOwnershipPhase::NativeAbsentPreparing,
        }
    }

    pub const fn key(&self) -> ExtensionNativeOwnershipKey {
        self.key
    }

    pub const fn operation(&self) -> ExtensionNativeOwnershipOperation {
        self.operation
    }

    pub const fn revision(&self) -> ExtensionNativeOwnershipEntryRevision {
        self.revision
    }

    pub fn package(&self) -> &ExtensionPackageIdentity {
        &self.package
    }

    pub const fn catalog_set_digest(&self) -> ExtensionCatalogSetDigest {
        self.catalog_set_digest
    }

    pub const fn catalog_role(&self) -> ExtensionCatalogGenerationRole {
        self.catalog_role
    }

    pub const fn store_catalog_revision(&self) -> ExtensionInstallCatalogRevision {
        self.store_catalog_revision
    }

    pub const fn store_install_revision(&self) -> ExtensionInstallRevision {
        self.store_install_revision
    }

    pub const fn store_grant_revision(&self) -> ExtensionGrantRevision {
        self.store_grant_revision
    }

    pub const fn grant_digest(&self) -> ExtensionGrantDigest {
        self.grant_digest
    }

    pub const fn runtime_backend(&self) -> ExtensionRuntimeBackendTarget {
        self.runtime_backend
    }

    /// Exact native owner identity, when one has been durably observed.
    pub const fn native_identity(&self) -> Option<ExtensionNativeOwnershipIdentity> {
        self.native_identity
    }

    pub const fn native_incarnation(&self) -> ExtensionNativeIncarnation {
        self.native_incarnation
    }

    pub const fn intent(&self) -> ExtensionNativeOwnershipIntent {
        self.intent
    }

    pub const fn phase(&self) -> ExtensionNativeOwnershipPhase {
        self.phase
    }

    /// Exact bounded compare-and-swap identity for this entry.
    pub const fn cas(&self) -> ExtensionNativeOwnershipEntryCas {
        ExtensionNativeOwnershipEntryCas {
            key: self.key,
            operation: self.operation,
            revision: self.revision,
            native_incarnation: self.native_incarnation,
        }
    }
}

/// Exact expected row identity for transition or clear.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionNativeOwnershipEntryCas {
    key: ExtensionNativeOwnershipKey,
    operation: ExtensionNativeOwnershipOperation,
    revision: ExtensionNativeOwnershipEntryRevision,
    native_incarnation: ExtensionNativeIncarnation,
}

impl ExtensionNativeOwnershipEntryCas {
    pub const fn key(self) -> ExtensionNativeOwnershipKey {
        self.key
    }

    pub const fn operation(self) -> ExtensionNativeOwnershipOperation {
        self.operation
    }

    pub const fn revision(self) -> ExtensionNativeOwnershipEntryRevision {
        self.revision
    }

    pub const fn native_incarnation(self) -> ExtensionNativeIncarnation {
        self.native_incarnation
    }
}

/// Complete bounded global reconciliation cohort.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionNativeOwnershipJournal {
    revision: ExtensionNativeOwnershipJournalRevision,
    operation_high_water: Option<ExtensionNativeOwnershipOperation>,
    native_incarnation_high_water: Option<ExtensionNativeIncarnation>,
    entries: Vec<ExtensionNativeOwnershipEntry>,
    retained_bytes: usize,
}

impl ExtensionNativeOwnershipJournal {
    /// Empty migration state.
    pub fn empty() -> Self {
        Self {
            revision: ExtensionNativeOwnershipJournalRevision::INITIAL,
            operation_high_water: None,
            native_incarnation_high_water: None,
            entries: Vec::new(),
            retained_bytes: 0,
        }
    }

    /// Reconstructs an exact durable cohort without sorting or filtering it.
    ///
    /// Persistence must submit rows in canonical key order. Unknown,
    /// duplicate, out-of-order, over-limit, or high-water-inconsistent state
    /// rejects the complete cohort; this boundary never canonicalizes hostile
    /// durable state into a smaller authority set.
    pub fn from_persisted(
        revision: ExtensionNativeOwnershipJournalRevision,
        operation_high_water: Option<ExtensionNativeOwnershipOperation>,
        native_incarnation_high_water: Option<ExtensionNativeIncarnation>,
        entries: Vec<ExtensionNativeOwnershipEntry>,
    ) -> Result<Self, ExtensionNativeOwnershipJournalError> {
        if !valid_revision_authority(
            revision,
            operation_high_water,
            native_incarnation_high_water,
        ) {
            return Err(ExtensionNativeOwnershipJournalError::InconsistentRevisionAuthority);
        }
        if entries.len() > MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES {
            return Err(ExtensionNativeOwnershipJournalError::TooManyEntries {
                count: entries.len(),
                max: MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES,
            });
        }
        let mut compact = Vec::with_capacity(entries.len());
        compact.extend(entries);
        compact.shrink_to_fit();
        let entries = compact;

        let mut previous = None;
        let mut operations = HashSet::with_capacity(entries.len());
        let mut incarnations = HashSet::with_capacity(entries.len());
        let mut live_entry_revision_sum = 0_u128;
        for entry in &entries {
            if let Some(prior) = previous {
                if entry.key == prior {
                    return Err(ExtensionNativeOwnershipJournalError::DuplicateKey(
                        entry.key,
                    ));
                }
                if entry.key < prior {
                    return Err(ExtensionNativeOwnershipJournalError::NonCanonicalOrder);
                }
            }
            previous = Some(entry.key);
            if !operations.insert(entry.operation) {
                return Err(ExtensionNativeOwnershipJournalError::DuplicateOperation(
                    entry.operation,
                ));
            }
            if !incarnations.insert(entry.native_incarnation) {
                return Err(ExtensionNativeOwnershipJournalError::DuplicateIncarnation(
                    entry.native_incarnation,
                ));
            }
            if entry.operation.get() != entry.native_incarnation.get() {
                return Err(ExtensionNativeOwnershipJournalError::OperationIncarnationMismatch);
            }
            if operation_high_water.is_none_or(|high_water| entry.operation > high_water) {
                return Err(ExtensionNativeOwnershipJournalError::OperationAboveHighWater);
            }
            if native_incarnation_high_water
                .is_none_or(|high_water| entry.native_incarnation > high_water)
            {
                return Err(ExtensionNativeOwnershipJournalError::IncarnationAboveHighWater);
            }
            if !valid_state(entry.intent, entry.phase) {
                return Err(ExtensionNativeOwnershipJournalError::InvalidState {
                    intent: entry.intent,
                    phase: entry.phase,
                });
            }
            if !valid_native_identity(
                entry.runtime_backend,
                entry.native_identity,
                entry.intent,
                entry.phase,
            ) {
                return Err(ExtensionNativeOwnershipJournalError::InvalidNativeIdentity);
            }
            if !valid_entry_revision(
                entry.runtime_backend,
                entry.native_identity,
                entry.intent,
                entry.phase,
                entry.revision,
            ) {
                return Err(ExtensionNativeOwnershipJournalError::InvalidEntryRevision {
                    intent: entry.intent,
                    phase: entry.phase,
                    revision: entry.revision,
                });
            }
            if u128::from(entry.operation.get()) + u128::from(entry.revision.get())
                > u128::from(revision.get())
            {
                return Err(ExtensionNativeOwnershipJournalError::EntryAboveJournalRevision);
            }
            live_entry_revision_sum = live_entry_revision_sum
                .checked_add(u128::from(entry.revision.get()))
                .ok_or(ExtensionNativeOwnershipJournalError::RevisionHistoryInconsistent)?;
        }
        let operation_count = operation_high_water.map_or(0_u128, |value| u128::from(value.get()));
        let live_count = entries.len() as u128;
        let cleared_count = operation_count
            .checked_sub(live_count)
            .ok_or(ExtensionNativeOwnershipJournalError::RevisionHistoryInconsistent)?;
        let minimum_revision = 1_u128
            .checked_add(live_entry_revision_sum)
            .and_then(|value| value.checked_add(cleared_count.checked_mul(3)?))
            .ok_or(ExtensionNativeOwnershipJournalError::RevisionHistoryInconsistent)?;
        let maximum_revision = 1_u128
            .checked_add(live_entry_revision_sum)
            .and_then(|value| value.checked_add(cleared_count.checked_mul(7)?))
            .ok_or(ExtensionNativeOwnershipJournalError::RevisionHistoryInconsistent)?;
        let durable_revision = u128::from(revision.get());
        if !(minimum_revision..=maximum_revision).contains(&durable_revision) {
            return Err(ExtensionNativeOwnershipJournalError::RevisionHistoryInconsistent);
        }
        let retained_bytes = entries
            .capacity()
            .checked_mul(size_of::<ExtensionNativeOwnershipEntry>())
            .ok_or(ExtensionNativeOwnershipJournalError::RetainedBytesExceeded)?;
        if retained_bytes > MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_RETAINED_BYTES {
            return Err(ExtensionNativeOwnershipJournalError::RetainedBytesExceeded);
        }
        Ok(Self {
            revision,
            operation_high_water,
            native_incarnation_high_water,
            entries,
            retained_bytes,
        })
    }

    pub const fn revision(&self) -> ExtensionNativeOwnershipJournalRevision {
        self.revision
    }

    pub const fn operation_high_water(&self) -> Option<ExtensionNativeOwnershipOperation> {
        self.operation_high_water
    }

    pub const fn native_incarnation_high_water(&self) -> Option<ExtensionNativeIncarnation> {
        self.native_incarnation_high_water
    }

    pub fn entries(&self) -> &[ExtensionNativeOwnershipEntry] {
        &self.entries
    }

    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub fn get(&self, key: ExtensionNativeOwnershipKey) -> Option<&ExtensionNativeOwnershipEntry> {
        self.entries
            .binary_search_by_key(&key, ExtensionNativeOwnershipEntry::key)
            .ok()
            .map(|index| &self.entries[index])
    }

    /// Applies one exact global-CAS begin, transition, or clear operation.
    pub fn apply(
        mut self,
        expected: ExtensionNativeOwnershipJournalRevision,
        mutation: ExtensionNativeOwnershipJournalMutation,
    ) -> Result<ExtensionNativeOwnershipJournalApplication, ExtensionNativeOwnershipApplyError>
    {
        if self.revision != expected {
            return Err(ExtensionNativeOwnershipApplyError::Conflict {
                current: self.revision,
            });
        }
        let next_journal_revision = self
            .revision
            .next()
            .ok_or(ExtensionNativeOwnershipApplyError::RevisionExhausted)?;

        let (kind, entry) = match mutation {
            ExtensionNativeOwnershipJournalMutation::Begin(preparation) => {
                if self.entries.len() >= MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_ENTRIES {
                    return Err(ExtensionNativeOwnershipApplyError::LimitReached);
                }
                let key = preparation.key;
                match self
                    .entries
                    .binary_search_by_key(&key, ExtensionNativeOwnershipEntry::key)
                {
                    Ok(_) => return Err(ExtensionNativeOwnershipApplyError::Invalid),
                    Err(index) => {
                        let operation = match self.operation_high_water {
                            Some(value) => value.next(),
                            None => Some(ExtensionNativeOwnershipOperation::INITIAL),
                        }
                        .ok_or(ExtensionNativeOwnershipApplyError::RevisionExhausted)?;
                        let incarnation = match self.native_incarnation_high_water {
                            Some(value) => value.next(),
                            None => Some(ExtensionNativeIncarnation::INITIAL),
                        }
                        .ok_or(ExtensionNativeOwnershipApplyError::RevisionExhausted)?;
                        let entry = ExtensionNativeOwnershipEntry::preparing(
                            *preparation,
                            operation,
                            incarnation,
                        );
                        self.entries.insert(index, entry.clone());
                        self.operation_high_water = Some(operation);
                        self.native_incarnation_high_water = Some(incarnation);
                        (ExtensionNativeOwnershipMutationKind::Begin, Some(entry))
                    }
                }
            }
            ExtensionNativeOwnershipJournalMutation::Transition {
                expected,
                intent,
                phase,
                attach_native_identity,
            } => {
                let index = self
                    .entries
                    .binary_search_by_key(&expected.key, ExtensionNativeOwnershipEntry::key)
                    .map_err(|_| ExtensionNativeOwnershipApplyError::Conflict {
                        current: self.revision,
                    })?;
                let current = &self.entries[index];
                if current.cas() != expected {
                    return Err(ExtensionNativeOwnershipApplyError::Conflict {
                        current: self.revision,
                    });
                }
                let identity_only_transition = attach_native_identity.is_some()
                    && current.intent == intent
                    && current.phase == ExtensionNativeOwnershipPhase::NativeMayOwn
                    && current.phase == phase;
                if !valid_transition(current.intent, current.phase, intent, phase)
                    && !identity_only_transition
                {
                    return Err(ExtensionNativeOwnershipApplyError::Invalid);
                }
                if current.native_identity.is_some() && attach_native_identity.is_some() {
                    return Err(ExtensionNativeOwnershipApplyError::Invalid);
                }
                let next_entry_revision = current
                    .revision
                    .next()
                    .ok_or(ExtensionNativeOwnershipApplyError::RevisionExhausted)?;
                let native_identity = current.native_identity.or(attach_native_identity);
                if !valid_native_identity(current.runtime_backend, native_identity, intent, phase)
                    || !valid_entry_revision(
                        current.runtime_backend,
                        native_identity,
                        intent,
                        phase,
                        next_entry_revision,
                    )
                {
                    return Err(ExtensionNativeOwnershipApplyError::Invalid);
                }
                self.entries[index].revision = next_entry_revision;
                self.entries[index].intent = intent;
                self.entries[index].phase = phase;
                self.entries[index].native_identity = native_identity;
                (
                    ExtensionNativeOwnershipMutationKind::Transition,
                    Some(self.entries[index].clone()),
                )
            }
            ExtensionNativeOwnershipJournalMutation::Clear { expected } => {
                let index = self
                    .entries
                    .binary_search_by_key(&expected.key, ExtensionNativeOwnershipEntry::key)
                    .map_err(|_| ExtensionNativeOwnershipApplyError::Conflict {
                        current: self.revision,
                    })?;
                let current = &self.entries[index];
                if current.cas() != expected {
                    return Err(ExtensionNativeOwnershipApplyError::Conflict {
                        current: self.revision,
                    });
                }
                if current.intent != ExtensionNativeOwnershipIntent::Release
                    || current.phase != ExtensionNativeOwnershipPhase::NativeAbsentReleasePending
                {
                    return Err(ExtensionNativeOwnershipApplyError::Invalid);
                }
                self.entries.remove(index);
                (ExtensionNativeOwnershipMutationKind::Clear, None)
            }
        };
        self.revision = next_journal_revision;
        // Mutation order is already canonical. Repack without sorting so a
        // capacity grown by `Vec::insert` cannot silently exceed the cohort's
        // retained-memory ceiling.
        let mut compact = Vec::with_capacity(self.entries.len());
        compact.extend(self.entries);
        compact.shrink_to_fit();
        let retained_bytes = compact
            .capacity()
            .checked_mul(size_of::<ExtensionNativeOwnershipEntry>())
            .ok_or(ExtensionNativeOwnershipApplyError::LimitReached)?;
        if retained_bytes > MAX_EXTENSION_NATIVE_OWNERSHIP_JOURNAL_RETAINED_BYTES {
            return Err(ExtensionNativeOwnershipApplyError::LimitReached);
        }
        self.entries = compact;
        self.retained_bytes = retained_bytes;
        Ok(ExtensionNativeOwnershipJournalApplication {
            journal: self,
            kind,
            entry,
        })
    }
}

/// One bounded journal mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionNativeOwnershipJournalMutation {
    /// Begin at definite native absence before any ownership-changing call.
    Begin(Box<ExtensionNativeOwnershipPreparation>),
    /// Transition one exact operation after the global and row CAS match.
    Transition {
        expected: ExtensionNativeOwnershipEntryCas,
        intent: ExtensionNativeOwnershipIntent,
        phase: ExtensionNativeOwnershipPhase,
        /// Attach once; `None` preserves the currently persisted identity.
        attach_native_identity: Option<ExtensionNativeOwnershipIdentity>,
    },
    /// Clear only a definite-native-absence, release-pending row.
    Clear {
        expected: ExtensionNativeOwnershipEntryCas,
    },
}

impl ExtensionNativeOwnershipJournalMutation {
    pub fn begin(preparation: ExtensionNativeOwnershipPreparation) -> Self {
        Self::Begin(Box::new(preparation))
    }

    pub const fn transition(
        expected: ExtensionNativeOwnershipEntryCas,
        intent: ExtensionNativeOwnershipIntent,
        phase: ExtensionNativeOwnershipPhase,
    ) -> Self {
        Self::Transition {
            expected,
            intent,
            phase,
            attach_native_identity: None,
        }
    }

    /// Transitions one exact row and atomically attaches its native identity.
    ///
    /// Attachment is one-shot. A stale replay conflicts through the row CAS;
    /// a fresh attempt to replace an attached identity is invalid.
    pub const fn transition_with_native_identity(
        expected: ExtensionNativeOwnershipEntryCas,
        intent: ExtensionNativeOwnershipIntent,
        phase: ExtensionNativeOwnershipPhase,
        native_identity: ExtensionNativeOwnershipIdentity,
    ) -> Self {
        Self::Transition {
            expected,
            intent,
            phase,
            attach_native_identity: Some(native_identity),
        }
    }

    pub const fn clear(expected: ExtensionNativeOwnershipEntryCas) -> Self {
        Self::Clear { expected }
    }

    /// Conservative heap-plus-inline actor admission charge.
    pub const fn retained_bytes(&self) -> usize {
        let bytes = size_of::<Self>()
            + match self {
                Self::Begin(_) => size_of::<ExtensionNativeOwnershipPreparation>(),
                Self::Transition { .. } | Self::Clear { .. } => 0,
            };
        debug_assert!(bytes <= MAX_EXTENSION_NATIVE_OWNERSHIP_MUTATION_RETAINED_BYTES);
        bytes
    }

    pub const fn profile(&self) -> ProfileId {
        match self {
            Self::Begin(preparation) => preparation.key.profile(),
            Self::Transition { expected, .. } | Self::Clear { expected } => expected.key.profile(),
        }
    }
}

/// Persistence action selected by the validated aggregate transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionNativeOwnershipMutationKind {
    Begin,
    Transition,
    Clear,
}

/// Validated next journal and the exact affected row, if one remains.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionNativeOwnershipJournalApplication {
    journal: ExtensionNativeOwnershipJournal,
    kind: ExtensionNativeOwnershipMutationKind,
    entry: Option<ExtensionNativeOwnershipEntry>,
}

impl ExtensionNativeOwnershipJournalApplication {
    pub const fn journal(&self) -> &ExtensionNativeOwnershipJournal {
        &self.journal
    }

    pub const fn kind(&self) -> ExtensionNativeOwnershipMutationKind {
        self.kind
    }

    pub const fn entry(&self) -> Option<&ExtensionNativeOwnershipEntry> {
        self.entry.as_ref()
    }
}

/// Complete-cohort reconstruction failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionNativeOwnershipJournalError {
    TooManyEntries {
        count: usize,
        max: usize,
    },
    DuplicateKey(ExtensionNativeOwnershipKey),
    DuplicateOperation(ExtensionNativeOwnershipOperation),
    DuplicateIncarnation(ExtensionNativeIncarnation),
    NonCanonicalOrder,
    InconsistentRevisionAuthority,
    OperationIncarnationMismatch,
    OperationAboveHighWater,
    IncarnationAboveHighWater,
    InvalidState {
        intent: ExtensionNativeOwnershipIntent,
        phase: ExtensionNativeOwnershipPhase,
    },
    InvalidEntryRevision {
        intent: ExtensionNativeOwnershipIntent,
        phase: ExtensionNativeOwnershipPhase,
        revision: ExtensionNativeOwnershipEntryRevision,
    },
    InvalidNativeIdentity,
    EntryAboveJournalRevision,
    RevisionHistoryInconsistent,
    RetainedBytesExceeded,
}

impl fmt::Display for ExtensionNativeOwnershipJournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid extension native-ownership journal: {self:?}"
        )
    }
}

impl Error for ExtensionNativeOwnershipJournalError {}

/// Pure journal mutation refusal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionNativeOwnershipApplyError {
    Conflict {
        current: ExtensionNativeOwnershipJournalRevision,
    },
    LimitReached,
    Invalid,
    RevisionExhausted,
}

fn valid_state(
    intent: ExtensionNativeOwnershipIntent,
    phase: ExtensionNativeOwnershipPhase,
) -> bool {
    matches!(
        (intent, phase),
        (
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeAbsentPreparing
                | ExtensionNativeOwnershipPhase::NativeMayOwn
                | ExtensionNativeOwnershipPhase::NativeOwned
        ) | (
            ExtensionNativeOwnershipIntent::Release,
            ExtensionNativeOwnershipPhase::NativeMayOwn
                | ExtensionNativeOwnershipPhase::NativeAbsentReleasePending
        )
    )
}

fn valid_revision_authority(
    revision: ExtensionNativeOwnershipJournalRevision,
    operation_high_water: Option<ExtensionNativeOwnershipOperation>,
    native_incarnation_high_water: Option<ExtensionNativeIncarnation>,
) -> bool {
    match (operation_high_water, native_incarnation_high_water) {
        (None, None) => revision == ExtensionNativeOwnershipJournalRevision::INITIAL,
        (Some(operation), Some(incarnation)) => {
            revision != ExtensionNativeOwnershipJournalRevision::INITIAL
                && operation.get() == incarnation.get()
                && operation.get() < revision.get()
        }
        (None, Some(_)) | (Some(_), None) => false,
    }
}

fn valid_entry_revision(
    backend: ExtensionRuntimeBackendTarget,
    native_identity: Option<ExtensionNativeOwnershipIdentity>,
    intent: ExtensionNativeOwnershipIntent,
    phase: ExtensionNativeOwnershipPhase,
    revision: ExtensionNativeOwnershipEntryRevision,
) -> bool {
    use ExtensionNativeOwnershipIntent::{Acquire, Release};
    use ExtensionNativeOwnershipPhase::{
        NativeAbsentPreparing, NativeAbsentReleasePending, NativeMayOwn, NativeOwned,
    };
    let is_native = matches!(
        backend,
        ExtensionRuntimeBackendTarget::MacosNative | ExtensionRuntimeBackendTarget::WindowsNative
    );
    let has_identity = native_identity.is_some();
    match (intent, phase, revision.get()) {
        (Acquire, NativeAbsentPreparing, 1) => !has_identity,
        (Acquire, NativeMayOwn, 2) => !has_identity,
        (Acquire, NativeMayOwn, 3) => is_native && has_identity,
        (Acquire, NativeOwned, 3) => !is_native || has_identity,
        (Acquire, NativeOwned, 4) => is_native && has_identity,
        (Release, NativeMayOwn, 3) => true,
        // v11 could reach release/may-own at revision four before a native
        // identifier existed. Preserve that conservative cleanup authority;
        // absence must never be inferred during migration or recovery.
        (Release, NativeMayOwn, 4) => true,
        (Release, NativeMayOwn, 5) => is_native && has_identity,
        (Release, NativeAbsentReleasePending, 2..=3) => !has_identity,
        (Release, NativeAbsentReleasePending, 4) => true,
        // The corresponding v11 cleanup frontier can also lack an identity.
        (Release, NativeAbsentReleasePending, 5) => true,
        (Release, NativeAbsentReleasePending, 6) => is_native && has_identity,
        _ => false,
    }
}

fn valid_native_identity(
    backend: ExtensionRuntimeBackendTarget,
    native_identity: Option<ExtensionNativeOwnershipIdentity>,
    intent: ExtensionNativeOwnershipIntent,
    phase: ExtensionNativeOwnershipPhase,
) -> bool {
    if native_identity.is_some_and(|identity| identity.backend() != backend) {
        return false;
    }
    if matches!(
        backend,
        ExtensionRuntimeBackendTarget::MacosCompatibility
            | ExtensionRuntimeBackendTarget::LinuxCompatibility
    ) && native_identity.is_some()
    {
        return false;
    }
    if intent == ExtensionNativeOwnershipIntent::Acquire
        && phase == ExtensionNativeOwnershipPhase::NativeOwned
        && matches!(
            backend,
            ExtensionRuntimeBackendTarget::MacosNative
                | ExtensionRuntimeBackendTarget::WindowsNative
        )
        && native_identity.is_none()
    {
        return false;
    }
    if phase == ExtensionNativeOwnershipPhase::NativeAbsentPreparing && native_identity.is_some() {
        return false;
    }
    true
}

fn valid_transition(
    current_intent: ExtensionNativeOwnershipIntent,
    current_phase: ExtensionNativeOwnershipPhase,
    next_intent: ExtensionNativeOwnershipIntent,
    next_phase: ExtensionNativeOwnershipPhase,
) -> bool {
    use ExtensionNativeOwnershipIntent::{Acquire, Release};
    use ExtensionNativeOwnershipPhase::{
        NativeAbsentPreparing, NativeAbsentReleasePending, NativeMayOwn, NativeOwned,
    };
    matches!(
        (current_intent, current_phase, next_intent, next_phase),
        (Acquire, NativeAbsentPreparing, Acquire, NativeMayOwn)
            | (
                Acquire,
                NativeAbsentPreparing,
                Release,
                NativeAbsentReleasePending
            )
            | (Acquire, NativeMayOwn, Acquire, NativeOwned)
            | (Acquire, NativeMayOwn, Release, NativeMayOwn)
            | (Acquire, NativeMayOwn, Release, NativeAbsentReleasePending)
            | (Acquire, NativeOwned, Release, NativeMayOwn)
            | (Release, NativeMayOwn, Release, NativeAbsentReleasePending)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::{
        ExtensionAuthorityId, ExtensionManifestDigest, ExtensionPackageKey,
        ExtensionPackagePayloadIdentity, ExtensionPackageRevision, ExtensionTreeDigest,
    };

    fn preparation(value: u128) -> ExtensionNativeOwnershipPreparation {
        ExtensionNativeOwnershipPreparation::new(
            ExtensionNativeOwnershipKey::new(
                ProfileId::from(1),
                ExtensionInstallId::from(value),
                ExtensionGrantBrowsingContext::Regular,
            ),
            ExtensionPackageIdentity::new(
                ExtensionAuthorityId::from_bytes([1; 32]),
                ExtensionPackageKey::from_bytes([2; 32]),
                ExtensionPackageRevision::INITIAL,
                ExtensionPackagePayloadIdentity::BundledTree,
                ExtensionManifestDigest::from_bytes([3; 32]),
                ExtensionTreeDigest::from_bytes([4; 32]),
            ),
            ExtensionCatalogSetDigest::from_bytes([5; 32]),
            ExtensionCatalogGenerationRole::Active,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallRevision::INITIAL,
            ExtensionGrantRevision::INITIAL,
            ExtensionGrantDigest::from_bytes([6; 32]),
            ExtensionRuntimeBackendTarget::MacosNative,
        )
    }

    fn native_identity(backend: ExtensionRuntimeBackendTarget) -> ExtensionNativeOwnershipIdentity {
        ExtensionNativeOwnershipIdentity::parse(backend, "abcdefghijklmnopabcdefghijklmnop")
            .unwrap()
    }

    fn begin(
        journal: ExtensionNativeOwnershipJournal,
        value: u128,
    ) -> ExtensionNativeOwnershipJournalApplication {
        let expected = journal.revision();
        journal
            .apply(
                expected,
                ExtensionNativeOwnershipJournalMutation::begin(preparation(value)),
            )
            .unwrap()
    }

    fn persisted_entry(
        operation: u64,
        incarnation: u64,
        entry_revision: u64,
        intent: ExtensionNativeOwnershipIntent,
        phase: ExtensionNativeOwnershipPhase,
        native_identity: Option<ExtensionNativeOwnershipIdentity>,
    ) -> Result<ExtensionNativeOwnershipEntry, ExtensionNativeOwnershipJournalError> {
        ExtensionNativeOwnershipEntry::from_persisted_with_native_identity(
            preparation(operation as u128).key(),
            ExtensionNativeOwnershipOperation::new(operation).unwrap(),
            ExtensionNativeOwnershipEntryRevision::new(entry_revision).unwrap(),
            ExtensionPackageIdentity::new(
                ExtensionAuthorityId::from_bytes([1; 32]),
                ExtensionPackageKey::from_bytes([2; 32]),
                ExtensionPackageRevision::INITIAL,
                ExtensionPackagePayloadIdentity::BundledTree,
                ExtensionManifestDigest::from_bytes([3; 32]),
                ExtensionTreeDigest::from_bytes([4; 32]),
            ),
            ExtensionCatalogSetDigest::from_bytes([5; 32]),
            ExtensionCatalogGenerationRole::Active,
            ExtensionInstallCatalogRevision::INITIAL,
            ExtensionInstallRevision::INITIAL,
            ExtensionGrantRevision::INITIAL,
            ExtensionGrantDigest::from_bytes([6; 32]),
            ExtensionRuntimeBackendTarget::MacosNative,
            native_identity,
            ExtensionNativeIncarnation::new(incarnation).unwrap(),
            intent,
            phase,
        )
    }

    #[test]
    fn operation_and_incarnation_high_waters_survive_clear() {
        let begun = begin(ExtensionNativeOwnershipJournal::empty(), 1);
        let entry = begun.entry().unwrap().clone();
        let journal = begun.journal;
        let revision = journal.revision();
        let release = journal
            .apply(
                revision,
                ExtensionNativeOwnershipJournalMutation::transition(
                    entry.cas(),
                    ExtensionNativeOwnershipIntent::Release,
                    ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
                ),
            )
            .unwrap();
        let release_entry = release.entry().unwrap().clone();
        let journal = release.journal;
        let revision = journal.revision();
        let cleared = journal
            .apply(
                revision,
                ExtensionNativeOwnershipJournalMutation::clear(release_entry.cas()),
            )
            .unwrap();
        assert!(cleared.journal.entries().is_empty());
        let second = begin(cleared.journal, 2);
        assert_eq!(second.entry().unwrap().operation().get(), 2);
        assert_eq!(second.entry().unwrap().native_incarnation().get(), 2);
    }

    #[test]
    fn journal_before_native_transition_and_release_path_are_strict() {
        let begun = begin(ExtensionNativeOwnershipJournal::empty(), 1);
        let preparing = begun.entry().unwrap().clone();
        let journal = begun.journal;
        let revision = journal.revision();
        let may_own = journal
            .apply(
                revision,
                ExtensionNativeOwnershipJournalMutation::transition(
                    preparing.cas(),
                    ExtensionNativeOwnershipIntent::Acquire,
                    ExtensionNativeOwnershipPhase::NativeMayOwn,
                ),
            )
            .unwrap();
        let may_own_entry = may_own.entry().unwrap().clone();
        let journal = may_own.journal;
        let revision = journal.revision();
        let owned = journal
            .apply(
                revision,
                ExtensionNativeOwnershipJournalMutation::transition_with_native_identity(
                    may_own_entry.cas(),
                    ExtensionNativeOwnershipIntent::Acquire,
                    ExtensionNativeOwnershipPhase::NativeOwned,
                    native_identity(ExtensionRuntimeBackendTarget::MacosNative),
                ),
            )
            .unwrap();
        let owned_entry = owned.entry().unwrap().clone();
        assert_eq!(
            owned_entry.phase(),
            ExtensionNativeOwnershipPhase::NativeOwned
        );
        assert_eq!(
            owned.journal.clone().apply(
                owned.journal.revision(),
                ExtensionNativeOwnershipJournalMutation::clear(owned_entry.cas())
            ),
            Err(ExtensionNativeOwnershipApplyError::Invalid)
        );
    }

    #[test]
    fn global_and_exact_row_cas_reject_stale_operations() {
        let begun = begin(ExtensionNativeOwnershipJournal::empty(), 1);
        let stale = begun.entry().unwrap().cas();
        let journal = begun.journal;
        assert_eq!(
            journal.clone().apply(
                ExtensionNativeOwnershipJournalRevision::INITIAL,
                ExtensionNativeOwnershipJournalMutation::transition(
                    stale,
                    ExtensionNativeOwnershipIntent::Acquire,
                    ExtensionNativeOwnershipPhase::NativeMayOwn,
                )
            ),
            Err(ExtensionNativeOwnershipApplyError::Conflict {
                current: journal.revision()
            })
        );
    }

    #[test]
    fn reconstruction_rejects_duplicate_and_noncanonical_rows() {
        let first = begin(ExtensionNativeOwnershipJournal::empty(), 1);
        let first_entry = first.entry().unwrap().clone();
        let second = begin(first.journal, 2);
        let mut entries = second.journal.entries().to_vec();
        entries.swap(0, 1);
        assert_eq!(
            ExtensionNativeOwnershipJournal::from_persisted(
                second.journal.revision(),
                second.journal.operation_high_water(),
                second.journal.native_incarnation_high_water(),
                entries,
            ),
            Err(ExtensionNativeOwnershipJournalError::NonCanonicalOrder)
        );
        assert_eq!(
            ExtensionNativeOwnershipJournal::from_persisted(
                second.journal.revision(),
                second.journal.operation_high_water(),
                second.journal.native_incarnation_high_water(),
                vec![first_entry.clone(), first_entry],
            ),
            Err(ExtensionNativeOwnershipJournalError::DuplicateKey(
                preparation(1).key()
            ))
        );
    }

    #[test]
    fn persisted_owned_phase_is_preserved_exactly() {
        let begun = begin(ExtensionNativeOwnershipJournal::empty(), 1);
        let preparing = begun.entry().unwrap().clone();
        let journal = begun.journal;
        let revision = journal.revision();
        let may_own = journal
            .apply(
                revision,
                ExtensionNativeOwnershipJournalMutation::transition(
                    preparing.cas(),
                    ExtensionNativeOwnershipIntent::Acquire,
                    ExtensionNativeOwnershipPhase::NativeMayOwn,
                ),
            )
            .unwrap();
        let current = may_own.entry().unwrap().clone();
        let journal = may_own.journal;
        let revision = journal.revision();
        let owned = journal
            .apply(
                revision,
                ExtensionNativeOwnershipJournalMutation::transition_with_native_identity(
                    current.cas(),
                    ExtensionNativeOwnershipIntent::Acquire,
                    ExtensionNativeOwnershipPhase::NativeOwned,
                    native_identity(ExtensionRuntimeBackendTarget::MacosNative),
                ),
            )
            .unwrap();
        assert_eq!(
            owned.journal.entries()[0].phase(),
            ExtensionNativeOwnershipPhase::NativeOwned
        );
    }

    #[test]
    fn reconstruction_rejects_impossible_revision_authority_and_clock_reuse() {
        assert_eq!(
            ExtensionNativeOwnershipJournal::from_persisted(
                ExtensionNativeOwnershipJournalRevision::INITIAL,
                Some(ExtensionNativeOwnershipOperation::INITIAL),
                Some(ExtensionNativeIncarnation::INITIAL),
                Vec::new(),
            ),
            Err(ExtensionNativeOwnershipJournalError::InconsistentRevisionAuthority)
        );
        assert_eq!(
            ExtensionNativeOwnershipJournal::from_persisted(
                ExtensionNativeOwnershipJournalRevision::new(2).unwrap(),
                Some(ExtensionNativeOwnershipOperation::INITIAL),
                Some(ExtensionNativeIncarnation::new(2).unwrap()),
                Vec::new(),
            ),
            Err(ExtensionNativeOwnershipJournalError::InconsistentRevisionAuthority)
        );
        assert_eq!(
            persisted_entry(
                1,
                2,
                1,
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeAbsentPreparing,
                None,
            ),
            Err(ExtensionNativeOwnershipJournalError::OperationIncarnationMismatch)
        );
    }

    #[test]
    fn entry_revision_is_exact_for_its_reachable_phase() {
        assert_eq!(
            persisted_entry(
                1,
                1,
                1,
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeOwned,
                Some(native_identity(ExtensionRuntimeBackendTarget::MacosNative)),
            ),
            Err(ExtensionNativeOwnershipJournalError::InvalidEntryRevision {
                intent: ExtensionNativeOwnershipIntent::Acquire,
                phase: ExtensionNativeOwnershipPhase::NativeOwned,
                revision: ExtensionNativeOwnershipEntryRevision::INITIAL,
            })
        );
        for (intent, phase, revision, identity) in [
            (
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeAbsentPreparing,
                1,
                None,
            ),
            (
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeMayOwn,
                2,
                None,
            ),
            (
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeOwned,
                3,
                Some(native_identity(ExtensionRuntimeBackendTarget::MacosNative)),
            ),
            (
                ExtensionNativeOwnershipIntent::Release,
                ExtensionNativeOwnershipPhase::NativeMayOwn,
                4,
                None,
            ),
            (
                ExtensionNativeOwnershipIntent::Release,
                ExtensionNativeOwnershipPhase::NativeMayOwn,
                4,
                Some(native_identity(ExtensionRuntimeBackendTarget::MacosNative)),
            ),
            (
                ExtensionNativeOwnershipIntent::Release,
                ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
                5,
                None,
            ),
            (
                ExtensionNativeOwnershipIntent::Release,
                ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
                5,
                Some(native_identity(ExtensionRuntimeBackendTarget::MacosNative)),
            ),
        ] {
            assert!(persisted_entry(1, 1, revision, intent, phase, identity).is_ok());
        }
    }

    #[test]
    fn reconstruction_enforces_exact_reachable_global_revision_range() {
        let begun = begin(ExtensionNativeOwnershipJournal::empty(), 1);
        let preparing = begun.entry().unwrap().clone();
        assert_eq!(
            ExtensionNativeOwnershipJournal::from_persisted(
                ExtensionNativeOwnershipJournalRevision::new(3).unwrap(),
                Some(ExtensionNativeOwnershipOperation::INITIAL),
                Some(ExtensionNativeIncarnation::INITIAL),
                vec![preparing],
            ),
            Err(ExtensionNativeOwnershipJournalError::RevisionHistoryInconsistent)
        );
        assert!(ExtensionNativeOwnershipJournal::from_persisted(
            ExtensionNativeOwnershipJournalRevision::new(4).unwrap(),
            Some(ExtensionNativeOwnershipOperation::INITIAL),
            Some(ExtensionNativeIncarnation::INITIAL),
            Vec::new(),
        )
        .is_ok());
        for impossible in [3, 9] {
            assert_eq!(
                ExtensionNativeOwnershipJournal::from_persisted(
                    ExtensionNativeOwnershipJournalRevision::new(impossible).unwrap(),
                    Some(ExtensionNativeOwnershipOperation::INITIAL),
                    Some(ExtensionNativeIncarnation::INITIAL),
                    Vec::new(),
                ),
                Err(ExtensionNativeOwnershipJournalError::RevisionHistoryInconsistent)
            );
        }
    }

    #[test]
    fn maximum_identity_history_clears_at_exact_revision_eight() {
        let begun = begin(ExtensionNativeOwnershipJournal::empty(), 1);
        let current = begun.entry().unwrap().clone();
        let revision = begun.journal.revision();
        let may_own = begun
            .journal
            .apply(
                revision,
                ExtensionNativeOwnershipJournalMutation::transition(
                    current.cas(),
                    ExtensionNativeOwnershipIntent::Acquire,
                    ExtensionNativeOwnershipPhase::NativeMayOwn,
                ),
            )
            .unwrap();
        let current = may_own.entry().unwrap().clone();
        let revision = may_own.journal.revision();
        let identified = may_own
            .journal
            .apply(
                revision,
                ExtensionNativeOwnershipJournalMutation::transition_with_native_identity(
                    current.cas(),
                    ExtensionNativeOwnershipIntent::Acquire,
                    ExtensionNativeOwnershipPhase::NativeMayOwn,
                    native_identity(ExtensionRuntimeBackendTarget::MacosNative),
                ),
            )
            .unwrap();
        let current = identified.entry().unwrap().clone();
        let revision = identified.journal.revision();
        let owned = identified
            .journal
            .apply(
                revision,
                ExtensionNativeOwnershipJournalMutation::transition(
                    current.cas(),
                    ExtensionNativeOwnershipIntent::Acquire,
                    ExtensionNativeOwnershipPhase::NativeOwned,
                ),
            )
            .unwrap();
        let current = owned.entry().unwrap().clone();
        let revision = owned.journal.revision();
        let releasing = owned
            .journal
            .apply(
                revision,
                ExtensionNativeOwnershipJournalMutation::transition(
                    current.cas(),
                    ExtensionNativeOwnershipIntent::Release,
                    ExtensionNativeOwnershipPhase::NativeMayOwn,
                ),
            )
            .unwrap();
        let current = releasing.entry().unwrap().clone();
        let revision = releasing.journal.revision();
        let absent = releasing
            .journal
            .apply(
                revision,
                ExtensionNativeOwnershipJournalMutation::transition(
                    current.cas(),
                    ExtensionNativeOwnershipIntent::Release,
                    ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
                ),
            )
            .unwrap();
        let current = absent.entry().unwrap().clone();
        let revision = absent.journal.revision();
        let cleared = absent
            .journal
            .apply(
                revision,
                ExtensionNativeOwnershipJournalMutation::clear(current.cas()),
            )
            .unwrap();

        assert!(cleared.journal.entries().is_empty());
        assert_eq!(cleared.journal.revision().get(), 8);
        assert!(ExtensionNativeOwnershipJournal::from_persisted(
            ExtensionNativeOwnershipJournalRevision::new(8).unwrap(),
            Some(ExtensionNativeOwnershipOperation::INITIAL),
            Some(ExtensionNativeIncarnation::INITIAL),
            Vec::new(),
        )
        .is_ok());
        assert_eq!(
            ExtensionNativeOwnershipJournal::from_persisted(
                ExtensionNativeOwnershipJournalRevision::new(9).unwrap(),
                Some(ExtensionNativeOwnershipOperation::INITIAL),
                Some(ExtensionNativeIncarnation::INITIAL),
                Vec::new(),
            ),
            Err(ExtensionNativeOwnershipJournalError::RevisionHistoryInconsistent)
        );
    }

    #[test]
    fn native_identity_is_closed_backend_bound_and_redacted() {
        let value = "abcdefghijklmnopabcdefghijklmnop";
        let macos = ExtensionNativeOwnershipIdentity::parse(
            ExtensionRuntimeBackendTarget::MacosNative,
            value,
        )
        .unwrap();
        assert_eq!(macos.persisted_kind(), 1);
        assert_eq!(macos.backend(), ExtensionRuntimeBackendTarget::MacosNative);
        assert!(!format!("{macos:?}").contains(value));
        assert_eq!(
            ExtensionNativeOwnershipIdentity::parse(
                ExtensionRuntimeBackendTarget::LinuxCompatibility,
                value,
            ),
            Err(ExtensionNativeOwnershipIdentityError::UnsupportedBackend)
        );
        for malformed in [
            "abcdefghijklmnopabcdefghijklmn",
            "abcdefghijklmnopabcdefghijklmnopq",
            "Abcdefghijklmnopabcdefghijklmnop",
            "abcdefghijklmnopabcdefghijklmn0p",
        ] {
            assert_eq!(
                ExtensionNativeOwnershipIdentity::parse(
                    ExtensionRuntimeBackendTarget::WindowsNative,
                    malformed,
                ),
                Err(ExtensionNativeOwnershipIdentityError::InvalidIdentifier)
            );
        }
        assert_eq!(
            ExtensionNativeOwnershipIdentity::from_persisted(3, [b'a'; 32]),
            Err(ExtensionNativeOwnershipIdentityError::UnknownKind)
        );
    }

    #[test]
    fn native_identity_attachment_is_atomic_immutable_and_exact_cas() {
        let begun = begin(ExtensionNativeOwnershipJournal::empty(), 1);
        let preparing = begun.entry().unwrap().clone();
        let revision = begun.journal.revision();
        let may_own = begun
            .journal
            .apply(
                revision,
                ExtensionNativeOwnershipJournalMutation::transition(
                    preparing.cas(),
                    ExtensionNativeOwnershipIntent::Acquire,
                    ExtensionNativeOwnershipPhase::NativeMayOwn,
                ),
            )
            .unwrap();
        let before_attach = may_own.entry().unwrap().clone();
        assert_eq!(before_attach.native_identity(), None);
        let identity = native_identity(ExtensionRuntimeBackendTarget::MacosNative);
        let revision = may_own.journal.revision();
        let owned = may_own
            .journal
            .apply(
                revision,
                ExtensionNativeOwnershipJournalMutation::transition_with_native_identity(
                    before_attach.cas(),
                    ExtensionNativeOwnershipIntent::Acquire,
                    ExtensionNativeOwnershipPhase::NativeOwned,
                    identity,
                ),
            )
            .unwrap();
        let owned_entry = owned.entry().unwrap().clone();
        assert_eq!(owned_entry.native_identity(), Some(identity));
        assert_eq!(owned_entry.revision().get(), 3);

        assert_eq!(
            owned.journal.clone().apply(
                owned.journal.revision(),
                ExtensionNativeOwnershipJournalMutation::transition_with_native_identity(
                    owned_entry.cas(),
                    ExtensionNativeOwnershipIntent::Release,
                    ExtensionNativeOwnershipPhase::NativeMayOwn,
                    native_identity(ExtensionRuntimeBackendTarget::MacosNative),
                ),
            ),
            Err(ExtensionNativeOwnershipApplyError::Invalid)
        );
        assert_eq!(
            owned.journal.clone().apply(
                owned.journal.revision(),
                ExtensionNativeOwnershipJournalMutation::transition_with_native_identity(
                    before_attach.cas(),
                    ExtensionNativeOwnershipIntent::Acquire,
                    ExtensionNativeOwnershipPhase::NativeOwned,
                    identity,
                ),
            ),
            Err(ExtensionNativeOwnershipApplyError::Conflict {
                current: owned.journal.revision(),
            })
        );
    }

    #[test]
    fn release_recovery_can_attach_once_without_claiming_owned() {
        let begun = begin(ExtensionNativeOwnershipJournal::empty(), 1);
        let preparing = begun.entry().unwrap().clone();
        let revision = begun.journal.revision();
        let may_own = begun
            .journal
            .apply(
                revision,
                ExtensionNativeOwnershipJournalMutation::transition(
                    preparing.cas(),
                    ExtensionNativeOwnershipIntent::Acquire,
                    ExtensionNativeOwnershipPhase::NativeMayOwn,
                ),
            )
            .unwrap();
        let acquiring = may_own.entry().unwrap().clone();
        let revision = may_own.journal.revision();
        let releasing = may_own
            .journal
            .apply(
                revision,
                ExtensionNativeOwnershipJournalMutation::transition(
                    acquiring.cas(),
                    ExtensionNativeOwnershipIntent::Release,
                    ExtensionNativeOwnershipPhase::NativeMayOwn,
                ),
            )
            .unwrap();
        let release_without_identity = releasing.entry().unwrap().clone();
        let identity = native_identity(ExtensionRuntimeBackendTarget::MacosNative);
        let revision = releasing.journal.revision();
        let identified = releasing
            .journal
            .apply(
                revision,
                ExtensionNativeOwnershipJournalMutation::transition_with_native_identity(
                    release_without_identity.cas(),
                    ExtensionNativeOwnershipIntent::Release,
                    ExtensionNativeOwnershipPhase::NativeMayOwn,
                    identity,
                ),
            )
            .unwrap();
        let identified_entry = identified.entry().unwrap().clone();
        assert_eq!(identified_entry.native_identity(), Some(identity));
        assert_eq!(
            identified_entry.intent(),
            ExtensionNativeOwnershipIntent::Release
        );
        assert_eq!(
            identified_entry.phase(),
            ExtensionNativeOwnershipPhase::NativeMayOwn
        );
        let revision = identified.journal.revision();
        let absent = identified
            .journal
            .apply(
                revision,
                ExtensionNativeOwnershipJournalMutation::transition(
                    identified_entry.cas(),
                    ExtensionNativeOwnershipIntent::Release,
                    ExtensionNativeOwnershipPhase::NativeAbsentReleasePending,
                ),
            )
            .unwrap();
        assert_eq!(absent.entry().unwrap().native_identity(), Some(identity));
    }

    #[test]
    fn persisted_identity_cannot_cross_backend_or_infer_owned_state() {
        let windows_identity = native_identity(ExtensionRuntimeBackendTarget::WindowsNative);
        assert_eq!(
            persisted_entry(
                1,
                1,
                3,
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeOwned,
                Some(windows_identity),
            ),
            Err(ExtensionNativeOwnershipJournalError::InvalidNativeIdentity)
        );
        assert_eq!(
            persisted_entry(
                1,
                1,
                3,
                ExtensionNativeOwnershipIntent::Acquire,
                ExtensionNativeOwnershipPhase::NativeOwned,
                None,
            ),
            Err(ExtensionNativeOwnershipJournalError::InvalidNativeIdentity)
        );
    }
}
