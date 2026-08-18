//! Profile-scoped, store-snapshot runtime eligibility for extensions.
//!
//! Eligibility is deliberately weaker than activation authority. It proves
//! only that one exact install, admitted structural manifest, and grant row
//! came from the same complete store cohort and currently satisfy the durable
//! user-intent prerequisites. An authenticated repository package lease and a
//! backend runtime admission are still required before any native execution.

use std::cell::Cell;
use std::fmt;
use std::marker::PhantomData;
use std::mem::size_of;
use std::sync::{Arc, OnceLock};

use url::Url;

use crate::ids::{ExtensionInstallId, ProfileId};

use super::cohort::ExtensionGrantCohortEntry;
use super::transient::ExtensionRuntimeFingerprintInput;
use super::{
    ApiPermissionName, ExtensionApiGrantDecision, ExtensionCatalogGenerationRole,
    ExtensionCatalogSetDigest, ExtensionCompatibilityBrokerPurpose,
    ExtensionCompatibilityBrokerWitness, ExtensionDocumentPurpose, ExtensionGrantAuthority,
    ExtensionGrantBrowsingContext, ExtensionGrantDigest, ExtensionGrantRevision, ExtensionInstall,
    ExtensionInstallCatalogRevision, ExtensionInstallRevision, ExtensionManifestDescriptor,
    ExtensionNativeGrantProjection, ExtensionNativeIncarnation, ExtensionNativeOwnershipEntry,
    ExtensionNativeOwnershipIntent, ExtensionNativeOwnershipOperation,
    ExtensionNativeOwnershipPhase, ExtensionPackageIdentity, ExtensionRuntimeBackendTarget,
    ExtensionRuntimeFingerprint, ExtensionRuntimeGeneration, ExtensionRuntimeInstance,
    ExtensionUrlScopeDecision, ExtensionUserInvocationKind,
    MACOS_NATIVE_BROKERED_COMPATIBILITY_TARGET,
};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) struct ExtensionRuntimeOperationAuthorityLineage {
    catalog_set_digest: ExtensionCatalogSetDigest,
    catalog_role: ExtensionCatalogGenerationRole,
    runtime_backend: ExtensionRuntimeBackendTarget,
    operation: ExtensionNativeOwnershipOperation,
    native_incarnation: ExtensionNativeIncarnation,
}

impl ExtensionRuntimeOperationAuthorityLineage {
    pub(super) fn from_entry(entry: &ExtensionNativeOwnershipEntry) -> Self {
        Self {
            catalog_set_digest: entry.catalog_set_digest(),
            catalog_role: entry.catalog_role(),
            runtime_backend: entry.runtime_backend(),
            operation: entry.operation(),
            native_incarnation: entry.native_incarnation(),
        }
    }

    fn matches_entry(self, entry: &ExtensionNativeOwnershipEntry) -> bool {
        self.catalog_set_digest == entry.catalog_set_digest()
            && self.catalog_role == entry.catalog_role()
            && self.runtime_backend == entry.runtime_backend()
            && self.operation == entry.operation()
            && self.native_incarnation == entry.native_incarnation()
    }
}

/// Stable, path-free denial from minting an extension operation capability.
///
/// This deliberately does not retain permission names, URLs, package data, or
/// filesystem state. Callers may project it into diagnostics without leaking
/// extension or browsing data across trust boundaries.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum ExtensionOperationAuthorityDenial {
    /// The submitted runtime is not the exact durable snapshot represented by
    /// the eligibility that was asked to mint the capability.
    RuntimeFingerprintMismatch,
    /// The exact closed operation's manifest API is not effectively granted.
    RequiredAuthorityMissing,
}

/// Stable, data-free reason a live runtime grant authority was not rebound.
///
/// A refusal never consumes or weakens the currently published operation
/// authority. The caller receives the proposed Store eligibility back through
/// [`ExtensionRuntimeGrantRebindRefusal`] and may retire the runtime or retry
/// from a fresh durable cohort.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimeGrantRebindDenial {
    /// The supplied current row is not the authority's exact owned frontier.
    CurrentAuthorityMismatch,
    /// The two rows do not describe one grant-only journal transition.
    InvalidOwnershipTransition,
    /// The proposed eligibility is not the exact next durable grant cohort.
    ReplacementAuthorityMismatch,
    /// Live replacement attempted to remove authority or change file/private
    /// access; those changes require full native retirement.
    NonMonotonicGrantChange,
}

/// Lossless refusal to replace one live runtime's grant authority.
#[must_use = "grant-rebind refusal retains the proposed Store eligibility"]
pub struct ExtensionRuntimeGrantRebindRefusal {
    reason: ExtensionRuntimeGrantRebindDenial,
    eligibility: Box<ExtensionRuntimeEligibility>,
}

impl ExtensionRuntimeGrantRebindRefusal {
    /// Stable refusal reason without package, permission, or profile data.
    pub const fn reason(&self) -> ExtensionRuntimeGrantRebindDenial {
        self.reason
    }

    /// Returns the exact unconsumed replacement eligibility.
    pub fn into_eligibility(self) -> ExtensionRuntimeEligibility {
        *self.eligibility
    }
}

impl fmt::Debug for ExtensionRuntimeGrantRebindRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeGrantRebindRefusal")
            .field("reason", &self.reason)
            .field("eligibility", &"<redacted>")
            .finish()
    }
}

/// Move-only capability for one trusted browser invocation to establish
/// transient `activeTab` scope for one exact runtime.
///
/// This type has no public constructor and is minted only by
/// [`ExtensionRuntimeOperationAuthority::mint_active_tab_grant_witness`]. Identity
/// getters remain non-authorizing: the engine must require possession of this
/// value and join it with its own trusted user gesture, current native runtime
/// owner, and committed document.
///
/// The capability deliberately cannot be cloned:
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionActiveTabGrantWitness;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionActiveTabGrantWitness>();
/// ```
///
/// Its fields are not a public construction surface:
///
/// ```compile_fail
/// use zephium_core::extensions::{
///     ExtensionActiveTabGrantWitness, ExtensionUserInvocationKind,
/// };
/// let _ = ExtensionActiveTabGrantWitness {
///     runtime: panic!("not constructible"),
///     invocation: ExtensionUserInvocationKind::ToolbarAction,
/// };
/// ```
///
/// It is process-local authority and cannot cross a serialization boundary:
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionActiveTabGrantWitness;
/// fn requires_serialize<T: serde::Serialize>() {}
/// requires_serialize::<ExtensionActiveTabGrantWitness>();
/// ```
#[must_use = "activeTab authority must be joined with a trusted engine gesture"]
pub struct ExtensionActiveTabGrantWitness {
    runtime: ExtensionRuntimeFingerprint,
    invocation: ExtensionUserInvocationKind,
}

impl ExtensionActiveTabGrantWitness {
    /// Non-authorizing identity of the exact runtime bound by this capability.
    pub const fn runtime(&self) -> &ExtensionRuntimeFingerprint {
        &self.runtime
    }

    /// Compact non-authorizing runtime key for native owner lookup.
    pub const fn runtime_instance(&self) -> ExtensionRuntimeInstance {
        self.runtime.instance()
    }

    /// Closed trusted invocation bound by this capability.
    pub const fn invocation(&self) -> ExtensionUserInvocationKind {
        self.invocation
    }

    /// Checks the complete runtime and invocation at the consuming boundary.
    ///
    /// Calling this method requires possession of the capability. A bare
    /// fingerprint or identity getter cannot reproduce the proof.
    pub fn matches(
        &self,
        runtime: &ExtensionRuntimeFingerprint,
        invocation: ExtensionUserInvocationKind,
    ) -> bool {
        &self.runtime == runtime && self.invocation == invocation
    }
}

impl fmt::Debug for ExtensionActiveTabGrantWitness {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionActiveTabGrantWitness")
            .field("runtime", &"<redacted>")
            .field("invocation", &self.invocation)
            .finish()
    }
}

/// Move-only purpose capability for one exact extension runtime.
///
/// Minting proves the closed purpose's API grant. It intentionally does not
/// accept or bind a URL: the engine must derive the currently committed
/// [`Url`] from its own view state and call
/// [`Self::decide_engine_document_url_scope`] at the operation boundary. The
/// returned host-scope decision must still be joined with native document
/// identity or valid transient `activeTab` scope.
///
/// The capability deliberately cannot be cloned:
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionDocumentAuthorityWitness;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionDocumentAuthorityWitness>();
/// ```
///
/// Its fields are not a public construction surface:
///
/// ```compile_fail
/// use zephium_core::extensions::{
///     ExtensionDocumentAuthorityWitness, ExtensionDocumentPurpose,
/// };
/// let _ = ExtensionDocumentAuthorityWitness {
///     runtime: panic!("not constructible"),
///     purpose: ExtensionDocumentPurpose::ExecuteScript,
///     manifest: panic!("not constructible"),
///     grants: panic!("not constructible"),
/// };
/// ```
///
/// It is process-local authority and cannot cross a serialization boundary:
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionDocumentAuthorityWitness;
/// fn requires_serialize<T: serde::Serialize>() {}
/// requires_serialize::<ExtensionDocumentAuthorityWitness>();
/// ```
#[must_use = "document authority must be joined with exact engine document state"]
pub struct ExtensionDocumentAuthorityWitness {
    runtime: ExtensionRuntimeFingerprint,
    purpose: ExtensionDocumentPurpose,
    manifest: Arc<ExtensionManifestDescriptor>,
    grants: Arc<ExtensionGrantAuthority>,
}

impl ExtensionDocumentAuthorityWitness {
    /// Non-authorizing identity of the exact runtime bound by this capability.
    pub const fn runtime(&self) -> &ExtensionRuntimeFingerprint {
        &self.runtime
    }

    /// Compact non-authorizing runtime key for native owner lookup.
    pub const fn runtime_instance(&self) -> ExtensionRuntimeInstance {
        self.runtime.instance()
    }

    /// Closed operation purpose whose API authority was proven at minting.
    pub const fn purpose(&self) -> ExtensionDocumentPurpose {
        self.purpose
    }

    /// Checks the complete runtime and purpose at the consuming boundary.
    pub fn matches(
        &self,
        runtime: &ExtensionRuntimeFingerprint,
        purpose: ExtensionDocumentPurpose,
    ) -> bool {
        &self.runtime == runtime && self.purpose == purpose
    }

    /// Evaluates durable host scope for the engine's exact committed URL.
    ///
    /// The retained manifest and grant owners are the same owners that minted
    /// this capability. This delegates to the complete grant decision, so the
    /// independent file and private-browsing gates remain enforced. `InScope`
    /// is not sufficient by itself: native document identity and the closed
    /// purpose must still be validated by the engine.
    pub fn decide_engine_document_url_scope(
        &self,
        engine_document_url: &Url,
    ) -> ExtensionUrlScopeDecision {
        self.grants.decide_url_scope(
            &self.manifest,
            engine_document_url,
            self.runtime.browsing_context(),
        )
    }
}

