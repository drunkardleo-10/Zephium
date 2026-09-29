//! General-purpose tool-calling transports for the lead agent.
//!
//! One [`LeadClient`] speaks one wire (OpenAI Responses, Anthropic Messages,
//! Gemini, OpenAI-compatible chat) to one endpoint, directly with the person's
//! key or through Zephium Cloud with a bearer. Every failure leaves as a closed
//! [`WorkModelError`]; provider text never crosses.

mod anthropic;
mod chat;
mod gemini;
pub mod keys;
mod listing;
pub mod models;
mod openai;
mod sse;

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use futures_util::StreamExt as _;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, RETRY_AFTER};
use reqwest::{Client, StatusCode, Url};
use serde_json::Value;
use zephium_core::work::model::*;
use zeroize::Zeroizing;

pub use listing::{check_key, fetch_cloud_catalog, list_models, LeadKeyCheck};

const OPENAI_BASE: &str = "https://api.openai.com/v1";
const ANTHROPIC_BASE: &str = "https://api.anthropic.com/v1";
const GEMINI_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";
const DEEPSEEK_BASE: &str = "https://api.deepseek.com";
const OPENROUTER_BASE: &str = "https://openrouter.ai/api/v1";
/// Zephium Cloud's production base; each wire appends its provider's own path.
pub const ZEPHIUM_CLOUD_BASE: &str = "https://api.zephium.app";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const USER_AGENT: &str = "Zephium/1 (lead)";

const MAX_RETRIES: u32 = 2;
/// A longer Retry-After goes back to the caller instead of holding the call.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(20);
const MAX_PAUSE_CONTINUATIONS: u32 = 3;
const MAX_ERROR_BODY_BYTES: usize = 16 * 1024;
const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
const MAX_SECRET_BYTES: usize = 4 * 1024;

pub(crate) const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_TOOL_ARGUMENT_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_TOOL_CALLS: usize = 64;
pub(crate) const MAX_SEARCHES: usize = 32;
pub(crate) const MAX_HITS: usize = 20;

/// An API key or bearer, zeroized on drop and never printed.
pub struct LeadSecret(Zeroizing<String>);

impl LeadSecret {
    /// Accepts one visible-ASCII secret; surrounding whitespace is dropped.
    pub fn new(secret: String) -> Result<Self, WorkModelError> {
        let secret = Zeroizing::new(secret);
        let trimmed = secret.trim();
        if trimmed.is_empty()
            || trimmed.len() > MAX_SECRET_BYTES
            || !trimmed.bytes().all(|byte| matches!(byte, 0x21..=0x7e))
        {
            return Err(WorkModelError::BadRequest);
        }
        Ok(Self(Zeroizing::new(trimmed.to_owned())))
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }

    fn header(&self, prefix: &str) -> Result<HeaderValue, WorkModelError> {
        let value = Zeroizing::new(format!("{prefix}{}", self.expose()));
        let mut header = HeaderValue::from_str(&value).map_err(|_| WorkModelError::Unauthorized)?;
        header.set_sensitive(true);
        Ok(header)
    }
}

impl std::fmt::Debug for LeadSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LeadSecret([redacted])")
    }
}

/// A future yielding the secret for one attempt.
pub type LeadSecretFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Arc<LeadSecret>, WorkModelError>> + Send + 'a>>;

/// Where a client's key or bearer comes from, read once per attempt.
pub trait LeadCredential: Send + Sync {
    /// The secret for the next attempt.
    fn secret(&self) -> LeadSecretFuture<'_>;
    /// The provider refused the secret; the next attempt reads it again.
    fn rejected(&self) {}
}

/// A secret held in memory, for the Cloud session and tests.
pub struct LeadStaticCredential(Arc<LeadSecret>);

impl LeadStaticCredential {
    /// Wraps an already validated secret.
    pub fn new(secret: LeadSecret) -> Self {
        Self(Arc::new(secret))
    }
}

