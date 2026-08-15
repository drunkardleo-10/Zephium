//! Bounded transport contracts for curated acquired extension packages.

use std::fmt;
use std::mem::size_of;

use crate::extensions::{
    ExtensionCatalogSetDigest, ExtensionPackageKey, ExtensionRuntimeBackendTarget,
    MAX_EXTENSION_ARCHIVE_BYTES,
};

/// Maximum exact canonical catalog bytes retained by one provisioning request.
pub const MAX_EXTENSION_ACQUIRED_CATALOG_BYTES: usize = 256 * 1024;
/// Maximum CRX3 envelope retained by one provisioning request.
///
/// The archive payload is bounded by [`MAX_EXTENSION_ARCHIVE_BYTES`]. The
/// additional MiB intentionally exceeds the parser's independently asserted
/// CRX header ceiling while keeping ingress accounting stable in Core.
pub const MAX_EXTENSION_ACQUIRED_CRX_BYTES: usize =
    MAX_EXTENSION_ARCHIVE_BYTES as usize + 1024 * 1024;
/// Maximum exact legal-notice bytes retained by one provisioning request.
pub const MAX_EXTENSION_ACQUIRED_LEGAL_NOTICE_BYTES: usize = 4 * 1024 * 1024;
/// Maximum logical bytes retained by one pending acquired-package request.
pub const MAX_EXTENSION_ACQUIRED_PROVISIONING_RETAINED_BYTES: usize =
    MAX_EXTENSION_ACQUIRED_CATALOG_BYTES
        + MAX_EXTENSION_ACQUIRED_CRX_BYTES
        + MAX_EXTENSION_ACQUIRED_LEGAL_NOTICE_BYTES
        + 1024;

const MAX_ACQUIRED_CATALOG_SELECTIONS: usize = 8;

/// Closed reviewed package profile requested at the untrusted transport edge.
///
/// Profiles can share a durable native backend while carrying different
/// compatibility contracts. In particular, both macOS native profiles own
/// the same `WKWebExtension` resources; only the brokered profile may mint
/// Zephium compatibility-broker capabilities after product admission.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ExtensionAcquiredRuntimeProfile {
    /// Unmodified Apple native compatibility contract.
    MacosNative,
    /// Apple native runtime with Zephium's sealed one-shot adapter contract.
    MacosNativeBrokered,
    /// Zephium compatibility runtime hosted by `WKWebView`.
    MacosCompatibility,
    /// Zephium compatibility runtime hosted by WebKitGTK.
    LinuxCompatibility,
    /// WebView2's native extension runtime.
    WindowsNative,
}

impl ExtensionAcquiredRuntimeProfile {
    /// Durable native/runtime resource family used by this profile.
    pub const fn runtime_backend(self) -> ExtensionRuntimeBackendTarget {
        match self {
            Self::MacosNative | Self::MacosNativeBrokered => {
                ExtensionRuntimeBackendTarget::MacosNative
            }
            Self::MacosCompatibility => ExtensionRuntimeBackendTarget::MacosCompatibility,
            Self::LinuxCompatibility => ExtensionRuntimeBackendTarget::LinuxCompatibility,
            Self::WindowsNative => ExtensionRuntimeBackendTarget::WindowsNative,
        }
    }

    /// Existing unbrokered profile for one durable backend.
    pub const fn from_runtime_backend(backend: ExtensionRuntimeBackendTarget) -> Self {
        match backend {
            ExtensionRuntimeBackendTarget::MacosNative => Self::MacosNative,
            ExtensionRuntimeBackendTarget::MacosCompatibility => Self::MacosCompatibility,
            ExtensionRuntimeBackendTarget::LinuxCompatibility => Self::LinuxCompatibility,
            ExtensionRuntimeBackendTarget::WindowsNative => Self::WindowsNative,
        }
    }
}

