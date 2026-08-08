//! Test-only authenticated activation assembly for engine registry fixtures.
//!
//! Product assembly remains repository-owned. The parent module includes this
//! file only under `cfg(test)`, and the workspace source guard pins this sole
//! test exception to this exact path and constructor count.

use zephium_core::extensions::{ExtensionNativeOwnershipEntry, ExtensionRuntimeOperationAuthority};
use zephium_extension_runtime_api::{
    ExtensionPackageAccess, ExtensionRuntimeHostActivationBinding,
    ExtensionRuntimeNativeIdentityExpectation,
};

pub(super) fn assemble_activation_binding(
    entry: ExtensionNativeOwnershipEntry,
    access: ExtensionPackageAccess,
    authority: ExtensionRuntimeOperationAuthority,
    expectation: ExtensionRuntimeNativeIdentityExpectation,
) -> ExtensionRuntimeHostActivationBinding {
    ExtensionRuntimeHostActivationBinding::try_from_authenticated_repository(
        entry,
        access,
        authority,
        expectation,
    )
    .expect("test fixture must carry exact authenticated activation authority")
}