impl LeadCredential for LeadStaticCredential {
    fn secret(&self) -> LeadSecretFuture<'_> {
        let secret = self.0.clone();
        Box::pin(async move { Ok(secret) })
    }
}

/// The endpoint and wire of one client.
#[derive(Clone, Debug)]
pub struct LeadTarget {
    wire: WorkModelWire,
    /// The upstream provider; for Cloud, the provider whose path it proxies.
    upstream: WorkModelProvider,
    base: Url,
    cloud: bool,
}

impl LeadTarget {
    /// The provider's own public endpoint.
    pub fn direct(provider: WorkModelProvider) -> Result<Self, WorkModelError> {
        let (wire, base) = match provider {
            WorkModelProvider::OpenAi => (WorkModelWire::OpenAiResponses, OPENAI_BASE),
            WorkModelProvider::Anthropic => (WorkModelWire::AnthropicMessages, ANTHROPIC_BASE),
            WorkModelProvider::Google => (WorkModelWire::Gemini, GEMINI_BASE),
            WorkModelProvider::DeepSeek => (WorkModelWire::ChatCompletions, DEEPSEEK_BASE),
            WorkModelProvider::OpenRouter => (WorkModelWire::ChatCompletions, OPENROUTER_BASE),
            WorkModelProvider::Compatible | WorkModelProvider::Cloud => {
                return Err(WorkModelError::BadRequest)
            }
        };
        Ok(Self {
            wire,
            upstream: provider,
            base: Url::parse(base).map_err(|_| WorkModelError::BadRequest)?,
            cloud: false,
        })
    }

    /// A person-configured OpenAI-compatible chat endpoint, such as
    /// `https://host/v1`. Plain HTTP is accepted only on this Mac.
    pub fn compatible(base: &str) -> Result<Self, WorkModelError> {
        Ok(Self {
            wire: WorkModelWire::ChatCompletions,
            upstream: WorkModelProvider::Compatible,
            base: compatible_base(base).ok_or(WorkModelError::BadRequest)?,
            cloud: false,
        })
    }

    /// Zephium Cloud proxying `upstream`'s native wire under `base`.
    pub fn cloud(base: &str, upstream: WorkModelProvider) -> Result<Self, WorkModelError> {
        let root = Url::parse(base).map_err(|_| WorkModelError::BadRequest)?;
        if root.scheme() != "https" && !is_loopback(&root) {
            return Err(WorkModelError::BadRequest);
        }
        let (wire, path) = match upstream {
            WorkModelProvider::OpenAi => (WorkModelWire::OpenAiResponses, "openai/v1"),
            WorkModelProvider::Anthropic => (WorkModelWire::AnthropicMessages, "anthropic/v1"),
            WorkModelProvider::Google => (WorkModelWire::Gemini, "google/v1beta"),
            WorkModelProvider::DeepSeek => (WorkModelWire::ChatCompletions, "deepseek/v1"),
            WorkModelProvider::OpenRouter => (WorkModelWire::ChatCompletions, "openrouter/v1"),
            WorkModelProvider::Compatible | WorkModelProvider::Cloud => {
                return Err(WorkModelError::BadRequest)
            }
        };
        Ok(Self {
            wire,
            upstream,
            base: join(&root, path)?,
            cloud: true,
        })
    }

    /// The wire this target speaks.
    pub fn wire(&self) -> WorkModelWire {
        self.wire
    }

    /// The upstream provider.
    pub fn upstream(&self) -> WorkModelProvider {
        self.upstream
    }

    fn call_url(&self, model: &str) -> Result<Url, WorkModelError> {
        match self.wire {
            WorkModelWire::OpenAiResponses => join(&self.base, "responses"),
            WorkModelWire::AnthropicMessages => join(&self.base, "messages"),
            WorkModelWire::ChatCompletions => join(&self.base, "chat/completions"),
            WorkModelWire::Gemini => {
                if model.is_empty()
                    || !model
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_'))
                {
                    return Err(WorkModelError::BadRequest);
                }
                let mut url = join(&self.base, &format!("models/{model}:streamGenerateContent"))?;
                url.set_query(Some("alt=sse"));
                Ok(url)
            }
        }
    }

