//! Product-only composition for curated acquired extension packages.
//!
//! This module is absent from the default desktop dependency graph. It owns
//! the only product slot for fixed origins and platform runtime selection, but
//! contains no network, repository, Store, profile, or native-view authority.

#[cfg(not(feature = "staging-extension-catalog"))]
use std::time::Duration;

#[cfg(not(feature = "staging-extension-catalog"))]
use url::Url;
use zephium_app::CallbackHandle;
use zephium_extension_authority::{
    ProductExtensionManifestAuthority, ProductExtensionRuntimeTarget,
};
use zephium_extension_distribution::ExtensionDistributionClient;
use zephium_extension_updater::{ExtensionDistributionPlan, ExtensionDistributionWorker};

#[cfg(not(feature = "staging-extension-catalog"))]
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(not(feature = "staging-extension-catalog"))]
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

struct SealedProductDistributionConfiguration {
    #[cfg(not(feature = "staging-extension-catalog"))]
    metadata_base: &'static str,
    #[cfg(not(feature = "staging-extension-catalog"))]
    targets_base: &'static str,
    runtime_targets: &'static [ProductExtensionRuntimeTarget],
}

/// Constructs the dormant product worker without issuing I/O.
pub(super) fn launch(
    shell: CallbackHandle,
) -> std::io::Result<Option<ExtensionDistributionWorker>> {
    let Some(configuration) = sealed_product_distribution_configuration() else {
        return Ok(None);
    };
    let authority = ProductExtensionManifestAuthority::product()
        .map_err(|_| invalid_configuration("manifest authority"))?;
    let selections = authority
        .active_acquired_runtime_selections_for_targets(configuration.runtime_targets)
        .map_err(|_| invalid_configuration("runtime selection"))?;
    #[cfg(not(feature = "staging-extension-catalog"))]
    let client = {
        let metadata_base = Url::parse(configuration.metadata_base)
            .map_err(|_| invalid_configuration("metadata origin"))?;
        let targets_base = Url::parse(configuration.targets_base)
            .map_err(|_| invalid_configuration("target origin"))?;
        ExtensionDistributionClient::new(
            metadata_base,
            targets_base,
            REQUEST_TIMEOUT,
            CONNECT_TIMEOUT,
        )
        .map_err(|_| invalid_configuration("distribution client"))?
    };
    #[cfg(feature = "staging-extension-catalog")]
    let client = ExtensionDistributionClient::staging()
        .map_err(|_| invalid_configuration("staging distribution client"))?;
    let plan = ExtensionDistributionPlan::new(client, selections)
        .map_err(|_| invalid_configuration("update plan"))?;
    ExtensionDistributionWorker::launch(plan, shell)
        .map(Some)
        .map_err(|_| invalid_configuration("worker launch"))
}

fn invalid_configuration(component: &'static str) -> std::io::Error {
    std::io::Error::other(format!(
        "curated extension distribution {component} is invalid"
    ))
}

// Deliberately absent until fixed production origins, an exact acquired
// catalog, corresponding manifest profiles, redistribution review, and the
// platform target policy land atomically. Never replace this with environment,
// command-line, preferences, or remotely supplied configuration.
#[cfg(not(feature = "staging-extension-catalog"))]
fn sealed_product_distribution_configuration() -> Option<SealedProductDistributionConfiguration> {
    None
}

#[cfg(feature = "staging-extension-catalog")]
fn sealed_product_distribution_configuration() -> Option<SealedProductDistributionConfiguration> {
    const MACOS_STAGING_TARGETS: &[ProductExtensionRuntimeTarget] = &[
        ProductExtensionRuntimeTarget::MacosNative,
        ProductExtensionRuntimeTarget::MacosNativeBrokered,
    ];
    Some(SealedProductDistributionConfiguration {
        runtime_targets: MACOS_STAGING_TARGETS,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(not(feature = "staging-extension-catalog"))]
    fn unprovisioned_build_has_no_network_or_worker_configuration() {
        assert!(super::sealed_product_distribution_configuration().is_none());
    }

    #[test]
    #[cfg(feature = "staging-extension-catalog")]
    fn staging_build_selects_the_complete_native_macos_profile_cohort() {
        let configuration = super::sealed_product_distribution_configuration().unwrap();
        assert_eq!(
            configuration.runtime_targets,
            [
                zephium_extension_authority::ProductExtensionRuntimeTarget::MacosNative,
                zephium_extension_authority::ProductExtensionRuntimeTarget::MacosNativeBrokered,
            ]
        );
    }
}
