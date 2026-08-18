use std::fmt;
use std::time::Duration;

use sha2::{Digest as _, Sha256};
use thiserror::Error;
use url::Url;
use zephium_core::ports::extensions::{
    ExtensionAcquiredPackageProvisioningRequest, ExtensionAcquiredRuntimeSelection,
    MAX_EXTENSION_ACQUIRED_CATALOG_BYTES, MAX_EXTENSION_ACQUIRED_CRX_BYTES,
    MAX_EXTENSION_ACQUIRED_LEGAL_NOTICE_BYTES,
};
use zephium_extension_authority::BundledCatalogAdmissionError;
use zephium_extension_package::{
    Crx3PackageError, ExtensionReleasePackage, VerifiedCrx3Package, MAX_CRX3_HEADER_BYTES,
};
use zephium_update_transport::{
    FixedOriginFetchError, FixedOriginTransport, FixedOriginTransportConfigError,
};

use crate::authentication::{CatalogAuthenticator, ProductCatalogAuthenticator};
use crate::layout::{catalog_url, crx3_url, legal_notice_url};
use crate::session::ExtensionDistributionSession;

const CRX3_PREFIX_BYTES: usize = 12;
const PRODUCT_USER_AGENT: &str = "Zephium-Extension-Distributor/1";

const _: () = assert!(
    MAX_EXTENSION_ACQUIRED_CRX_BYTES
        >= zephium_core::extensions::MAX_EXTENSION_ARCHIVE_BYTES as usize
            + MAX_CRX3_HEADER_BYTES
            + CRX3_PREFIX_BYTES
);
const _: () = assert!(
    MAX_EXTENSION_ACQUIRED_LEGAL_NOTICE_BYTES
        >= zephium_extension_package::MAX_EXTENSION_LEGAL_NOTICE_BYTES as usize
);

/// Failure before a product distribution client can issue network I/O.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum ExtensionDistributionClientError {
    /// This build contains no reviewed active extension catalog anchor.
    #[error("extension distribution is not product-provisioned")]
    ProductAuthorityUnavailable,
    /// Compiled product authority material is internally inconsistent.
    #[error("extension distribution product authority is invalid")]
    ProductAuthorityInvalid,
    /// Fixed HTTPS origins or transport deadlines are invalid.
    #[error("extension distribution transport configuration is invalid")]
    TransportConfiguration,
}

/// Stable failure while acquiring one authenticated catalog or package.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum ExtensionDistributionError {
    /// The fixed-origin network boundary refused or failed the request.
    #[error("extension distribution transport failed: {0}")]
    Transport(#[from] FixedOriginFetchError),
    /// Exact catalog bytes did not match the product-sealed active authority.
    #[error("extension distribution catalog was rejected")]
    CatalogRejected,
    /// Runtime selections were incomplete, duplicated, unordered, or unknown.
    #[error("extension distribution selection is invalid")]
    InvalidSelection,
    /// The requested stable catalog index does not exist.
    #[error("extension distribution package index is invalid")]
    InvalidPackageIndex,
    /// A catalog row was not an acquired CRX3 package with Chromium identity.
    #[error("extension distribution package shape is unsupported")]
    UnsupportedPackage,
    /// Derived immutable-object URL construction failed closed.
    #[error("extension distribution object identity is invalid")]
    InvalidObjectIdentity,
    /// The CRX3 signature, developer identity, or catalog-bound ZIP mismatched.
    #[error("extension distribution CRX3 object was rejected")]
    CrxRejected,
    /// Legal bytes differed from the exact catalog binding.
    #[error("extension distribution legal object was rejected")]
    LegalNoticeRejected,
    /// A path-free service request could not satisfy its memory contract.
    #[error("extension distribution request exceeded its bound")]
    RequestRejected,
}