impl fmt::Debug for ExtensionDocumentAuthorityWitness {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionDocumentAuthorityWitness")
            .field("runtime", &"<redacted>")
            .field("purpose", &self.purpose)
            .field("authority", &"<redacted>")
            .finish()
    }
}

fn active_tab_api_permission() -> &'static ApiPermissionName {
    static ACTIVE_TAB: OnceLock<ApiPermissionName> = OnceLock::new();
    ACTIVE_TAB.get_or_init(|| {
        ApiPermissionName::parse_exact("activeTab")
            .expect("the closed activeTab permission token must remain valid")
    })
}

fn scripting_api_permission() -> &'static ApiPermissionName {
    static SCRIPTING: OnceLock<ApiPermissionName> = OnceLock::new();
    SCRIPTING.get_or_init(|| {
        ApiPermissionName::parse_exact("scripting")
            .expect("the closed scripting permission token must remain valid")
    })
}

fn history_api_permission() -> &'static ApiPermissionName {
    static HISTORY: OnceLock<ApiPermissionName> = OnceLock::new();
    HISTORY.get_or_init(|| {
        ApiPermissionName::parse_exact("history")
            .expect("the closed history permission token must remain valid")
    })
}

fn search_api_permission() -> &'static ApiPermissionName {
    static SEARCH: OnceLock<ApiPermissionName> = OnceLock::new();
    SEARCH.get_or_init(|| {
        ApiPermissionName::parse_exact("search")
            .expect("the closed search permission token must remain valid")
    })
}

fn sessions_api_permission() -> &'static ApiPermissionName {
    static SESSIONS: OnceLock<ApiPermissionName> = OnceLock::new();
    SESSIONS.get_or_init(|| {
        ApiPermissionName::parse_exact("sessions")
            .expect("the closed sessions permission token must remain valid")
    })
}

fn compatibility_broker_api_permission(
    purpose: ExtensionCompatibilityBrokerPurpose,
) -> &'static ApiPermissionName {
    match purpose {
        ExtensionCompatibilityBrokerPurpose::RecentHistory => history_api_permission(),
        ExtensionCompatibilityBrokerPurpose::DefaultSearch => search_api_permission(),
        ExtensionCompatibilityBrokerPurpose::RestoreRecentSession => sessions_api_permission(),
    }
}

fn invocation_api_permission(
    invocation: ExtensionUserInvocationKind,
) -> &'static ApiPermissionName {
    match invocation {
        ExtensionUserInvocationKind::ToolbarAction => active_tab_api_permission(),
    }
}

fn document_purpose_api_permission(
    purpose: ExtensionDocumentPurpose,
) -> &'static ApiPermissionName {
    match purpose {
        ExtensionDocumentPurpose::ExecuteScript
        | ExtensionDocumentPurpose::InsertCss
        | ExtensionDocumentPurpose::RemoveCss => scripting_api_permission(),
    }
}

/// Exact fail-closed reason one atomic profile cohort cannot yield runtime
/// eligibility for an install.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimeEligibilityDenial {
    /// The complete catalog snapshot contains no such live install.
    InstallNotFound,
    /// Durable user intent currently requests that the install remain off.
    Disabled,
    /// No exact package-bound grant root has been initialized.
    GrantsUninitialized,
    /// At least one required manifest API or host declaration is not granted.
    RequiredAuthorityMissing,
    /// Private execution remains disabled until incognito manifest semantics,
    /// storage separation, and native process isolation are modeled together.
    PrivateBrowsingUnsupported,
}

/// Structural refusal to project an exact successful grant mutation directly
/// into runtime eligibility.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum ExtensionCommittedRuntimeEligibilityError {
    /// Install, manifest, and grant authority do not name one package/install.
    CohortMismatch,
    /// Durable user intent no longer requests an enabled runtime.
    Disabled,
    /// A required manifest declaration is unexpectedly absent.
    RequiredAuthorityMissing,
    /// Private execution has not passed its independent isolation gates.
    PrivateBrowsingUnsupported,
}

/// Owned, bounded eligibility projected from one complete atomic store cohort.
///
/// This value pins the exact manifest and grant owners without deep-cloning
/// compiled matchers. It binds all durable revisions needed to invalidate a
/// later runtime generation, but it does not authenticate package bytes,
/// materialize a tree, authorize an internal scheme, or prove native
/// activation. It intentionally does not implement `Clone`.
#[must_use = "runtime eligibility must be joined with repository and native authority"]
pub struct ExtensionRuntimeEligibility {
    profile: ProfileId,
    catalog_revision: ExtensionInstallCatalogRevision,
    install_id: ExtensionInstallId,
    install_revision: ExtensionInstallRevision,
    grant_revision: ExtensionGrantRevision,
    grant_digest: ExtensionGrantDigest,
    browsing_context: ExtensionGrantBrowsingContext,
    manifest: Arc<ExtensionManifestDescriptor>,
    grants: Arc<ExtensionGrantAuthority>,
}

impl ExtensionRuntimeEligibility {
    pub(super) fn from_entry(
        entry: ExtensionGrantCohortEntry<'_>,
        browsing_context: ExtensionGrantBrowsingContext,
    ) -> Result<Self, ExtensionRuntimeEligibilityDenial> {
        let install = entry.install();
        if !install.desired_enabled() {
            return Err(ExtensionRuntimeEligibilityDenial::Disabled);
        }
        let grants = entry
            .authority_arc()
            .ok_or(ExtensionRuntimeEligibilityDenial::GrantsUninitialized)?;
        if !grants.has_required_api_and_host_grants_for(entry.manifest()) {
            return Err(ExtensionRuntimeEligibilityDenial::RequiredAuthorityMissing);
        }
        if browsing_context == ExtensionGrantBrowsingContext::Private {
            return Err(ExtensionRuntimeEligibilityDenial::PrivateBrowsingUnsupported);
        }

        Ok(Self {
            profile: entry.profile(),
            catalog_revision: entry.catalog_revision(),
            install_id: install.id(),
            install_revision: install.revision(),
            grant_revision: grants.revision(),
            grant_digest: grants.digest(),
            browsing_context,
            manifest: Arc::clone(entry.manifest_arc()),
            grants: Arc::clone(grants),
        })
    }

    /// Projects the exact result of one already-committed grant transaction.
    ///
    /// This avoids a second Store round trip in the narrow interval where the
    /// profile grant database has advanced but the native-ownership journal
    /// and in-memory operation authority have not. All inputs are the owned,
    /// validated values returned by that transaction plus the same admitted
    /// manifest. The result remains structural and non-authorizing.
    pub fn from_committed_grant_authority(
        profile: ProfileId,
        catalog_revision: ExtensionInstallCatalogRevision,
        install: ExtensionInstall,
        manifest: Arc<ExtensionManifestDescriptor>,
        grants: ExtensionGrantAuthority,
        browsing_context: ExtensionGrantBrowsingContext,
    ) -> Result<Self, ExtensionCommittedRuntimeEligibilityError> {
        if install.id() != grants.install_id()
            || install.package() != manifest.package()
            || grants.package() != manifest.package()
        {
            return Err(ExtensionCommittedRuntimeEligibilityError::CohortMismatch);
        }
        if !install.desired_enabled() {
            return Err(ExtensionCommittedRuntimeEligibilityError::Disabled);
        }
        if !grants.has_required_api_and_host_grants_for(&manifest) {
            return Err(ExtensionCommittedRuntimeEligibilityError::RequiredAuthorityMissing);
        }
        if browsing_context == ExtensionGrantBrowsingContext::Private {
            return Err(ExtensionCommittedRuntimeEligibilityError::PrivateBrowsingUnsupported);
        }
        let grant_revision = grants.revision();
        let grant_digest = grants.digest();
        Ok(Self {
            profile,
            catalog_revision,
            install_id: install.id(),
            install_revision: install.revision(),
            grant_revision,
            grant_digest,
            browsing_context,
            manifest,
            grants: Arc::new(grants),
        })
    }

    /// Exact durable profile that owns this install and grant snapshot.
    pub const fn profile(&self) -> ProfileId {
        self.profile
    }

    /// Complete install-catalog revision observed with this eligibility.
    pub const fn catalog_revision(&self) -> ExtensionInstallCatalogRevision {
        self.catalog_revision
    }

    /// Stable profile-scoped installation identity.
    pub const fn install_id(&self) -> ExtensionInstallId {
        self.install_id
    }

    /// Exact durable install-row revision.
    pub const fn install_revision(&self) -> ExtensionInstallRevision {
        self.install_revision
    }

    /// Exact durable grant-row revision.
    pub const fn grant_revision(&self) -> ExtensionGrantRevision {
        self.grant_revision
    }

    /// Exact complete grant-authority digest observed at the same revision.
    pub const fn grant_digest(&self) -> ExtensionGrantDigest {
        self.grant_digest
    }

    /// Browsing partition whose grant decisions this value may evaluate.
    pub const fn browsing_context(&self) -> ExtensionGrantBrowsingContext {
        self.browsing_context
    }

    /// Immutable package identity shared by the install, manifest, and grant.
    pub fn package(&self) -> &ExtensionPackageIdentity {
        self.manifest.package()
    }

    /// Exact admitted structural manifest pinned by the store snapshot.
    pub fn manifest(&self) -> &ExtensionManifestDescriptor {
        &self.manifest
    }

    /// Shared exact manifest owner for trusted cross-database validation.
    ///
    /// The manifest is immutable and non-authorizing. Returning the retained
    /// owner avoids reparsing or deep-copying declarations when a serialized
    /// coordinator must submit the same Store-admitted manifest to another
    /// durable authority fence.
    pub fn manifest_arc(&self) -> &Arc<ExtensionManifestDescriptor> {
        &self.manifest
    }

    /// Conservative logical heap-plus-inline charge retained by this snapshot.
    ///
    /// The manifest and grants are immutable shared values. Counting their
    /// complete logical charge here deliberately does not assume another owner
    /// will keep either allocation alive.
    pub fn retained_bytes(&self) -> usize {
        size_of::<Self>().saturating_add(self.retained_heap_bytes())
    }

    pub(crate) fn retained_heap_bytes(&self) -> usize {
        self.manifest
            .retained_bytes()
            .saturating_add(self.grants.retained_bytes())
    }

    pub(super) fn into_operation_authority(
        self,
        generation: ExtensionRuntimeGeneration,
        lineage: ExtensionRuntimeOperationAuthorityLineage,
    ) -> ExtensionRuntimeOperationAuthority {
        let fingerprint = self.fingerprint(generation);
        ExtensionRuntimeOperationAuthority {
            eligibility: self,
            fingerprint,
            lineage,
            not_sync: PhantomData,
        }
    }

