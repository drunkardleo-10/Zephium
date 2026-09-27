//! User-selected Chrome Web Store acquisition. All traffic goes directly to
//! Google; returned packages remain untrusted installation inputs to the service.
use zephium_extension_package::{
    ChromiumExtensionId, VerifiedCrx3Package, MAX_CRX3_HEADER_BYTES, MAX_EXTENSION_ARCHIVE_BYTES,
};
use zephium_update_transport::chrome_store::ChromeStoreTransport;

// A canceled caller cannot start a second archive verification while its
// previous blocking task is still finishing. No queued downloads or worker.
static ORIGINAL_PACKAGE_SLOT: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

/// Canonical store listing selector. It contains no arbitrary download URL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChromeStoreListing {
    id: ChromiumExtensionId,
}
impl ChromeStoreListing {
    /// Recognizes a real store listing URL. Query parameters are not forwarded.
    pub fn parse(value: &str) -> Option<Self> {
        if value.len() > 4096 {
            return None;
        }
        let url = url::Url::parse(value).ok()?;
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
        {
            return None;
        }
        let parts = url.path().trim_matches('/').split('/').collect::<Vec<_>>();
        let parts = match url.host_str()? {
            "chromewebstore.google.com" => parts.as_slice(),
            "chrome.google.com" if parts.first() == Some(&"webstore") => &parts[1..],
            _ => return None,
        };
        let id = match parts {
            ["detail", id] => *id,
            ["detail", slug, id] if !slug.is_empty() => *id,
            _ => return None,
        };
        Some(Self {
            id: ChromiumExtensionId::parse(id).ok()?,
        })
    }
    /// Exact selected publisher-derived store identifier.
    pub fn extension_id(&self) -> &str {
        self.id.as_str()
    }
}

/// An authenticated original response from Google's fixed update/CDN route.
/// It is not consent, compatibility approval, or native execution authority.
pub struct DownloadedChromeExtension {
    id: ChromiumExtensionId,
    bytes: Box<[u8]>,
}
impl std::fmt::Debug for DownloadedChromeExtension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadedChromeExtension")
            .field("bytes", &self.bytes.len())
            .finish_non_exhaustive()
    }
}
impl DownloadedChromeExtension {
    /// Transfers the selected ID and original bytes into the service mailbox.
    pub fn into_parts(self) -> (String, Box<[u8]>) {
        (self.id.as_str().to_owned(), self.bytes)
    }
}
/// Closed acquisition failure; response text and source URLs are never logged.
#[derive(Debug, thiserror::Error)]
pub enum ChromeStoreDownloadError {
    /// A previous acquisition or its finishing verifier still owns the slot.
    #[error("another extension download is still finishing; please retry")]
    Busy,
    /// The network request failed or crossed the fixed source boundary.
    #[error("extension download failed")]
    Transport,
    /// Google did not offer a package for the selected item.
    #[error("this extension is not currently available from the store")]
    NotOffered,
    /// The original signature or requested extension ID did not match.
    #[error("extension package authentication failed")]
    Package,
}
/// On-demand, cookieless client. Construction starts no worker or timer.
pub struct ChromeStoreClient {
    transport: ChromeStoreTransport,
}
impl ChromeStoreClient {
    /// Uses the compiled package-selection version for the download protocol.
    /// Runtime compatibility is assessed independently after acquisition.
    pub fn new() -> Result<Self, ChromeStoreDownloadError> {
        ChromeStoreTransport::new(zephium_core::webview2::LATEST_REVIEWED_TEXT)
            .map(|transport| Self { transport })
            .map_err(|_| ChromeStoreDownloadError::Transport)
    }
    /// Fetches and authenticates an original package for one explicit selection.
    /// Dropping the future cancels network I/O. A signature verification that
    /// has already started finishes on the blocking pool, then releases bytes.
    pub async fn download(
        &self,
        listing: ChromeStoreListing,
    ) -> Result<DownloadedChromeExtension, ChromeStoreDownloadError> {
        self.fetch_original(listing, None)
            .await?
            .ok_or(ChromeStoreDownloadError::NotOffered)
    }

    /// Checks for a newer original package. None means the store offered no
    /// update, not signed freshness evidence or permission to change a checkpoint.
    pub async fn download_update(
        &self,
        listing: ChromeStoreListing,
        installed_version: &str,
    ) -> Result<Option<DownloadedChromeExtension>, ChromeStoreDownloadError> {
        self.fetch_original(listing, Some(installed_version)).await
    }

    async fn fetch_original(
        &self,
        listing: ChromeStoreListing,
        installed_version: Option<&str>,
    ) -> Result<Option<DownloadedChromeExtension>, ChromeStoreDownloadError> {
        let slot = ORIGINAL_PACKAGE_SLOT
            .try_acquire()
            .map_err(|_| ChromeStoreDownloadError::Busy)?;
        let limit = MAX_EXTENSION_ARCHIVE_BYTES as usize + MAX_CRX3_HEADER_BYTES + 12;
        let bytes = self
            .transport
            .fetch(listing.id.as_str(), installed_version, limit)
            .await
            .map_err(|_| ChromeStoreDownloadError::Transport)?;
        let Some(bytes) = bytes else {
            return Ok(None);
        };
        // CRX verification hashes the whole archive. Keep that CPU work off
        // the async executor used for downloads and other browser services.
        // Move the sole response buffer; never clone the package for this hop.
        tokio::task::spawn_blocking(move || {
            let _slot = slot;
            authenticate_original(listing, bytes)
        })
        .await
        .map_err(|_| ChromeStoreDownloadError::Package)?
        .map(Some)
    }
}

fn authenticate_original(
    listing: ChromeStoreListing,
    bytes: Box<[u8]>,
) -> Result<DownloadedChromeExtension, ChromeStoreDownloadError> {
    VerifiedCrx3Package::parse_and_verify(&bytes, Some(&listing.id))
        .map_err(|_| ChromeStoreDownloadError::Package)?;
    Ok(DownloadedChromeExtension {
        id: listing.id,
        bytes,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn listing_is_an_id_selector_not_a_page_supplied_fetch_url() {
        let id = "eimadpbcbfnmbkopoojfekhnkhdbieeh";
        for value in [
            format!("https://chromewebstore.google.com/detail/dark-reader/{id}?hl=en"),
            format!("https://chrome.google.com/webstore/detail/dark-reader/{id}"),
        ] {
            assert_eq!(
                ChromeStoreListing::parse(&value).unwrap().extension_id(),
                id
            );
        }
        for value in [
            format!("https://attacker.invalid/detail/name/{id}"),
            format!("https://chromewebstore.google.com.attacker.invalid/detail/name/{id}"),
            format!("http://chromewebstore.google.com/detail/name/{id}"),
            format!("https://user@chromewebstore.google.com/detail/name/{id}"),
            format!("https://chromewebstore.google.com/detail/name/{id}/redirect"),
            "https://chromewebstore.google.com/category/extensions".into(),
        ] {
            assert!(ChromeStoreListing::parse(&value).is_none());
        }
    }
}