/// One move-only, path-free package payload from an authenticated transport.
///
/// Construction enforces transport memory ceilings only. It does not parse or
/// trust any byte, grant installation authority, or authorize network access.
#[must_use = "acquired package bytes must be provisioned or deliberately discarded"]
pub struct ExtensionAcquiredPackageProvisioningRequest {
    catalog_bytes: Vec<u8>,
    package_key: ExtensionPackageKey,
    runtime_profile: ExtensionAcquiredRuntimeProfile,
    crx3_bytes: Vec<u8>,
    legal_notice_bytes: Vec<u8>,
    retained_bytes: usize,
}

impl ExtensionAcquiredPackageProvisioningRequest {
    /// Creates one bounded request without authenticating its contents.
    pub fn new(
        catalog_bytes: Vec<u8>,
        package_key: ExtensionPackageKey,
        runtime_backend: ExtensionRuntimeBackendTarget,
        crx3_bytes: Vec<u8>,
        legal_notice_bytes: Vec<u8>,
    ) -> Result<Self, ExtensionAcquiredProvisioningRequestError> {
        Self::new_for_profile(
            catalog_bytes,
            package_key,
            ExtensionAcquiredRuntimeProfile::from_runtime_backend(runtime_backend),
            crx3_bytes,
            legal_notice_bytes,
        )
    }

    /// Creates one bounded request for an exact reviewed runtime profile.
    pub fn new_for_profile(
        catalog_bytes: Vec<u8>,
        package_key: ExtensionPackageKey,
        runtime_profile: ExtensionAcquiredRuntimeProfile,
        crx3_bytes: Vec<u8>,
        legal_notice_bytes: Vec<u8>,
    ) -> Result<Self, ExtensionAcquiredProvisioningRequestError> {
        if catalog_bytes.is_empty()
            || catalog_bytes.len() > MAX_EXTENSION_ACQUIRED_CATALOG_BYTES
            || catalog_bytes.capacity() > MAX_EXTENSION_ACQUIRED_CATALOG_BYTES
        {
            return Err(ExtensionAcquiredProvisioningRequestError::InvalidCatalogBytes);
        }
        if crx3_bytes.is_empty()
            || crx3_bytes.len() > MAX_EXTENSION_ACQUIRED_CRX_BYTES
            || crx3_bytes.capacity() > MAX_EXTENSION_ACQUIRED_CRX_BYTES
        {
            return Err(ExtensionAcquiredProvisioningRequestError::InvalidCrxBytes);
        }
        if legal_notice_bytes.is_empty()
            || legal_notice_bytes.len() > MAX_EXTENSION_ACQUIRED_LEGAL_NOTICE_BYTES
            || legal_notice_bytes.capacity() > MAX_EXTENSION_ACQUIRED_LEGAL_NOTICE_BYTES
        {
            return Err(ExtensionAcquiredProvisioningRequestError::InvalidLegalNoticeBytes);
        }
        let retained_bytes = size_of::<Self>()
            .checked_add(catalog_bytes.capacity())
            .and_then(|value| value.checked_add(crx3_bytes.capacity()))
            .and_then(|value| value.checked_add(legal_notice_bytes.capacity()))
            .ok_or(ExtensionAcquiredProvisioningRequestError::AccountingOverflow)?;
        if retained_bytes > MAX_EXTENSION_ACQUIRED_PROVISIONING_RETAINED_BYTES {
            return Err(ExtensionAcquiredProvisioningRequestError::RetainedBytesExceeded);
        }
        Ok(Self {
            catalog_bytes,
            package_key,
            runtime_profile,
            crx3_bytes,
            legal_notice_bytes,
            retained_bytes,
        })
    }

    /// Returns the exact package selector. It carries no package authority.
    pub const fn package_key(&self) -> ExtensionPackageKey {
        self.package_key
    }

    /// Returns the requested reviewed runtime backend.
    pub const fn runtime_backend(&self) -> ExtensionRuntimeBackendTarget {
        self.runtime_profile.runtime_backend()
    }

    /// Returns the exact requested reviewed compatibility profile.
    pub const fn runtime_profile(&self) -> ExtensionAcquiredRuntimeProfile {
        self.runtime_profile
    }