    fn authorize(
        &self,
        headers: &mut HeaderMap,
        secret: &LeadSecret,
    ) -> Result<(), WorkModelError> {
        if self.wire == WorkModelWire::AnthropicMessages {
            headers.insert(
                "anthropic-version",
                HeaderValue::from_static(ANTHROPIC_VERSION),
            );
        }
        if self.cloud {
            headers.insert("authorization", secret.header("Bearer ")?);
            return Ok(());
        }
        match self.upstream {
            WorkModelProvider::Anthropic => {
                headers.insert("x-api-key", secret.header("")?);
            }
            WorkModelProvider::Google => {
                headers.insert("x-goog-api-key", secret.header("")?);
            }
            _ => {
                headers.insert("authorization", secret.header("Bearer ")?);
            }
        }
        if self.upstream == WorkModelProvider::OpenRouter {
            headers.insert("x-title", HeaderValue::from_static("Zephium"));
            headers.insert(
                "http-referer",
                HeaderValue::from_static("https://zephium.app"),
            );
        }
        Ok(())
    }
}

fn join(base: &Url, path: &str) -> Result<Url, WorkModelError> {
    let mut url = base.clone();
    url.path_segments_mut()
        .map_err(|_| WorkModelError::BadRequest)?
        .pop_if_empty()
        .extend(path.split('/'));
    // `extend` percent-encodes ':'; Gemini's method suffix must stay literal.
    let path = url.path().replace("%3A", ":");
    url.set_path(&path);
    Ok(url)
}

fn is_loopback(url: &Url) -> bool {
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
}

pub(crate) fn compatible_base(base: &str) -> Option<Url> {
    let url = Url::parse(base.trim()).ok()?;
    let allowed = url.scheme() == "https" || (url.scheme() == "http" && is_loopback(&url));
    (allowed
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.host_str().is_some())
    .then_some(url)
}

fn http(local: bool) -> Result<&'static Client, WorkModelError> {
    static SECURE: OnceLock<Option<Client>> = OnceLock::new();
    static LOCAL: OnceLock<Option<Client>> = OnceLock::new();
    let slot = if local { &LOCAL } else { &SECURE };
    slot.get_or_init(|| {
        let builder = Client::builder()
            .user_agent(USER_AGENT)
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(180))
            .timeout(Duration::from_secs(20 * 60))
            .pool_idle_timeout(Duration::from_secs(60))
            .https_only(!local);
        let builder = if local { builder.no_proxy() } else { builder };
        builder.build().ok()
    })
    .as_ref()
    .ok_or(WorkModelError::Network)
}

/// One model on one endpoint. Cheap to share; holds no socket at rest.
pub struct LeadClient {
    target: LeadTarget,
    credential: Arc<dyn LeadCredential>,
    price: Option<WorkModelPrice>,
    search_fee_micros: u64,
    #[cfg(test)]
    backoff: Duration,
}

impl LeadClient {
    /// A client for `target`, priced from the catalog when it knows the model.
    pub fn new(
        target: LeadTarget,
        credential: Arc<dyn LeadCredential>,
        price: Option<WorkModelPrice>,
    ) -> Self {
        Self {
            search_fee_micros: models::search_fee_micros(target.upstream),
            target,
            credential,
            price,
            #[cfg(test)]
            backoff: Duration::from_millis(1),
        }
    }

    /// The endpoint this client calls.
    pub fn target(&self) -> &LeadTarget {
        &self.target
    }

