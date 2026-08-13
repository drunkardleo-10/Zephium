//! Bounded, non-authorizing runtime optional-permission requests.

use std::error::Error;
use std::fmt;
use std::mem::size_of;

use crate::extensions::{
    ApiPermissionName, ExtensionGrantRevision, ExtensionRuntimeGeneration,
    MAX_EXTENSION_API_PERMISSIONS, MAX_EXTENSION_API_PERMISSION_NAME_BYTES,
    MAX_EXTENSION_HOST_PERMISSION_PATTERNS,
};
use crate::injection::{MatchPattern, MAX_MATCH_PATTERN_RETAINED_BUDGET_BYTES};

use super::ExtensionActivationPendingReason;

/// Maximum retained bytes for one runtime-originated optional grant request.
pub const MAX_EXTENSION_RUNTIME_GRANT_REQUEST_RETAINED_BYTES: usize =
    size_of::<ExtensionRuntimeGrantRequest>()
        + MAX_EXTENSION_API_PERMISSIONS
            * (size_of::<ApiPermissionName>() + MAX_EXTENSION_API_PERMISSION_NAME_BYTES)
        + MAX_EXTENSION_HOST_PERMISSION_PATTERNS
            * (size_of::<MatchPattern>() + MAX_MATCH_PATTERN_RETAINED_BUDGET_BYTES)
        + 256;

/// Canonical optional API and host declarations requested by one live runtime.
///
/// This value is selector data only. The extension service must bind it to an
/// exact live runtime generation, reauthenticate the installed manifest, and
/// prove every target is optional before constructing durable grant authority.
/// File/private toggles and revocation are intentionally absent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionRuntimeGrantRequest {
    api: Box<[ApiPermissionName]>,
    hosts: Box<[MatchPattern]>,
    retained_bytes: usize,
}

impl ExtensionRuntimeGrantRequest {
    /// Canonicalizes one bounded request and rejects empty or duplicate input.
    pub fn new(
        mut api: Vec<ApiPermissionName>,
        mut hosts: Vec<MatchPattern>,
    ) -> Result<Self, ExtensionRuntimeGrantRequestError> {
        if api.is_empty() && hosts.is_empty() {
            return Err(ExtensionRuntimeGrantRequestError::Empty);
        }
        if api.len() > MAX_EXTENSION_API_PERMISSIONS
            || hosts.len() > MAX_EXTENSION_HOST_PERMISSION_PATTERNS
        {
            return Err(ExtensionRuntimeGrantRequestError::TooManyTargets);
        }
        api.sort_unstable();
        hosts.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
        if api.windows(2).any(|pair| pair[0] == pair[1])
            || hosts
                .windows(2)
                .any(|pair| pair[0].as_str() == pair[1].as_str())
        {
            return Err(ExtensionRuntimeGrantRequestError::DuplicateTarget);
        }
        let retained_bytes = api
            .iter()
            .try_fold(
                size_of::<Self>()
                    .checked_add(api.len().saturating_mul(size_of::<ApiPermissionName>()))
                    .and_then(|bytes| {
                        bytes.checked_add(hosts.len().saturating_mul(size_of::<MatchPattern>()))
                    })
                    .ok_or(ExtensionRuntimeGrantRequestError::AccountingOverflow)?,
                |bytes, name| bytes.checked_add(name.len()),
            )
            .and_then(|bytes| {
                hosts.iter().try_fold(bytes, |bytes, pattern| {
                    bytes.checked_add(pattern.retained_budget_bytes())
                })
            })
            .ok_or(ExtensionRuntimeGrantRequestError::AccountingOverflow)?;
        if retained_bytes > MAX_EXTENSION_RUNTIME_GRANT_REQUEST_RETAINED_BYTES {
            return Err(ExtensionRuntimeGrantRequestError::RetainedBytesExceeded);
        }
        Ok(Self {
            api: api.into_boxed_slice(),
            hosts: hosts.into_boxed_slice(),
            retained_bytes,
        })
    }

    /// Returns canonical requested optional API names.
    pub fn api(&self) -> &[ApiPermissionName] {
        &self.api
    }

    /// Returns canonical requested optional host patterns.
    pub fn hosts(&self) -> &[MatchPattern] {
        &self.hosts
    }

