//! Profile-scoped, store-snapshot runtime eligibility for extensions.
//!
//! Eligibility is deliberately weaker than activation authority. It proves
//! only that one exact install, admitted structural manifest, and grant row
//! came from the same complete store cohort and currently satisfy the durable
//! user-intent prerequisites. An authenticated repository package lease and a
//! backend runtime admission are still required before any native execution.

use std::sync::Arc;

use url::Url;

use crate::ids::{ExtensionInstallId, ProfileId};

use super::cohort::ExtensionGrantCohortEntry;
use super::transient::ExtensionRuntimeFingerprintInput;
use super::{
    ApiPermissionName, ExtensionApiGrantDecision, ExtensionGrantAuthority,
    ExtensionGrantBrowsingContext, ExtensionGrantDigest, ExtensionGrantRevision,
    ExtensionInstallCatalogRevision, ExtensionInstallRevision, ExtensionManifestDescriptor,
    ExtensionPackageIdentity, ExtensionRuntimeFingerprint, ExtensionRuntimeGeneration,
    ExtensionUrlScopeDecision,
};

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
