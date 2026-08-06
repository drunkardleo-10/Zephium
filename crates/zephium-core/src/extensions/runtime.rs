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
    ExtensionCatalogSetDigest, ExtensionDocumentPurpose, ExtensionGrantAuthority,
    ExtensionGrantBrowsingContext, ExtensionGrantDigest, ExtensionGrantRevision,
    ExtensionInstallCatalogRevision, ExtensionInstallRevision, ExtensionManifestDescriptor,
    ExtensionNativeIncarnation, ExtensionNativeOwnershipEntry, ExtensionNativeOwnershipOperation,
    ExtensionPackageIdentity, ExtensionRuntimeBackendTarget, ExtensionRuntimeFingerprint,
    ExtensionRuntimeGeneration, ExtensionRuntimeInstance, ExtensionUrlScopeDecision,
    ExtensionUserInvocationKind,
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

    /// Checks the complete non-fresh Store/package-pin lineage carried by this
    /// authority against one structural native-ownership row.
    ///
    /// This does not prove the row is current. It prevents a capability from a
    /// different catalog generation, backend, journal operation, or native
    /// incarnation from being routed into this owner even when both share the
    /// same user-facing runtime fingerprint.
    #[must_use]
    pub fn matches_native_ownership_lineage(&self, entry: &ExtensionNativeOwnershipEntry) -> bool {
        let instance = self.fingerprint.instance();
        entry.key().profile() == instance.profile()
            && entry.key().install_id() == instance.install_id()
            && entry.key().browsing_context() == self.fingerprint.browsing_context()
            && entry.package() == self.fingerprint.package()
            && entry.store_catalog_revision() == self.fingerprint.catalog_revision()
            && entry.store_install_revision() == self.fingerprint.install_revision()
            && entry.store_grant_revision() == self.fingerprint.grant_revision()
            && entry.grant_digest() == self.fingerprint.grant_digest()
            && self.lineage.matches_entry(entry)
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

    pub(super) const fn eligibility(&self) -> &ExtensionRuntimeEligibility {
        &self.eligibility
    }

    pub(super) fn into_eligibility(self) -> ExtensionRuntimeEligibility {
        self.eligibility
    }
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
        ExtensionCompatibilityTargetId, ExtensionContentSecurityPolicyDeclaration,
        ExtensionGrantCohort, ExtensionGrantDenial, ExtensionGrantManifestBinding,
        ExtensionGrantManifestBindings, ExtensionHostPermissionSet, ExtensionInstall,
        ExtensionInstallCatalog, ExtensionManifestDeclarations, ExtensionManifestDigest,
        ExtensionManifestExecutionSurfaces, ExtensionManifestResourceDigest, ExtensionPackageKey,
        ExtensionPackagePayloadIdentity, ExtensionPackageRevision, ExtensionTreeDigest,
    };
    use crate::injection::{MatchOptions, MatchPattern, MatchSet};

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
        let declarations = ExtensionManifestDeclarations::new(
            api(&[]),
            api(&["activeTab", "scripting"]),
            None,
            Some(hosts(&[ALL_URLS])),
            None,
            None,
            Vec::new(),
            ExtensionManifestExecutionSurfaces::new(
                Vec::new(),
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
                ExtensionCompatibilityTargetId::parse_exact("test.runtime.authority.v1").unwrap(),
                compatibility,
            )
            .unwrap(),
        )
    }

    fn cohort(
        profile: ProfileId,
        install_id: ExtensionInstallId,
        granted_api: &[&str],
        file_access: bool,
        private_access: bool,
    ) -> ExtensionGrantCohort {
        let manifest = manifest();
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
        let authority = ExtensionGrantAuthority::initialize(
            &install,
            granted_api
                .iter()
                .map(|name| ApiPermissionName::parse_exact(name).unwrap())
                .collect(),
            vec![MatchPattern::parse(ALL_URLS).unwrap()],
            file_access,
            private_access,
            &manifest,
        )
        .unwrap();
        ExtensionGrantCohort::from_persisted(profile, catalog, bindings, vec![authority]).unwrap()
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
