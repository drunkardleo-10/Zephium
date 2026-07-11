use url::Url;

const ALLOWED_SCHEMES: &[&str] = &["http", "https", "about"];
const SEARCH_BASE: &str = "https://duckduckgo.com/";

/// Turn raw omnibox text into a URL: explicit URLs pass through, bare hosts get
/// `https://`, everything else becomes a search query.
pub fn classify(input: &str) -> Url {
    let s = input.trim();
    if s.is_empty() {
        return Url::parse("about:blank").expect("static url");
    }
    if s.contains("://") {
        if let Ok(url) = Url::parse(s) {
            return url;
        }
    }
    if looks_like_host(s) {
        if let Ok(url) = Url::parse(&format!("https://{s}")) {
            return url;
        }
    }
    let mut url = Url::parse(SEARCH_BASE).expect("static url");
    url.query_pairs_mut().append_pair("q", s);
    url
}

/// Whether the omnibox input will be treated as a web search rather than a URL.
pub fn is_query(input: &str) -> bool {
    let s = input.trim();
    !s.is_empty() && !s.contains("://") && !looks_like_host(s)
}

/// Whether a URL may commit. Blocks file, javascript, internal and external app
/// schemes; only http/https/about pass.
pub fn is_allowed(url: &Url) -> bool {
    ALLOWED_SCHEMES.contains(&url.scheme())
}

/// Gate for page-initiated navigations (the engine hands us a resolved URL).
pub fn is_allowed_str(url: &str) -> bool {
    Url::parse(url).map(|u| is_allowed(&u)).unwrap_or(false)
}

fn looks_like_host(s: &str) -> bool {
    if s.contains(char::is_whitespace) {
        return false;
    }
    if s == "localhost" || s.starts_with("localhost:") || s.starts_with("localhost/") {
        return true;
    }
    let authority = s.split(['/', '?', '#']).next().unwrap_or(s);
    // `user@host` in bare input is an email or a `trusted.com@evil.com`
    // spoof; both belong in search, never in the address.
    if authority.contains('@') {
        return false;
    }
    let host = authority.split(':').next().unwrap_or(authority);
    if host.parse::<std::net::Ipv4Addr>().is_ok() {
        return true;
    }
    matches!(host.rsplit_once('.'), Some((label, tld)) if !label.is_empty() && tld.len() >= 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn bare_host_gets_https() {
        assert_eq!(classify("example.com").as_str(), "https://example.com/");
        assert_eq!(classify("  github.com  ").as_str(), "https://github.com/");
        assert_eq!(classify("sub.example.com:8080/x").scheme(), "https");
    }

    #[test]
    fn explicit_url_passes_through() {
        assert_eq!(classify("https://x.com/a").as_str(), "https://x.com/a");
    }

    #[test]
    fn is_query_splits_urls_from_text() {
        assert!(is_query("hello world"));
        assert!(!is_query("example.com"));
        assert!(!is_query("https://x.com/a"));
        assert!(!is_query(""));
    }

    #[test]
    fn text_becomes_search() {
        let u = classify("hello world");
        assert_eq!(u.host_str(), Some("duckduckgo.com"));
        assert_eq!(u.query(), Some("q=hello+world"));
    }

    #[test]
    fn scheme_gate_blocks_dangerous() {
        assert!(!is_allowed(&Url::parse("javascript:alert(1)").unwrap()));
        assert!(!is_allowed(&Url::parse("file:///etc/passwd").unwrap()));
        assert!(is_allowed(&Url::parse("https://example.com").unwrap()));
        assert!(is_allowed(&Url::parse("about:blank").unwrap()));
        assert!(is_allowed_str("https://x.com/"));
        assert!(!is_allowed_str("javascript:1"));
        assert!(!is_allowed_str("not a url"));
    }

    #[test]
    fn omnibox_routes_dangerous_to_search() {
        let u = classify("javascript:alert(1)");
        assert!(is_allowed(&u));
        assert_eq!(u.host_str(), Some("duckduckgo.com"));
    }

    #[test]
    fn userinfo_input_searches_instead_of_spoofing() {
        assert_eq!(
            classify("paypal.com@evil.com").host_str(),
            Some("duckduckgo.com")
        );
        assert_eq!(
            classify("someone@example.com").host_str(),
            Some("duckduckgo.com")
        );
        assert!(is_query("paypal.com@evil.com"));
    }

    #[test]
    fn ipv4_literals_navigate() {
        assert_eq!(classify("192.168.1.1").as_str(), "https://192.168.1.1/");
        assert_eq!(
            classify("192.168.1.1:8080/admin").as_str(),
            "https://192.168.1.1:8080/admin"
        );
        // not a valid address: searched, not navigated
        assert!(is_query("999.1.1.1"));
    }

    proptest! {
        #[test]
        fn classify_never_panics(s in "\\PC*") {
            let _ = classify(&s);
        }
    }
}