    /// Returns the exact logical memory charged while this request is pending.
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    /// Decomposes the move-only request at the authenticating service boundary.
    pub fn into_parts(
        self,
    ) -> (
        Vec<u8>,
        ExtensionPackageKey,
        ExtensionAcquiredRuntimeProfile,
        Vec<u8>,
        Vec<u8>,
    ) {
        (
            self.catalog_bytes,
            self.package_key,
            self.runtime_profile,
            self.crx3_bytes,
            self.legal_notice_bytes,
        )
    }
}

impl fmt::Debug for ExtensionAcquiredPackageProvisioningRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionAcquiredPackageProvisioningRequest")
            .field("package_key", &self.package_key)
            .field("runtime_profile", &self.runtime_profile)
            .field("catalog_bytes", &self.catalog_bytes.len())
            .field("crx3_bytes", &self.crx3_bytes.len())
            .field("legal_notice_bytes", &self.legal_notice_bytes.len())
            .field("retained_bytes", &self.retained_bytes)
            .finish()
    }
}

/// One package/profile row in an exact complete acquired-catalog selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtensionAcquiredRuntimeSelection {
    package_key: ExtensionPackageKey,
    runtime_profile: ExtensionAcquiredRuntimeProfile,
}

impl ExtensionAcquiredRuntimeSelection {
    /// Creates one non-authorizing selection row.
    pub const fn new(
        package_key: ExtensionPackageKey,
        runtime_backend: ExtensionRuntimeBackendTarget,
    ) -> Self {
        Self::new_for_profile(
            package_key,
            ExtensionAcquiredRuntimeProfile::from_runtime_backend(runtime_backend),
        )
    }

    /// Creates one non-authorizing exact profile selection row.
    pub const fn new_for_profile(
        package_key: ExtensionPackageKey,
        runtime_profile: ExtensionAcquiredRuntimeProfile,
    ) -> Self {
        Self {
            package_key,
            runtime_profile,
        }
    }

    /// Returns the selected package key.
    pub const fn package_key(self) -> ExtensionPackageKey {
        self.package_key
    }

    /// Returns the selected runtime backend.
    pub const fn runtime_backend(self) -> ExtensionRuntimeBackendTarget {
        self.runtime_profile.runtime_backend()
    }

    /// Returns the exact selected compatibility profile.
    pub const fn runtime_profile(self) -> ExtensionAcquiredRuntimeProfile {
        self.runtime_profile
    }
}

/// Move-only request to activate one complete, already materialized catalog.
#[must_use = "catalog activation authority must be settled or discarded"]
pub struct ExtensionAcquiredCatalogActivationRequest {
    catalog_bytes: Vec<u8>,
    selections: Vec<ExtensionAcquiredRuntimeSelection>,
    retained_bytes: usize,
}

impl ExtensionAcquiredCatalogActivationRequest {
    /// Creates one bounded exact-projection request.
    pub fn new(
        catalog_bytes: Vec<u8>,
        selections: Vec<ExtensionAcquiredRuntimeSelection>,
    ) -> Result<Self, ExtensionAcquiredProvisioningRequestError> {
        if catalog_bytes.is_empty()
            || catalog_bytes.len() > MAX_EXTENSION_ACQUIRED_CATALOG_BYTES
            || catalog_bytes.capacity() > MAX_EXTENSION_ACQUIRED_CATALOG_BYTES
        {
            return Err(ExtensionAcquiredProvisioningRequestError::InvalidCatalogBytes);
        }
        if selections.is_empty()
            || selections.len() > MAX_ACQUIRED_CATALOG_SELECTIONS
            || selections.capacity() > MAX_ACQUIRED_CATALOG_SELECTIONS
            || selections
                .windows(2)
                .any(|pair| pair[0].package_key().bytes() >= pair[1].package_key().bytes())
        {
            return Err(ExtensionAcquiredProvisioningRequestError::InvalidSelection);
        }
        let retained_bytes = size_of::<Self>()
            .checked_add(catalog_bytes.capacity())
            .and_then(|value| {
                value.checked_add(
                    selections
                        .capacity()
                        .checked_mul(size_of::<ExtensionAcquiredRuntimeSelection>())?,
                )
            })
            .ok_or(ExtensionAcquiredProvisioningRequestError::AccountingOverflow)?;
        if retained_bytes > MAX_EXTENSION_ACQUIRED_PROVISIONING_RETAINED_BYTES {
            return Err(ExtensionAcquiredProvisioningRequestError::RetainedBytesExceeded);
        }
        Ok(Self {
            catalog_bytes,
            selections,
            retained_bytes,
        })
    }

