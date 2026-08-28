//! Exact private extension-lab inputs generated beneath `target/`.
//!
//! This module is compiled only by the separately identified, non-shipping
//! macOS lab build. The fixed root is deliberate: neither a caller nor runtime
//! configuration can redirect product authority to arbitrary bytes.

use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use zephium_core::extensions::{ExtensionCompatibilityLevel, ExtensionManifestDeclaration};
use zephium_extension_package::{
    parse_bounded_json, BoundedJsonLimits, PortableRelativePath,
    MAX_EXTENSION_RELEASE_CATALOG_BYTES,
};

const ACTIVE_CATALOG_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/zephium-extension-lab/v1/metadata/catalog-v1.json"
));
const ACTIVE_MANIFEST_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/zephium-extension-lab/v1/product/manifest.json"
));
const ACTIVE_TREE_INDEX_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/zephium-extension-lab/v1/product/tree-index.json"
));
const ACTIVE_CLASSIFIED_PROFILE_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/zephium-extension-lab/v1/product/classified-manifest-profiles-v1.json"
));
const ROLLBACK_CATALOG_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/zephium-extension-lab/v1/rollback/metadata/catalog-v1.json"
));
const ROLLBACK_MANIFEST_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/zephium-extension-lab/v1/rollback/product/manifest.json"
));
const ROLLBACK_TREE_INDEX_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/zephium-extension-lab/v1/rollback/product/tree-index.json"
));
const ROLLBACK_CLASSIFIED_PROFILE_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/zephium-extension-lab/v1/rollback/product/classified-manifest-profiles-v1.json"
));

#[derive(Clone, Copy)]
pub(crate) struct LocalLabGeneration {
    pub(crate) catalog_bytes: &'static [u8],
    pub(crate) manifest_bytes: &'static [u8],
    pub(crate) tree_index_bytes: &'static [u8],
    classified_profile_bytes: &'static [u8],
}

pub(crate) const ACTIVE_GENERATION: LocalLabGeneration = LocalLabGeneration {
    catalog_bytes: ACTIVE_CATALOG_BYTES,
    manifest_bytes: ACTIVE_MANIFEST_BYTES,
    tree_index_bytes: ACTIVE_TREE_INDEX_BYTES,
    classified_profile_bytes: ACTIVE_CLASSIFIED_PROFILE_BYTES,
};

pub(crate) const ROLLBACK_GENERATION: LocalLabGeneration = LocalLabGeneration {
    catalog_bytes: ROLLBACK_CATALOG_BYTES,
    manifest_bytes: ROLLBACK_MANIFEST_BYTES,
    tree_index_bytes: ROLLBACK_TREE_INDEX_BYTES,
    classified_profile_bytes: ROLLBACK_CLASSIFIED_PROFILE_BYTES,
};

