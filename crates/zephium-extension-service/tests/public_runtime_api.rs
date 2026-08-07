use std::time::Instant;

use zephium_core::extensions::ExtensionNativeOwnershipKey;
use zephium_extension_service::{
    ExtensionServiceOwner, ExtensionServiceRuntimeActivationOutcome,
    ExtensionServiceRuntimeRetirementOutcome,
};

#[test]
fn runtime_lifecycle_mutation_remains_public() {
    let activate: fn(
        &mut ExtensionServiceOwner,
        ExtensionNativeOwnershipKey,
        Instant,
    ) -> ExtensionServiceRuntimeActivationOutcome = ExtensionServiceOwner::activate_runtime_until;
    let retire: fn(
        &mut ExtensionServiceOwner,
        ExtensionNativeOwnershipKey,
        Instant,
    ) -> ExtensionServiceRuntimeRetirementOutcome = ExtensionServiceOwner::retire_runtime_until;

    let _ = (activate, retire);
}
