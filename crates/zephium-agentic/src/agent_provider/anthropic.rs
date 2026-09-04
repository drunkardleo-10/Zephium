//! Bounded Anthropic Messages SSE normalization.
//!
//! The decoder accepts only assistant text and the fixed client browser-tool
//! vocabulary requested by the immutable Messages body. Partial tool JSON is
//! retained under a hard byte ceiling and is converted into a closed browser
//! proposal only after the provider declares a `tool_use` stop. Thinking,
//! server tools, citations, fallback blocks, and provider-authored error text
//! never cross the public boundary.

use std::borrow::Cow;
use std::fmt;

use serde::Deserialize;
use serde_json::Value;

use super::sse::{SseDecoder, SseEvent};
use super::{
    AgentBrowserToolCall, AgentBrowserToolCallId, AgentBrowserToolKind, AgentProviderCallConfig,
    AgentProviderCallIdentity, AgentProviderCompletion, AgentProviderFailure,
    AgentProviderFailureClass, AgentProviderFinishedStream, AgentProviderKind,
    AgentProviderProtocolError, AgentProviderResponseIdentity, AgentProviderStopReason,
    AgentProviderStreamBatch, AgentProviderStreamBudget, AgentProviderStreamConclusion,
    AgentProviderStreamStats, AgentProviderTerminalFailure, AgentProviderTextDelta,
    AgentProviderUsage,
};

const MAX_ANTHROPIC_MESSAGE_ID_BYTES: usize = 128;
const MAX_ANTHROPIC_CONTENT_BLOCKS: usize = 16;
const MAX_ANTHROPIC_STOP_SEQUENCE_BYTES: usize = 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamPhase {
    AwaitStart,
    Content,
    AwaitMessageStop,
    Terminal,
}

enum ContentBlockState {
    Text { open: bool },
    Tool(ToolAccumulator),
}

impl ContentBlockState {
    const fn is_open(&self) -> bool {
        match self {
            Self::Text { open } => *open,
            Self::Tool(tool) => tool.open,
        }
    }
}

struct ToolAccumulator {
    id: String,
    name: AgentBrowserToolKind,
    arguments: String,
    open: bool,
}

enum PendingTerminal {
    Completed {
        stop: AgentProviderStopReason,
        usage: AgentProviderUsage,
        tool_only_output: bool,
        identity: AgentProviderResponseIdentity,
    },
    Failed {
        failure: AgentProviderFailure,
        usage: Option<AgentProviderUsage>,
        identity: Option<AgentProviderResponseIdentity>,
    },
}

#[derive(Clone, Copy)]
struct UsageState {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_input_tokens: u64,
    cache_creation_input_tokens: u64,
    reasoning_output_tokens: u64,
}

impl UsageState {
    fn from_start(usage: AnthropicStartUsage<'_>) -> Result<Self, AgentProviderProtocolError> {
        let state = Self {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_read_input_tokens: usage.cache_read_input_tokens,
            cache_creation_input_tokens: usage.cache_creation_input_tokens,
            reasoning_output_tokens: usage
                .output_tokens_details
                .map_or(0, |details| details.thinking_tokens),
        };
        state.normalize()?;
        Ok(state)
    }

    fn update(&mut self, usage: AnthropicDeltaUsage) -> Result<(), AgentProviderProtocolError> {
        update_cumulative(&mut self.input_tokens, usage.input_tokens)?;
        update_cumulative(
            &mut self.cache_read_input_tokens,
            usage.cache_read_input_tokens,
        )?;
        update_cumulative(
            &mut self.cache_creation_input_tokens,
            usage.cache_creation_input_tokens,
        )?;
        update_cumulative(&mut self.output_tokens, Some(usage.output_tokens))?;
        if let Some(details) = usage.output_tokens_details {
            update_cumulative(
                &mut self.reasoning_output_tokens,
                Some(details.thinking_tokens),
            )?;
        }
        self.normalize()?;
        Ok(())
    }

    fn normalize(self) -> Result<AgentProviderUsage, AgentProviderProtocolError> {
        let total_input = self
            .input_tokens
            .checked_add(self.cache_read_input_tokens)
            .and_then(|tokens| tokens.checked_add(self.cache_creation_input_tokens))
            .ok_or(AgentProviderProtocolError::Usage)?;
        AgentProviderUsage::try_new(
            total_input,
            self.output_tokens,
            self.cache_read_input_tokens,
            self.cache_creation_input_tokens,
            self.reasoning_output_tokens,
        )
        .map_err(|_| AgentProviderProtocolError::Usage)
    }
}

fn update_cumulative(
    current: &mut u64,
    next: Option<u64>,
) -> Result<(), AgentProviderProtocolError> {
    if let Some(next) = next {
        if next < *current {
            return Err(AgentProviderProtocolError::Usage);
        }
        *current = next;
    }
    Ok(())
}

/// Single-owner incremental decoder for one Anthropic Messages SSE body.
///
/// The HTTP shell supplies arbitrary byte chunks and must call `finish` once
/// EOF is reached. Protocol errors are content-free. Mid-stream provider
/// errors become typed terminal failures and never surface their message text.
#[must_use]
pub(super) struct AnthropicMessagesStreamDecoder {
    call: AgentProviderCallIdentity,
    config: AgentProviderCallConfig,
    budget: AgentProviderStreamBudget,
    sse: SseDecoder,
    phase: StreamPhase,
    has_message_id: bool,
    response_identity: Option<AgentProviderResponseIdentity>,
    blocks: Vec<ContentBlockState>,
    output_text_bytes: u32,
    tool_argument_bytes: u32,
    decoded_tool_calls: u8,
    usage: Option<UsageState>,
    stop: Option<AgentProviderStopReason>,
    has_stop_sequence: bool,
    terminal: Option<PendingTerminal>,
    tool: Option<AgentBrowserToolCall>,
    failure: Option<AgentProviderProtocolError>,
}