    async fn run(
        &self,
        mut request: WorkModelRequest,
        events: &(dyn Fn(WorkModelEvent) + Send + Sync),
    ) -> Result<WorkModelOutcome, WorkModelError> {
        if request.model.wire != self.target.wire {
            return Err(WorkModelError::BadRequest);
        }
        let mut total = Decoded::default();
        for continuation in 0..=MAX_PAUSE_CONTINUATIONS {
            let body = self.body(&request)?;
            let decoded = self.attempt(&request.model.model, &body, events).await?;
            total.absorb(decoded);
            if total.stop != DecodedStop::Paused || continuation == MAX_PAUSE_CONTINUATIONS {
                break;
            }
            // A server-side tool loop paused: send the turn so far back so it resumes.
            if total.resumed {
                request.messages.pop();
            }
            request
                .messages
                .push(WorkModelMessage::Assistant(total.assistant.clone()));
            total.resumed = true;
        }
        let stop = match total.stop {
            DecodedStop::End(stop) => stop,
            DecodedStop::Paused => WorkModelStop::MaxTokens,
            DecodedStop::Open => return Err(WorkModelError::Protocol),
        };
        let usage = self.usage(&total);
        Ok(WorkModelOutcome {
            stop,
            usage,
            assistant: total.assistant,
        })
    }

    fn body(&self, request: &WorkModelRequest) -> Result<Vec<u8>, WorkModelError> {
        let body = match self.target.wire {
            WorkModelWire::OpenAiResponses => openai::body(request, &self.target)?,
            WorkModelWire::AnthropicMessages => anthropic::body(request, &self.target)?,
            WorkModelWire::Gemini => gemini::body(request)?,
            WorkModelWire::ChatCompletions => chat::body(request, &self.target)?,
        };
        serde_json::to_vec(&body).map_err(|_| WorkModelError::BadRequest)
    }

    fn usage(&self, total: &Decoded) -> WorkModelUsage {
        let mut usage = total.usage;
        usage.cost_micros = total.provider_cost_micros.or_else(|| {
            let price = self.price.as_ref()?;
            let cached = u128::from(usage.cached_input_tokens.min(usage.input_tokens));
            let written = u128::from(total.cache_write_tokens);
            let fresh = u128::from(usage.input_tokens).saturating_sub(cached + written);
            let micros = fresh * u128::from(price.input)
                + cached * u128::from(price.cached_input)
                + written * u128::from(price.input) * 5 / 4
                + u128::from(usage.output_tokens) * u128::from(price.output);
            let fees = u128::from(total.searches) * u128::from(self.search_fee_micros);
            u64::try_from(micros.div_ceil(1_000_000) + fees).ok()
        });
        usage
    }

    async fn attempt(
        &self,
        model: &str,
        body: &[u8],
        events: &(dyn Fn(WorkModelEvent) + Send + Sync),
    ) -> Result<Decoded, WorkModelError> {
        let url = self.target.call_url(model)?;
        let client = http(url.scheme() == "http")?;
        let mut retries = 0;
        loop {
            let secret = self.credential.secret().await?;
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            headers.insert("accept", HeaderValue::from_static("text/event-stream"));
            headers.insert("accept-encoding", HeaderValue::from_static("identity"));
            self.target.authorize(&mut headers, &secret)?;
            drop(secret);
            let failure = match client
                .post(url.clone())
                .headers(headers)
                .body(body.to_vec())
                .send()
                .await
            {
                Err(error) => Failure::from_transport(&error),
                Ok(response) if response.status().is_success() => {
                    match self.stream(response, events).await {
                        Ok(decoded) => return Ok(decoded),
                        Err(StreamFailure::AfterOutput(error)) => return Err(error),
                        Err(StreamFailure::BeforeOutput(error)) => Failure::retryable(error),
                    }
                }
                Ok(response) => Failure::from_response(response).await,
            };
            if failure.error == WorkModelError::Unauthorized {
                self.credential.rejected();
            }
            let wait = match failure.retry_wait(retries) {
                Some(wait) => wait,
                None => return Err(failure.error),
            };
            retries += 1;
            #[cfg(test)]
            let wait = wait.min(self.backoff);
            tokio::time::sleep(wait).await;
        }
    }

