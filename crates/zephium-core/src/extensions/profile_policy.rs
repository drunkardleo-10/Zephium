//! Profile-wide extension execution policy, separate from per-install grants.

use std::error::Error;
use std::fmt;
use std::mem::size_of;

use sha2::{Digest, Sha256};
use url::{Host, Url};

use crate::injection::{
    MatchPattern, MatchPatternComponents, MatchPatternHost, MatchPatternPort, MatchPatternScheme,
    MAX_MATCH_PATTERN_RETAINED_BUDGET_BYTES,
};

use super::EXTENSION_SHA256_BYTES;

const MAX_DURABLE_EXTENSION_PROFILE_POLICY_REVISION: u64 = i64::MAX as u64;
const PROFILE_POLICY_FIXED_BYTES: usize = 512;
const PROFILE_POLICY_DIGEST_DOMAIN: &[u8] = b"zephium.extension.profile-policy.v1\0";

/// Maximum exact web hosts a profile may pause extensions on.
pub const MAX_EXTENSION_SITE_DENIALS_PER_PROFILE: usize = 128;
/// Conservative retained-memory ceiling for one complete profile policy.
pub const MAX_EXTENSION_PROFILE_POLICY_RETAINED_BYTES: usize = PROFILE_POLICY_FIXED_BYTES
    + MAX_EXTENSION_SITE_DENIALS_PER_PROFILE
        * (size_of::<ExtensionSiteAccessScope>() + MAX_MATCH_PATTERN_RETAINED_BUDGET_BYTES);
pub const MAX_EXTENSION_PROFILE_POLICY_MUTATION_RETAINED_BYTES: usize =
    size_of::<ExtensionProfilePolicyMutation>() + MAX_MATCH_PATTERN_RETAINED_BUDGET_BYTES + 64;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExtensionProfilePolicyRevision(u64);

impl ExtensionProfilePolicyRevision {
    pub const INITIAL: Self = Self(1);

    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 || value > MAX_DURABLE_EXTENSION_PROFILE_POLICY_REVISION {
            None
        } else {
            Some(Self(value))
        }
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Self::new(next),
            None => None,
        }
    }
}

/// Content digest of the complete profile-wide execution policy.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExtensionProfilePolicyDigest([u8; EXTENSION_SHA256_BYTES]);

impl ExtensionProfilePolicyDigest {
    pub const fn from_bytes(bytes: [u8; EXTENSION_SHA256_BYTES]) -> Self {
        Self(bytes)
    }

    pub const fn bytes(self) -> [u8; EXTENSION_SHA256_BYTES] {
        self.0
    }
}

impl fmt::Debug for ExtensionProfilePolicyDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "ExtensionProfilePolicyDigest({:02x}{:02x}{:02x}{:02x}…)",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}

/// Canonical whole-host scope for pausing extension access.
///
/// A scope names exactly one HTTP or HTTPS DNS/IPv4 host, all ports and all
/// paths. Wildcards, file URLs, IPv6, and path-specific rules are deliberately
/// absent until every selected native backend can enforce the same semantics.
#[derive(Clone, Eq, PartialEq)]
pub struct ExtensionSiteAccessScope(MatchPattern);

impl fmt::Debug for ExtensionSiteAccessScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ExtensionSiteAccessScope(<redacted>)")
    }
}

impl ExtensionSiteAccessScope {
    pub fn from_url(url: &Url) -> Result<Self, ExtensionProfilePolicyError> {
        let scheme = match url.scheme() {
            "http" => "http",
            "https" => "https",
            _ => return Err(ExtensionProfilePolicyError::InvalidSiteScope),
        };
        if !url.username().is_empty() || url.password().is_some() {
            return Err(ExtensionProfilePolicyError::InvalidSiteScope);
        }
        let host = match url.host() {
            Some(Host::Domain(domain)) => domain.to_owned(),
            Some(Host::Ipv4(address)) => address.to_string(),
            Some(Host::Ipv6(_)) | None => {
                return Err(ExtensionProfilePolicyError::InvalidSiteScope)
            }
        };
        Self::parse_exact(&format!("{scheme}://{host}/*"))
    }

