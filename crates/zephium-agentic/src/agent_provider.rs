//! Provider-neutral, content-redacted model transport contracts.
//!
//! This module contains no HTTP client, credential, task, timer, retry loop,
//! browser authority, or model-selected native operation. It defines the
//! bounded identity, usage, failure, and stream-budget vocabulary shared by
//! the first provider codecs. Vendor wire data remains private to those
//! codecs and can cross the public boundary only as closed typed values.

mod openai;
mod request;
mod sse;
mod tool;

use std::fmt;

use thiserror::Error;

use crate::{
    AgentActiveModelCall, AgentModelCallAdmission, AgentModelCallId, AgentModelCallRequest,
    AgentPlanLeaseId, AgentPlanNodeId, AgentRunManifestId, SemanticTokenMeasurement,
    SemanticTokenizerRevision, MAX_AGENT_RUN_MODEL_TOKENS,
};

pub use openai::OpenAiResponsesStreamDecoder;
pub use request::{
    AgentPreparedObservationRequest, AgentPreparedReadRequest, AgentProviderEndpoint,
    AgentProviderInputOutcome, AgentProviderObjective, AgentProviderObjectiveError,
    AgentProviderRequest, AgentProviderRequestError, AgentProviderRequestSettlement,
    MAX_AGENT_BROWSER_NAVIGATION_URL_BYTES, MAX_AGENT_PROVIDER_OBJECTIVE_BYTES,
    MAX_AGENT_PROVIDER_OBJECTIVE_TOKENS, MAX_AGENT_PROVIDER_REQUEST_BYTES,
};
pub use tool::{
    AgentBrowserActProposal, AgentBrowserHumanReason, AgentBrowserScopeProposal,
    AgentBrowserSemanticQuery, AgentBrowserToolCall, AgentBrowserToolCallId,
    AgentBrowserToolContractError, AgentBrowserToolKind, AgentBrowserToolProposal,
    MAX_AGENT_BROWSER_SEMANTIC_QUERY_BYTES, MAX_AGENT_PROVIDER_TOOL_CALL_ID_BYTES,
};

/// Maximum bytes in one pinned provider model revision.
pub const MAX_AGENT_PROVIDER_MODEL_REVISION_BYTES: usize = 96;
/// Maximum decoded UTF-8 bytes in one SSE line.
pub const MAX_AGENT_PROVIDER_SSE_LINE_BYTES: usize = 512 * 1024;
/// Maximum decoded UTF-8 bytes in one complete SSE event payload.
pub const MAX_AGENT_PROVIDER_SSE_EVENT_BYTES: usize = 512 * 1024;
/// Maximum aggregate provider response-body bytes accepted for one model call.
pub const MAX_AGENT_PROVIDER_STREAM_WIRE_BYTES: u32 = 2 * 1024 * 1024;
/// Maximum provider SSE events accepted for one model call.
pub const MAX_AGENT_PROVIDER_STREAM_EVENTS: u32 = 4_096;
/// Maximum model-authored plain-text bytes accepted for one model call.
pub const MAX_AGENT_PROVIDER_OUTPUT_TEXT_BYTES: u32 = 64 * 1024;
/// Maximum client tool calls accepted from one model response.
pub const MAX_AGENT_PROVIDER_TOOL_CALLS: u8 = 8;
/// Maximum aggregate tool-argument bytes accepted for one model response.
pub const MAX_AGENT_PROVIDER_TOOL_ARGUMENT_BYTES: u32 = 32 * 1024;
/// Maximum provider-requested retry delay surfaced to policy.
pub const MAX_AGENT_PROVIDER_RETRY_AFTER_MILLIS: u64 = 24 * 60 * 60 * 1_000;

/// First provider protocols qualified through the shared adapter contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentProviderKind {
    /// OpenAI Responses API.
    OpenAiResponses,
    /// Anthropic Messages API.
    AnthropicMessages,
}

/// Bounded exact provider model revision selected before request admission.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct AgentProviderModelRevision(String);