    async fn stream(
        &self,
        response: reqwest::Response,
        events: &(dyn Fn(WorkModelEvent) + Send + Sync),
    ) -> Result<Decoded, StreamFailure> {
        let mut decoder = WireDecoder::new(self.target.wire);
        let mut framer = sse::SseFramer::default();
        let mut frames = Vec::new();
        let mut out = Vec::new();
        let mut emitted = false;
        let mut body = response.bytes_stream();
        let fail = |emitted: bool, error: WorkModelError| {
            if emitted {
                StreamFailure::AfterOutput(error)
            } else {
                StreamFailure::BeforeOutput(error)
            }
        };
        let mut done = false;
        while !done {
            let chunk = match body.next().await {
                Some(Ok(chunk)) => chunk,
                Some(Err(error)) => {
                    return Err(fail(emitted, Failure::from_transport(&error).error))
                }
                None => break,
            };
            framer
                .push(&chunk, &mut frames)
                .map_err(|error| fail(emitted, error))?;
            for frame in frames.drain(..) {
                match decoder.event(&frame, &mut out) {
                    Ok(Flow::Continue) => {}
                    Ok(Flow::Done) => done = true,
                    Err(error) => return Err(fail(emitted, error)),
                }
            }
            for event in out.drain(..) {
                emitted = true;
                events(event);
            }
        }
        if !done {
            framer
                .finish(&mut frames)
                .map_err(|error| fail(emitted, error))?;
            for frame in frames.drain(..) {
                decoder
                    .event(&frame, &mut out)
                    .map_err(|error| fail(emitted, error))?;
            }
        }
        let decoded = decoder
            .finish(&mut out)
            .map_err(|error| fail(emitted, error))?;
        for event in out.drain(..) {
            events(event);
        }
        Ok(decoded)
    }
}

impl WorkModelClient for LeadClient {
    fn call<'a>(
        &'a self,
        request: WorkModelRequest,
        events: &'a (dyn Fn(WorkModelEvent) + Send + Sync),
    ) -> WorkModelFuture<'a> {
        Box::pin(self.run(request, events))
    }
}

enum StreamFailure {
    BeforeOutput(WorkModelError),
    AfterOutput(WorkModelError),
}

struct Failure {
    error: WorkModelError,
    retry_after: Option<Duration>,
}

impl Failure {
    fn retryable(error: WorkModelError) -> Self {
        Self {
            error,
            retry_after: None,
        }
    }

    fn from_transport(error: &reqwest::Error) -> Self {
        Self::retryable(if error.is_builder() {
            WorkModelError::Protocol
        } else {
            WorkModelError::Network
        })
    }

    async fn from_response(response: reqwest::Response) -> Self {
        let status = response.status();
        let retry_after = retry_after(response.headers());
        let body = bounded_body(response).await;
        let error = classify(status, &body);
        let error = match error {
            WorkModelError::RateLimited { .. } => WorkModelError::RateLimited {
                retry_after_ms: retry_after.map(|wait| wait.as_millis() as u64),
            },
            other => other,
        };
        Self { error, retry_after }
    }

    fn retry_wait(&self, retries: u32) -> Option<Duration> {
        let retryable = matches!(
            self.error,
            WorkModelError::RateLimited { .. } | WorkModelError::Overloaded
        );
        if !retryable || retries >= MAX_RETRIES {
            return None;
        }
        let wait = self
            .retry_after
            .unwrap_or(Duration::from_millis(1_000 * 3_u64.pow(retries)));
        (wait <= MAX_RETRY_WAIT).then_some(wait)
    }
}

