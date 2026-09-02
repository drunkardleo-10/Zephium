//! Fixed-endpoint HTTPS transport for bounded agent model calls.
//!
//! The semantic core constructs and policy-admits every byte before this shell
//! sees it. This crate adds only provider credentials, exact HTTPS endpoints,
//! bounded concurrency, cancellation, response framing, and the existing
//! provider-neutral stream decoder. It exposes no arbitrary URL, generic HTTP
//! request, provider-native browser tool, raw response body, or automatic retry.

#![deny(missing_docs)]
#![deny(unsafe_code)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use futures_util::TryStreamExt;
use reqwest::header::{
    HeaderMap, HeaderName, HeaderValue, ACCEPT, ACCEPT_ENCODING, AUTHORIZATION, CACHE_CONTROL,
    CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, RETRY_AFTER,
};
use reqwest::redirect::Policy;
use reqwest::{Client, StatusCode, Url};
use thiserror::Error;
use tokio::sync::Notify;
use zephium_agentic::{
    AgentActiveModelCall, AgentCommittedProviderRequest, AgentModelCallReceipt,
    AgentModelCallSettlement, AgentModelCallUnaccountedSettlement, AgentModelUsageAccounting,
    AgentPolicyError, AgentProviderCallConfig, AgentProviderCallIdentity,
    AgentProviderContinuationSeed, AgentProviderEndpoint, AgentProviderFailure,
    AgentProviderFailureClass, AgentProviderInputEvidence, AgentProviderInputMetricReceipt,
    AgentProviderInputMetrics, AgentProviderKind, AgentProviderPricingError,
    AgentProviderPricingSchedule, AgentProviderRequestError, AgentProviderRetryAfter,
    AgentProviderStopReason, AgentProviderStreamBatch, AgentProviderStreamConclusion,
    AgentProviderStreamDecoder, AgentProviderTransportInput, AgentProviderUsage, AgentRunPolicy,
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
/// Maximum decoded response-header list bytes accepted from a provider.
pub const MAX_AGENT_PROVIDER_RESPONSE_HEADER_BYTES: u32 = 64 * 1_024;
/// Per-field accounting overhead from the HTTP/2 header-list size definition.
const HTTP_HEADER_FIELD_OVERHEAD_BYTES: usize = 32;
/// Maximum byte width of one provider-transport shutdown proof.
pub const MAX_AGENT_PROVIDER_TRANSPORT_SHUTDOWN_PROOF_BYTES: usize = 32;

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
        Ok(Self { secret })
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
    // `HeaderValue::from_bytes` copies this temporary zeroizing buffer into
    // its library-owned `Bytes`. Construct it only at dispatch: the pinned
    // HTTP type has no zeroizing drop, so an earlier validation construction
    // would create needless secret residue as well as a second allocation.
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
    /// Linearizes the last pre-disclosure cancellation check with commit.
    /// This mutex never crosses an await or network operation.
    commit_gate: Mutex<()>,
}

impl AgentProviderCancellation {
    /// Creates one initially-live cancellation authority without a task or timer.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AgentProviderCancellationInner {
                cancelled: AtomicBool::new(false),
                notify: Notify::new(),
                commit_gate: Mutex::new(()),
            }),
        }
    }

    /// Makes cancellation sticky and wakes all currently admitted attempts.
    pub fn cancel(&self) -> bool {
        // A disclosure commit holds this gate only for its synchronous policy
        // mutation. Once this lock is acquired, either that commit already
        // linearized or this cancellation will be visible to its final check.
        let _commit_gate = self
            .inner
            .commit_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.publish_cancellation()
    }

    fn publish_cancellation(&self) -> bool {
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

    /// Acquires the synchronous semantic-disclosure boundary. Poison means a
    /// prior commit panicked in an unwind-capable build; cancellation becomes
    /// sticky and no later disclosure may proceed through this authority.
    fn lock_commit_gate(&self) -> Option<MutexGuard<'_, ()>> {
        match self.inner.commit_gate.lock() {
            Ok(guard) => Some(guard),
            Err(poisoned) => {
                let guard = poisoned.into_inner();
                self.publish_cancellation();
                drop(guard);
                None
            }
        }
    }

    fn shares_authority(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
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
    drained: Notify,
    shutdown_waiting: AtomicBool,
}

struct AgentProviderTransportShutdownWaitGuard<'a> {
    waiting: &'a AtomicBool,
}

impl Drop for AgentProviderTransportShutdownWaitGuard<'_> {
    fn drop(&mut self) {
        self.waiting.store(false, Ordering::Release);
    }
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

    /// Whether no provider attempt is currently admitted.
    ///
    /// Idle is observational only: new admission remains possible until the
    /// transport is sealed and therefore cannot prove shutdown quiescence.
    pub const fn is_idle(self) -> bool {
        self.active == 0
    }

    /// Whether sticky shutdown admission and every admitted attempt are drained.
    pub const fn is_quiescent(self) -> bool {
        self.sealed && self.active == 0
    }
}

/// Constructor-closed evidence of one permanently sealed, idle transport.
///
/// The proof is process-local and non-authorizing. It does not prove policy
/// settlement, provider usage accounting, durable audit delivery, or native
/// browser drain.
#[must_use]
pub struct AgentProviderTransportShutdownProof {
    snapshot: AgentProviderTransportSnapshot,
}

impl AgentProviderTransportShutdownProof {
    /// Exact sealed, zero-active snapshot admitted by the constructor.
    pub const fn snapshot(&self) -> AgentProviderTransportSnapshot {
        self.snapshot
    }
}

impl fmt::Debug for AgentProviderTransportShutdownProof {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderTransportShutdownProof")
            .field("sealed", &self.snapshot.is_sealed())
            .field("active_attempts", &self.snapshot.active_attempts())
            .finish()
    }
}

const _: () = assert!(
    std::mem::size_of::<AgentProviderTransportShutdownProof>()
        <= MAX_AGENT_PROVIDER_TRANSPORT_SHUTDOWN_PROOF_BYTES
);

/// Closed refusal to prove terminal provider-transport drain.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentProviderTransportShutdownError {
    /// Shared transport state could not be read safely.
    #[error("agent provider transport shutdown state is unavailable")]
    State,
    /// Provider admission has not been permanently sealed.
    #[error("agent provider transport is not sealed for shutdown")]
    Unsealed,
    /// At least one admitted provider attempt still owns its slot.
    #[error("agent provider transport shutdown still has active attempts")]
    Pending,
    /// The caller-owned absolute shutdown deadline elapsed before drain.
    #[error("agent provider transport shutdown deadline elapsed")]
    Deadline,
    /// Another caller already owns the sole provider-drain wait.
    #[error("agent provider transport shutdown wait is already active")]
    WaiterActive,
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
                drained: Notify::new(),
                shutdown_waiting: AtomicBool::new(false),
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

    /// Proves that sticky shutdown admission and all provider attempts drained.
    ///
    /// Call [`Self::seal`] first, cancel or terminally settle every retained
    /// attempt, and retry this nonblocking check under the process shutdown
    /// deadline. Once returned, the fact remains true because transport
    /// admission cannot reopen after sealing.
    pub fn try_prove_shutdown(
        &self,
    ) -> Result<AgentProviderTransportShutdownProof, AgentProviderTransportShutdownError> {
        let snapshot = self
            .snapshot()
            .map_err(|_| AgentProviderTransportShutdownError::State)?;
        if !snapshot.is_sealed() {
            return Err(AgentProviderTransportShutdownError::Unsealed);
        }
        if !snapshot.is_idle() {
            return Err(AgentProviderTransportShutdownError::Pending);
        }
        Ok(AgentProviderTransportShutdownProof { snapshot })
    }

    /// Seals admission and asynchronously waits for every provider slot to drain.
    ///
    /// Sealing is synchronous, sticky, and happens before the deadline check,
    /// so dropping or timing out this future cannot reopen provider work. The
    /// returned future registers before sampling state, sleeps only on a slot-
    /// release notification or the caller's absolute monotonic deadline, and
    /// creates no worker, polling timer, socket, or provider request. At most
    /// one drain future may exist for the shared transport; dropping it
    /// synchronously releases only that wait admission, never the sticky seal.
    ///
    /// A successful return proves only transport-slot drain. The caller must
    /// independently retain and settle every terminal provider result's policy
    /// and usage authority.
    pub fn seal_and_prove_shutdown_until(
        &self,
        deadline: Instant,
    ) -> Result<
        impl std::future::Future<
                Output = Result<
                    AgentProviderTransportShutdownProof,
                    AgentProviderTransportShutdownError,
                >,
            > + Send
            + '_,
        AgentProviderTransportShutdownError,
    > {
        self.seal();
        self.shared
            .shutdown_waiting
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| AgentProviderTransportShutdownError::WaiterActive)?;
        let wait_guard = AgentProviderTransportShutdownWaitGuard {
            waiting: &self.shared.shutdown_waiting,
        };
        Ok(async move {
            let _wait_guard = wait_guard;
            let deadline_sleep = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline));
            tokio::pin!(deadline_sleep);

            loop {
                if Instant::now() >= deadline {
                    return Err(AgentProviderTransportShutdownError::Deadline);
                }

                let drained = self.shared.drained.notified();
                tokio::pin!(drained);
                drained.as_mut().enable();
                match self.try_prove_shutdown() {
                    Ok(proof) => return Ok(proof),
                    Err(AgentProviderTransportShutdownError::Pending) => {}
                    Err(error) => return Err(error),
                }

                tokio::select! {
                    () = &mut drained => {}
                    () = &mut deadline_sleep => {
                        return Err(AgentProviderTransportShutdownError::Deadline);
                    }
                }
            }
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
        let cancellation_gate = match cancellation.lock_commit_gate() {
            Some(gate) => gate,
            None => {
                let _outcome = input
                    .cancel(policy)
                    .map_err(AgentProviderAdmissionError::Settlement)?;
                drop(slot);
                return Err(AgentProviderAdmissionError::Cancelled);
            }
        };
        // The transport shutdown authority is private, but tolerate an exact
        // shared authority defensively so a future internal composition cannot
        // self-deadlock while acquiring the two commit gates.
        let shutdown_gate = if cancellation.shares_authority(&self.shared.shutdown) {
            None
        } else {
            match self.shared.shutdown.lock_commit_gate() {
                Some(gate) => Some(gate),
                None => {
                    drop(cancellation_gate);
                    self.seal();
                    let _outcome = input
                        .cancel(policy)
                        .map_err(AgentProviderAdmissionError::Settlement)?;
                    drop(slot);
                    return Err(AgentProviderAdmissionError::Cancelled);
                }
            }
        };
        if cancellation.is_cancelled() || self.shared.shutdown.is_cancelled() {
            drop(shutdown_gate);
            drop(cancellation_gate);
            let _outcome = input
                .cancel(policy)
                .map_err(AgentProviderAdmissionError::Settlement)?;
            drop(slot);
            return Err(AgentProviderAdmissionError::Cancelled);
        }
        let committed = match input.commit(policy) {
            Ok(committed) => committed,
            Err(error) => {
                drop(shutdown_gate);
                drop(cancellation_gate);
                drop(slot);
                return Err(AgentProviderAdmissionError::Settlement(error));
            }
        };
        slot.mark_committed();
        drop(shutdown_gate);
        drop(cancellation_gate);
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

    fn fail_stop(&self) {
        // Match the public shutdown order: make cancellation sticky before
        // publishing the admission seal, then recover poison only to retain
        // that terminal state. No transport clone may admit after this call.
        self.shared.shutdown.cancel();
        match self.shared.state.lock() {
            Ok(mut state) => state.sealed = true,
            Err(poisoned) => poisoned.into_inner().sealed = true,
        }
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
        let idle = state.active.is_empty();
        drop(state);
        if idle {
            self.shared.drained.notify_waiters();
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

/// Trustworthy provider-usage knowledge retained at the transport boundary.
///
/// This value separates a request proven not to have reached network dispatch
/// from an ambiguous send or response. A provider-reported value is normalized
/// by the fixed decoder; it still requires the exact trusted pricing revision
/// before policy can settle cost.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentProviderUsageKnowledge {
    /// The HTTP send future was never polled, so provider usage and cost are zero.
    ExactZeroBeforeDispatch,
    /// The fixed provider decoder supplied internally consistent token counters.
    ProviderReported(AgentProviderUsage),
    /// Network dispatch may have occurred and no trustworthy usage was returned.
    UnknownAfterDispatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentProviderDispatchEvidence {
    NotDispatched,
    MayHaveDispatched,
}

/// Terminal result joined to exact policy authority and pricing identity.
#[must_use]
pub struct AgentProviderTransportResult {
    active: AgentActiveModelCall,
    config: AgentProviderCallConfig,
    outcome: AgentProviderTransportOutcome,
    failure_dispatch: AgentProviderDispatchEvidence,
    continuation: Option<AgentProviderContinuationSeed>,
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

    /// Exact provider, model, tokenizer, pricing, and response bounds for the call.
    pub const fn config(&self) -> &AgentProviderCallConfig {
        &self.config
    }

    /// Closed content-free terminal transport outcome.
    pub const fn outcome(&self) -> AgentProviderTransportOutcome {
        self.outcome
    }

    /// Exact usage evidence that determines the only safe settlement route.
    pub const fn usage_knowledge(&self) -> AgentProviderUsageKnowledge {
        match self.outcome {
            AgentProviderTransportOutcome::Stream(AgentProviderStreamConclusion::Completed(
                completion,
            )) => AgentProviderUsageKnowledge::ProviderReported(completion.usage()),
            AgentProviderTransportOutcome::Stream(AgentProviderStreamConclusion::Failed(
                failure,
            )) => match failure.usage() {
                Some(usage) => AgentProviderUsageKnowledge::ProviderReported(usage),
                None => AgentProviderUsageKnowledge::UnknownAfterDispatch,
            },
            AgentProviderTransportOutcome::Failed(_) => match self.failure_dispatch {
                AgentProviderDispatchEvidence::NotDispatched => {
                    AgentProviderUsageKnowledge::ExactZeroBeforeDispatch
                }
                AgentProviderDispatchEvidence::MayHaveDispatched => {
                    AgentProviderUsageKnowledge::UnknownAfterDispatch
                }
            },
        }
    }

    /// Whether one exact committed observation/tool-only terminal retained a seed.
    ///
    /// The seed is still not continuation authority until the caller consumes
    /// it with the exact tool correlation emitted by this response.
    pub const fn has_continuation_seed(&self) -> bool {
        self.continuation.is_some()
    }

    /// Converts transport evidence into the sole safe policy-settlement route.
    ///
    /// Provider-reported usage retains the exact fixed provider/model/tokenizer
    /// configuration until trusted pricing supplies cost. Pre-dispatch failures
    /// settle exact zero; ambiguous sends consume the complete reservation.
    pub fn into_policy_settlement(self) -> AgentProviderPolicySettlement {
        self.into_policy_settlement_with_continuation().0
    }

    /// Separates terminal policy settlement from an optional one-shot seed.
    ///
    /// The seed is returned only for a valid completed response whose assistant
    /// output contains exactly one client tool call and no text or reasoning.
    /// Pricing and policy settlement remain mandatory and independent;
    /// retaining the seed cannot settle or replay the call.
    pub fn into_policy_settlement_with_continuation(
        self,
    ) -> (
        AgentProviderPolicySettlement,
        Option<AgentProviderContinuationSeed>,
    ) {
        let Self {
            active,
            config,
            outcome,
            failure_dispatch,
            continuation,
        } = self;
        let settlement = match outcome {
            AgentProviderTransportOutcome::Stream(conclusion) => match conclusion {
                AgentProviderStreamConclusion::Completed(completion) => {
                    AgentProviderPolicySettlement::PricingRequired(AgentProviderPricingSettlement {
                        active,
                        config,
                        conclusion,
                        settlement: AgentModelCallSettlement::Completed,
                        usage: completion.usage(),
                    })
                }
                AgentProviderStreamConclusion::Failed(failure) => match failure.usage() {
                    Some(usage) => AgentProviderPolicySettlement::PricingRequired(
                        AgentProviderPricingSettlement {
                            active,
                            config,
                            conclusion,
                            settlement: settlement_for_failure(failure.failure()),
                            usage,
                        },
                    ),
                    None => {
                        AgentProviderPolicySettlement::Immediate(AgentProviderImmediateSettlement {
                            active,
                            outcome: AgentProviderTransportOutcome::Stream(conclusion),
                            accounting: AgentProviderImmediateAccounting::ReservationCeiling(
                                unaccounted_settlement_for_failure(failure.failure()),
                            ),
                        })
                    }
                },
            },
            AgentProviderTransportOutcome::Failed(failure) => {
                let settlement = settlement_for_failure(failure);
                let accounting = match failure_dispatch {
                    AgentProviderDispatchEvidence::NotDispatched => {
                        AgentProviderImmediateAccounting::ExactZero(settlement)
                    }
                    AgentProviderDispatchEvidence::MayHaveDispatched => {
                        AgentProviderImmediateAccounting::ReservationCeiling(
                            unaccounted_settlement_for_failure(failure),
                        )
                    }
                };
                AgentProviderPolicySettlement::Immediate(AgentProviderImmediateSettlement {
                    active,
                    outcome: AgentProviderTransportOutcome::Failed(failure),
                    accounting,
                })
            }
        };
        (settlement, continuation)
    }
}

impl fmt::Debug for AgentProviderTransportResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderTransportResult")
            .field("active", &self.active)
            .field("config", &self.config)
            .field("outcome", &self.outcome)
            .field("usage", &self.usage_knowledge())
            .field("continuation", &self.continuation.is_some())
            .finish()
    }
}