impl AgentProviderModelRevision {
    /// Accepts an ASCII model revision rather than a URL, path, or free text.
    pub fn try_new(value: String) -> Result<Self, AgentProviderContractError> {
        if value.is_empty()
            || value.len() > MAX_AGENT_PROVIDER_MODEL_REVISION_BYTES
            || value.contains("://")
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
            })
        {
            return Err(AgentProviderContractError::ModelRevision);
        }
        Ok(Self(value))
    }

    /// Exact value for a fixed provider request body.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AgentProviderModelRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderModelRevision")
            .field("bytes", &self.0.len())
            .field("value", &"[redacted]")
            .finish()
    }
}

/// Content-free correlation copied from one exact policy authority.
///
/// This value is intentionally cloneable for callbacks and metrics. It omits
/// the policy admission guard and therefore cannot commit input, spend a
/// reservation, authorize a retry, or settle a call.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AgentProviderCallIdentity {
    manifest: AgentRunManifestId,
    call: AgentModelCallId,
    lease: AgentPlanLeaseId,
    node: AgentPlanNodeId,
}

impl AgentProviderCallIdentity {
    /// Projects non-authorizing correlation from a prepared policy admission.
    pub fn from_admission(admission: &AgentModelCallAdmission) -> Self {
        Self {
            manifest: admission.manifest(),
            call: admission.id(),
            lease: admission.lease(),
            node: admission.node(),
        }
    }

    /// Projects non-authorizing correlation from a committed model call.
    pub fn from_active(active: &AgentActiveModelCall) -> Self {
        Self {
            manifest: active.manifest(),
            call: active.id(),
            lease: active.lease(),
            node: active.node(),
        }
    }

    /// Exact immutable run-manifest revision.
    pub const fn manifest(self) -> AgentRunManifestId {
        self.manifest
    }

    /// Exact monotonic model-call identity.
    pub const fn call(self) -> AgentModelCallId {
        self.call
    }

    /// Exact mutable plan lease reserving the call.
    pub const fn lease(self) -> AgentPlanLeaseId {
        self.lease
    }

    /// Exact responsible plan node.
    pub const fn node(self) -> AgentPlanNodeId {
        self.node
    }

    /// Whether this correlation still names the supplied committed authority.
    pub fn matches_active(self, active: &AgentActiveModelCall) -> bool {
        self == Self::from_active(active)
    }
}

impl fmt::Debug for AgentProviderCallIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderCallIdentity")
            .field("manifest", &self.manifest)
            .field("call", &self.call)
            .field("lease", &self.lease)
            .field("node", &self.node)
            .finish()
    }
}

/// Per-call stream ceilings, always bounded by process-wide hard limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentProviderStreamBudget {
    max_wire_bytes: u32,
    max_events: u32,
    max_output_text_bytes: u32,
    max_tool_calls: u8,
    max_tool_argument_bytes: u32,
}

impl AgentProviderStreamBudget {
    /// Conservative default for one browser-agent model turn.
    pub const STANDARD: Self = Self {
        max_wire_bytes: 1024 * 1024,
        max_events: 2_048,
        max_output_text_bytes: 32 * 1024,
        max_tool_calls: MAX_AGENT_PROVIDER_TOOL_CALLS,
        max_tool_argument_bytes: 16 * 1024,
    };

    /// Validates nonzero call ceilings under every process-wide maximum.
    pub const fn try_new(
        max_wire_bytes: u32,
        max_events: u32,
        max_output_text_bytes: u32,
        max_tool_calls: u8,
        max_tool_argument_bytes: u32,
    ) -> Result<Self, AgentProviderContractError> {
        if max_wire_bytes == 0
            || max_wire_bytes > MAX_AGENT_PROVIDER_STREAM_WIRE_BYTES
            || max_events == 0
            || max_events > MAX_AGENT_PROVIDER_STREAM_EVENTS
            || max_output_text_bytes == 0
            || max_output_text_bytes > MAX_AGENT_PROVIDER_OUTPUT_TEXT_BYTES
            || max_tool_calls == 0
            || max_tool_calls > MAX_AGENT_PROVIDER_TOOL_CALLS
            || max_tool_argument_bytes == 0
            || max_tool_argument_bytes > MAX_AGENT_PROVIDER_TOOL_ARGUMENT_BYTES
        {
            return Err(AgentProviderContractError::StreamBudget);
        }
        Ok(Self {
            max_wire_bytes,
            max_events,
            max_output_text_bytes,
            max_tool_calls,
            max_tool_argument_bytes,
        })
    }

