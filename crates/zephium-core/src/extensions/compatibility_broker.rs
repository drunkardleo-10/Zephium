//! Bounded, versioned requests for Zephium-owned extension compatibility adapters.
//!
//! This protocol is deliberately not a generic native-messaging surface. A
//! reviewed package adapter may address one fixed application identifier and
//! select only a closed operation. Native code must still join the callback
//! to the exact published runtime and consume an operation-authority witness
//! before constructing [`ExtensionCompatibilityBrokerRequest`].

use std::fmt;

use super::{ExtensionRuntimeFingerprint, ExtensionRuntimeInstance};

/// The only application identifier accepted by Zephium's internal broker.
pub const EXTENSION_COMPATIBILITY_BROKER_APPLICATION_ID: &str = "app.zephium.extension-broker.v1";

/// Maximum encoded request accepted from an extension adapter.
pub const MAX_EXTENSION_COMPATIBILITY_BROKER_REQUEST_BYTES: usize = 128;
/// Maximum encoded response returned to an extension adapter.
pub const MAX_EXTENSION_COMPATIBILITY_BROKER_RESPONSE_BYTES: usize = 64 * 1024;
/// Maximum recent-history rows returned by one broker request.
pub const MAX_EXTENSION_COMPATIBILITY_HISTORY_RESULTS: u16 = 100;
/// Complete process-wide and per-profile pending callback ceilings.
pub const MAX_PENDING_EXTENSION_COMPATIBILITY_BROKER_REQUESTS: usize = 32;
pub const MAX_PENDING_EXTENSION_COMPATIBILITY_BROKER_REQUESTS_PER_PROFILE: usize = 8;

const RECENT_HISTORY_PREFIX: &str = "v1/history.recent/";

/// Closed compatibility operation whose product grant must be proven.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExtensionCompatibilityBrokerPurpose {
    RecentHistory,
}

/// Move-only proof that one exact published runtime holds the API authority
/// required by a compatibility operation.
#[must_use = "compatibility authority must be consumed by the native broker"]
pub struct ExtensionCompatibilityBrokerWitness {
    runtime: ExtensionRuntimeFingerprint,
    purpose: ExtensionCompatibilityBrokerPurpose,
}

impl ExtensionCompatibilityBrokerWitness {
    pub(super) const fn new(
        runtime: ExtensionRuntimeFingerprint,
        purpose: ExtensionCompatibilityBrokerPurpose,
    ) -> Self {
        Self { runtime, purpose }
    }

    /// Non-authorizing identity of the exact runtime bound by this witness.
    pub const fn runtime(&self) -> &ExtensionRuntimeFingerprint {
        &self.runtime
    }

    pub const fn runtime_instance(&self) -> ExtensionRuntimeInstance {
        self.runtime.instance()
    }

    pub const fn purpose(&self) -> ExtensionCompatibilityBrokerPurpose {
        self.purpose
    }
}

impl fmt::Debug for ExtensionCompatibilityBrokerWitness {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionCompatibilityBrokerWitness")
            .field("runtime", &"<redacted>")
            .field("purpose", &self.purpose)
            .finish()
    }
}

/// Process-local correlation identity allocated by the native broker.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExtensionCompatibilityBrokerRequestId(u64);

impl ExtensionCompatibilityBrokerRequestId {
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 {
            None
        } else {
            Some(Self(value))
        }
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Validated request semantics. JavaScript supplies only the bounded wire
/// representation; the native adapter allocates the correlation identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionCompatibilityBrokerOperation {
    RecentHistory { limit: u16 },
}

impl ExtensionCompatibilityBrokerOperation {
    pub const fn purpose(self) -> ExtensionCompatibilityBrokerPurpose {
        match self {
            Self::RecentHistory { .. } => ExtensionCompatibilityBrokerPurpose::RecentHistory,
        }
    }