    /// Returns the exact logical memory charged while pending.
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    /// Decomposes the request at the service-owned authentication boundary.
    pub fn into_parts(self) -> (Vec<u8>, Vec<ExtensionAcquiredRuntimeSelection>) {
        (self.catalog_bytes, self.selections)
    }
}

impl fmt::Debug for ExtensionAcquiredCatalogActivationRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionAcquiredCatalogActivationRequest")
            .field("catalog_bytes", &self.catalog_bytes.len())
            .field("selections", &self.selections)
            .field("retained_bytes", &self.retained_bytes)
            .finish()
    }
}

/// Stable construction rejection for acquired provisioning transport values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionAcquiredProvisioningRequestError {
    /// Catalog bytes are empty or exceed the transport ceiling.
    InvalidCatalogBytes,
    /// CRX bytes are empty or exceed the transport ceiling.
    InvalidCrxBytes,
    /// Legal notice bytes are empty or exceed the transport ceiling.
    InvalidLegalNoticeBytes,
    /// The complete selection is empty, oversized, duplicated, or unordered.
    InvalidSelection,
    /// Retained-byte accounting overflowed.
    AccountingOverflow,
    /// Exact retained bytes exceed the complete request ceiling.
    RetainedBytesExceeded,
}

/// Terminal settlement for one acquired package materialization attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionAcquiredPackageProvisioningOutcome {
    /// The exact package was newly materialized durably.
    Materialized,
    /// The exact completed package already existed and was fully reverified.
    AlreadyMaterialized,
    /// Authentication, compatibility, projection, or capacity rejected input.
    Rejected,
    /// The worker could not admit the operation without mutating authority.
    Unavailable,
    /// The operation may have crossed a durable commit boundary before the
    /// caller's observation ended; retrying the exact request is required.
    OutcomeUnknown,
    /// Internal recovery or a security invariant failed closed.
    FailedClosed,
}

/// Terminal settlement for one complete acquired-catalog activation attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionAcquiredCatalogActivationOutcome {
    /// The exact candidate became current.
    Activated(ExtensionCatalogSetDigest),
    /// The exact catalog set was already current and was fully reverified.
    AlreadyActive(ExtensionCatalogSetDigest),
    /// Authentication, compatibility, projection, or completeness rejected input.
    Rejected,
    /// The worker could not admit the operation without mutating authority.
    Unavailable,
    /// The operation may have crossed a durable commit boundary before the
    /// caller's observation ended; exact retry is required.
    OutcomeUnknown,
    /// Internal recovery or a security invariant failed closed.
    FailedClosed,
}

/// Callback owned by an admitted acquired-package provisioning operation.
pub type ExtensionAcquiredPackageProvisioningCallback =
    Box<dyn FnOnce(ExtensionAcquiredPackageProvisioningOutcome) + Send>;
/// Callback owned by an admitted complete-catalog activation operation.
pub type ExtensionAcquiredCatalogActivationCallback =
    Box<dyn FnOnce(ExtensionAcquiredCatalogActivationOutcome) + Send>;

#[cfg(test)]
mod tests {
    use super::*;

    fn package(value: u8) -> ExtensionPackageKey {
        ExtensionPackageKey::from_bytes([value; 32])
    }