impl AnthropicMessagesStreamDecoder {
    /// Starts one decoder for an exact call and Anthropic Messages config.
    pub fn try_new(
        call: AgentProviderCallIdentity,
        config: &AgentProviderCallConfig,
    ) -> Result<Self, AgentProviderProtocolError> {
        if config.provider() != AgentProviderKind::AnthropicMessages {
            return Err(AgentProviderProtocolError::Event);
        }
        Ok(Self {
            call,
            config: config.clone(),
            budget: config.stream_budget(),
            sse: SseDecoder::new(
                config.stream_budget().max_events(),
                config.stream_budget().max_wire_bytes(),
            )?,
            phase: StreamPhase::AwaitStart,
            has_message_id: false,
            response_identity: None,
            blocks: Vec::with_capacity(MAX_ANTHROPIC_CONTENT_BLOCKS),
            output_text_bytes: 0,
            tool_argument_bytes: 0,
            decoded_tool_calls: 0,
            usage: None,
            stop: None,
            has_stop_sequence: false,
            terminal: None,
            tool: None,
            failure: None,
        })
    }

    /// Decodes one arbitrary transport chunk into bounded normalized events.
    pub(super) fn push(
        &mut self,
        bytes: &[u8],
    ) -> Result<AgentProviderStreamBatch, AgentProviderProtocolError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let result = self.push_inner(bytes);
        if let Err(error) = result {
            self.failure = Some(error);
        }
        result
    }

    fn push_inner(
        &mut self,
        bytes: &[u8],
    ) -> Result<AgentProviderStreamBatch, AgentProviderProtocolError> {
        let mut wire_events = Vec::new();
        self.sse.push(bytes, &mut wire_events)?;
        let mut events = Vec::new();
        for event in wire_events {
            self.handle_event(event, &mut events)?;
        }
        Ok(AgentProviderStreamBatch::new(self.call, events))
    }

    /// Requires complete SSE framing and exactly one terminal provider event.
    pub(super) fn finish(
        mut self,
    ) -> Result<AgentProviderFinishedStream, AgentProviderProtocolError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let stats = self.stats();
        self.sse.finish()?;
        if self.phase != StreamPhase::Terminal {
            return Err(AgentProviderProtocolError::Terminal);
        }
        let terminal = self
            .terminal
            .take()
            .ok_or(AgentProviderProtocolError::Terminal)?;
        let (conclusion, tool) = match terminal {
            PendingTerminal::Completed {
                stop,
                usage,
                tool_only_output,
                identity,
            } => (
                AgentProviderStreamConclusion::Completed(
                    AgentProviderCompletion::new_with_response_identity(
                        self.call,
                        stop,
                        usage,
                        stats,
                        tool_only_output,
                        identity,
                    ),
                ),
                if stop == AgentProviderStopReason::ToolCalls {
                    Some(
                        self.tool
                            .take()
                            .ok_or(AgentProviderProtocolError::Terminal)?,
                    )
                } else {
                    None
                },
            ),
            PendingTerminal::Failed {
                failure,
                usage,
                identity,
            } => {
                let failure = match identity {
                    Some(identity) => AgentProviderTerminalFailure::new_with_response_identity(
                        self.call, failure, usage, stats, identity,
                    ),
                    None => AgentProviderTerminalFailure::new(self.call, failure, usage, stats),
                };
                (AgentProviderStreamConclusion::Failed(failure), None)
            }
        };
        AgentProviderFinishedStream::new(conclusion, tool)
    }

    fn handle_event(
        &mut self,
        event: SseEvent,
        output: &mut Vec<AgentProviderTextDelta>,
    ) -> Result<(), AgentProviderProtocolError> {
        if self.phase == StreamPhase::Terminal {
            return Err(AgentProviderProtocolError::Sequence);
        }
        if event.data() == "[DONE]" {
            return Err(AgentProviderProtocolError::Event);
        }
        let kind = parse_event_type(event.data())?;
        if event.event() != kind {
            return Err(AgentProviderProtocolError::Event);
        }
        if self.phase == StreamPhase::AwaitMessageStop && kind != "message_stop" {
            return Err(AgentProviderProtocolError::Sequence);
        }
        match kind {
            "message_start" => self.handle_message_start(event.data()),
            "content_block_start" => self.handle_content_block_start(event.data()),
            "content_block_delta" => self.handle_content_block_delta(event.data(), output),
            "content_block_stop" => self.handle_content_block_stop(event.data()),
            "message_delta" => self.handle_message_delta(event.data(), output),
            "message_stop" => self.handle_message_stop(event.data()),
            "ping" => self.handle_ping(event.data()),
            "error" => self.handle_error(event.data()),
            _ => Err(AgentProviderProtocolError::UnsupportedOutput),
        }
    }

    fn handle_message_start(&mut self, data: &str) -> Result<(), AgentProviderProtocolError> {
        if self.phase != StreamPhase::AwaitStart {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let event: MessageStartEnvelope<'_> = parse(data)?;
        if event.kind != "message_start"
            || event.message.kind != "message"
            || event.message.role != "assistant"
            || !event.message.content.is_empty()
            || event.message.stop_reason.is_some()
            || event.message.stop_sequence.is_some()
        {
            return Err(AgentProviderProtocolError::Event);
        }
        validate_message_id(event.message.id)?;
        let identity = AgentProviderResponseIdentity::try_attested(
            &self.config,
            event.message.model,
            event.message.usage.service_tier,
            Some(event.message.usage.inference_geo),
        )?;
        self.usage = Some(UsageState::from_start(event.message.usage)?);
        self.response_identity = Some(identity);
        self.has_message_id = true;
        self.phase = StreamPhase::Content;
        Ok(())
    }

    fn handle_content_block_start(&mut self, data: &str) -> Result<(), AgentProviderProtocolError> {
        self.require_content_phase()?;
        if self.blocks.len() >= MAX_ANTHROPIC_CONTENT_BLOCKS {
            return Err(AgentProviderProtocolError::Limit);
        }
        if self.blocks.last().is_some_and(ContentBlockState::is_open) {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let event: ContentBlockStartEnvelope<'_> = parse(data)?;
        if event.kind != "content_block_start" || event.index != self.blocks.len() {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let block = match event.content_block.kind {
            "text" => {
                if event.content_block.text != Some("")
                    || event.content_block.id.is_some()
                    || event.content_block.name.is_some()
                    || event.content_block.input.is_some()
                {
                    return Err(AgentProviderProtocolError::Event);
                }
                ContentBlockState::Text { open: true }
            }
            "tool_use" => {
                if self
                    .blocks
                    .iter()
                    .any(|block| matches!(block, ContentBlockState::Tool(_)))
                {
                    return Err(AgentProviderProtocolError::Sequence);
                }
                let id = event
                    .content_block
                    .id
                    .ok_or(AgentProviderProtocolError::Event)?;
                let name = event
                    .content_block
                    .name
                    .ok_or(AgentProviderProtocolError::Event)?;
                let input = event
                    .content_block
                    .input
                    .ok_or(AgentProviderProtocolError::Event)?;
                if event.content_block.text.is_some()
                    || input.as_object().is_none_or(|object| !object.is_empty())
                {
                    return Err(AgentProviderProtocolError::Event);
                }
                AgentBrowserToolCallId::try_new(id.to_owned())
                    .map_err(|_| AgentProviderProtocolError::ToolCall)?;
                let name = AgentBrowserToolKind::parse(name)
                    .ok_or(AgentProviderProtocolError::ToolCall)?;
                ContentBlockState::Tool(ToolAccumulator {
                    id: id.to_owned(),
                    name,
                    arguments: String::new(),
                    open: true,
                })
            }
            "thinking"
            | "redacted_thinking"
            | "server_tool_use"
            | "web_search_tool_result"
            | "web_fetch_tool_result"
            | "code_execution_tool_result"
            | "fallback" => return Err(AgentProviderProtocolError::UnsupportedOutput),
            _ => return Err(AgentProviderProtocolError::UnsupportedOutput),
        };
        self.blocks.push(block);
        Ok(())
    }

    fn handle_content_block_delta(
        &mut self,
        data: &str,
        output: &mut Vec<AgentProviderTextDelta>,
    ) -> Result<(), AgentProviderProtocolError> {
        self.require_content_phase()?;
        let event: ContentBlockDeltaEnvelope<'_> = parse(data)?;
        if event.kind != "content_block_delta"
            || event.index.checked_add(1) != Some(self.blocks.len())
        {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let block = self
            .blocks
            .get_mut(event.index)
            .ok_or(AgentProviderProtocolError::Sequence)?;
        match (block, event.delta.kind) {
            (ContentBlockState::Text { open: true }, "text_delta") => {
                let text = event.delta.text.ok_or(AgentProviderProtocolError::Event)?;
                if event.delta.partial_json.is_some() {
                    return Err(AgentProviderProtocolError::Event);
                }
                let bytes =
                    u32::try_from(text.len()).map_err(|_| AgentProviderProtocolError::Limit)?;
                let next = self
                    .output_text_bytes
                    .checked_add(bytes)
                    .ok_or(AgentProviderProtocolError::Limit)?;
                if next > self.budget.max_output_text_bytes() {
                    return Err(AgentProviderProtocolError::Limit);
                }
                self.output_text_bytes = next;
                if !text.is_empty() {
                    output.push(AgentProviderTextDelta::new(text.into_owned()));
                }
                Ok(())
            }
            (ContentBlockState::Tool(tool), "input_json_delta") if tool.open => {
                let partial = event
                    .delta
                    .partial_json
                    .ok_or(AgentProviderProtocolError::Event)?;
                if event.delta.text.is_some() {
                    return Err(AgentProviderProtocolError::Event);
                }
                let bytes =
                    u32::try_from(partial.len()).map_err(|_| AgentProviderProtocolError::Limit)?;
                let next = self
                    .tool_argument_bytes
                    .checked_add(bytes)
                    .ok_or(AgentProviderProtocolError::Limit)?;
                if next > self.budget.max_tool_argument_bytes() {
                    return Err(AgentProviderProtocolError::Limit);
                }
                tool.arguments.push_str(partial.as_ref());
                self.tool_argument_bytes = next;
                Ok(())
            }
            (ContentBlockState::Text { .. }, "thinking_delta" | "signature_delta") => {
                Err(AgentProviderProtocolError::UnsupportedOutput)
            }
            (_, "citations_delta") => Err(AgentProviderProtocolError::UnsupportedOutput),
            _ => Err(AgentProviderProtocolError::Sequence),
        }
    }

    fn handle_content_block_stop(&mut self, data: &str) -> Result<(), AgentProviderProtocolError> {
        self.require_content_phase()?;
        let event: IndexedEnvelope<'_> = parse(data)?;
        if event.kind != "content_block_stop"
            || event.index.checked_add(1) != Some(self.blocks.len())
        {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let block = self
            .blocks
            .get_mut(event.index)
            .ok_or(AgentProviderProtocolError::Sequence)?;
        match block {
            ContentBlockState::Text { open } => {
                if !*open {
                    return Err(AgentProviderProtocolError::Sequence);
                }
                *open = false;
            }
            ContentBlockState::Tool(tool) => {
                if !tool.open {
                    return Err(AgentProviderProtocolError::Sequence);
                }
                if tool.arguments.is_empty() {
                    let next = self
                        .tool_argument_bytes
                        .checked_add(2)
                        .ok_or(AgentProviderProtocolError::Limit)?;
                    if next > self.budget.max_tool_argument_bytes() {
                        return Err(AgentProviderProtocolError::Limit);
                    }
                    tool.arguments.push_str("{}");
                    self.tool_argument_bytes = next;
                }
                tool.open = false;
            }
        }
        Ok(())
    }

    fn handle_message_delta(
        &mut self,
        data: &str,
        _output: &mut Vec<AgentProviderTextDelta>,
    ) -> Result<(), AgentProviderProtocolError> {
        if self.phase != StreamPhase::Content || self.blocks.iter().any(ContentBlockState::is_open)
        {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let event: MessageDeltaEnvelope<'_> = parse(data)?;
        if event.kind != "message_delta" {
            return Err(AgentProviderProtocolError::Event);
        }
        let reason = event
            .delta
            .stop_reason
            .ok_or(AgentProviderProtocolError::Sequence)?;
        let stop = match reason {
            "end_turn" => AgentProviderStopReason::Completed,
            "tool_use" => AgentProviderStopReason::ToolCalls,
            "max_tokens" | "model_context_window_exceeded" => AgentProviderStopReason::OutputLimit,
            "stop_sequence" => AgentProviderStopReason::StopSequence,
            "refusal" => AgentProviderStopReason::Refused,
            "pause_turn" => AgentProviderStopReason::Paused,
            _ => return Err(AgentProviderProtocolError::UnsupportedOutput),
        };
        self.validate_stop_shape(stop, event.delta.stop_sequence.as_deref())?;
        if stop == AgentProviderStopReason::ToolCalls {
            let tool = self
                .blocks
                .iter_mut()
                .find_map(|block| match block {
                    ContentBlockState::Tool(tool) => Some(tool),
                    ContentBlockState::Text { .. } => None,
                })
                .ok_or(AgentProviderProtocolError::Terminal)?;
            let id = tool.id.clone();
            let name = tool.name;
            let arguments = std::mem::take(&mut tool.arguments);
            let call = AgentBrowserToolCall::decode(self.call, id, name.as_str(), arguments)
                .map_err(|_| AgentProviderProtocolError::ToolCall)?;
            self.decoded_tool_calls = 1;
            self.tool = Some(call);
        }
        self.usage
            .as_mut()
            .ok_or(AgentProviderProtocolError::Usage)?
            .update(event.usage)?;
        self.stop = Some(stop);
        self.phase = StreamPhase::AwaitMessageStop;
        Ok(())
    }

    fn validate_stop_shape(
        &mut self,
        stop: AgentProviderStopReason,
        stop_sequence: Option<&str>,
    ) -> Result<(), AgentProviderProtocolError> {
        let has_tool = self
            .blocks
            .iter()
            .any(|block| matches!(block, ContentBlockState::Tool(_)));
        if stop == AgentProviderStopReason::ToolCalls && !has_tool {
            return Err(AgentProviderProtocolError::Terminal);
        }
        if has_tool
            && !matches!(
                stop,
                AgentProviderStopReason::ToolCalls | AgentProviderStopReason::OutputLimit
            )
        {
            return Err(AgentProviderProtocolError::Terminal);
        }
        match (stop, stop_sequence) {
            (AgentProviderStopReason::StopSequence, Some(sequence))
                if !sequence.is_empty() && sequence.len() <= MAX_ANTHROPIC_STOP_SEQUENCE_BYTES =>
            {
                self.has_stop_sequence = true;
                Ok(())
            }
            (AgentProviderStopReason::StopSequence, _) => Err(AgentProviderProtocolError::Terminal),
            (_, None) => Ok(()),
            (_, Some(_)) => Err(AgentProviderProtocolError::Terminal),
        }
    }

    fn handle_message_stop(&mut self, data: &str) -> Result<(), AgentProviderProtocolError> {
        if self.phase != StreamPhase::AwaitMessageStop
            || self.stop.is_none()
            || self.blocks.iter().any(ContentBlockState::is_open)
        {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let event: EventType<'_> = parse(data)?;
        if event.kind != "message_stop" {
            return Err(AgentProviderProtocolError::Event);
        }
        let usage = self
            .usage
            .ok_or(AgentProviderProtocolError::Usage)?
            .normalize()?;
        let stop = self.stop.ok_or(AgentProviderProtocolError::Terminal)?;
        let tool_only_output = stop == AgentProviderStopReason::ToolCalls
            && matches!(self.blocks.as_slice(), [ContentBlockState::Tool(_)]);
        let identity = self
            .response_identity
            .ok_or(AgentProviderProtocolError::Sequence)?;
        self.terminal = Some(PendingTerminal::Completed {
            stop,
            usage,
            tool_only_output,
            identity,
        });
        self.phase = StreamPhase::Terminal;
        Ok(())
    }

    fn handle_ping(&self, data: &str) -> Result<(), AgentProviderProtocolError> {
        if self.phase == StreamPhase::AwaitStart {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let event: EventType<'_> = parse(data)?;
        if event.kind == "ping" {
            Ok(())
        } else {
            Err(AgentProviderProtocolError::Event)
        }
    }

    fn handle_error(&mut self, data: &str) -> Result<(), AgentProviderProtocolError> {
        let event: ErrorEnvelope<'_> = parse(data)?;
        if event.kind != "error" || event.error.kind.is_empty() {
            return Err(AgentProviderProtocolError::Event);
        }
        let class = match event.error.kind {
            "invalid_request_error" | "request_too_large" => {
                AgentProviderFailureClass::InvalidRequest
            }
            "authentication_error" => AgentProviderFailureClass::Authentication,
            "permission_error" => AgentProviderFailureClass::Permission,
            "not_found_error" => AgentProviderFailureClass::NotFound,
            "conflict_error" => AgentProviderFailureClass::Conflict,
            "rate_limit_error" => AgentProviderFailureClass::RateLimited,
            "overloaded_error" => AgentProviderFailureClass::Overloaded,
            "timeout_error" => AgentProviderFailureClass::Timeout,
            "api_error" => AgentProviderFailureClass::Overloaded,
            "billing_error" => AgentProviderFailureClass::Provider,
            _ => AgentProviderFailureClass::Provider,
        };
        let failure = AgentProviderFailure::try_new(class, None)
            .map_err(|_| AgentProviderProtocolError::Event)?;
        let usage = self.usage.and_then(|usage| usage.normalize().ok());
        self.terminal = Some(PendingTerminal::Failed {
            failure,
            usage,
            identity: self.response_identity,
        });
        self.phase = StreamPhase::Terminal;
        Ok(())
    }

    fn require_content_phase(&self) -> Result<(), AgentProviderProtocolError> {
        if self.phase == StreamPhase::Content {
            Ok(())
        } else {
            Err(AgentProviderProtocolError::Sequence)
        }
    }

    fn stats(&self) -> AgentProviderStreamStats {
        AgentProviderStreamStats::new(
            self.sse.wire_bytes(),
            self.sse.events(),
            self.output_text_bytes,
            self.decoded_tool_calls,
            self.tool_argument_bytes,
        )
    }
}

impl fmt::Debug for AnthropicMessagesStreamDecoder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AnthropicMessagesStreamDecoder")
            .field("call", &self.call)
            .field("config", &self.config)
            .field("budget", &self.budget)
            .field("phase", &self.phase)
            .field("has_message_id", &self.has_message_id)
            .field("content_blocks", &self.blocks.len())
            .field("has_stop_sequence", &self.has_stop_sequence)
            .field("stats", &self.stats())
            .finish()
    }
}

#[derive(Deserialize)]
struct EventType<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
}

#[derive(Deserialize)]
struct MessageStartEnvelope<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    #[serde(borrow)]
    message: MessageStartHead<'a>,
}

#[derive(Deserialize)]
struct MessageStartHead<'a> {
    #[serde(borrow)]
    id: &'a str,
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    #[serde(borrow)]
    role: &'a str,
    #[serde(borrow)]
    model: &'a str,
    content: Vec<Value>,
    #[serde(borrow)]
    stop_reason: Option<&'a str>,
    #[serde(borrow)]
    stop_sequence: Option<&'a str>,
    usage: AnthropicStartUsage<'a>,
}

#[derive(Clone, Copy, Deserialize)]
struct AnthropicStartUsage<'a> {
    input_tokens: u64,
    output_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    output_tokens_details: Option<AnthropicOutputDetails>,
    #[serde(borrow)]
    service_tier: &'a str,
    #[serde(borrow)]
    inference_geo: &'a str,
}

#[derive(Clone, Copy, Deserialize)]
struct AnthropicOutputDetails {
    thinking_tokens: u64,
}

#[derive(Deserialize)]
struct ContentBlockStartEnvelope<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    index: usize,
    #[serde(borrow)]
    content_block: ContentBlockHead<'a>,
}