/// Move-only safe settlement selected from terminal transport evidence.
#[must_use]
pub enum AgentProviderPolicySettlement {
    /// No pricing lookup is needed: usage is exact zero or unknowable.
    Immediate(AgentProviderImmediateSettlement),
    /// Exact normalized tokens require the matching trusted pricing revision.
    PricingRequired(AgentProviderPricingSettlement),
}

impl AgentProviderPolicySettlement {
    /// Exact non-authorizing call correlation retained with policy authority.
    pub fn call(&self) -> AgentProviderCallIdentity {
        match self {
            Self::Immediate(settlement) => settlement.call(),
            Self::PricingRequired(settlement) => settlement.call(),
        }
    }
}

impl fmt::Debug for AgentProviderPolicySettlement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Immediate(settlement) => formatter
                .debug_tuple("Immediate")
                .field(settlement)
                .finish(),
            Self::PricingRequired(settlement) => formatter
                .debug_tuple("PricingRequired")
                .field(settlement)
                .finish(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentProviderImmediateAccounting {
    ExactZero(AgentModelCallSettlement),
    ReservationCeiling(AgentModelCallUnaccountedSettlement),
}

/// Move-only terminal authority that can settle without a pricing lookup.
#[must_use]
pub struct AgentProviderImmediateSettlement {
    active: AgentActiveModelCall,
    outcome: AgentProviderTransportOutcome,
    accounting: AgentProviderImmediateAccounting,
}

impl AgentProviderImmediateSettlement {
    /// Exact non-authorizing call correlation.
    pub fn call(&self) -> AgentProviderCallIdentity {
        AgentProviderCallIdentity::from_active(&self.active)
    }

    /// Content-free terminal transport outcome retained for supervision.
    pub const fn outcome(&self) -> AgentProviderTransportOutcome {
        self.outcome
    }

    /// Completed, provider-failed, or cancelled policy terminal class.
    pub const fn settlement(&self) -> AgentModelCallSettlement {
        match self.accounting {
            AgentProviderImmediateAccounting::ExactZero(settlement) => settlement,
            AgentProviderImmediateAccounting::ReservationCeiling(settlement) => match settlement {
                AgentModelCallUnaccountedSettlement::ProviderFailed => {
                    AgentModelCallSettlement::ProviderFailed
                }
                AgentModelCallUnaccountedSettlement::Cancelled => {
                    AgentModelCallSettlement::Cancelled
                }
            },
        }
    }

    /// Exact-zero or reservation-ceiling accounting selected by transport proof.
    pub const fn usage_accounting(&self) -> AgentModelUsageAccounting {
        match self.accounting {
            AgentProviderImmediateAccounting::ExactZero(_) => AgentModelUsageAccounting::Exact,
            AgentProviderImmediateAccounting::ReservationCeiling(_) => {
                AgentModelUsageAccounting::ReservationCeiling
            }
        }
    }

    /// Consumes the exact active authority in the selected safe policy path.
    pub fn settle(
        self,
        policy: &mut AgentRunPolicy,
    ) -> Result<AgentModelCallReceipt, AgentPolicyError> {
        match self.accounting {
            AgentProviderImmediateAccounting::ExactZero(settlement) => {
                policy.settle_model_call(self.active, settlement, 0, 0, 0)
            }
            AgentProviderImmediateAccounting::ReservationCeiling(settlement) => {
                policy.settle_model_call_unaccounted(self.active, settlement)
            }
        }
    }
}

impl fmt::Debug for AgentProviderImmediateSettlement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderImmediateSettlement")
            .field("active", &self.active)
            .field("outcome", &self.outcome)
            .field("settlement", &self.settlement())
            .field("usage_accounting", &self.usage_accounting())
            .finish()
    }
}

/// Move-only exact-usage authority awaiting one trusted pricing result.
///
/// A pricing adapter must match this exact provider, model, tokenizer, and
/// normalized usage. The object retains active policy authority if lookup
/// cannot produce a price, preventing an unpriced completion fallback.
#[must_use]
pub struct AgentProviderPricingSettlement {
    active: AgentActiveModelCall,
    config: AgentProviderCallConfig,
    conclusion: AgentProviderStreamConclusion,
    settlement: AgentModelCallSettlement,
    usage: AgentProviderUsage,
}

impl AgentProviderPricingSettlement {
    /// Exact non-authorizing call correlation.
    pub fn call(&self) -> AgentProviderCallIdentity {
        AgentProviderCallIdentity::from_active(&self.active)
    }

    /// Exact provider/model/tokenizer/billing/pricing bounds used for pricing.
    pub const fn config(&self) -> &AgentProviderCallConfig {
        &self.config
    }

    /// Normalized content-free terminal conclusion retained for supervision.
    pub const fn conclusion(&self) -> AgentProviderStreamConclusion {
        self.conclusion
    }

    /// Completed, provider-failed, or cancelled policy terminal class.
    pub const fn settlement(&self) -> AgentModelCallSettlement {
        self.settlement
    }

    /// Exact normalized provider token counters to price and settle.
    pub const fn usage(&self) -> AgentProviderUsage {
        self.usage
    }

    /// Prices and settles through one exact immutable trusted schedule.
    ///
    /// Identity/range/arithmetic refusal returns this move-only settlement in
    /// the error so the caller can retry a corrected catalog lookup without
    /// losing active policy authority.
    pub fn settle(
        self,
        policy: &mut AgentRunPolicy,
        schedule: &AgentProviderPricingSchedule,
    ) -> Result<AgentModelCallReceipt, AgentProviderPricingSettlementError> {
        let priced = match schedule.try_price(&self.config, self.usage) {
            Ok(priced) => priced,
            Err(error) => {
                return Err(AgentProviderPricingSettlementError::Pricing {
                    error,
                    unsettled: Box::new(self),
                });
            }
        };
        policy
            .settle_model_call_priced(self.active, self.settlement, priced)
            .map_err(AgentProviderPricingSettlementError::Policy)
    }
}

impl fmt::Debug for AgentProviderPricingSettlement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderPricingSettlement")
            .field("active", &self.active)
            .field("config", &self.config)
            .field("conclusion", &self.conclusion)
            .field("settlement", &self.settlement)
            .field("usage", &self.usage)
            .finish()
    }
}

/// Failure while joining trusted pricing to exact terminal policy authority.
#[derive(Debug, Error)]
pub enum AgentProviderPricingSettlementError {
    /// Schedule identity, input range, or checked arithmetic refused pricing.
    #[error("agent provider terminal usage could not be priced")]
    Pricing {
        /// Closed content-free pricing refusal class.
        error: AgentProviderPricingError,
        /// Exact move-only terminal authority retained for corrected lookup.
        unsettled: Box<AgentProviderPricingSettlement>,
    },
    /// Policy consumed the terminal transition and failed stopped.
    #[error("agent provider priced policy settlement failed")]
    Policy(#[source] AgentPolicyError),
}

impl AgentProviderPricingSettlementError {
    /// Content-free pricing refusal when policy was not invoked.
    pub const fn pricing_error(&self) -> Option<AgentProviderPricingError> {
        match self {
            Self::Pricing { error, .. } => Some(*error),
            Self::Policy(_) => None,
        }
    }