pub(crate) trait ArtifactTransport {
    async fn fetch_bounded(
        &self,
        url: Url,
        max_bytes: usize,
    ) -> Result<Box<[u8]>, FixedOriginFetchError>;
}

impl ArtifactTransport for FixedOriginTransport {
    async fn fetch_bounded(
        &self,
        url: Url,
        max_bytes: usize,
    ) -> Result<Box<[u8]>, FixedOriginFetchError> {
        FixedOriginTransport::fetch_bounded(self, url, max_bytes).await
    }
}

pub(crate) enum ProductArtifactTransport {
    Fixed(FixedOriginTransport),
    #[cfg(feature = "staging-extension-catalog")]
    EmbeddedStaging,
}

impl ArtifactTransport for ProductArtifactTransport {
    async fn fetch_bounded(
        &self,
        url: Url,
        max_bytes: usize,
    ) -> Result<Box<[u8]>, FixedOriginFetchError> {
        match self {
            Self::Fixed(transport) => transport.fetch_bounded(url, max_bytes).await,
            #[cfg(feature = "staging-extension-catalog")]
            Self::EmbeddedStaging => EmbeddedStagingArtifactTransport.fetch_bounded(url, max_bytes),
        }
    }
}

impl fmt::Debug for ProductArtifactTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fixed(transport) => transport.fmt(formatter),
            #[cfg(feature = "staging-extension-catalog")]
            Self::EmbeddedStaging => formatter
                .debug_struct("EmbeddedStagingArtifactTransport")
                .finish_non_exhaustive(),
        }
    }
}

#[cfg(feature = "staging-extension-catalog")]
struct EmbeddedStagingArtifactTransport;

#[cfg(feature = "staging-extension-catalog")]
impl EmbeddedStagingArtifactTransport {
    fn fetch_bounded(
        &self,
        url: Url,
        max_bytes: usize,
    ) -> Result<Box<[u8]>, FixedOriginFetchError> {
        let bytes = match url.as_str() {
            crate::staging::CATALOG_URL => crate::staging::CATALOG_BYTES,
            crate::staging::CRX3_URL => crate::staging::CRX3_BYTES,
            crate::staging::LEGAL_URL => crate::staging::LEGAL_BYTES,
            _ => return Err(FixedOriginFetchError::Boundary),
        };
        if max_bytes == 0 || bytes.len() > max_bytes {
            return Err(FixedOriginFetchError::ResponseTooLarge);
        }
        let mut output = Vec::new();
        output
            .try_reserve_exact(bytes.len())
            .map_err(|_| FixedOriginFetchError::CapacityUnavailable)?;
        if output.capacity() > max_bytes {
            return Err(FixedOriginFetchError::ResponseTooLarge);
        }
        output.extend_from_slice(bytes);
        Ok(output.into_boxed_slice())
    }
}

pub(crate) struct DistributionClient<T, A> {
    pub(crate) transport: T,
    pub(crate) authenticator: A,
    pub(crate) catalog_url: Url,
    pub(crate) targets_base: Url,
}

