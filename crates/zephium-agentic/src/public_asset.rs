//! One bounded, cookie-free fetch of a public image for media admission.
//! The bytes are data for the store's own sniffing and decoding; nothing
//! here trusts a server's declared type beyond a coarse `image/*` gate.
use reqwest::{redirect::Policy, Client};
use std::time::Duration;

/// Largest public image body accepted before decoding.
pub const MAX_PUBLIC_IMAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_REDIRECTS: usize = 3;

/// Why a public image was not fetched. Never carries response content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicAssetError {
    /// Not a public HTTPS URL.
    InvalidUrl,
    /// Transport failure, non-200 status, or a refused redirect.
    Unavailable,
    /// The body exceeds [`MAX_PUBLIC_IMAGE_BYTES`].
    TooLarge,
    /// The server did not answer with an image body.
    NotAnImage,
}

/// Only public HTTPS origins: no loopback, private, link-local, or `.local`.
pub fn public_https(url: &url::Url) -> bool {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.as_str().len() > 4096
    {
        return false;
    }
    match url.host() {
        Some(url::Host::Domain(domain)) => {
            let domain = domain.trim_end_matches('.').to_ascii_lowercase();
            !domain.is_empty()
                && domain != "localhost"
                && !domain.ends_with(".localhost")
                && !domain.ends_with(".local")
                && !domain.ends_with(".internal")
                && domain.contains('.')
        }
        Some(url::Host::Ipv4(ip)) => {
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]))
        }
        Some(url::Host::Ipv6(ip)) => {
            let segments = ip.segments();
            !(ip.is_loopback()
                || ip.is_unspecified()
                || (segments[0] & 0xfe00) == 0xfc00
                || (segments[0] & 0xffc0) == 0xfe80
                || ip.to_ipv4_mapped().is_some())
        }
        None => false,
    }
}

/// Fetches one public image without cookies, following at most three public
/// HTTPS redirects, and returns at most [`MAX_PUBLIC_IMAGE_BYTES`].
pub async fn fetch_public_image(url: &str) -> Result<Vec<u8>, PublicAssetError> {
    let parsed = url::Url::parse(url).map_err(|_| PublicAssetError::InvalidUrl)?;
    if !public_https(&parsed) {
        return Err(PublicAssetError::InvalidUrl);
    }
    let client = Client::builder()
        .https_only(true)
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS || !public_https(attempt.url()) {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .referer(false)
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(8))
        .pool_max_idle_per_host(0)
        .user_agent(crate::provider_transport::PRODUCT_USER_AGENT)
        .build()
        .map_err(|_| PublicAssetError::Unavailable)?;
    let response = client
        .get(parsed)
        .header(
            reqwest::header::ACCEPT,
            "image/png,image/jpeg,image/webp,image/gif",
        )
        .send()
        .await
        .map_err(|_| PublicAssetError::Unavailable)?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(PublicAssetError::Unavailable);
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !content_type.starts_with("image/") {
        return Err(PublicAssetError::NotAnImage);
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_PUBLIC_IMAGE_BYTES as u64)
    {
        return Err(PublicAssetError::TooLarge);
    }
    let mut bytes = Vec::new();
    let mut stream = response;
    while let Some(chunk) = stream
        .chunk()
        .await
        .map_err(|_| PublicAssetError::Unavailable)?
    {
        if bytes.len() + chunk.len() > MAX_PUBLIC_IMAGE_BYTES {
            return Err(PublicAssetError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        return Err(PublicAssetError::NotAnImage);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_https_origins_are_fetchable() {
        for ok in [
            "https://cdn.example/a.png",
            "https://93.184.216.34/logo.webp",
            "https://images.example.co.uk/x.jpg?w=200",
        ] {
            assert!(public_https(&url::Url::parse(ok).unwrap()), "{ok}");
        }
        for bad in [
            "http://cdn.example/a.png",
            "https://localhost/a.png",
            "https://api.localhost/a.png",
            "https://printer.local/a.png",
            "https://metadata.internal/a.png",
            "https://127.0.0.1/a.png",
            "https://10.0.0.5/a.png",
            "https://169.254.169.254/latest",
            "https://100.64.0.1/a.png",
            "https://[::1]/a.png",
            "https://[fd00::1]/a.png",
            "https://[fe80::1]/a.png",
            "https://user:pw@cdn.example/a.png",
            "https://intranet/a.png",
        ] {
            assert!(!public_https(&url::Url::parse(bad).unwrap()), "{bad}");
        }
    }
}