    /// Maximum aggregate provider response-body bytes.
    pub const fn max_wire_bytes(self) -> u32 {
        self.max_wire_bytes
    }

    /// Maximum accepted SSE events, including pings and ignored future events.
    pub const fn max_events(self) -> u32 {
        self.max_events
    }

    /// Maximum aggregate model-authored plain-text bytes.
    pub const fn max_output_text_bytes(self) -> u32 {
        self.max_output_text_bytes
    }

    /// Maximum complete client tool calls.
    pub const fn max_tool_calls(self) -> u8 {
        self.max_tool_calls
    }

    /// Maximum aggregate incomplete and complete tool-argument bytes.
    pub const fn max_tool_argument_bytes(self) -> u32 {
        self.max_tool_argument_bytes
    }
}

/// Fixed provider selection and response bounds for one admitted call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentProviderCallConfig {
    provider: AgentProviderKind,
    model: AgentProviderModelRevision,
    tokenizer: SemanticTokenizerRevision,
    fixed_input_tokens: u32,
    max_output_tokens: u32,
    stream: AgentProviderStreamBudget,
}

impl AgentProviderCallConfig {
    /// Binds one provider/model revision to explicit output and stream limits.
    pub fn try_new(
        provider: AgentProviderKind,
        model: AgentProviderModelRevision,
        tokenizer: SemanticTokenizerRevision,
        fixed_input_tokens: u32,
        max_output_tokens: u32,
        stream: AgentProviderStreamBudget,
    ) -> Result<Self, AgentProviderContractError> {
        if fixed_input_tokens == 0 || u64::from(fixed_input_tokens) > MAX_AGENT_RUN_MODEL_TOKENS {
            return Err(AgentProviderContractError::InputTokens);
        }
        if max_output_tokens == 0 || u64::from(max_output_tokens) > MAX_AGENT_RUN_MODEL_TOKENS {
            return Err(AgentProviderContractError::OutputTokens);
        }
        Ok(Self {
            provider,
            model,
            tokenizer,
            fixed_input_tokens,
            max_output_tokens,
            stream,
        })
    }

    /// Selected provider protocol.
    pub const fn provider(&self) -> AgentProviderKind {
        self.provider
    }

    /// Exact selected provider model revision.
    pub const fn model(&self) -> &AgentProviderModelRevision {
        &self.model
    }

    /// Exact tokenizer/counting revision used for input admission.
    pub const fn tokenizer(&self) -> &SemanticTokenizerRevision {
        &self.tokenizer
    }

    /// Exact pinned envelope/tool-schema token count reserved beyond payload.
    pub const fn fixed_input_tokens(&self) -> u32 {
        self.fixed_input_tokens
    }

    /// Hard requested provider output-token ceiling.
    pub const fn max_output_tokens(&self) -> u32 {
        self.max_output_tokens
    }

    /// Hard per-call stream ceilings.
    pub const fn stream_budget(&self) -> AgentProviderStreamBudget {
        self.stream
    }

    pub(crate) fn validate_request(
        &self,
        request: AgentModelCallRequest,
        payload: &SemanticTokenMeasurement,
        objective: &SemanticTokenMeasurement,
    ) -> Result<(), AgentProviderContractError> {
        if payload.revision() != &self.tokenizer || objective.revision() != &self.tokenizer {
            return Err(AgentProviderContractError::TokenizerRevision);
        }
        let additional_input_tokens = u64::from(objective.tokens())
            .checked_add(u64::from(self.fixed_input_tokens))
            .ok_or(AgentProviderContractError::AdmissionBudget)?;
        if additional_input_tokens > u64::from(request.budget().additional_input_tokens())
            || u64::from(self.max_output_tokens) > u64::from(request.budget().output_tokens())
            || u64::from(payload.tokens())
                .checked_add(additional_input_tokens)
                .is_none()
        {
            return Err(AgentProviderContractError::AdmissionBudget);
        }
        Ok(())
    }
}