impl<T, A> DistributionClient<T, A>
where
    T: ArtifactTransport + Sync,
    A: CatalogAuthenticator,
{
    pub(crate) async fn begin(
        &self,
        selections: Vec<ExtensionAcquiredRuntimeSelection>,
    ) -> Result<ExtensionDistributionSession, ExtensionDistributionError> {
        let catalog_bytes = self
            .transport
            .fetch_bounded(
                self.catalog_url.clone(),
                MAX_EXTENSION_ACQUIRED_CATALOG_BYTES,
            )
            .await?;
        let catalog = self
            .authenticator
            .authenticate(&catalog_bytes)
            .map_err(|()| ExtensionDistributionError::CatalogRejected)?;
        ExtensionDistributionSession::new(catalog_bytes, catalog, selections)
            .map_err(|_| ExtensionDistributionError::InvalidSelection)
    }

    pub(crate) async fn fetch_package(
        &self,
        session: &ExtensionDistributionSession,
        index: usize,
    ) -> Result<ExtensionAcquiredPackageProvisioningRequest, ExtensionDistributionError> {
        let selection = session
            .selection(index)
            .ok_or(ExtensionDistributionError::InvalidPackageIndex)?;
        let package = session
            .catalog
            .package(selection.package_key())
            .ok_or(ExtensionDistributionError::InvalidSelection)?;
        let (archive_length, archive_digest) = package
            .payload()
            .acquired_zip_evidence()
            .ok_or(ExtensionDistributionError::UnsupportedPackage)?;
        if package.chromium().is_none() {
            return Err(ExtensionDistributionError::UnsupportedPackage);
        }

        let legal = package.provenance().legal_notice();
        let legal_max = usize::try_from(legal.length())
            .ok()
            .filter(|length| *length <= MAX_EXTENSION_ACQUIRED_LEGAL_NOTICE_BYTES)
            .ok_or(ExtensionDistributionError::UnsupportedPackage)?;
        let legal_url = legal_notice_url(&self.targets_base, &legal.sha256())
            .map_err(|()| ExtensionDistributionError::InvalidObjectIdentity)?;
        let legal_bytes = self.transport.fetch_bounded(legal_url, legal_max).await?;
        legal
            .verify_bytes(&legal_bytes)
            .map_err(|_| ExtensionDistributionError::LegalNoticeRejected)?;

        let crx_max = usize::try_from(archive_length.get())
            .ok()
            .and_then(|length| length.checked_add(MAX_CRX3_HEADER_BYTES))
            .and_then(|length| length.checked_add(CRX3_PREFIX_BYTES))
            .filter(|length| *length <= MAX_EXTENSION_ACQUIRED_CRX_BYTES)
            .ok_or(ExtensionDistributionError::UnsupportedPackage)?;
        let crx_url = crx3_url(
            &self.targets_base,
            selection.package_key(),
            package.identity().revision(),
            archive_digest,
        )
        .map_err(|()| ExtensionDistributionError::InvalidObjectIdentity)?;
        let crx_bytes = self.transport.fetch_bounded(crx_url, crx_max).await?;
        authenticate_crx(package, &crx_bytes)
            .map_err(|_| ExtensionDistributionError::CrxRejected)?;

        ExtensionAcquiredPackageProvisioningRequest::new_for_profile(
            session.catalog_bytes.clone().into_vec(),
            selection.package_key(),
            selection.runtime_profile(),
            crx_bytes.into_vec(),
            legal_bytes.into_vec(),
        )
        .map_err(|_| ExtensionDistributionError::RequestRejected)
    }
}

fn authenticate_crx(
    package: &ExtensionReleasePackage,
    bytes: &[u8],
) -> Result<(), Crx3PackageError> {
    let chromium = package
        .chromium()
        .ok_or(Crx3PackageError::ExpectedIdMismatch)?;
    let verified = VerifiedCrx3Package::parse_and_verify(bytes, Some(chromium.extension_id()))?;
    if verified.developer_key_sha256() != chromium.manifest_key_sha256() {
        return Err(Crx3PackageError::ExpectedIdMismatch);
    }
    let Some((expected_length, expected_digest)) = package.payload().acquired_zip_evidence() else {
        return Err(Crx3PackageError::Archive);
    };
    if u64::try_from(verified.archive_bytes().len()).ok() != Some(expected_length.get())
        || <[u8; 32]>::from(Sha256::digest(verified.archive_bytes())) != expected_digest.bytes()
    {
        return Err(Crx3PackageError::Archive);
    }
    Ok(())
}

/// Fixed-origin client for one product-sealed acquired extension catalog.
///
/// Construction fails before creating an HTTP client when the application has
/// no compiled product authority. Therefore an unprovisioned ordinary build
/// preserves the extension subsystem's inert startup path.
pub struct ExtensionDistributionClient {
    pub(crate) inner: DistributionClient<ProductArtifactTransport, ProductCatalogAuthenticator>,
}

