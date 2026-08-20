//! Process-local extension runtime and user-invocation vocabulary.
//!
//! These values are deliberately non-persistent and non-authorizing. They
//! let the service and native engine correlate one exact runtime generation
//! and choose from closed, audited operation purposes without accepting API
//! permission names or invocation kinds from extension JavaScript.

use crate::ids::{ExtensionInstallId, ProfileId};

use super::{
    ExtensionGrantBrowsingContext, ExtensionGrantDigest, ExtensionGrantRevision,
    ExtensionInstallCatalogRevision, ExtensionInstallRevision, ExtensionPackageIdentity,
    ExtensionProfilePolicyDigest, ExtensionProfilePolicyRevision,
};

/// Process-local, non-wrapping identity for one extension runtime generation.
///
/// A late native callback from a disabled, updated, or restarted extension
/// must never alias a replacement generation. Exhaustion therefore requires
/// retiring extension execution for the process instead of wrapping.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExtensionRuntimeGeneration(u64);

impl ExtensionRuntimeGeneration {
    /// First legal process-local runtime generation.
    pub const INITIAL: Self = Self(1);

    /// Constructs a nonzero process-local generation.
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 {
            None
        } else {
            Some(Self(value))
        }
    }

    /// Returns the opaque numeric generation for native correlation only.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next generation, refusing counter wrap.
    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }
}

/// Compact map key for one profile-scoped runtime generation.
///
/// This value is freely copyable because it is identity, not authority. A
/// broker must additionally retain the exact eligibility, authenticated
/// package lease, and native owner associated with this key.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExtensionRuntimeInstance {
    profile: ProfileId,
    install_id: ExtensionInstallId,
    generation: ExtensionRuntimeGeneration,
}

impl ExtensionRuntimeInstance {
    /// Constructs a non-authorizing runtime identity.
    pub const fn new(
        profile: ProfileId,
        install_id: ExtensionInstallId,
        generation: ExtensionRuntimeGeneration,
    ) -> Self {
        Self {
            profile,
            install_id,
            generation,
        }
    }

    /// Exact profile that owns the runtime.
    pub const fn profile(self) -> ProfileId {
        self.profile
    }

    /// Stable profile-scoped installation identity.
    pub const fn install_id(self) -> ExtensionInstallId {
        self.install_id
    }

    /// Exact process-local runtime generation.
    pub const fn generation(self) -> ExtensionRuntimeGeneration {
        self.generation
    }
}

/// Non-authorizing reconciliation fingerprint for one exact runtime input.
///
/// The compact [`ExtensionRuntimeInstance`] is insufficient to compare a live
/// runtime with durable state. This fingerprint captures every store-bound
/// input whose change must retire the old native owner. Possession still does
/// not prove package authentication, native activation, or operation access.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionRuntimeFingerprint {
    instance: ExtensionRuntimeInstance,
    catalog_revision: ExtensionInstallCatalogRevision,
    install_revision: ExtensionInstallRevision,
    grant_revision: ExtensionGrantRevision,
    grant_digest: ExtensionGrantDigest,
    profile_policy_revision: ExtensionProfilePolicyRevision,
    profile_policy_digest: ExtensionProfilePolicyDigest,
    package: ExtensionPackageIdentity,
    browsing_context: ExtensionGrantBrowsingContext,
}

pub(super) struct ExtensionRuntimeFingerprintInput {
    pub(super) instance: ExtensionRuntimeInstance,
    pub(super) catalog_revision: ExtensionInstallCatalogRevision,
    pub(super) install_revision: ExtensionInstallRevision,
    pub(super) grant_revision: ExtensionGrantRevision,
    pub(super) grant_digest: ExtensionGrantDigest,
    pub(super) profile_policy_revision: ExtensionProfilePolicyRevision,
    pub(super) profile_policy_digest: ExtensionProfilePolicyDigest,
    pub(super) package: ExtensionPackageIdentity,
    pub(super) browsing_context: ExtensionGrantBrowsingContext,
}

impl ExtensionRuntimeFingerprint {
    pub(super) fn from_eligibility(input: ExtensionRuntimeFingerprintInput) -> Self {
        let ExtensionRuntimeFingerprintInput {
            instance,
            catalog_revision,
            install_revision,
            grant_revision,
            grant_digest,
            profile_policy_revision,
            profile_policy_digest,
            package,
            browsing_context,
        } = input;
        Self {
            instance,
            catalog_revision,
            install_revision,
            grant_revision,
            grant_digest,
            profile_policy_revision,
            profile_policy_digest,
            package,
            browsing_context,
        }
    }