    /// Projects a complete, non-authorizing reconciliation fingerprint.
    ///
    /// A runtime coordinator uses this to detect any durable input change and
    /// retire the old native generation. The returned value is still not an
    /// activation or operation capability: the coordinator must retain this
    /// eligibility and join it with authenticated repository and native
    /// ownership separately.
    pub fn fingerprint(
        &self,
        generation: ExtensionRuntimeGeneration,
    ) -> ExtensionRuntimeFingerprint {
        ExtensionRuntimeFingerprint::from_eligibility(ExtensionRuntimeFingerprintInput {
            instance: super::ExtensionRuntimeInstance::new(
                self.profile,
                self.install_id,
                generation,
            ),
            catalog_revision: self.catalog_revision,
            install_revision: self.install_revision,
            grant_revision: self.grant_revision,
            grant_digest: self.grant_digest,
            package: self.manifest.package().clone(),
            browsing_context: self.browsing_context,
        })
    }

    /// Evaluates one exact declared API permission against this snapshot.
    ///
    /// A granted result is still insufficient for an operation: the runtime
    /// broker must additionally validate a closed operation purpose and join
    /// this eligibility with package, scheme, document, and native authority.
    pub fn decide_api(&self, name: &ApiPermissionName) -> ExtensionApiGrantDecision {
        self.grants
            .decide_api(&self.manifest, name, self.browsing_context)
    }

    /// Evaluates one concrete URL against the exact host/file/private grants.
    ///
    /// `InScope` remains a scope result, not permission to fetch, inject, or
    /// expose data. The operation broker must independently validate purpose.
    pub fn decide_url_scope(&self, url: &Url) -> ExtensionUrlScopeDecision {
        self.grants
            .decide_url_scope(&self.manifest, url, self.browsing_context)
    }
}

/// Move-only authority for privileged operations against one exact runtime.
///
/// This capability can only be created when the package-pin acquisition
/// binding is linearly split for runtime handoff. It owns the complete Store
/// eligibility and the one process-local generation fingerprint derived from
/// it. The native host must retain this value alongside authenticated package
/// access and exact native ownership; none of those inputs is sufficient on
/// its own.
///
/// The capability deliberately cannot be cloned:
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionRuntimeOperationAuthority;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionRuntimeOperationAuthority>();
/// ```
///
/// It is movable to the serialized runtime owner, but deliberately cannot be
/// shared across threads:
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionRuntimeOperationAuthority;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<ExtensionRuntimeOperationAuthority>();
/// ```
///
/// Its fields are not a public construction surface:
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionRuntimeOperationAuthority;
/// let _ = ExtensionRuntimeOperationAuthority {
///     eligibility: panic!("not constructible"),
///     fingerprint: panic!("not constructible"),
///     not_sync: std::marker::PhantomData,
/// };
/// ```
///
/// It is process-local authority and cannot cross a serialization boundary:
///
/// ```compile_fail
/// use zephium_core::extensions::ExtensionRuntimeOperationAuthority;
/// fn requires_serialize<T: serde::Serialize>() {}
/// requires_serialize::<ExtensionRuntimeOperationAuthority>();
/// ```
///
/// A bare Store eligibility cannot mint operation witnesses:
///
/// ```compile_fail
/// use zephium_core::extensions::{
///     ExtensionRuntimeEligibility, ExtensionRuntimeFingerprint,
///     ExtensionUserInvocationKind,
/// };
/// fn cannot_mint(
///     eligibility: &ExtensionRuntimeEligibility,
///     runtime: &ExtensionRuntimeFingerprint,
/// ) {
///     let _ = eligibility.mint_active_tab_grant_witness(
///         runtime,
///         ExtensionUserInvocationKind::ToolbarAction,
///     );
/// }
/// ```
#[must_use = "runtime operation authority must remain joined with native and package authority"]
pub struct ExtensionRuntimeOperationAuthority {
    eligibility: ExtensionRuntimeEligibility,
    fingerprint: ExtensionRuntimeFingerprint,
    lineage: ExtensionRuntimeOperationAuthorityLineage,
    not_sync: PhantomData<Cell<()>>,
}

impl ExtensionRuntimeOperationAuthority {
    /// Complete exact runtime generation this capability authorizes.
    ///
    /// The fingerprint remains non-authorizing identity when separated from
    /// this move-only capability.
    pub const fn fingerprint(&self) -> &ExtensionRuntimeFingerprint {
        &self.fingerprint
    }

    /// Conservative logical heap-plus-inline charge retained by this authority.
    pub fn retained_bytes(&self) -> usize {
        size_of::<Self>().saturating_add(self.eligibility.retained_heap_bytes())
    }

    /// Projects the complete effective grant state for one exact native
    /// runtime generation.
    ///
    /// The caller must submit the full fingerprint it is configuring. A stale
    /// generation, grant revision/digest, package, profile, or browsing
    /// context is rejected before any declaration can be observed. The
    /// returned borrowed view allocates nothing and remains structural rather
    /// than authorizing; native activation must retain this operation
    /// authority and separately join authenticated package and ownership
    /// evidence.
    pub fn native_grant_projection(
        &self,
        runtime: &ExtensionRuntimeFingerprint,
    ) -> Result<ExtensionNativeGrantProjection<'_>, ExtensionOperationAuthorityDenial> {
        if runtime != &self.fingerprint {
            return Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch);
        }
        Ok(ExtensionNativeGrantProjection::new(
            &self.fingerprint,
            &self.eligibility.manifest,
            &self.eligibility.grants,
        ))
    }

    /// Checks the complete non-fresh Store/package-pin lineage carried by this
    /// authority against one structural native-ownership row.
    ///
    /// This does not prove the row is current. It prevents a capability from a
    /// different catalog generation, backend, journal operation, or native
    /// incarnation from being routed into this owner even when both share the
    /// same user-facing runtime fingerprint.
    #[must_use]
    pub fn matches_native_ownership_lineage(&self, entry: &ExtensionNativeOwnershipEntry) -> bool {
        self.matches_native_ownership_stable_lineage(entry)
            && entry.store_grant_revision() == self.fingerprint.grant_revision()
            && entry.grant_digest() == self.fingerprint.grant_digest()
    }

    pub(super) fn matches_native_ownership_stable_lineage(
        &self,
        entry: &ExtensionNativeOwnershipEntry,
    ) -> bool {
        let instance = self.fingerprint.instance();
        entry.key().profile() == instance.profile()
            && entry.key().install_id() == instance.install_id()
            && entry.key().browsing_context() == self.fingerprint.browsing_context()
            && entry.package() == self.fingerprint.package()
            && entry.store_catalog_revision() == self.fingerprint.catalog_revision()
            && entry.store_install_revision() == self.fingerprint.install_revision()
            && self.lineage.matches_entry(entry)
    }

    /// Replaces this live runtime's grant authority without changing its
    /// process-local generation or native/package lineage.
    ///
    /// This is intentionally narrower than general permission mutation. It
    /// accepts only the exact next grant revision, an unchanged manifest and
    /// install cohort, an additive API/host grant set, unchanged file/private
    /// access, and a journal row whose only changed fields are grant revision
    /// and digest. Revocation and browsing-partition changes must retire the
    /// native owner instead.
    ///
    /// Validation completes before either field is replaced. On refusal this
    /// authority remains byte-for-byte authoritative for the old fingerprint,
    /// while the proposed eligibility is returned losslessly.
    pub fn try_rebind_grants(
        &mut self,
        current_entry: &ExtensionNativeOwnershipEntry,
        rebound_entry: &ExtensionNativeOwnershipEntry,
        eligibility: ExtensionRuntimeEligibility,
    ) -> Result<ExtensionRuntimeFingerprint, ExtensionRuntimeGrantRebindRefusal> {
        let deny = |reason, eligibility| ExtensionRuntimeGrantRebindRefusal {
            reason,
            eligibility: Box::new(eligibility),
        };
        if !self.matches_native_ownership_lineage(current_entry)
            || current_entry.intent() != ExtensionNativeOwnershipIntent::Acquire
            || current_entry.phase() != ExtensionNativeOwnershipPhase::NativeOwned
        {
            return Err(deny(
                ExtensionRuntimeGrantRebindDenial::CurrentAuthorityMismatch,
                eligibility,
            ));
        }
        if !is_grant_only_owned_transition(current_entry, rebound_entry) {
            return Err(deny(
                ExtensionRuntimeGrantRebindDenial::InvalidOwnershipTransition,
                eligibility,
            ));
        }

        let generation = self.fingerprint.instance().generation();
        let replacement_fingerprint = eligibility.fingerprint(generation);
        if replacement_fingerprint.instance() != self.fingerprint.instance()
            || replacement_fingerprint.package() != self.fingerprint.package()
            || replacement_fingerprint.catalog_revision() != self.fingerprint.catalog_revision()
            || replacement_fingerprint.install_revision() != self.fingerprint.install_revision()
            || replacement_fingerprint.browsing_context() != self.fingerprint.browsing_context()
            || replacement_fingerprint.grant_revision() != rebound_entry.store_grant_revision()
            || replacement_fingerprint.grant_digest() != rebound_entry.grant_digest()
        {
            return Err(deny(
                ExtensionRuntimeGrantRebindDenial::ReplacementAuthorityMismatch,
                eligibility,
            ));
        }
        if !eligibility_is_monotonic_grant_extension(&self.eligibility, &eligibility) {
            return Err(deny(
                ExtensionRuntimeGrantRebindDenial::NonMonotonicGrantChange,
                eligibility,
            ));
        }

        self.eligibility = eligibility;
        self.fingerprint = replacement_fingerprint.clone();
        debug_assert!(self.matches_native_ownership_lineage(rebound_entry));
        Ok(replacement_fingerprint)
    }

    pub(super) fn retained_heap_bytes(&self) -> usize {
        self.eligibility.retained_heap_bytes()
    }

    /// Mints transient `activeTab` authority for one exact trusted invocation.
    ///
    /// The caller must submit the complete runtime fingerprint it is operating
    /// against. The engine must still join the returned witness with a trusted
    /// user gesture and its exact current native document.
    pub fn mint_active_tab_grant_witness(
        &self,
        runtime: &ExtensionRuntimeFingerprint,
        invocation: ExtensionUserInvocationKind,
    ) -> Result<ExtensionActiveTabGrantWitness, ExtensionOperationAuthorityDenial> {
        if runtime != &self.fingerprint {
            return Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch);
        }
        if self
            .eligibility
            .decide_api(invocation_api_permission(invocation))
            != ExtensionApiGrantDecision::Granted
        {
            return Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing);
        }
        Ok(ExtensionActiveTabGrantWitness {
            runtime: self.fingerprint.clone(),
            invocation,
        })
    }

    /// Mints one exact closed document-purpose capability.
    ///
    /// This proves only the purpose-specific API authority and exact runtime
    /// snapshot. No URL is accepted here. The engine must derive its current
    /// committed URL and evaluate durable scope through the returned witness,
    /// then join either that result or exact transient `activeTab` document
    /// authority with native view identity.
    pub fn mint_document_authority_witness(
        &self,
        runtime: &ExtensionRuntimeFingerprint,
        purpose: ExtensionDocumentPurpose,
    ) -> Result<ExtensionDocumentAuthorityWitness, ExtensionOperationAuthorityDenial> {
        if runtime != &self.fingerprint {
            return Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch);
        }
        if self
            .eligibility
            .decide_api(document_purpose_api_permission(purpose))
            != ExtensionApiGrantDecision::Granted
        {
            return Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing);
        }
        Ok(ExtensionDocumentAuthorityWitness {
            runtime: self.fingerprint.clone(),
            purpose,
            manifest: Arc::clone(&self.eligibility.manifest),
            grants: Arc::clone(&self.eligibility.grants),
        })
    }

    /// Mints authority for one closed Zephium compatibility-broker operation.
    ///
    /// This does not authorize generic native messaging. The native adapter
    /// must additionally bind the callback to this exact published runtime,
    /// accept only Zephium's fixed internal application identifier, and
    /// consume the returned witness when constructing the Shell request.
    pub fn mint_compatibility_broker_witness(
        &self,
        runtime: &ExtensionRuntimeFingerprint,
        purpose: ExtensionCompatibilityBrokerPurpose,
    ) -> Result<ExtensionCompatibilityBrokerWitness, ExtensionOperationAuthorityDenial> {
        if runtime != &self.fingerprint {
            return Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch);
        }
        if self.eligibility.manifest.compatibility_target().as_str()
            != MACOS_NATIVE_BROKERED_COMPATIBILITY_TARGET
        {
            return Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing);
        }
        if self
            .eligibility
            .decide_api(compatibility_broker_api_permission(purpose))
            != ExtensionApiGrantDecision::Granted
        {
            return Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing);
        }
        Ok(ExtensionCompatibilityBrokerWitness::new(
            self.fingerprint.clone(),
            purpose,
        ))
    }

    pub(super) const fn eligibility(&self) -> &ExtensionRuntimeEligibility {
        &self.eligibility
    }

    pub(super) fn into_eligibility(self) -> ExtensionRuntimeEligibility {
        self.eligibility
    }
}