/// Normalized provider token accounting from one terminal response.
///
/// `input_tokens` and `output_tokens` are the totals supplied to policy.
/// Cache and reasoning fields are informational subsets after vendor-specific
/// normalization and are never added to those totals a second time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentProviderUsage {
    input_tokens: u64,
    output_tokens: u64,
    cached_input_tokens: u64,
    cache_write_input_tokens: u64,
    reasoning_output_tokens: u64,
}

/// One bounded untrusted plain-text delta from a provider stream.
///
/// This value is never HTML, policy, a browser operation, or trusted UI. A
/// presentation consumer must render it as escaped text and must not persist
/// or log it indiscriminately.
#[derive(Eq, PartialEq)]
pub struct AgentProviderTextDelta(String);

impl AgentProviderTextDelta {
    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }

    /// Exact untrusted plain text for an explicitly authorized consumer.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Exact UTF-8 byte count for stream accounting.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the provider emitted an empty delta.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for AgentProviderTextDelta {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderTextDelta")
            .field("bytes", &self.0.len())
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Closed normalized incremental provider output.
#[derive(Eq, PartialEq)]
pub enum AgentProviderStreamEvent {
    /// Bounded untrusted plain text. It never authorizes a browser operation.
    TextDelta(AgentProviderTextDelta),
    /// Closed pre-policy browser proposal from one complete client tool call.
    ///
    /// The response terminal must still confirm `ToolCalls`; this value alone
    /// cannot be bound, authorized, or executed.
    ToolCall(AgentBrowserToolCall),
}

impl fmt::Debug for AgentProviderStreamEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TextDelta(delta) => formatter.debug_tuple("TextDelta").field(delta).finish(),
            Self::ToolCall(call) => formatter.debug_tuple("ToolCall").field(call).finish(),
        }
    }
}

/// One bounded batch emitted from an arbitrary transport chunk.
#[derive(Eq, PartialEq)]
pub struct AgentProviderStreamBatch {
    call: AgentProviderCallIdentity,
    events: Vec<AgentProviderStreamEvent>,
}

impl AgentProviderStreamBatch {
    pub(crate) fn new(
        call: AgentProviderCallIdentity,
        events: Vec<AgentProviderStreamEvent>,
    ) -> Self {
        Self { call, events }
    }

    /// Exact non-authorizing call correlation.
    pub const fn call(&self) -> AgentProviderCallIdentity {
        self.call
    }

    /// Ordered normalized events decoded from this transport chunk.
    pub fn events(&self) -> &[AgentProviderStreamEvent] {
        &self.events
    }

    /// Consumes the batch without copying model-authored text.
    pub fn into_events(self) -> Vec<AgentProviderStreamEvent> {
        self.events
    }
}

impl fmt::Debug for AgentProviderStreamBatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderStreamBatch")
            .field("call", &self.call)
            .field("events", &self.events.len())
            .finish()
    }
}

/// Content-free counters for one terminal provider stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentProviderStreamStats {
    wire_bytes: u32,
    events: u32,
    output_text_bytes: u32,
    tool_calls: u8,
    tool_argument_bytes: u32,
}

impl AgentProviderStreamStats {
    pub(crate) const fn new(
        wire_bytes: u32,
        events: u32,
        output_text_bytes: u32,
        tool_calls: u8,
        tool_argument_bytes: u32,
    ) -> Self {
        Self {
            wire_bytes,
            events,
            output_text_bytes,
            tool_calls,
            tool_argument_bytes,
        }
    }

    /// Accepted aggregate provider response-body bytes.
    pub const fn wire_bytes(self) -> u32 {
        self.wire_bytes
    }

    /// Accepted SSE event count, including ignored typed lifecycle events.
    pub const fn events(self) -> u32 {
        self.events
    }

    /// Accepted model-authored plain-text bytes.
    pub const fn output_text_bytes(self) -> u32 {
        self.output_text_bytes
    }

    /// Accepted complete typed client tool calls.
    pub const fn tool_calls(self) -> u8 {
        self.tool_calls
    }

