//! Non-authorizing recognition of a native top-level store location.

use url::Url;

use crate::ChromiumExtensionId;

/// One recognized Chrome Web Store listing. This value contains no package,
/// source-provider eligibility, user gesture, consent, or install authority.
/// The browser must independently bind it to the live top-level document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChromeWebStoreListing {
    extension_id: ChromiumExtensionId,
}

impl ChromeWebStoreListing {
    /// Recognizes an exact current store origin and `/detail/id` or
    /// `/detail/slug/id` path. Query parameters are display-only and never
    /// supply package identity, download endpoints, or installation authority.
    pub fn parse_top_level_url(value: &str) -> Option<Self> {
        if value.len() > 2048
            || value.contains('\\')
            || value
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
        {
            return None;
        }
        let url = Url::parse(value).ok()?;
        if url.scheme() != "https"
            || url.host_str() != Some("chromewebstore.google.com")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.fragment().is_some()
        {
            return None;
        }
        let mut segments = url.path_segments()?;
        if segments.next()? != "detail" {
            return None;
        }
        let first = segments.next()?;
        let id = match segments.next() {
            None => first,
            // Localized titles may be percent-encoded. The slug is cosmetic;
            // only the final canonical id selects an extension. Encoded path
            // separators remain refused to avoid router/path disagreement.
            Some(id)
                if !first.is_empty()
                    && first.len() <= 768
                    && !first.as_bytes().windows(3).any(|bytes| {
                        bytes.eq_ignore_ascii_case(b"%2f") || bytes.eq_ignore_ascii_case(b"%5c")
                    }) =>
            {
                id
            }
            _ => return None,
        };
        if segments.next().is_some() {
            return None;
        }
        Some(Self {
            extension_id: ChromiumExtensionId::parse(id).ok()?,
        })
    }

    /// Returns only the validated listing identifier.
    pub const fn extension_id(&self) -> &ChromiumExtensionId {
        &self.extension_id
    }

    /// Builds the real store link for a recommendation without a server URL.
    pub fn url_for_id(id: &ChromiumExtensionId) -> Url {
        Url::parse(&format!(
            "https://chromewebstore.google.com/detail/{}",
            id.as_str()
        ))
        .expect("validated Chromium id produces a fixed HTTPS store URL")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "dbepggeogbaibhgnhhndojpepiihcmeb";

    #[test]
    fn recognizes_real_listing_without_trusting_query_identity() {
        let id = ChromiumExtensionId::parse(ID).unwrap();
        let canonical = ChromeWebStoreListing::url_for_id(&id);
        assert_eq!(
            ChromeWebStoreListing::parse_top_level_url(canonical.as_str())
                .unwrap()
                .extension_id(),
            &id
        );
        let url = format!("https://chromewebstore.google.com/detail/vimium/{ID}?hl=en&extension_id=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa&url=https://evil.example/");
        assert_eq!(
            ChromeWebStoreListing::parse_top_level_url(&url)
                .unwrap()
                .extension_id(),
            &id
        );
        let localized =
            format!("https://chromewebstore.google.com/detail/%E6%8B%A1%E5%BC%B5/{ID}?hl=ja");
        assert_eq!(
            ChromeWebStoreListing::parse_top_level_url(&localized)
                .unwrap()
                .extension_id(),
            &id
        );
    }

    #[test]
    fn rejects_spoofed_origins_ambiguous_paths_and_non_listing_pages() {
        for url in [
            format!("http://chromewebstore.google.com/detail/{ID}"),
            format!("https://chromewebstore.google.com.evil.example/detail/{ID}"),
            format!("https://chromewebstore.google.com@evil.example/detail/{ID}"),
            format!("https://user@chromewebstore.google.com/detail/{ID}"),
            format!("https://chromewebstore.google.com:444/detail/{ID}"),
            format!("https://chromewebstore.google.com/detail/{ID}#install"),
            format!("https://chromewebstore.google.com/detail/{ID}/extra"),
            format!("https://chromewebstore.google.com/detail//{ID}"),
            format!("https://chromewebstore.google.com/detail/name%2fother/{ID}"),
            format!("https://chromewebstore.google.com/detail/{ID}/"),
            format!("https://chromewebstore.google.com/detail/%64{}", &ID[1..]),
            format!("https://chromewebstore.google.com\\detail\\{ID}"),
            format!("https://chromewebstore.google.com/search/{ID}"),
            format!(
                "https://chromewebstore.google.com/detail/{}",
                ID.to_uppercase()
            ),
        ] {
            assert!(
                ChromeWebStoreListing::parse_top_level_url(&url).is_none(),
                "{url}"
            );
        }
    }
}
