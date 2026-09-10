//! Frozen authority for bounded discovery through observed non-sensitive links.
use super::*;

/// One canonical production navigation boundary.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct AgentNavigationOriginRule {
    origin: SemanticOrigin,
    path_prefix: String,
    allow_query: bool,
    allow_fragment: bool,
}

impl AgentNavigationOriginRule {
    /// Creates one slash-delimited path boundary under an already-approved origin.
    pub fn try_new(
        origin: SemanticOrigin,
        path_prefix: String,
        allow_query: bool,
        allow_fragment: bool,
    ) -> Result<Self, AgentManifestContractError> {
        if !valid_path_prefix(&path_prefix) {
            return Err(AgentManifestContractError::NavigationRoute);
        }
        Ok(Self {
            origin,
            path_prefix,
            allow_query,
            allow_fragment,
        })
    }
    /// Canonical allowed origin.
    pub const fn origin(&self) -> &SemanticOrigin {
        &self.origin
    }
    /// Slash-delimited allowed path prefix.
    pub fn path_prefix(&self) -> &str {
        &self.path_prefix
    }
    /// Whether exact public query-bearing links are allowed.
    pub const fn allows_query(&self) -> bool {
        self.allow_query
    }
    /// Whether exact public fragment-bearing links are allowed.
    pub const fn allows_fragment(&self) -> bool {
        self.allow_fragment
    }
    fn admits(&self, target: &crate::ContextNavigationTarget) -> bool {
        let url = target.as_url();
        SemanticOrigin::parse(url.as_str()).as_ref() == Ok(&self.origin)
            && url.username().is_empty()
            && url.password().is_none()
            && url.path().starts_with(&self.path_prefix)
            && safe_path(url.path())
            && (self.allow_query || url.query().is_none())
            && (self.allow_fragment || url.fragment().is_none())
    }
}