    /// Parses the allocation-free v1 request vocabulary.
    pub fn parse_wire(value: &str) -> Result<Self, ExtensionCompatibilityBrokerRequestError> {
        if value.is_empty() || value.len() > MAX_EXTENSION_COMPATIBILITY_BROKER_REQUEST_BYTES {
            return Err(ExtensionCompatibilityBrokerRequestError::InvalidWireRequest);
        }
        let Some(limit) = value.strip_prefix(RECENT_HISTORY_PREFIX) else {
            return Err(ExtensionCompatibilityBrokerRequestError::UnsupportedOperation);
        };
        if limit.is_empty()
            || (limit.len() > 1 && limit.starts_with('0'))
            || !limit.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(ExtensionCompatibilityBrokerRequestError::InvalidWireRequest);
        }
        let limit = limit
            .parse::<u16>()
            .map_err(|_| ExtensionCompatibilityBrokerRequestError::InvalidWireRequest)?;
        if limit == 0 || limit > MAX_EXTENSION_COMPATIBILITY_HISTORY_RESULTS {
            return Err(ExtensionCompatibilityBrokerRequestError::InvalidLimit);
        }
        Ok(Self::RecentHistory { limit })
    }
}

/// One authority-bound request delivered to Shell.
#[must_use = "the compatibility request must be settled exactly once"]
#[derive(Clone, Eq, PartialEq)]
pub struct ExtensionCompatibilityBrokerRequest {
    runtime: ExtensionRuntimeInstance,
    id: ExtensionCompatibilityBrokerRequestId,
    operation: ExtensionCompatibilityBrokerOperation,
}

impl ExtensionCompatibilityBrokerRequest {
    /// Consumes the exact operation witness. Bare runtime identity and parsed
    /// JavaScript input cannot construct an authorized Shell request.
    pub fn authorize(
        id: ExtensionCompatibilityBrokerRequestId,
        operation: ExtensionCompatibilityBrokerOperation,
        witness: ExtensionCompatibilityBrokerWitness,
    ) -> Result<Self, ExtensionCompatibilityBrokerRequestError> {
        if witness.purpose() != operation.purpose() {
            return Err(ExtensionCompatibilityBrokerRequestError::AuthorityMismatch);
        }
        Ok(Self {
            runtime: witness.runtime_instance(),
            id,
            operation,
        })
    }

    pub const fn runtime(&self) -> ExtensionRuntimeInstance {
        self.runtime
    }

    pub const fn id(&self) -> ExtensionCompatibilityBrokerRequestId {
        self.id
    }

    pub const fn operation(&self) -> ExtensionCompatibilityBrokerOperation {
        self.operation
    }
}

impl fmt::Debug for ExtensionCompatibilityBrokerRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionCompatibilityBrokerRequest")
            .field("runtime", &"<redacted>")
            .field("id", &self.id)
            .field("operation", &self.operation)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionCompatibilityBrokerRequestError {
    InvalidWireRequest,
    UnsupportedOperation,
    InvalidLimit,
    AuthorityMismatch,
}

/// One sanitized history row returned by the browser-owned Store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionCompatibilityHistoryEntry {
    pub url: String,
    pub title: String,
    pub last_visit: i64,
}

/// Successful result for one exact closed operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionCompatibilityBrokerResult {
    RecentHistory(Box<[ExtensionCompatibilityHistoryEntry]>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionCompatibilityBrokerRejection {
    InvalidContext,
    InvalidRequest,
    Unauthorized,
    Unsupported,
    CapacityExceeded,
    BackendUnavailable,
    ResponseTooLarge,
    ShuttingDown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionCompatibilityBrokerSettlement {
    Applied(ExtensionCompatibilityBrokerResult),
    Rejected(ExtensionCompatibilityBrokerRejection),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_parser_accepts_only_canonical_bounded_requests() {
        assert_eq!(
            ExtensionCompatibilityBrokerOperation::parse_wire("v1/history.recent/100"),
            Ok(ExtensionCompatibilityBrokerOperation::RecentHistory { limit: 100 })
        );
        for invalid in [
            "",
            "v1/history.recent/0",
            "v1/history.recent/01",
            "v1/history.recent/101",
            "v1/history.recent/-1",
            "v1/history.search/1",
            "v2/history.recent/1",
        ] {
            assert!(ExtensionCompatibilityBrokerOperation::parse_wire(invalid).is_err());
        }
        assert!(ExtensionCompatibilityBrokerOperation::parse_wire(
            &"x".repeat(MAX_EXTENSION_COMPATIBILITY_BROKER_REQUEST_BYTES + 1)
        )
        .is_err());
    }
}