    /// Returns conservative logical retained bytes for mailbox admission.
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

/// Stable construction refusal for a runtime grant request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionRuntimeGrantRequestError {
    /// No API or host target was supplied.
    Empty,
    /// A target class exceeded its manifest grammar ceiling.
    TooManyTargets,
    /// One exact API name or canonical host pattern appeared more than once.
    DuplicateTarget,
    /// Retained-memory accounting overflowed.
    AccountingOverflow,
    /// The request exceeded its exported retained-memory ceiling.
    RetainedBytesExceeded,
}

impl fmt::Display for ExtensionRuntimeGrantRequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("runtime grant request is empty"),
            Self::TooManyTargets => {
                formatter.write_str("runtime grant request has too many targets")
            }
            Self::DuplicateTarget => {
                formatter.write_str("runtime grant request contains a duplicate target")
            }
            Self::AccountingOverflow => {
                formatter.write_str("runtime grant request accounting overflowed")
            }
            Self::RetainedBytesExceeded => {
                formatter.write_str("runtime grant request exceeds its retained-byte ceiling")
            }
        }
    }
}

impl Error for ExtensionRuntimeGrantRequestError {}

/// Runtime state after a durable optional-grant change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionRuntimeGrantRuntimeState {
    /// The requesting context was reactivated at a new exact generation.
    Active(ExtensionRuntimeGeneration),
    /// Durable grants changed, but runtime reactivation did not settle active.
    PendingActivation(ExtensionActivationPendingReason),
}

/// Exact result of one serialized live-runtime optional-grant transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionRuntimeGrantOutcome {
    /// One atomic grant revision committed after native retirement.
    Granted {
        /// The new durable grant revision.
        revision: ExtensionGrantRevision,
        /// Truthful post-commit state of the requesting context.
        runtime: ExtensionRuntimeGrantRuntimeState,
    },
    /// Every requested target was already granted; no runtime was retired.
    AlreadyGranted {
        /// The unchanged durable grant revision.
        revision: ExtensionGrantRevision,
        /// The still-live requesting runtime generation.
        generation: ExtensionRuntimeGeneration,
    },
    /// The addressed runtime generation or durable cohort was stale.
    Conflict,
    /// A target was not an optional declaration in the authenticated manifest.
    Rejected,
    /// A retryable dependency or deadline prevented a definite change.
    Unavailable,
    /// Store admitted the write but its commit outcome is unknown.
    OutcomeUnknown,
    /// Runtime, Store, repository, or protocol integrity failed closed.
    FailedClosed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_grant_request_is_canonical_bounded_and_distinct() {
        let request = ExtensionRuntimeGrantRequest::new(
            vec![
                ApiPermissionName::parse_exact("tabs").unwrap(),
                ApiPermissionName::parse_exact("storage").unwrap(),
            ],
            vec![
                MatchPattern::parse("https://z.example/*").unwrap(),
                MatchPattern::parse("https://a.example/*").unwrap(),
            ],
        )
        .unwrap();
        assert_eq!(
            request
                .api()
                .iter()
                .map(ApiPermissionName::as_str)
                .collect::<Vec<_>>(),
            ["storage", "tabs"]
        );
        assert_eq!(
            request
                .hosts()
                .iter()
                .map(MatchPattern::as_str)
                .collect::<Vec<_>>(),
            ["https://a.example/*", "https://z.example/*"]
        );
        assert!(request.retained_bytes() <= MAX_EXTENSION_RUNTIME_GRANT_REQUEST_RETAINED_BYTES);

        assert_eq!(
            ExtensionRuntimeGrantRequest::new(Vec::new(), Vec::new()),
            Err(ExtensionRuntimeGrantRequestError::Empty)
        );
        assert_eq!(
            ExtensionRuntimeGrantRequest::new(
                vec![
                    ApiPermissionName::parse_exact("tabs").unwrap(),
                    ApiPermissionName::parse_exact("tabs").unwrap(),
                ],
                Vec::new(),
            ),
            Err(ExtensionRuntimeGrantRequestError::DuplicateTarget)
        );
        assert_eq!(
            ExtensionRuntimeGrantRequest::new(
                Vec::new(),
                vec![
                    MatchPattern::parse("https://example.com/*").unwrap(),
                    MatchPattern::parse("https://example.com/*").unwrap(),
                ],
            ),
            Err(ExtensionRuntimeGrantRequestError::DuplicateTarget)
        );
    }
}
