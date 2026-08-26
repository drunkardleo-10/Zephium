//! Path-free publisher native-host requirements.
//!
//! These values are structural metadata. They contain no executable path,
//! filesystem root, process handle, port, user grant, or native authority. A
//! sealed product manifest profile must bind one exact requirement before a
//! platform broker may attempt discovery and code-signature verification.

use std::error::Error;
use std::fmt;
use std::mem::size_of;

use super::ExtensionPackageIdentity;

/// Maximum bytes in one Chromium native-messaging host identifier.
pub const MAX_EXTENSION_NATIVE_HOST_NAME_BYTES: usize = 255;
/// Exact byte length of an Apple Developer Team ID.
pub const MAX_EXTENSION_MACOS_TEAM_IDENTIFIER_BYTES: usize = 10;
/// Maximum bytes in one macOS code-signing identifier.
pub const MAX_EXTENSION_MACOS_SIGNING_IDENTIFIER_BYTES: usize = 255;
/// Maximum logical retained bytes for one publisher-host requirement.
pub const MAX_EXTENSION_PUBLISHER_NATIVE_HOST_RETAINED_BYTES: usize = 2 * 1024;

/// Exact publisher identity required of a macOS native host executable.
#[derive(Clone, Eq, PartialEq)]
pub struct ExtensionMacosPublisherIdentity {
    team_identifier: Box<str>,
    signing_identifier: Box<str>,
    retained_bytes: usize,
}

impl ExtensionMacosPublisherIdentity {
    /// Constructs one strict Developer ID identity without granting trust.
    pub fn new(
        team_identifier: impl Into<Box<str>>,
        signing_identifier: impl Into<Box<str>>,
    ) -> Result<Self, ExtensionPublisherNativeHostError> {
        let team_identifier = team_identifier.into();
        let signing_identifier = signing_identifier.into();
        if team_identifier.len() != MAX_EXTENSION_MACOS_TEAM_IDENTIFIER_BYTES
            || !team_identifier
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        {
            return Err(ExtensionPublisherNativeHostError::InvalidTeamIdentifier);
        }
        if signing_identifier.is_empty()
            || signing_identifier.len() > MAX_EXTENSION_MACOS_SIGNING_IDENTIFIER_BYTES
            || !signing_identifier.is_ascii()
            || signing_identifier.starts_with('.')
            || signing_identifier.ends_with('.')
            || signing_identifier.contains("..")
            || !signing_identifier
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        {
            return Err(ExtensionPublisherNativeHostError::InvalidSigningIdentifier);
        }
        let retained_bytes = size_of::<Self>()
            .checked_add(team_identifier.len())
            .and_then(|bytes| bytes.checked_add(signing_identifier.len()))
            .ok_or(ExtensionPublisherNativeHostError::RetainedBytesExceeded)?;
        Ok(Self {
            team_identifier,
            signing_identifier,
            retained_bytes,
        })
    }

    /// Returns the exact Apple Developer Team ID.
    pub fn team_identifier(&self) -> &str {
        &self.team_identifier
    }

    /// Returns the exact code-signing identifier.
    pub fn signing_identifier(&self) -> &str {
        &self.signing_identifier
    }

    const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

impl fmt::Debug for ExtensionMacosPublisherIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionMacosPublisherIdentity")
            .field("team_identifier", &self.team_identifier)
            .field("signing_identifier", &self.signing_identifier)
            .finish()
    }
}

/// One exact package-bound publisher native-host requirement.
#[derive(Clone, Eq, PartialEq)]
pub struct ExtensionPublisherNativeHostRequirement {
    package: ExtensionPackageIdentity,
    host_name: Box<str>,
    upstream_chromium_extension_id: Box<str>,
    macos_publisher: ExtensionMacosPublisherIdentity,
    retained_bytes: usize,
}

impl ExtensionPublisherNativeHostRequirement {
    /// Constructs path-free metadata for later sealed product admission.
    pub fn new(
        package: ExtensionPackageIdentity,
        host_name: impl Into<Box<str>>,
        upstream_chromium_extension_id: impl Into<Box<str>>,
        macos_publisher: ExtensionMacosPublisherIdentity,
    ) -> Result<Self, ExtensionPublisherNativeHostError> {
        let host_name = host_name.into();
        let upstream_chromium_extension_id = upstream_chromium_extension_id.into();
        if !valid_host_name(&host_name) {
            return Err(ExtensionPublisherNativeHostError::InvalidHostName);
        }
        if upstream_chromium_extension_id.len() != 32
            || !upstream_chromium_extension_id
                .bytes()
                .all(|byte| matches!(byte, b'a'..=b'p'))
        {
            return Err(ExtensionPublisherNativeHostError::InvalidChromiumExtensionId);
        }
        let retained_bytes = size_of::<Self>()
            .checked_add(host_name.len())
            .and_then(|bytes| bytes.checked_add(upstream_chromium_extension_id.len()))
            .and_then(|bytes| bytes.checked_add(macos_publisher.retained_bytes()))
            .ok_or(ExtensionPublisherNativeHostError::RetainedBytesExceeded)?;
        if retained_bytes > MAX_EXTENSION_PUBLISHER_NATIVE_HOST_RETAINED_BYTES {
            return Err(ExtensionPublisherNativeHostError::RetainedBytesExceeded);
        }
        Ok(Self {
            package,
            host_name,
            upstream_chromium_extension_id,
            macos_publisher,
            retained_bytes,
        })
    }