    /// Compact process-local runtime map key.
    pub const fn instance(&self) -> ExtensionRuntimeInstance {
        self.instance
    }

    /// Complete install-catalog revision observed by the runtime.
    pub const fn catalog_revision(&self) -> ExtensionInstallCatalogRevision {
        self.catalog_revision
    }

    /// Exact durable install-row revision observed by the runtime.
    pub const fn install_revision(&self) -> ExtensionInstallRevision {
        self.install_revision
    }

    /// Exact durable grant-row revision observed by the runtime.
    pub const fn grant_revision(&self) -> ExtensionGrantRevision {
        self.grant_revision
    }

    /// Digest of the complete grant authority observed by the runtime.
    pub const fn grant_digest(&self) -> ExtensionGrantDigest {
        self.grant_digest
    }

    pub const fn profile_policy_revision(&self) -> ExtensionProfilePolicyRevision {
        self.profile_policy_revision
    }

    pub const fn profile_policy_digest(&self) -> ExtensionProfilePolicyDigest {
        self.profile_policy_digest
    }

    /// Complete immutable package identity expected by the runtime.
    pub const fn package(&self) -> &ExtensionPackageIdentity {
        &self.package
    }

    /// Browsing partition for which this runtime was admitted.
    pub const fn browsing_context(&self) -> ExtensionGrantBrowsingContext {
        self.browsing_context
    }
}

/// Trusted browser invocation that may establish transient extension access.
///
/// The initial surface is intentionally narrow. Page content and extension
/// JavaScript must never deserialize or select this value; privileged browser
/// chrome chooses it at a user-gesture boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExtensionUserInvocationKind {
    /// A user invoked the extension's browser-toolbar action.
    ToolbarAction,
}

impl ExtensionUserInvocationKind {
    /// API permission that lets this invocation mint transient host scope.
    ///
    /// The trusted action event itself may still be dispatched when the
    /// permission is absent. This mapping is only for the separate active-tab
    /// grant broker and must never become action-dispatch admission.
    pub const fn transient_grant_api_name(self) -> &'static str {
        match self {
            Self::ToolbarAction => "activeTab",
        }
    }
}

/// Closed purpose for one transient, native-document-scoped operation.
///
/// Host scope and an API declaration are independent. A broker must require
/// the API named here and separately join either durable URL scope or a valid
/// active-tab origin witness before asking the engine for execution.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExtensionDocumentPurpose {
    /// Execute admitted JavaScript through the `scripting` API.
    ExecuteScript,
    /// Insert admitted CSS through the `scripting` API.
    InsertCss,
    /// Remove a previously inserted CSS registration through `scripting`.
    RemoveCss,
}

impl ExtensionDocumentPurpose {
    /// Exact manifest API permission required for this operation.
    pub const fn required_api_name(self) -> &'static str {
        match self {
            Self::ExecuteScript | Self::InsertCss | Self::RemoveCss => "scripting",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_generations_are_nonzero_and_never_wrap() {
        assert_eq!(ExtensionRuntimeGeneration::new(0), None);
        assert_eq!(ExtensionRuntimeGeneration::INITIAL.get(), 1);
        assert_eq!(
            ExtensionRuntimeGeneration::INITIAL
                .next()
                .map(|value| value.get()),
            Some(2)
        );
        assert_eq!(
            ExtensionRuntimeGeneration::new(u64::MAX)
                .expect("maximum nonzero generation")
                .next(),
            None
        );
    }

    #[test]
    fn runtime_instance_is_only_the_exact_compact_key() {
        let instance = ExtensionRuntimeInstance::new(
            ProfileId::from(7),
            ExtensionInstallId::from(9),
            ExtensionRuntimeGeneration::new(11).unwrap(),
        );
        assert_eq!(instance.profile(), ProfileId::from(7));
        assert_eq!(instance.install_id(), ExtensionInstallId::from(9));
        assert_eq!(instance.generation().get(), 11);
    }

    #[test]
    fn invocation_and_document_purposes_map_to_closed_api_names() {
        assert_eq!(
            ExtensionUserInvocationKind::ToolbarAction.transient_grant_api_name(),
            "activeTab"
        );
        for purpose in [
            ExtensionDocumentPurpose::ExecuteScript,
            ExtensionDocumentPurpose::InsertCss,
            ExtensionDocumentPurpose::RemoveCss,
        ] {
            assert_eq!(purpose.required_api_name(), "scripting");
        }
    }
}
