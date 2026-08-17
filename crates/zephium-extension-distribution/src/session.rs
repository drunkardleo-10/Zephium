use std::fmt;
use std::mem::size_of;

use zephium_core::ports::extensions::{
    ExtensionAcquiredCatalogActivationRequest, ExtensionAcquiredProvisioningRequestError,
    ExtensionAcquiredRuntimeSelection, MAX_EXTENSION_ACQUIRED_CATALOG_BYTES,
};
use zephium_extension_package::{
    ExtensionReleaseCatalog, ExtensionReleaseCatalogDigest, ExtensionReleaseCatalogRevision,
    MAX_EXTENSION_PACKAGE_LINES, MAX_EXTENSION_RELEASE_CATALOG_RETAINED_BYTES,
};

/// Maximum logical bytes retained by one authenticated distribution session.
///
/// Package and legal bytes are charged independently by the move-only service
/// request. A session retains only one exact catalog, its parsed projection,
/// and the complete product-selected runtime projection.
pub const MAX_EXTENSION_DISTRIBUTION_SESSION_RETAINED_BYTES: usize =
    MAX_EXTENSION_ACQUIRED_CATALOG_BYTES
        + MAX_EXTENSION_RELEASE_CATALOG_RETAINED_BYTES
        + MAX_EXTENSION_PACKAGE_LINES * size_of::<ExtensionAcquiredRuntimeSelection>()
        + 1024;

/// One authenticated catalog and exact complete runtime projection.
///
/// Construction is crate-private and follows product catalog admission. The
/// value contains no repository, install, profile, or native authority.
#[must_use = "a distribution session must be provisioned, activated, or deliberately discarded"]
pub struct ExtensionDistributionSession {
    pub(crate) catalog_bytes: Box<[u8]>,
    pub(crate) catalog: ExtensionReleaseCatalog,
    pub(crate) selections: Box<[ExtensionAcquiredRuntimeSelection]>,
    retained_bytes: usize,
}

impl ExtensionDistributionSession {
    pub(crate) fn new(
        catalog_bytes: Box<[u8]>,
        catalog: ExtensionReleaseCatalog,
        selections: Vec<ExtensionAcquiredRuntimeSelection>,
    ) -> Result<Self, ExtensionAcquiredProvisioningRequestError> {
        if selections.len() != catalog.packages().len()
            || selections.is_empty()
            || selections.len() > MAX_EXTENSION_PACKAGE_LINES
            || selections
                .windows(2)
                .any(|pair| pair[0].package_key().bytes() >= pair[1].package_key().bytes())
            || selections
                .iter()
                .any(|selection| catalog.package(selection.package_key()).is_none())
        {
            return Err(ExtensionAcquiredProvisioningRequestError::InvalidSelection);
        }
        let selections = selections.into_boxed_slice();
        let retained_bytes = size_of::<Self>()
            .checked_add(catalog_bytes.len())
            .and_then(|value| value.checked_add(catalog.retained_bytes()))
            .and_then(|value| {
                value.checked_add(
                    selections
                        .len()
                        .checked_mul(size_of::<ExtensionAcquiredRuntimeSelection>())?,
                )
            })
            .ok_or(ExtensionAcquiredProvisioningRequestError::AccountingOverflow)?;
        if retained_bytes > MAX_EXTENSION_DISTRIBUTION_SESSION_RETAINED_BYTES {
            return Err(ExtensionAcquiredProvisioningRequestError::RetainedBytesExceeded);
        }
        Ok(Self {
            catalog_bytes,
            catalog,
            selections,
            retained_bytes,
        })
    }

    /// Returns the number of exact package/runtime rows in this catalog.
    pub const fn package_count(&self) -> usize {
        self.selections.len()
    }

    /// Returns the authenticated release-catalog revision.
    pub const fn catalog_revision(&self) -> ExtensionReleaseCatalogRevision {
        self.catalog.revision()
    }

    /// Returns SHA-256 of the exact authenticated catalog bytes.
    pub const fn catalog_digest(&self) -> ExtensionReleaseCatalogDigest {
        self.catalog.digest()
    }

    /// Returns the bounded logical heap charge retained by this session.
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    /// Returns one exact selection by stable catalog order.
    pub fn selection(&self, index: usize) -> Option<ExtensionAcquiredRuntimeSelection> {
        self.selections.get(index).copied()
    }

    /// Converts the completed session into a path-free activation request.
    ///
    /// The service reauthenticates the exact catalog and requires every named
    /// package to have completed durable materialization before promotion.
    pub fn into_activation_request(
        self,
    ) -> Result<ExtensionAcquiredCatalogActivationRequest, ExtensionAcquiredProvisioningRequestError>
    {
        ExtensionAcquiredCatalogActivationRequest::new(
            self.catalog_bytes.into_vec(),
            self.selections.into_vec(),
        )
    }
}

impl fmt::Debug for ExtensionDistributionSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionDistributionSession")
            .field("catalog_revision", &self.catalog.revision())
            .field("catalog_digest", &self.catalog.digest())
            .field("package_count", &self.selections.len())
            .field("retained_bytes", &self.retained_bytes)
            .finish()
    }
}
