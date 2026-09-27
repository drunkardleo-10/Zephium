//! Local-only, explicit removal of unavailable integration surfaces. Original
//! declarations remain Unsupported; only the fully reassessed output may run.
use super::BetaSourceAdmissionError as Error;
use zephium_core::extensions::{ApiPermissionName, ExtensionManifestDeclaration as D};
use zephium_extension_package::{parse_bounded_json, AdmittedExtensionManifest, BoundedJsonLimits};

#[derive(Default)]
pub(super) struct WithheldFeatures {
    pub(super) optional_api: Vec<ApiPermissionName>,
    pub(super) optional_hosts: Vec<Box<str>>,
    pub(super) external_messaging: bool,
}

impl WithheldFeatures {
    pub(super) fn plan(manifest: &AdmittedExtensionManifest, bytes: &[u8]) -> Result<Self, Error> {
        use zephium_core::extensions::ExtensionCompatibilityLevel::Unsupported;
        let mut result = Self::default();
        for row in manifest.descriptor().compatibility() {
            if row.level() != Unsupported {
                continue;
            }
            match row.declaration() {
                D::OptionalApiPermission(name) => result.optional_api.push(name.clone()),
                D::OptionalHostPermission(pattern) => {
                    result.optional_hosts.push(pattern.as_ref().into())
                }
                D::UnmodeledAuthority(name) if name.as_str() == "externally_connectable" => {
                    let root = parse_bounded_json(bytes, BoundedJsonLimits::extension_manifest())
                        .map_err(|_| Error::SourceMismatch)?
                        .into_value();
                    validate_external(&root["externally_connectable"])?;
                    result.external_messaging = true;
                }
                _ => {}
            }
        }
        Ok(result)
    }

    pub(super) fn removes(&self, declaration: &D, manifest: &AdmittedExtensionManifest) -> bool {
        match declaration {
            D::OptionalApiPermission(name) => self.optional_api.contains(name),
            D::OptionalHostPermission(pattern) => self
                .optional_hosts
                .iter()
                .any(|host| host.as_ref() == pattern.as_ref()),
            D::NativeMessaging => {
                !manifest
                    .descriptor()
                    .declarations()
                    .required_api()
                    .contains_exact("nativeMessaging")
                    && self
                        .optional_api
                        .iter()
                        .any(|name| name.as_str() == "nativeMessaging")
            }
            D::Offscreen => self.optional_only("offscreen", manifest),
            D::SidePanel { .. } => self.optional_only("sidePanel", manifest),
            D::UnmodeledAuthority(name) => {
                self.external_messaging && name.as_str() == "externally_connectable"
            }
            _ => false,
        }
    }

    fn optional_only(&self, name: &str, manifest: &AdmittedExtensionManifest) -> bool {
        !manifest
            .descriptor()
            .declarations()
            .required_api()
            .contains_exact(name)
            && self
                .optional_api
                .iter()
                .any(|permission| permission.as_str() == name)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.optional_api.is_empty() && self.optional_hosts.is_empty() && !self.external_messaging
    }
    pub(super) fn retained_bytes(&self) -> usize {
        self.optional_api.capacity() * std::mem::size_of::<ApiPermissionName>()
            + self
                .optional_api
                .iter()
                .map(|name| name.as_str().len())
                .sum::<usize>()
            + self.optional_hosts.capacity() * std::mem::size_of::<Box<str>>()
            + self
                .optional_hosts
                .iter()
                .map(|host| host.len())
                .sum::<usize>()
    }
    pub(super) fn evidence(&self) -> serde_json::Value {
        serde_json::json!({
            "optional_api": self.optional_api.iter().map(ApiPermissionName::as_str).collect::<Vec<_>>(),
            "optional_hosts": self.optional_hosts,
            "external_messaging": self.external_messaging,
        })
    }
}

fn validate_external(value: &serde_json::Value) -> Result<(), Error> {
    let object = value.as_object().ok_or(Error::SourceMismatch)?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "ids" | "matches" | "accepts_tls_channel_id"))
    {
        return Err(Error::SourceMismatch);
    }
    if object
        .get("accepts_tls_channel_id")
        .is_some_and(|value| !value.is_boolean())
    {
        return Err(Error::SourceMismatch);
    }
    for field in ["ids", "matches"] {
        let Some(value) = object.get(field) else {
            continue;
        };
        let values = value
            .as_array()
            .filter(|values| values.len() <= 128)
            .ok_or(Error::SourceMismatch)?;
        for value in values {
            let value = value.as_str().ok_or(Error::SourceMismatch)?;
            if field == "ids" {
                if value != "*"
                    && zephium_extension_package::ChromiumExtensionId::parse(value).is_err()
                {
                    return Err(Error::SourceMismatch);
                }
            } else if zephium_core::injection::MatchPattern::parse(value).is_err() {
                return Err(Error::SourceMismatch);
            }
        }
    }
    Ok(())
}