    /// Returns the complete exact extension package identity.
    pub const fn package(&self) -> &ExtensionPackageIdentity {
        &self.package
    }

    /// Returns the only native host identifier this profile may request.
    pub fn host_name(&self) -> &str {
        &self.host_name
    }

    /// Returns the upstream authenticated Chromium extension identity.
    pub fn upstream_chromium_extension_id(&self) -> &str {
        &self.upstream_chromium_extension_id
    }

    /// Returns the required macOS publisher identity.
    pub const fn macos_publisher(&self) -> &ExtensionMacosPublisherIdentity {
        &self.macos_publisher
    }

    /// Returns the explicit logical retained-memory charge.
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

impl fmt::Debug for ExtensionPublisherNativeHostRequirement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionPublisherNativeHostRequirement")
            .field("package", &self.package)
            .field("host_name", &self.host_name)
            .field(
                "upstream_chromium_extension_id",
                &self.upstream_chromium_extension_id,
            )
            .field("macos_publisher", &self.macos_publisher)
            .field("retained_bytes", &self.retained_bytes)
            .finish()
    }
}

fn valid_host_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_EXTENSION_NATIVE_HOST_NAME_BYTES
        && !value.starts_with('.')
        && !value.ends_with('.')
        && !value.contains("..")
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.')
        })
}

/// Publisher native-host structural metadata failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionPublisherNativeHostError {
    /// The native host identifier violated Chromium's grammar.
    InvalidHostName,
    /// The upstream Chromium extension identifier was not exact lowercase `a` through `p`.
    InvalidChromiumExtensionId,
    /// The Apple Developer Team ID was not ten uppercase alphanumeric bytes.
    InvalidTeamIdentifier,
    /// The macOS code-signing identifier was invalid or unbounded.
    InvalidSigningIdentifier,
    /// Retained-memory accounting overflowed or exceeded its ceiling.
    RetainedBytesExceeded,
}

impl fmt::Display for ExtensionPublisherNativeHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidHostName => "publisher native host name is invalid",
            Self::InvalidChromiumExtensionId => {
                "publisher native host Chromium extension identity is invalid"
            }
            Self::InvalidTeamIdentifier => "publisher native host Team ID is invalid",
            Self::InvalidSigningIdentifier => "publisher native host signing identifier is invalid",
            Self::RetainedBytesExceeded => {
                "publisher native host retained-memory bound was exceeded"
            }
        })
    }
}

impl Error for ExtensionPublisherNativeHostError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::{
        ExtensionAuthorityId, ExtensionManifestDigest, ExtensionPackageKey,
        ExtensionPackagePayloadIdentity, ExtensionPackageRevision, ExtensionTreeDigest,
    };

    fn package() -> ExtensionPackageIdentity {
        ExtensionPackageIdentity::new(
            ExtensionAuthorityId::from_bytes([1; 32]),
            ExtensionPackageKey::from_bytes([2; 32]),
            ExtensionPackageRevision::new(3).unwrap(),
            ExtensionPackagePayloadIdentity::BundledTree,
            ExtensionManifestDigest::from_bytes([4; 32]),
            ExtensionTreeDigest::from_bytes([5; 32]),
        )
    }

    #[test]
    fn requirement_is_exact_path_free_and_package_bound() {
        let publisher =
            ExtensionMacosPublisherIdentity::new("A1B2C3D4E5", "com.example.browser-support")
                .unwrap();
        let requirement = ExtensionPublisherNativeHostRequirement::new(
            package(),
            "com.1password.1password",
            "aeblfdkhhhdcdjpifhhbdiojplfjncoa",
            publisher,
        )
        .unwrap();
        assert_eq!(requirement.package(), &package());
        assert_eq!(requirement.host_name(), "com.1password.1password");
        assert_eq!(
            requirement.macos_publisher().team_identifier(),
            "A1B2C3D4E5"
        );
        assert!(requirement.retained_bytes() <= MAX_EXTENSION_PUBLISHER_NATIVE_HOST_RETAINED_BYTES);
        assert!(!format!("{requirement:?}").contains("/Applications/"));
    }

    #[test]
    fn requirement_rejects_widened_or_ambiguous_identity() {
        let publisher =
            ExtensionMacosPublisherIdentity::new("A1B2C3D4E5", "com.example.browser-support")
                .unwrap();
        for name in ["", ".host", "host.", "host..peer", "Host", "host-peer"] {
            assert_eq!(
                ExtensionPublisherNativeHostRequirement::new(
                    package(),
                    name,
                    "aeblfdkhhhdcdjpifhhbdiojplfjncoa",
                    publisher.clone(),
                ),
                Err(ExtensionPublisherNativeHostError::InvalidHostName)
            );
        }
        assert_eq!(
            ExtensionMacosPublisherIdentity::new("TEAM", "com.example.host"),
            Err(ExtensionPublisherNativeHostError::InvalidTeamIdentifier)
        );
        assert_eq!(
            ExtensionPublisherNativeHostRequirement::new(
                package(),
                "com.example.host",
                "*",
                publisher,
            ),
            Err(ExtensionPublisherNativeHostError::InvalidChromiumExtensionId)
        );
    }
}
