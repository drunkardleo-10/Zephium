//! Bounded, versioned requests for Zephium-owned extension compatibility adapters.
//!
//! This protocol is deliberately not a generic native-messaging surface. A
//! reviewed package adapter may address one fixed application identifier and
//! select only a closed operation. Native code must still join the callback
//! to the exact published runtime and consume an operation-authority witness
//! before constructing [`ExtensionCompatibilityBrokerRequest`].

use std::fmt;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;

use super::{ExtensionRuntimeFingerprint, ExtensionRuntimeInstance};

/// The only application identifier accepted by Zephium's internal broker.
pub const EXTENSION_COMPATIBILITY_BROKER_APPLICATION_ID: &str = "app.zephium.extension-broker.v1";

/// Maximum encoded request accepted from an extension adapter.
pub const MAX_EXTENSION_COMPATIBILITY_SEARCH_QUERY_BYTES: usize = 1024;
pub const MAX_EXTENSION_COMPATIBILITY_BROKER_REQUEST_BYTES: usize = 1536;
/// Maximum encoded response returned to an extension adapter.
pub const MAX_EXTENSION_COMPATIBILITY_BROKER_RESPONSE_BYTES: usize = 64 * 1024;
/// Maximum recent-history rows returned by one broker request.
pub const MAX_EXTENSION_COMPATIBILITY_HISTORY_RESULTS: u16 = 100;
/// Complete process-wide and per-profile pending callback ceilings.
pub const MAX_PENDING_EXTENSION_COMPATIBILITY_BROKER_REQUESTS: usize = 32;
pub const MAX_PENDING_EXTENSION_COMPATIBILITY_BROKER_REQUESTS_PER_PROFILE: usize = 8;

const RECENT_HISTORY_PREFIX: &str = "v1/history.recent/";
const DEFAULT_SEARCH_CURRENT_PREFIX: &str = "v1/search.default/current/";
const DEFAULT_SEARCH_NEW_PREFIX: &str = "v1/search.default/new/";
const RESTORE_RECENT_SESSION: &str = "v1/sessions.restore/recent";
const OPEN_OPTIONS_PAGE: &str = "v1/options.open";

/// Closed compatibility operation whose product grant must be proven.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExtensionCompatibilityBrokerPurpose {
    RecentHistory,
    DefaultSearch,
    RestoreRecentSession,
    OpenOptionsPage,
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
pub enum ExtensionCompatibilitySearchDisposition {
    CurrentTab,
    NewTab,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionCompatibilityBrokerOperation {
    RecentHistory {
        limit: u16,
    },
    DefaultSearch {
        disposition: ExtensionCompatibilitySearchDisposition,
        query: Box<str>,
    },
    RestoreRecentSession,
    OpenOptionsPage,
}

impl ExtensionCompatibilityBrokerOperation {
    pub const fn purpose(&self) -> ExtensionCompatibilityBrokerPurpose {
        match self {
            Self::RecentHistory { .. } => ExtensionCompatibilityBrokerPurpose::RecentHistory,
            Self::DefaultSearch { .. } => ExtensionCompatibilityBrokerPurpose::DefaultSearch,
            Self::RestoreRecentSession => ExtensionCompatibilityBrokerPurpose::RestoreRecentSession,
            Self::OpenOptionsPage => ExtensionCompatibilityBrokerPurpose::OpenOptionsPage,
        }
    }