async fn bounded_body(response: reqwest::Response) -> Vec<u8> {
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(Ok(chunk)) = stream.next().await {
        let room = MAX_ERROR_BODY_BYTES.saturating_sub(body.len());
        body.extend_from_slice(&chunk[..chunk.len().min(room)]);
        if body.len() >= MAX_ERROR_BODY_BYTES {
            break;
        }
    }
    body
}

fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    if let Some(ms) = headers
        .get("retry-after-ms")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|ms| ms.is_finite() && *ms >= 0.0)
    {
        return Some(Duration::from_millis(ms.min(86_400_000.0) as u64));
    }
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds.min(86_400)));
    }
    let at = httpdate::parse_http_date(value).ok()?;
    Some(
        at.duration_since(std::time::SystemTime::now())
            .unwrap_or_default(),
    )
}

/// Maps a failed HTTP answer to a closed error. The body is read only for
/// provider error codes; none of its text leaves this function.
fn classify(status: StatusCode, body: &[u8]) -> WorkModelError {
    let json: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let error = json.get("error").unwrap_or(&json);
    let code = [
        error.get("code").and_then(Value::as_str),
        error.get("type").and_then(Value::as_str),
        error.get("status").and_then(Value::as_str),
    ];
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let too_long = code.contains(&Some("context_length_exceeded"))
        || message.contains("prompt is too long")
        || message.contains("context length")
        || message.contains("context window")
        || message.contains("maximum context")
        || message.contains("input token count");
    match status.as_u16() {
        401 | 403 => WorkModelError::Unauthorized,
        402 => WorkModelError::OverBudget,
        413 => WorkModelError::ContextTooLong,
        429 if code.contains(&Some("insufficient_quota")) => WorkModelError::OverBudget,
        400..=499
            if code.iter().flatten().any(|code| {
                matches!(
                    *code,
                    "credit_balance_exhausted"
                        | "billing_hard_limit_reached"
                        | "billing_not_active"
                )
            }) || message.contains("credit balance") =>
        {
            WorkModelError::OverBudget
        }
        429 => WorkModelError::RateLimited {
            retry_after_ms: None,
        },
        500 | 502 | 503 | 504 | 529 => WorkModelError::Overloaded,
        400 | 404 | 422 if too_long => WorkModelError::ContextTooLong,
        400..=499 => WorkModelError::BadRequest,
        _ => WorkModelError::Protocol,
    }
}

/// Closed mapping of in-stream error codes shared by the wires.
pub(crate) fn stream_error(code: &str) -> WorkModelError {
    match code {
        "rate_limit_exceeded" | "rate_limit_error" | "RESOURCE_EXHAUSTED" | "429" => {
            WorkModelError::RateLimited {
                retry_after_ms: None,
            }
        }
        "overloaded_error"
        | "server_error"
        | "api_error"
        | "server_is_overloaded"
        | "slow_down"
        | "UNAVAILABLE"
        | "INTERNAL"
        | "500"
        | "502"
        | "503"
        | "529" => WorkModelError::Overloaded,
        "context_length_exceeded" => WorkModelError::ContextTooLong,
        "authentication_error"
        | "permission_error"
        | "PERMISSION_DENIED"
        | "UNAUTHENTICATED"
        | "401"
        | "403" => WorkModelError::Unauthorized,
        "insufficient_quota"
        | "credit_balance_exhausted"
        | "billing_hard_limit_reached"
        | "billing_not_active"
        | "402" => WorkModelError::OverBudget,
        "invalid_request_error" | "invalid_prompt" | "INVALID_ARGUMENT" | "400" => {
            WorkModelError::BadRequest
        }
        _ => WorkModelError::Protocol,
    }
}