fn is_grant_only_owned_transition(
    current: &ExtensionNativeOwnershipEntry,
    rebound: &ExtensionNativeOwnershipEntry,
) -> bool {
    current.key() == rebound.key()
        && current.operation() == rebound.operation()
        && current.revision() == rebound.revision()
        && current.package() == rebound.package()
        && current.catalog_set_digest() == rebound.catalog_set_digest()
        && current.catalog_role() == rebound.catalog_role()
        && current.store_catalog_revision() == rebound.store_catalog_revision()
        && current.store_install_revision() == rebound.store_install_revision()
        && current.store_grant_revision().next() == Some(rebound.store_grant_revision())
        && current.grant_digest() != rebound.grant_digest()
        && current.runtime_backend() == rebound.runtime_backend()
        && current.expected_native_identity() == rebound.expected_native_identity()
        && current.native_identity() == rebound.native_identity()
        && current.native_incarnation() == rebound.native_incarnation()
        && rebound.intent() == ExtensionNativeOwnershipIntent::Acquire
        && rebound.phase() == ExtensionNativeOwnershipPhase::NativeOwned
}

fn eligibility_is_monotonic_grant_extension(
    current: &ExtensionRuntimeEligibility,
    replacement: &ExtensionRuntimeEligibility,
) -> bool {
    if current.manifest != replacement.manifest {
        return false;
    }
    let current = current.grants.persistence_projection();
    let replacement = replacement.grants.persistence_projection();
    let strictly_adds_api_or_host = replacement.api_grant_count() > current.api_grant_count()
        || replacement.host_grant_count() > current.host_grant_count();
    strictly_adds_api_or_host
        && current.persisted_file_access() == replacement.persisted_file_access()
        && current.persisted_private_access() == replacement.persisted_private_access()
        && current
            .api_grants()
            .all(|old| replacement.api_grants().any(|new| new == old))
        && current.host_grants().all(|old| {
            replacement
                .host_grants()
                .any(|new| new.as_str() == old.as_str())
        })
}