    /// Recovers exact authority only when pricing refused before policy use.
    pub fn into_unsettled(self) -> Option<AgentProviderPricingSettlement> {
        match self {
            Self::Pricing { unsettled, .. } => Some(*unsettled),
            Self::Policy(_) => None,
        }
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

    /// Exact content-free semantic input proof committed at admission.
    ///
    /// Clone an observation proof before consuming the attempt when a future
    /// bounded continuation protocol needs an exact baseline. Losing this
    /// optional proof grants no authority and requires a fresh snapshot.
    pub const fn input_evidence(&self) -> &AgentProviderInputEvidence {
        self.committed.input_evidence()
    }

    /// Content-free metrics for the exact input that crossed disclosure commit.
    pub const fn input_metrics(&self) -> AgentProviderInputMetrics {
        self.committed.input_metrics()
    }

    /// Copyable exact identity and metrics proof for run-local qualification.
    pub fn input_metric_receipt(&self) -> AgentProviderInputMetricReceipt {
        self.committed.input_metric_receipt()
    }

    /// Cancels after commitment without polling or transmitting the HTTP request.
    ///
    /// Provider usage and cost are provably zero, while semantic disclosure
    /// taint remains committed because admission already crossed that boundary.
    pub fn cancel_without_dispatch(mut self) -> AgentProviderTransportResult {
        let slot = self.slot.take();
        let (request, input, _) = self.committed.into_parts();
        let (active, _) = input.into_parts();
        let (_, config, _, _) = request.into_transport_parts();
        finish_attempt(
            active,
            config,
            cancelled_failure(),
            AgentProviderDispatchEvidence::NotDispatched,
            None,
            slot,
        )
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
        let (request, input, continuation) = self.committed.into_parts();
        let (active, _) = input.into_parts();
        let (call, config, endpoint_class, body) = request.into_transport_parts();
        if !call.matches_active(&active)
            || !provider_endpoint_matches(self.provider, endpoint_class)
        {
            return finish_attempt(
                active,
                config,
                protocol_failure(),
                AgentProviderDispatchEvidence::NotDispatched,
                continuation,
                slot,
            );
        }
        // Avoid materializing an authentication header or request for an
        // attempt whose sticky cancellation was already observable when this
        // future began. The later check remains required to close the window
        // after request construction and before the send future is polled.
        if self.cancellation.is_cancelled() || self.shutdown.is_cancelled() {
            return finish_attempt(
                active,
                config,
                cancelled_failure(),
                AgentProviderDispatchEvidence::NotDispatched,
                continuation,
                slot,
            );
        }
        let mut decoder = match AgentProviderStreamDecoder::try_new(call, &config) {
            Ok(decoder) => decoder,
            Err(_) => {
                return finish_attempt(
                    active,
                    config,
                    protocol_failure(),
                    AgentProviderDispatchEvidence::NotDispatched,
                    continuation,
                    slot,
                );
            }
        };
        let credential = match self.credential.into_sensitive_header(self.provider) {
            Ok(credential) => credential,
            Err(_) => {
                return finish_attempt(
                    active,
                    config,
                    protocol_failure(),
                    AgentProviderDispatchEvidence::NotDispatched,
                    continuation,
                    slot,
                );
            }
        };
        let request = provider_request(
            &self.client,
            self.endpoint.clone(),
            self.provider,
            credential,
            body,
        );
        if self.cancellation.is_cancelled() || self.shutdown.is_cancelled() {
            return finish_attempt(
                active,
                config,
                cancelled_failure(),
                AgentProviderDispatchEvidence::NotDispatched,
                continuation,
                slot,
            );
        }
        let response = tokio::select! {
            biased;
            () = self.cancellation.cancelled() => {
                return finish_attempt(
                    active,
                    config,
                    cancelled_failure(),
                    AgentProviderDispatchEvidence::MayHaveDispatched,
                    continuation,
                    slot,
                );
            }
            () = self.shutdown.cancelled() => {
                return finish_attempt(
                    active,
                    config,
                    cancelled_failure(),
                    AgentProviderDispatchEvidence::MayHaveDispatched,
                    continuation,
                    slot,
                );
            }
            response = request.send() => response,
        };
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                let outcome = network_failure(&error);
                return finish_attempt(
                    active,
                    config,
                    outcome,
                    AgentProviderDispatchEvidence::MayHaveDispatched,
                    continuation,
                    slot,
                );
            }
        };
        if response.url() != &self.endpoint {
            return finish_attempt(
                active,
                config,
                protocol_failure(),
                AgentProviderDispatchEvidence::MayHaveDispatched,
                continuation,
                slot,
            );
        }
        if !response_headers_admitted(response.headers()) {
            return finish_attempt(
                active,
                config,
                protocol_failure(),
                AgentProviderDispatchEvidence::MayHaveDispatched,
                continuation,
                slot,
            );
        }
        if response.status() != StatusCode::OK {
            let failure = status_failure(response.status(), response.headers());
            return finish_attempt(
                active,
                config,
                AgentProviderTransportOutcome::Failed(failure),
                AgentProviderDispatchEvidence::MayHaveDispatched,
                continuation,
                slot,
            );
        }
        if !response_content_length_admitted(
            response.headers(),
            config.stream_budget().max_wire_bytes(),
        ) || !response_encoding_admitted(response.headers())
            || !response_content_type_admitted(response.headers())
        {
            return finish_attempt(
                active,
                config,
                protocol_failure(),
                AgentProviderDispatchEvidence::MayHaveDispatched,
                continuation,
                slot,
            );
        }

        let mut stream = response.bytes_stream();
        loop {
            let next = tokio::select! {
                biased;
                () = self.cancellation.cancelled() => {
                    return finish_attempt(
                        active,
                        config,
                        cancelled_failure(),
                        AgentProviderDispatchEvidence::MayHaveDispatched,
                        continuation,
                        slot,
                    );
                }
                () = self.shutdown.cancelled() => {
                    return finish_attempt(
                        active,
                        config,
                        cancelled_failure(),
                        AgentProviderDispatchEvidence::MayHaveDispatched,
                        continuation,
                        slot,
                    );
                }
                next = stream.try_next() => next,
            };
            let chunk = match next {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(error) => {
                    let outcome = network_failure(&error);
                    return finish_attempt(
                        active,
                        config,
                        outcome,
                        AgentProviderDispatchEvidence::MayHaveDispatched,
                        continuation,
                        slot,
                    );
                }
            };
            let batch = match decoder.push(&chunk) {
                Ok(batch) => batch,
                Err(_) => {
                    return finish_attempt(
                        active,
                        config,
                        protocol_failure(),
                        AgentProviderDispatchEvidence::MayHaveDispatched,
                        continuation,
                        slot,
                    );
                }
            };
            let disposition = if batch.events().is_empty() {
                AgentProviderBatchDisposition::Continue
            } else {
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| consume(batch))) {
                    Ok(disposition) => disposition,
                    Err(_) => {
                        if let Some(slot) = &slot {
                            slot.fail_stop();
                        }
                        return finish_attempt(
                            active,
                            config,
                            integration_failure(),
                            AgentProviderDispatchEvidence::MayHaveDispatched,
                            continuation,
                            slot,
                        );
                    }
                }
            };
            if disposition == AgentProviderBatchDisposition::Cancel {
                return finish_attempt(
                    active,
                    config,
                    cancelled_failure(),
                    AgentProviderDispatchEvidence::MayHaveDispatched,
                    continuation,
                    slot,
                );
            }
        }
        let outcome = match decoder.finish() {
            Ok(conclusion) => AgentProviderTransportOutcome::Stream(conclusion),
            Err(_) => protocol_failure(),
        };
        finish_attempt(
            active,
            config,
            outcome,
            AgentProviderDispatchEvidence::MayHaveDispatched,
            continuation,
            slot,
        )
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
    config: AgentProviderCallConfig,
    mut outcome: AgentProviderTransportOutcome,
    mut failure_dispatch: AgentProviderDispatchEvidence,
    mut continuation: Option<AgentProviderContinuationSeed>,
    mut slot: Option<AgentProviderSlot>,
) -> AgentProviderTransportResult {
    if let AgentProviderTransportOutcome::Stream(conclusion) = outcome {
        let call = match conclusion {
            AgentProviderStreamConclusion::Completed(completion) => completion.call(),
            AgentProviderStreamConclusion::Failed(failure) => failure.call(),
        };
        if !call.matches_active(&active) {
            outcome = protocol_failure();
            failure_dispatch = AgentProviderDispatchEvidence::MayHaveDispatched;
            continuation = None;
        }
    }
    if !matches!(
        outcome,
        AgentProviderTransportOutcome::Stream(AgentProviderStreamConclusion::Completed(
            completion
        )) if completion.stop() == AgentProviderStopReason::ToolCalls
            && completion.tool_only_output()
            && completion.stats().tool_calls() == 1
    ) {
        continuation = None;
    }
    if let Some(slot) = &mut slot {
        slot.mark_completed();
    }
    drop(slot);
    AgentProviderTransportResult {
        active,
        config,
        outcome,
        failure_dispatch,
        continuation,
    }
}

const fn settlement_for_failure(failure: AgentProviderFailure) -> AgentModelCallSettlement {
    if matches!(failure.class(), AgentProviderFailureClass::Cancelled) {
        AgentModelCallSettlement::Cancelled
    } else {
        AgentModelCallSettlement::ProviderFailed
    }
}