    /// Parses the allocation-free v1 request vocabulary.
    pub fn parse_wire(value: &str) -> Result<Self, ExtensionCompatibilityBrokerRequestError> {
        if value.is_empty() || value.len() > MAX_EXTENSION_COMPATIBILITY_BROKER_REQUEST_BYTES {
            return Err(ExtensionCompatibilityBrokerRequestError::InvalidWireRequest);
        }
        if value == RESTORE_RECENT_SESSION {
            return Ok(Self::RestoreRecentSession);
        }
        if value == OPEN_OPTIONS_PAGE {
            return Ok(Self::OpenOptionsPage);
        }
        if let Some(encoded) = value.strip_prefix(DEFAULT_SEARCH_CURRENT_PREFIX) {
            return parse_search(encoded, ExtensionCompatibilitySearchDisposition::CurrentTab);
        }
        if let Some(encoded) = value.strip_prefix(DEFAULT_SEARCH_NEW_PREFIX) {
            return parse_search(encoded, ExtensionCompatibilitySearchDisposition::NewTab);
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

fn parse_search(
    encoded: &str,
    disposition: ExtensionCompatibilitySearchDisposition,
) -> Result<ExtensionCompatibilityBrokerOperation, ExtensionCompatibilityBrokerRequestError> {
    if encoded.is_empty()
        || encoded.contains('=')
        || !encoded
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ExtensionCompatibilityBrokerRequestError::InvalidWireRequest);
    }
    let mut decoded = [0_u8; MAX_EXTENSION_COMPATIBILITY_SEARCH_QUERY_BYTES];
    let length = URL_SAFE_NO_PAD
        .decode_slice(encoded, &mut decoded)
        .map_err(|_| ExtensionCompatibilityBrokerRequestError::InvalidWireRequest)?;
    let query = std::str::from_utf8(&decoded[..length])
        .map_err(|_| ExtensionCompatibilityBrokerRequestError::InvalidWireRequest)?;
    if query.trim().is_empty()
        || query.chars().any(char::is_control)
        || URL_SAFE_NO_PAD.encode(query.as_bytes()) != encoded
    {
        return Err(ExtensionCompatibilityBrokerRequestError::InvalidWireRequest);
    }
    Ok(ExtensionCompatibilityBrokerOperation::DefaultSearch {
        disposition,
        query: query.into(),
    })
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

    pub const fn operation(&self) -> &ExtensionCompatibilityBrokerOperation {
        &self.operation
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
    DefaultSearch {
        opened: bool,
    },
    RecentSessionRestore {
        restored: bool,
    },
    /// Shell authorized the exact runtime/context request. Native settlement
    /// still determines whether the options surface was actually presented.
    OptionsPageOpenAuthorized,
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
        let encoded = URL_SAFE_NO_PAD.encode("hello π".as_bytes());
        assert_eq!(
            ExtensionCompatibilityBrokerOperation::parse_wire(&format!(
                "{DEFAULT_SEARCH_CURRENT_PREFIX}{encoded}"
            )),
            Ok(ExtensionCompatibilityBrokerOperation::DefaultSearch {
                disposition: ExtensionCompatibilitySearchDisposition::CurrentTab,
                query: "hello π".into(),
            })
        );
        assert_eq!(
            ExtensionCompatibilityBrokerOperation::parse_wire(RESTORE_RECENT_SESSION),
            Ok(ExtensionCompatibilityBrokerOperation::RestoreRecentSession)
        );
        assert_eq!(
            ExtensionCompatibilityBrokerOperation::parse_wire(OPEN_OPTIONS_PAGE),
            Ok(ExtensionCompatibilityBrokerOperation::OpenOptionsPage)
        );
        let maximum = "a".repeat(MAX_EXTENSION_COMPATIBILITY_SEARCH_QUERY_BYTES);
        let maximum_wire = format!(
            "{DEFAULT_SEARCH_NEW_PREFIX}{}",
            URL_SAFE_NO_PAD.encode(maximum.as_bytes())
        );
        assert!(matches!(
            ExtensionCompatibilityBrokerOperation::parse_wire(&maximum_wire),
            Ok(ExtensionCompatibilityBrokerOperation::DefaultSearch {
                disposition: ExtensionCompatibilitySearchDisposition::NewTab,
                query,
            }) if query.as_ref() == maximum
        ));
        let oversized = "a".repeat(MAX_EXTENSION_COMPATIBILITY_SEARCH_QUERY_BYTES + 1);
        assert!(ExtensionCompatibilityBrokerOperation::parse_wire(&format!(
            "{DEFAULT_SEARCH_CURRENT_PREFIX}{}",
            URL_SAFE_NO_PAD.encode(oversized.as_bytes())
        ))
        .is_err());
        assert!(ExtensionCompatibilityBrokerOperation::parse_wire(&format!(
            "{DEFAULT_SEARCH_CURRENT_PREFIX}{}",
            URL_SAFE_NO_PAD.encode(b"line\nbreak")
        ))
        .is_err());
        for invalid in [
            "",
            "v1/history.recent/0",
            "v1/history.recent/01",
            "v1/history.recent/101",
            "v1/history.recent/-1",
            "v1/history.search/1",
            "v2/history.recent/1",
            "v1/search.default/current/",
            "v1/search.default/current/%%%%",
            "v1/search.default/current/SGVsbG8=",
            "v1/search.default/other/SGVsbG8",
            "v1/sessions.restore/other",
        ] {
            assert!(ExtensionCompatibilityBrokerOperation::parse_wire(invalid).is_err());
        }
        assert!(ExtensionCompatibilityBrokerOperation::parse_wire(
            &"x".repeat(MAX_EXTENSION_COMPATIBILITY_BROKER_REQUEST_BYTES + 1)
        )
        .is_err());
    }
}
