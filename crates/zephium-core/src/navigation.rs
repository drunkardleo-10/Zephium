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

/// Whether a URL may commit. Blocks file, javascript, internal and external app
/// schemes; only http/https/about pass.
pub fn is_allowed(url: &Url) -> bool {
    ALLOWED_SCHEMES.contains(&url.scheme())
}

fn looks_like_host(s: &str) -> bool {
    if s.contains(char::is_whitespace) {
        return false;
    }
    if s == "localhost" || s.starts_with("localhost:") || s.starts_with("localhost/") {
        return true;
    }
    let host = s.split(['/', '?', '#']).next().unwrap_or(s);
    let host = host.split(':').next().unwrap_or(host);
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
    }

    #[test]
    fn omnibox_routes_dangerous_to_search() {
        let u = classify("javascript:alert(1)");
        assert!(is_allowed(&u));
        assert_eq!(u.host_str(), Some("duckduckgo.com"));
    }

    proptest! {
        #[test]
        fn classify_never_panics(s in "\\PC*") {
            let _ = classify(&s);
        }
    }
}