impl fmt::Debug for AgentNavigationOriginRule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentNavigationOriginRule")
            .field("allow_query", &self.allow_query)
            .field("allow_fragment", &self.allow_fragment)
            .field("scope", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum DiscoveryProfile {
    Restrictive,
    Production,
}

/// Read-only navigation scope. Destinations are selected from the current
/// acknowledged document, never added to authority by model text.
#[derive(Clone, Eq, PartialEq)]
pub struct AgentNavigationDiscovery {
    departure: crate::ContextNavigationTarget,
    origin: SemanticOrigin,
    path_prefix: String,
    max_hops: usize,
    document_policy: crate::WorkBrowserDocumentPolicy,
    profile: DiscoveryProfile,
    rules: Vec<AgentNavigationOriginRule>,
    max_visits_per_destination: usize,
}

impl AgentNavigationDiscovery {
    /// Approves the legacy restrictive same-origin subtree.
    pub fn try_new(
        departure: crate::ContextNavigationTarget,
        path_prefix: String,
        max_hops: usize,
    ) -> Result<Self, AgentManifestContractError> {
        Self::try_new_with_document_policy(
            departure,
            path_prefix,
            max_hops,
            crate::WorkBrowserDocumentPolicy::Exact,
        )
    }

    /// Adds trusted query finalization without changing restrictive discovery.
    pub fn try_new_with_document_policy(
        departure: crate::ContextNavigationTarget,
        path_prefix: String,
        max_hops: usize,
        document_policy: crate::WorkBrowserDocumentPolicy,
    ) -> Result<Self, AgentManifestContractError> {
        let origin = SemanticOrigin::parse(departure.as_url().as_str())
            .map_err(|_| AgentManifestContractError::NavigationRoute)?;
        if document_policy == crate::WorkBrowserDocumentPolicy::InitialQueryFinalization
            || !document_policy.admits_request(&departure)
            || max_hops == 0
            || max_hops > MAX_AGENT_NAVIGATION_ROUTE_HOPS
            || !valid_restrictive_prefix(&path_prefix)
            || departure.as_url().query().is_some()
            || departure.as_url().fragment().is_some()
            || departure.as_url().as_str().len() > crate::MAX_AGENT_BROWSER_NAVIGATION_URL_BYTES
        {
            return Err(AgentManifestContractError::NavigationRoute);
        }
        let rules = vec![AgentNavigationOriginRule::try_new(
            origin.clone(),
            path_prefix.clone(),
            false,
            false,
        )?];
        Ok(Self {
            departure,
            origin,
            path_prefix,
            max_hops,
            document_policy,
            profile: DiscoveryProfile::Restrictive,
            rules,
            max_visits_per_destination: 1,
        })
    }

    /// Freezes bounded production browsing. Every destination must still be an
    /// exact public link in the current acknowledged document.
    pub fn try_new_production(
        departure: crate::ContextNavigationTarget,
        mut rules: Vec<AgentNavigationOriginRule>,
        max_hops: usize,
        max_visits_per_destination: usize,
    ) -> Result<Self, AgentManifestContractError> {
        let origin = SemanticOrigin::parse(departure.as_url().as_str())
            .map_err(|_| AgentManifestContractError::NavigationRoute)?;
        rules.sort();
        if rules.is_empty()
            || rules.len() > MAX_AGENT_NAVIGATION_DISCOVERY_RULES
            || rules.windows(2).any(|pair| pair[0] == pair[1])
            || max_hops == 0
            || max_hops > MAX_AGENT_NAVIGATION_DISCOVERY_HOPS
            || max_visits_per_destination == 0
            || max_visits_per_destination > MAX_AGENT_NAVIGATION_DESTINATION_VISITS
            || departure.as_url().as_str().len() > crate::MAX_AGENT_BROWSER_NAVIGATION_URL_BYTES
            || !crate::semantic_wire::model_safe_public_url(&departure)
            || !rules.iter().any(|rule| rule.admits(&departure))
        {
            return Err(AgentManifestContractError::NavigationRoute);
        }
        let path_prefix = rules
            .iter()
            .find(|rule| rule.origin() == &origin && rule.admits(&departure))
            .ok_or(AgentManifestContractError::NavigationRoute)?
            .path_prefix
            .clone();
        Ok(Self {
            departure,
            origin,
            path_prefix,
            max_hops,
            document_policy: crate::WorkBrowserDocumentPolicy::Exact,
            profile: DiscoveryProfile::Production,
            rules,
            max_visits_per_destination,
        })
    }

    /// Exact starting document.
    pub const fn departure(&self) -> &crate::ContextNavigationTarget {
        &self.departure
    }
    /// Canonical departure origin.
    pub const fn origin(&self) -> &SemanticOrigin {
        &self.origin
    }
    /// Canonical origins named by this scope.
    pub fn origins(&self) -> impl Iterator<Item = &SemanticOrigin> {
        self.rules.iter().map(AgentNavigationOriginRule::origin)
    }
    /// Restrictive prefix, or the production rule covering departure.
    pub fn path_prefix(&self) -> &str {
        &self.path_prefix
    }
    /// Maximum committed transitions.
    pub const fn max_hops(&self) -> usize {
        self.max_hops
    }
    /// Native final-document policy.
    pub const fn document_policy(&self) -> crate::WorkBrowserDocumentPolicy {
        self.document_policy
    }
    /// Whether this is the separately authorized production profile.
    pub const fn is_production(&self) -> bool {
        matches!(self.profile, DiscoveryProfile::Production)
    }
    /// Canonical production rules; restrictive profiles contain one equivalent rule.
    pub fn rules(&self) -> &[AgentNavigationOriginRule] {
        &self.rules
    }
    /// Maximum visits to one exact destination.
    pub const fn max_visits_per_destination(&self) -> usize {
        self.max_visits_per_destination
    }
    /// Tests whether an origin was explicitly named.
    pub fn admits_origin(&self, origin: &SemanticOrigin) -> bool {
        self.rules.iter().any(|rule| rule.origin() == origin)
    }
    /// Scope-only check; policy also requires a current public link and exact history budgets.
    pub fn admits(&self, target: &crate::ContextNavigationTarget) -> bool {
        target.as_url().as_str().len() <= crate::MAX_AGENT_BROWSER_NAVIGATION_URL_BYTES
            && self.rules.iter().any(|rule| rule.admits(target))
            && (self.is_production() || !target.as_url().path().contains('%'))
            && (self.is_production() || target != &self.departure)
    }
}

impl fmt::Debug for AgentNavigationDiscovery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentNavigationDiscovery")
            .field(
                "profile",
                &if self.is_production() {
                    "production"
                } else {
                    "restrictive"
                },
            )
            .field("rules", &self.rules.len())
            .field("max_hops", &self.max_hops)
            .field(
                "max_visits_per_destination",
                &self.max_visits_per_destination,
            )
            .field("scope", &"[redacted]")
            .finish()
    }
}

