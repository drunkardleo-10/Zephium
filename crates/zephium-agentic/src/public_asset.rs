//! One bounded, cookie-free fetch of a public image for media admission.
//! The bytes are data for the store's own sniffing and decoding; nothing
//! here trusts a server's declared type beyond a coarse `image/*` gate.
use reqwest::{redirect::Policy, Client};
use std::time::Duration;

/// Largest public image body accepted before decoding; admission scales a
/// large photo down to display size before it is stored.
pub const MAX_PUBLIC_IMAGE_BYTES: usize = 24 * 1024 * 1024;
/// The long edge a display-sized variant is asked for.
pub const DISPLAY_EDGE: u32 = 1600;
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

/// The same picture at display size, when its CDN takes size parameters in
/// the address: LEGO, Shopify, Cloudinary, imgix, Contentful, Sanity and
/// Airbnb. None when the host is not one of them or the address already asks
/// for a display size.
pub fn display_variant(url: &str) -> Option<String> {
    let mut parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    let path = parsed.path().to_owned();
    let edge = DISPLAY_EDGE.to_string();
    let sized = |parsed: &url::Url, keys: &[&str]| {
        parsed.query_pairs().any(|(key, value)| {
            keys.contains(&key.as_ref())
                && value
                    .parse::<u32>()
                    .is_ok_and(|size| size > 0 && size <= DISPLAY_EDGE)
        })
    };
    let set = |parsed: &mut url::Url, pairs: &[(&str, &str)]| {
        let kept: Vec<(String, String)> = parsed
            .query_pairs()
            .filter(|(key, _)| !pairs.iter().any(|(own, _)| own == key))
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        let mut query = parsed.query_pairs_mut();
        query.clear();
        for (key, value) in &kept {
            query.append_pair(key, value);
        }
        for (key, value) in pairs {
            query.append_pair(key, value);
        }
    };
    if (host == "www.lego.com" || host == "lego.com") && path.starts_with("/cdn/") {
        if sized(&parsed, &["width", "height"]) {
            return None;
        }
        set(
            &mut parsed,
            &[
                ("format", "jpg"),
                ("fit", "bounds"),
                ("quality", "80"),
                ("width", &edge),
                ("height", &edge),
                ("dpr", "1"),
            ],
        );
    } else if host == "cdn.shopify.com" || path.starts_with("/cdn/shop/") {
        if sized(&parsed, &["width", "height"]) {
            return None;
        }
        set(&mut parsed, &[("width", &edge)]);
    } else if host.ends_with(".imgix.net") {
        if sized(&parsed, &["w", "h"]) {
            return None;
        }
        set(
            &mut parsed,
            &[("w", &edge), ("fit", "max"), ("auto", "format,compress")],
        );
    } else if host == "images.ctfassets.net" {
        if sized(&parsed, &["w", "h"]) {
            return None;
        }
        set(&mut parsed, &[("w", &edge), ("fm", "jpg"), ("q", "80")]);
    } else if host == "cdn.sanity.io" {
        if sized(&parsed, &["w", "h"]) {
            return None;
        }
        set(
            &mut parsed,
            &[("w", &edge), ("auto", "format"), ("fit", "max")],
        );
    } else if host.ends_with("muscache.com") && path.starts_with("/im/") {
        if sized(&parsed, &["im_w"]) {
            return None;
        }
        set(&mut parsed, &[("im_w", "1200")]);
    } else if host == "res.cloudinary.com" {
        let (head, tail) = path.split_once("/image/upload/")?;
        let first = tail.split('/').next().unwrap_or_default();
        if first
            .split(',')
            .any(|t| t.starts_with("w_") || t.starts_with("h_"))
        {
            return None;
        }
        parsed.set_path(&format!(
            "{head}/image/upload/c_limit,w_{edge},f_auto,q_auto/{tail}"
        ));
    } else {
        return None;
    }
    let variant = parsed.to_string();
    (variant != url && public_https(&parsed)).then_some(variant)
}

/// Fetches one public image without cookies, following at most three public
/// HTTPS redirects, and returns at most [`MAX_PUBLIC_IMAGE_BYTES`]. A CDN
/// that serves display sizes is asked for one first; the address as given is
/// the fallback.
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
    if let Some(variant) = display_variant(url).and_then(|v| url::Url::parse(&v).ok()) {
        if let Ok(bytes) = fetch_with(&client, variant).await {
            return Ok(bytes);
        }
    }
    fetch_with(&client, parsed).await
}

async fn fetch_with(client: &Client, url: url::Url) -> Result<Vec<u8>, PublicAssetError> {
    let response = client
        .get(url)
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
    fn cdns_are_asked_for_a_display_sized_picture() {
        let lego =
            display_variant("https://www.lego.com/cdn/cs/set/assets/blt1/21066_Prod_en-gb.png")
                .unwrap();
        assert!(
            lego.contains("width=1600") && lego.contains("format=jpg"),
            "{lego}"
        );
        let resized = display_variant(
            "https://www.lego.com/cdn/cs/set/assets/blt1/21064.png?fit=bounds&format=jpg&quality=80&width=3000&height=3000&dpr=1",
        )
        .unwrap();
        assert!(
            resized.contains("width=1600") && !resized.contains("3000"),
            "{resized}"
        );
        assert_eq!(
            display_variant("https://www.lego.com/cdn/cs/set/assets/a.png?width=800&height=800"),
            None
        );
        assert!(
            display_variant("https://cdn.shopify.com/s/files/1/a.jpg?v=12")
                .unwrap()
                .ends_with("v=12&width=1600")
        );
        assert_eq!(
            display_variant("https://res.cloudinary.com/demo/image/upload/v1/sample.jpg").as_deref(),
            Some("https://res.cloudinary.com/demo/image/upload/c_limit,w_1600,f_auto,q_auto/v1/sample.jpg")
        );
        assert_eq!(
            display_variant("https://res.cloudinary.com/demo/image/upload/w_400/v1/sample.jpg"),
            None
        );
        assert!(
            display_variant("https://a0.muscache.com/im/pictures/x.jpeg")
                .unwrap()
                .ends_with("im_w=1200")
        );
        assert!(display_variant("https://example.imgix.net/a.jpg")
            .unwrap()
            .contains("w=1600"));
        assert_eq!(display_variant("https://example.com/a.jpg"), None);
    }

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