    /// Accepted aggregate incomplete and complete tool-argument bytes.
    pub const fn tool_argument_bytes(self) -> u32 {
        self.tool_argument_bytes
    }
}

/// One normalized successful terminal provider response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentProviderCompletion {
    call: AgentProviderCallIdentity,
    stop: AgentProviderStopReason,
    usage: AgentProviderUsage,
    stats: AgentProviderStreamStats,
}

impl AgentProviderCompletion {
    pub(crate) const fn new(
        call: AgentProviderCallIdentity,
        stop: AgentProviderStopReason,
        usage: AgentProviderUsage,
        stats: AgentProviderStreamStats,
    ) -> Self {
        Self {
            call,
            stop,
            usage,
            stats,
        }
    }

    /// Exact non-authorizing call correlation.
    pub const fn call(self) -> AgentProviderCallIdentity {
        self.call
    }

    /// Normalized successful stop class.
    pub const fn stop(self) -> AgentProviderStopReason {
        self.stop
    }

    /// Exact normalized provider token counters.
    pub const fn usage(self) -> AgentProviderUsage {
        self.usage
    }

    /// Content-free bounded stream counters.
    pub const fn stats(self) -> AgentProviderStreamStats {
        self.stats
    }
}

/// One normalized terminal provider failure after a stream began.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentProviderTerminalFailure {
    call: AgentProviderCallIdentity,
    failure: AgentProviderFailure,
    usage: Option<AgentProviderUsage>,
    stats: AgentProviderStreamStats,
}

impl AgentProviderTerminalFailure {
    pub(crate) const fn new(
        call: AgentProviderCallIdentity,
        failure: AgentProviderFailure,
        usage: Option<AgentProviderUsage>,
        stats: AgentProviderStreamStats,
    ) -> Self {
        Self {
            call,
            failure,
            usage,
            stats,
        }
    }

    /// Exact non-authorizing call correlation.
    pub const fn call(self) -> AgentProviderCallIdentity {
        self.call
    }

    /// Closed content-free provider failure.
    pub const fn failure(self) -> AgentProviderFailure {
        self.failure
    }

    /// Provider usage when the terminal event supplied valid accounting.
    pub const fn usage(self) -> Option<AgentProviderUsage> {
        self.usage
    }

    /// Content-free bounded stream counters.
    pub const fn stats(self) -> AgentProviderStreamStats {
        self.stats
    }
}

/// Sole normalized terminal outcome for one provider stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentProviderStreamConclusion {
    /// Provider returned one valid completed or incomplete response.
    Completed(AgentProviderCompletion),
    /// Provider returned one closed failure or cancellation.
    Failed(AgentProviderTerminalFailure),
}

impl AgentProviderUsage {
    /// Validates checked totals and subset details without capping real overage.
    pub const fn try_new(
        input_tokens: u64,
        output_tokens: u64,
        cached_input_tokens: u64,
        cache_write_input_tokens: u64,
        reasoning_output_tokens: u64,
    ) -> Result<Self, AgentProviderContractError> {
        if input_tokens.checked_add(output_tokens).is_none()
            || cached_input_tokens > input_tokens
            || cache_write_input_tokens > input_tokens
            || reasoning_output_tokens > output_tokens
        {
            return Err(AgentProviderContractError::Usage);
        }
        Ok(Self {
            input_tokens,
            output_tokens,
            cached_input_tokens,
            cache_write_input_tokens,
            reasoning_output_tokens,
        })
    }

    /// Provider-accounted total input tokens for policy settlement.
    pub const fn input_tokens(self) -> u64 {
        self.input_tokens
    }

    /// Provider-accounted total output tokens for policy settlement.
    pub const fn output_tokens(self) -> u64 {
        self.output_tokens
    }

    /// Cached input subset after vendor normalization.
    pub const fn cached_input_tokens(self) -> u64 {
        self.cached_input_tokens
    }

    /// Cache-write input subset after vendor normalization.
    pub const fn cache_write_input_tokens(self) -> u64 {
        self.cache_write_input_tokens
    }

    /// Reasoning-token subset of output, never reasoning content.
    pub const fn reasoning_output_tokens(self) -> u64 {
        self.reasoning_output_tokens
    }

