//! Fixed-endpoint HTTPS transport for bounded agent model calls.
//!
//! The semantic core constructs and policy-admits every byte before this shell
//! sees it. This crate adds only provider credentials, exact HTTPS endpoints,
//! bounded concurrency, cancellation, response framing, and the existing
//! provider-neutral stream decoder. It exposes no arbitrary URL, generic HTTP
//! request, provider-native browser tool, raw response body, or automatic retry.

#![deny(missing_docs)]
#![deny(unsafe_code)]

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::TryStreamExt;
use reqwest::header::{
    HeaderMap, HeaderName, HeaderValue, ACCEPT, ACCEPT_ENCODING, AUTHORIZATION, CACHE_CONTROL,
    CONTENT_ENCODING, CONTENT_TYPE, RETRY_AFTER,
};
use reqwest::redirect::Policy;
use reqwest::{Client, StatusCode, Url};
use thiserror::Error;
use tokio::sync::Notify;
use zephium_agentic::{
    AgentActiveModelCall, AgentCommittedProviderRequest, AgentProviderCallIdentity,
    AgentProviderEndpoint, AgentProviderFailure, AgentProviderFailureClass, AgentProviderKind,
    AgentProviderRequestError, AgentProviderRetryAfter, AgentProviderStreamBatch,
    AgentProviderStreamConclusion, AgentProviderStreamDecoder, AgentProviderTransportInput,
    AgentRunPolicy,
};
use zeroize::Zeroizing;

/// Maximum secret bytes accepted for one provider credential.
pub const MAX_AGENT_PROVIDER_CREDENTIAL_BYTES: usize = 1_024;
/// Maximum simultaneous admitted HTTP attempts in one transport instance.
pub const MAX_AGENT_PROVIDER_TRANSPORT_CALLS: usize = 4;
/// Maximum total request lifetime accepted by transport configuration.
pub const MAX_AGENT_PROVIDER_REQUEST_TIMEOUT_MILLIS: u64 = 10 * 60 * 1_000;
/// Maximum connection-establishment deadline accepted by configuration.
pub const MAX_AGENT_PROVIDER_CONNECT_TIMEOUT_MILLIS: u64 = 30 * 1_000;
/// Maximum interval without response-body progress accepted by configuration.
pub const MAX_AGENT_PROVIDER_READ_TIMEOUT_MILLIS: u64 = 2 * 60 * 1_000;
/// Maximum HTTP/2 response-header list bytes accepted from a provider.
pub const MAX_AGENT_PROVIDER_RESPONSE_HEADER_BYTES: u32 = 64 * 1_024;

const OPENAI_RESPONSES_URL: &str = "https://api.openai.com/v1/responses";
const ANTHROPIC_MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const PRODUCT_USER_AGENT: &str = "Zephium-Agent-Browser/0.1";

/// Bounded deadlines for the shared provider client.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentProviderTransportConfig {
    request_timeout: Duration,
    connect_timeout: Duration,
    read_timeout: Duration,
}

impl AgentProviderTransportConfig {
    /// Conservative production defaults with an idle-body progress deadline.
    pub const STANDARD: Self = Self {
        request_timeout: Duration::from_secs(5 * 60),
        connect_timeout: Duration::from_secs(15),
        read_timeout: Duration::from_secs(60),
    };

    /// Validates nonzero, ordered deadlines against product hard ceilings.
    pub fn try_new(
        request_timeout: Duration,
        connect_timeout: Duration,
        read_timeout: Duration,
    ) -> Result<Self, AgentProviderTransportConfigError> {
        if request_timeout.is_zero()
            || connect_timeout.is_zero()
            || read_timeout.is_zero()
            || connect_timeout > request_timeout
            || read_timeout > request_timeout
            || request_timeout.as_millis() > u128::from(MAX_AGENT_PROVIDER_REQUEST_TIMEOUT_MILLIS)
            || connect_timeout.as_millis() > u128::from(MAX_AGENT_PROVIDER_CONNECT_TIMEOUT_MILLIS)
            || read_timeout.as_millis() > u128::from(MAX_AGENT_PROVIDER_READ_TIMEOUT_MILLIS)
        {
            return Err(AgentProviderTransportConfigError::Deadlines);
        }
        Ok(Self {
            request_timeout,
            connect_timeout,
            read_timeout,
        })
    }

    /// End-to-end deadline including connection and response streaming.
    pub const fn request_timeout(self) -> Duration {
        self.request_timeout
    }

    /// Connection-establishment deadline.
    pub const fn connect_timeout(self) -> Duration {
        self.connect_timeout
    }

    /// Maximum interval without response-body progress.
    pub const fn read_timeout(self) -> Duration {
        self.read_timeout
    }
}

/// Failure to construct the fixed provider client.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentProviderTransportConfigError {
    /// One or more deadlines were empty, contradictory, or above a hard bound.
    #[error("agent provider transport deadlines are invalid")]
    Deadlines,
    /// A compile-time provider endpoint was not an exact absolute URL.
    #[error("agent provider endpoint configuration is invalid")]
    Endpoint,
    /// The redirect-free HTTPS client could not be constructed.
    #[error("agent provider HTTPS client is unavailable")]
    Client,
}

/// Move-only, provider-bound credential whose owned bytes are zeroized on drop.
#[must_use]
pub struct AgentProviderCredential {
    provider: AgentProviderKind,
    secret: Zeroizing<Vec<u8>>,
}

impl AgentProviderCredential {
    /// Consumes and validates one visible-ASCII provider secret.
    pub fn try_new(
        provider: AgentProviderKind,
        secret: String,
    ) -> Result<Self, AgentProviderCredentialError> {
        let secret = Zeroizing::new(secret.into_bytes());
        if secret.is_empty()
            || secret.len() > MAX_AGENT_PROVIDER_CREDENTIAL_BYTES
            || !secret.iter().all(|byte| matches!(byte, 0x21..=0x7e))
        {
            return Err(AgentProviderCredentialError::Content);
        }
        Ok(Self { provider, secret })
    }

    /// Exact provider protocol this credential may authenticate.
    pub const fn provider(&self) -> AgentProviderKind {
        self.provider
    }

    /// Credential byte count without exposing the secret.
    pub fn byte_len(&self) -> usize {
        self.secret.len()
    }
}

impl fmt::Debug for AgentProviderCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderCredential")
            .field("provider", &self.provider)
            .field("bytes", &self.secret.len())
            .field("secret", &"[redacted]")
            .finish()
    }
}

/// Refusal to retain a provider credential.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentProviderCredentialError {
    /// Credential was empty, oversized, or not visible ASCII.
    #[error("agent provider credential is invalid")]
    Content,
    /// Memory for a bounded zeroizing credential copy was unavailable.
    #[error("agent provider credential memory is unavailable")]
    Capacity,
}

struct AgentProviderAttemptCredential {
    secret: Zeroizing<Vec<u8>>,
}

impl AgentProviderAttemptCredential {
    fn try_from_credential(
        credential: &AgentProviderCredential,
    ) -> Result<Self, AgentProviderCredentialError> {
        let mut secret = Zeroizing::new(Vec::new());
        secret
            .try_reserve_exact(credential.secret.len())
            .map_err(|_| AgentProviderCredentialError::Capacity)?;
        secret.extend_from_slice(&credential.secret);
        let attempt = Self { secret };
        let _validated = sensitive_header(credential.provider, &attempt.secret)?;
        Ok(attempt)
    }

    fn into_sensitive_header(
        self,
        provider: AgentProviderKind,
    ) -> Result<HeaderValue, AgentProviderCredentialError> {
        sensitive_header(provider, &self.secret)
    }
}

fn sensitive_header(
    provider: AgentProviderKind,
    secret: &[u8],
) -> Result<HeaderValue, AgentProviderCredentialError> {
    let prefix = if provider == AgentProviderKind::OpenAiResponses {
        b"Bearer ".as_slice()
    } else {
        &[]
    };
    let capacity = prefix
        .len()
        .checked_add(secret.len())
        .ok_or(AgentProviderCredentialError::Capacity)?;
    let mut encoded = Zeroizing::new(Vec::new());
    encoded
        .try_reserve_exact(capacity)
        .map_err(|_| AgentProviderCredentialError::Capacity)?;
    encoded.extend_from_slice(prefix);
    encoded.extend_from_slice(secret);
    let mut value =
        HeaderValue::from_bytes(&encoded).map_err(|_| AgentProviderCredentialError::Content)?;
    value.set_sensitive(true);
    Ok(value)
}

