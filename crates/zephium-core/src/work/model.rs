//! Provider-neutral model calls for the lead agent and its helpers.
//!
//! One request shape for every provider (OpenAI Responses, Anthropic
//! Messages, Gemini, OpenAI-compatible chat, Zephium Cloud). Transports live in
//! `zephium-agentic`; the loop in `zephium-app` sees only these types. Text
//! fields carry model, page and person text, so `Debug` never prints them.

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum WorkModelProvider {
    OpenAi,
    Anthropic,
    Google,
    DeepSeek,
    OpenRouter,
    /// Any OpenAI-compatible chat endpoint the person configured.
    Compatible,
    /// Zephium Cloud: the upstream's native body, our base URL and bearer.
    Cloud,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum WorkModelRole {
    Lead,
    Page,
    Light,
    Decision,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkModelReasoning {
    Low,
    Medium,
    High,
}

/// The wire family a model speaks; Cloud models name their upstream's.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkModelWire {
    OpenAiResponses,
    AnthropicMessages,
    Gemini,
    ChatCompletions,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WorkModelRef {
    pub provider: WorkModelProvider,
    pub wire: WorkModelWire,
    /// The provider's model id, sent as is.
    pub model: String,
}

#[derive(Clone)]
pub struct WorkModelSystemBlock {
    pub text: String,
    /// A cache breakpoint closes this block; nothing volatile comes before it.
    pub cache: bool,
}

#[derive(Clone)]
pub struct WorkModelTool {
    pub name: String,
    pub description: String,
    /// JSON Schema of the arguments object.
    pub schema: Value,
}

#[derive(Clone)]
pub struct WorkModelToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone)]
pub struct WorkModelToolResult {
    pub call: String,
    /// Compact JSON or text the tool returned.
    pub content: String,
    pub is_error: bool,
}

#[derive(Clone)]
pub enum WorkModelPart {
    Text(String),
    Image {
        media_type: String,
        bytes: Vec<u8>,
    },
    ToolCall(WorkModelToolCall),
    /// A provider item the transport must replay verbatim on the next call
    /// (reasoning items, encrypted thinking, native search blocks).
    Replay(Value),
}

#[derive(Clone)]
pub enum WorkModelMessage {
    User(Vec<WorkModelPart>),
    Assistant(Vec<WorkModelPart>),
    ToolResults(Vec<WorkModelToolResult>),
}

#[derive(Clone)]
pub struct WorkModelRequest {
    pub model: WorkModelRef,
    pub system: Vec<WorkModelSystemBlock>,
    pub tools: Vec<WorkModelTool>,
    pub messages: Vec<WorkModelMessage>,
    pub max_output_tokens: u32,
    pub reasoning: Option<WorkModelReasoning>,
    /// Offer the provider's own web search tool when it has one.
    pub native_search: bool,
    pub parallel_tools: bool,
}

#[derive(Clone)]
pub struct WorkModelSearchHit {
    pub url: String,
    pub title: String,
    pub snippet: String,
}

/// Streamed while a call runs, in order.
#[derive(Clone)]
pub enum WorkModelEvent {
    Text(String),
    ToolCall(WorkModelToolCall),
    /// A provider-native search the model ran inside the call.
    Search {
        query: String,
        hits: Vec<WorkModelSearchHit>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkModelStop {
    EndTurn,
    ToolUse,
    MaxTokens,
    Refused,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorkModelUsage {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    /// Cost in millionths of a US dollar, including provider tool fees, when
    /// the catalog prices the model.
    pub cost_micros: Option<u64>,
}

pub struct WorkModelOutcome {
    pub stop: WorkModelStop,
    pub usage: WorkModelUsage,
    /// The assistant turn to append to the conversation, replay items included.
    pub assistant: Vec<WorkModelPart>,
}

/// Closed failures: no provider-authored text crosses this boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkModelError {
    MissingKey,
    Unauthorized,
    RateLimited { retry_after_ms: Option<u64> },
    Overloaded,
    ContextTooLong,
    BadRequest,
    Network,
    Protocol,
    Cancelled,
    OverBudget,
}

impl fmt::Display for WorkModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for WorkModelError {}

pub type WorkModelFuture<'a> =
    Pin<Box<dyn Future<Output = Result<WorkModelOutcome, WorkModelError>> + Send + 'a>>;

pub trait WorkModelClient: Send + Sync {
    /// One model call. `events` receives text, tool calls and native searches
    /// as they stream; dropping the future cancels the call.
    fn call<'a>(
        &'a self,
        request: WorkModelRequest,
        events: &'a (dyn Fn(WorkModelEvent) + Send + Sync),
    ) -> WorkModelFuture<'a>;
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WorkModelSupports {
    pub tools: bool,
    pub vision: bool,
    pub prompt_cache: bool,
    pub reasoning: bool,
    pub native_search: bool,
}

/// Per million tokens, in millionths of a US dollar.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WorkModelPrice {
    #[cfg_attr(feature = "ipc-types", specta(type = u32))]
    pub input: u64,
    #[cfg_attr(feature = "ipc-types", specta(type = u32))]
    pub cached_input: u64,
    #[cfg_attr(feature = "ipc-types", specta(type = u32))]
    pub output: u64,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WorkModelEntry {
    /// Stable catalog id, such as `anthropic/claude-sonnet-5`.
    pub id: String,
    pub model: WorkModelRef,
    pub display_name: String,
    pub roles: Vec<WorkModelRole>,
    pub recommended: bool,
    pub context_window: u32,
    pub max_output: u32,
    pub supports: WorkModelSupports,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<WorkModelPrice>,
}

macro_rules! redacted_debug {
    ($($ty:ty),* $(,)?) => {$(
        impl fmt::Debug for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($ty), "([content redacted])"))
            }
        }
    )*};
}

redacted_debug!(
    WorkModelSystemBlock,
    WorkModelTool,
    WorkModelToolCall,
    WorkModelToolResult,
    WorkModelPart,
    WorkModelMessage,
    WorkModelRequest,
    WorkModelSearchHit,
    WorkModelEvent,
    WorkModelOutcome,
);