    /// Checked provider-accounted input plus output tokens.
    pub const fn total_tokens(self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

/// Normalized successful provider stop class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentProviderStopReason {
    /// The model completed its turn normally.
    Completed,
    /// The model emitted one or more complete client tool calls.
    ToolCalls,
    /// The provider stopped at the configured output limit.
    OutputLimit,
    /// The model explicitly refused the request.
    Refused,
    /// Provider safety filtering prevented a complete result.
    ContentFiltered,
    /// A caller-selected stop sequence ended generation.
    StopSequence,
    /// The provider paused a resumable turn without completing it.
    Paused,
}

/// Closed terminal provider failure class without provider-authored text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentProviderFailureClass {
    /// Locally invalid fixed request or provider HTTP 400-class refusal.
    InvalidRequest,
    /// Provider authentication was missing or rejected.
    Authentication,
    /// Credential lacks authority for the selected provider resource.
    Permission,
    /// Selected model or provider resource was not found.
    NotFound,
    /// Provider state conflicted with this exact request.
    Conflict,
    /// Provider rate limit rejected this attempt.
    RateLimited,
    /// Provider was temporarily overloaded or unavailable.
    Overloaded,
    /// Exact transport deadline expired.
    Timeout,
    /// Network/TLS/body transport failed after dispatch.
    Transport,
    /// Provider wire data violated the typed protocol contract.
    Protocol,
    /// Provider returned another terminal server-side failure.
    Provider,
    /// Exact run cancellation won the terminal race.
    Cancelled,
}

impl AgentProviderFailureClass {
    /// Whether policy may consider a newly admitted retry.
    ///
    /// This never authorizes or schedules a retry. A retry requires a new
    /// supervisor decision, model-call identity, budget reservation, and
    /// cancellation check.
    pub const fn retry_disposition(self) -> AgentProviderRetryDisposition {
        match self {
            Self::RateLimited | Self::Overloaded | Self::Timeout | Self::Transport => {
                AgentProviderRetryDisposition::PolicyMayRetry
            }
            Self::InvalidRequest
            | Self::Authentication
            | Self::Permission
            | Self::NotFound
            | Self::Conflict
            | Self::Protocol
            | Self::Provider
            | Self::Cancelled => AgentProviderRetryDisposition::Never,
        }
    }
}

/// Non-authorizing retry classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentProviderRetryDisposition {
    /// The same failure must not be automatically repeated.
    Never,
    /// Policy may decide to create one newly admitted attempt.
    PolicyMayRetry,
}

/// Bounded provider-requested delay offered only to a future retry policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentProviderRetryAfter(u64);

impl AgentProviderRetryAfter {
    /// Validates a nonzero delay no longer than one day.
    pub const fn try_from_millis(millis: u64) -> Result<Self, AgentProviderContractError> {
        if millis == 0 || millis > MAX_AGENT_PROVIDER_RETRY_AFTER_MILLIS {
            Err(AgentProviderContractError::RetryAfter)
        } else {
            Ok(Self(millis))
        }
    }

    /// Exact bounded provider delay in milliseconds.
    pub const fn millis(self) -> u64 {
        self.0
    }
}

/// Content-free terminal provider failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentProviderFailure {
    class: AgentProviderFailureClass,
    retry_after: Option<AgentProviderRetryAfter>,
}

impl AgentProviderFailure {
    /// Constructs one closed failure and validates retry-after compatibility.
    pub const fn try_new(
        class: AgentProviderFailureClass,
        retry_after: Option<AgentProviderRetryAfter>,
    ) -> Result<Self, AgentProviderContractError> {
        if retry_after.is_some()
            && !matches!(
                class.retry_disposition(),
                AgentProviderRetryDisposition::PolicyMayRetry
            )
        {
            return Err(AgentProviderContractError::RetryAfter);
        }
        Ok(Self { class, retry_after })
    }

    /// Closed terminal failure class.
    pub const fn class(self) -> AgentProviderFailureClass {
        self.class
    }

    /// Bounded provider hint, not retry authority.
    pub const fn retry_after(self) -> Option<AgentProviderRetryAfter> {
        self.retry_after
    }