/// Sticky cancellation shared from one run tree into its provider attempts.
#[derive(Clone)]
pub struct AgentProviderCancellation {
    inner: Arc<AgentProviderCancellationInner>,
}

struct AgentProviderCancellationInner {
    cancelled: AtomicBool,
    notify: Notify,
}

impl AgentProviderCancellation {
    /// Creates one initially-live cancellation authority without a task or timer.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AgentProviderCancellationInner {
                cancelled: AtomicBool::new(false),
                notify: Notify::new(),
            }),
        }
    }

    /// Makes cancellation sticky and wakes all currently admitted attempts.
    pub fn cancel(&self) -> bool {
        let first = !self.inner.cancelled.swap(true, Ordering::SeqCst);
        if first {
            self.inner.notify.notify_waiters();
        }
        first
    }

    /// Whether cancellation has become sticky.
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    fn try_claim_commit(&self) -> bool {
        !self.inner.cancelled.load(Ordering::SeqCst)
    }

    async fn cancelled(&self) {
        loop {
            let notified = self.inner.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

impl Default for AgentProviderCancellation {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for AgentProviderCancellation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderCancellation")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

#[derive(Clone)]
struct ProviderEndpoints {
    openai: Url,
    anthropic: Url,
    https_only: bool,
}

impl ProviderEndpoints {
    fn production() -> Result<Self, AgentProviderTransportConfigError> {
        let openai = Url::parse(OPENAI_RESPONSES_URL)
            .map_err(|_| AgentProviderTransportConfigError::Endpoint)?;
        let anthropic = Url::parse(ANTHROPIC_MESSAGES_URL)
            .map_err(|_| AgentProviderTransportConfigError::Endpoint)?;
        if !exact_production_url(&openai, "api.openai.com", "/v1/responses")
            || !exact_production_url(&anthropic, "api.anthropic.com", "/v1/messages")
        {
            return Err(AgentProviderTransportConfigError::Endpoint);
        }
        Ok(Self {
            openai,
            anthropic,
            https_only: true,
        })
    }

    fn endpoint(&self, endpoint: AgentProviderEndpoint) -> &Url {
        match endpoint {
            AgentProviderEndpoint::OpenAiResponses => &self.openai,
            AgentProviderEndpoint::AnthropicMessages => &self.anthropic,
        }
    }

    #[cfg(test)]
    fn loopback(openai: Url, anthropic: Url) -> Result<Self, AgentProviderTransportConfigError> {
        if !exact_loopback_url(&openai) || !exact_loopback_url(&anthropic) {
            return Err(AgentProviderTransportConfigError::Endpoint);
        }
        Ok(Self {
            openai,
            anthropic,
            https_only: false,
        })
    }
}

fn exact_production_url(url: &Url, host: &str, path: &str) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some(host)
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == path
        && url.query().is_none()
        && url.fragment().is_none()
}

#[cfg(test)]
fn exact_loopback_url(url: &Url) -> bool {
    url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.path().starts_with('/')
        && url.query().is_none()
        && url.fragment().is_none()
}

struct TransportState {
    sealed: bool,
    active: Vec<AgentProviderCallIdentity>,
}

struct SharedTransportState {
    state: Mutex<TransportState>,
    shutdown: AgentProviderCancellation,
}

/// Content-free snapshot of provider transport resource ownership.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentProviderTransportSnapshot {
    sealed: bool,
    active: usize,
}

impl AgentProviderTransportSnapshot {
    /// Whether the transport refuses every future admission.
    pub const fn is_sealed(self) -> bool {
        self.sealed
    }

    /// Exact number of admitted attempts that have not returned a result.
    pub const fn active_attempts(self) -> usize {
        self.active
    }

    /// Process-local hard concurrency ceiling.
    pub const fn attempt_limit(self) -> usize {
        MAX_AGENT_PROVIDER_TRANSPORT_CALLS
    }

    /// Whether shutdown has drained every admitted attempt.
    pub const fn is_quiescent(self) -> bool {
        self.active == 0
    }
}

/// Failure to read or mutate transport resource state safely.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentProviderTransportStateError {
    /// A prior panic poisoned the bounded admission registry.
    #[error("agent provider transport state is unavailable")]
    Unavailable,
}

/// Shared fixed-endpoint provider transport with bounded synchronous admission.
///
/// Construction allocates a client and a four-entry registry. It starts no
/// task, timer, request, or socket. Clones share the same admission ceiling and
/// shutdown cancellation root.
#[derive(Clone)]
pub struct AgentProviderTransport {
    client: Client,
    config: AgentProviderTransportConfig,
    endpoints: ProviderEndpoints,
    shared: Arc<SharedTransportState>,
}

impl AgentProviderTransport {
    /// Builds the production client with two private exact HTTPS endpoints.
    pub fn try_new(
        config: AgentProviderTransportConfig,
    ) -> Result<Self, AgentProviderTransportConfigError> {
        Self::try_new_with_endpoints(config, ProviderEndpoints::production()?)
    }

    fn try_new_with_endpoints(
        config: AgentProviderTransportConfig,
        endpoints: ProviderEndpoints,
    ) -> Result<Self, AgentProviderTransportConfigError> {
        AgentProviderTransportConfig::try_new(
            config.request_timeout,
            config.connect_timeout,
            config.read_timeout,
        )?;
        let builder = Client::builder()
            .https_only(endpoints.https_only)
            .redirect(Policy::none())
            .referer(false)
            .retry(reqwest::retry::never())
            .timeout(config.request_timeout)
            .connect_timeout(config.connect_timeout)
            .read_timeout(config.read_timeout)
            .http2_max_header_list_size(MAX_AGENT_PROVIDER_RESPONSE_HEADER_BYTES)
            .pool_max_idle_per_host(0)
            .tcp_nodelay(true)
            .user_agent(HeaderValue::from_static(PRODUCT_USER_AGENT));
        #[cfg(test)]
        let builder = if endpoints.https_only {
            builder
        } else {
            builder.no_proxy()
        };
        let client = builder
            .build()
            .map_err(|_| AgentProviderTransportConfigError::Client)?;
        Ok(Self {
            client,
            config,
            endpoints,
            shared: Arc::new(SharedTransportState {
                state: Mutex::new(TransportState {
                    sealed: false,
                    active: Vec::with_capacity(MAX_AGENT_PROVIDER_TRANSPORT_CALLS),
                }),
                shutdown: AgentProviderCancellation::new(),
            }),
        })
    }

    #[cfg(test)]
    fn try_new_loopback(
        config: AgentProviderTransportConfig,
        openai: Url,
        anthropic: Url,
    ) -> Result<Self, AgentProviderTransportConfigError> {
        Self::try_new_with_endpoints(config, ProviderEndpoints::loopback(openai, anthropic)?)
    }

    /// Validated transport deadlines.
    pub const fn config(&self) -> AgentProviderTransportConfig {
        self.config
    }

    /// Returns the current bounded resource snapshot.
    pub fn snapshot(
        &self,
    ) -> Result<AgentProviderTransportSnapshot, AgentProviderTransportStateError> {
        let state = self
            .shared
            .state
            .lock()
            .map_err(|_| AgentProviderTransportStateError::Unavailable)?;
        Ok(AgentProviderTransportSnapshot {
            sealed: state.sealed,
            active: state.active.len(),
        })
    }

    /// Permanently refuses new calls and cancels every admitted attempt.
    pub fn seal(&self) {
        // Publish sticky cancellation before the admission flag so no caller
        // can observe shutdown, race through the separate cancellation check,
        // and commit after seal's linearization point.
        self.shared.shutdown.cancel();
        match self.shared.state.lock() {
            Ok(mut state) => state.sealed = true,
            Err(poisoned) => poisoned.into_inner().sealed = true,
        }
    }