#[derive(Deserialize)]
struct ContentBlockHead<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    #[serde(borrow)]
    text: Option<&'a str>,
    #[serde(borrow)]
    id: Option<&'a str>,
    #[serde(borrow)]
    name: Option<&'a str>,
    input: Option<Value>,
}

#[derive(Deserialize)]
struct ContentBlockDeltaEnvelope<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    index: usize,
    #[serde(borrow)]
    delta: ContentBlockDelta<'a>,
}

#[derive(Deserialize)]
struct ContentBlockDelta<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    #[serde(borrow)]
    text: Option<Cow<'a, str>>,
    #[serde(borrow)]
    partial_json: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
struct IndexedEnvelope<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    index: usize,
}

#[derive(Deserialize)]
struct MessageDeltaEnvelope<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    #[serde(borrow)]
    delta: MessageDelta<'a>,
    usage: AnthropicDeltaUsage,
}

#[derive(Deserialize)]
struct MessageDelta<'a> {
    #[serde(borrow)]
    stop_reason: Option<&'a str>,
    #[serde(borrow)]
    stop_sequence: Option<Cow<'a, str>>,
}

#[derive(Clone, Copy, Deserialize)]
struct AnthropicDeltaUsage {
    input_tokens: Option<u64>,
    output_tokens: u64,
    cache_read_input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    output_tokens_details: Option<AnthropicOutputDetails>,
}

