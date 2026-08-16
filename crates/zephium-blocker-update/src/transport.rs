pub(crate) use zephium_update_transport::FixedOriginTransport;

#[cfg(test)]
use async_trait::async_trait;
#[cfg(test)]
use tough::{Transport, TransportError, TransportErrorKind, TransportStream};
#[cfg(test)]
use url::Url;

#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct FixedFilesystemTransport {
    metadata_base: Url,
    targets_base: Url,
}

#[cfg(test)]
impl FixedFilesystemTransport {
    pub(crate) fn new(metadata_base: Url, targets_base: Url) -> Self {
        Self {
            metadata_base,
            targets_base,
        }
    }
}

#[cfg(test)]
#[async_trait]
impl Transport for FixedFilesystemTransport {
    async fn fetch(&self, url: Url) -> Result<TransportStream, TransportError> {
        let admitted = is_below_file_base(&url, &self.metadata_base)
            || is_below_file_base(&url, &self.targets_base);
        if !admitted {
            return Err(TransportError::new(
                TransportErrorKind::UnsupportedUrlScheme,
                "file://<rejected>/…",
            ));
        }
        tough::FilesystemTransport.fetch(url).await
    }
}

#[cfg(test)]
fn is_below_file_base(candidate: &Url, base: &Url) -> bool {
    candidate.scheme() == "file"
        && base.scheme() == "file"
        && candidate.host_str() == base.host_str()
        && candidate.query().is_none()
        && candidate.fragment().is_none()
        && candidate.path().starts_with(base.path())
}