    /// Admits, commits, and binds one exact request without starting network I/O.
    ///
    /// All credential, endpoint, cancellation, duplicate, capacity, and
    /// shutdown checks finish before semantic disclosure commits. Once this
    /// returns an attempt, every later outcome is post-commit and must consume
    /// the returned active policy authority exactly once.
    pub fn try_admit(
        &self,
        input: AgentProviderTransportInput,
        policy: &mut AgentRunPolicy,
        credential: &AgentProviderCredential,
        cancellation: AgentProviderCancellation,
    ) -> Result<AgentProviderAttempt, AgentProviderAdmissionError> {
        let call = input.request().call();
        let provider = input.request().config().provider();
        let endpoint = input.request().endpoint();
        if credential.provider() != provider {
            let _outcome = input
                .refuse(policy)
                .map_err(AgentProviderAdmissionError::Settlement)?;
            return Err(AgentProviderAdmissionError::CredentialProvider);
        }
        if !provider_endpoint_matches(provider, endpoint) {
            let _outcome = input
                .refuse(policy)
                .map_err(AgentProviderAdmissionError::Settlement)?;
            return Err(AgentProviderAdmissionError::Endpoint);
        }
        let credential = match AgentProviderAttemptCredential::try_from_credential(credential) {
            Ok(credential) => credential,
            Err(_) => {
                let _outcome = input
                    .refuse(policy)
                    .map_err(AgentProviderAdmissionError::Settlement)?;
                return Err(AgentProviderAdmissionError::Credential);
            }
        };
        let mut slot = match self.reserve(call) {
            Ok(slot) => slot,
            Err(error) => {
                let _outcome = input
                    .refuse(policy)
                    .map_err(AgentProviderAdmissionError::Settlement)?;
                return Err(error.into());
            }
        };
        if !cancellation.try_claim_commit() || !self.shared.shutdown.try_claim_commit() {
            let _outcome = input
                .cancel(policy)
                .map_err(AgentProviderAdmissionError::Settlement)?;
            drop(slot);
            return Err(AgentProviderAdmissionError::Cancelled);
        }
        let committed = match input.commit(policy) {
            Ok(committed) => committed,
            Err(error) => {
                drop(slot);
                return Err(AgentProviderAdmissionError::Settlement(error));
            }
        };
        slot.mark_committed();
        Ok(AgentProviderAttempt {
            client: self.client.clone(),
            endpoint: self.endpoints.endpoint(endpoint).clone(),
            provider,
            credential,
            committed,
            cancellation,
            shutdown: self.shared.shutdown.clone(),
            slot: Some(slot),
        })
    }

    fn reserve(&self, call: AgentProviderCallIdentity) -> Result<AgentProviderSlot, ReserveError> {
        let mut state = match self.shared.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                poisoned.into_inner().sealed = true;
                self.shared.shutdown.cancel();
                return Err(ReserveError::State);
            }
        };
        if state.sealed {
            return Err(ReserveError::Sealed);
        }
        if state.active.contains(&call) {
            return Err(ReserveError::Duplicate);
        }
        if state.active.len() >= MAX_AGENT_PROVIDER_TRANSPORT_CALLS {
            return Err(ReserveError::Capacity);
        }
        state.active.push(call);
        Ok(AgentProviderSlot {
            shared: Arc::clone(&self.shared),
            call,
            committed: false,
            completed: false,
        })
    }
}

impl fmt::Debug for AgentProviderTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderTransport")
            .field("config", &self.config)
            .field("snapshot", &self.snapshot().ok())
            .field("endpoints", &"[fixed]")
            .finish()
    }
}

fn provider_endpoint_matches(provider: AgentProviderKind, endpoint: AgentProviderEndpoint) -> bool {
    matches!(
        (provider, endpoint),
        (
            AgentProviderKind::OpenAiResponses,
            AgentProviderEndpoint::OpenAiResponses
        ) | (
            AgentProviderKind::AnthropicMessages,
            AgentProviderEndpoint::AnthropicMessages
        )
    )
}

enum ReserveError {
    Sealed,
    Capacity,
    Duplicate,
    State,
}

impl From<ReserveError> for AgentProviderAdmissionError {
    fn from(error: ReserveError) -> Self {
        match error {
            ReserveError::Sealed => Self::Sealed,
            ReserveError::Capacity => Self::Capacity,
            ReserveError::Duplicate => Self::Duplicate,
            ReserveError::State => Self::State,
        }
    }
}

struct AgentProviderSlot {
    shared: Arc<SharedTransportState>,
    call: AgentProviderCallIdentity,
    committed: bool,
    completed: bool,
}

impl AgentProviderSlot {
    fn mark_committed(&mut self) {
        self.committed = true;
    }

    fn mark_completed(&mut self) {
        self.completed = true;
    }
}

impl Drop for AgentProviderSlot {
    fn drop(&mut self) {
        let mut state = match self.shared.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                self.shared.shutdown.cancel();
                let mut state = poisoned.into_inner();
                state.sealed = true;
                state
            }
        };
        if self.committed && !self.completed {
            state.sealed = true;
            self.shared.shutdown.cancel();
        }
        if let Some(index) = state.active.iter().position(|call| call == &self.call) {
            state.active.swap_remove(index);
        } else {
            state.sealed = true;
            self.shared.shutdown.cancel();
        }
    }
}

/// Pre-network refusal from exact provider transport admission.
#[derive(Debug, Error)]
pub enum AgentProviderAdmissionError {
    /// Credential was bound to the other provider protocol.
    #[error("agent provider credential does not match the request provider")]
    CredentialProvider,
    /// Credential could not be encoded as the provider's sensitive header.
    #[error("agent provider credential header is invalid")]
    Credential,
    /// Provider kind and fixed request endpoint did not agree.
    #[error("agent provider request endpoint is invalid")]
    Endpoint,
    /// Four other model calls currently own all transport capacity.
    #[error("agent provider transport concurrency ceiling reached")]
    Capacity,
    /// The same exact call already owns a transport attempt.
    #[error("agent provider transport call is already active")]
    Duplicate,
    /// Transport shutdown permanently closed admission.
    #[error("agent provider transport is sealed")]
    Sealed,
    /// Exact cancellation won before semantic disclosure committed.
    #[error("agent provider request was cancelled before commitment")]
    Cancelled,
    /// Bounded transport admission state became unavailable.
    #[error("agent provider transport state is unavailable")]
    State,
    /// One-shot policy or semantic input settlement failed.
    #[error("agent provider input settlement failed")]
    Settlement(#[source] AgentProviderRequestError),
}

/// Consumer decision after receiving one normalized nonempty stream batch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentProviderBatchDisposition {
    /// Continue reading the exact provider stream.
    Continue,
    /// Stop this committed call and settle it as cancelled.
    Cancel,
}

/// Terminal network/decoder outcome for one committed provider request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentProviderTransportOutcome {
    /// The fixed provider decoder produced one normalized terminal result.
    Stream(AgentProviderStreamConclusion),
    /// HTTP, transport, cancellation, or protocol failed outside a terminal event.
    Failed(AgentProviderFailure),
}

/// Terminal result joined to the exact active policy authority.
#[must_use]
pub struct AgentProviderTransportResult {
    active: AgentActiveModelCall,
    outcome: AgentProviderTransportOutcome,
}

impl AgentProviderTransportResult {
    /// Exact non-authorizing correlation for this result.
    pub fn call(&self) -> AgentProviderCallIdentity {
        AgentProviderCallIdentity::from_active(&self.active)
    }

    /// Exact policy authority that must receive one terminal settlement.
    pub const fn active(&self) -> &AgentActiveModelCall {
        &self.active
    }

    /// Closed content-free terminal transport outcome.
    pub const fn outcome(&self) -> AgentProviderTransportOutcome {
        self.outcome
    }

    /// Moves terminal policy authority and transport outcome together.
    pub fn into_parts(self) -> (AgentActiveModelCall, AgentProviderTransportOutcome) {
        (self.active, self.outcome)
    }
}

impl fmt::Debug for AgentProviderTransportResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderTransportResult")
            .field("active", &self.active)
            .field("outcome", &self.outcome)
            .finish()
    }
}