pub(crate) enum Flow {
    Continue,
    Done,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum DecodedStop {
    #[default]
    Open,
    End(WorkModelStop),
    /// Anthropic `pause_turn`: a server tool loop wants the turn sent back.
    Paused,
}

#[derive(Default)]
pub(crate) struct Decoded {
    pub(crate) stop: DecodedStop,
    pub(crate) usage: WorkModelUsage,
    pub(crate) cache_write_tokens: u64,
    pub(crate) searches: u32,
    pub(crate) assistant: Vec<WorkModelPart>,
    /// The provider's own cost report (OpenRouter), in micro-USD.
    pub(crate) provider_cost_micros: Option<u64>,
    resumed: bool,
}

impl Decoded {
    fn absorb(&mut self, next: Decoded) {
        self.stop = next.stop;
        self.usage.input_tokens += next.usage.input_tokens;
        self.usage.cached_input_tokens += next.usage.cached_input_tokens;
        self.usage.output_tokens += next.usage.output_tokens;
        self.usage.reasoning_tokens += next.usage.reasoning_tokens;
        self.cache_write_tokens += next.cache_write_tokens;
        self.searches += next.searches;
        self.provider_cost_micros = match (self.provider_cost_micros, next.provider_cost_micros) {
            (Some(a), Some(b)) => Some(a + b),
            (None, b) if !self.resumed => b,
            _ => None,
        };
        self.assistant.extend(next.assistant);
    }
}

enum WireDecoder {
    OpenAi(openai::Decoder),
    Anthropic(anthropic::Decoder),
    Gemini(gemini::Decoder),
    Chat(chat::Decoder),
}

impl WireDecoder {
    fn new(wire: WorkModelWire) -> Self {
        match wire {
            WorkModelWire::OpenAiResponses => Self::OpenAi(Default::default()),
            WorkModelWire::AnthropicMessages => Self::Anthropic(Default::default()),
            WorkModelWire::Gemini => Self::Gemini(Default::default()),
            WorkModelWire::ChatCompletions => Self::Chat(Default::default()),
        }
    }

    fn event(
        &mut self,
        frame: &sse::SseEvent,
        out: &mut Vec<WorkModelEvent>,
    ) -> Result<Flow, WorkModelError> {
        match self {
            Self::OpenAi(decoder) => decoder.event(frame, out),
            Self::Anthropic(decoder) => decoder.event(frame, out),
            Self::Gemini(decoder) => decoder.event(frame, out),
            Self::Chat(decoder) => decoder.event(frame, out),
        }
    }

    fn finish(self, out: &mut Vec<WorkModelEvent>) -> Result<Decoded, WorkModelError> {
        match self {
            Self::OpenAi(decoder) => decoder.finish(out),
            Self::Anthropic(decoder) => decoder.finish(out),
            Self::Gemini(decoder) => decoder.finish(out),
            Self::Chat(decoder) => decoder.finish(out),
        }
    }
}

/// Parses one SSE data payload.
pub(crate) fn json(data: &str) -> Result<Value, WorkModelError> {
    serde_json::from_str(data).map_err(|_| WorkModelError::Protocol)
}

pub(crate) fn u64_at(value: &Value, path: &[&str]) -> u64 {
    let mut cursor = value;
    for key in path {
        match cursor.get(key) {
            Some(next) => cursor = next,
            None => return 0,
        }
    }
    cursor.as_u64().unwrap_or(0)
}

/// Tool arguments as JSON; unparsable arguments reach the loop as a string so
/// its schema check can hand the model a correctable fault.
pub(crate) fn arguments(raw: &str) -> Value {
    if raw.trim().is_empty() {
        return Value::Object(Default::default());
    }
    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_owned()))
}

pub(crate) fn push_text(buffer: &mut String, text: &str) -> Result<(), WorkModelError> {
    if buffer.len() + text.len() > MAX_TEXT_BYTES {
        return Err(WorkModelError::Protocol);
    }
    buffer.push_str(text);
    Ok(())
}

