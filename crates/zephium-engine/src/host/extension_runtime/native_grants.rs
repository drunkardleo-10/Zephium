//! Engine-owned structural grants retained beside one activation reservation.

use std::fmt;
use std::mem::size_of;

use zephium_core::extensions::{
    ExtensionNativeGrantRequirement, ExtensionNativeGrantSnapshot, ExtensionRuntimeBackendTarget,
    ExtensionRuntimeFingerprint,
};
use zephium_extension_runtime_api::{
    ExtensionRuntimeHostActivationContext, ExtensionRuntimeHostBindError,
    ExtensionRuntimeNativeIdentityExpectation, ExtensionRuntimeTarget,
};

/// Move-only structural grant state retained by one exact engine reservation.
///
/// This value is not operation authority. Its shared manifest and grant
/// allocations remain charged through the matching linear authority state for
/// the complete lifetime of the reservation.
pub(super) struct EngineNativeGrantSnapshot {
    grants: ExtensionNativeGrantSnapshot,
}

impl EngineNativeGrantSnapshot {
    /// Validates and consumes one ephemeral trusted-host context without native work.
    pub(super) fn try_from_context(
        context: ExtensionRuntimeHostActivationContext<'_>,
    ) -> Result<Self, ExtensionRuntimeHostBindError> {
        if !activation_shape_is_consistent(&context)
            || !owner_matches_runtime(&context)
            || !complete_required_grants_are_satisfied(context.native_grants())
        {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }

        let expected_runtime = context.fingerprint().clone();
        let grants = context.into_native_grant_snapshot();
        if grants.runtime() != &expected_runtime {
            return Err(ExtensionRuntimeHostBindError::InternalInvariant);
        }
        Ok(Self { grants })
    }

    #[cfg(test)]
    pub(super) fn from_valid_core_snapshot_for_test(grants: ExtensionNativeGrantSnapshot) -> Self {
        assert!(complete_required_snapshot_grants_are_satisfied(&grants));
        Self { grants }
    }

    pub(super) const fn runtime(&self) -> &ExtensionRuntimeFingerprint {
        self.grants.runtime()
    }

    #[cfg(test)]
    pub(super) const fn grants(&self) -> &ExtensionNativeGrantSnapshot {
        &self.grants
    }

    /// Exact reservation storage charged beside the matching authority state.
    ///
    /// The shared manifest/grant allocations are already charged in full by
    /// every API control state that can coexist with this reservation. This
    /// method therefore includes only this wrapper and snapshot's new inline
    /// storage. The enclosing reservation separately charges its Box.
    pub(super) const fn operation_authority_companion_retained_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_sub(size_of::<ExtensionNativeGrantSnapshot>())
            .saturating_add(self.grants.operation_authority_companion_retained_bytes())
    }
}

impl fmt::Debug for EngineNativeGrantSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EngineNativeGrantSnapshot")
            .field("runtime", &"<redacted>")
            .field("grants", &"<redacted>")
            .finish()
    }
}

fn activation_shape_is_consistent(context: &ExtensionRuntimeHostActivationContext<'_>) -> bool {
    matches!(
        (
            context.owner().backend(),
            context.target(),
            context.identity_expectation(),
        ),
        (
            ExtensionRuntimeBackendTarget::MacosNative,
            ExtensionRuntimeTarget::NativeWebExtension,
            ExtensionRuntimeNativeIdentityExpectation::MacosWebExtension(_),
        ) | (
            ExtensionRuntimeBackendTarget::WindowsNative,
            ExtensionRuntimeTarget::NativeWebExtension,
            ExtensionRuntimeNativeIdentityExpectation::WindowsWebView2Extension(_),
        ) | (
            ExtensionRuntimeBackendTarget::MacosCompatibility
                | ExtensionRuntimeBackendTarget::LinuxCompatibility,
            ExtensionRuntimeTarget::Compatibility,
            ExtensionRuntimeNativeIdentityExpectation::Compatibility,
        )
    )
}

fn owner_matches_runtime(context: &ExtensionRuntimeHostActivationContext<'_>) -> bool {
    let key = context.owner().cas().key();
    let runtime = context.fingerprint();
    let instance = runtime.instance();
    key.profile() == instance.profile()
        && key.install_id() == instance.install_id()
        && key.browsing_context() == runtime.browsing_context()
        && context.native_grants().grant_revision() == runtime.grant_revision()
        && context.native_grants().grant_digest() == runtime.grant_digest()
        && context.native_grants().browsing_context() == runtime.browsing_context()
}

fn complete_required_grants_are_satisfied(
    projection: &zephium_core::extensions::ExtensionNativeGrantProjection<'_>,
) -> bool {
    let mut api_count = 0_usize;
    for grant in projection.api_grants() {
        let Some(next) = api_count.checked_add(1) else {
            return false;
        };
        api_count = next;
        if grant.requirement() == ExtensionNativeGrantRequirement::Required
            && !grant.decision().is_granted()
        {
            return false;
        }
    }
    if api_count != projection.api_grant_count() {
        return false;
    }

    let mut host_count = 0_usize;
    for grant in projection.host_grants() {
        let Some(next) = host_count.checked_add(1) else {
            return false;
        };
        host_count = next;
        if grant.requirement() == ExtensionNativeGrantRequirement::Required
            && !grant.decision().is_granted()
        {
            return false;
        }
    }
    host_count == projection.host_grant_count()
}

#[cfg(test)]
fn complete_required_snapshot_grants_are_satisfied(
    snapshot: &ExtensionNativeGrantSnapshot,
) -> bool {
    snapshot.api_grants().all(|grant| {
        grant.requirement() != ExtensionNativeGrantRequirement::Required
            || grant.decision().is_granted()
    }) && snapshot.host_grants().all(|grant| {
        grant.requirement() != ExtensionNativeGrantRequirement::Required
            || grant.decision().is_granted()
    })
}