/// One admitted and committed request that has not started network I/O.
#[must_use]
pub struct AgentProviderAttempt {
    client: Client,
    endpoint: Url,
    provider: AgentProviderKind,
    credential: AgentProviderAttemptCredential,
    committed: AgentCommittedProviderRequest,
    cancellation: AgentProviderCancellation,
    shutdown: AgentProviderCancellation,
    slot: Option<AgentProviderSlot>,
}

impl AgentProviderAttempt {
    /// Exact non-authorizing call correlation.
    pub fn call(&self) -> AgentProviderCallIdentity {
        self.committed.request().call()
    }

    /// Cancels after commitment without polling or transmitting the HTTP request.
    ///
    /// Provider usage is unknowable at this boundary and the returned authority
    /// must therefore use conservative unaccounted policy settlement.
    pub fn cancel_without_dispatch(mut self) -> AgentProviderTransportResult {
        let slot = self.slot.take();
        let (_, active) = self.committed.into_parts();
        finish_attempt(active, cancelled_failure(), slot)
    }

    /// Transmits once, decodes bounded SSE, and returns terminal policy authority.
    ///
    /// The callback sees only normalized nonempty batches. Returning `Cancel`
    /// wins before another body chunk is polled. The client has both redirects
    /// and automatic retries disabled, so this future can issue at most one POST.
    pub async fn execute<F>(mut self, mut consume: F) -> AgentProviderTransportResult
    where
        F: FnMut(AgentProviderStreamBatch) -> AgentProviderBatchDisposition,
    {
        let slot = self.slot.take();
        let (request, active) = self.committed.into_parts();
        let (call, config, endpoint_class, body) = request.into_transport_parts();
        if !call.matches_active(&active)
            || !provider_endpoint_matches(self.provider, endpoint_class)
        {
            return finish_attempt(active, protocol_failure(), slot);
        }
        let mut decoder = match AgentProviderStreamDecoder::try_new(call, &config) {
            Ok(decoder) => decoder,
            Err(_) => return finish_attempt(active, protocol_failure(), slot),
        };
        let credential = match self.credential.into_sensitive_header(self.provider) {
            Ok(credential) => credential,
            Err(_) => return finish_attempt(active, protocol_failure(), slot),
        };
        let request = provider_request(
            &self.client,
            self.endpoint.clone(),
            self.provider,
            credential,
            body,
        );
        let response = tokio::select! {
            biased;
            () = self.cancellation.cancelled() => {
                return finish_attempt(active, cancelled_failure(), slot);
            }
            () = self.shutdown.cancelled() => {
                return finish_attempt(active, cancelled_failure(), slot);
            }
            response = request.send() => response,
        };
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                let outcome = network_failure(&error);
                return finish_attempt(active, outcome, slot);
            }
        };
        if response.url() != &self.endpoint {
            return finish_attempt(active, protocol_failure(), slot);
        }
        if response.status() != StatusCode::OK {
            let failure = status_failure(response.status(), response.headers());
            return finish_attempt(active, AgentProviderTransportOutcome::Failed(failure), slot);
        }
        if !response_encoding_admitted(response.headers())
            || !response_content_type_admitted(response.headers())
        {
            return finish_attempt(active, protocol_failure(), slot);
        }

        let mut stream = response.bytes_stream();
        loop {
            let next = tokio::select! {
                biased;
                () = self.cancellation.cancelled() => {
                    return finish_attempt(active, cancelled_failure(), slot);
                }
                () = self.shutdown.cancelled() => {
                    return finish_attempt(active, cancelled_failure(), slot);
                }
                next = stream.try_next() => next,
            };
            let chunk = match next {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(error) => {
                    let outcome = network_failure(&error);
                    return finish_attempt(active, outcome, slot);
                }
            };
            let batch = match decoder.push(&chunk) {
                Ok(batch) => batch,
                Err(_) => return finish_attempt(active, protocol_failure(), slot),
            };
            if !batch.events().is_empty() && consume(batch) == AgentProviderBatchDisposition::Cancel
            {
                return finish_attempt(active, cancelled_failure(), slot);
            }
        }
        let outcome = match decoder.finish() {
            Ok(conclusion) => AgentProviderTransportOutcome::Stream(conclusion),
            Err(_) => protocol_failure(),
        };
        finish_attempt(active, outcome, slot)
    }
}

impl fmt::Debug for AgentProviderAttempt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderAttempt")
            .field("call", &self.call())
            .field("provider", &self.provider)
            .field("endpoint", &"[fixed]")
            .field("credential", &"[redacted]")
            .field("request_bytes", &self.committed.request().byte_len())
            .field("cancelled", &self.cancellation.is_cancelled())
            .finish()
    }
}

fn provider_request(
    client: &Client,
    endpoint: Url,
    provider: AgentProviderKind,
    credential: HeaderValue,
    body: Vec<u8>,
) -> reqwest::RequestBuilder {
    let request = client
        .post(endpoint)
        .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .header(ACCEPT, HeaderValue::from_static("text/event-stream"))
        .header(ACCEPT_ENCODING, HeaderValue::from_static("identity"))
        .header(CACHE_CONTROL, HeaderValue::from_static("no-store"))
        .body(body);
    match provider {
        AgentProviderKind::OpenAiResponses => request.header(AUTHORIZATION, credential),
        AgentProviderKind::AnthropicMessages => request
            .header(HeaderName::from_static("x-api-key"), credential)
            .header(
                HeaderName::from_static("anthropic-version"),
                HeaderValue::from_static(ANTHROPIC_VERSION),
            ),
    }
}

fn finish_attempt(
    active: AgentActiveModelCall,
    outcome: AgentProviderTransportOutcome,
    mut slot: Option<AgentProviderSlot>,
) -> AgentProviderTransportResult {
    if let Some(slot) = &mut slot {
        slot.mark_completed();
    }
    drop(slot);
    AgentProviderTransportResult { active, outcome }
}

fn cancelled_failure() -> AgentProviderTransportOutcome {
    AgentProviderTransportOutcome::Failed(AgentProviderFailure::new(
        AgentProviderFailureClass::Cancelled,
    ))
}

fn protocol_failure() -> AgentProviderTransportOutcome {
    AgentProviderTransportOutcome::Failed(AgentProviderFailure::new(
        AgentProviderFailureClass::Protocol,
    ))
}

fn network_failure(error: &reqwest::Error) -> AgentProviderTransportOutcome {
    let class = if error.is_timeout() {
        AgentProviderFailureClass::Timeout
    } else {
        AgentProviderFailureClass::Transport
    };
    AgentProviderTransportOutcome::Failed(AgentProviderFailure::new(class))
}

fn status_failure(status: StatusCode, headers: &HeaderMap) -> AgentProviderFailure {
    let class = match status.as_u16() {
        401 => AgentProviderFailureClass::Authentication,
        403 => AgentProviderFailureClass::Permission,
        404 => AgentProviderFailureClass::NotFound,
        408 => AgentProviderFailureClass::Timeout,
        409 => AgentProviderFailureClass::Conflict,
        429 => AgentProviderFailureClass::RateLimited,
        400..=499 => AgentProviderFailureClass::InvalidRequest,
        500 | 502 | 503 | 504 | 529 => AgentProviderFailureClass::Overloaded,
        300..=399 => AgentProviderFailureClass::Protocol,
        _ => AgentProviderFailureClass::Provider,
    };
    let retry_after = if matches!(
        class,
        AgentProviderFailureClass::RateLimited
            | AgentProviderFailureClass::Overloaded
            | AgentProviderFailureClass::Timeout
            | AgentProviderFailureClass::Transport
    ) {
        parse_retry_after(headers)
    } else {
        None
    };
    AgentProviderFailure::try_new(class, retry_after)
        .unwrap_or_else(|_| AgentProviderFailure::new(class))
}

fn parse_retry_after(headers: &HeaderMap) -> Option<AgentProviderRetryAfter> {
    let mut values = headers.get_all(RETRY_AFTER).iter();
    let bytes = match (values.next(), values.next()) {
        (Some(value), None) => value.as_bytes(),
        _ => return None,
    };
    if bytes.is_empty()
        || (bytes.len() > 1 && bytes[0] == b'0')
        || !bytes.iter().all(u8::is_ascii_digit)
    {
        return None;
    }
    let seconds = bytes.iter().try_fold(0_u64, |value, byte| {
        value
            .checked_mul(10)?
            .checked_add(u64::from(byte.saturating_sub(b'0')))
    })?;
    let millis = seconds.checked_mul(1_000)?;
    AgentProviderRetryAfter::try_from_millis(millis).ok()
}

