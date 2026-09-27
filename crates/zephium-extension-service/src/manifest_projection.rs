//! Shared, bounded compatibility presentation for reviewed and external packages.
use zephium_core::extensions::ExtensionManifestDeclaration;
use zephium_core::ports::extensions::{
    ExtensionManagementCompatibility, ExtensionManagementLimitation,
};

pub(crate) fn compatibility(
    manifest: &zephium_core::extensions::ExtensionManifestDescriptor,
) -> Option<(
    ExtensionManagementCompatibility,
    Vec<ExtensionManagementLimitation>,
)> {
    let compatibility = ExtensionManagementCompatibility::from_levels(
        manifest
            .compatibility()
            .iter()
            .map(|classification| classification.level()),
    )?;
    let mut limitations = Vec::new();
    for classification in manifest.compatibility().iter().filter(|classification| {
        classification.level() == zephium_core::extensions::ExtensionCompatibilityLevel::Degraded
    }) {
        let limitation = match classification.declaration() {
            ExtensionManifestDeclaration::RequiredApiPermission(name)
            | ExtensionManifestDeclaration::OptionalApiPermission(name) => {
                ExtensionManagementLimitation::api_permission(name.as_str()).ok()?
            }
            ExtensionManifestDeclaration::RequiredHostPermission(_)
            | ExtensionManifestDeclaration::OptionalHostPermission(_) => {
                ExtensionManagementLimitation::HostAccess
            }
            ExtensionManifestDeclaration::Background => ExtensionManagementLimitation::Background,
            ExtensionManifestDeclaration::Action => ExtensionManagementLimitation::Action,
            ExtensionManifestDeclaration::Offscreen => ExtensionManagementLimitation::Offscreen,
            ExtensionManifestDeclaration::NativeMessaging => {
                ExtensionManagementLimitation::NativeMessaging
            }
            ExtensionManifestDeclaration::Override(_) => {
                ExtensionManagementLimitation::BrowserOverride
            }
            ExtensionManifestDeclaration::ExtensionPagesCsp => {
                ExtensionManagementLimitation::ExtensionPagesCsp
            }
            ExtensionManifestDeclaration::Sandbox => ExtensionManagementLimitation::Sandbox,
            ExtensionManifestDeclaration::ContentScript { .. } => {
                ExtensionManagementLimitation::ContentScripts
            }
            ExtensionManifestDeclaration::WebAccessibleResources { .. } => {
                ExtensionManagementLimitation::WebAccessibleResources
            }
            ExtensionManifestDeclaration::MinimumChromiumVersion(_) => {
                ExtensionManagementLimitation::MinimumBrowserVersion
            }
            ExtensionManifestDeclaration::Commands(_) => ExtensionManagementLimitation::Commands,
            ExtensionManifestDeclaration::SidePanel { .. } => {
                ExtensionManagementLimitation::SidePanel
            }
            ExtensionManifestDeclaration::ManagedStorageSchema { .. } => {
                ExtensionManagementLimitation::ManagedStorage
            }
            ExtensionManifestDeclaration::OptionsPage { .. } => {
                ExtensionManagementLimitation::OptionsPage
            }
            ExtensionManifestDeclaration::DeclarativeNetRequest(_) => {
                ExtensionManagementLimitation::DeclarativeNetRequest
            }
            ExtensionManifestDeclaration::UnmodeledAuthority(_) => return None,
        };
        limitations.push(limitation);
    }
    Some((compatibility, limitations))
}
