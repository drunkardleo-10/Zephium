//! Fixed-origin HTTPS transport for authenticated Zephium repositories.
//!
//! This crate owns only the untrusted network boundary shared by independent
//! update domains. It grants no package, catalog, activation, or filesystem
//! authority. Callers must authenticate every returned byte through their own
//! signed repository and product policy.

#![deny(missing_docs)]
#![deny(unsafe_code)]

use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::TryStreamExt;
use reqwest::header::{
    HeaderMap, HeaderValue, ACCEPT, ACCEPT_ENCODING, CACHE_CONTROL, CONTENT_ENCODING,
};
use reqwest::redirect::Policy;
use reqwest::{Client, StatusCode};
use thiserror::Error;
use tough::{Transport, TransportError, TransportErrorKind, TransportStream};
use url::Url;

/// Configuration failure before any request can be issued.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum FixedOriginTransportConfigError {
    /// A base URL is not an unambiguous absolute HTTPS directory.
    #[error("repository base URL is invalid")]
    InvalidBaseUrl,
    /// Request and connection deadlines are empty or contradictory.
    #[error("repository transport deadlines are invalid")]
    InvalidDeadlines,
    /// The static product user agent is not a valid HTTP header value.
    #[error("repository transport user agent is invalid")]
    InvalidUserAgent,
    /// The HTTPS client could not be constructed.
    #[error("repository HTTPS client is unavailable")]
    ClientUnavailable,
}

/// Redirect-free HTTPS transport confined to two exact repository directories.
///
/// Metadata and target origins may differ, but each request must remain below
/// one of the two configured base paths. URLs carrying credentials, queries,
/// fragments, backslashes, or percent-encoded path components are refused so
/// intermediary-specific decoding cannot widen the boundary.
#[derive(Clone)]
pub struct FixedOriginTransport {
    client: Client,
    metadata_base: Url,
    targets_base: Url,
}

#[derive(Debug, Error)]
enum RedactedNetworkError {
    #[error("repository request timed out")]
    Timeout,
    #[error("repository connection failed")]
    Connect,
    #[error("repository request failed")]
    Request,
    #[error("repository response body failed")]
    Body,
    #[error("repository transport failed")]
    Other,
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
    /// Constructs one transport without performing network I/O.
    ///
    /// The caller retains responsibility for a stricter domain-specific upper
    /// bound on both deadlines and for authenticating repository metadata.
    pub fn new(
        metadata_base: Url,
        targets_base: Url,
        request_timeout: Duration,
        connect_timeout: Duration,
        user_agent: &'static str,
    ) -> Result<Self, FixedOriginTransportConfigError> {
        if !valid_base(&metadata_base) || !valid_base(&targets_base) {
            return Err(FixedOriginTransportConfigError::InvalidBaseUrl);
        }
        if connect_timeout.is_zero()
            || request_timeout.is_zero()
            || connect_timeout > request_timeout
        {
            return Err(FixedOriginTransportConfigError::InvalidDeadlines);
        }
        let user_agent = HeaderValue::from_str(user_agent)
            .map_err(|_| FixedOriginTransportConfigError::InvalidUserAgent)?;
        if user_agent.is_empty() {
            return Err(FixedOriginTransportConfigError::InvalidUserAgent);
        }
        let client = Client::builder()
            .https_only(true)
            .redirect(Policy::none())
            .timeout(request_timeout)
            .connect_timeout(connect_timeout)
            .user_agent(user_agent)
            .build()
            .map_err(|_| FixedOriginTransportConfigError::ClientUnavailable)?;
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
                HeaderValue::from_static(
                    "application/octet-stream, application/json;q=0.9, text/plain;q=0.8",
                ),
            )
            .header(ACCEPT_ENCODING, HeaderValue::from_static("identity"))
            .header(CACHE_CONTROL, HeaderValue::from_static("no-cache"))
            .send()
            .await
            .map_err(|error| {
                TransportError::new_with_cause(
                    TransportErrorKind::Other,
                    &diagnostic_url,
                    redact_network_error(&error),
                )
            })?;

        // Redirects are disabled. Revalidate both the final boundary and exact
        // URL so a future client configuration change cannot silently weaken
        // this policy.
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
        if !response_encoding_admitted(response.headers()) {
            return Err(TransportError::new(
                TransportErrorKind::Other,
                diagnostic_url,
            ));
        }

        let stream_url = diagnostic_url.clone();
        let stream = response.bytes_stream().map_err(move |error| {
            TransportError::new_with_cause(
                TransportErrorKind::Other,
                &stream_url,
                redact_network_error(&error),
            )
        });
        Ok(Box::pin(stream))
    }
}

fn response_encoding_admitted(headers: &HeaderMap) -> bool {
    let mut encodings = headers.get_all(CONTENT_ENCODING).iter();
    match (encodings.next(), encodings.next()) {
        (None, None) => true,
        (Some(encoding), None) => encoding.as_bytes().eq_ignore_ascii_case(b"identity"),
        _ => false,
    }
}