fn valid_restrictive_prefix(path: &str) -> bool {
    valid_path_prefix(path) && !path.contains('%')
}
fn valid_path_prefix(path: &str) -> bool {
    path.starts_with('/')
        && path.ends_with('/')
        && path.len() <= 1024
        && !path.contains(['?', '#', '\\'])
        && !path.split('/').any(|part| matches!(part, "." | ".."))
        && safe_path(path)
}
fn safe_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            index += 1;
            continue;
        }
        let Some(pair) = bytes.get(index + 1..index + 3) else {
            return false;
        };
        let Some(value) = hex(pair[0]).and_then(|high| hex(pair[1]).map(|low| high * 16 + low))
        else {
            return false;
        };
        if matches!(value, b'%' | b'/' | b'\\' | b'.' | b'?' | b'#' | 0) {
            return false;
        }
        index += 3;
    }
    true
}
const fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target(value: &str) -> crate::ContextNavigationTarget {
        crate::ContextNavigationTarget::parse(value).unwrap()
    }
    fn rule(origin: &str, path: &str, query: bool, fragment: bool) -> AgentNavigationOriginRule {
        AgentNavigationOriginRule::try_new(
            SemanticOrigin::parse(origin).unwrap(),
            path.into(),
            query,
            fragment,
        )
        .unwrap()
    }
    #[test]
    fn restrictive_discovery_contract_is_unchanged() {
        let initial = target("https://example.test/docs");
        let scope = AgentNavigationDiscovery::try_new(initial.clone(), "/docs/".into(), 2).unwrap();
        assert!(scope.admits(&target("https://example.test/docs/topic")));
        for denied in [
            "https://example.test/docs",
            "https://example.test/docs-other/topic",
            "https://other.test/docs/topic",
            "https://example.test/docs/topic?q=1",
            "https://example.test/docs/topic#part",
            "https://example.test/docs/topic%20name",
            "https://example.test/docs/%61",
            "https://example.test/docs/%2e%2e/private",
        ] {
            assert!(!scope.admits(&target(denied)), "{denied}");
        }
        assert!(!scope.is_production());
    }
    #[test]
    fn production_scope_is_explicit_bounded_and_supports_normal_links() {
        let departure = target("https://search.example.test/app/start?q=one#top");
        let scope = AgentNavigationDiscovery::try_new_production(
            departure.clone(),
            vec![
                rule("https://search.example.test", "/app/", true, true),
                rule("https://docs.example.test", "/guide/", true, false),
            ],
            12,
            2,
        )
        .unwrap();
        assert!(scope.admits(&departure));
        assert!(scope.admits(&target("https://search.example.test/app/results?q=two#row")));
        assert!(scope.admits(&target("https://docs.example.test/guide/a%20b?q=two")));
        for denied in [
            "https://search.example.test/private/x",
            "https://docs.example.test/guide/a#fragment",
            "https://docs.example.test/guide/%2e%2e/private",
            "https://docs.example.test/guide/%252e%252e/private",
            "https://other.example.test/guide/x",
        ] {
            assert!(!scope.admits(&target(denied)), "{denied}");
        }
        assert_eq!(scope.max_hops(), 12);
        assert_eq!(scope.max_visits_per_destination(), 2);
        assert!(!format!("{scope:?}").contains("example.test"));
    }
    #[test]
    fn production_scope_rejects_unbounded_or_ambiguous_rules() {
        let departure = target("https://example.test/app/start");
        let base = rule("https://example.test", "/app/", false, false);
        assert!(AgentNavigationDiscovery::try_new_production(
            departure.clone(),
            vec![base.clone(), base],
            2,
            1
        )
        .is_err());
        assert!(AgentNavigationDiscovery::try_new_production(
            departure.clone(),
            vec![rule("https://example.test", "/app/", false, false)],
            MAX_AGENT_NAVIGATION_DISCOVERY_HOPS + 1,
            1
        )
        .is_err());
        assert!(AgentNavigationDiscovery::try_new_production(
            departure,
            vec![rule("https://example.test", "/other/", false, false)],
            2,
            1
        )
        .is_err());
        for sensitive in [
            "https://example.test/app/start?token=shortsecret",
            "https://example.test/app/start?access%5Ftoken=shortsecret",
            "https://example.test/app/start?code=Qm9VT3F2cW1ROGxobTVoQ2c",
            "https://example.test/app/start#q=ghp%5Fabcdefghijklmnop",
            "https://example.test/app/start#access_token%3Dshortsecret",
            "https://example.test/app/start?access_token%3Dshortsecret",
            "https://example.test/app/start#q=token=shortsecret",
        ] {
            assert!(
                AgentNavigationDiscovery::try_new_production(
                    target(sensitive),
                    vec![rule("https://example.test", "/app/", true, true)],
                    2,
                    1,
                )
                .is_err(),
                "{sensitive}"
            );
        }
    }
}