    /// Non-authorizing retry classification.
    pub const fn retry_disposition(self) -> AgentProviderRetryDisposition {
        self.class.retry_disposition()
    }
}

/// Refusal to construct or decode a provider-neutral contract.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentProviderContractError {
    /// Configuration selected a different provider protocol than the adapter.
    #[error("agent provider kind does not match the selected adapter")]
    ProviderKind,
    /// Model revision was empty, oversized, URL/path-like, or unsafe ASCII.
    #[error("agent provider model revision is invalid")]
    ModelRevision,
    /// Fixed request-envelope input tokens were zero or exceeded the hard limit.
    #[error("agent provider fixed input-token count is invalid")]
    InputTokens,
    /// Requested output-token ceiling was zero or exceeded the hard limit.
    #[error("agent provider output-token ceiling is invalid")]
    OutputTokens,
    /// Provider/tokenizer selection or request ceilings exceed exact admission.
    #[error("agent provider call does not fit its policy admission")]
    AdmissionBudget,
    /// Semantic payload was measured with a different tokenizer revision.
    #[error("agent provider tokenizer revision does not match semantic payload")]
    TokenizerRevision,
    /// One or more stream ceilings were zero or exceeded hard limits.
    #[error("agent provider stream budget is invalid")]
    StreamBudget,
    /// Provider usage overflowed or subset counters contradicted totals.
    #[error("agent provider usage is invalid")]
    Usage,
    /// Retry-after was zero, oversized, or attached to a terminal class.
    #[error("agent provider retry-after is invalid")]
    RetryAfter,
}