impl ExtensionDistributionClient {
    /// Opens a product-authenticated client without performing network I/O.
    pub fn new(
        metadata_base: Url,
        targets_base: Url,
        request_timeout: Duration,
        connect_timeout: Duration,
    ) -> Result<Self, ExtensionDistributionClientError> {
        let authenticator = ProductCatalogAuthenticator::new().map_err(classify_authority_error)?;
        let transport = FixedOriginTransport::new(
            metadata_base.clone(),
            targets_base.clone(),
            request_timeout,
            connect_timeout,
            PRODUCT_USER_AGENT,
        )
        .map_err(classify_transport_config)?;
        let catalog_url = catalog_url(&metadata_base)
            .map_err(|()| ExtensionDistributionClientError::TransportConfiguration)?;
        Ok(Self {
            inner: DistributionClient {
                transport: ProductArtifactTransport::Fixed(transport),
                authenticator,
                catalog_url,
                targets_base,
            },
        })
    }

    /// Opens the exact embedded client for the explicit non-shipping staging
    /// catalog without constructing an HTTP client or performing I/O.
    #[cfg(feature = "staging-extension-catalog")]
    pub fn staging() -> Result<Self, ExtensionDistributionClientError> {
        let authenticator = ProductCatalogAuthenticator::new().map_err(classify_authority_error)?;
        let metadata_base = Url::parse(crate::staging::METADATA_BASE)
            .map_err(|_| ExtensionDistributionClientError::TransportConfiguration)?;
        let targets_base = Url::parse(crate::staging::TARGETS_BASE)
            .map_err(|_| ExtensionDistributionClientError::TransportConfiguration)?;
        let catalog_url = catalog_url(&metadata_base)
            .map_err(|()| ExtensionDistributionClientError::TransportConfiguration)?;
        if catalog_url.as_str() != crate::staging::CATALOG_URL {
            return Err(ExtensionDistributionClientError::TransportConfiguration);
        }
        Ok(Self {
            inner: DistributionClient {
                transport: ProductArtifactTransport::EmbeddedStaging,
                authenticator,
                catalog_url,
                targets_base,
            },
        })
    }

    /// Fetches and authenticates the exact active catalog and binds the
    /// complete product-selected runtime projection.
    pub async fn begin(
        &self,
        selections: Vec<ExtensionAcquiredRuntimeSelection>,
    ) -> Result<ExtensionDistributionSession, ExtensionDistributionError> {
        self.inner.begin(selections).await
    }

    /// Fetches, verifies, and bounds one package in stable catalog order.
    ///
    /// Callers should submit the returned move-only request to the extension
    /// service and settle it before requesting the next index. The API retains
    /// no CRX or legal bytes after ownership moves into the request.
    pub async fn fetch_package(
        &self,
        session: &ExtensionDistributionSession,
        index: usize,
    ) -> Result<ExtensionAcquiredPackageProvisioningRequest, ExtensionDistributionError> {
        self.inner.fetch_package(session, index).await
    }
}

impl fmt::Debug for ExtensionDistributionClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionDistributionClient")
            .field("transport", &self.inner.transport)
            .finish_non_exhaustive()
    }
}

fn classify_authority_error(
    error: BundledCatalogAdmissionError,
) -> ExtensionDistributionClientError {
    if error == BundledCatalogAdmissionError::Unprovisioned {
        ExtensionDistributionClientError::ProductAuthorityUnavailable
    } else {
        ExtensionDistributionClientError::ProductAuthorityInvalid
    }
}

fn classify_transport_config(
    _error: FixedOriginTransportConfigError,
) -> ExtensionDistributionClientError {
    ExtensionDistributionClientError::TransportConfiguration
}

#[cfg(test)]
pub(crate) mod tests;