const SCHEMA: u32 = 1;
const KIND: &str = "zephium-extension-classified-manifest-profiles";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocalLabAdmissionPolicy {
    pub(crate) digest_sha256: String,
    pub(crate) license_rules: Vec<LocalLabLicenseRule>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocalLabLicenseRule {
    pub(crate) expression: String,
    pub(crate) corresponding_source_required: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocalLabProfileInput {
    pub(crate) package_key: String,
    pub(crate) package_revision: u64,
    pub(crate) manifest_sha256: String,
    pub(crate) tree_sha256: String,
    pub(crate) tree_index_sha256: String,
    pub(crate) tree_index_length: u64,
    pub(crate) manifest_target: String,
    pub(crate) tree_index_target: String,
    pub(crate) compatibility_target: Option<String>,
    pub(crate) compatibility_receipt_sha256: Option<String>,
    #[serde(default)]
    pub(crate) publisher_native_host: Option<LocalLabPublisherNativeHost>,
    pub(crate) declarations: Vec<LocalLabDeclarationReviewRow>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocalLabPublisherNativeHost {
    pub(crate) host_name: String,
    pub(crate) upstream_chromium_extension_id: String,
    pub(crate) macos_team_identifier: String,
    pub(crate) macos_signing_identifier: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocalLabProfile {
    pub(crate) input: LocalLabProfileInput,
    pub(crate) compatibility_digest: String,
    pub(crate) admission_digest: String,
    pub(crate) activatable: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocalLabConfiguration {
    schema: u32,
    kind: String,
    product_authority: bool,
    pub(crate) catalog_sha256: String,
    pub(crate) admission_policy: LocalLabAdmissionPolicy,
    all_activatable: bool,
    pub(crate) profiles: Vec<LocalLabProfile>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocalLabDeclarationReviewRow {
    declaration: LocalLabDeclarationReviewKey,
    level: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LocalLabDeclarationReviewKey {
    RequiredApi {
        name: String,
    },
    OptionalApi {
        name: String,
    },
    RequiredHost {
        pattern: String,
    },
    OptionalHost {
        pattern: String,
    },
    Background,
    Action,
    Offscreen,
    NativeMessaging,
    Override {
        target: String,
    },
    ExtensionPagesCsp,
    Sandbox,
    ContentScript {
        index: u16,
        descriptor_sha256: String,
    },
    WebAccessibleResources {
        index: u16,
        resources_sha256: String,
    },
    MinimumChromiumVersion {
        version: String,
    },
    Commands {
        count: usize,
        descriptor_sha256: String,
    },
    SidePanel {
        resource_sha256: String,
    },
    ManagedStorageSchema {
        resource_sha256: String,
    },
    OptionsPage {
        descriptor_sha256: String,
    },
    DeclarativeNetRequest {
        ruleset_count: usize,
        enabled_ruleset_count: usize,
        descriptor_sha256: String,
    },
    UnmodeledAuthority {
        name: String,
    },
}

impl LocalLabConfiguration {
    pub(crate) fn load(generation: &LocalLabGeneration) -> Result<Self, ()> {
        if generation.classified_profile_bytes.is_empty()
            || generation.classified_profile_bytes.len() > MAX_EXTENSION_RELEASE_CATALOG_BYTES
        {
            return Err(());
        }
        let bounded = parse_bounded_json(
            generation.classified_profile_bytes,
            BoundedJsonLimits::manifest_profile_review(),
        )
        .map_err(|_| ())?;
        let configuration: Self = serde_json::from_value(bounded.into_value()).map_err(|_| ())?;
        if configuration.schema != SCHEMA
            || configuration.kind != KIND
            || configuration.product_authority
            || !configuration.all_activatable
            || configuration.profiles.len() != 1
            || !configuration.profiles[0].activatable
            || configuration.admission_policy.license_rules.is_empty()
            || lower_hex(&Sha256::digest(generation.catalog_bytes)) != configuration.catalog_sha256
        {
            return Err(());
        }
        let profile = &configuration.profiles[0];
        let input = &profile.input;
        if PortableRelativePath::parse(&input.manifest_target).is_err()
            || PortableRelativePath::parse(&input.tree_index_target).is_err()
            || input.compatibility_target.as_deref().is_none()
            || input
                .compatibility_receipt_sha256
                .as_deref()
                .and_then(|digest| decode_lower_hex_32(digest).ok())
                .is_none()
            || decode_lower_hex_32(&profile.compatibility_digest).is_err()
            || decode_lower_hex_32(&profile.admission_digest).is_err()
        {
            return Err(());
        }
        Ok(configuration)
    }
}

impl LocalLabDeclarationReviewRow {
    pub(crate) fn classification(
        rows: &[Self],
        declaration: &ExtensionManifestDeclaration,
    ) -> Option<ExtensionCompatibilityLevel> {
        let key = LocalLabDeclarationReviewKey::from_declaration(declaration);
        let row = rows.iter().find(|row| row.declaration == key)?;
        match row.level.as_str() {
            "compatible" => Some(ExtensionCompatibilityLevel::Compatible),
            "degraded" => Some(ExtensionCompatibilityLevel::Degraded),
            "unsupported" => Some(ExtensionCompatibilityLevel::Unsupported),
            _ => None,
        }
    }
}

impl LocalLabDeclarationReviewKey {
    fn from_declaration(declaration: &ExtensionManifestDeclaration) -> Self {
        use ExtensionManifestDeclaration as Declaration;

        match declaration {
            Declaration::RequiredApiPermission(name) => Self::RequiredApi {
                name: name.as_str().to_owned(),
            },
            Declaration::OptionalApiPermission(name) => Self::OptionalApi {
                name: name.as_str().to_owned(),
            },
            Declaration::RequiredHostPermission(pattern) => Self::RequiredHost {
                pattern: pattern.as_ref().to_owned(),
            },
            Declaration::OptionalHostPermission(pattern) => Self::OptionalHost {
                pattern: pattern.as_ref().to_owned(),
            },
            Declaration::Background => Self::Background,
            Declaration::Action => Self::Action,
            Declaration::Offscreen => Self::Offscreen,
            Declaration::NativeMessaging => Self::NativeMessaging,
            Declaration::Override(target) => Self::Override {
                target: match target {
                    zephium_core::extensions::ExtensionOverrideTarget::NewTab => "new_tab",
                    zephium_core::extensions::ExtensionOverrideTarget::Bookmarks => "bookmarks",
                    zephium_core::extensions::ExtensionOverrideTarget::History => "history",
                }
                .to_owned(),
            },
            Declaration::ExtensionPagesCsp => Self::ExtensionPagesCsp,
            Declaration::Sandbox => Self::Sandbox,
            Declaration::ContentScript {
                index,
                descriptor_digest,
            } => Self::ContentScript {
                index: *index,
                descriptor_sha256: lower_hex(descriptor_digest.as_bytes()),
            },
            Declaration::WebAccessibleResources {
                index,
                resources_digest,
            } => Self::WebAccessibleResources {
                index: *index,
                resources_sha256: lower_hex(resources_digest.as_bytes()),
            },
            Declaration::MinimumChromiumVersion(version) => Self::MinimumChromiumVersion {
                version: version
                    .components()
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join("."),
            },
            Declaration::Commands(commands) => Self::Commands {
                count: commands.command_count(),
                descriptor_sha256: lower_hex(commands.descriptor_digest().as_bytes()),
            },
            Declaration::SidePanel { resource } => Self::SidePanel {
                resource_sha256: lower_hex(resource.as_bytes()),
            },
            Declaration::ManagedStorageSchema { resource } => Self::ManagedStorageSchema {
                resource_sha256: lower_hex(resource.as_bytes()),
            },
            Declaration::OptionsPage { descriptor } => Self::OptionsPage {
                descriptor_sha256: lower_hex(descriptor.as_bytes()),
            },
            Declaration::DeclarativeNetRequest(declaration) => Self::DeclarativeNetRequest {
                ruleset_count: declaration.ruleset_count(),
                enabled_ruleset_count: declaration.enabled_ruleset_count(),
                descriptor_sha256: lower_hex(declaration.descriptor_digest().as_bytes()),
            },
            Declaration::UnmodeledAuthority(name) => Self::UnmodeledAuthority {
                name: name.as_str().to_owned(),
            },
        }
    }
}

pub(crate) fn decode_lower_hex_32(value: &str) -> Result<[u8; 32], ()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(());
    }
    let mut output = [0_u8; 32];
    for (index, slot) in output.iter_mut().enumerate() {
        let high = decode_nibble(value.as_bytes()[index * 2])?;
        let low = decode_nibble(value.as_bytes()[index * 2 + 1])?;
        *slot = (high << 4) | low;
    }
    Ok(output)
}

fn decode_nibble(value: u8) -> Result<u8, ()> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(()),
    }
}

fn lower_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}