/// Closed provider stream protocol failure without wire or model text.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentProviderProtocolError {
    /// SSE framing, UTF-8, or end-of-stream state was invalid.
    #[error("agent provider stream framing is invalid")]
    Framing,
    /// A line, event, text, tool, argument, or event-count ceiling was exceeded.
    #[error("agent provider stream ceiling exceeded")]
    Limit,
    /// A recognized provider event had an invalid typed payload.
    #[error("agent provider stream event is invalid")]
    Event,
    /// Recognized events appeared in an impossible or ambiguous order.
    #[error("agent provider stream sequence is invalid")]
    Sequence,
    /// Provider emitted an output class this browser adapter never requested.
    #[error("agent provider output class is unsupported")]
    UnsupportedOutput,
    /// A complete client tool call failed the closed browser-tool contract.
    #[error("agent provider tool call is invalid")]
    ToolCall,
    /// Terminal provider usage was absent or internally inconsistent.
    #[error("agent provider terminal usage is invalid")]
    Usage,
    /// Stream ended without one unambiguous terminal outcome.
    #[error("agent provider terminal event is invalid")]
    Terminal,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenizer(value: &str) -> SemanticTokenizerRevision {
        SemanticTokenizerRevision::try_new(value.to_owned()).expect("valid tokenizer")
    }

    #[test]
    fn provider_configuration_and_diagnostics_are_bounded() {
        let model =
            AgentProviderModelRevision::try_new("gpt-5.6-sol".to_owned()).expect("valid revision");
        assert_eq!(model.as_str(), "gpt-5.6-sol");
        assert!(!format!("{model:?}").contains("gpt-5.6-sol"));
        let config = AgentProviderCallConfig::try_new(
            AgentProviderKind::OpenAiResponses,
            model,
            tokenizer("openai:gpt-5.6-sol:v1"),
            512,
            4_096,
            AgentProviderStreamBudget::STANDARD,
        )
        .expect("valid config");
        assert_eq!(config.fixed_input_tokens(), 512);
        assert_eq!(config.max_output_tokens(), 4_096);
        assert_eq!(config.stream_budget().max_tool_calls(), 8);

        for invalid in ["", "../model", "https://model", "model name", "model/alias"] {
            assert_eq!(
                AgentProviderModelRevision::try_new(invalid.to_owned()),
                Err(AgentProviderContractError::ModelRevision)
            );
        }
        assert_eq!(
            AgentProviderModelRevision::try_new("x".repeat(97)),
            Err(AgentProviderContractError::ModelRevision)
        );
        assert_eq!(
            AgentProviderCallConfig::try_new(
                AgentProviderKind::AnthropicMessages,
                AgentProviderModelRevision::try_new("claude-opus-5".to_owned())
                    .expect("valid revision"),
                tokenizer("anthropic:claude-opus-5:v1"),
                512,
                0,
                AgentProviderStreamBudget::STANDARD,
            ),
            Err(AgentProviderContractError::OutputTokens)
        );
    }

    #[test]
    fn stream_budget_refuses_every_zero_and_over_limit_dimension() {
        assert_eq!(
            AgentProviderStreamBudget::try_new(0, 1, 1, 1, 1),
            Err(AgentProviderContractError::StreamBudget)
        );
        assert_eq!(
            AgentProviderStreamBudget::try_new(1, 0, 1, 1, 1),
            Err(AgentProviderContractError::StreamBudget)
        );
        assert_eq!(
            AgentProviderStreamBudget::try_new(1, 1, 0, 1, 1),
            Err(AgentProviderContractError::StreamBudget)
        );
        assert_eq!(
            AgentProviderStreamBudget::try_new(1, 1, 1, 0, 1),
            Err(AgentProviderContractError::StreamBudget)
        );
        assert_eq!(
            AgentProviderStreamBudget::try_new(1, 1, 1, 1, 0),
            Err(AgentProviderContractError::StreamBudget)
        );
        assert_eq!(
            AgentProviderStreamBudget::try_new(1, MAX_AGENT_PROVIDER_STREAM_EVENTS + 1, 1, 1, 1,),
            Err(AgentProviderContractError::StreamBudget)
        );
        let maximum = AgentProviderStreamBudget::try_new(
            MAX_AGENT_PROVIDER_STREAM_WIRE_BYTES,
            MAX_AGENT_PROVIDER_STREAM_EVENTS,
            MAX_AGENT_PROVIDER_OUTPUT_TEXT_BYTES,
            MAX_AGENT_PROVIDER_TOOL_CALLS,
            MAX_AGENT_PROVIDER_TOOL_ARGUMENT_BYTES,
        )
        .expect("hard boundary");
        assert_eq!(
            maximum.max_wire_bytes(),
            MAX_AGENT_PROVIDER_STREAM_WIRE_BYTES
        );
        assert_eq!(maximum.max_events(), MAX_AGENT_PROVIDER_STREAM_EVENTS);
    }

    #[test]
    fn usage_keeps_vendor_details_as_subsets_without_hiding_overage() {
        let usage =
            AgentProviderUsage::try_new(5_000, 900, 2_000, 1_000, 700).expect("valid usage");
        assert_eq!(usage.total_tokens(), 5_900);
        assert_eq!(usage.cached_input_tokens(), 2_000);
        assert_eq!(usage.cache_write_input_tokens(), 1_000);
        assert_eq!(usage.reasoning_output_tokens(), 700);
        assert_eq!(
            AgentProviderUsage::try_new(10, 5, 11, 0, 0),
            Err(AgentProviderContractError::Usage)
        );
        assert_eq!(
            AgentProviderUsage::try_new(u64::MAX, 1, 0, 0, 0),
            Err(AgentProviderContractError::Usage)
        );
    }

    #[test]
    fn retry_hints_never_become_retry_authority() {
        let retry_after = AgentProviderRetryAfter::try_from_millis(2_000).expect("delay");
        let failure = AgentProviderFailure::try_new(
            AgentProviderFailureClass::RateLimited,
            Some(retry_after),
        )
        .expect("retryable class");
        assert_eq!(
            failure.retry_disposition(),
            AgentProviderRetryDisposition::PolicyMayRetry
        );
        assert_eq!(failure.retry_after(), Some(retry_after));
        assert_eq!(
            AgentProviderFailure::try_new(
                AgentProviderFailureClass::Authentication,
                Some(retry_after),
            ),
            Err(AgentProviderContractError::RetryAfter)
        );
        assert_eq!(
            AgentProviderFailureClass::Cancelled.retry_disposition(),
            AgentProviderRetryDisposition::Never
        );
    }
}