    #[test]
    fn package_request_is_bounded_move_only_and_redacts_bytes() {
        let request = ExtensionAcquiredPackageProvisioningRequest::new(
            vec![1, 2],
            package(1),
            ExtensionRuntimeBackendTarget::MacosNative,
            vec![3, 4, 5],
            vec![6],
        )
        .unwrap();
        assert!(request.retained_bytes() <= MAX_EXTENSION_ACQUIRED_PROVISIONING_RETAINED_BYTES);
        let debug = format!("{request:?}");
        assert!(!debug.contains("[1, 2]"));
        assert!(!debug.contains("[3, 4, 5]"));
        let (catalog, key, profile, crx, legal) = request.into_parts();
        assert_eq!(&*catalog, &[1, 2]);
        assert_eq!(key, package(1));
        assert_eq!(profile, ExtensionAcquiredRuntimeProfile::MacosNative);
        assert_eq!(&*crx, &[3, 4, 5]);
        assert_eq!(&*legal, &[6]);
    }

    #[test]
    fn brokered_profile_is_distinct_but_shares_the_macos_native_backend() {
        assert_ne!(
            ExtensionAcquiredRuntimeProfile::MacosNative,
            ExtensionAcquiredRuntimeProfile::MacosNativeBrokered
        );
        assert_eq!(
            ExtensionAcquiredRuntimeProfile::MacosNative.runtime_backend(),
            ExtensionRuntimeBackendTarget::MacosNative
        );
        assert_eq!(
            ExtensionAcquiredRuntimeProfile::MacosNativeBrokered.runtime_backend(),
            ExtensionRuntimeBackendTarget::MacosNative
        );

        let request = ExtensionAcquiredPackageProvisioningRequest::new_for_profile(
            vec![1],
            package(2),
            ExtensionAcquiredRuntimeProfile::MacosNativeBrokered,
            vec![2],
            vec![3],
        )
        .unwrap();
        assert_eq!(
            request.runtime_profile(),
            ExtensionAcquiredRuntimeProfile::MacosNativeBrokered
        );
        assert_eq!(
            request.runtime_backend(),
            ExtensionRuntimeBackendTarget::MacosNative
        );
    }

    #[test]
    fn package_request_accounts_allocated_capacity_without_copying() {
        let mut catalog = Vec::with_capacity(64);
        catalog.push(1);
        let mut crx = Vec::with_capacity(128);
        crx.push(2);
        let mut legal = Vec::with_capacity(32);
        legal.push(3);
        let expected = size_of::<ExtensionAcquiredPackageProvisioningRequest>()
            + catalog.capacity()
            + crx.capacity()
            + legal.capacity();

        let request = ExtensionAcquiredPackageProvisioningRequest::new(
            catalog,
            package(1),
            ExtensionRuntimeBackendTarget::MacosNative,
            crx,
            legal,
        )
        .unwrap();
        assert_eq!(request.retained_bytes(), expected);
    }

    #[test]
    fn tiny_payload_with_oversized_allocation_is_rejected() {
        let mut catalog = Vec::with_capacity(MAX_EXTENSION_ACQUIRED_CATALOG_BYTES + 1);
        catalog.push(1);
        assert!(matches!(
            ExtensionAcquiredPackageProvisioningRequest::new(
                catalog,
                package(1),
                ExtensionRuntimeBackendTarget::MacosNative,
                vec![2],
                vec![3],
            ),
            Err(ExtensionAcquiredProvisioningRequestError::InvalidCatalogBytes)
        ));
    }

    #[test]
    fn complete_selection_requires_canonical_unique_order() {
        let selection = |value| {
            ExtensionAcquiredRuntimeSelection::new(
                package(value),
                ExtensionRuntimeBackendTarget::MacosNative,
            )
        };
        assert!(ExtensionAcquiredCatalogActivationRequest::new(
            vec![1],
            vec![selection(1), selection(2)],
        )
        .is_ok());
        for invalid in [
            Vec::new(),
            vec![selection(1), selection(1)],
            vec![selection(2), selection(1)],
        ] {
            assert!(matches!(
                ExtensionAcquiredCatalogActivationRequest::new(vec![1], invalid),
                Err(ExtensionAcquiredProvisioningRequestError::InvalidSelection)
            ));
        }
    }
}
