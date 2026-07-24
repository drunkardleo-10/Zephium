use async_trait::async_trait;
use futures_util::TryStreamExt;
use reqwest::header::{HeaderValue, ACCEPT, ACCEPT_ENCODING, CACHE_CONTROL};
use reqwest::redirect::Policy;
use reqwest::{Client, StatusCode};
use std::fmt;
use tough::{Transport, TransportError, TransportErrorKind, TransportStream};
use url::Url;

use crate::types::UpdateLimits;

#[derive(Clone)]
pub(crate) struct FixedOriginTransport {
    client: Client,
    metadata_base: Url,
    targets_base: Url,
}

impl fmt::Debug for FixedOriginTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FixedOriginTransport")
            .field("metadata_origin", &redacted_origin(&self.metadata_base))
            .field("targets_origin", &redacted_origin(&self.targets_base))
            .finish_non_exhaustive()
    }
}

impl FixedOriginTransport {
    pub(crate) fn new(
        metadata_base: Url,
        targets_base: Url,
        limits: UpdateLimits,
    ) -> Result<Self, TransportError> {
        let display_url = redacted_origin(&metadata_base);
        let client = Client::builder()
            .https_only(true)
            .redirect(Policy::none())
            .timeout(limits.request_timeout)
            .connect_timeout(limits.connect_timeout)
            .user_agent("Zephium-Filter-Updater/1")
            .build()
            .map_err(|error| {
                TransportError::new_with_cause(TransportErrorKind::Other, display_url, error)
            })?;
        Ok(Self {
            client,
            metadata_base,
            targets_base,
        })
    }

    fn admits(&self, url: &Url) -> bool {
        is_below_fixed_base(url, &self.metadata_base)
            || is_below_fixed_base(url, &self.targets_base)
    }
}

#[async_trait]
impl Transport for FixedOriginTransport {
    async fn fetch(&self, url: Url) -> Result<TransportStream, TransportError> {
        if !self.admits(&url) {
            return Err(TransportError::new(
                TransportErrorKind::UnsupportedUrlScheme,
                redacted_origin(&url),
            ));
        }

        let diagnostic_url = redacted_origin(&url);
        let response = self
            .client
            .get(url.clone())
            .header(
                ACCEPT,
                HeaderValue::from_static("application/json, text/plain;q=0.9"),
            )
            .header(ACCEPT_ENCODING, HeaderValue::from_static("identity"))
            .header(CACHE_CONTROL, HeaderValue::from_static("no-cache"))
            .send()
            .await
            .map_err(|error| {
                TransportError::new_with_cause(TransportErrorKind::Other, &diagnostic_url, error)
            })?;

        // Redirects are disabled in the client. Treat every 3xx as a terminal
        // error and independently revalidate the final URL so a future client
        // configuration change cannot silently weaken the origin boundary.
        if !self.admits(response.url()) || response.url() != &url {
            return Err(TransportError::new(
                TransportErrorKind::Other,
                diagnostic_url,
            ));
        }
        let status = response.status();
        if !status.is_success() {
            let kind = if matches!(
                status,
                StatusCode::FORBIDDEN | StatusCode::NOT_FOUND | StatusCode::GONE
            ) {
                TransportErrorKind::FileNotFound
            } else {
                TransportErrorKind::Other
            };
            return Err(TransportError::new(kind, diagnostic_url));
        }

        let stream_url = diagnostic_url.clone();
        let stream = response.bytes_stream().map_err(move |error| {
            TransportError::new_with_cause(TransportErrorKind::Other, &stream_url, error)
        });
        Ok(Box::pin(stream))
    }
}

fn is_below_fixed_base(candidate: &Url, base: &Url) -> bool {
    let lowercase_path = candidate.path().to_ascii_lowercase();
    candidate.scheme() == "https"
        && candidate.scheme() == base.scheme()
        && candidate.host_str() == base.host_str()
        && candidate.port_or_known_default() == base.port_or_known_default()
        && candidate.username().is_empty()
        && candidate.password().is_none()
        && candidate.query().is_none()
        && candidate.fragment().is_none()
        && !candidate.path().contains('\\')
        && !lowercase_path.contains("%2f")
        && !lowercase_path.contains("%5c")
        && !lowercase_path.contains("%2e")
        && !lowercase_path.contains("%25")
        && candidate.path().starts_with(base.path())
}

fn redacted_origin(url: &Url) -> String {
    let host = url.host_str().unwrap_or("<invalid>");
    match url.port() {
        Some(port) => format!("{}://{host}:{port}/…", url.scheme()),
        None => format!("{}://{host}/…", url.scheme()),
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_origin_and_base_path_are_required() {
        let metadata = Url::parse("https://updates.example/metadata/").unwrap();
        let targets = Url::parse("https://cdn.example/targets/").unwrap();
        let transport =
            FixedOriginTransport::new(metadata, targets, UpdateLimits::default()).unwrap();

        assert!(transport
            .admits(&Url::parse("https://updates.example/metadata/timestamp.json").unwrap()));
        assert!(transport.admits(&Url::parse("https://cdn.example/targets/a.txt").unwrap()));
        assert!(
            !transport.admits(&Url::parse("https://updates.example/other/timestamp.json").unwrap())
        );
        assert!(!transport
            .admits(&Url::parse("https://updates.example.evil/metadata/root.json").unwrap()));
        assert!(
            !transport.admits(&Url::parse("http://updates.example/metadata/root.json").unwrap())
        );
        assert!(!transport
            .admits(&Url::parse("https://updates.example/metadata/root.json?token=x").unwrap()));
        assert!(!transport
            .admits(&Url::parse("https://updates.example/metadata/%2f..%2fprivate").unwrap()));
        assert!(
            !transport.admits(&Url::parse("https://updates.example/metadata/%5cprivate").unwrap())
        );
        assert!(!transport
            .admits(&Url::parse("https://updates.example/metadata/%2e%2e/private").unwrap()));
        assert!(!transport
            .admits(&Url::parse("https://updates.example/metadata/%252fprivate").unwrap()));
    }
}