    pub fn parse_exact(value: &str) -> Result<Self, ExtensionProfilePolicyError> {
        let pattern = MatchPattern::parse(value)
            .map_err(|_| ExtensionProfilePolicyError::InvalidSiteScope)?;
        if pattern.as_str() != value {
            return Err(ExtensionProfilePolicyError::InvalidSiteScope);
        }
        let valid = matches!(
            pattern.components(),
            MatchPatternComponents::Standard {
                scheme: MatchPatternScheme::Http | MatchPatternScheme::Https,
                host: Some(MatchPatternHost::ExactDomain(_) | MatchPatternHost::ExactIpv4(_)),
                port: MatchPatternPort::Any,
                path,
            } if path.as_str() == "/*"
        );
        if !valid {
            return Err(ExtensionProfilePolicyError::InvalidSiteScope);
        }
        Ok(Self(pattern))
    }

    pub fn pattern(&self) -> &MatchPattern {
        &self.0
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn retained_bytes(&self) -> usize {
        self.0.retained_budget_bytes()
    }
}

/// Complete durable profile-wide policy. Per-install grants are intersected
/// with this policy; neither domain may mint authority for the other.
#[derive(Clone, Eq, PartialEq)]
pub struct ExtensionProfilePolicy {
    revision: ExtensionProfilePolicyRevision,
    digest: ExtensionProfilePolicyDigest,
    paused: bool,
    denied_sites: Box<[ExtensionSiteAccessScope]>,
    retained_bytes: usize,
}

impl fmt::Debug for ExtensionProfilePolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionProfilePolicy")
            .field("revision", &self.revision)
            .field("digest", &self.digest)
            .field("paused", &self.paused)
            .field("denied_site_count", &self.denied_sites.len())
            .field("retained_bytes", &self.retained_bytes)
            .finish()
    }
}

impl ExtensionProfilePolicy {
    pub fn initial() -> Self {
        Self::build(ExtensionProfilePolicyRevision::INITIAL, false, Vec::new())
            .expect("the empty initial extension policy is valid")
    }

    pub fn from_persisted(
        revision: ExtensionProfilePolicyRevision,
        paused: bool,
        denied_sites: Vec<ExtensionSiteAccessScope>,
    ) -> Result<Self, ExtensionProfilePolicyError> {
        Self::build(revision, paused, denied_sites)
    }

    fn build(
        revision: ExtensionProfilePolicyRevision,
        paused: bool,
        denied_sites: Vec<ExtensionSiteAccessScope>,
    ) -> Result<Self, ExtensionProfilePolicyError> {
        if denied_sites.len() > MAX_EXTENSION_SITE_DENIALS_PER_PROFILE {
            return Err(ExtensionProfilePolicyError::TooManySiteDenials);
        }
        let mut denied_sites = denied_sites.into_boxed_slice();
        denied_sites.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
        if denied_sites
            .windows(2)
            .any(|pair| pair[0].as_str() == pair[1].as_str())
        {
            return Err(ExtensionProfilePolicyError::DuplicateSiteDenial);
        }
        let retained_bytes = denied_sites.iter().try_fold(
            PROFILE_POLICY_FIXED_BYTES
                .checked_add(denied_sites.len() * size_of::<ExtensionSiteAccessScope>())
                .ok_or(ExtensionProfilePolicyError::AccountingOverflow)?,
            |bytes, scope| {
                bytes
                    .checked_add(scope.retained_bytes())
                    .ok_or(ExtensionProfilePolicyError::AccountingOverflow)
            },
        )?;
        if retained_bytes > MAX_EXTENSION_PROFILE_POLICY_RETAINED_BYTES {
            return Err(ExtensionProfilePolicyError::RetainedBytesExceeded);
        }
        let digest = policy_digest(paused, &denied_sites)?;
        Ok(Self {
            revision,
            digest,
            paused,
            denied_sites,
            retained_bytes,
        })
    }

    pub const fn revision(&self) -> ExtensionProfilePolicyRevision {
        self.revision
    }

    pub const fn digest(&self) -> ExtensionProfilePolicyDigest {
        self.digest
    }

    pub const fn paused(&self) -> bool {
        self.paused
    }

    pub fn denied_sites(&self) -> &[ExtensionSiteAccessScope] {
        &self.denied_sites
    }

