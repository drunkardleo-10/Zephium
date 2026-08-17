use zephium_app::CallbackHandle;
use zephium_core::ports::extensions::{
    ExtensionAcquiredCatalogActivationCallback, ExtensionAcquiredCatalogActivationRequest,
    ExtensionAcquiredPackageProvisioningCallback, ExtensionAcquiredPackageProvisioningRequest,
    ExtensionManagementAdmission,
};
use zephium_extension_distribution::ExtensionDistributionServicePort;

/// Non-blocking adapter from the distribution coordinator to the Shell actor.
///
/// The contained callback handle is weak: retaining this port cannot keep the
/// browser actor, native engine, Store, or extension service alive.
#[derive(Clone)]
pub struct ShellExtensionDistributionPort {
    shell: CallbackHandle,
}

impl ShellExtensionDistributionPort {
    /// Binds the coordinator to one already published Shell actor.
    pub const fn new(shell: CallbackHandle) -> Self {
        Self { shell }
    }
}

impl ExtensionDistributionServicePort for ShellExtensionDistributionPort {
    fn begin_provision_acquired_package(
        &self,
        request: ExtensionAcquiredPackageProvisioningRequest,
        deadline: std::time::Instant,
        done: ExtensionAcquiredPackageProvisioningCallback,
    ) -> ExtensionManagementAdmission {
        self.shell
            .begin_provision_acquired_extension_package(request, deadline, done)
    }

    fn begin_activate_acquired_catalog(
        &self,
        request: ExtensionAcquiredCatalogActivationRequest,
        deadline: std::time::Instant,
        done: ExtensionAcquiredCatalogActivationCallback,
    ) -> ExtensionManagementAdmission {
        self.shell
            .begin_activate_acquired_extension_catalog(request, deadline, done)
    }
}
