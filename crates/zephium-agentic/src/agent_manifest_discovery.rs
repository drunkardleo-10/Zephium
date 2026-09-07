//! Frozen authority for bounded discovery through observed public links.
use super::*;

/// Public, read-only navigation scope. Destinations are selected from the
/// current acknowledged document, never added to authority by model text.
#[derive(Clone, Eq, PartialEq)]
pub struct AgentNavigationDiscovery {
    departure: crate::ContextNavigationTarget,
    origin: SemanticOrigin,
    path_prefix: String,
    max_hops: usize,
}

impl AgentNavigationDiscovery {
    /// Approves an exact starting document and one same-origin path subtree.
    /// The prefix is slash-delimited; queries, fragments and repeats are absent.
    pub fn try_new(
        departure: crate::ContextNavigationTarget,
        path_prefix: String,
        max_hops: usize,
    ) -> Result<Self, AgentManifestContractError> {
        let origin = SemanticOrigin::parse(departure.as_url().as_str())
            .map_err(|_| AgentManifestContractError::NavigationRoute)?;
        if max_hops == 0
            || max_hops > MAX_AGENT_NAVIGATION_ROUTE_HOPS
            || !path_prefix.starts_with('/')
            || !path_prefix.ends_with('/')
            || path_prefix.len() > 1024
            || path_prefix.contains(['?', '#', '%', '\\'])
            || path_prefix
                .split('/')
                .any(|part| matches!(part, "." | ".."))
            || departure.as_url().query().is_some()
            || departure.as_url().fragment().is_some()
            || departure.as_url().as_str().len() > crate::MAX_AGENT_BROWSER_NAVIGATION_URL_BYTES
        {
            return Err(AgentManifestContractError::NavigationRoute);
        }
        Ok(Self {
            departure,
            origin,
            path_prefix,
            max_hops,
        })
    }

    /// Exact starting document, not an answer or route hint.
    pub const fn departure(&self) -> &crate::ContextNavigationTarget {
        &self.departure
    }
    /// Frozen canonical origin.
    pub const fn origin(&self) -> &SemanticOrigin {
        &self.origin
    }
    /// Approved slash-delimited path subtree.
    pub fn path_prefix(&self) -> &str {
        &self.path_prefix
    }
    /// Total navigation allowance under the original run budgets.
    pub const fn max_hops(&self) -> usize {
        self.max_hops
    }
    /// Tests scope only; the policy additionally requires current source links.
    pub fn admits(&self, target: &crate::ContextNavigationTarget) -> bool {
        target != &self.departure
            && target.as_url().query().is_none()
            && target.as_url().fragment().is_none()
            && target.as_url().as_str().len() <= crate::MAX_AGENT_BROWSER_NAVIGATION_URL_BYTES
            && SemanticOrigin::parse(target.as_url().as_str()).as_ref() == Ok(&self.origin)
            && target.as_url().path().starts_with(&self.path_prefix)
            && !target.as_url().path().contains('%')
    }
}

impl fmt::Debug for AgentNavigationDiscovery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentNavigationDiscovery")
            .field("max_hops", &self.max_hops)
            .field("scope", &"[redacted]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target(value: &str) -> crate::ContextNavigationTarget {
        crate::ContextNavigationTarget::parse(value).unwrap()
    }
    #[test]
    fn discovery_is_bounded_same_origin_and_slash_delimited() {
        let initial = target("https://example.test/docs");
        let scope = AgentNavigationDiscovery::try_new(initial.clone(), "/docs/".into(), 2).unwrap();
        assert!(scope.admits(&target("https://example.test/docs/topic")));
        for denied in [
            "https://example.test/docs",
            "https://example.test/docs-other/topic",
            "https://other.test/docs/topic",
            "http://example.test/docs/topic",
            "https://example.test:444/docs/topic",
            "https://example.test/docs/topic?q=1",
            "https://example.test/docs/topic#part",
            "https://example.test/docs/%2e%2e/private",
            "https://example.test/docs/a%2fb",
        ] {
            assert!(!scope.admits(&target(denied)), "{denied}");
        }
        for (prefix, hops) in [
            ("/docs/", 0),
            ("/docs/", 3),
            ("docs/", 1),
            ("/docs", 1),
            ("/docs/../", 1),
            ("/docs/?", 1),
            ("/docs/%2f/", 1),
        ] {
            assert!(
                AgentNavigationDiscovery::try_new(initial.clone(), prefix.into(), hops).is_err()
            );
        }
        assert!(!format!("{scope:?}").contains("example.test"));
    }
}