fn response_encoding_admitted(headers: &HeaderMap) -> bool {
    let mut values = headers.get_all(CONTENT_ENCODING).iter();
    match (values.next(), values.next()) {
        (None, None) => true,
        (Some(value), None) => value.as_bytes().eq_ignore_ascii_case(b"identity"),
        _ => false,
    }
}

fn response_content_type_admitted(headers: &HeaderMap) -> bool {
    let mut values = headers.get_all(CONTENT_TYPE).iter();
    let value = match (values.next(), values.next()) {
        (Some(value), None) => match value.to_str() {
            Ok(value) => value,
            Err(_) => return false,
        },
        _ => return false,
    };
    let mut parts = value.split(';').map(str::trim);
    if !parts
        .next()
        .is_some_and(|mime| mime.eq_ignore_ascii_case("text/event-stream"))
    {
        return false;
    }
    match (parts.next(), parts.next()) {
        (None, None) => true,
        (Some(parameter), None) => parameter.eq_ignore_ascii_case("charset=utf-8"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc::{self, Receiver};
    use std::thread::{self, JoinHandle};
    use std::time::{Duration, Instant};

    use serde_json::json;
    use zephium_agentic::{
        decode_semantic_snapshot, encode_semantic_observation, AgentAccountAttestationId,
        AgentAccountScope, AgentContextAccountBinding, AgentEffectScope, AgentModelCallBudget,
        AgentModelCallId, AgentModelCallRequest, AgentModelCallSettlement,
        AgentModelCallUnaccountedSettlement, AgentPlanLeaseBinding, AgentPlanLeaseId,
        AgentPlanNodeAuthority, AgentPlanNodeId, AgentPlanNodeScope,
        AgentPreparedObservationRequest, AgentProviderCallConfig, AgentProviderModelRevision,
        AgentProviderObjective, AgentProviderStopReason, AgentProviderStreamBudget, AgentRunBudget,
        AgentRunManifest, AgentRunManifestId, AgentRunScope, ContextCapabilities,
        ContextCapability, ContextId, ContextIdentity, ContextKind, ContextOperationId,
        ContextRegistry, ContextRunId, ContextSettlement, FrameGeneration, FrameId,
        SemanticDecodeContext, SemanticEffectClass, SemanticFrameJoin, SemanticFrameTrust,
        SemanticInvocationId, SemanticModelEncodingBudget, SemanticObservationAssembler,
        SemanticObservationBudget, SemanticObservationId, SemanticObservationRequest,
        SemanticOrigin, SemanticSensitivity, SemanticSnapshotGeneration, SemanticTokenCountQuality,
        SemanticTokenCountRequirement, SemanticTokenCounter, SemanticTokenCounterError,
        SemanticTokenMeasurement, SemanticTokenizerRevision, SEMANTIC_WIRE_VERSION,
    };
    use zephium_core::ids::ProfileId;

    use super::*;

    const NOW: u64 = 2_000;
    const EXPIRES_AT: u64 = 100_000;

    struct FixedCounter {
        revision: SemanticTokenizerRevision,
        tokens: u32,
    }

    impl SemanticTokenCounter for FixedCounter {
        fn count_tokens(
            &self,
            input: &str,
        ) -> Result<SemanticTokenMeasurement, SemanticTokenCounterError> {
            if input.is_empty() {
                return Err(SemanticTokenCounterError::InvalidResult);
            }
            SemanticTokenMeasurement::try_new(
                self.revision.clone(),
                self.tokens,
                SemanticTokenCountQuality::ExactLocal,
            )
            .map_err(|_| SemanticTokenCounterError::InvalidResult)
        }
    }

    fn provider_fixture(
        provider: AgentProviderKind,
    ) -> (AgentRunPolicy, AgentProviderTransportInput) {
        let run = ContextRunId::generate();
        let profile = ProfileId::generate();
        let identity =
            ContextIdentity::new(ContextId::generate(), run, profile, ContextKind::Owned);
        let capabilities = ContextCapabilities::try_new(
            ContextKind::Owned,
            &[ContextCapability::Observe, ContextCapability::Act],
        )
        .expect("capabilities");
        let mut registry = ContextRegistry::new();
        registry.reserve(identity, capabilities).expect("reserve");
        let operation = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("begin context");
        registry
            .settle_construction(identity.id(), operation, ContextSettlement::Applied)
            .expect("construct context");
        let context = registry.join(identity.id()).expect("context join");
        let origin = SemanticOrigin::parse("https://fixture.example.test/page").expect("origin");
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            FrameGeneration::INITIAL,
            origin.clone(),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let invocation = SemanticInvocationId::new(11).expect("invocation");
        let wire = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": invocation.get(),
            "g": 11,
            "c": "complete",
            "n": [
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "paragraph", "t": "synthetic fixture marker"}
            ]
        }))
        .expect("wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                invocation,
                frame,
                SemanticSnapshotGeneration::new(11).expect("snapshot generation"),
            ),
            &wire,
        )
        .expect("snapshot");
        let observation = SemanticObservationAssembler::new(
            SemanticObservationRequest::initial(
                SemanticObservationId::new(1).expect("observation"),
                context,
                SemanticObservationBudget::try_new(8, 4_096, 1).expect("observation budget"),
            ),
            snapshot,
        )
        .expect("observation assembler")
        .finish()
        .expect("observation");
        let tokenizer =
            SemanticTokenizerRevision::try_new("transport-test-v1".to_owned()).expect("tokenizer");
        let payload = encode_semantic_observation(
            &observation,
            SemanticModelEncodingBudget::try_new(
                8_192,
                1_000,
                SemanticTokenCountRequirement::Exact,
            )
            .expect("encoding budget"),
        )
        .expect("encode")
        .admit(
            &FixedCounter {
                revision: tokenizer.clone(),
                tokens: 10,
            },
            &tokenizer,
        )
        .expect("payload");
        let objective = AgentProviderObjective::try_admit(
            "Summarize the synthetic marker".to_owned(),
            &FixedCounter {
                revision: tokenizer.clone(),
                tokens: 3,
            },
            &tokenizer,
        )
        .expect("objective");
        let effects = AgentEffectScope::try_new(&[SemanticEffectClass::Read]).expect("effects");
        let run_budget = AgentRunBudget::try_new(8, 1_000, 10_000, 1).expect("run budget");
        let scope = AgentRunScope::try_new(
            vec![profile],
            vec![AgentAccountScope::Anonymous],
            vec![origin.clone()],
            SemanticSensitivity::Public,
            effects,
            Vec::new(),
        )
        .expect("scope");
        let node = AgentPlanNodeId::generate();
        let authority = AgentPlanNodeAuthority::try_new(
            vec![profile],
            vec![AgentAccountScope::Anonymous],
            vec![origin],
            SemanticSensitivity::Public,
            effects,
        )
        .expect("authority");
        let manifest = AgentRunManifest::try_new(
            AgentRunManifestId::generate(),
            run,
            scope,
            run_budget,
            zephium_agentic::AgentPolicyInstant::from_millis(1_000),
            zephium_agentic::AgentPolicyInstant::from_millis(EXPIRES_AT),
            vec![AgentPlanNodeScope::new(
                node,
                authority,
                run_budget,
                zephium_agentic::AgentPolicyInstant::from_millis(EXPIRES_AT - 1),
            )],
        )
        .expect("manifest");
        let lease = AgentPlanLeaseId::generate();
        let mut policy =
            AgentRunPolicy::try_new(manifest, vec![AgentPlanLeaseBinding::new(lease, node)])
                .expect("policy");
        let account = AgentContextAccountBinding::new(
            AgentAccountAttestationId::generate(),
            context,
            AgentAccountScope::Anonymous,
            zephium_agentic::AgentPolicyInstant::from_millis(NOW - 1),
        );
        let request = AgentModelCallRequest::new(
            AgentModelCallId::new(1).expect("call"),
            lease,
            account,
            AgentModelCallBudget::try_new(8, 20, 100).expect("model budget"),
            zephium_agentic::AgentPolicyInstant::from_millis(NOW),
        );
        let config = AgentProviderCallConfig::try_new(
            provider,
            AgentProviderModelRevision::try_new(
                match provider {
                    AgentProviderKind::OpenAiResponses => "gpt-5.6-sol",
                    AgentProviderKind::AnthropicMessages => "claude-opus-5",
                }
                .to_owned(),
            )
            .expect("model"),
            tokenizer,
            5,
            20,
            AgentProviderStreamBudget::STANDARD,
        )
        .expect("provider config");
        let prepared = match provider {
            AgentProviderKind::OpenAiResponses => AgentPreparedObservationRequest::try_openai(
                &mut policy,
                request,
                &observation,
                payload,
                &objective,
                config,
            ),
            AgentProviderKind::AnthropicMessages => AgentPreparedObservationRequest::try_anthropic(
                &mut policy,
                request,
                &observation,
                payload,
                &objective,
                config,
            ),
        }
        .expect("prepared request");
        (policy, prepared.into_transport_input())
    }

    struct CapturedRequest {
        head: Vec<u8>,
        body: Vec<u8>,
    }

    struct OneShotServer {
        openai: Url,
        anthropic: Url,
        request: Receiver<Result<CapturedRequest, &'static str>>,
        thread: Option<JoinHandle<()>>,
    }

    struct DropConnectionServer {
        openai: Url,
        anthropic: Url,
        requests: Receiver<usize>,
        thread: Option<JoinHandle<()>>,
    }

    impl DropConnectionServer {
        fn spawn() -> Self {
            let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
            listener.set_nonblocking(true).expect("nonblocking");
            let address = listener.local_addr().expect("loopback address");
            let openai = Url::parse(&format!("http://{address}/v1/responses")).expect("openai URL");
            let anthropic =
                Url::parse(&format!("http://{address}/v1/messages")).expect("anthropic URL");
            let (sender, requests) = mpsc::sync_channel(1);
            let thread = thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_millis(500);
                let mut count = 0;
                while Instant::now() < deadline {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let _deadline = stream.set_read_timeout(Some(Duration::from_secs(2)));
                            if read_request(&mut stream).is_ok() {
                                count += 1;
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
                let _sent = sender.send(count);
            });
            Self {
                openai,
                anthropic,
                requests,
                thread: Some(thread),
            }
        }

        fn finish(mut self) -> usize {
            let requests = self
                .requests
                .recv_timeout(Duration::from_secs(2))
                .expect("request count");
            self.thread
                .take()
                .expect("server thread")
                .join()
                .expect("server join");
            requests
        }
    }

    impl OneShotServer {
        fn spawn(status: &str, response_headers: &[(&str, &str)], chunks: Vec<Vec<u8>>) -> Self {
            let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
            let address = listener.local_addr().expect("loopback address");
            let openai = Url::parse(&format!("http://{address}/v1/responses")).expect("openai URL");
            let anthropic =
                Url::parse(&format!("http://{address}/v1/messages")).expect("anthropic URL");
            let body_len = chunks.iter().map(Vec::len).sum::<usize>();
            let mut response_head =
                format!("HTTP/1.1 {status}\r\nContent-Length: {body_len}\r\nConnection: close\r\n");
            for (name, value) in response_headers {
                response_head.push_str(name);
                response_head.push_str(": ");
                response_head.push_str(value);
                response_head.push_str("\r\n");
            }
            response_head.push_str("\r\n");
            let response_head = response_head.into_bytes();
            let (sender, request) = mpsc::sync_channel(1);
            let thread =
                thread::spawn(move || {
                    let result = listener.accept().map_err(|_| "accept failed").and_then(
                        |(mut stream, _)| {
                            stream
                                .set_read_timeout(Some(Duration::from_secs(3)))
                                .map_err(|_| "read deadline failed")?;
                            let captured = read_request(&mut stream)?;
                            stream
                                .write_all(&response_head)
                                .map_err(|_| "response head failed")?;
                            for chunk in chunks {
                                stream
                                    .write_all(&chunk)
                                    .map_err(|_| "response body failed")?;
                                thread::sleep(Duration::from_millis(5));
                            }
                            Ok(captured)
                        },
                    );
                    let _sent = sender.send(result);
                });
            Self {
                openai,
                anthropic,
                request,
                thread: Some(thread),
            }
        }

        fn finish(mut self) -> CapturedRequest {
            let request = self
                .request
                .recv_timeout(Duration::from_secs(5))
                .expect("server result")
                .expect("valid request");
            self.thread
                .take()
                .expect("server thread")
                .join()
                .expect("server join");
            request
        }
    }

    fn read_request(stream: &mut TcpStream) -> Result<CapturedRequest, &'static str> {
        let mut bytes = Vec::with_capacity(8 * 1_024);
        let mut buffer = [0_u8; 8 * 1_024];
        let (header_end, content_length) = loop {
            let read = stream
                .read(&mut buffer)
                .map_err(|_| "request read failed")?;
            if read == 0 {
                return Err("request ended before headers");
            }
            bytes.extend_from_slice(&buffer[..read]);
            if bytes.len() > zephium_agentic::MAX_AGENT_PROVIDER_REQUEST_BYTES + 64 * 1_024 {
                return Err("request exceeded fixture bound");
            }
            if let Some(header_end) = find_bytes(&bytes, b"\r\n\r\n") {
                let head = std::str::from_utf8(&bytes[..header_end])
                    .map_err(|_| "request headers were not ASCII")?;
                let content_length = head
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .ok_or("content length missing")?;
                break (header_end + 4, content_length);
            }
        };
        while bytes.len() < header_end.saturating_add(content_length) {
            let read = stream
                .read(&mut buffer)
                .map_err(|_| "request body failed")?;
            if read == 0 {
                return Err("request body ended early");
            }
            bytes.extend_from_slice(&buffer[..read]);
        }
        if bytes.len() != header_end.saturating_add(content_length) {
            return Err("request body length changed");
        }
        Ok(CapturedRequest {
            head: bytes[..header_end].to_vec(),
            body: bytes[header_end..].to_vec(),
        })
    }

    fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    fn test_transport(server: &OneShotServer) -> AgentProviderTransport {
        AgentProviderTransport::try_new_loopback(
            AgentProviderTransportConfig::try_new(
                Duration::from_secs(5),
                Duration::from_secs(2),
                Duration::from_secs(2),
            )
            .expect("config"),
            server.openai.clone(),
            server.anthropic.clone(),
        )
        .expect("transport")
    }

    fn openai_success_stream() -> Vec<u8> {
        [
            "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\",\"model\":\"gpt-5.6-sol\"}}\n\n",
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
            "event: response.output_text.done\ndata: {\"type\":\"response.output_text.done\",\"text\":\"hello\"}\n\n",
            "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\",\"model\":\"gpt-5.6-sol\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\"}]}],\"usage\":{\"input_tokens\":17,\"output_tokens\":3,\"total_tokens\":20,\"input_tokens_details\":{\"cached_tokens\":0},\"output_tokens_details\":{\"reasoning_tokens\":0}}}}\n\n",
            "data: [DONE]\n\n",
        ]
        .concat()
        .into_bytes()
    }

    #[test]
    fn configuration_credentials_and_response_headers_are_strict_and_redacted() {
        assert_eq!(
            AgentProviderTransportConfig::try_new(
                Duration::ZERO,
                Duration::from_secs(1),
                Duration::from_secs(1)
            ),
            Err(AgentProviderTransportConfigError::Deadlines)
        );
        assert_eq!(
            AgentProviderTransportConfig::try_new(
                Duration::from_secs(5),
                Duration::from_secs(6),
                Duration::from_secs(1)
            ),
            Err(AgentProviderTransportConfigError::Deadlines)
        );
        for invalid in ["", "has space", "line\nbreak"] {
            assert_eq!(
                AgentProviderCredential::try_new(
                    AgentProviderKind::OpenAiResponses,
                    invalid.to_owned()
                )
                .expect_err("invalid credential"),
                AgentProviderCredentialError::Content
            );
        }
        let credential = AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "synthetic-openai-key".to_owned(),
        )
        .expect("credential");
        let debug = format!("{credential:?}");
        assert!(!debug.contains("synthetic-openai-key"));
        let header = sensitive_header(credential.provider, &credential.secret).expect("header");
        assert!(header.is_sensitive());
        assert!(!format!("{header:?}").contains("synthetic-openai-key"));

        let production = ProviderEndpoints::production().expect("production endpoints");
        assert!(production.https_only);
        assert_eq!(production.openai.as_str(), OPENAI_RESPONSES_URL);
        assert_eq!(production.anthropic.as_str(), ANTHROPIC_MESSAGES_URL);

        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream; charset=utf-8"),
        );
        assert!(response_content_type_admitted(&headers));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        assert!(!response_content_type_admitted(&headers));
        headers.insert(RETRY_AFTER, HeaderValue::from_static("2"));
        assert_eq!(
            parse_retry_after(&headers).map(AgentProviderRetryAfter::millis),
            Some(2_000)
        );
        headers.insert(RETRY_AFTER, HeaderValue::from_static("02"));
        assert_eq!(parse_retry_after(&headers), None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn openai_request_is_single_copy_single_post_and_normalized() {
        let stream = openai_success_stream();
        let split = stream.len() / 2;
        let server = OneShotServer::spawn(
            "200 OK",
            &[
                ("Content-Type", "text/event-stream; charset=utf-8"),
                ("Content-Encoding", "identity"),
            ],
            vec![stream[..split].to_vec(), stream[split..].to_vec()],
        );
        let transport = test_transport(&server);
        let credential = AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "synthetic-openai-key".to_owned(),
        )
        .expect("credential");
        let (mut policy, input) = provider_fixture(AgentProviderKind::OpenAiResponses);
        let attempt = transport
            .try_admit(
                input,
                &mut policy,
                &credential,
                AgentProviderCancellation::new(),
            )
            .expect("admission");
        let attempt_debug = format!("{attempt:?}");
        assert!(!attempt_debug.contains("synthetic-openai-key"));
        assert!(!attempt_debug.contains("synthetic fixture marker"));
        assert_eq!(transport.snapshot().expect("snapshot").active_attempts(), 1);

        let mut text = String::new();
        let result = attempt
            .execute(|batch| {
                for event in batch.into_events() {
                    match event {
                        zephium_agentic::AgentProviderStreamEvent::TextDelta(delta) => {
                            text.push_str(delta.as_str());
                        }
                        zephium_agentic::AgentProviderStreamEvent::ToolCall(_) => {
                            return AgentProviderBatchDisposition::Cancel;
                        }
                    }
                }
                AgentProviderBatchDisposition::Continue
            })
            .await;
        let AgentProviderTransportOutcome::Stream(AgentProviderStreamConclusion::Completed(
            completion,
        )) = result.outcome()
        else {
            panic!("normalized completion expected")
        };
        assert_eq!(text, "hello");
        assert_eq!(completion.stop(), AgentProviderStopReason::Completed);
        assert_eq!(completion.usage().total_tokens(), 20);
        let (active, _) = result.into_parts();
        policy
            .settle_model_call(active, AgentModelCallSettlement::Completed, 17, 3, 80)
            .expect("policy settlement");
        assert!(transport.snapshot().expect("snapshot").is_quiescent());

        let captured = server.finish();
        let head = std::str::from_utf8(&captured.head)
            .expect("request head")
            .to_ascii_lowercase();
        assert!(head.starts_with("post /v1/responses http/1.1\r\n"));
        assert!(head.contains("authorization: bearer synthetic-openai-key\r\n"));
        assert!(head.contains("accept: text/event-stream\r\n"));
        assert!(head.contains("accept-encoding: identity\r\n"));
        assert!(!head.contains("x-api-key"));
        let body: serde_json::Value = serde_json::from_slice(&captured.body).expect("request body");
        assert_eq!(body["stream"], true);
        assert_eq!(body["store"], false);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn anthropic_status_is_closed_retry_hint_and_never_reads_error_body() {
        let server = OneShotServer::spawn(
            "429 Too Many Requests",
            &[("Content-Type", "application/json"), ("Retry-After", "2")],
            vec![b"provider-authored-error-must-not-escape".to_vec()],
        );
        let transport = test_transport(&server);
        let credential = AgentProviderCredential::try_new(
            AgentProviderKind::AnthropicMessages,
            "synthetic-anthropic-key".to_owned(),
        )
        .expect("credential");
        let (mut policy, input) = provider_fixture(AgentProviderKind::AnthropicMessages);
        let attempt = transport
            .try_admit(
                input,
                &mut policy,
                &credential,
                AgentProviderCancellation::new(),
            )
            .expect("admission");
        let result = attempt
            .execute(|_| AgentProviderBatchDisposition::Continue)
            .await;
        let AgentProviderTransportOutcome::Failed(failure) = result.outcome() else {
            panic!("HTTP failure expected")
        };
        assert_eq!(failure.class(), AgentProviderFailureClass::RateLimited);
        assert_eq!(
            failure.retry_after().map(AgentProviderRetryAfter::millis),
            Some(2_000)
        );
        let rendered = format!("{result:?}");
        assert!(!rendered.contains("provider-authored-error"));
        let (active, _) = result.into_parts();
        policy
            .settle_model_call_unaccounted(
                active,
                AgentModelCallUnaccountedSettlement::ProviderFailed,
            )
            .expect("conservative settlement");

        let captured = server.finish();
        let head = std::str::from_utf8(&captured.head)
            .expect("request head")
            .to_ascii_lowercase();
        assert!(head.starts_with("post /v1/messages http/1.1\r\n"));
        assert!(head.contains("x-api-key: synthetic-anthropic-key\r\n"));
        assert!(head.contains("anthropic-version: 2023-06-01\r\n"));
        assert!(!head.contains("authorization:"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn low_level_connection_failure_is_never_automatically_retried() {
        let server = DropConnectionServer::spawn();
        let transport = AgentProviderTransport::try_new_loopback(
            AgentProviderTransportConfig::try_new(
                Duration::from_secs(3),
                Duration::from_secs(1),
                Duration::from_secs(1),
            )
            .expect("config"),
            server.openai.clone(),
            server.anthropic.clone(),
        )
        .expect("transport");
        let credential = AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "synthetic-openai-key".to_owned(),
        )
        .expect("credential");
        let (mut policy, input) = provider_fixture(AgentProviderKind::OpenAiResponses);
        let attempt = transport
            .try_admit(
                input,
                &mut policy,
                &credential,
                AgentProviderCancellation::new(),
            )
            .expect("admission");
        let result = attempt
            .execute(|_| AgentProviderBatchDisposition::Continue)
            .await;
        let AgentProviderTransportOutcome::Failed(failure) = result.outcome() else {
            panic!("transport failure expected")
        };
        assert_eq!(failure.class(), AgentProviderFailureClass::Transport);
        let (active, _) = result.into_parts();
        policy
            .settle_model_call_unaccounted(
                active,
                AgentModelCallUnaccountedSettlement::ProviderFailed,
            )
            .expect("conservative settlement");
        assert_eq!(server.finish(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn redirect_response_is_not_followed() {
        let server = OneShotServer::spawn(
            "307 Temporary Redirect",
            &[("Location", "http://127.0.0.1:9/must-not-follow")],
            Vec::new(),
        );
        let transport = test_transport(&server);
        let credential = AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "synthetic-openai-key".to_owned(),
        )
        .expect("credential");
        let (mut policy, input) = provider_fixture(AgentProviderKind::OpenAiResponses);
        let attempt = transport
            .try_admit(
                input,
                &mut policy,
                &credential,
                AgentProviderCancellation::new(),
            )
            .expect("admission");
        let result = attempt
            .execute(|_| AgentProviderBatchDisposition::Continue)
            .await;
        let AgentProviderTransportOutcome::Failed(failure) = result.outcome() else {
            panic!("protocol failure expected")
        };
        assert_eq!(failure.class(), AgentProviderFailureClass::Protocol);
        let (active, _) = result.into_parts();
        policy
            .settle_model_call_unaccounted(
                active,
                AgentModelCallUnaccountedSettlement::ProviderFailed,
            )
            .expect("conservative settlement");
        let captured = server.finish();
        let head = std::str::from_utf8(&captured.head).expect("request head");
        assert!(head.starts_with("POST /v1/responses HTTP/1.1\r\n"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancellation_capacity_duplicate_and_shutdown_are_exact() {
        let openai = Url::parse("http://127.0.0.1:9/v1/responses").expect("openai URL");
        let anthropic = Url::parse("http://127.0.0.1:9/v1/messages").expect("anthropic URL");
        let transport = AgentProviderTransport::try_new_loopback(
            AgentProviderTransportConfig::STANDARD,
            openai,
            anthropic,
        )
        .expect("transport");
        let credential = AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "synthetic-openai-key".to_owned(),
        )
        .expect("credential");

        let (mut cancelled_policy, cancelled_input) =
            provider_fixture(AgentProviderKind::OpenAiResponses);
        let cancelled = AgentProviderCancellation::new();
        cancelled.cancel();
        assert!(matches!(
            transport.try_admit(
                cancelled_input,
                &mut cancelled_policy,
                &credential,
                cancelled
            ),
            Err(AgentProviderAdmissionError::Cancelled)
        ));
        assert_eq!(cancelled_policy.pending_model_calls(), 0);

        let mut admitted = Vec::new();
        for _ in 0..MAX_AGENT_PROVIDER_TRANSPORT_CALLS {
            let (mut policy, input) = provider_fixture(AgentProviderKind::OpenAiResponses);
            let attempt = transport
                .try_admit(
                    input,
                    &mut policy,
                    &credential,
                    AgentProviderCancellation::new(),
                )
                .expect("bounded admission");
            admitted.push((attempt, policy));
        }
        assert_eq!(transport.snapshot().expect("snapshot").active_attempts(), 4);

        assert!(matches!(
            transport.reserve(admitted[0].0.call()),
            Err(ReserveError::Duplicate)
        ));

        let (mut excess_policy, excess_input) =
            provider_fixture(AgentProviderKind::OpenAiResponses);
        assert!(matches!(
            transport.try_admit(
                excess_input,
                &mut excess_policy,
                &credential,
                AgentProviderCancellation::new()
            ),
            Err(AgentProviderAdmissionError::Capacity)
        ));
        assert_eq!(excess_policy.pending_model_calls(), 0);

        for (attempt, mut policy) in admitted {
            let (active, outcome) = attempt.cancel_without_dispatch().into_parts();
            assert!(matches!(
                outcome,
                AgentProviderTransportOutcome::Failed(failure)
                    if failure.class() == AgentProviderFailureClass::Cancelled
            ));
            policy
                .settle_model_call_unaccounted(
                    active,
                    AgentModelCallUnaccountedSettlement::Cancelled,
                )
                .expect("cancel settlement");
        }
        assert!(transport.snapshot().expect("snapshot").is_quiescent());

        transport.seal();
        let (mut sealed_policy, sealed_input) =
            provider_fixture(AgentProviderKind::OpenAiResponses);
        assert!(matches!(
            transport.try_admit(
                sealed_input,
                &mut sealed_policy,
                &credential,
                AgentProviderCancellation::new()
            ),
            Err(AgentProviderAdmissionError::Sealed)
        ));
        assert_eq!(sealed_policy.pending_model_calls(), 0);
        assert!(transport.snapshot().expect("snapshot").is_sealed());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn post_commit_run_and_shutdown_cancellation_win_before_dispatch() {
        let transport = AgentProviderTransport::try_new_loopback(
            AgentProviderTransportConfig::STANDARD,
            Url::parse("http://127.0.0.1:9/v1/responses").expect("openai URL"),
            Url::parse("http://127.0.0.1:9/v1/messages").expect("anthropic URL"),
        )
        .expect("transport");
        let credential = AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "synthetic-openai-key".to_owned(),
        )
        .expect("credential");

        let cancellation = AgentProviderCancellation::new();
        let (mut policy, input) = provider_fixture(AgentProviderKind::OpenAiResponses);
        let attempt = transport
            .try_admit(input, &mut policy, &credential, cancellation.clone())
            .expect("admission");
        cancellation.cancel();
        let result = attempt
            .execute(|_| AgentProviderBatchDisposition::Continue)
            .await;
        let (active, outcome) = result.into_parts();
        assert!(matches!(
            outcome,
            AgentProviderTransportOutcome::Failed(failure)
                if failure.class() == AgentProviderFailureClass::Cancelled
        ));
        policy
            .settle_model_call_unaccounted(active, AgentModelCallUnaccountedSettlement::Cancelled)
            .expect("run cancellation settlement");

        let (mut shutdown_policy, shutdown_input) =
            provider_fixture(AgentProviderKind::OpenAiResponses);
        let attempt = transport
            .try_admit(
                shutdown_input,
                &mut shutdown_policy,
                &credential,
                AgentProviderCancellation::new(),
            )
            .expect("shutdown admission");
        transport.seal();
        let result = attempt
            .execute(|_| AgentProviderBatchDisposition::Continue)
            .await;
        let (active, outcome) = result.into_parts();
        assert!(matches!(
            outcome,
            AgentProviderTransportOutcome::Failed(failure)
                if failure.class() == AgentProviderFailureClass::Cancelled
        ));
        shutdown_policy
            .settle_model_call_unaccounted(active, AgentModelCallUnaccountedSettlement::Cancelled)
            .expect("shutdown cancellation settlement");
        let snapshot = transport.snapshot().expect("snapshot");
        assert!(snapshot.is_sealed());
        assert!(snapshot.is_quiescent());
    }

    #[test]
    fn abandoned_committed_attempt_fail_stops_the_shared_transport() {
        let transport = AgentProviderTransport::try_new_loopback(
            AgentProviderTransportConfig::STANDARD,
            Url::parse("http://127.0.0.1:9/v1/responses").expect("openai URL"),
            Url::parse("http://127.0.0.1:9/v1/messages").expect("anthropic URL"),
        )
        .expect("transport");
        let credential = AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "synthetic-openai-key".to_owned(),
        )
        .expect("credential");
        let (mut policy, input) = provider_fixture(AgentProviderKind::OpenAiResponses);
        let attempt = transport
            .try_admit(
                input,
                &mut policy,
                &credential,
                AgentProviderCancellation::new(),
            )
            .expect("admission");
        drop(attempt);
        let snapshot = transport.snapshot().expect("snapshot");
        assert!(snapshot.is_sealed());
        assert!(snapshot.is_quiescent());
        assert_eq!(policy.pending_model_calls(), 1);
    }

    #[test]
    fn retry_after_and_status_mapping_never_grant_retry_authority() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("3"));
        let rate_limit = status_failure(StatusCode::TOO_MANY_REQUESTS, &headers);
        assert_eq!(rate_limit.class(), AgentProviderFailureClass::RateLimited);
        assert_eq!(
            rate_limit
                .retry_after()
                .map(AgentProviderRetryAfter::millis),
            Some(3_000)
        );
        assert_eq!(
            rate_limit.retry_disposition(),
            zephium_agentic::AgentProviderRetryDisposition::PolicyMayRetry
        );
        let auth = status_failure(StatusCode::UNAUTHORIZED, &headers);
        assert_eq!(auth.class(), AgentProviderFailureClass::Authentication);
        assert_eq!(auth.retry_after(), None);
        assert_eq!(
            auth.retry_disposition(),
            zephium_agentic::AgentProviderRetryDisposition::Never
        );
    }

    #[test]
    fn request_reader_uses_a_bounded_exact_body() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener");
        let address = listener.local_addr().expect("address");
        let thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            read_request(&mut stream).expect("request")
        });
        let mut stream = TcpStream::connect(address).expect("connect");
        stream
            .write_all(b"POST / HTTP/1.1\r\nContent-Length: 4\r\n\r\ntest")
            .expect("write");
        let captured = thread.join().expect("join");
        assert_eq!(captured.body.len(), 4);
    }

    #[test]
    fn cancellation_wait_registration_has_no_lost_wakeup() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let cancellation = AgentProviderCancellation::new();
            cancellation.cancel();
            tokio::time::timeout(Duration::from_millis(50), cancellation.cancelled())
                .await
                .expect("sticky cancellation");
        });
    }
}
