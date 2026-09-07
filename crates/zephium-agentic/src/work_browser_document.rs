//! Trusted initial-document policy, not model or successor navigation authority.
use crate::ContextNavigationTarget;

/// Closed construction policy supplied before native allocation or dispatch.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WorkBrowserDocumentPolicy {
    /// The native current URL must remain exactly the requested URL.
    #[default]
    Exact,
    /// After the exact initial HTTPS document finishes, freeze its one native
    /// location sample. A query-free request may acquire an opaque nonempty
    /// query, but no other URL bytes may change. No second load is authorized.
    InitialQueryFinalization,
}

impl WorkBrowserDocumentPolicy {
    /// Checks trusted admission, before any request is sent to the native port.
    pub fn admits_request(self, requested: &ContextNavigationTarget) -> bool {
        let url = requested.as_url();
        self == Self::Exact
            || (url.scheme() == "https"
                && url.query().is_none()
                && url.fragment().is_none()
                && url.username().is_empty()
                && url.password().is_none())
    }

    /// Rechecks a native construction receipt. This relation alone is not a
    /// native identity/commit proof; only the original request may supply one.
    pub fn admits_final_document(
        self,
        requested: &ContextNavigationTarget,
        effective: &ContextNavigationTarget,
    ) -> bool {
        if !self.admits_request(requested) {
            return false;
        }
        if requested == effective {
            return true;
        }
        self == Self::InitialQueryFinalization
            && effective
                .as_url()
                .query()
                .is_some_and(|query| !query.is_empty())
            && effective.as_url().fragment().is_none()
            && effective
                .as_url()
                .as_str()
                .split_once('?')
                .map(|(base, _)| base)
                == Some(requested.as_url().as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_relation_is_opaque_but_never_a_url_or_navigation_substitution() {
        let source = ContextNavigationTarget::parse("https://example.test/product").unwrap();
        let policy = WorkBrowserDocumentPolicy::InitialQueryFinalization;
        for query in ["a=one", "arbitrary=two&another=three"] {
            let target =
                ContextNavigationTarget::parse(&format!("https://example.test/product?{query}"))
                    .unwrap();
            assert!(policy.admits_final_document(&source, &target));
            assert!(!WorkBrowserDocumentPolicy::Exact.admits_final_document(&source, &target));
            assert!(!policy.admits_request(&target));
        }
        for changed in [
            "https://example.test/product?",
            "https://example.test/product?x=1#f",
            "https://example.test/other?x=1",
            "https://else.test/product?x=1",
            "http://example.test/product?x=1",
            "https://example.test:444/product?x=1",
        ] {
            let target = ContextNavigationTarget::parse(changed).unwrap();
            assert!(!policy.admits_final_document(&source, &target));
        }
        assert!(policy.admits_final_document(&source, &source));
        let prefix = "https://example.test/product?";
        let at_bound = ContextNavigationTarget::parse(&format!(
            "{prefix}{}",
            "x".repeat(8_192 - prefix.len())
        ))
        .unwrap();
        assert!(policy.admits_final_document(&source, &at_bound));
        // The existing target type enforces the same byte ceiling before a
        // native receipt can carry a page-derived URL into this relation.
        assert!(ContextNavigationTarget::parse(&format!("{}x", at_bound.as_url())).is_err());
        assert!(
            ContextNavigationTarget::parse("https://user:secret@example.test/product?x=1").is_err()
        );
    }
}