    pub fn denies(&self, scope: &ExtensionSiteAccessScope) -> bool {
        self.denied_sites
            .binary_search_by(|candidate| candidate.as_str().cmp(scope.as_str()))
            .is_ok()
    }

    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub fn apply(
        &self,
        expected: ExtensionProfilePolicyRevision,
        mutation: ExtensionProfilePolicyMutation,
    ) -> Result<ExtensionProfilePolicyApplication, ExtensionProfilePolicyApplyError> {
        if expected != self.revision {
            return Err(ExtensionProfilePolicyApplyError::RevisionConflict {
                expected,
                current: self.revision,
            });
        }
        let mut paused = self.paused;
        let mut denied_sites = self.denied_sites.to_vec();
        match mutation {
            ExtensionProfilePolicyMutation::SetPaused(value) => paused = value,
            ExtensionProfilePolicyMutation::SetSiteDenied { scope, denied } => {
                match denied_sites
                    .binary_search_by(|candidate| candidate.as_str().cmp(scope.as_str()))
                {
                    Ok(index) if !denied => {
                        denied_sites.remove(index);
                    }
                    Ok(_) => {}
                    Err(index) if denied => {
                        if denied_sites.len() == MAX_EXTENSION_SITE_DENIALS_PER_PROFILE {
                            return Err(ExtensionProfilePolicyApplyError::LimitReached);
                        }
                        denied_sites.insert(index, scope);
                    }
                    Err(_) => {}
                }
            }
        }
        let changed = paused != self.paused || denied_sites.as_slice() != self.denied_sites();
        let revision = if changed {
            self.revision
                .next()
                .ok_or(ExtensionProfilePolicyApplyError::RevisionExhausted)?
        } else {
            self.revision
        };
        let policy = Self::build(revision, paused, denied_sites)
            .map_err(ExtensionProfilePolicyApplyError::InvalidPolicy)?;
        Ok(ExtensionProfilePolicyApplication { policy, changed })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionProfilePolicyMutation {
    SetPaused(bool),
    SetSiteDenied {
        scope: ExtensionSiteAccessScope,
        denied: bool,
    },
}

impl ExtensionProfilePolicyMutation {
    pub fn retained_bytes(&self) -> usize {
        let payload = match self {
            Self::SetPaused(_) => 0,
            Self::SetSiteDenied { scope, .. } => scope.retained_bytes(),
        };
        let retained = size_of::<Self>().saturating_add(payload);
        debug_assert!(retained <= MAX_EXTENSION_PROFILE_POLICY_MUTATION_RETAINED_BYTES);
        retained
    }
}

#[must_use = "profile policy application must be persisted or discarded"]
pub struct ExtensionProfilePolicyApplication {
    policy: ExtensionProfilePolicy,
    changed: bool,
}

impl ExtensionProfilePolicyApplication {
    pub const fn changed(&self) -> bool {
        self.changed
    }

    pub fn policy(&self) -> &ExtensionProfilePolicy {
        &self.policy
    }

    pub fn into_policy(self) -> ExtensionProfilePolicy {
        self.policy
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionProfilePolicyApplyError {
    RevisionConflict {
        expected: ExtensionProfilePolicyRevision,
        current: ExtensionProfilePolicyRevision,
    },
    LimitReached,
    RevisionExhausted,
    InvalidPolicy(ExtensionProfilePolicyError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionProfilePolicyError {
    InvalidSiteScope,
    TooManySiteDenials,
    DuplicateSiteDenial,
    AccountingOverflow,
    RetainedBytesExceeded,
}

impl fmt::Display for ExtensionProfilePolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid extension profile policy: {self:?}")
    }
}

impl Error for ExtensionProfilePolicyError {}

fn policy_digest(
    paused: bool,
    denied_sites: &[ExtensionSiteAccessScope],
) -> Result<ExtensionProfilePolicyDigest, ExtensionProfilePolicyError> {
    let count = u32::try_from(denied_sites.len())
        .map_err(|_| ExtensionProfilePolicyError::AccountingOverflow)?;
    let mut digest = Sha256::new();
    digest.update(PROFILE_POLICY_DIGEST_DOMAIN);
    digest.update([u8::from(paused)]);
    digest.update(count.to_be_bytes());
    for scope in denied_sites {
        let length = u32::try_from(scope.as_str().len())
            .map_err(|_| ExtensionProfilePolicyError::AccountingOverflow)?;
        digest.update(length.to_be_bytes());
        digest.update(scope.as_str().as_bytes());
    }
    Ok(ExtensionProfilePolicyDigest::from_bytes(
        digest.finalize().into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(value: &str) -> ExtensionSiteAccessScope {
        ExtensionSiteAccessScope::parse_exact(value).unwrap()
    }

    #[test]
    fn site_scope_is_exact_web_host_only() {
        for valid in ["https://example.com/*", "http://127.0.0.1/*"] {
            assert_eq!(site(valid).as_str(), valid);
        }
        assert_eq!(
            ExtensionSiteAccessScope::from_url(
                &Url::parse("https://Example.COM:8443/path?secret=value").unwrap()
            )
            .unwrap()
            .as_str(),
            "https://example.com/*"
        );
        for invalid in [
            "<all_urls>",
            "*://example.com/*",
            "https://*.example.com/*",
            "https://example.com:8443/*",
            "https://[::1]/*",
            "https://example.com/path/*",
            "file:///*",
        ] {
            assert_eq!(
                ExtensionSiteAccessScope::parse_exact(invalid),
                Err(ExtensionProfilePolicyError::InvalidSiteScope)
            );
        }
    }

    #[test]
    fn policy_is_canonical_digest_bound_and_revision_exact() {
        let initial = ExtensionProfilePolicy::initial();
        assert!(!initial.paused());
        assert!(initial.denied_sites().is_empty());
        let first = initial
            .apply(
                initial.revision(),
                ExtensionProfilePolicyMutation::SetSiteDenied {
                    scope: site("https://z.example/*"),
                    denied: true,
                },
            )
            .unwrap()
            .into_policy();
        let second = first
            .apply(
                first.revision(),
                ExtensionProfilePolicyMutation::SetSiteDenied {
                    scope: site("https://a.example/*"),
                    denied: true,
                },
            )
            .unwrap()
            .into_policy();
        assert_eq!(
            second
                .denied_sites()
                .iter()
                .map(ExtensionSiteAccessScope::as_str)
                .collect::<Vec<_>>(),
            ["https://a.example/*", "https://z.example/*"]
        );
        let reconstructed = ExtensionProfilePolicy::from_persisted(
            second.revision(),
            false,
            vec![site("https://z.example/*"), site("https://a.example/*")],
        )
        .unwrap();
        assert_eq!(reconstructed.digest(), second.digest());
        assert_eq!(reconstructed, second);
        let debug = format!("{second:?}");
        assert!(debug.contains("denied_site_count"));
        assert!(!debug.contains("a.example"));
        assert!(!debug.contains("z.example"));
    }

    #[test]
    fn no_op_does_not_spend_revision_and_limits_fail_closed() {
        let initial = ExtensionProfilePolicy::initial();
        let no_op = initial
            .apply(
                initial.revision(),
                ExtensionProfilePolicyMutation::SetPaused(false),
            )
            .unwrap();
        assert!(!no_op.changed());
        assert_eq!(no_op.policy().revision(), initial.revision());
        assert!(matches!(
            initial.apply(
                ExtensionProfilePolicyRevision::new(2).unwrap(),
                ExtensionProfilePolicyMutation::SetPaused(true),
            ),
            Err(ExtensionProfilePolicyApplyError::RevisionConflict { .. })
        ));

        let sites = (0..MAX_EXTENSION_SITE_DENIALS_PER_PROFILE)
            .map(|index| site(&format!("https://site-{index}.example/*")))
            .collect::<Vec<_>>();
        let full = ExtensionProfilePolicy::from_persisted(
            ExtensionProfilePolicyRevision::INITIAL,
            false,
            sites,
        )
        .unwrap();
        assert!(matches!(
            full.apply(
                full.revision(),
                ExtensionProfilePolicyMutation::SetSiteDenied {
                    scope: site("https://overflow.example/*"),
                    denied: true,
                },
            ),
            Err(ExtensionProfilePolicyApplyError::LimitReached)
        ));
        assert!(full.retained_bytes() <= MAX_EXTENSION_PROFILE_POLICY_RETAINED_BYTES);
    }
}