const fn unaccounted_settlement_for_failure(
    failure: AgentProviderFailure,
) -> AgentModelCallUnaccountedSettlement {
    if matches!(failure.class(), AgentProviderFailureClass::Cancelled) {
        AgentModelCallUnaccountedSettlement::Cancelled
    } else {
        AgentModelCallUnaccountedSettlement::ProviderFailed
    }
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

fn integration_failure() -> AgentProviderTransportOutcome {
    AgentProviderTransportOutcome::Failed(AgentProviderFailure::new(
        AgentProviderFailureClass::Integration,
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

/// Applies one protocol-neutral decoded header-list ceiling before any status,
/// retry hint, content-type, or body processing. Reqwest forwards the same
/// limit to HTTP/2 before decode; pinned reqwest does not expose Hyper's HTTP/1
/// receive-buffer ceiling, so this additional checked pass is also mandatory
/// after an HTTP/1 response has been parsed.
fn response_headers_admitted(headers: &HeaderMap) -> bool {
    headers
        .iter()
        .try_fold(0_usize, |total, (name, value)| {
            total
                .checked_add(name.as_str().len())?
                .checked_add(value.as_bytes().len())?
                .checked_add(HTTP_HEADER_FIELD_OVERHEAD_BYTES)
        })
        .is_some_and(
            |bytes| match usize::try_from(MAX_AGENT_PROVIDER_RESPONSE_HEADER_BYTES) {
                Ok(limit) => bytes <= limit,
                Err(_) => false,
            },
        )
}

fn response_encoding_admitted(headers: &HeaderMap) -> bool {
    let mut values = headers.get_all(CONTENT_ENCODING).iter();
    match (values.next(), values.next()) {
        (None, None) => true,
        (Some(value), None) => value.as_bytes().eq_ignore_ascii_case(b"identity"),
        _ => false,
    }
}

/// Refuses a declared success body larger than this exact call's wire budget
/// before the response body stream can allocate or yield any bytes. Streaming
/// responses may omit the field, but an ambiguous, non-canonical, or duplicate
/// declaration is never trusted as a resource bound.
fn response_content_length_admitted(headers: &HeaderMap, max_wire_bytes: u32) -> bool {
    let mut values = headers.get_all(CONTENT_LENGTH).iter();
    let bytes = match (values.next(), values.next()) {
        (None, None) => return true,
        (Some(value), None) => value.as_bytes(),
        _ => return false,
    };
    if bytes.is_empty()
        || (bytes.len() > 1 && bytes[0] == b'0')
        || !bytes.iter().all(u8::is_ascii_digit)
    {
        return false;
    }
    bytes
        .iter()
        .try_fold(0_u64, |value, byte| {
            value
                .checked_mul(10)?
                .checked_add(u64::from(byte.saturating_sub(b'0')))
        })
        .is_some_and(|declared| declared <= u64::from(max_wire_bytes))
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
    use std::sync::mpsc::{self, Receiver, SyncSender};
    use std::thread::{self, JoinHandle};
    use std::time::{Duration, Instant};

    use serde_json::json;
    use zephium_agentic::{
        compute_semantic_diff, decode_semantic_snapshot, encode_semantic_diff,
        encode_semantic_extraction_request, encode_semantic_observation, read_semantic_observation,
        AgentAccountAttestationId, AgentAccountScope, AgentContextAccountBinding, AgentEffectScope,
        AgentModelCallBudget, AgentModelCallId, AgentModelCallRequest, AgentModelCallSettlement,
        AgentPlanLeaseBinding, AgentPlanLeaseId, AgentPlanNodeAuthority, AgentPlanNodeId,
        AgentPlanNodeScope, AgentPreparedObservationRequest, AgentProviderCallConfig,
        AgentProviderDiffRequestDraft, AgentProviderExtractionRequestDraft,
        AgentProviderLocalInputTokenCounter, AgentProviderModelRevision, AgentProviderObjective,
        AgentProviderPricingError, AgentProviderPricingProfile, AgentProviderPricingRevision,
        AgentProviderPricingSchedule, AgentProviderStopReason, AgentProviderStreamBudget,
        AgentProviderTokenRates, AgentRunBudget, AgentRunManifest, AgentRunManifestId,
        AgentRunScope, ContextCapabilities, ContextCapability, ContextId, ContextIdentity,
        ContextKind, ContextOperationId, ContextRegistry, ContextRunId, ContextSettlement,
        FrameGeneration, FrameId, SemanticCaptureInstant, SemanticDecodeContext,
        SemanticDiffBudget, SemanticDiffOutcome, SemanticEffectClass,
        SemanticExtractionFieldSchema, SemanticExtractionSchema, SemanticExtractionSchemaId,
        SemanticFrameJoin, SemanticFrameTrust, SemanticInvocationId, SemanticModelEncodingBudget,
        SemanticObservation, SemanticObservationAssembler, SemanticObservationBudget,
        SemanticObservationId, SemanticObservationRequest, SemanticOrigin, SemanticReadAuthority,
        SemanticReadBudget, SemanticReadSensitivityLimit, SemanticSensitivity,
        SemanticSnapshotGeneration, SemanticTokenCountQuality, SemanticTokenCountRequirement,
        SemanticTokenCounter, SemanticTokenCounterError, SemanticTokenMeasurement,
        SemanticTokenizerRevision, SEMANTIC_WIRE_VERSION,
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

    struct FixedStructuredCounter {
        revision: SemanticTokenizerRevision,
        tokens: u32,
    }

    impl FixedStructuredCounter {
        fn count(
            &self,
            tokenizer: &SemanticTokenizerRevision,
            request_body: &[u8],
        ) -> Result<SemanticTokenMeasurement, SemanticTokenCounterError> {
            if tokenizer != &self.revision || request_body.is_empty() {
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

    impl AgentProviderLocalInputTokenCounter for FixedStructuredCounter {
        fn count_openai_responses_input(
            &self,
            model: &AgentProviderModelRevision,
            tokenizer: &SemanticTokenizerRevision,
            request_body: &[u8],
        ) -> Result<SemanticTokenMeasurement, SemanticTokenCounterError> {
            if model.as_str() != "gpt-5.6-sol" {
                return Err(SemanticTokenCounterError::Unavailable);
            }
            self.count(tokenizer, request_body)
        }

        fn count_anthropic_messages_input(
            &self,
            model: &AgentProviderModelRevision,
            tokenizer: &SemanticTokenizerRevision,
            request_body: &[u8],
        ) -> Result<SemanticTokenMeasurement, SemanticTokenCounterError> {
            if model.as_str() != "claude-opus-5" {
                return Err(SemanticTokenCounterError::Unavailable);
            }
            self.count(tokenizer, request_body)
        }
    }

    struct StatefulProviderFixture {
        policy: AgentRunPolicy,
        input: AgentProviderTransportInput,
        observation: SemanticObservation,
        account: AgentContextAccountBinding,
        lease: AgentPlanLeaseId,
        config: AgentProviderCallConfig,
    }

    fn provider_fixture(
        provider: AgentProviderKind,
    ) -> (AgentRunPolicy, AgentProviderTransportInput) {
        let fixture = stateful_provider_fixture(provider);
        (fixture.policy, fixture.input)
    }

    fn stateful_provider_fixture(provider: AgentProviderKind) -> StatefulProviderFixture {
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
            zephium_agentic::AgentProviderPricingProfile::try_new(
                zephium_agentic::AgentProviderPricingRevision::new(1).expect("pricing revision"),
                16_384,
            )
            .expect("pricing profile"),
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
                config.clone(),
            ),
            AgentProviderKind::AnthropicMessages => AgentPreparedObservationRequest::try_anthropic(
                &mut policy,
                request,
                &observation,
                payload,
                &objective,
                config.clone(),
            ),
        }
        .expect("prepared request");
        StatefulProviderFixture {
            policy,
            input: prepared.into_transport_input(),
            observation,
            account,
            lease,
            config,
        }
    }

    fn successor_observation(previous: &SemanticObservation) -> SemanticObservation {
        let invocation = SemanticInvocationId::new(12).expect("successor invocation");
        let wire = serde_json::to_vec(&json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": invocation.get(),
            "g": 12,
            "c": "complete",
            "n": [
                {"k": 1, "r": "document", "o": 16},
                {"k": 2, "p": 0, "r": "paragraph", "t": "updated synthetic marker"}
            ]
        }))
        .expect("successor wire");
        let snapshot = decode_semantic_snapshot(
            SemanticDecodeContext::new(
                invocation,
                previous.frames()[0].frame().clone(),
                SemanticSnapshotGeneration::new(12).expect("successor snapshot generation"),
            ),
            &wire,
        )
        .expect("successor snapshot");
        SemanticObservationAssembler::new(
            SemanticObservationRequest::initial(
                SemanticObservationId::new(2).expect("successor observation"),
                previous.request().context(),
                SemanticObservationBudget::try_new(8, 4_096, 1)
                    .expect("successor observation budget"),
            ),
            snapshot,
        )
        .expect("successor observation assembler")
        .finish()
        .expect("successor observation")
    }

    fn pricing_schedule(config: &AgentProviderCallConfig) -> AgentProviderPricingSchedule {
        let rates = match config.provider() {
            AgentProviderKind::OpenAiResponses => {
                AgentProviderTokenRates::try_new(1_000_000, 500_000, 1_250_000, 21_000_000)
                    .expect("OpenAI synthetic rates")
            }
            AgentProviderKind::AnthropicMessages => {
                AgentProviderTokenRates::try_new(1_000_000, 1_000_000, 1_000_000, 10_000_000)
                    .expect("Anthropic synthetic rates")
            }
        };
        AgentProviderPricingSchedule::new(
            config.provider(),
            config.model().clone(),
            config.tokenizer().clone(),
            config.pricing_profile(),
            rates,
        )
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

    struct StalledResponseServer {
        openai: Url,
        anthropic: Url,
        ready: Receiver<()>,
        release: SyncSender<()>,
        request: Receiver<Result<CapturedRequest, &'static str>>,
        thread: Option<JoinHandle<()>>,
    }

    impl StalledResponseServer {
        fn spawn() -> Self {
            let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
            let address = listener.local_addr().expect("loopback address");
            let openai = Url::parse(&format!("http://{address}/v1/responses")).expect("openai URL");
            let anthropic =
                Url::parse(&format!("http://{address}/v1/messages")).expect("anthropic URL");
            let (ready_sender, ready) = mpsc::sync_channel(1);
            let (release, release_receiver) = mpsc::sync_channel(1);
            let (request_sender, request) = mpsc::sync_channel(1);
            let thread =
                thread::spawn(move || {
                    let result = listener.accept().map_err(|_| "accept failed").and_then(
                        |(mut stream, _)| {
                            stream
                                .set_read_timeout(Some(Duration::from_secs(3)))
                                .map_err(|_| "read deadline failed")?;
                            let captured = read_request(&mut stream)?;
                            ready_sender.send(()).map_err(|_| "ready failed")?;
                            release_receiver
                                .recv_timeout(Duration::from_secs(3))
                                .map_err(|_| "release failed")?;
                            Ok(captured)
                        },
                    );
                    let _sent = request_sender.send(result);
                });
            Self {
                openai,
                anthropic,
                ready,
                release,
                request,
                thread: Some(thread),
            }
        }

        fn wait_until_request(&self) {
            self.ready
                .recv_timeout(Duration::from_secs(3))
                .expect("request dispatch");
        }

        fn finish(mut self) -> CapturedRequest {
            self.release.send(()).expect("release server");
            let request = self
                .request
                .recv_timeout(Duration::from_secs(3))
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
                            // Count transport attempts at accept. This fixture
                            // intentionally drops the connection, so request
                            // parsing is not a prerequisite for proving that
                            // the HTTP client opened another attempt.
                            count += 1;
                            let _deadline = stream.set_read_timeout(Some(Duration::from_secs(2)));
                            let _request = read_request(&mut stream);
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
            "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\",\"model\":\"gpt-5.6-sol\",\"service_tier\":\"default\"}}\n\n",
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
            "event: response.output_text.done\ndata: {\"type\":\"response.output_text.done\",\"text\":\"hello\"}\n\n",
            "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\",\"model\":\"gpt-5.6-sol\",\"service_tier\":\"default\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\"}]}],\"usage\":{\"input_tokens\":17,\"output_tokens\":3,\"total_tokens\":20,\"input_tokens_details\":{\"cached_tokens\":0},\"output_tokens_details\":{\"reasoning_tokens\":0}}}}\n\n",
            "data: [DONE]\n\n",
        ]
        .concat()
        .into_bytes()
    }

    fn openai_tool_stream() -> Vec<u8> {
        [
            "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_tool_1\",\"status\":\"in_progress\",\"model\":\"gpt-5.6-sol\",\"service_tier\":\"default\"}}\n\n",
            "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_tool_1\",\"call_id\":\"call_tool_1\",\"name\":\"back\",\"arguments\":\"\",\"status\":\"in_progress\"}}\n\n",
            "event: response.function_call_arguments.delta\ndata: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_tool_1\",\"delta\":\"{}\"}\n\n",
            "event: response.function_call_arguments.done\ndata: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_tool_1\",\"name\":\"back\",\"arguments\":\"{}\"}\n\n",
            "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_tool_1\",\"call_id\":\"call_tool_1\",\"name\":\"back\",\"arguments\":\"{}\",\"status\":\"completed\"}}\n\n",
            "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_tool_1\",\"status\":\"completed\",\"model\":\"gpt-5.6-sol\",\"service_tier\":\"default\",\"output\":[{\"type\":\"function_call\",\"id\":\"fc_tool_1\",\"call_id\":\"call_tool_1\",\"name\":\"back\",\"arguments\":\"{}\",\"status\":\"completed\"}],\"usage\":{\"input_tokens\":17,\"output_tokens\":3,\"total_tokens\":20,\"input_tokens_details\":{\"cached_tokens\":0},\"output_tokens_details\":{\"reasoning_tokens\":0}}}}\n\n",
            "data: [DONE]\n\n",
        ]
        .concat()
        .into_bytes()
    }

    fn anthropic_tool_stream() -> Vec<u8> {
        [
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_tool_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-opus-5\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":7,\"cache_creation_input_tokens\":3,\"cache_read_input_tokens\":5,\"output_tokens\":1,\"output_tokens_details\":{\"thinking_tokens\":0},\"service_tier\":\"standard\",\"inference_geo\":\"global\"}}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_back_1\",\"name\":\"back\",\"input\":{}}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":4}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        ]
        .concat()
        .into_bytes()
    }

    fn provider_tool_stream(provider: AgentProviderKind) -> Vec<u8> {
        match provider {
            AgentProviderKind::OpenAiResponses => openai_tool_stream(),
            AgentProviderKind::AnthropicMessages => anthropic_tool_stream(),
        }
    }

    fn sse_json(event: &str, payload: serde_json::Value) -> String {
        format!(
            "event: {event}\ndata: {}\n\n",
            serde_json::to_string(&payload).expect("SSE fixture JSON")
        )
    }

    fn openai_extract_tool_stream() -> Vec<u8> {
        let arguments = r#"{"schema_id":71}"#;
        [
            sse_json(
                "response.created",
                json!({"type":"response.created","response":{
                    "id":"resp_extract_tool_1","status":"in_progress",
                    "model":"gpt-5.6-sol","service_tier":"default"
                }}),
            ),
            sse_json(
                "response.output_item.added",
                json!({"type":"response.output_item.added","item":{
                    "type":"function_call","id":"fc_extract_tool_1",
                    "call_id":"call_extract_tool_1","name":"extract",
                    "arguments":"","status":"in_progress"
                }}),
            ),
            sse_json(
                "response.function_call_arguments.delta",
                json!({"type":"response.function_call_arguments.delta",
                    "item_id":"fc_extract_tool_1","delta":arguments}),
            ),
            sse_json(
                "response.function_call_arguments.done",
                json!({"type":"response.function_call_arguments.done",
                    "item_id":"fc_extract_tool_1","name":"extract",
                    "arguments":arguments}),
            ),
            sse_json(
                "response.output_item.done",
                json!({"type":"response.output_item.done","item":{
                    "type":"function_call","id":"fc_extract_tool_1",
                    "call_id":"call_extract_tool_1","name":"extract",
                    "arguments":arguments,"status":"completed"
                }}),
            ),
            sse_json(
                "response.completed",
                json!({"type":"response.completed","response":{
                    "id":"resp_extract_tool_1","status":"completed",
                    "model":"gpt-5.6-sol","service_tier":"default",
                    "output":[{"type":"function_call","id":"fc_extract_tool_1",
                        "call_id":"call_extract_tool_1","name":"extract",
                        "arguments":arguments,"status":"completed"}],
                    "usage":{"input_tokens":17,"output_tokens":3,"total_tokens":20,
                        "input_tokens_details":{"cached_tokens":0},
                        "output_tokens_details":{"reasoning_tokens":0}}
                }}),
            ),
            "data: [DONE]\n\n".to_owned(),
        ]
        .concat()
        .into_bytes()
    }

    fn anthropic_extract_tool_stream() -> Vec<u8> {
        [
            sse_json(
                "message_start",
                json!({"type":"message_start","message":{
                    "id":"msg_extract_tool_1","type":"message","role":"assistant",
                    "content":[],"model":"claude-opus-5","stop_reason":null,
                    "stop_sequence":null,"usage":{"input_tokens":7,
                        "cache_creation_input_tokens":3,"cache_read_input_tokens":5,
                        "output_tokens":1,"output_tokens_details":{"thinking_tokens":0},
                        "service_tier":"standard","inference_geo":"global"}
                }}),
            ),
            sse_json(
                "content_block_start",
                json!({"type":"content_block_start","index":0,"content_block":{
                    "type":"tool_use","id":"toolu_extract_1","name":"extract",
                    "input":{}
                }}),
            ),
            sse_json(
                "content_block_delta",
                json!({"type":"content_block_delta","index":0,"delta":{
                    "type":"input_json_delta","partial_json":"{\"schema_id\":71}"
                }}),
            ),
            sse_json(
                "content_block_stop",
                json!({"type":"content_block_stop","index":0}),
            ),
            sse_json(
                "message_delta",
                json!({"type":"message_delta","delta":{"stop_reason":"tool_use",
                    "stop_sequence":null},"usage":{"output_tokens":4}}),
            ),
            sse_json("message_stop", json!({"type":"message_stop"})),
        ]
        .concat()
        .into_bytes()
    }

    fn provider_extract_tool_stream(provider: AgentProviderKind) -> Vec<u8> {
        match provider {
            AgentProviderKind::OpenAiResponses => openai_extract_tool_stream(),
            AgentProviderKind::AnthropicMessages => anthropic_extract_tool_stream(),
        }
    }

    fn openai_extraction_output_stream(output: &str, input_tokens: u32) -> Vec<u8> {
        [
            sse_json(
                "response.created",
                json!({"type":"response.created","response":{
                    "id":"resp_extract_output_1","status":"in_progress",
                    "model":"gpt-5.6-sol","service_tier":"default"
                }}),
            ),
            sse_json(
                "response.output_text.delta",
                json!({"type":"response.output_text.delta","delta":output}),
            ),
            sse_json(
                "response.output_text.done",
                json!({"type":"response.output_text.done","text":output}),
            ),
            sse_json(
                "response.completed",
                json!({"type":"response.completed","response":{
                    "id":"resp_extract_output_1","status":"completed",
                    "model":"gpt-5.6-sol","service_tier":"default",
                    "output":[{"type":"message","content":[{"type":"output_text"}]}],
                    "usage":{"input_tokens":input_tokens,"output_tokens":3,
                        "total_tokens":input_tokens + 3,
                        "input_tokens_details":{"cached_tokens":0},
                        "output_tokens_details":{"reasoning_tokens":0}}
                }}),
            ),
            "data: [DONE]\n\n".to_owned(),
        ]
        .concat()
        .into_bytes()
    }

    fn anthropic_extraction_output_stream(output: &str, input_tokens: u32) -> Vec<u8> {
        [
            sse_json(
                "message_start",
                json!({"type":"message_start","message":{
                    "id":"msg_extract_output_1","type":"message","role":"assistant",
                    "content":[],"model":"claude-opus-5","stop_reason":null,
                    "stop_sequence":null,"usage":{"input_tokens":input_tokens,
                        "cache_creation_input_tokens":0,"cache_read_input_tokens":0,
                        "output_tokens":1,"output_tokens_details":{"thinking_tokens":0},
                        "service_tier":"standard","inference_geo":"global"}
                }}),
            ),
            sse_json(
                "content_block_start",
                json!({"type":"content_block_start","index":0,
                    "content_block":{"type":"text","text":""}}),
            ),
            sse_json(
                "content_block_delta",
                json!({"type":"content_block_delta","index":0,
                    "delta":{"type":"text_delta","text":output}}),
            ),
            sse_json(
                "content_block_stop",
                json!({"type":"content_block_stop","index":0}),
            ),
            sse_json(
                "message_delta",
                json!({"type":"message_delta","delta":{"stop_reason":"end_turn",
                    "stop_sequence":null},"usage":{"output_tokens":4}}),
            ),
            sse_json("message_stop", json!({"type":"message_stop"})),
        ]
        .concat()
        .into_bytes()
    }

    fn provider_extraction_output_stream(
        provider: AgentProviderKind,
        output: &str,
        input_tokens: u32,
    ) -> Vec<u8> {
        match provider {
            AgentProviderKind::OpenAiResponses => {
                openai_extraction_output_stream(output, input_tokens)
            }
            AgentProviderKind::AnthropicMessages => {
                anthropic_extraction_output_stream(output, input_tokens)
            }
        }
    }

    fn openai_mixed_tool_stream() -> Vec<u8> {
        [
            "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_mixed_1\",\"status\":\"in_progress\",\"model\":\"gpt-5.6-sol\",\"service_tier\":\"default\"}}\n\n",
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"working\"}\n\n",
            "event: response.output_text.done\ndata: {\"type\":\"response.output_text.done\",\"text\":\"working\"}\n\n",
            "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_mixed_1\",\"call_id\":\"call_mixed_1\",\"name\":\"back\",\"arguments\":\"\",\"status\":\"in_progress\"}}\n\n",
            "event: response.function_call_arguments.delta\ndata: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_mixed_1\",\"delta\":\"{}\"}\n\n",
            "event: response.function_call_arguments.done\ndata: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_mixed_1\",\"name\":\"back\",\"arguments\":\"{}\"}\n\n",
            "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_mixed_1\",\"call_id\":\"call_mixed_1\",\"name\":\"back\",\"arguments\":\"{}\",\"status\":\"completed\"}}\n\n",
            "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_mixed_1\",\"status\":\"completed\",\"model\":\"gpt-5.6-sol\",\"service_tier\":\"default\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\"}]},{\"type\":\"function_call\",\"id\":\"fc_mixed_1\",\"call_id\":\"call_mixed_1\",\"name\":\"back\",\"arguments\":\"{}\",\"status\":\"completed\"}],\"usage\":{\"input_tokens\":17,\"output_tokens\":3,\"total_tokens\":20,\"input_tokens_details\":{\"cached_tokens\":0},\"output_tokens_details\":{\"reasoning_tokens\":0}}}}\n\n",
            "data: [DONE]\n\n",
        ]
        .concat()
        .into_bytes()
    }

    fn anthropic_failure_stream() -> Vec<u8> {
        [
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-opus-5\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":7,\"cache_creation_input_tokens\":3,\"cache_read_input_tokens\":5,\"output_tokens\":1,\"output_tokens_details\":{\"thinking_tokens\":0},\"service_tier\":\"standard\",\"inference_geo\":\"global\"}}}\n\n",
            "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"provider-authored-sensitive-detail\"}}\n\n",
        ]
        .concat()
        .into_bytes()
    }

    fn anthropic_failure_without_usage_stream() -> Vec<u8> {
        "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"provider-authored-sensitive-detail\"}}\n\n"
            .as_bytes()
            .to_vec()
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

        let request = provider_request(
            &Client::new(),
            Url::parse(OPENAI_RESPONSES_URL).expect("fixed endpoint"),
            credential.provider,
            header,
            br#"{"input":"private request body marker"}"#.to_vec(),
        )
        .build()
        .expect("fixed request");
        let request_debug = format!("{request:?}");
        assert!(!request_debug.contains("synthetic-openai-key"));
        assert!(!request_debug.contains("private request body marker"));

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

        let header_limit = usize::try_from(MAX_AGENT_PROVIDER_RESPONSE_HEADER_BYTES)
            .expect("header limit fits host usize");
        let field_name = HeaderName::from_static("x-agent-boundary");
        let exact_value_len = header_limit
            .checked_sub(field_name.as_str().len())
            .and_then(|remaining| remaining.checked_sub(HTTP_HEADER_FIELD_OVERHEAD_BYTES))
            .expect("header limit exceeds field overhead");
        let exact_value = vec![b'a'; exact_value_len];
        let mut bounded_headers = HeaderMap::new();
        bounded_headers.insert(
            field_name.clone(),
            HeaderValue::from_bytes(&exact_value).expect("bounded header value"),
        );
        assert!(response_headers_admitted(&bounded_headers));
        bounded_headers.append(field_name, HeaderValue::from_static("b"));
        assert!(!response_headers_admitted(&bounded_headers));

        let mut declared_body = HeaderMap::new();
        assert!(response_content_length_admitted(&declared_body, 1_024));
        declared_body.insert(CONTENT_LENGTH, HeaderValue::from_static("1024"));
        assert!(response_content_length_admitted(&declared_body, 1_024));
        declared_body.insert(CONTENT_LENGTH, HeaderValue::from_static("1025"));
        assert!(!response_content_length_admitted(&declared_body, 1_024));
        for invalid in ["", "00", "01", "+1", "1_024", "18446744073709551616"] {
            declared_body.insert(
                CONTENT_LENGTH,
                HeaderValue::from_str(invalid).expect("syntactically valid header value"),
            );
            assert!(!response_content_length_admitted(&declared_body, 1_024));
        }
        declared_body.insert(CONTENT_LENGTH, HeaderValue::from_static("1"));
        declared_body.append(CONTENT_LENGTH, HeaderValue::from_static("1"));
        assert!(!response_content_length_admitted(&declared_body, 1_024));
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
        let continuation = attempt.input_evidence().clone();
        let input_metrics = attempt.input_metrics();
        let metric_receipt = attempt.input_metric_receipt();
        assert_eq!(metric_receipt.call(), attempt.call().call());
        assert_eq!(metric_receipt.metrics(), input_metrics);
        assert!(input_metrics.serialized_request_bytes() > 0);
        let zephium_agentic::AgentProviderSemanticInputStats::Observation(semantic_stats) =
            input_metrics.semantic()
        else {
            panic!("observation input metrics")
        };
        assert_eq!(semantic_stats.nodes(), 2);
        assert_eq!(
            input_metrics.semantic().disclosed_bytes(),
            semantic_stats.bytes()
        );
        assert_eq!(
            input_metrics
                .semantic_payload_tokens()
                .expect("semantic token count")
                .tokens(),
            10
        );
        assert_eq!(input_metrics.structured_input_tokens(), None);
        let metric_debug = format!("{metric_receipt:?}");
        assert!(metric_debug.contains("[redacted]"));
        assert!(!metric_debug.contains("synthetic fixture marker"));
        assert!(!metric_debug.contains("fixture.example.test"));
        let acknowledgement = continuation
            .observation_acknowledgement()
            .expect("observation continuation");
        assert_eq!(acknowledgement.observation().get(), 1);
        assert_eq!(acknowledgement.generation().get(), 1);
        assert!(continuation.read_receipt().is_none());
        let continuation_debug = format!("{continuation:?}");
        assert!(!continuation_debug.contains("synthetic fixture marker"));
        assert!(!continuation_debug.contains("fixture.example.test"));
        assert!(continuation_debug.contains("[redacted]"));
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
        assert_eq!(
            continuation
                .observation_acknowledgement()
                .expect("retained continuation")
                .observation()
                .get(),
            1
        );
        assert_eq!(
            result.usage_knowledge(),
            AgentProviderUsageKnowledge::ProviderReported(completion.usage())
        );
        assert!(!result.has_continuation_seed());
        let AgentProviderPolicySettlement::PricingRequired(settlement) =
            result.into_policy_settlement()
        else {
            panic!("trusted pricing must be required")
        };
        assert_eq!(
            settlement.config().provider(),
            AgentProviderKind::OpenAiResponses
        );
        assert_eq!(settlement.config().model().as_str(), "gpt-5.6-sol");
        assert_eq!(settlement.usage(), completion.usage());
        assert_eq!(settlement.settlement(), AgentModelCallSettlement::Completed);
        let settlement_debug = format!("{settlement:?}");
        assert!(!settlement_debug.contains("gpt-5.6-sol"));
        assert!(!settlement_debug.contains("transport-test-v1"));
        assert_eq!(policy.pending_model_calls(), 1);
        let wrong_profile = AgentProviderPricingProfile::try_new(
            AgentProviderPricingRevision::new(2).expect("pricing revision"),
            settlement.config().pricing_profile().max_input_tokens(),
        )
        .expect("wrong profile");
        let wrong_schedule = AgentProviderPricingSchedule::new(
            settlement.config().provider(),
            settlement.config().model().clone(),
            settlement.config().tokenizer().clone(),
            wrong_profile,
            AgentProviderTokenRates::try_new(1, 1, 1, 1).expect("rates"),
        );
        let error = settlement
            .settle(&mut policy, &wrong_schedule)
            .expect_err("mismatched catalog must retain authority");
        assert_eq!(
            error.pricing_error(),
            Some(AgentProviderPricingError::Identity)
        );
        let settlement = error.into_unsettled().expect("retained settlement");
        assert_eq!(policy.pending_model_calls(), 1);
        let schedule = pricing_schedule(settlement.config());
        let receipt = settlement
            .settle(&mut policy, &schedule)
            .expect("catalog-priced policy settlement");
        assert_eq!(
            receipt.usage_accounting(),
            AgentModelUsageAccounting::PricedCeiling
        );
        assert_eq!(receipt.input_tokens(), 17);
        assert_eq!(receipt.output_tokens(), 3);
        assert_eq!(receipt.cost_micro_usd(), 80);
        let attribution = receipt
            .pricing_attribution()
            .expect("checked pricing attribution");
        assert_eq!(attribution.provider(), AgentProviderKind::OpenAiResponses);
        assert_eq!(attribution.pricing_revision().value(), 1);
        assert_eq!(attribution.schedule_guard(), schedule.accounting_guard());
        assert_eq!(attribution.cached_input_tokens(), 0);
        assert_eq!(attribution.cache_write_input_tokens(), 0);
        assert_eq!(attribution.reasoning_output_tokens(), 0);
        assert!(transport.snapshot().expect("snapshot").is_idle());

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
        assert_eq!(body["service_tier"], "default");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn exact_single_tool_terminal_retains_one_shot_observation_seed() {
        let server = OneShotServer::spawn(
            "200 OK",
            &[
                ("Content-Type", "text/event-stream; charset=utf-8"),
                ("Content-Encoding", "identity"),
            ],
            vec![openai_tool_stream()],
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

        let mut correlation = None;
        let result = attempt
            .execute(|batch| {
                for event in batch.into_events() {
                    if let zephium_agentic::AgentProviderStreamEvent::ToolCall(tool) = event {
                        assert!(correlation.is_none(), "fixture emitted more than one tool");
                        correlation = Some(tool.into_continuation_parts().0);
                    }
                }
                AgentProviderBatchDisposition::Continue
            })
            .await;
        let AgentProviderTransportOutcome::Stream(AgentProviderStreamConclusion::Completed(
            completion,
        )) = result.outcome()
        else {
            panic!("normalized tool completion expected")
        };
        assert_eq!(completion.stop(), AgentProviderStopReason::ToolCalls);
        assert_eq!(completion.stats().tool_calls(), 1);
        assert!(result.has_continuation_seed());
        assert!(!format!("{result:?}").contains("resp_tool_1"));

        let (settlement, seed) = result.into_policy_settlement_with_continuation();
        let continuation = seed
            .expect("single tool observation seed")
            .join_terminal_tool(completion, correlation.expect("tool correlation"))
            .expect("exact terminal join");
        assert_eq!(continuation.prior_call(), completion.call());
        assert_eq!(continuation.provider(), AgentProviderKind::OpenAiResponses);
        assert_eq!(
            continuation.tool_kind(),
            zephium_agentic::AgentBrowserToolKind::Back
        );
        assert_eq!(continuation.argument_bytes(), 2);
        let continuation_debug = format!("{continuation:?}");
        assert!(!continuation_debug.contains("fc_tool_1"));
        assert!(!continuation_debug.contains("call_tool_1"));

        let AgentProviderPolicySettlement::PricingRequired(settlement) = settlement else {
            panic!("reported tool usage must be priced")
        };
        let schedule = pricing_schedule(settlement.config());
        let receipt = settlement
            .settle(&mut policy, &schedule)
            .expect("catalog-priced tool settlement");
        assert_eq!(receipt.input_tokens(), 17);
        assert_eq!(receipt.output_tokens(), 3);
        assert_eq!(policy.pending_model_calls(), 0);
        assert!(transport.snapshot().expect("snapshot").is_idle());

        let captured = server.finish();
        let body: serde_json::Value = serde_json::from_slice(&captured.body).expect("request body");
        assert_eq!(body["store"], false);
        assert!(body.get("previous_response_id").is_none());
    }

    async fn qualify_admitted_diff_transport(provider: AgentProviderKind) {
        let initial_server = OneShotServer::spawn(
            "200 OK",
            &[
                ("Content-Type", "text/event-stream; charset=utf-8"),
                ("Content-Encoding", "identity"),
            ],
            vec![provider_tool_stream(provider)],
        );
        let initial_transport = test_transport(&initial_server);
        let credential = AgentProviderCredential::try_new(
            provider,
            match provider {
                AgentProviderKind::OpenAiResponses => "synthetic-openai-key",
                AgentProviderKind::AnthropicMessages => "synthetic-anthropic-key",
            }
            .to_owned(),
        )
        .expect("credential");
        let mut fixture = stateful_provider_fixture(provider);
        let mut initial_correlation = None;
        let initial_result = initial_transport
            .try_admit(
                fixture.input,
                &mut fixture.policy,
                &credential,
                AgentProviderCancellation::new(),
            )
            .expect("initial admission")
            .execute(|batch| {
                for event in batch.into_events() {
                    if let zephium_agentic::AgentProviderStreamEvent::ToolCall(tool) = event {
                        assert!(
                            initial_correlation.is_none(),
                            "initial fixture emitted multiple tools"
                        );
                        initial_correlation = Some(tool.into_continuation_parts().0);
                    }
                }
                AgentProviderBatchDisposition::Continue
            })
            .await;
        let AgentProviderTransportOutcome::Stream(AgentProviderStreamConclusion::Completed(
            initial_completion,
        )) = initial_result.outcome()
        else {
            panic!("initial tool completion expected")
        };
        assert_eq!(
            initial_completion.stop(),
            AgentProviderStopReason::ToolCalls
        );
        let (initial_settlement, initial_seed) =
            initial_result.into_policy_settlement_with_continuation();
        let continuation = initial_seed
            .expect("initial continuation seed")
            .join_terminal_tool(
                initial_completion,
                initial_correlation.expect("initial tool correlation"),
            )
            .expect("initial exact terminal join");
        let AgentProviderPolicySettlement::PricingRequired(initial_settlement) = initial_settlement
        else {
            panic!("initial reported usage must be priced")
        };
        let initial_schedule = pricing_schedule(initial_settlement.config());
        initial_settlement
            .settle(&mut fixture.policy, &initial_schedule)
            .expect("initial catalog-priced settlement");
        assert_eq!(fixture.policy.pending_model_calls(), 0);
        assert!(initial_transport
            .snapshot()
            .expect("initial snapshot")
            .is_idle());
        initial_server.finish();

        let current = successor_observation(&fixture.observation);
        let diff = match compute_semantic_diff(
            &fixture.observation,
            continuation.baseline(),
            &current,
            SemanticDiffBudget::ACTION,
        ) {
            SemanticDiffOutcome::Diff(diff) => diff,
            SemanticDiffOutcome::FreshSnapshot(reason) => {
                panic!("unexpected fresh snapshot: {reason:?}")
            }
        };
        let diff_payload =
            encode_semantic_diff(&diff, SemanticModelEncodingBudget::ACTION_DIFF_EXACT)
                .expect("encode diff")
                .admit(
                    &FixedCounter {
                        revision: fixture.config.tokenizer().clone(),
                        tokens: 10,
                    },
                    fixture.config.tokenizer(),
                )
                .expect("admit diff");
        let request = AgentModelCallRequest::new(
            AgentModelCallId::new(2).expect("diff call"),
            fixture.lease,
            fixture.account,
            AgentModelCallBudget::try_new(20, 20, 100).expect("diff call budget"),
            zephium_agentic::AgentPolicyInstant::from_millis(NOW),
        );
        let draft = AgentProviderDiffRequestDraft::try_new(
            continuation
                .bind_diff_request(request, &fixture.config, &diff, diff_payload)
                .expect("bind diff request"),
        )
        .expect("encode stateless diff request");
        let structured_input_tokens = match provider {
            AgentProviderKind::OpenAiResponses => 17,
            AgentProviderKind::AnthropicMessages => 15,
        };
        let prepared = draft
            .try_prepare(
                &mut fixture.policy,
                request,
                &diff,
                &FixedStructuredCounter {
                    revision: fixture.config.tokenizer().clone(),
                    tokens: structured_input_tokens,
                },
            )
            .expect("admit exact whole diff request");
        assert_eq!(
            prepared.structured_input_measurement().tokens(),
            structured_input_tokens
        );
        assert_eq!(
            fixture.policy.accounting().reserved_model_tokens(),
            u64::from(structured_input_tokens) + 20
        );

        let diff_server = OneShotServer::spawn(
            "200 OK",
            &[
                ("Content-Type", "text/event-stream; charset=utf-8"),
                ("Content-Encoding", "identity"),
            ],
            vec![provider_tool_stream(provider)],
        );
        let diff_transport = test_transport(&diff_server);
        let diff_attempt = diff_transport
            .try_admit(
                prepared.into_transport_input(),
                &mut fixture.policy,
                &credential,
                AgentProviderCancellation::new(),
            )
            .expect("diff transport admission");
        let committed_acknowledgement = diff_attempt
            .input_evidence()
            .diff_receipt()
            .expect("committed diff receipt")
            .acknowledgement()
            .clone();
        assert_eq!(
            committed_acknowledgement.observation(),
            current.request().id()
        );
        assert_eq!(
            committed_acknowledgement.generation(),
            current.request().generation()
        );
        assert_eq!(
            committed_acknowledgement.context(),
            current.request().context()
        );
        assert_eq!(
            diff_attempt
                .input_evidence()
                .observation_acknowledgement()
                .expect("diff acknowledgement"),
            &committed_acknowledgement
        );
        assert_eq!(
            fixture.policy.accounting().reserved_model_tokens(),
            u64::from(structured_input_tokens) + 20
        );

        let mut diff_correlation = None;
        let diff_result = diff_attempt
            .execute(|batch| {
                for event in batch.into_events() {
                    if let zephium_agentic::AgentProviderStreamEvent::ToolCall(tool) = event {
                        assert!(
                            diff_correlation.is_none(),
                            "diff fixture emitted multiple tools"
                        );
                        diff_correlation = Some(tool.into_continuation_parts().0);
                    }
                }
                AgentProviderBatchDisposition::Continue
            })
            .await;
        let AgentProviderTransportOutcome::Stream(AgentProviderStreamConclusion::Completed(
            diff_completion,
        )) = diff_result.outcome()
        else {
            panic!("diff tool completion expected")
        };
        assert_eq!(diff_completion.stop(), AgentProviderStopReason::ToolCalls);
        assert!(diff_result.has_continuation_seed());
        assert!(!format!("{diff_result:?}").contains("updated synthetic marker"));
        let (diff_settlement, diff_seed) = diff_result.into_policy_settlement_with_continuation();
        let next_continuation = diff_seed
            .expect("diff continuation seed")
            .join_terminal_tool(
                diff_completion,
                diff_correlation.expect("diff tool correlation"),
            )
            .expect("diff exact terminal join");
        assert_eq!(next_continuation.baseline(), &committed_acknowledgement);
        let AgentProviderPolicySettlement::PricingRequired(diff_settlement) = diff_settlement
        else {
            panic!("diff reported usage must be priced")
        };
        let diff_schedule = pricing_schedule(diff_settlement.config());
        let receipt = diff_settlement
            .settle(&mut fixture.policy, &diff_schedule)
            .expect("diff catalog-priced settlement");
        assert_eq!(receipt.input_tokens(), u64::from(structured_input_tokens));
        assert_eq!(
            receipt.output_tokens(),
            match provider {
                AgentProviderKind::OpenAiResponses => 3,
                AgentProviderKind::AnthropicMessages => 4,
            }
        );
        assert_eq!(fixture.policy.pending_model_calls(), 0);
        assert!(diff_transport.snapshot().expect("diff snapshot").is_idle());

        let captured = diff_server.finish();
        let body: serde_json::Value =
            serde_json::from_slice(&captured.body).expect("diff request body");
        match provider {
            AgentProviderKind::OpenAiResponses => {
                assert_eq!(body["store"], false);
                assert!(body.get("previous_response_id").is_none());
                let input = body["input"].as_array().expect("fixed OpenAI input");
                assert_eq!(input.len(), 4);
                assert_eq!(input[2]["type"], "function_call");
                assert_eq!(input[3]["type"], "function_call_output");
                assert_eq!(input[2]["call_id"], input[3]["call_id"]);
            }
            AgentProviderKind::AnthropicMessages => {
                let messages = body["messages"]
                    .as_array()
                    .expect("fixed Anthropic messages");
                assert_eq!(messages.len(), 3);
                assert_eq!(messages[1]["role"], "assistant");
                assert_eq!(messages[1]["content"][0]["type"], "tool_use");
                assert_eq!(messages[2]["role"], "user");
                assert_eq!(messages[2]["content"][0]["type"], "tool_result");
                assert_eq!(
                    messages[1]["content"][0]["id"],
                    messages[2]["content"][0]["tool_use_id"]
                );
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn openai_diff_is_single_post_and_retains_the_next_exact_seed() {
        qualify_admitted_diff_transport(AgentProviderKind::OpenAiResponses).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn anthropic_diff_is_single_post_and_retains_the_next_exact_seed() {
        qualify_admitted_diff_transport(AgentProviderKind::AnthropicMessages).await;
    }

    async fn qualify_admitted_extraction_transport(provider: AgentProviderKind) {
        let initial_server = OneShotServer::spawn(
            "200 OK",
            &[
                ("Content-Type", "text/event-stream; charset=utf-8"),
                ("Content-Encoding", "identity"),
            ],
            vec![provider_extract_tool_stream(provider)],
        );
        let initial_transport = test_transport(&initial_server);
        let credential = AgentProviderCredential::try_new(
            provider,
            match provider {
                AgentProviderKind::OpenAiResponses => "synthetic-openai-key",
                AgentProviderKind::AnthropicMessages => "synthetic-anthropic-key",
            }
            .to_owned(),
        )
        .expect("credential");
        let mut fixture = stateful_provider_fixture(provider);
        let mut correlation = None;
        let initial_result = initial_transport
            .try_admit(
                fixture.input,
                &mut fixture.policy,
                &credential,
                AgentProviderCancellation::new(),
            )
            .expect("initial extraction admission")
            .execute(|batch| {
                for event in batch.into_events() {
                    if let zephium_agentic::AgentProviderStreamEvent::ToolCall(tool) = event {
                        assert!(correlation.is_none(), "multiple extraction proposals");
                        correlation = Some(tool.into_continuation_parts().0);
                    }
                }
                AgentProviderBatchDisposition::Continue
            })
            .await;
        let AgentProviderTransportOutcome::Stream(AgentProviderStreamConclusion::Completed(
            initial_completion,
        )) = initial_result.outcome()
        else {
            panic!("initial extraction tool completion expected: {initial_result:?}")
        };
        assert_eq!(
            initial_completion.stop(),
            AgentProviderStopReason::ToolCalls
        );
        let (initial_settlement, seed) = initial_result.into_policy_settlement_with_continuation();
        let continuation = seed
            .expect("extraction continuation seed")
            .join_terminal_tool(
                initial_completion,
                correlation.expect("extraction correlation"),
            )
            .expect("exact extraction terminal join");
        let AgentProviderPolicySettlement::PricingRequired(initial_settlement) = initial_settlement
        else {
            panic!("initial extraction usage must be priced")
        };
        let initial_schedule = pricing_schedule(initial_settlement.config());
        initial_settlement
            .settle(&mut fixture.policy, &initial_schedule)
            .expect("initial extraction settlement");
        initial_server.finish();

        let read = read_semantic_observation(
            &fixture.observation,
            SemanticReadAuthority::Initial,
            SemanticCaptureInstant::from_millis(NOW - 2),
            SemanticReadSensitivityLimit::PublicOnly,
            SemanticReadBudget::STANDARD,
        )
        .expect("extraction read");
        let schema = SemanticExtractionSchema::try_new(
            SemanticExtractionSchemaId::new(71).expect("schema id"),
            vec![
                SemanticExtractionFieldSchema::try_text("title".to_owned(), true, 64)
                    .expect("title field"),
            ],
        )
        .expect("extraction schema");
        let payload = encode_semantic_extraction_request(
            &schema,
            &read,
            SemanticModelEncodingBudget::try_new(
                32 * 1024,
                32 * 1024,
                SemanticTokenCountRequirement::Exact,
            )
            .expect("extraction budget"),
        )
        .expect("encode extraction input")
        .admit(
            &FixedCounter {
                revision: fixture.config.tokenizer().clone(),
                tokens: 10,
            },
            fixture.config.tokenizer(),
        )
        .expect("admit extraction input");
        let request = AgentModelCallRequest::new(
            AgentModelCallId::new(2).expect("extraction call"),
            fixture.lease,
            fixture.account,
            AgentModelCallBudget::try_new(30, 20, 100).expect("extraction call budget"),
            zephium_agentic::AgentPolicyInstant::from_millis(NOW),
        );
        let draft = AgentProviderExtractionRequestDraft::try_new(
            continuation
                .bind_extraction_request(request, &fixture.config, &schema, &read, payload)
                .expect("bind extraction request"),
        )
        .expect("encode extraction request");
        let structured_input_tokens = match provider {
            AgentProviderKind::OpenAiResponses => 17,
            AgentProviderKind::AnthropicMessages => 15,
        };
        let prepared = draft
            .try_prepare(
                &mut fixture.policy,
                request,
                &schema,
                &read,
                &FixedStructuredCounter {
                    revision: fixture.config.tokenizer().clone(),
                    tokens: structured_input_tokens,
                },
            )
            .expect("admit exact extraction request");
        let (input, output_binding) = prepared.into_transport_parts();
        let output = r#"{"v":1,"schema":71,"fields":[{"name":"title","value":{"k":"text","value":"synthetic fixture marker","sources":["@r1"]}}]}"#;
        let output_server = OneShotServer::spawn(
            "200 OK",
            &[
                ("Content-Type", "text/event-stream; charset=utf-8"),
                ("Content-Encoding", "identity"),
            ],
            vec![provider_extraction_output_stream(
                provider,
                output,
                structured_input_tokens,
            )],
        );
        let output_transport = test_transport(&output_server);
        let attempt = output_transport
            .try_admit(
                input,
                &mut fixture.policy,
                &credential,
                AgentProviderCancellation::new(),
            )
            .expect("extraction transport admission");
        assert!(attempt
            .input_evidence()
            .extraction_receipt()
            .is_some_and(|receipt| receipt.matches(&schema, &read)));
        let mut collector = output_binding
            .start(attempt.input_evidence())
            .expect("bind committed extraction input");
        let output_result = attempt
            .execute(|batch| {
                collector
                    .push_batch(batch)
                    .expect("collect extraction stream batch");
                AgentProviderBatchDisposition::Continue
            })
            .await;
        let AgentProviderTransportOutcome::Stream(AgentProviderStreamConclusion::Completed(
            output_completion,
        )) = output_result.outcome()
        else {
            panic!("completed extraction output expected")
        };
        assert_eq!(output_completion.stop(), AgentProviderStopReason::Completed);
        assert!(!output_result.has_continuation_seed());
        assert_eq!(collector.retained_bytes(), output.len());
        let extracted = collector
            .finish(
                AgentProviderStreamConclusion::Completed(output_completion),
                &schema,
                &read,
                SemanticReadSensitivityLimit::PublicOnly,
            )
            .expect("admit extracted output");
        assert_eq!(extracted.schema(), schema.id());
        assert_eq!(extracted.stats().fields(), 1);
        assert_eq!(extracted.stats().source_edges(), 1);
        let (settlement, seed) = output_result.into_policy_settlement_with_continuation();
        assert!(seed.is_none());
        let AgentProviderPolicySettlement::PricingRequired(settlement) = settlement else {
            panic!("extraction usage must be priced")
        };
        let schedule = pricing_schedule(settlement.config());
        let receipt = settlement
            .settle(&mut fixture.policy, &schedule)
            .expect("extraction pricing settlement");
        assert_eq!(receipt.input_tokens(), u64::from(structured_input_tokens));
        assert_eq!(fixture.policy.taints().len(), 1);
        assert_eq!(fixture.policy.pending_model_calls(), 0);
        assert!(output_transport
            .snapshot()
            .expect("extraction snapshot")
            .is_idle());

        let captured = output_server.finish();
        let body: serde_json::Value =
            serde_json::from_slice(&captured.body).expect("extraction request body");
        assert!(body.get("tools").is_none());
        assert!(body.get("tool_choice").is_none());
        let format_schema = match provider {
            AgentProviderKind::OpenAiResponses => {
                assert_eq!(body["text"]["format"]["type"], "json_schema");
                assert_eq!(body["text"]["format"]["strict"], true);
                assert_eq!(body["input"][2]["type"], "function_call");
                assert_eq!(body["input"][2]["name"], "extract");
                assert_eq!(body["input"][3]["type"], "function_call_output");
                &body["text"]["format"]["schema"]
            }
            AgentProviderKind::AnthropicMessages => {
                assert_eq!(body["output_config"]["format"]["type"], "json_schema");
                assert_eq!(body["messages"][1]["content"][0]["type"], "tool_use");
                assert_eq!(body["messages"][1]["content"][0]["name"], "extract");
                assert_eq!(body["messages"][2]["content"][0]["type"], "tool_result");
                &body["output_config"]["format"]["schema"]
            }
        };
        assert!(!serde_json::to_string(format_schema)
            .expect("fixed output schema")
            .contains("title"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn openai_extraction_is_constrained_single_post_and_terminal() {
        qualify_admitted_extraction_transport(AgentProviderKind::OpenAiResponses).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn anthropic_extraction_is_constrained_single_post_and_terminal() {
        qualify_admitted_extraction_transport(AgentProviderKind::AnthropicMessages).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn mixed_assistant_text_and_tool_terminal_destroys_continuation_seed() {
        let server = OneShotServer::spawn(
            "200 OK",
            &[
                ("Content-Type", "text/event-stream; charset=utf-8"),
                ("Content-Encoding", "identity"),
            ],
            vec![openai_mixed_tool_stream()],
        );
        let transport = test_transport(&server);
        let credential = AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "synthetic-openai-key".to_owned(),
        )
        .expect("credential");
        let (mut policy, input) = provider_fixture(AgentProviderKind::OpenAiResponses);
        let result = transport
            .try_admit(
                input,
                &mut policy,
                &credential,
                AgentProviderCancellation::new(),
            )
            .expect("admission")
            .execute(|_| AgentProviderBatchDisposition::Continue)
            .await;
        let AgentProviderTransportOutcome::Stream(AgentProviderStreamConclusion::Completed(
            completion,
        )) = result.outcome()
        else {
            panic!("mixed completion expected")
        };
        assert_eq!(completion.stop(), AgentProviderStopReason::ToolCalls);
        assert_eq!(completion.stats().output_text_bytes(), 7);
        assert!(!completion.tool_only_output());
        assert!(!result.has_continuation_seed());

        let (settlement, seed) = result.into_policy_settlement_with_continuation();
        assert!(seed.is_none());
        let AgentProviderPolicySettlement::PricingRequired(settlement) = settlement else {
            panic!("reported mixed-output usage must be priced")
        };
        let schedule = pricing_schedule(settlement.config());
        settlement
            .settle(&mut policy, &schedule)
            .expect("mixed-output settlement");
        assert_eq!(policy.pending_model_calls(), 0);
        assert!(transport.snapshot().expect("snapshot").is_idle());
        server.finish();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn terminal_failure_with_usage_retains_catalog_ceiling_accounting() {
        let server = OneShotServer::spawn(
            "200 OK",
            &[
                ("Content-Type", "text/event-stream"),
                ("Content-Encoding", "identity"),
            ],
            vec![anthropic_failure_stream()],
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
        let AgentProviderTransportOutcome::Stream(AgentProviderStreamConclusion::Failed(failure)) =
            result.outcome()
        else {
            panic!("normalized terminal failure expected")
        };
        assert_eq!(
            failure.failure().class(),
            AgentProviderFailureClass::Overloaded
        );
        let usage = failure.usage().expect("provider usage");
        assert_eq!(usage.input_tokens(), 15);
        assert_eq!(usage.output_tokens(), 1);
        assert_eq!(
            result.usage_knowledge(),
            AgentProviderUsageKnowledge::ProviderReported(usage)
        );
        assert!(!format!("{result:?}").contains("provider-authored-sensitive-detail"));

        let AgentProviderPolicySettlement::PricingRequired(settlement) =
            result.into_policy_settlement()
        else {
            panic!("reported failure usage must be priced")
        };
        assert_eq!(
            settlement.config().provider(),
            AgentProviderKind::AnthropicMessages
        );
        assert_eq!(
            settlement.settlement(),
            AgentModelCallSettlement::ProviderFailed
        );
        let schedule = pricing_schedule(settlement.config());
        let receipt = settlement
            .settle(&mut policy, &schedule)
            .expect("priced failure settlement");
        assert_eq!(
            receipt.usage_accounting(),
            AgentModelUsageAccounting::PricedCeiling
        );
        assert_eq!(receipt.input_tokens(), 15);
        assert_eq!(receipt.output_tokens(), 1);
        assert_eq!(receipt.cost_micro_usd(), 25);
        let attribution = receipt
            .pricing_attribution()
            .expect("checked pricing attribution");
        assert_eq!(attribution.provider(), AgentProviderKind::AnthropicMessages);
        assert_eq!(attribution.schedule_guard(), schedule.accounting_guard());
        assert_eq!(attribution.cached_input_tokens(), 5);
        assert_eq!(attribution.cache_write_input_tokens(), 3);
        assert_eq!(attribution.reasoning_output_tokens(), 0);
        assert!(transport.snapshot().expect("snapshot").is_idle());
        server.finish();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn terminal_failure_without_usage_consumes_the_reservation_ceiling() {
        let server = OneShotServer::spawn(
            "200 OK",
            &[("Content-Type", "text/event-stream")],
            vec![anthropic_failure_without_usage_stream()],
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
        assert!(matches!(
            result.outcome(),
            AgentProviderTransportOutcome::Stream(AgentProviderStreamConclusion::Failed(failure))
                if failure.usage().is_none()
                    && failure.failure().class() == AgentProviderFailureClass::Overloaded
        ));
        assert_eq!(
            result.usage_knowledge(),
            AgentProviderUsageKnowledge::UnknownAfterDispatch
        );
        let AgentProviderPolicySettlement::Immediate(settlement) = result.into_policy_settlement()
        else {
            panic!("missing terminal usage must settle conservatively")
        };
        let receipt = settlement
            .settle(&mut policy)
            .expect("reservation-ceiling settlement");
        assert_eq!(
            receipt.usage_accounting(),
            AgentModelUsageAccounting::ReservationCeiling
        );
        assert_eq!(receipt.input_tokens(), 18);
        assert_eq!(receipt.output_tokens(), 20);
        assert_eq!(receipt.cost_micro_usd(), 100);
        assert!(transport.snapshot().expect("snapshot").is_idle());
        server.finish();
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
        assert_eq!(
            result.usage_knowledge(),
            AgentProviderUsageKnowledge::UnknownAfterDispatch
        );
        let AgentProviderPolicySettlement::Immediate(settlement) = result.into_policy_settlement()
        else {
            panic!("unknown usage must settle immediately")
        };
        assert_eq!(
            settlement.usage_accounting(),
            AgentModelUsageAccounting::ReservationCeiling
        );
        assert_eq!(
            settlement.settlement(),
            AgentModelCallSettlement::ProviderFailed
        );
        let receipt = settlement
            .settle(&mut policy)
            .expect("conservative settlement");
        assert_eq!(receipt.input_tokens(), 18);
        assert_eq!(receipt.output_tokens(), 20);
        assert_eq!(receipt.cost_micro_usd(), 100);

        let captured = server.finish();
        let head = std::str::from_utf8(&captured.head)
            .expect("request head")
            .to_ascii_lowercase();
        assert!(head.starts_with("post /v1/messages http/1.1\r\n"));
        assert!(head.contains("x-api-key: synthetic-anthropic-key\r\n"));
        assert!(head.contains("anthropic-version: 2023-06-01\r\n"));
        assert!(!head.contains("authorization:"));
        let body: serde_json::Value = serde_json::from_slice(&captured.body).expect("request body");
        assert_eq!(body["service_tier"], "standard_only");
        assert_eq!(body["inference_geo"], "global");
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
        assert_eq!(
            result.usage_knowledge(),
            AgentProviderUsageKnowledge::UnknownAfterDispatch
        );
        let AgentProviderPolicySettlement::Immediate(settlement) = result.into_policy_settlement()
        else {
            panic!("unknown usage must settle immediately")
        };
        let receipt = settlement
            .settle(&mut policy)
            .expect("conservative settlement");
        assert_eq!(
            receipt.usage_accounting(),
            AgentModelUsageAccounting::ReservationCeiling
        );
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
        assert_eq!(
            result.usage_knowledge(),
            AgentProviderUsageKnowledge::UnknownAfterDispatch
        );
        let AgentProviderPolicySettlement::Immediate(settlement) = result.into_policy_settlement()
        else {
            panic!("unknown usage must settle immediately")
        };
        settlement
            .settle(&mut policy)
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
            let result = attempt.cancel_without_dispatch();
            assert_eq!(
                result.usage_knowledge(),
                AgentProviderUsageKnowledge::ExactZeroBeforeDispatch
            );
            assert!(matches!(
                result.outcome(),
                AgentProviderTransportOutcome::Failed(failure)
                    if failure.class() == AgentProviderFailureClass::Cancelled
            ));
            let AgentProviderPolicySettlement::Immediate(settlement) =
                result.into_policy_settlement()
            else {
                panic!("pre-dispatch cancellation must settle immediately")
            };
            assert_eq!(
                settlement.usage_accounting(),
                AgentModelUsageAccounting::Exact
            );
            assert_eq!(settlement.settlement(), AgentModelCallSettlement::Cancelled);
            let receipt = settlement.settle(&mut policy).expect("cancel settlement");
            assert_eq!(receipt.input_tokens(), 0);
            assert_eq!(receipt.output_tokens(), 0);
            assert_eq!(receipt.cost_micro_usd(), 0);
            assert_eq!(policy.pending_model_calls(), 0);
            assert_eq!(policy.taints().len(), 1);
            let accounting = policy.accounting();
            assert_eq!(accounting.consumed_operations(), 1);
            assert_eq!(accounting.reserved_operations(), 0);
            assert_eq!(accounting.consumed_model_tokens(), 0);
            assert_eq!(accounting.consumed_cost_micro_usd(), 0);
        }
        assert!(transport.snapshot().expect("snapshot").is_idle());

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

    #[test]
    fn cancellation_linearizes_after_an_inflight_disclosure_gate() {
        let cancellation = AgentProviderCancellation::new();
        let commit_gate = cancellation.lock_commit_gate().expect("commit gate");
        let (attempted_sender, attempted) = mpsc::sync_channel(0);
        let (completed_sender, completed) = mpsc::sync_channel(0);
        let cancelling = cancellation.clone();
        let cancellation_thread = thread::spawn(move || {
            attempted_sender.send(()).expect("announce cancellation");
            let first = cancelling.cancel();
            completed_sender
                .send(first)
                .expect("report cancellation result");
        });

        attempted.recv().expect("cancellation attempted");
        assert!(!cancellation.is_cancelled());
        assert!(matches!(
            completed.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        drop(commit_gate);
        assert!(completed
            .recv_timeout(Duration::from_secs(1))
            .expect("cancellation completed"));
        cancellation_thread.join().expect("cancellation thread");
        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn poisoned_disclosure_gate_becomes_sticky_cancellation() {
        let cancellation = AgentProviderCancellation::new();
        let poisoning = cancellation.clone();
        let _ = std::panic::catch_unwind(move || {
            let _gate = poisoning.lock_commit_gate().expect("commit gate");
            panic!("poison provider disclosure gate");
        });

        assert!(cancellation.lock_commit_gate().is_none());
        assert!(cancellation.is_cancelled());
        assert!(!cancellation.cancel());
    }

    #[test]
    fn shared_run_and_shutdown_authority_cannot_self_deadlock() {
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
                transport.shared.shutdown.clone(),
            )
            .expect("shared-authority admission");

        let settlement = attempt.cancel_without_dispatch().into_policy_settlement();
        let AgentProviderPolicySettlement::Immediate(settlement) = settlement else {
            panic!("unpolled attempt must settle immediately")
        };
        settlement.settle(&mut policy).expect("settlement");
        assert!(transport.snapshot().expect("snapshot").is_idle());
    }

    #[test]
    fn shutdown_proof_distinguishes_open_idle_from_sealed_drain() {
        let transport = AgentProviderTransport::try_new_loopback(
            AgentProviderTransportConfig::STANDARD,
            Url::parse("http://127.0.0.1:9/v1/responses").expect("openai URL"),
            Url::parse("http://127.0.0.1:9/v1/messages").expect("anthropic URL"),
        )
        .expect("transport");
        let shared_clone = transport.clone();
        let idle = transport.snapshot().expect("idle snapshot");
        assert!(idle.is_idle());
        assert!(!idle.is_quiescent());
        assert_eq!(
            transport
                .try_prove_shutdown()
                .expect_err("open admission cannot prove shutdown"),
            AgentProviderTransportShutdownError::Unsealed
        );

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
            .expect("attempt");
        transport.seal();
        let pending = transport.snapshot().expect("pending snapshot");
        assert!(pending.is_sealed());
        assert!(!pending.is_idle());
        assert!(!pending.is_quiescent());
        assert_eq!(
            transport
                .try_prove_shutdown()
                .expect_err("active attempt cannot prove shutdown"),
            AgentProviderTransportShutdownError::Pending
        );

        let result = attempt.cancel_without_dispatch();
        let AgentProviderPolicySettlement::Immediate(settlement) = result.into_policy_settlement()
        else {
            panic!("pre-dispatch shutdown cancellation must settle immediately")
        };
        settlement
            .settle(&mut policy)
            .expect("cancelled policy settlement");
        let proof = transport
            .try_prove_shutdown()
            .expect("sealed idle transport proof");
        assert!(proof.snapshot().is_quiescent());
        assert_eq!(proof.snapshot().active_attempts(), 0);
        assert_eq!(
            format!("{proof:?}"),
            "AgentProviderTransportShutdownProof { sealed: true, active_attempts: 0 }"
        );
        assert!(
            std::mem::size_of::<AgentProviderTransportShutdownProof>()
                <= MAX_AGENT_PROVIDER_TRANSPORT_SHUTDOWN_PROOF_BYTES
        );

        let (mut post_proof_policy, post_proof_input) =
            provider_fixture(AgentProviderKind::OpenAiResponses);
        assert!(matches!(
            shared_clone.try_admit(
                post_proof_input,
                &mut post_proof_policy,
                &credential,
                AgentProviderCancellation::new()
            ),
            Err(AgentProviderAdmissionError::Sealed)
        ));
        assert_eq!(post_proof_policy.pending_model_calls(), 0);
        assert!(proof.snapshot().is_quiescent());
    }

    #[test]
    fn shutdown_wait_seals_before_its_future_is_polled_or_retained() {
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
            .expect("attempt");

        let unpolled = transport
            .seal_and_prove_shutdown_until(
                Instant::now()
                    .checked_add(Duration::from_secs(1))
                    .expect("deadline"),
            )
            .expect("shutdown wait admission");
        assert!(transport.snapshot().expect("sealed snapshot").is_sealed());
        assert!(matches!(
            transport.seal_and_prove_shutdown_until(
                Instant::now()
                    .checked_add(Duration::from_secs(1))
                    .expect("deadline")
            ),
            Err(AgentProviderTransportShutdownError::WaiterActive)
        ));
        drop(unpolled);
        let readmitted_wait = transport
            .seal_and_prove_shutdown_until(
                Instant::now()
                    .checked_add(Duration::from_secs(1))
                    .expect("deadline"),
            )
            .expect("dropped wait releases bounded admission");
        drop(readmitted_wait);
        assert!(matches!(
            transport.try_prove_shutdown(),
            Err(AgentProviderTransportShutdownError::Pending)
        ));

        let result = attempt.cancel_without_dispatch();
        let AgentProviderPolicySettlement::Immediate(settlement) = result.into_policy_settlement()
        else {
            panic!("pre-dispatch shutdown cancellation must settle immediately")
        };
        settlement
            .settle(&mut policy)
            .expect("cancelled policy settlement");
        assert!(transport
            .try_prove_shutdown()
            .expect("cancelled future leaves sticky seal")
            .snapshot()
            .is_quiescent());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_wait_wakes_on_last_slot_and_deadline_refuses_pending_drain() {
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
        let (mut first_policy, first_input) = provider_fixture(AgentProviderKind::OpenAiResponses);
        let (mut second_policy, second_input) =
            provider_fixture(AgentProviderKind::OpenAiResponses);
        let first = transport
            .try_admit(
                first_input,
                &mut first_policy,
                &credential,
                AgentProviderCancellation::new(),
            )
            .expect("first attempt");
        let second = transport
            .try_admit(
                second_input,
                &mut second_policy,
                &credential,
                AgentProviderCancellation::new(),
            )
            .expect("second attempt");
        let shutdown = transport
            .seal_and_prove_shutdown_until(
                Instant::now()
                    .checked_add(Duration::from_secs(1))
                    .expect("deadline"),
            )
            .expect("shutdown wait admission");

        let settle_attempts = async move {
            tokio::task::yield_now().await;
            for (attempt, policy) in [(first, &mut first_policy), (second, &mut second_policy)] {
                let result = attempt.cancel_without_dispatch();
                let AgentProviderPolicySettlement::Immediate(settlement) =
                    result.into_policy_settlement()
                else {
                    panic!("pre-dispatch shutdown cancellation must settle immediately")
                };
                settlement
                    .settle(policy)
                    .expect("cancelled policy settlement");
            }
        };
        let (proof, ()) = tokio::join!(shutdown, settle_attempts);
        assert!(proof
            .expect("last slot wakes shutdown waiter")
            .snapshot()
            .is_quiescent());

        let pending = AgentProviderTransport::try_new_loopback(
            AgentProviderTransportConfig::STANDARD,
            Url::parse("http://127.0.0.1:9/v1/responses").expect("openai URL"),
            Url::parse("http://127.0.0.1:9/v1/messages").expect("anthropic URL"),
        )
        .expect("pending transport");
        let (mut pending_policy, pending_input) =
            provider_fixture(AgentProviderKind::OpenAiResponses);
        let pending_attempt = pending
            .try_admit(
                pending_input,
                &mut pending_policy,
                &credential,
                AgentProviderCancellation::new(),
            )
            .expect("pending attempt");
        assert_eq!(
            pending
                .seal_and_prove_shutdown_until(Instant::now())
                .expect("shutdown wait admission")
                .await
                .expect_err("elapsed deadline cannot prove pending drain"),
            AgentProviderTransportShutdownError::Deadline
        );
        assert!(pending.snapshot().expect("pending snapshot").is_sealed());
        let result = pending_attempt.cancel_without_dispatch();
        let AgentProviderPolicySettlement::Immediate(settlement) = result.into_policy_settlement()
        else {
            panic!("pre-dispatch shutdown cancellation must settle immediately")
        };
        settlement
            .settle(&mut pending_policy)
            .expect("cancelled pending settlement");
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
        assert_eq!(
            result.usage_knowledge(),
            AgentProviderUsageKnowledge::ExactZeroBeforeDispatch
        );
        assert!(matches!(
            result.outcome(),
            AgentProviderTransportOutcome::Failed(failure)
                if failure.class() == AgentProviderFailureClass::Cancelled
        ));
        let AgentProviderPolicySettlement::Immediate(settlement) = result.into_policy_settlement()
        else {
            panic!("sticky cancellation must settle immediately")
        };
        let receipt = settlement
            .settle(&mut policy)
            .expect("run cancellation settlement");
        assert_eq!(receipt.usage_accounting(), AgentModelUsageAccounting::Exact);
        assert_eq!(receipt.input_tokens(), 0);
        assert_eq!(receipt.output_tokens(), 0);
        assert_eq!(receipt.cost_micro_usd(), 0);

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
        assert_eq!(
            result.usage_knowledge(),
            AgentProviderUsageKnowledge::ExactZeroBeforeDispatch
        );
        assert!(matches!(
            result.outcome(),
            AgentProviderTransportOutcome::Failed(failure)
                if failure.class() == AgentProviderFailureClass::Cancelled
        ));
        let AgentProviderPolicySettlement::Immediate(settlement) = result.into_policy_settlement()
        else {
            panic!("sticky shutdown must settle immediately")
        };
        settlement
            .settle(&mut shutdown_policy)
            .expect("shutdown cancellation settlement");
        let snapshot = transport.snapshot().expect("snapshot");
        assert!(snapshot.is_sealed());
        assert!(snapshot.is_quiescent());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn post_dispatch_cancellation_consumes_the_reservation_ceiling() {
        let server = StalledResponseServer::spawn();
        let transport = AgentProviderTransport::try_new_loopback(
            AgentProviderTransportConfig::try_new(
                Duration::from_secs(5),
                Duration::from_secs(2),
                Duration::from_secs(2),
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
        let cancellation = AgentProviderCancellation::new();
        let (mut policy, input) = provider_fixture(AgentProviderKind::OpenAiResponses);
        let attempt = transport
            .try_admit(input, &mut policy, &credential, cancellation.clone())
            .expect("admission");
        let task = tokio::spawn(async move {
            attempt
                .execute(|_| AgentProviderBatchDisposition::Continue)
                .await
        });

        server.wait_until_request();
        cancellation.cancel();
        let result = task.await.expect("transport task");
        assert_eq!(
            result.usage_knowledge(),
            AgentProviderUsageKnowledge::UnknownAfterDispatch
        );
        assert!(matches!(
            result.outcome(),
            AgentProviderTransportOutcome::Failed(failure)
                if failure.class() == AgentProviderFailureClass::Cancelled
        ));
        let AgentProviderPolicySettlement::Immediate(settlement) = result.into_policy_settlement()
        else {
            panic!("ambiguous cancellation must settle immediately")
        };
        assert_eq!(
            settlement.usage_accounting(),
            AgentModelUsageAccounting::ReservationCeiling
        );
        assert_eq!(settlement.settlement(), AgentModelCallSettlement::Cancelled);
        let receipt = settlement
            .settle(&mut policy)
            .expect("conservative cancellation settlement");
        assert_eq!(receipt.input_tokens(), 18);
        assert_eq!(receipt.output_tokens(), 20);
        assert_eq!(receipt.cost_micro_usd(), 100);
        assert!(transport.snapshot().expect("snapshot").is_idle());

        let captured = server.finish();
        let head = std::str::from_utf8(&captured.head).expect("request head");
        assert!(head.starts_with("POST /v1/responses HTTP/1.1\r\n"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn panicking_batch_consumer_returns_authority_and_fail_stops_transport() {
        let server = OneShotServer::spawn(
            "200 OK",
            &[
                ("Content-Type", "text/event-stream; charset=utf-8"),
                ("Content-Encoding", "identity"),
            ],
            vec![openai_success_stream()],
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
        let mut consumed = false;
        let result = attempt
            .execute(|batch| {
                consumed = true;
                assert!(!batch.events().is_empty());
                panic!("synthetic provider batch consumer panic");
            })
            .await;
        assert!(consumed);
        assert_eq!(
            result.usage_knowledge(),
            AgentProviderUsageKnowledge::UnknownAfterDispatch
        );
        assert!(matches!(
            result.outcome(),
            AgentProviderTransportOutcome::Failed(failure)
                if failure.class() == AgentProviderFailureClass::Integration
                    && failure.retry_disposition()
                        == zephium_agentic::AgentProviderRetryDisposition::Never
        ));
        let snapshot = transport.snapshot().expect("snapshot");
        assert!(snapshot.is_sealed());
        assert!(snapshot.is_quiescent());

        let (mut refused_policy, refused_input) =
            provider_fixture(AgentProviderKind::OpenAiResponses);
        assert!(matches!(
            transport.try_admit(
                refused_input,
                &mut refused_policy,
                &credential,
                AgentProviderCancellation::new(),
            ),
            Err(AgentProviderAdmissionError::Sealed)
        ));
        assert_eq!(refused_policy.pending_model_calls(), 0);

        let AgentProviderPolicySettlement::Immediate(settlement) = result.into_policy_settlement()
        else {
            panic!("integration panic must settle immediately")
        };
        assert_eq!(
            settlement.usage_accounting(),
            AgentModelUsageAccounting::ReservationCeiling
        );
        assert_eq!(
            settlement.settlement(),
            AgentModelCallSettlement::ProviderFailed
        );
        let receipt = settlement
            .settle(&mut policy)
            .expect("conservative integration settlement");
        assert_eq!(receipt.input_tokens(), 18);
        assert_eq!(receipt.output_tokens(), 20);
        assert_eq!(receipt.cost_micro_usd(), 100);
        assert_eq!(policy.pending_model_calls(), 0);
        let _captured = server.finish();
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