fn redact_network_error(error: &reqwest::Error) -> RedactedNetworkError {
    if error.is_timeout() {
        RedactedNetworkError::Timeout
    } else if error.is_connect() {
        RedactedNetworkError::Connect
    } else if error.is_request() {
        RedactedNetworkError::Request
    } else if error.is_body() {
        RedactedNetworkError::Body
    } else {
        RedactedNetworkError::Other
    }
}

fn valid_base(url: &Url) -> bool {
    url.scheme() == "https"
        && url.has_host()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.path().starts_with('/')
        && url.path().ends_with('/')
        && valid_path(url.path())
}

fn is_below_fixed_base(candidate: &Url, base: &Url) -> bool {
    candidate.scheme() == "https"
        && candidate.scheme() == base.scheme()
        && candidate.host_str() == base.host_str()
        && candidate.port_or_known_default() == base.port_or_known_default()
        && candidate.username().is_empty()
        && candidate.password().is_none()
        && candidate.query().is_none()
        && candidate.fragment().is_none()
        && valid_path(candidate.path())
        && candidate.path().starts_with(base.path())
}

fn valid_path(path: &str) -> bool {
    path.is_ascii()
        && !path.contains('\\')
        && !path.contains('%')
        && !path.bytes().any(|byte| byte.is_ascii_control())
}

fn redacted_origin(url: &Url) -> String {
    let host = url.host_str().unwrap_or("<invalid>");
    match url.port() {
        Some(port) => format!("{}://{host}:{port}/…", url.scheme()),
        None => format!("{}://{host}/…", url.scheme()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transport() -> FixedOriginTransport {
        FixedOriginTransport::new(
            Url::parse("https://updates.example/metadata/").unwrap(),
            Url::parse("https://cdn.example/targets/").unwrap(),
            Duration::from_secs(30),
            Duration::from_secs(10),
            "Zephium-Test-Updater/1",
        )
        .unwrap()
    }

    #[test]
    fn exact_origins_ports_and_base_directories_are_required() {
        let transport = transport();

        assert!(transport
            .admits(&Url::parse("https://updates.example/metadata/timestamp.json").unwrap()));
        assert!(transport.admits(&Url::parse("https://cdn.example/targets/a.crx").unwrap()));
        for rejected in [
            "https://updates.example/metadata-evil/timestamp.json",
            "https://updates.example/other/timestamp.json",
            "https://updates.example.evil/metadata/root.json",
            "http://updates.example/metadata/root.json",
            "https://updates.example:444/metadata/root.json",
            "https://updates.example/metadata/root.json?token=x",
            "https://updates.example/metadata/root.json#fragment",
            "https://updates.example/metadata/%2f..%2fprivate",
            "https://updates.example/metadata/%5cprivate",
            "https://updates.example/metadata/%2e%2e/private",
            "https://updates.example/metadata/%252fprivate",
        ] {
            assert!(
                !transport.admits(&Url::parse(rejected).unwrap()),
                "{rejected}"
            );
        }
    }

    #[test]
    fn ambiguous_bases_and_deadlines_are_rejected() {
        let good = Url::parse("https://updates.example/metadata/").unwrap();
        let target = Url::parse("https://cdn.example/targets/").unwrap();
        for invalid in [
            "http://updates.example/metadata/",
            "https://user@updates.example/metadata/",
            "https://updates.example/metadata",
            "https://updates.example/metadata/?channel=stable",
            "https://updates.example/%6detadata/",
        ] {
            assert_eq!(
                FixedOriginTransport::new(
                    Url::parse(invalid).unwrap(),
                    target.clone(),
                    Duration::from_secs(30),
                    Duration::from_secs(10),
                    "Zephium-Test-Updater/1",
                )
                .unwrap_err(),
                FixedOriginTransportConfigError::InvalidBaseUrl,
                "{invalid}"
            );
        }
        assert_eq!(
            FixedOriginTransport::new(
                good,
                target,
                Duration::from_secs(5),
                Duration::from_secs(6),
                "Zephium-Test-Updater/1",
            )
            .unwrap_err(),
            FixedOriginTransportConfigError::InvalidDeadlines
        );
    }

    #[test]
    fn debug_output_never_contains_repository_paths() {
        let rendered = format!("{:?}", transport());
        assert!(rendered.contains("https://updates.example/…"));
        assert!(rendered.contains("https://cdn.example/…"));
        assert!(!rendered.contains("/metadata/"));
        assert!(!rendered.contains("/targets/"));
    }

    #[test]
    fn successful_response_encoding_must_be_absent_or_exact_identity() {
        let mut headers = HeaderMap::new();
        assert!(response_encoding_admitted(&headers));
        headers.insert(CONTENT_ENCODING, HeaderValue::from_static("identity"));
        assert!(response_encoding_admitted(&headers));
        headers.insert(CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        assert!(!response_encoding_admitted(&headers));
        headers.append(CONTENT_ENCODING, HeaderValue::from_static("identity"));
        assert!(!response_encoding_admitted(&headers));
    }
}