impl fmt::Debug for ExtensionRuntimeOperationAuthority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeOperationAuthority")
            .field("runtime", &"<redacted>")
            .field("authority", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::{
        ExtensionApiPermissionSet, ExtensionArchiveDigest, ExtensionAuthorityId,
        ExtensionCompatibilityClassification, ExtensionCompatibilityLevel,
        ExtensionCompatibilityTargetId, ExtensionContentScriptDeclaration,
        ExtensionContentScriptGlobDeclaration, ExtensionContentScriptResourceDigest,
        ExtensionContentScriptRunAt, ExtensionContentScriptWorld,
        ExtensionContentSecurityPolicyDeclaration, ExtensionGrantCohort, ExtensionGrantDenial,
        ExtensionGrantManifestBinding, ExtensionGrantManifestBindings, ExtensionHostPermissionSet,
        ExtensionInstall, ExtensionInstallCatalog, ExtensionManifestDeclarations,
        ExtensionManifestDigest, ExtensionManifestExecutionSurfaces,
        ExtensionManifestResourceDigest, ExtensionNativeGrantDecision,
        ExtensionNativeGrantRequirement, ExtensionNativeGrantSnapshot,
        ExtensionNativeOwnershipEntryRevision, ExtensionNativeOwnershipKey, ExtensionPackageKey,
        ExtensionPackagePayloadIdentity, ExtensionPackageRevision, ExtensionTreeDigest,
        MAX_EXTENSION_CONTENT_SCRIPT_DECLARATIONS,
    };
    use crate::injection::{MatchOptions, MatchPattern, MatchSet};
    use proptest::prelude::*;
    use std::collections::BTreeSet;

    const ALL_URLS: &str = "<all_urls>";

    fn operation_lineage(value: u64) -> ExtensionRuntimeOperationAuthorityLineage {
        ExtensionRuntimeOperationAuthorityLineage {
            catalog_set_digest: ExtensionCatalogSetDigest::from_bytes([17; 32]),
            catalog_role: ExtensionCatalogGenerationRole::Active,
            runtime_backend: ExtensionRuntimeBackendTarget::LinuxCompatibility,
            operation: ExtensionNativeOwnershipOperation::new(value).unwrap(),
            native_incarnation: ExtensionNativeIncarnation::new(value).unwrap(),
        }
    }

    fn package() -> ExtensionPackageIdentity {
        ExtensionPackageIdentity::new(
            ExtensionAuthorityId::from_bytes([1; 32]),
            ExtensionPackageKey::from_bytes([2; 32]),
            ExtensionPackageRevision::INITIAL,
            ExtensionPackagePayloadIdentity::acquired_zip(
                3,
                ExtensionArchiveDigest::from_bytes([3; 32]),
            )
            .unwrap(),
            ExtensionManifestDigest::from_bytes([4; 32]),
            ExtensionTreeDigest::from_bytes([5; 32]),
        )
    }

    fn api(names: &[&str]) -> ExtensionApiPermissionSet {
        ExtensionApiPermissionSet::new(
            names
                .iter()
                .map(|name| ApiPermissionName::parse_exact(name).unwrap())
                .collect(),
        )
        .unwrap()
    }

    fn hosts(patterns: &[&str]) -> ExtensionHostPermissionSet {
        ExtensionHostPermissionSet::new(
            MatchSet::parse(
                patterns,
                std::iter::empty::<&str>(),
                MatchOptions::default(),
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn manifest() -> Arc<ExtensionManifestDescriptor> {
        projection_manifest(&[], &["activeTab", "scripting"], &[], &[ALL_URLS], &[])
    }

    fn content_script(patterns: &[&str], digest_byte: u8) -> ExtensionContentScriptDeclaration {
        ExtensionContentScriptDeclaration::new(
            MatchSet::parse(
                patterns.iter().copied(),
                std::iter::empty::<&str>(),
                MatchOptions::default(),
            )
            .unwrap(),
            ExtensionContentScriptRunAt::DocumentStart,
            true,
            ExtensionContentScriptWorld::Isolated,
            1,
            0,
            ExtensionContentScriptGlobDeclaration::Absent,
            ExtensionContentScriptResourceDigest::from_bytes([digest_byte; 32]),
        )
        .unwrap()
    }

    fn projection_manifest(
        required_api: &[&str],
        optional_api: &[&str],
        required_hosts: &[&str],
        optional_hosts: &[&str],
        content_script_hosts: &[&[&str]],
    ) -> Arc<ExtensionManifestDescriptor> {
        projection_manifest_for_target(
            required_api,
            optional_api,
            required_hosts,
            optional_hosts,
            content_script_hosts,
            "test.runtime.authority.v1",
        )
    }

    fn projection_manifest_for_target(
        required_api: &[&str],
        optional_api: &[&str],
        required_hosts: &[&str],
        optional_hosts: &[&str],
        content_script_hosts: &[&[&str]],
        compatibility_target: &str,
    ) -> Arc<ExtensionManifestDescriptor> {
        let scripts = content_script_hosts
            .iter()
            .filter(|patterns| !patterns.is_empty())
            .enumerate()
            .map(|(index, patterns)| content_script(patterns, 32 + index as u8))
            .collect();
        let declarations = ExtensionManifestDeclarations::new(
            api(required_api),
            api(optional_api),
            (!required_hosts.is_empty()).then(|| hosts(required_hosts)),
            (!optional_hosts.is_empty()).then(|| hosts(optional_hosts)),
            None,
            None,
            Vec::new(),
            ExtensionManifestExecutionSurfaces::new(
                scripts,
                ExtensionContentSecurityPolicyDeclaration::new(
                    ExtensionManifestResourceDigest::from_bytes([6; 32]),
                ),
                None,
                Vec::new(),
            )
            .unwrap(),
            Vec::new(),
        )
        .unwrap();
        let compatibility = declarations
            .declaration_keys()
            .into_iter()
            .map(|declaration| {
                ExtensionCompatibilityClassification::new(
                    declaration,
                    ExtensionCompatibilityLevel::Compatible,
                )
            })
            .collect();
        Arc::new(
            ExtensionManifestDescriptor::new(
                package(),
                3,
                declarations,
                ExtensionCompatibilityTargetId::parse_exact(compatibility_target).unwrap(),
                compatibility,
            )
            .unwrap(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn cohort_with_manifest(
        profile: ProfileId,
        install_id: ExtensionInstallId,
        manifest: Arc<ExtensionManifestDescriptor>,
        granted_api: &[&str],
        granted_hosts: &[&str],
        file_access: bool,
        private_access: bool,
    ) -> ExtensionGrantCohort {
        cohort_with_manifest_at_revision(
            profile,
            install_id,
            manifest,
            ExtensionGrantRevision::INITIAL,
            granted_api,
            granted_hosts,
            file_access,
            private_access,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn cohort_with_manifest_at_revision(
        profile: ProfileId,
        install_id: ExtensionInstallId,
        manifest: Arc<ExtensionManifestDescriptor>,
        grant_revision: ExtensionGrantRevision,
        granted_api: &[&str],
        granted_hosts: &[&str],
        file_access: bool,
        private_access: bool,
    ) -> ExtensionGrantCohort {
        let install = ExtensionInstall::from_persisted(
            install_id,
            ExtensionInstallRevision::new(7).unwrap(),
            manifest.package().clone(),
            true,
        );
        let catalog = ExtensionInstallCatalog::from_persisted(
            ExtensionInstallCatalogRevision::new(9).unwrap(),
            Some(install_id),
            vec![install.clone()],
        )
        .unwrap();
        let bindings =
            ExtensionGrantManifestBindings::new(vec![ExtensionGrantManifestBinding::new(
                install_id,
                Arc::clone(&manifest),
            )])
            .unwrap();
        let authority = ExtensionGrantAuthority::from_persisted(
            &install,
            grant_revision,
            manifest.package().clone(),
            granted_api
                .iter()
                .map(|name| ApiPermissionName::parse_exact(name).unwrap())
                .collect(),
            granted_hosts
                .iter()
                .map(|pattern| MatchPattern::parse(pattern).unwrap())
                .collect(),
            file_access,
            private_access,
            &manifest,
        )
        .unwrap();
        ExtensionGrantCohort::from_persisted(profile, catalog, bindings, vec![authority]).unwrap()
    }

    fn owned_entry(
        runtime: &ExtensionRuntimeFingerprint,
        lineage: ExtensionRuntimeOperationAuthorityLineage,
    ) -> ExtensionNativeOwnershipEntry {
        ExtensionNativeOwnershipEntry::from_persisted(
            ExtensionNativeOwnershipKey::new(
                runtime.instance().profile(),
                runtime.instance().install_id(),
                runtime.browsing_context(),
            ),
            lineage.operation,
            ExtensionNativeOwnershipEntryRevision::new(3).unwrap(),
            runtime.package().clone(),
            lineage.catalog_set_digest,
            lineage.catalog_role,
            runtime.catalog_revision(),
            runtime.install_revision(),
            runtime.grant_revision(),
            runtime.grant_digest(),
            lineage.runtime_backend,
            lineage.native_incarnation,
            ExtensionNativeOwnershipIntent::Acquire,
            ExtensionNativeOwnershipPhase::NativeOwned,
        )
        .unwrap()
    }

    fn cohort(
        profile: ProfileId,
        install_id: ExtensionInstallId,
        granted_api: &[&str],
        file_access: bool,
        private_access: bool,
    ) -> ExtensionGrantCohort {
        let manifest = manifest();
        cohort_with_manifest(
            profile,
            install_id,
            manifest,
            granted_api,
            &[ALL_URLS],
            file_access,
            private_access,
        )
    }

    fn eligible_runtime(
        profile: ProfileId,
        install_id: ExtensionInstallId,
        generation: ExtensionRuntimeGeneration,
        granted_api: &[&str],
        file_access: bool,
    ) -> (
        ExtensionRuntimeOperationAuthority,
        ExtensionRuntimeFingerprint,
    ) {
        let cohort = cohort(profile, install_id, granted_api, file_access, false);
        let eligibility = cohort
            .runtime_eligibility(install_id, ExtensionGrantBrowsingContext::Regular)
            .unwrap();
        let runtime = eligibility.fingerprint(generation);
        (
            eligibility.into_operation_authority(generation, operation_lineage(generation.get())),
            runtime,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn eligible_runtime_with_manifest(
        profile: ProfileId,
        install_id: ExtensionInstallId,
        generation: ExtensionRuntimeGeneration,
        manifest: Arc<ExtensionManifestDescriptor>,
        granted_api: &[&str],
        granted_hosts: &[&str],
        file_access: bool,
        private_access: bool,
    ) -> (
        ExtensionRuntimeOperationAuthority,
        ExtensionRuntimeFingerprint,
    ) {
        let cohort = cohort_with_manifest(
            profile,
            install_id,
            manifest,
            granted_api,
            granted_hosts,
            file_access,
            private_access,
        );
        let eligibility = cohort
            .runtime_eligibility(install_id, ExtensionGrantBrowsingContext::Regular)
            .unwrap();
        let runtime = eligibility.fingerprint(generation);
        (
            eligibility.into_operation_authority(generation, operation_lineage(generation.get())),
            runtime,
        )
    }

    #[test]
    fn active_tab_witness_is_exact_full_runtime_and_invocation_bound() {
        let profile = ProfileId::from(17);
        let install_id = ExtensionInstallId::from(23);
        let generation = ExtensionRuntimeGeneration::new(29).unwrap();
        let (authority, runtime) = eligible_runtime(
            profile,
            install_id,
            generation,
            &["activeTab", "scripting"],
            false,
        );
        let witness = authority
            .mint_active_tab_grant_witness(&runtime, ExtensionUserInvocationKind::ToolbarAction)
            .unwrap();

        assert_eq!(witness.runtime(), &runtime);
        assert_eq!(witness.runtime_instance(), runtime.instance());
        assert_eq!(
            witness.invocation(),
            ExtensionUserInvocationKind::ToolbarAction
        );
        assert!(witness.matches(&runtime, ExtensionUserInvocationKind::ToolbarAction));

        let (_, wrong_generation) = eligible_runtime(
            profile,
            install_id,
            generation.next().unwrap(),
            &["activeTab", "scripting"],
            false,
        );
        assert!(!witness.matches(
            &wrong_generation,
            ExtensionUserInvocationKind::ToolbarAction
        ));

        let (_, wrong_profile) = eligible_runtime(
            ProfileId::from(18),
            install_id,
            generation,
            &["activeTab", "scripting"],
            false,
        );
        assert!(matches!(
            authority.mint_active_tab_grant_witness(
                &wrong_profile,
                ExtensionUserInvocationKind::ToolbarAction,
            ),
            Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch)
        ));

        let (_, wrong_install) = eligible_runtime(
            profile,
            ExtensionInstallId::from(24),
            generation,
            &["activeTab", "scripting"],
            false,
        );
        assert!(matches!(
            authority.mint_active_tab_grant_witness(
                &wrong_install,
                ExtensionUserInvocationKind::ToolbarAction,
            ),
            Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch)
        ));

        let (_, wrong_grants) =
            eligible_runtime(profile, install_id, generation, &["activeTab"], false);
        assert!(matches!(
            authority.mint_active_tab_grant_witness(
                &wrong_grants,
                ExtensionUserInvocationKind::ToolbarAction,
            ),
            Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch)
        ));
    }

    #[test]
    fn operation_authority_is_send_bounded_and_exactly_generation_bound() {
        fn assert_send<T: Send>() {}

        assert_send::<ExtensionRuntimeOperationAuthority>();
        let (authority, runtime) = eligible_runtime(
            ProfileId::from(27),
            ExtensionInstallId::from(28),
            ExtensionRuntimeGeneration::new(30).unwrap(),
            &["activeTab", "scripting"],
            false,
        );

        assert_eq!(authority.fingerprint(), &runtime);
        assert_eq!(
            authority.fingerprint().instance().generation(),
            ExtensionRuntimeGeneration::new(30).unwrap()
        );
        assert!(authority.retained_bytes() >= size_of::<ExtensionRuntimeOperationAuthority>());
        assert_eq!(
            format!("{authority:?}"),
            "ExtensionRuntimeOperationAuthority { runtime: \"<redacted>\", authority: \"<redacted>\" }"
        );
    }

    #[test]
    fn live_grant_rebind_is_exact_additive_and_preserves_native_lineage() {
        let profile = ProfileId::from(211);
        let install_id = ExtensionInstallId::from(223);
        let generation = ExtensionRuntimeGeneration::new(227).unwrap();
        let manifest = manifest();
        let current_cohort = cohort_with_manifest_at_revision(
            profile,
            install_id,
            Arc::clone(&manifest),
            ExtensionGrantRevision::INITIAL,
            &["activeTab"],
            &[ALL_URLS],
            false,
            false,
        );
        let current_eligibility = current_cohort
            .runtime_eligibility(install_id, ExtensionGrantBrowsingContext::Regular)
            .unwrap();
        let current_runtime = current_eligibility.fingerprint(generation);
        let lineage = operation_lineage(229);
        let mut authority = current_eligibility.into_operation_authority(generation, lineage);
        let current_entry = owned_entry(&current_runtime, lineage);
        let stale_witness = authority
            .mint_active_tab_grant_witness(
                &current_runtime,
                ExtensionUserInvocationKind::ToolbarAction,
            )
            .unwrap();

        let next_cohort = cohort_with_manifest_at_revision(
            profile,
            install_id,
            manifest,
            current_runtime.grant_revision().next().unwrap(),
            &["activeTab", "scripting"],
            &[ALL_URLS],
            false,
            false,
        );
        let next_eligibility = next_cohort
            .runtime_eligibility(install_id, ExtensionGrantBrowsingContext::Regular)
            .unwrap();
        let next_runtime = next_eligibility.fingerprint(generation);
        let rebound_entry = owned_entry(&next_runtime, lineage);

        assert_eq!(
            authority
                .try_rebind_grants(&current_entry, &rebound_entry, next_eligibility)
                .unwrap(),
            next_runtime
        );
        assert_eq!(authority.fingerprint(), &next_runtime);
        assert!(authority.matches_native_ownership_lineage(&rebound_entry));
        assert!(!authority.matches_native_ownership_lineage(&current_entry));
        assert!(!stale_witness.matches(&next_runtime, ExtensionUserInvocationKind::ToolbarAction));
        assert!(matches!(
            authority.mint_document_authority_witness(
                &current_runtime,
                ExtensionDocumentPurpose::ExecuteScript,
            ),
            Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch)
        ));
        assert!(
            authority
                .mint_document_authority_witness(
                    &next_runtime,
                    ExtensionDocumentPurpose::ExecuteScript,
                )
                .is_ok()
        );
    }

    #[test]
    fn live_grant_rebind_refuses_removal_and_preserves_current_authority() {
        let profile = ProfileId::from(233);
        let install_id = ExtensionInstallId::from(239);
        let generation = ExtensionRuntimeGeneration::new(241).unwrap();
        let manifest = manifest();
        let current_cohort = cohort_with_manifest_at_revision(
            profile,
            install_id,
            Arc::clone(&manifest),
            ExtensionGrantRevision::INITIAL,
            &["activeTab", "scripting"],
            &[ALL_URLS],
            false,
            false,
        );
        let current_eligibility = current_cohort
            .runtime_eligibility(install_id, ExtensionGrantBrowsingContext::Regular)
            .unwrap();
        let current_runtime = current_eligibility.fingerprint(generation);
        let lineage = operation_lineage(251);
        let mut authority = current_eligibility.into_operation_authority(generation, lineage);
        let current_entry = owned_entry(&current_runtime, lineage);

        let reduced_cohort = cohort_with_manifest_at_revision(
            profile,
            install_id,
            manifest,
            current_runtime.grant_revision().next().unwrap(),
            &["activeTab"],
            &[ALL_URLS],
            false,
            false,
        );
        let reduced_eligibility = reduced_cohort
            .runtime_eligibility(install_id, ExtensionGrantBrowsingContext::Regular)
            .unwrap();
        let reduced_runtime = reduced_eligibility.fingerprint(generation);
        let rebound_entry = owned_entry(&reduced_runtime, lineage);
        let refusal = authority
            .try_rebind_grants(&current_entry, &rebound_entry, reduced_eligibility)
            .unwrap_err();

        assert_eq!(
            refusal.reason(),
            ExtensionRuntimeGrantRebindDenial::NonMonotonicGrantChange
        );
        assert_eq!(
            refusal.into_eligibility().fingerprint(generation),
            reduced_runtime
        );
        assert_eq!(authority.fingerprint(), &current_runtime);
        assert!(authority.matches_native_ownership_lineage(&current_entry));
        assert!(authority
            .mint_document_authority_witness(
                &current_runtime,
                ExtensionDocumentPurpose::ExecuteScript,
            )
            .is_ok());
    }

    #[test]
    fn live_grant_rebind_refuses_skipped_rows_and_mismatched_eligibility_losslessly() {
        let profile = ProfileId::from(257);
        let install_id = ExtensionInstallId::from(263);
        let generation = ExtensionRuntimeGeneration::new(269).unwrap();
        let manifest = manifest();
        let current_cohort = cohort_with_manifest_at_revision(
            profile,
            install_id,
            Arc::clone(&manifest),
            ExtensionGrantRevision::INITIAL,
            &["activeTab"],
            &[ALL_URLS],
            false,
            false,
        );
        let current_eligibility = current_cohort
            .runtime_eligibility(install_id, ExtensionGrantBrowsingContext::Regular)
            .unwrap();
        let current_runtime = current_eligibility.fingerprint(generation);
        let lineage = operation_lineage(271);
        let mut authority = current_eligibility.into_operation_authority(generation, lineage);
        let current_entry = owned_entry(&current_runtime, lineage);

        let next_revision = current_runtime.grant_revision().next().unwrap();
        let next_cohort = cohort_with_manifest_at_revision(
            profile,
            install_id,
            Arc::clone(&manifest),
            next_revision,
            &["activeTab", "scripting"],
            &[ALL_URLS],
            false,
            false,
        );
        let next_eligibility = next_cohort
            .runtime_eligibility(install_id, ExtensionGrantBrowsingContext::Regular)
            .unwrap();
        let next_runtime = next_eligibility.fingerprint(generation);
        let skipped_cohort = cohort_with_manifest_at_revision(
            profile,
            install_id,
            Arc::clone(&manifest),
            next_revision.next().unwrap(),
            &["activeTab", "scripting"],
            &[ALL_URLS],
            false,
            false,
        );
        let skipped_runtime = skipped_cohort
            .runtime_eligibility(install_id, ExtensionGrantBrowsingContext::Regular)
            .unwrap()
            .fingerprint(generation);
        let refusal = authority
            .try_rebind_grants(
                &current_entry,
                &owned_entry(&skipped_runtime, lineage),
                next_eligibility,
            )
            .unwrap_err();
        assert_eq!(
            refusal.reason(),
            ExtensionRuntimeGrantRebindDenial::InvalidOwnershipTransition
        );
        assert_eq!(
            refusal.into_eligibility().fingerprint(generation),
            next_runtime
        );
        assert_eq!(authority.fingerprint(), &current_runtime);

        let mismatched_cohort = cohort_with_manifest_at_revision(
            profile,
            install_id,
            manifest,
            next_revision,
            &["activeTab"],
            &[ALL_URLS],
            false,
            false,
        );
        let mismatched_eligibility = mismatched_cohort
            .runtime_eligibility(install_id, ExtensionGrantBrowsingContext::Regular)
            .unwrap();
        let mismatched_runtime = mismatched_eligibility.fingerprint(generation);
        let refusal = authority
            .try_rebind_grants(
                &current_entry,
                &owned_entry(&next_runtime, lineage),
                mismatched_eligibility,
            )
            .unwrap_err();
        assert_eq!(
            refusal.reason(),
            ExtensionRuntimeGrantRebindDenial::ReplacementAuthorityMismatch
        );
        assert_eq!(
            refusal.into_eligibility().fingerprint(generation),
            mismatched_runtime
        );
        assert_eq!(authority.fingerprint(), &current_runtime);
        assert!(authority.matches_native_ownership_lineage(&current_entry));
    }

    #[test]
    fn missing_active_tab_or_scripting_authority_fails_closed() {
        let profile = ProfileId::from(31);
        let install_id = ExtensionInstallId::from(37);
        let generation = ExtensionRuntimeGeneration::new(41).unwrap();
        let (without_active_tab, runtime) =
            eligible_runtime(profile, install_id, generation, &["scripting"], false);
        assert!(matches!(
            without_active_tab.mint_active_tab_grant_witness(
                &runtime,
                ExtensionUserInvocationKind::ToolbarAction,
            ),
            Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing)
        ));

        let (without_scripting, runtime) =
            eligible_runtime(profile, install_id, generation, &["activeTab"], false);
        for purpose in [
            ExtensionDocumentPurpose::ExecuteScript,
            ExtensionDocumentPurpose::InsertCss,
            ExtensionDocumentPurpose::RemoveCss,
        ] {
            assert!(matches!(
                without_scripting.mint_document_authority_witness(&runtime, purpose),
                Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing)
            ));
        }
    }

    #[test]
    fn document_witness_retains_exact_owners_and_evaluates_engine_url_scope() {
        let profile = ProfileId::from(43);
        let install_id = ExtensionInstallId::from(47);
        let generation = ExtensionRuntimeGeneration::new(53).unwrap();
        let (authority, runtime) = eligible_runtime(
            profile,
            install_id,
            generation,
            &["activeTab", "scripting"],
            false,
        );
        let witness = authority
            .mint_document_authority_witness(&runtime, ExtensionDocumentPurpose::ExecuteScript)
            .unwrap();
        drop(authority);

        assert_eq!(witness.runtime(), &runtime);
        assert_eq!(witness.runtime_instance(), runtime.instance());
        assert_eq!(witness.purpose(), ExtensionDocumentPurpose::ExecuteScript);
        assert!(witness.matches(&runtime, ExtensionDocumentPurpose::ExecuteScript));
        assert!(!witness.matches(&runtime, ExtensionDocumentPurpose::InsertCss));
        assert_eq!(
            witness.decide_engine_document_url_scope(
                &Url::parse("https://scope.example/document").unwrap(),
            ),
            ExtensionUrlScopeDecision::InScope
        );
        assert_eq!(
            witness.decide_engine_document_url_scope(
                &Url::parse("file:///private/extension-secret.txt").unwrap(),
            ),
            ExtensionUrlScopeDecision::OutOfScope(ExtensionGrantDenial::FileAccessNotGranted)
        );

        let (authority, runtime) = eligible_runtime(
            profile,
            install_id,
            generation.next().unwrap(),
            &["activeTab", "scripting"],
            true,
        );
        let file_witness = authority
            .mint_document_authority_witness(&runtime, ExtensionDocumentPurpose::ExecuteScript)
            .unwrap();
        assert_eq!(
            file_witness.decide_engine_document_url_scope(
                &Url::parse("file:///private/extension-secret.txt").unwrap(),
            ),
            ExtensionUrlScopeDecision::InScope
        );
    }

    #[test]
    fn document_mint_rejects_a_different_runtime_fingerprint() {
        let profile = ProfileId::from(59);
        let install_id = ExtensionInstallId::from(61);
        let generation = ExtensionRuntimeGeneration::new(67).unwrap();
        let (authority, runtime) = eligible_runtime(
            profile,
            install_id,
            generation,
            &["activeTab", "scripting"],
            false,
        );
        let witness = authority
            .mint_document_authority_witness(&runtime, ExtensionDocumentPurpose::InsertCss)
            .unwrap();
        let (_, wrong_generation) = eligible_runtime(
            profile,
            install_id,
            generation.next().unwrap(),
            &["activeTab", "scripting"],
            false,
        );
        assert!(!witness.matches(&wrong_generation, ExtensionDocumentPurpose::InsertCss));

        let (_, wrong_grants) =
            eligible_runtime(profile, install_id, generation, &["scripting"], false);
        assert!(matches!(
            authority.mint_document_authority_witness(
                &wrong_grants,
                ExtensionDocumentPurpose::InsertCss,
            ),
            Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch)
        ));
    }

    #[test]
    fn compatibility_broker_witness_requires_exact_history_authority() {
        let profile = ProfileId::from(63);
        let install_id = ExtensionInstallId::from(65);
        let generation = ExtensionRuntimeGeneration::new(69).unwrap();
        let history_manifest = projection_manifest_for_target(
            &["history"],
            &[],
            &[],
            &[],
            &[],
            MACOS_NATIVE_BROKERED_COMPATIBILITY_TARGET,
        );
        let (authority, runtime) = eligible_runtime_with_manifest(
            profile,
            install_id,
            generation,
            history_manifest,
            &["history"],
            &[],
            false,
            false,
        );
        let witness = authority
            .mint_compatibility_broker_witness(
                &runtime,
                ExtensionCompatibilityBrokerPurpose::RecentHistory,
            )
            .unwrap();
        assert_eq!(witness.runtime_instance(), runtime.instance());
        assert_eq!(
            witness.purpose(),
            ExtensionCompatibilityBrokerPurpose::RecentHistory
        );
        let request = crate::extensions::ExtensionCompatibilityBrokerRequest::authorize(
            crate::extensions::ExtensionCompatibilityBrokerRequestId::new(1).unwrap(),
            crate::extensions::ExtensionCompatibilityBrokerOperation::RecentHistory { limit: 25 },
            witness,
        )
        .unwrap();
        assert_eq!(request.runtime(), runtime.instance());

        let optional_history_manifest = projection_manifest_for_target(
            &[],
            &["history"],
            &[],
            &[],
            &[],
            MACOS_NATIVE_BROKERED_COMPATIBILITY_TARGET,
        );
        let (without_history, without_history_runtime) = eligible_runtime_with_manifest(
            profile,
            install_id,
            generation.next().unwrap(),
            optional_history_manifest,
            &[],
            &[],
            false,
            false,
        );
        assert!(matches!(
            without_history.mint_compatibility_broker_witness(
                &without_history_runtime,
                ExtensionCompatibilityBrokerPurpose::RecentHistory,
            ),
            Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing)
        ));

        let ordinary_manifest = projection_manifest(&["history"], &[], &[], &[], &[]);
        let (ordinary, ordinary_runtime) = eligible_runtime_with_manifest(
            profile,
            install_id,
            generation.next().unwrap().next().unwrap(),
            ordinary_manifest,
            &["history"],
            &[],
            false,
            false,
        );
        assert!(matches!(
            ordinary.mint_compatibility_broker_witness(
                &ordinary_runtime,
                ExtensionCompatibilityBrokerPurpose::RecentHistory,
            ),
            Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing)
        ));
    }

    #[test]
    fn compatibility_broker_search_and_session_witnesses_require_distinct_grants() {
        let profile = ProfileId::from(83);
        let install_id = ExtensionInstallId::from(89);
        for (offset, permission, purpose) in [
            (
                0,
                "search",
                ExtensionCompatibilityBrokerPurpose::DefaultSearch,
            ),
            (
                1,
                "sessions",
                ExtensionCompatibilityBrokerPurpose::RestoreRecentSession,
            ),
        ] {
            let generation = ExtensionRuntimeGeneration::new(97 + offset).unwrap();
            let manifest = projection_manifest_for_target(
                &[permission],
                &[],
                &[],
                &[],
                &[],
                MACOS_NATIVE_BROKERED_COMPATIBILITY_TARGET,
            );
            let (authority, runtime) = eligible_runtime_with_manifest(
                profile,
                install_id,
                generation,
                manifest,
                &[permission],
                &[],
                false,
                false,
            );
            let witness = authority
                .mint_compatibility_broker_witness(&runtime, purpose)
                .unwrap();
            assert_eq!(witness.purpose(), purpose);

            let (without, without_runtime) = eligible_runtime_with_manifest(
                profile,
                install_id,
                generation.next().unwrap(),
                projection_manifest_for_target(
                    &[],
                    &[permission],
                    &[],
                    &[],
                    &[],
                    MACOS_NATIVE_BROKERED_COMPATIBILITY_TARGET,
                ),
                &[],
                &[],
                false,
                false,
            );
            assert!(matches!(
                without.mint_compatibility_broker_witness(&without_runtime, purpose),
                Err(ExtensionOperationAuthorityDenial::RequiredAuthorityMissing)
            ));
        }
    }

    #[test]
    fn private_runtime_eligibility_remains_unmintable() {
        let profile = ProfileId::from(71);
        let install_id = ExtensionInstallId::from(73);
        let cohort = cohort(profile, install_id, &["activeTab", "scripting"], true, true);
        assert!(matches!(
            cohort.runtime_eligibility(install_id, ExtensionGrantBrowsingContext::Private),
            Err(ExtensionRuntimeEligibilityDenial::PrivateBrowsingUnsupported)
        ));
    }

    #[test]
    fn committed_grant_authority_projects_without_a_second_store_cohort_load() {
        let profile = ProfileId::from(13);
        let install_id = ExtensionInstallId::from(17);
        let manifest = manifest();
        let install = ExtensionInstall::from_persisted(
            install_id,
            ExtensionInstallRevision::new(5).unwrap(),
            manifest.package().clone(),
            true,
        );
        let grants = ExtensionGrantAuthority::from_persisted(
            &install,
            ExtensionGrantRevision::new(7).unwrap(),
            manifest.package().clone(),
            vec![ApiPermissionName::parse_exact("activeTab").unwrap()],
            vec![MatchPattern::parse(ALL_URLS).unwrap()],
            false,
            false,
            &manifest,
        )
        .unwrap();
        let eligibility = ExtensionRuntimeEligibility::from_committed_grant_authority(
            profile,
            ExtensionInstallCatalogRevision::new(11).unwrap(),
            install,
            Arc::clone(&manifest),
            grants,
            ExtensionGrantBrowsingContext::Regular,
        )
        .expect("exact committed mutation projects");

        assert_eq!(eligibility.profile(), profile);
        assert_eq!(eligibility.install_id(), install_id);
        assert_eq!(eligibility.grant_revision().get(), 7);
        assert_eq!(eligibility.package(), manifest.package());
        assert!(Arc::ptr_eq(eligibility.manifest_arc(), &manifest));
    }

    #[test]
    fn native_projection_enumerates_every_api_declaration_and_explicit_denial() {
        let manifest = projection_manifest(
            &["zRequired", "bRequired"],
            &["mOptional", "aOptional"],
            &[],
            &[],
            &[],
        );
        let (authority, runtime) = eligible_runtime_with_manifest(
            ProfileId::from(97),
            ExtensionInstallId::from(101),
            ExtensionRuntimeGeneration::new(103).unwrap(),
            manifest,
            &["zRequired", "bRequired", "mOptional"],
            &[],
            false,
            false,
        );
        let projection = authority.native_grant_projection(&runtime).unwrap();
        let grants = projection
            .api_grants()
            .map(|grant| (grant.name().as_str(), grant.requirement(), grant.decision()))
            .collect::<Vec<_>>();

        assert_eq!(projection.api_grant_count(), 4);
        assert_eq!(
            grants,
            vec![
                (
                    "aOptional",
                    ExtensionNativeGrantRequirement::Optional,
                    ExtensionNativeGrantDecision::Denied,
                ),
                (
                    "bRequired",
                    ExtensionNativeGrantRequirement::Required,
                    ExtensionNativeGrantDecision::Granted,
                ),
                (
                    "mOptional",
                    ExtensionNativeGrantRequirement::Optional,
                    ExtensionNativeGrantDecision::Granted,
                ),
                (
                    "zRequired",
                    ExtensionNativeGrantRequirement::Required,
                    ExtensionNativeGrantDecision::Granted,
                ),
            ]
        );
        assert!(grants
            .iter()
            .filter(|(_, requirement, _)| {
                *requirement == ExtensionNativeGrantRequirement::Required
            })
            .all(|(_, _, decision)| decision.is_granted()));
    }

    #[test]
    fn native_projection_merges_content_script_hosts_canonically_without_duplicates() {
        let first_script = ["https://a.example/*", "https://z.example/*"];
        let second_script = ["https://b.example/*", "https://a.example/*"];
        let script_hosts: [&[&str]; 2] = [&first_script, &second_script];
        let manifest = projection_manifest(
            &[],
            &[],
            &["https://z.example/*", "https://c.example/*"],
            &["https://y.example/*", "https://m.example/*"],
            &script_hosts,
        );
        let (authority, runtime) = eligible_runtime_with_manifest(
            ProfileId::from(107),
            ExtensionInstallId::from(109),
            ExtensionRuntimeGeneration::new(113).unwrap(),
            manifest,
            &[],
            &[
                "https://a.example/*",
                "https://b.example/*",
                "https://c.example/*",
                "https://z.example/*",
                "https://y.example/*",
            ],
            false,
            false,
        );
        let projection = authority.native_grant_projection(&runtime).unwrap();
        let grants = projection
            .host_grants()
            .map(|grant| {
                (
                    grant.pattern().as_str(),
                    grant.requirement(),
                    grant.decision(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(projection.host_grant_count(), 6);
        assert_eq!(
            grants,
            vec![
                (
                    "https://a.example/*",
                    ExtensionNativeGrantRequirement::Required,
                    ExtensionNativeGrantDecision::Granted,
                ),
                (
                    "https://b.example/*",
                    ExtensionNativeGrantRequirement::Required,
                    ExtensionNativeGrantDecision::Granted,
                ),
                (
                    "https://c.example/*",
                    ExtensionNativeGrantRequirement::Required,
                    ExtensionNativeGrantDecision::Granted,
                ),
                (
                    "https://m.example/*",
                    ExtensionNativeGrantRequirement::Optional,
                    ExtensionNativeGrantDecision::Denied,
                ),
                (
                    "https://y.example/*",
                    ExtensionNativeGrantRequirement::Optional,
                    ExtensionNativeGrantDecision::Granted,
                ),
                (
                    "https://z.example/*",
                    ExtensionNativeGrantRequirement::Required,
                    ExtensionNativeGrantDecision::Granted,
                ),
            ]
        );
    }

    #[test]
    fn native_projection_covers_every_bounded_content_script_source_and_optional_tail() {
        let script_hosts = (0..MAX_EXTENSION_CONTENT_SCRIPT_DECLARATIONS)
            .map(|index| format!("https://script-{index:02}.example/*"))
            .collect::<Vec<_>>();
        let script_rows = script_hosts
            .iter()
            .map(|pattern| vec![pattern.as_str()])
            .collect::<Vec<_>>();
        let script_refs = script_rows.iter().map(Vec::as_slice).collect::<Vec<_>>();
        let optional = "https://zz-optional.example/*";
        let manifest = projection_manifest(&[], &[], &[], &[optional], &script_refs);
        let granted_hosts = script_hosts
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(optional))
            .collect::<Vec<_>>();
        let (authority, runtime) = eligible_runtime_with_manifest(
            ProfileId::from(173),
            ExtensionInstallId::from(179),
            ExtensionRuntimeGeneration::new(181).unwrap(),
            manifest,
            &[],
            &granted_hosts,
            false,
            false,
        );
        let projection = authority.native_grant_projection(&runtime).unwrap();
        let grants = projection.host_grants().collect::<Vec<_>>();

        assert_eq!(grants.len(), MAX_EXTENSION_CONTENT_SCRIPT_DECLARATIONS + 1);
        assert_eq!(
            grants.first().unwrap().pattern().as_str(),
            "https://script-00.example/*"
        );
        assert_eq!(
            grants[MAX_EXTENSION_CONTENT_SCRIPT_DECLARATIONS - 1].requirement(),
            ExtensionNativeGrantRequirement::Required
        );
        assert_eq!(grants.last().unwrap().pattern().as_str(), optional);
        assert_eq!(
            grants.last().unwrap().requirement(),
            ExtensionNativeGrantRequirement::Optional
        );
        assert!(grants.iter().all(|grant| grant.decision().is_granted()));
    }

    #[test]
    fn native_projection_item_debug_never_exposes_permission_or_host_text() {
        let manifest = projection_manifest(
            &["scripting"],
            &[],
            &["https://private.example/*"],
            &[],
            &[],
        );
        let (authority, runtime) = eligible_runtime_with_manifest(
            ProfileId::from(191),
            ExtensionInstallId::from(193),
            ExtensionRuntimeGeneration::new(197).unwrap(),
            manifest,
            &["scripting"],
            &["https://private.example/*"],
            false,
            false,
        );
        let projection = authority.native_grant_projection(&runtime).unwrap();
        let api_debug = format!("{:?}", projection.api_grants().next().unwrap());
        let host_debug = format!("{:?}", projection.host_grants().next().unwrap());

        assert!(!api_debug.contains("scripting"));
        assert!(!host_debug.contains("private.example"));
        assert!(api_debug.contains("<redacted>"));
        assert!(host_debug.contains("<redacted>"));
    }

    #[test]
    fn native_projection_keeps_file_and_private_flags_independent_and_effective() {
        let profile = ProfileId::from(127);
        let install_id = ExtensionInstallId::from(131);
        let generation = ExtensionRuntimeGeneration::new(137).unwrap();
        let (without_file, runtime) = eligible_runtime_with_manifest(
            profile,
            install_id,
            generation,
            manifest(),
            &[],
            &[ALL_URLS],
            false,
            true,
        );
        let projection = without_file.native_grant_projection(&runtime).unwrap();

        assert_eq!(
            projection.browsing_context(),
            ExtensionGrantBrowsingContext::Regular
        );
        assert_eq!(projection.host_grant_count(), 1);
        assert_eq!(
            projection.host_grants().next().unwrap().decision(),
            ExtensionNativeGrantDecision::Granted
        );
        assert!(!projection.file_scheme_access_granted());
        assert!(!projection.private_context_access_granted());

        let (with_file, runtime) = eligible_runtime_with_manifest(
            profile,
            install_id,
            generation.next().unwrap(),
            manifest(),
            &[],
            &[ALL_URLS],
            true,
            false,
        );
        let projection = with_file.native_grant_projection(&runtime).unwrap();
        assert!(projection.file_scheme_access_granted());
        assert!(!projection.private_context_access_granted());
    }

    #[test]
    fn native_projection_is_exact_runtime_bound_and_reports_grant_context() {
        let profile = ProfileId::from(139);
        let install_id = ExtensionInstallId::from(149);
        let generation = ExtensionRuntimeGeneration::new(151).unwrap();
        let (authority, runtime) = eligible_runtime(
            profile,
            install_id,
            generation,
            &["activeTab", "scripting"],
            false,
        );
        let projection = authority.native_grant_projection(&runtime).unwrap();

        assert_eq!(projection.runtime(), &runtime);
        assert_eq!(projection.grant_revision(), runtime.grant_revision());
        assert_eq!(projection.grant_digest(), runtime.grant_digest());
        assert_eq!(
            projection.retained_bytes(),
            std::mem::size_of_val(&projection)
        );
        assert_eq!(
            format!("{projection:?}"),
            "ExtensionNativeGrantProjection { runtime: \"<redacted>\", authority: \"<redacted>\" }"
        );

        let (_, wrong_generation) = eligible_runtime(
            profile,
            install_id,
            generation.next().unwrap(),
            &["activeTab", "scripting"],
            false,
        );
        assert!(matches!(
            authority.native_grant_projection(&wrong_generation),
            Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch)
        ));

        let (_, different_grants) =
            eligible_runtime(profile, install_id, generation, &["scripting"], false);
        assert_eq!(runtime.grant_revision(), different_grants.grant_revision());
        assert_ne!(runtime.grant_digest(), different_grants.grant_digest());
        assert!(matches!(
            authority.native_grant_projection(&different_grants),
            Err(ExtensionOperationAuthorityDenial::RuntimeFingerprintMismatch)
        ));
    }

    #[test]
    fn owned_native_grant_snapshot_is_shallow_bounded_and_reprojects_after_authority_drop() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        let manifest = projection_manifest(
            &["scripting"],
            &["activeTab"],
            &["https://required.example/*"],
            &["https://optional.example/*"],
            &[],
        );
        let (authority, runtime) = eligible_runtime_with_manifest(
            ProfileId::from(199),
            ExtensionInstallId::from(211),
            ExtensionRuntimeGeneration::new(223).unwrap(),
            Arc::clone(&manifest),
            &["scripting"],
            &["https://required.example/*"],
            false,
            false,
        );
        let manifest_owners_before_snapshot = Arc::strong_count(&manifest);
        let snapshot = authority
            .native_grant_projection(&runtime)
            .unwrap()
            .into_owned_snapshot();
        assert_eq!(
            Arc::strong_count(&manifest),
            manifest_owners_before_snapshot + 1
        );
        let witness = authority
            .mint_document_authority_witness(&runtime, ExtensionDocumentPurpose::ExecuteScript)
            .expect("structural snapshot creation leaves operation authority usable");
        assert!(witness.matches(&runtime, ExtensionDocumentPurpose::ExecuteScript));
        drop(witness);
        drop(authority);
        assert_eq!(
            Arc::strong_count(&manifest),
            manifest_owners_before_snapshot
        );

        assert_send::<ExtensionNativeGrantSnapshot>();
        assert_sync::<ExtensionNativeGrantSnapshot>();
        assert_eq!(snapshot.runtime(), &runtime);
        assert_eq!(snapshot.grant_revision(), runtime.grant_revision());
        assert_eq!(snapshot.grant_digest(), runtime.grant_digest());
        assert_eq!(
            snapshot.browsing_context(),
            ExtensionGrantBrowsingContext::Regular
        );
        assert_eq!(
            snapshot.operation_authority_companion_retained_bytes(),
            size_of::<ExtensionNativeGrantSnapshot>()
        );
        assert!(
            snapshot.retained_bytes() > snapshot.operation_authority_companion_retained_bytes()
        );
        assert_eq!(
            format!("{snapshot:?}"),
            "ExtensionNativeGrantSnapshot { runtime: \"<redacted>\", authority: \"<redacted>\" }"
        );

        assert_eq!(snapshot.api_grant_count(), 2);
        assert_eq!(snapshot.host_grant_count(), 2);
        assert_eq!(
            snapshot
                .api_grants()
                .find(|grant| grant.name().as_str() == "activeTab")
                .unwrap()
                .decision(),
            ExtensionNativeGrantDecision::Denied
        );
        assert_eq!(
            snapshot
                .host_grants()
                .find(|grant| grant.pattern().as_str() == "https://optional.example/*")
                .unwrap()
                .decision(),
            ExtensionNativeGrantDecision::Denied
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn native_host_projection_is_globally_sorted_and_deduplicated(
            explicit in prop::collection::vec(0_u8..12, 0..16),
            first_script in prop::collection::vec(0_u8..12, 0..16),
            second_script in prop::collection::vec(0_u8..12, 0..16),
            optional in prop::collection::vec(20_u8..32, 0..16),
        ) {
            fn canonical_hosts(values: Vec<u8>) -> Vec<String> {
                values
                    .into_iter()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .map(|value| format!("https://h{value}.example/*"))
                    .collect()
            }

            let explicit = canonical_hosts(explicit);
            let first_script = canonical_hosts(first_script);
            let second_script = canonical_hosts(second_script);
            let optional = canonical_hosts(optional);
            let explicit_refs = explicit.iter().map(String::as_str).collect::<Vec<_>>();
            let first_refs = first_script.iter().map(String::as_str).collect::<Vec<_>>();
            let second_refs = second_script.iter().map(String::as_str).collect::<Vec<_>>();
            let optional_refs = optional.iter().map(String::as_str).collect::<Vec<_>>();
            let script_refs: [&[&str]; 2] = [&first_refs, &second_refs];

            let manifest = projection_manifest(
                &[],
                &[],
                &explicit_refs,
                &optional_refs,
                &script_refs,
            );
            let required = explicit
                .iter()
                .chain(first_script.iter())
                .chain(second_script.iter())
                .cloned()
                .collect::<BTreeSet<_>>();
            let granted_optional = optional
                .iter()
                .filter(|pattern| pattern.as_bytes()[9] % 2 == 0)
                .cloned()
                .collect::<BTreeSet<_>>();
            let granted = required
                .iter()
                .chain(granted_optional.iter())
                .map(String::as_str)
                .collect::<Vec<_>>();
            let (authority, runtime) = eligible_runtime_with_manifest(
                ProfileId::from(157),
                ExtensionInstallId::from(163),
                ExtensionRuntimeGeneration::new(167).unwrap(),
                manifest,
                &[],
                &granted,
                false,
                false,
            );
            let projection = authority.native_grant_projection(&runtime).unwrap();
            let actual = projection
                .host_grants()
                .map(|grant| {
                    (
                        grant.pattern().as_str().to_owned(),
                        grant.requirement(),
                        grant.decision(),
                    )
                })
                .collect::<Vec<_>>();
            let mut expected = required
                .iter()
                .map(|pattern| {
                    (
                        pattern.clone(),
                        ExtensionNativeGrantRequirement::Required,
                        ExtensionNativeGrantDecision::Granted,
                    )
                })
                .chain(optional.iter().map(|pattern| {
                    (
                        pattern.clone(),
                        ExtensionNativeGrantRequirement::Optional,
                        if granted_optional.contains(pattern) {
                            ExtensionNativeGrantDecision::Granted
                        } else {
                            ExtensionNativeGrantDecision::Denied
                        },
                    )
                }))
                .collect::<Vec<_>>();
            expected.sort_unstable_by(|left, right| left.0.cmp(&right.0));

            prop_assert_eq!(projection.host_grant_count(), actual.len());
            prop_assert_eq!(actual, expected);
        }
    }

    #[test]
    fn capability_debug_output_redacts_runtime_and_grant_identity() {
        let (authority, runtime) = eligible_runtime(
            ProfileId::from(79),
            ExtensionInstallId::from(83),
            ExtensionRuntimeGeneration::new(89).unwrap(),
            &["activeTab", "scripting"],
            false,
        );
        let active_tab = authority
            .mint_active_tab_grant_witness(&runtime, ExtensionUserInvocationKind::ToolbarAction)
            .unwrap();
        let document = authority
            .mint_document_authority_witness(&runtime, ExtensionDocumentPurpose::RemoveCss)
            .unwrap();

        assert_eq!(
            format!("{active_tab:?}"),
            "ExtensionActiveTabGrantWitness { runtime: \"<redacted>\", invocation: ToolbarAction }"
        );
        assert_eq!(
            format!("{document:?}"),
            "ExtensionDocumentAuthorityWitness { runtime: \"<redacted>\", purpose: RemoveCss, authority: \"<redacted>\" }"
        );
    }
}