#[derive(Deserialize)]
struct ErrorEnvelope<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    #[serde(borrow)]
    error: ErrorHead<'a>,
}

#[derive(Deserialize)]
struct ErrorHead<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
}

fn parse_event_type(data: &str) -> Result<&str, AgentProviderProtocolError> {
    let event: EventType<'_> = parse(data)?;
    Ok(event.kind)
}

fn parse<'a, T>(data: &'a str) -> Result<T, AgentProviderProtocolError>
where
    T: Deserialize<'a>,
{
    serde_json::from_str(data).map_err(|_| AgentProviderProtocolError::Event)
}

fn validate_message_id(value: &str) -> Result<(), AgentProviderProtocolError> {
    if value.is_empty()
        || value.len() > MAX_ANTHROPIC_MESSAGE_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        Err(AgentProviderProtocolError::Event)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AgentModelCallId, AgentPlanLeaseId, AgentPlanNodeId, AgentProviderModelRevision,
        AgentProviderPricingProfile, AgentProviderPricingRevision, AgentProviderReasoningEffort,
        AgentRunManifestId, SemanticTokenizerRevision,
    };

    fn call() -> AgentProviderCallIdentity {
        AgentProviderCallIdentity {
            manifest: AgentRunManifestId::from_raw(1),
            manifest_guard: [0; 32],
            call: AgentModelCallId::new(2).expect("call"),
            lease: AgentPlanLeaseId::from_raw(3),
            node: AgentPlanNodeId::from_raw(4),
        }
    }

    fn config(max_text: u32, max_arguments: u32) -> AgentProviderCallConfig {
        AgentProviderCallConfig::try_for_test(
            AgentProviderKind::AnthropicMessages,
            AgentProviderModelRevision::try_new("claude-opus-5".to_owned()).expect("model"),
            AgentProviderReasoningEffort::None,
            SemanticTokenizerRevision::try_new("anthropic:claude-opus-5:v1".to_owned())
                .expect("tokenizer"),
            AgentProviderPricingProfile::try_new(
                AgentProviderPricingRevision::new(1).expect("pricing revision"),
                16_384,
            )
            .expect("pricing profile"),
            512,
            1_024,
            AgentProviderStreamBudget::try_new(64 * 1024, 64, max_text, 1, max_arguments)
                .expect("budget"),
        )
        .expect("config")
    }

    fn sse(event: &str, data: &str) -> String {
        format!("event: {event}\ndata: {data}\n\n")
    }

    fn start(id: &str) -> String {
        sse(
            "message_start",
            &serde_json::json!({
                "type": "message_start",
                "message": {
                    "id": id,
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": "claude-opus-5",
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": {
                        "input_tokens": 7,
                        "cache_creation_input_tokens": 3,
                        "cache_read_input_tokens": 5,
                        "output_tokens": 1,
                        "output_tokens_details": {"thinking_tokens": 0},
                        "service_tier": "standard",
                        "inference_geo": "global"
                    }
                }
            })
            .to_string(),
        )
    }

    fn message_delta(reason: &str, output_tokens: u64) -> String {
        sse(
            "message_delta",
            &serde_json::json!({
                "type": "message_delta",
                "delta": {"stop_reason": reason, "stop_sequence": null},
                "usage": {"output_tokens": output_tokens}
            })
            .to_string(),
        )
    }

    fn null_message_delta(output_tokens: u64) -> String {
        sse(
            "message_delta",
            &serde_json::json!({
                "type": "message_delta",
                "delta": {"stop_reason": null, "stop_sequence": null},
                "usage": {"output_tokens": output_tokens}
            })
            .to_string(),
        )
    }

    fn tool_decoder_awaiting_message_stop() -> AnthropicMessagesStreamDecoder {
        let mut decoder =
            AnthropicMessagesStreamDecoder::try_new(call(), &config(64, 1_024)).expect("decoder");
        let prefix = [
            start("msg_stop_boundary"),
            sse(
                "content_block_start",
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_stop_boundary","name":"back","input":{}}}"#,
            ),
            sse(
                "content_block_stop",
                r#"{"type":"content_block_stop","index":0}"#,
            ),
            message_delta("tool_use", 12),
        ]
        .concat();
        decoder
            .push(prefix.as_bytes())
            .expect("terminal stop delta");
        assert_eq!(decoder.phase, StreamPhase::AwaitMessageStop);
        decoder
    }

    fn message_stop() -> String {
        sse("message_stop", r#"{"type":"message_stop"}"#)
    }

    #[test]
    fn fragmented_text_and_ping_normalize_usage_while_unknown_events_fail_closed() {
        let mut decoder =
            AnthropicMessagesStreamDecoder::try_new(call(), &config(64, 1_024)).expect("decoder");
        let stream = [
            start("msg_1"),
            sse(
                "content_block_start",
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            ),
            sse("ping", r#"{"type":"ping"}"#),
            sse(
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello "}}"#,
            ),
            sse(
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"world"}}"#,
            ),
            sse(
                "content_block_stop",
                r#"{"type":"content_block_stop","index":0}"#,
            ),
            message_delta("end_turn", 9),
            message_stop(),
        ]
        .concat();
        let first_split = stream.len() / 3;
        let second_split = first_split * 2;
        let first = decoder
            .push(&stream.as_bytes()[..first_split])
            .expect("first");
        let second = decoder
            .push(&stream.as_bytes()[first_split..second_split])
            .expect("second");
        let third = decoder
            .push(&stream.as_bytes()[second_split..])
            .expect("third");
        let text = first
            .into_deltas()
            .into_iter()
            .chain(second.into_deltas())
            .chain(third.into_deltas())
            .map(|delta| delta.as_str().to_owned())
            .collect::<String>();
        assert_eq!(text, "hello world");
        let AgentProviderStreamConclusion::Completed(completion) =
            decoder.finish().expect("terminal").conclusion()
        else {
            panic!("completion");
        };
        assert_eq!(completion.stop(), AgentProviderStopReason::Completed);
        assert!(!completion.tool_only_output());
        assert_eq!(completion.usage().input_tokens(), 15);
        assert_eq!(completion.usage().cached_input_tokens(), 5);
        assert_eq!(completion.usage().cache_write_input_tokens(), 3);
        assert_eq!(completion.usage().output_tokens(), 9);
        assert_eq!(completion.stats().output_text_bytes(), 11);
        assert_eq!(completion.stats().events(), 8);

        let mut bytewise =
            AnthropicMessagesStreamDecoder::try_new(call(), &config(64, 1_024)).expect("decoder");
        let mut bytewise_text = String::new();
        for chunk in stream.as_bytes().chunks(1) {
            for delta in bytewise.push(chunk).expect("one-byte chunk").into_deltas() {
                bytewise_text.push_str(delta.as_str());
            }
        }
        assert_eq!(bytewise_text, "hello world");
        assert!(matches!(
            bytewise.finish().expect("bytewise terminal").conclusion(),
            AgentProviderStreamConclusion::Completed(completion)
                if completion.stop() == AgentProviderStopReason::Completed
        ));

        let mut unknown =
            AnthropicMessagesStreamDecoder::try_new(call(), &config(64, 1_024)).expect("decoder");
        unknown
            .push(start("msg_unknown").as_bytes())
            .expect("start");
        assert_eq!(
            unknown.push(
                sse(
                    "future_accounting_hint",
                    r#"{"type":"future_accounting_hint","opaque":{"must":"not escape"}}"#,
                )
                .as_bytes(),
            ),
            Err(AgentProviderProtocolError::UnsupportedOutput)
        );
    }

    #[test]
    fn provider_billing_mode_must_attest_standard_global_processing() {
        for (expected, replacement) in [
            (
                "\"service_tier\":\"standard\"",
                "\"service_tier\":\"priority\"",
            ),
            ("\"inference_geo\":\"global\"", "\"inference_geo\":\"us\""),
        ] {
            let mut decoder = AnthropicMessagesStreamDecoder::try_new(call(), &config(64, 1_024))
                .expect("decoder");
            let mismatched = start("msg_billing").replace(expected, replacement);
            assert_eq!(
                decoder.push(mismatched.as_bytes()),
                Err(AgentProviderProtocolError::Event)
            );
        }
    }

    #[test]
    fn terminal_stop_subphase_rejects_every_intervening_event_without_usage_mutation() {
        for intervening in [
            null_message_delta(99),
            message_delta("tool_use", 99),
            sse("ping", r#"{"type":"ping"}"#),
            sse(
                "content_block_stop",
                r#"{"type":"content_block_stop","index":0}"#,
            ),
            sse(
                "error",
                r#"{"type":"error","error":{"type":"overloaded_error","message":"ignored"}}"#,
            ),
        ] {
            let mut decoder = tool_decoder_awaiting_message_stop();
            assert_eq!(
                decoder.push(intervening.as_bytes()),
                Err(AgentProviderProtocolError::Sequence)
            );
            assert_eq!(
                decoder.usage.expect("usage retained").output_tokens,
                12,
                "a rejected post-stop event must not mutate authenticated usage"
            );
            assert!(matches!(
                decoder.finish(),
                Err(AgentProviderProtocolError::Sequence)
            ));
        }

        let mut null_before_stop =
            AnthropicMessagesStreamDecoder::try_new(call(), &config(64, 1_024)).expect("decoder");
        null_before_stop
            .push(start("msg_null_stop").as_bytes())
            .expect("start");
        assert_eq!(
            null_before_stop.push(null_message_delta(99).as_bytes()),
            Err(AgentProviderProtocolError::Sequence)
        );
        assert_eq!(
            null_before_stop
                .usage
                .expect("usage retained")
                .output_tokens,
            1
        );

        let mut exact = tool_decoder_awaiting_message_stop();
        exact.push(message_stop().as_bytes()).expect("message stop");
        assert!(matches!(
            exact.finish().expect("EOF terminal").conclusion(),
            AgentProviderStreamConclusion::Completed(completion)
                if completion.stop() == AgentProviderStopReason::ToolCalls
                    && completion.usage().output_tokens() == 12
        ));
    }

    #[test]
    fn complete_tool_json_becomes_one_closed_browser_proposal() {
        let mut decoder =
            AnthropicMessagesStreamDecoder::try_new(call(), &config(64, 1_024)).expect("decoder");
        let arguments = r#"{"url":"https://example.test/path"}"#;
        let stream = [
            start("msg_tool"),
            sse(
                "content_block_start",
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"navigate","input":{}}}"#,
            ),
            sse(
                "content_block_delta",
                &serde_json::json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {"type": "input_json_delta", "partial_json": "{\"url\":"}
                })
                .to_string(),
            ),
            sse(
                "content_block_delta",
                &serde_json::json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {"type": "input_json_delta", "partial_json": "\"https://example.test/path\"}"}
                })
                .to_string(),
            ),
            sse(
                "content_block_stop",
                r#"{"type":"content_block_stop","index":0}"#,
            ),
            message_delta("tool_use", 12),
            message_stop(),
        ]
        .concat();
        let batch = decoder.push(stream.as_bytes()).expect("tool stream");
        assert!(batch.deltas().is_empty());
        let finished = decoder.finish().expect("terminal");
        let (conclusion, tool) = finished.into_parts();
        let tool = tool.expect("EOF-private tool");
        assert_eq!(tool.id().as_str(), "toolu_1");
        let (_, proposal) = tool.into_parts();
        let crate::AgentBrowserToolProposal::Navigate(target) = proposal else {
            panic!("navigate");
        };
        assert_eq!(target.as_url().as_str(), "https://example.test/path");
        let AgentProviderStreamConclusion::Completed(completion) = conclusion else {
            panic!("completion");
        };
        assert_eq!(completion.stop(), AgentProviderStopReason::ToolCalls);
        assert!(completion.tool_only_output());
        assert_eq!(completion.stats().tool_calls(), 1);
        assert_eq!(
            completion.stats().tool_argument_bytes(),
            u32::try_from(arguments.len()).expect("argument bytes")
        );
    }

    #[test]
    fn malformed_tool_arguments_fail_stop_before_raw_json_escapes() {
        let mut decoder =
            AnthropicMessagesStreamDecoder::try_new(call(), &config(64, 1_024)).expect("decoder");
        let arguments = r##"{"url":"https://example.test","selector":"#private"}"##;
        let stream = [
            start("msg_bad_tool"),
            sse(
                "content_block_start",
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_bad","name":"navigate","input":{}}}"#,
            ),
            sse(
                "content_block_delta",
                &serde_json::json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {"type": "input_json_delta", "partial_json": arguments}
                })
                .to_string(),
            ),
            sse(
                "content_block_stop",
                r#"{"type":"content_block_stop","index":0}"#,
            ),
        ]
        .concat();
        decoder.push(stream.as_bytes()).expect("tool fragments");
        assert_eq!(
            decoder.push(message_delta("tool_use", 12).as_bytes()),
            Err(AgentProviderProtocolError::ToolCall)
        );
        assert_eq!(decoder.push(b""), Err(AgentProviderProtocolError::ToolCall));
        assert!(!format!("{decoder:?}").contains("#private"));
    }

    #[test]
    fn canonical_empty_tool_input_is_accounted_under_the_argument_ceiling() {
        let tool_start = sse(
            "content_block_start",
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_back","name":"back","input":{}}}"#,
        );
        let tool_stop = sse(
            "content_block_stop",
            r#"{"type":"content_block_stop","index":0}"#,
        );

        let mut decoder =
            AnthropicMessagesStreamDecoder::try_new(call(), &config(64, 2)).expect("decoder");
        let stream = [
            start("msg_empty"),
            tool_start.clone(),
            tool_stop.clone(),
            message_delta("tool_use", 4),
            message_stop(),
        ]
        .concat();
        let batch = decoder.push(stream.as_bytes()).expect("empty tool");
        assert!(batch.deltas().is_empty());
        let AgentProviderStreamConclusion::Completed(completion) =
            decoder.finish().expect("terminal").conclusion()
        else {
            panic!("completion");
        };
        assert_eq!(completion.stats().tool_argument_bytes(), 2);

        let mut limited =
            AnthropicMessagesStreamDecoder::try_new(call(), &config(64, 1)).expect("decoder");
        limited
            .push(start("msg_empty_limit").as_bytes())
            .expect("start");
        limited.push(tool_start.as_bytes()).expect("tool start");
        assert_eq!(
            limited.push(tool_stop.as_bytes()),
            Err(AgentProviderProtocolError::Limit)
        );
    }

    #[test]
    fn incomplete_tool_at_output_limit_never_escapes_as_a_proposal() {
        let mut decoder =
            AnthropicMessagesStreamDecoder::try_new(call(), &config(64, 1_024)).expect("decoder");
        let stream = [
            start("msg_partial"),
            sse(
                "content_block_start",
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_partial","name":"navigate","input":{}}}"#,
            ),
            sse(
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"url\":"}}"#,
            ),
            sse(
                "content_block_stop",
                r#"{"type":"content_block_stop","index":0}"#,
            ),
            message_delta("max_tokens", 20),
            message_stop(),
        ]
        .concat();
        let batch = decoder.push(stream.as_bytes()).expect("limited stream");
        assert!(batch.deltas().is_empty());
        let AgentProviderStreamConclusion::Completed(completion) =
            decoder.finish().expect("terminal").conclusion()
        else {
            panic!("completion");
        };
        assert_eq!(completion.stop(), AgentProviderStopReason::OutputLimit);
        assert_eq!(completion.stats().tool_calls(), 0);
    }

    #[test]
    fn unsupported_blocks_sequence_errors_and_limits_fail_stop() {
        let mut thinking =
            AnthropicMessagesStreamDecoder::try_new(call(), &config(64, 1_024)).expect("decoder");
        thinking.push(start("msg_think").as_bytes()).expect("start");
        assert_eq!(
            thinking.push(
                sse(
                    "content_block_start",
                    r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}"#,
                )
                .as_bytes(),
            ),
            Err(AgentProviderProtocolError::UnsupportedOutput)
        );

        let mut wrong_index =
            AnthropicMessagesStreamDecoder::try_new(call(), &config(64, 1_024)).expect("decoder");
        wrong_index
            .push(start("msg_index").as_bytes())
            .expect("start");
        assert_eq!(
            wrong_index.push(
                sse(
                    "content_block_start",
                    r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#,
                )
                .as_bytes(),
            ),
            Err(AgentProviderProtocolError::Sequence)
        );

        let mut second_tool =
            AnthropicMessagesStreamDecoder::try_new(call(), &config(64, 1_024)).expect("decoder");
        second_tool
            .push(start("msg_parallel").as_bytes())
            .expect("start");
        second_tool
            .push(
                sse(
                    "content_block_start",
                    r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_first","name":"back","input":{}}}"#,
                )
                .as_bytes(),
            )
            .expect("first tool");
        second_tool
            .push(
                sse(
                    "content_block_stop",
                    r#"{"type":"content_block_stop","index":0}"#,
                )
                .as_bytes(),
            )
            .expect("first tool stop");
        assert_eq!(
            second_tool.push(
                sse(
                    "content_block_start",
                    r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_second","name":"forward","input":{}}}"#,
                )
                .as_bytes(),
            ),
            Err(AgentProviderProtocolError::Sequence)
        );

        let mut text_limit =
            AnthropicMessagesStreamDecoder::try_new(call(), &config(3, 1_024)).expect("decoder");
        text_limit
            .push(start("msg_limit").as_bytes())
            .expect("start");
        text_limit
            .push(
                sse(
                    "content_block_start",
                    r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
                )
                .as_bytes(),
            )
            .expect("text start");
        assert_eq!(
            text_limit.push(
                sse(
                    "content_block_delta",
                    r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"four"}}"#,
                )
                .as_bytes(),
            ),
            Err(AgentProviderProtocolError::Limit)
        );
    }

    #[test]
    fn stop_tool_shape_model_and_usage_are_exact() {
        let mut no_tool =
            AnthropicMessagesStreamDecoder::try_new(call(), &config(64, 1_024)).expect("decoder");
        no_tool.push(start("msg_none").as_bytes()).expect("start");
        assert_eq!(
            no_tool.push(message_delta("tool_use", 2).as_bytes()),
            Err(AgentProviderProtocolError::Terminal)
        );

        let mut wrong_model =
            AnthropicMessagesStreamDecoder::try_new(call(), &config(64, 1_024)).expect("decoder");
        let event = start("msg_model").replace("claude-opus-5", "claude-other");
        assert_eq!(
            wrong_model.push(event.as_bytes()),
            Err(AgentProviderProtocolError::Event)
        );

        let mut decreasing =
            AnthropicMessagesStreamDecoder::try_new(call(), &config(64, 1_024)).expect("decoder");
        decreasing
            .push(start("msg_usage").as_bytes())
            .expect("start");
        assert_eq!(
            decreasing.push(message_delta("end_turn", 0).as_bytes()),
            Err(AgentProviderProtocolError::Usage)
        );
    }

    #[test]
    fn midstream_error_is_typed_and_provider_text_is_redacted() {
        let mut decoder =
            AnthropicMessagesStreamDecoder::try_new(call(), &config(64, 1_024)).expect("decoder");
        decoder.push(start("msg_error").as_bytes()).expect("start");
        decoder
            .push(
                sse(
                    "error",
                    r#"{"type":"error","error":{"type":"overloaded_error","message":"sensitive provider detail"}}"#,
                )
                .as_bytes(),
            )
            .expect("error event");
        let conclusion = decoder.finish().expect("terminal failure").conclusion();
        assert!(!format!("{conclusion:?}").contains("sensitive provider detail"));
        let AgentProviderStreamConclusion::Failed(failure) = conclusion else {
            panic!("failure");
        };
        assert_eq!(
            failure.failure().class(),
            AgentProviderFailureClass::Overloaded
        );
        assert_eq!(
            failure.failure().retry_disposition(),
            super::super::AgentProviderRetryDisposition::PolicyMayRetry
        );
        assert_eq!(failure.usage().expect("partial usage").input_tokens(), 15);
    }

    #[test]
    fn post_terminal_done_and_trailing_framing_fail_closed() {
        let terminal_stream = [
            start("msg_eof_gate"),
            message_delta("end_turn", 2),
            message_stop(),
        ]
        .concat();

        let mut post_terminal =
            AnthropicMessagesStreamDecoder::try_new(call(), &config(64, 1_024)).expect("decoder");
        let terminal_batch = post_terminal
            .push(terminal_stream.as_bytes())
            .expect("terminal events");
        assert!(terminal_batch.deltas().is_empty());
        assert_eq!(
            post_terminal.push(sse("ping", r#"{"type":"ping"}"#).as_bytes()),
            Err(AgentProviderProtocolError::Sequence)
        );

        let mut done =
            AnthropicMessagesStreamDecoder::try_new(call(), &config(64, 1_024)).expect("decoder");
        done.push(start("msg_done").as_bytes()).expect("start");
        assert_eq!(
            done.push(b"data: [DONE]\n\n"),
            Err(AgentProviderProtocolError::Event)
        );

        let mut trailing =
            AnthropicMessagesStreamDecoder::try_new(call(), &config(64, 1_024)).expect("decoder");
        trailing
            .push(terminal_stream.as_bytes())
            .expect("terminal events");
        trailing
            .push(b"data: unfinished")
            .expect("buffer bounded trailing bytes");
        assert!(matches!(
            trailing.finish(),
            Err(AgentProviderProtocolError::Framing)
        ));
    }
}
