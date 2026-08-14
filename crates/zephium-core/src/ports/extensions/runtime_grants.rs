//! Bounded, non-authorizing runtime optional-permission requests.

use std::error::Error;
use std::fmt;
use std::mem::size_of;
use std::num::NonZeroU64;
use std::sync::Arc;

use crate::extensions::{
    ApiPermissionName, ExtensionGrantRevision, ExtensionNativeOwnershipKey,
    ExtensionRuntimeGeneration, ExtensionRuntimeInstance, MAX_EXTENSION_API_PERMISSIONS,
    MAX_EXTENSION_API_PERMISSION_NAME_BYTES, MAX_EXTENSION_HOST_PERMISSION_PATTERNS,
};
use crate::injection::{MatchPattern, MAX_MATCH_PATTERN_RETAINED_BUDGET_BYTES};

/// Maximum retained bytes for one runtime-originated optional grant request.
pub const MAX_EXTENSION_RUNTIME_GRANT_REQUEST_RETAINED_BYTES: usize =
    size_of::<ExtensionRuntimeGrantRequest>()
        + MAX_EXTENSION_API_PERMISSIONS
            * (size_of::<ApiPermissionName>() + MAX_EXTENSION_API_PERMISSION_NAME_BYTES)
        + MAX_EXTENSION_HOST_PERMISSION_PATTERNS
            * (size_of::<MatchPattern>() + MAX_MATCH_PATTERN_RETAINED_BUDGET_BYTES)
        + 256;

/// Process-local correlation identity for one native optional-grant prompt.
///
/// Values never wrap within a broker. Exhaustion permanently closes new
/// admission for that broker so a delayed Shell settlement cannot alias a
/// newer WebKit completion cohort.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExtensionRuntimeGrantRequestId(NonZeroU64);

impl ExtensionRuntimeGrantRequestId {
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

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

/// Bounded, non-authorizing native request delivered to the application actor.
///
/// The runtime identity is only a stale-callback fence. Approval still has to
/// pass through the serialized extension service, which reauthenticates the
/// installed manifest and current durable grant cohort before changing any
/// authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionRuntimeGrantPrompt {
    id: ExtensionRuntimeGrantRequestId,
    runtime: ExtensionRuntimeInstance,
    key: ExtensionNativeOwnershipKey,
    extension_name: Arc<str>,
    request: ExtensionRuntimeGrantRequest,
}

impl ExtensionRuntimeGrantPrompt {
    pub fn new(
        id: ExtensionRuntimeGrantRequestId,
        runtime: ExtensionRuntimeInstance,
        key: ExtensionNativeOwnershipKey,
        extension_name: impl Into<Arc<str>>,
        request: ExtensionRuntimeGrantRequest,
    ) -> Result<Self, ExtensionRuntimeGrantPromptError> {
        if key.profile() != runtime.profile() || key.install_id() != runtime.install_id() {
            return Err(ExtensionRuntimeGrantPromptError::IdentityMismatch);
        }
        let extension_name = extension_name.into();
        if super::management::validate_display_text(&extension_name, 75, true).is_err() {
            return Err(ExtensionRuntimeGrantPromptError::InvalidDisplayName);
        }
        Ok(Self {
            id,
            runtime,
            key,
            extension_name,
            request,
        })
    }

    pub const fn id(&self) -> ExtensionRuntimeGrantRequestId {
        self.id
    }

    pub const fn runtime(&self) -> ExtensionRuntimeInstance {
        self.runtime
    }

    pub const fn key(&self) -> ExtensionNativeOwnershipKey {
        self.key
    }

    pub fn extension_name(&self) -> &str {
        &self.extension_name
    }

    pub const fn request(&self) -> &ExtensionRuntimeGrantRequest {
        &self.request
    }

    pub fn into_request(self) -> ExtensionRuntimeGrantRequest {
        self.request
    }