pub(crate) fn image_base64(bytes: &[u8]) -> Result<String, WorkModelError> {
    use base64::Engine as _;
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
        return Err(WorkModelError::BadRequest);
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

pub(crate) fn media_type_ok(media_type: &str) -> Result<(), WorkModelError> {
    match media_type {
        "image/png" | "image/jpeg" | "image/webp" | "image/gif" => Ok(()),
        _ => Err(WorkModelError::BadRequest),
    }
}

/// Replay items are tagged with the wire that produced them; another wire
/// drops them, so switching models mid-work stays valid.
pub(crate) fn replay(wire: &str, value: Value) -> WorkModelPart {
    WorkModelPart::Replay(serde_json::json!({ "wire": wire, "value": value }))
}

pub(crate) fn replayed<'a>(part: &'a WorkModelPart, wire: &str) -> Option<&'a Value> {
    match part {
        WorkModelPart::Replay(value) if value.get("wire").and_then(Value::as_str) == Some(wire) => {
            value.get("value")
        }
        _ => None,
    }
}

pub(crate) fn system_text(request: &WorkModelRequest) -> String {
    request
        .system
        .iter()
        .map(|block| block.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Native searches a call ran, completed with the titles and passages the
/// answer cited, then released as events once the answer text settles.
#[derive(Default)]
pub(crate) struct Searches {
    pending: Vec<(Option<String>, WorkModelSearchHitSet)>,
    pub(crate) count: u32,
}

#[derive(Default)]
pub(crate) struct WorkModelSearchHitSet {
    query: String,
    hits: Vec<WorkModelSearchHit>,
}

impl Searches {
    pub(crate) fn begin(&mut self, key: Option<String>, query: &str) {
        self.count = self.count.saturating_add(1);
        if self.pending.len() >= MAX_SEARCHES {
            return;
        }
        self.pending.push((
            key,
            WorkModelSearchHitSet {
                query: clip(query, 400),
                hits: Vec::new(),
            },
        ));
    }

    pub(crate) fn hit(&mut self, key: Option<&str>, url: &str, title: &str, snippet: &str) {
        let Some(url) = web_url(url) else { return };
        let set = match key {
            Some(key) => self
                .pending
                .iter_mut()
                .rev()
                .find(|(pending, _)| pending.as_deref() == Some(key)),
            None => self.pending.last_mut(),
        };
        let Some((_, set)) = set else { return };
        if let Some(existing) = set.hits.iter_mut().find(|hit| hit.url == url) {
            if existing.title.is_empty() {
                existing.title = clip(title, 300);
            }
            if existing.snippet.is_empty() {
                existing.snippet = clip(snippet, 400);
            }
        } else if set.hits.len() < MAX_HITS {
            set.hits.push(WorkModelSearchHit {
                url,
                title: clip(title, 300),
                snippet: clip(snippet, 400),
            });
        }
    }

    /// A citation in the answer: fills the hit it names, or joins the last search.
    pub(crate) fn cite(&mut self, url: &str, title: &str, passage: &str) {
        let Some(clean) = web_url(url) else { return };
        let known = self
            .pending
            .iter()
            .rev()
            .find(|(_, set)| set.hits.iter().any(|hit| hit.url == clean))
            .map(|(key, _)| key.clone());
        match known {
            Some(key) => self.hit(key.as_deref(), url, title, passage),
            None => self.hit(None, url, title, passage),
        }
    }

    pub(crate) fn flush(&mut self, out: &mut Vec<WorkModelEvent>) {
        for (_, set) in self.pending.drain(..) {
            out.push(WorkModelEvent::Search {
                query: set.query,
                hits: set.hits,
            });
        }
    }
}

fn web_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    (matches!(parsed.scheme(), "https" | "http") && url.len() <= 2048).then(|| url.to_owned())
}

pub(crate) fn clip(text: &str, max_chars: usize) -> String {
    let text = text.trim();
    match text.char_indices().nth(max_chars) {
        Some((end, _)) => text[..end].to_owned(),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod wire_tests;