    pub fn retained_bytes(&self) -> usize {
        size_of::<Self>() + self.extension_name.len() + self.request.retained_bytes()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionRuntimeGrantPromptError {
    IdentityMismatch,
    InvalidDisplayName,
}

/// Trusted Shell settlement for one retained native permission cohort.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionRuntimeGrantPromptSettlement {
    /// The serialized service proved the complete requested cohort durable
    /// and rebound the same live native runtime before this settlement.
    Granted,
    /// The user or product policy denied the complete cohort. No subset may
    /// be retained for a combined API-and-host request.
    Denied,
    /// The prompt or authority transaction could not be completed safely.
    /// WebKit observes a denial, while diagnostics retain the distinction.
    Unavailable,
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
    /// The requesting context retained its exact generation while its
    /// published operation authority was rebound in place.
    Active(ExtensionRuntimeGeneration),
}

/// Exact result of one serialized live-runtime optional-grant transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionRuntimeGrantOutcome {
    /// One atomic grant revision committed and the exact live runtime was
    /// rebound without replacing its native owner or generation.
    Granted {
        /// The new durable grant revision.
        revision: ExtensionGrantRevision,
        /// Truthful post-commit state of the requesting context.
        runtime: ExtensionRuntimeGrantRuntimeState,
    },
    /// Every requested target was already granted; no authority changed.
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
    /// A retryable dependency or deadline prevented admission before any
    /// durable grant change could have committed.
    Unavailable,
    /// The durable write or its exact journal/host publication settlement
    /// cannot yet be observed; a later serialized request must reconcile it.
    OutcomeUnknown,
    /// Runtime, Store, repository, or protocol integrity failed closed.
    FailedClosed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{ExtensionInstallId, ProfileId};

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

    #[test]
    fn native_prompt_retains_exact_runtime_request_and_nonzero_identity() {
        assert!(ExtensionRuntimeGrantRequestId::new(0).is_none());
        let id = ExtensionRuntimeGrantRequestId::new(7).unwrap();
        let runtime = ExtensionRuntimeInstance::new(
            ProfileId::from(11),
            ExtensionInstallId::from(12),
            ExtensionRuntimeGeneration::new(13).unwrap(),
        );
        let key = ExtensionNativeOwnershipKey::new(
            runtime.profile(),
            runtime.install_id(),
            crate::extensions::ExtensionGrantBrowsingContext::Regular,
        );
        let request = ExtensionRuntimeGrantRequest::new(
            vec![ApiPermissionName::parse_exact("clipboardWrite").unwrap()],
            vec![MatchPattern::parse("https://example.invalid/*").unwrap()],
        )
        .unwrap();
        let expected = request.clone();
        let prompt =
            ExtensionRuntimeGrantPrompt::new(id, runtime, key, "Fixture extension", request)
                .unwrap();

        assert_eq!(prompt.id(), id);
        assert_eq!(prompt.runtime(), runtime);
        assert_eq!(prompt.key(), key);
        assert_eq!(prompt.extension_name(), "Fixture extension");
        assert_eq!(prompt.request(), &expected);
        assert!(prompt.retained_bytes() >= expected.retained_bytes());
        assert_eq!(prompt.into_request(), expected);
    }

    #[test]
    fn native_prompt_rejects_cross_owner_or_unsafe_display_identity() {
        let id = ExtensionRuntimeGrantRequestId::new(1).unwrap();
        let runtime = ExtensionRuntimeInstance::new(
            ProfileId::from(1),
            ExtensionInstallId::from(2),
            ExtensionRuntimeGeneration::INITIAL,
        );
        let request = || {
            ExtensionRuntimeGrantRequest::new(
                vec![ApiPermissionName::parse_exact("tabs").unwrap()],
                Vec::new(),
            )
            .unwrap()
        };
        let wrong = ExtensionNativeOwnershipKey::new(
            ProfileId::from(9),
            runtime.install_id(),
            crate::extensions::ExtensionGrantBrowsingContext::Regular,
        );
        assert_eq!(
            ExtensionRuntimeGrantPrompt::new(id, runtime, wrong, "Extension", request()),
            Err(ExtensionRuntimeGrantPromptError::IdentityMismatch)
        );
        let exact = ExtensionNativeOwnershipKey::new(
            runtime.profile(),
            runtime.install_id(),
            crate::extensions::ExtensionGrantBrowsingContext::Regular,
        );
        for invalid in ["bad\nname", "...", "Name\u{202e}txt"] {
            assert_eq!(
                ExtensionRuntimeGrantPrompt::new(id, runtime, exact, invalid, request()),
                Err(ExtensionRuntimeGrantPromptError::InvalidDisplayName)
            );
        }
    }
}
