//! Bounded OpenAI Responses SSE normalization.
//!
//! The decoder accepts response lifecycle, plain-text/refusal output, the
//! fixed client browser-tool vocabulary, and terminal usage. Function-call
//! JSON remains private, is bounded while streaming, and crosses the public
//! boundary only after the closed browser-tool decoder succeeds. Provider
//! built-in tools and plaintext reasoning are never requested or surfaced.
//! Encrypted reasoning is retained only as a bounded opaque stateless-replay
//! item and cannot cross the tool-correlation boundary.

use std::borrow::Cow;
use std::fmt;

use serde::de::{Error as _, IgnoredAny, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::sse::{SseDecoder, SseEvent};
use super::tool::{
    OpenAiResponseReplay, OpenAiResponseReplayItem, MAX_OPENAI_ENCRYPTED_REASONING_AGGREGATE_BYTES,
    MAX_OPENAI_ENCRYPTED_REASONING_ITEM_BYTES,
};
use super::{
    AgentBrowserToolCall, AgentBrowserToolCallId, AgentBrowserToolKind, AgentProviderCallConfig,
    AgentProviderCallIdentity, AgentProviderCompletion, AgentProviderFailure,
    AgentProviderFailureClass, AgentProviderFinishedStream, AgentProviderKind,
    AgentProviderProtocolError, AgentProviderProtocolEvent, AgentProviderResponseIdentity,
    AgentProviderStopReason, AgentProviderStreamBatch, AgentProviderStreamBudget,
    AgentProviderStreamConclusion, AgentProviderStreamStats, AgentProviderTerminalFailure,
    AgentProviderTextDelta, AgentProviderUsage,
};

const MAX_OPENAI_RESPONSE_ID_BYTES: usize = 128;
const MAX_OPENAI_TERMINAL_OUTPUT_ITEMS: usize = 16;
const MAX_OPENAI_TERMINAL_CONTENT_PARTS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamPhase {
    AwaitCreated,
    InProgress,
    Terminal,
}

enum OutputState {
    None,
    Text {
        hash: Sha256,
        bytes: u32,
        done: bool,
    },
    Refusal {
        hash: Sha256,
        bytes: u32,
        done: bool,
    },
}

impl OutputState {
    fn bytes(&self) -> u32 {
        match self {
            Self::None => 0,
            Self::Text { bytes, .. } | Self::Refusal { bytes, .. } => *bytes,
        }
    }

    fn is_done(&self) -> bool {
        match self {
            Self::None => true,
            Self::Text { done, .. } | Self::Refusal { done, .. } => *done,
        }
    }

    fn is_refusal(&self) -> bool {
        matches!(self, Self::Refusal { .. })
    }
}

struct ToolAccumulator {
    item_id: String,
    call_id: String,
    name: AgentBrowserToolKind,
    arguments: Option<String>,
    argument_bytes: u32,
    argument_guard: Option<[u8; 32]>,
    item_done: bool,
    call: Option<AgentBrowserToolCall>,
}

enum OutputItemAccumulator {
    Message {
        item_id: String,
        done: bool,
    },
    Reasoning {
        item_id: String,
        encrypted_content: Option<String>,
        done: bool,
    },
    FunctionCall {
        tool_index: usize,
    },
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ContentPartKind {
    OutputText,
    Refusal,
}

struct ContentPartAccumulator {
    item_id: String,
    output_index: usize,
    content_index: usize,
    kind: ContentPartKind,
    output_done: bool,
    part_done: bool,
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
        identity: AgentProviderResponseIdentity,
    },
}

/// Single-owner incremental decoder for one OpenAI Responses SSE body.
///
/// The HTTP shell supplies arbitrary byte chunks and must call `finish` once
/// EOF is reached. Protocol errors are content-free. A terminal provider
/// failure is returned as a typed conclusion rather than being confused with
/// malformed wire data.
#[must_use]
pub(super) struct OpenAiResponsesStreamDecoder {
    call: AgentProviderCallIdentity,
    config: AgentProviderCallConfig,
    budget: AgentProviderStreamBudget,
    sse: SseDecoder,
    phase: StreamPhase,
    response_id: Option<String>,
    response_identity: Option<AgentProviderResponseIdentity>,
    saw_in_progress: bool,
    output: OutputState,
    content_part: Option<ContentPartAccumulator>,
    output_items: Vec<OutputItemAccumulator>,
    tools: Vec<ToolAccumulator>,
    tool_argument_bytes: u32,
    encrypted_reasoning_bytes: usize,
    terminal: Option<PendingTerminal>,
    saw_done: bool,
    failure: Option<AgentProviderProtocolError>,
    protocol_event: Option<AgentProviderProtocolEvent>,
}

impl OpenAiResponsesStreamDecoder {
    /// Starts one decoder for an exact call and OpenAI Responses configuration.
    pub fn try_new(
        call: AgentProviderCallIdentity,
        config: &AgentProviderCallConfig,
    ) -> Result<Self, AgentProviderProtocolError> {
        if config.provider() != AgentProviderKind::OpenAiResponses {
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
            phase: StreamPhase::AwaitCreated,
            response_id: None,
            response_identity: None,
            saw_in_progress: false,
            output: OutputState::None,
            content_part: None,
            output_items: Vec::with_capacity(MAX_OPENAI_TERMINAL_OUTPUT_ITEMS),
            tools: Vec::with_capacity(usize::from(config.stream_budget().max_tool_calls())),
            tool_argument_bytes: 0,
            encrypted_reasoning_bytes: 0,
            terminal: None,
            saw_done: false,
            failure: None,
            protocol_event: None,
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
            } => {
                if !self.output.is_done() {
                    return Err(AgentProviderProtocolError::Terminal);
                }
                let tool = if stop == AgentProviderStopReason::ToolCalls {
                    Some(
                        self.tools
                            .first_mut()
                            .and_then(|tool| tool.call.take())
                            .ok_or(AgentProviderProtocolError::Terminal)?,
                    )
                } else {
                    None
                };
                (
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
                    tool,
                )
            }
            PendingTerminal::Failed {
                failure,
                usage,
                identity,
            } => (
                AgentProviderStreamConclusion::Failed(
                    AgentProviderTerminalFailure::new_with_response_identity(
                        self.call, failure, usage, stats, identity,
                    ),
                ),
                None,
            ),
        };
        AgentProviderFinishedStream::new(conclusion, tool)
    }

    fn handle_event(
        &mut self,
        event: SseEvent,
        output: &mut Vec<AgentProviderTextDelta>,
    ) -> Result<(), AgentProviderProtocolError> {
        self.protocol_event = Some(openai_protocol_event(&event));
        if event.data() == "[DONE]" {
            if self.phase != StreamPhase::Terminal || self.saw_done {
                return Err(AgentProviderProtocolError::Terminal);
            }
            self.saw_done = true;
            return Ok(());
        }
        let kind = parse_event_type(event.data())?;
        if event.event() != kind {
            return Err(AgentProviderProtocolError::Event);
        }
        match kind {
            "response.created" => self.handle_created(event.data()),
            "response.in_progress" => self.handle_in_progress(event.data()),
            "response.output_item.added" | "response.output_item.done" => {
                self.handle_output_item(event.data())
            }
            "response.content_part.added" | "response.content_part.done" => {
                self.handle_content_part(event.data())
            }
            "response.output_text.delta" => self.handle_output_delta(event.data(), false, output),
            "response.output_text.done" => self.handle_output_done(event.data(), false),
            "response.refusal.delta" => self.handle_output_delta(event.data(), true, output),
            "response.refusal.done" => self.handle_output_done(event.data(), true),
            "response.function_call_arguments.delta" => {
                self.handle_tool_arguments_delta(event.data())
            }
            "response.function_call_arguments.done" => {
                self.handle_tool_arguments_done(event.data())
            }
            "response.completed" | "response.incomplete" => {
                self.handle_success_terminal(event.data(), kind)
            }
            "response.failed" | "response.cancelled" => {
                self.handle_failure_terminal(event.data(), kind)
            }
            "error" => self.handle_stream_error(event.data()),
            "ping" => self.require_in_progress(),
            "response.reasoning_text.delta"
            | "response.reasoning_text.done"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_summary_text.done" => {
                Err(AgentProviderProtocolError::UnsupportedOutput)
            }
            _ => Err(AgentProviderProtocolError::UnsupportedOutput),
        }
    }

    #[cfg(feature = "provider-transport")]
    pub(super) const fn protocol_event(&self) -> Option<AgentProviderProtocolEvent> {
        self.protocol_event
    }

    fn handle_created(&mut self, data: &str) -> Result<(), AgentProviderProtocolError> {
        if self.phase != StreamPhase::AwaitCreated {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let event: ResponseEnvelope<'_> = parse(data)?;
        if event.kind != "response.created"
            || event.response.status != "in_progress"
            || event.response.service_tier != self.config.response_route().response_service_tier()
        {
            return Err(AgentProviderProtocolError::Event);
        }
        validate_response_id(event.response.id)?;
        let identity = AgentProviderResponseIdentity::try_attested(
            &self.config,
            event.response.model,
            event.response.service_tier,
            None,
        )?;
        self.response_id = Some(event.response.id.to_owned());
        self.response_identity = Some(identity);
        self.phase = StreamPhase::InProgress;
        Ok(())
    }

    fn handle_in_progress(&mut self, data: &str) -> Result<(), AgentProviderProtocolError> {
        self.require_in_progress()?;
        if self.saw_in_progress {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let event: ResponseEnvelope<'_> = parse(data)?;
        if event.kind != "response.in_progress"
            || event.response.status != "in_progress"
            || !self.response_identity_matches(event.response.model, event.response.service_tier)
        {
            return Err(AgentProviderProtocolError::Event);
        }
        self.validate_response_id(event.response.id)?;
        self.saw_in_progress = true;
        Ok(())
    }

    fn handle_output_item(&mut self, data: &str) -> Result<(), AgentProviderProtocolError> {
        self.require_in_progress()?;
        let event: OutputItemEnvelope<'_> = parse(data)?;
        if !matches!(
            event.kind,
            "response.output_item.added" | "response.output_item.done"
        ) {
            return Err(AgentProviderProtocolError::Event);
        }
        if event.kind == "response.output_item.added"
            && self.output_items.len() >= MAX_OPENAI_TERMINAL_OUTPUT_ITEMS
        {
            return Err(AgentProviderProtocolError::Limit);
        }
        match (event.kind, event.item.kind) {
            ("response.output_item.added", "message") => {
                self.handle_message_item_added(event.output_index, event.item)
            }
            ("response.output_item.done", "message") => {
                self.handle_message_item_done(event.output_index, event.item)
            }
            ("response.output_item.added", "reasoning") => {
                self.handle_reasoning_item_added(event.output_index, event.item)
            }
            ("response.output_item.done", "reasoning") => {
                self.handle_reasoning_item_done(event.output_index, event.item)
            }
            ("response.output_item.added", "function_call") => {
                if event.output_index != self.output_items.len() {
                    return Err(AgentProviderProtocolError::Sequence);
                }
                let tool_index = self.handle_tool_item_added(event.item)?;
                self.output_items
                    .push(OutputItemAccumulator::FunctionCall { tool_index });
                Ok(())
            }
            ("response.output_item.done", "function_call") => {
                let tool_index = match self.output_items.get(event.output_index) {
                    Some(OutputItemAccumulator::FunctionCall { tool_index }) => *tool_index,
                    _ => return Err(AgentProviderProtocolError::Sequence),
                };
                self.handle_tool_item_done(tool_index, event.item)
            }
            _ => Err(AgentProviderProtocolError::UnsupportedOutput),
        }
    }

    fn handle_message_item_added(
        &mut self,
        output_index: usize,
        item: OutputItemHead<'_>,
    ) -> Result<(), AgentProviderProtocolError> {
        let item_id = item.id.ok_or(AgentProviderProtocolError::Event)?;
        if output_index != self.output_items.len()
            || item.status != Some("in_progress")
            || item.role != Some("assistant")
            || item.call_id.is_some()
            || item.name.is_some()
            || item.arguments.is_some()
            || item.summary.is_some()
            || item.encrypted_content.is_some()
            || self
                .output_items
                .iter()
                .any(|item| matches!(item, OutputItemAccumulator::Message { .. }))
            || self.output_item_id_exists(item_id)
        {
            return Err(AgentProviderProtocolError::Sequence);
        }
        validate_response_id(item_id)?;
        self.output_items.push(OutputItemAccumulator::Message {
            item_id: item_id.to_owned(),
            done: false,
        });
        Ok(())
    }

    fn handle_message_item_done(
        &mut self,
        output_index: usize,
        item: OutputItemHead<'_>,
    ) -> Result<(), AgentProviderProtocolError> {
        let item_id = item.id.ok_or(AgentProviderProtocolError::Event)?;
        let Some(OutputItemAccumulator::Message {
            item_id: expected,
            done,
        }) = self.output_items.get_mut(output_index)
        else {
            return Err(AgentProviderProtocolError::Sequence);
        };
        if *done
            || expected != item_id
            || item.status != Some("completed")
            || item.role != Some("assistant")
            || item.call_id.is_some()
            || item.name.is_some()
            || item.arguments.is_some()
            || item.summary.is_some()
            || item.encrypted_content.is_some()
            || self.content_part.as_ref().is_none_or(|part| {
                part.item_id != item_id
                    || part.output_index != output_index
                    || !part.output_done
                    || !part.part_done
            })
        {
            return Err(AgentProviderProtocolError::Sequence);
        }
        *done = true;
        Ok(())
    }

    fn handle_reasoning_item_added(
        &mut self,
        output_index: usize,
        item: OutputItemHead<'_>,
    ) -> Result<(), AgentProviderProtocolError> {
        let item_id = item.id.ok_or(AgentProviderProtocolError::Event)?;
        if let Some(encrypted_content) = item.encrypted_content.as_deref() {
            validate_encrypted_reasoning(encrypted_content)?;
        }
        if output_index != self.output_items.len()
            || !matches!(item.status, None | Some("in_progress"))
            || item.role.is_some()
            || item.call_id.is_some()
            || item.name.is_some()
            || item.arguments.is_some()
            || item
                .summary
                .as_ref()
                .is_none_or(|summary| !summary.is_empty())
            || self.output_item_id_exists(item_id)
        {
            return Err(AgentProviderProtocolError::Sequence);
        }
        validate_response_id(item_id)?;
        self.output_items.push(OutputItemAccumulator::Reasoning {
            item_id: item_id.to_owned(),
            encrypted_content: None,
            done: false,
        });
        Ok(())
    }

    fn handle_reasoning_item_done(
        &mut self,
        output_index: usize,
        item: OutputItemHead<'_>,
    ) -> Result<(), AgentProviderProtocolError> {
        let item_id = item.id.ok_or(AgentProviderProtocolError::Event)?;
        let encrypted_content = item
            .encrypted_content
            .ok_or(AgentProviderProtocolError::UnsupportedOutput)?;
        validate_encrypted_reasoning(&encrypted_content)?;
        if !matches!(item.status, None | Some("completed"))
            || item.role.is_some()
            || item.call_id.is_some()
            || item.name.is_some()
            || item.arguments.is_some()
            || item
                .summary
                .as_ref()
                .is_none_or(|summary| !summary.is_empty())
        {
            return Err(AgentProviderProtocolError::UnsupportedOutput);
        }
        let next_encrypted_bytes = self
            .encrypted_reasoning_bytes
            .checked_add(encrypted_content.len())
            .filter(|bytes| *bytes <= MAX_OPENAI_ENCRYPTED_REASONING_AGGREGATE_BYTES)
            .ok_or(AgentProviderProtocolError::Limit)?;
        if self.output_items.iter().any(|existing| {
            matches!(
                existing,
                OutputItemAccumulator::Reasoning {
                    encrypted_content: Some(retained),
                    ..
                } if retained.as_str() == encrypted_content.as_ref()
            )
        }) {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let Some(OutputItemAccumulator::Reasoning {
            item_id: expected,
            encrypted_content: retained,
            done,
        }) = self.output_items.get_mut(output_index)
        else {
            return Err(AgentProviderProtocolError::Sequence);
        };
        if *done || expected != item_id || retained.is_some() {
            return Err(AgentProviderProtocolError::Sequence);
        }
        *retained = Some(encrypted_content.into_owned());
        *done = true;
        self.encrypted_reasoning_bytes = next_encrypted_bytes;
        Ok(())
    }

    fn output_item_id_exists(&self, item_id: &str) -> bool {
        self.output_items.iter().any(|item| match item {
            OutputItemAccumulator::Message {
                item_id: existing, ..
            }
            | OutputItemAccumulator::Reasoning {
                item_id: existing, ..
            } => existing == item_id,
            OutputItemAccumulator::FunctionCall { tool_index } => self
                .tools
                .get(*tool_index)
                .is_some_and(|tool| tool.item_id == item_id),
        })
    }

    fn handle_tool_item_added(
        &mut self,
        item: OutputItemHead<'_>,
    ) -> Result<usize, AgentProviderProtocolError> {
        if self.tools.len() >= usize::from(self.budget.max_tool_calls()) {
            return Err(AgentProviderProtocolError::Limit);
        }
        let item_id = item.id.ok_or(AgentProviderProtocolError::Event)?;
        let call_id = item.call_id.ok_or(AgentProviderProtocolError::Event)?;
        let name = item.name.ok_or(AgentProviderProtocolError::Event)?;
        let arguments = item.arguments.ok_or(AgentProviderProtocolError::Event)?;
        if item.status != Some("in_progress")
            || !arguments.is_empty()
            || item.role.is_some()
            || item.summary.is_some()
            || item.encrypted_content.is_some()
        {
            return Err(AgentProviderProtocolError::Sequence);
        }
        validate_response_id(item_id)?;
        AgentBrowserToolCallId::try_new(call_id.to_owned())
            .map_err(|_| AgentProviderProtocolError::ToolCall)?;
        let name = AgentBrowserToolKind::parse(name).ok_or(AgentProviderProtocolError::ToolCall)?;
        if self.output_item_id_exists(item_id)
            || self
                .tools
                .iter()
                .any(|tool| tool.item_id == item_id || tool.call_id == call_id)
        {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let tool_index = self.tools.len();
        self.tools.push(ToolAccumulator {
            item_id: item_id.to_owned(),
            call_id: call_id.to_owned(),
            name,
            arguments: Some(String::new()),
            argument_bytes: 0,
            argument_guard: None,
            item_done: false,
            call: None,
        });
        Ok(tool_index)
    }

    fn handle_tool_item_done(
        &mut self,
        tool_index: usize,
        item: OutputItemHead<'_>,
    ) -> Result<(), AgentProviderProtocolError> {
        let item_id = item.id.ok_or(AgentProviderProtocolError::Event)?;
        let call_id = item.call_id.ok_or(AgentProviderProtocolError::Event)?;
        let name = item.name.ok_or(AgentProviderProtocolError::Event)?;
        let arguments = item.arguments.ok_or(AgentProviderProtocolError::Event)?;
        let tool = self
            .tools
            .get_mut(tool_index)
            .ok_or(AgentProviderProtocolError::Sequence)?;
        let argument_guard = tool
            .argument_guard
            .ok_or(AgentProviderProtocolError::Sequence)?;
        let actual_guard: [u8; 32] = Sha256::digest(arguments.as_bytes()).into();
        if tool.item_done
            || item.status != Some("completed")
            || tool.item_id != item_id
            || tool.call_id != call_id
            || tool.name.as_str() != name
            || item.role.is_some()
            || item.summary.is_some()
            || item.encrypted_content.is_some()
            || usize::try_from(tool.argument_bytes).ok() != Some(arguments.len())
            || argument_guard != actual_guard
        {
            return Err(AgentProviderProtocolError::Sequence);
        }
        tool.item_done = true;
        Ok(())
    }

    fn handle_content_part(&mut self, data: &str) -> Result<(), AgentProviderProtocolError> {
        self.require_in_progress()?;
        let event: ContentPartEnvelope<'_> = parse(data)?;
        if !matches!(
            event.kind,
            "response.content_part.added" | "response.content_part.done"
        ) {
            return Err(AgentProviderProtocolError::Event);
        }
        let kind = match event.part.kind {
            "output_text" => ContentPartKind::OutputText,
            "refusal" => ContentPartKind::Refusal,
            _ => return Err(AgentProviderProtocolError::UnsupportedOutput),
        };
        match event.kind {
            "response.content_part.added" => {
                if event.content_index != 0 || self.content_part.is_some() {
                    return Err(AgentProviderProtocolError::Sequence);
                }
                let Some(OutputItemAccumulator::Message {
                    item_id,
                    done: false,
                }) = self.output_items.get(event.output_index)
                else {
                    return Err(AgentProviderProtocolError::Sequence);
                };
                if item_id != event.item_id {
                    return Err(AgentProviderProtocolError::Sequence);
                }
                self.content_part = Some(ContentPartAccumulator {
                    item_id: event.item_id.to_owned(),
                    output_index: event.output_index,
                    content_index: event.content_index,
                    kind,
                    output_done: false,
                    part_done: false,
                });
                Ok(())
            }
            "response.content_part.done" => {
                let Some(part) = self.content_part.as_mut() else {
                    return Err(AgentProviderProtocolError::Sequence);
                };
                if part.item_id != event.item_id
                    || part.output_index != event.output_index
                    || part.content_index != event.content_index
                    || part.kind != kind
                    || !part.output_done
                    || part.part_done
                {
                    return Err(AgentProviderProtocolError::Sequence);
                }
                part.part_done = true;
                Ok(())
            }
            _ => Err(AgentProviderProtocolError::Event),
        }
    }

    fn handle_output_delta(
        &mut self,
        data: &str,
        refusal: bool,
        output: &mut Vec<AgentProviderTextDelta>,
    ) -> Result<(), AgentProviderProtocolError> {
        self.require_in_progress()?;
        let event: DeltaEnvelope<'_> = parse(data)?;
        let expected = if refusal {
            "response.refusal.delta"
        } else {
            "response.output_text.delta"
        };
        if event.kind != expected {
            return Err(AgentProviderProtocolError::Event);
        }
        self.validate_content_event(
            event.item_id,
            event.output_index,
            event.content_index,
            refusal,
            false,
        )?;
        let delta_bytes =
            u32::try_from(event.delta.len()).map_err(|_| AgentProviderProtocolError::Limit)?;
        let state = match (&mut self.output, refusal) {
            (OutputState::None, false) => {
                self.output = OutputState::Text {
                    hash: Sha256::new(),
                    bytes: 0,
                    done: false,
                };
                &mut self.output
            }
            (OutputState::None, true) => {
                self.output = OutputState::Refusal {
                    hash: Sha256::new(),
                    bytes: 0,
                    done: false,
                };
                &mut self.output
            }
            (OutputState::Text { .. }, false) | (OutputState::Refusal { .. }, true) => {
                &mut self.output
            }
            _ => return Err(AgentProviderProtocolError::Sequence),
        };
        let (hash, bytes, done) = match state {
            OutputState::Text { hash, bytes, done }
            | OutputState::Refusal { hash, bytes, done } => (hash, bytes, done),
            OutputState::None => return Err(AgentProviderProtocolError::Sequence),
        };
        if *done {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let next = bytes
            .checked_add(delta_bytes)
            .ok_or(AgentProviderProtocolError::Limit)?;
        if next > self.budget.max_output_text_bytes() {
            return Err(AgentProviderProtocolError::Limit);
        }
        hash.update(event.delta.as_bytes());
        *bytes = next;
        if !event.delta.is_empty() {
            output.push(AgentProviderTextDelta::new(event.delta.into_owned()));
        }
        Ok(())
    }

    fn handle_output_done(
        &mut self,
        data: &str,
        refusal: bool,
    ) -> Result<(), AgentProviderProtocolError> {
        self.require_in_progress()?;
        let event: DoneEnvelope<'_> = parse(data)?;
        let expected = if refusal {
            "response.refusal.done"
        } else {
            "response.output_text.done"
        };
        if event.kind != expected {
            return Err(AgentProviderProtocolError::Event);
        }
        self.validate_content_event(
            event.item_id,
            event.output_index,
            event.content_index,
            refusal,
            false,
        )?;
        let state = match (&mut self.output, refusal) {
            (OutputState::None, false) => {
                self.output = OutputState::Text {
                    hash: Sha256::new(),
                    bytes: 0,
                    done: false,
                };
                &mut self.output
            }
            (OutputState::None, true) => {
                self.output = OutputState::Refusal {
                    hash: Sha256::new(),
                    bytes: 0,
                    done: false,
                };
                &mut self.output
            }
            (OutputState::Text { .. }, false) | (OutputState::Refusal { .. }, true) => {
                &mut self.output
            }
            _ => return Err(AgentProviderProtocolError::Sequence),
        };
        let (hash, bytes, done) = match state {
            OutputState::Text { hash, bytes, done }
            | OutputState::Refusal { hash, bytes, done } => (hash, bytes, done),
            OutputState::None => return Err(AgentProviderProtocolError::Sequence),
        };
        if *done || usize::try_from(*bytes).ok() != Some(event.text.len()) {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let expected_hash = hash.clone().finalize();
        let actual_hash = Sha256::digest(event.text.as_bytes());
        if expected_hash.as_slice() != actual_hash.as_slice() {
            return Err(AgentProviderProtocolError::Sequence);
        }
        *done = true;
        let Some(part) = self.content_part.as_mut() else {
            return Err(AgentProviderProtocolError::Sequence);
        };
        part.output_done = true;
        Ok(())
    }

    fn validate_content_event(
        &self,
        item_id: &str,
        output_index: usize,
        content_index: usize,
        refusal: bool,
        require_output_done: bool,
    ) -> Result<(), AgentProviderProtocolError> {
        let expected_kind = if refusal {
            ContentPartKind::Refusal
        } else {
            ContentPartKind::OutputText
        };
        let Some(part) = self.content_part.as_ref() else {
            return Err(AgentProviderProtocolError::Sequence);
        };
        if part.item_id != item_id
            || part.output_index != output_index
            || part.content_index != content_index
            || part.kind != expected_kind
            || part.part_done
            || part.output_done != require_output_done
        {
            return Err(AgentProviderProtocolError::Sequence);
        }
        Ok(())
    }

    fn handle_tool_arguments_delta(
        &mut self,
        data: &str,
    ) -> Result<(), AgentProviderProtocolError> {
        self.require_in_progress()?;
        let event: ToolArgumentsDeltaEnvelope<'_> = parse(data)?;
        if event.kind != "response.function_call_arguments.delta" {
            return Err(AgentProviderProtocolError::Event);
        }
        let delta_bytes =
            u32::try_from(event.delta.len()).map_err(|_| AgentProviderProtocolError::Limit)?;
        let next_total = self
            .tool_argument_bytes
            .checked_add(delta_bytes)
            .ok_or(AgentProviderProtocolError::Limit)?;
        if next_total > self.budget.max_tool_argument_bytes() {
            return Err(AgentProviderProtocolError::Limit);
        }
        let tool_index = self
            .tools
            .iter()
            .position(|tool| tool.item_id == event.item_id)
            .ok_or(AgentProviderProtocolError::Sequence)?;
        if event.output_index.is_some_and(|output_index| {
            !matches!(
                self.output_items.get(output_index),
                Some(OutputItemAccumulator::FunctionCall {
                    tool_index: expected,
                }) if *expected == tool_index
            )
        }) {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let tool = self
            .tools
            .get_mut(tool_index)
            .ok_or(AgentProviderProtocolError::Sequence)?;
        let arguments = tool
            .arguments
            .as_mut()
            .ok_or(AgentProviderProtocolError::Sequence)?;
        if tool.item_done || tool.argument_guard.is_some() {
            return Err(AgentProviderProtocolError::Sequence);
        }
        arguments.push_str(event.delta.as_ref());
        tool.argument_bytes = tool
            .argument_bytes
            .checked_add(delta_bytes)
            .ok_or(AgentProviderProtocolError::Limit)?;
        self.tool_argument_bytes = next_total;
        Ok(())
    }

    fn handle_tool_arguments_done(&mut self, data: &str) -> Result<(), AgentProviderProtocolError> {
        self.require_in_progress()?;
        let event: ToolArgumentsDoneEnvelope<'_> = parse(data)?;
        if event.kind != "response.function_call_arguments.done" {
            return Err(AgentProviderProtocolError::Event);
        }
        let tool_index = self
            .tools
            .iter()
            .position(|tool| tool.item_id == event.item_id)
            .ok_or(AgentProviderProtocolError::Sequence)?;
        if event.output_index.is_some_and(|output_index| {
            !matches!(
                self.output_items.get(output_index),
                Some(OutputItemAccumulator::FunctionCall {
                    tool_index: expected,
                }) if *expected == tool_index
            )
        }) {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let tool = self
            .tools
            .get_mut(tool_index)
            .ok_or(AgentProviderProtocolError::Sequence)?;
        if tool.item_done
            || tool.argument_guard.is_some()
            || event.name.is_some_and(|name| tool.name.as_str() != name)
        {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let mut arguments = tool
            .arguments
            .take()
            .ok_or(AgentProviderProtocolError::Sequence)?;
        if arguments.is_empty() && !event.arguments.is_empty() {
            let argument_bytes = u32::try_from(event.arguments.len())
                .map_err(|_| AgentProviderProtocolError::Limit)?;
            let next_total = self
                .tool_argument_bytes
                .checked_add(argument_bytes)
                .ok_or(AgentProviderProtocolError::Limit)?;
            if next_total > self.budget.max_tool_argument_bytes() {
                return Err(AgentProviderProtocolError::Limit);
            }
            arguments = event.arguments.into_owned();
            tool.argument_bytes = argument_bytes;
            self.tool_argument_bytes = next_total;
        } else if arguments.as_str() != event.arguments.as_ref() {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let guard: [u8; 32] = Sha256::digest(arguments.as_bytes()).into();
        let call = AgentBrowserToolCall::decode_openai_with_replay(
            self.call,
            tool.item_id.clone(),
            tool.call_id.clone(),
            tool.name.as_str(),
            arguments,
            None,
        )
        .map_err(|_| AgentProviderProtocolError::ToolCall)?;
        tool.argument_guard = Some(guard);
        tool.call = Some(call);
        Ok(())
    }

    fn handle_success_terminal(
        &mut self,
        data: &str,
        kind: &str,
    ) -> Result<(), AgentProviderProtocolError> {
        self.require_in_progress()?;
        if !self.output.is_done()
            || self.tools.iter().any(|tool| !tool.item_done)
            || self.output_items.iter().any(|item| match item {
                OutputItemAccumulator::Message { done, .. }
                | OutputItemAccumulator::Reasoning { done, .. } => !done,
                OutputItemAccumulator::FunctionCall { .. } => false,
            })
        {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let event: TerminalEnvelope<'_> = parse(data)?;
        if event.kind != kind
            || !self.response_identity_matches(event.response.model, event.response.service_tier)
            || event.response.status
                != if kind == "response.completed" {
                    "completed"
                } else {
                    "incomplete"
                }
        {
            return Err(AgentProviderProtocolError::Event);
        }
        self.validate_response_id(event.response.id)?;
        let validated_output = validate_terminal_output(
            &event.response.output,
            &self.output_items,
            self.content_part.as_ref(),
            &self.tools,
        )?;
        if !validated_output.kind.matches_state(&self.output) {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let tool_only_output = !self.tools.is_empty()
            && validated_output.kind == TerminalOutputKind::None
            && validated_output.replay.is_some();
        let usage = event
            .response
            .usage
            .ok_or(AgentProviderProtocolError::Usage)?
            .normalize()?;
        let stop = if self.output.is_refusal() {
            AgentProviderStopReason::Refused
        } else if kind == "response.completed" && !self.tools.is_empty() {
            AgentProviderStopReason::ToolCalls
        } else if kind == "response.completed" {
            AgentProviderStopReason::Completed
        } else {
            match event
                .response
                .incomplete_details
                .ok_or(AgentProviderProtocolError::Terminal)?
                .reason
            {
                "max_output_tokens" => AgentProviderStopReason::OutputLimit,
                "content_filter" => AgentProviderStopReason::ContentFiltered,
                _ => return Err(AgentProviderProtocolError::Terminal),
            }
        };
        let identity = self
            .response_identity
            .ok_or(AgentProviderProtocolError::Sequence)?;
        if stop == AgentProviderStopReason::ToolCalls {
            let replay = validated_output
                .replay
                .ok_or(AgentProviderProtocolError::UnsupportedOutput)?;
            let tool = self
                .tools
                .first_mut()
                .ok_or(AgentProviderProtocolError::Sequence)?;
            tool.call
                .as_mut()
                .ok_or(AgentProviderProtocolError::Sequence)?
                .attach_openai_replay(replay);
        }
        self.terminal = Some(PendingTerminal::Completed {
            stop,
            usage,
            tool_only_output,
            identity,
        });
        self.phase = StreamPhase::Terminal;
        Ok(())
    }

    fn handle_failure_terminal(
        &mut self,
        data: &str,
        kind: &str,
    ) -> Result<(), AgentProviderProtocolError> {
        self.require_in_progress()?;
        let event: TerminalEnvelope<'_> = parse(data)?;
        let expected_status = if kind == "response.failed" {
            "failed"
        } else {
            "cancelled"
        };
        if event.kind != kind
            || event.response.status != expected_status
            || !self.response_identity_matches(event.response.model, event.response.service_tier)
        {
            return Err(AgentProviderProtocolError::Event);
        }
        self.validate_response_id(event.response.id)?;
        let usage = event
            .response
            .usage
            .map(OpenAiUsage::normalize)
            .transpose()?;
        let class = if kind == "response.cancelled" {
            AgentProviderFailureClass::Cancelled
        } else {
            AgentProviderFailureClass::Provider
        };
        let failure = AgentProviderFailure::try_new(class, None)
            .map_err(|_| AgentProviderProtocolError::Event)?;
        let identity = self
            .response_identity
            .ok_or(AgentProviderProtocolError::Sequence)?;
        self.terminal = Some(PendingTerminal::Failed {
            failure,
            usage,
            identity,
        });
        self.phase = StreamPhase::Terminal;
        Ok(())
    }

    fn handle_stream_error(&mut self, data: &str) -> Result<(), AgentProviderProtocolError> {
        self.require_in_progress()?;
        let event: ErrorEnvelope<'_> = parse(data)?;
        if event.kind != "error"
            || (event.error.code.is_empty() && event.error.error_type.is_empty())
        {
            return Err(AgentProviderProtocolError::Event);
        }
        let failure = AgentProviderFailure::try_new(AgentProviderFailureClass::Provider, None)
            .map_err(|_| AgentProviderProtocolError::Event)?;
        let identity = self
            .response_identity
            .ok_or(AgentProviderProtocolError::Sequence)?;
        self.terminal = Some(PendingTerminal::Failed {
            failure,
            usage: None,
            identity,
        });
        self.phase = StreamPhase::Terminal;
        Ok(())
    }

    fn require_in_progress(&self) -> Result<(), AgentProviderProtocolError> {
        if self.phase == StreamPhase::InProgress {
            Ok(())
        } else {
            Err(AgentProviderProtocolError::Sequence)
        }
    }

    fn validate_response_id(&self, response_id: &str) -> Result<(), AgentProviderProtocolError> {
        if self.response_id.as_deref() == Some(response_id) {
            Ok(())
        } else {
            Err(AgentProviderProtocolError::Sequence)
        }
    }

    fn response_identity_matches(&self, model: &str, service_tier: &str) -> bool {
        self.response_identity
            .is_some_and(|identity| identity.matches_attestation(model, service_tier, None))
    }

    fn stats(&self) -> AgentProviderStreamStats {
        AgentProviderStreamStats::new(
            self.sse.wire_bytes(),
            self.sse.events(),
            self.output.bytes(),
            u8::try_from(self.tools.len()).unwrap_or(self.budget.max_tool_calls()),
            self.tool_argument_bytes,
        )
    }
}

impl fmt::Debug for OpenAiResponsesStreamDecoder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenAiResponsesStreamDecoder")
            .field("call", &self.call)
            .field("config", &self.config)
            .field("has_response_identity", &self.response_identity.is_some())
            .field("budget", &self.budget)
            .field("phase", &self.phase)
            .field("has_response_id", &self.response_id.is_some())
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
struct ResponseEnvelope<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    #[serde(borrow)]
    response: ResponseHead<'a>,
}

#[derive(Deserialize)]
struct ResponseHead<'a> {
    #[serde(borrow)]
    id: &'a str,
    #[serde(borrow)]
    status: &'a str,
    #[serde(borrow)]
    model: &'a str,
    #[serde(borrow)]
    service_tier: &'a str,
}

#[derive(Deserialize)]
struct OutputItemEnvelope<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    output_index: usize,
    #[serde(borrow)]
    item: OutputItemHead<'a>,
}

#[derive(Deserialize)]
struct OutputItemHead<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    #[serde(borrow)]
    id: Option<&'a str>,
    #[serde(borrow)]
    call_id: Option<&'a str>,
    #[serde(borrow)]
    name: Option<&'a str>,
    #[serde(borrow)]
    arguments: Option<Cow<'a, str>>,
    #[serde(borrow)]
    status: Option<&'a str>,
    #[serde(borrow)]
    role: Option<&'a str>,
    #[serde(default)]
    summary: Option<Vec<Value>>,
    #[serde(borrow)]
    encrypted_content: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
struct ContentPartEnvelope<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    #[serde(borrow)]
    item_id: &'a str,
    output_index: usize,
    content_index: usize,
    #[serde(borrow)]
    part: ContentPartHead<'a>,
}

#[derive(Deserialize)]
struct ContentPartHead<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
}

#[derive(Deserialize)]
struct DeltaEnvelope<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    #[serde(borrow)]
    item_id: &'a str,
    output_index: usize,
    content_index: usize,
    #[serde(borrow)]
    delta: Cow<'a, str>,
}

#[derive(Deserialize)]
struct DoneEnvelope<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    #[serde(borrow)]
    item_id: &'a str,
    output_index: usize,
    content_index: usize,
    #[serde(borrow, alias = "refusal")]
    text: Cow<'a, str>,
}

#[derive(Deserialize)]
struct ToolArgumentsDeltaEnvelope<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    #[serde(borrow)]
    item_id: &'a str,
    output_index: Option<usize>,
    #[serde(borrow)]
    delta: Cow<'a, str>,
}

#[derive(Deserialize)]
struct ToolArgumentsDoneEnvelope<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    #[serde(borrow)]
    item_id: &'a str,
    #[serde(borrow)]
    name: Option<&'a str>,
    output_index: Option<usize>,
    #[serde(borrow)]
    arguments: Cow<'a, str>,
}

#[derive(Deserialize)]
struct TerminalEnvelope<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    #[serde(borrow)]
    response: TerminalResponse<'a>,
}

#[derive(Deserialize)]
struct TerminalResponse<'a> {
    #[serde(borrow)]
    id: &'a str,
    #[serde(borrow)]
    status: &'a str,
    #[serde(borrow)]
    model: &'a str,
    #[serde(borrow)]
    service_tier: &'a str,
    usage: Option<OpenAiUsage>,
    #[serde(borrow)]
    incomplete_details: Option<IncompleteDetails<'a>>,
    #[serde(
        borrow,
        default,
        deserialize_with = "deserialize_bounded_terminal_output"
    )]
    output: Vec<TerminalOutputItem<'a>>,
}

#[derive(Deserialize)]
struct IncompleteDetails<'a> {
    #[serde(borrow)]
    reason: &'a str,
}

#[derive(Deserialize)]
struct TerminalOutputItem<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    #[serde(borrow, default)]
    content: Vec<TerminalContentPart<'a>>,
    #[serde(borrow)]
    id: Option<&'a str>,
    #[serde(borrow)]
    call_id: Option<&'a str>,
    #[serde(borrow)]
    name: Option<&'a str>,
    #[serde(borrow)]
    arguments: Option<Cow<'a, str>>,
    #[serde(borrow)]
    status: Option<&'a str>,
    #[serde(borrow)]
    role: Option<&'a str>,
    #[serde(default)]
    summary: Option<Vec<Value>>,
    #[serde(borrow)]
    encrypted_content: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
struct TerminalContentPart<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
}

fn deserialize_bounded_terminal_output<'de, D>(
    deserializer: D,
) -> Result<Vec<TerminalOutputItem<'de>>, D::Error>
where
    D: Deserializer<'de>,
{
    struct TerminalOutputVisitor;

    impl<'de> Visitor<'de> for TerminalOutputVisitor {
        type Value = Vec<TerminalOutputItem<'de>>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a bounded OpenAI response output array")
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let capacity = sequence
                .size_hint()
                .unwrap_or(0)
                .min(MAX_OPENAI_TERMINAL_OUTPUT_ITEMS);
            let mut output = Vec::with_capacity(capacity);
            while output.len() < MAX_OPENAI_TERMINAL_OUTPUT_ITEMS {
                let Some(item) = sequence.next_element()? else {
                    return Ok(output);
                };
                output.push(item);
            }
            if sequence.next_element::<IgnoredAny>()?.is_some() {
                return Err(A::Error::custom(
                    "OpenAI response output item limit exceeded",
                ));
            }
            Ok(output)
        }
    }

    deserializer.deserialize_seq(TerminalOutputVisitor)
}

#[derive(Clone, Copy, Deserialize)]
struct OpenAiUsage {
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
    #[serde(default)]
    input_tokens_details: OpenAiInputDetails,
    #[serde(default)]
    output_tokens_details: OpenAiOutputDetails,
}

impl OpenAiUsage {
    fn normalize(self) -> Result<AgentProviderUsage, AgentProviderProtocolError> {
        if self.input_tokens.checked_add(self.output_tokens) != Some(self.total_tokens) {
            return Err(AgentProviderProtocolError::Usage);
        }
        AgentProviderUsage::try_new(
            self.input_tokens,
            self.output_tokens,
            self.input_tokens_details.cached_tokens,
            self.input_tokens_details.cache_write_tokens,
            self.output_tokens_details.reasoning_tokens,
        )
        .map_err(|_| AgentProviderProtocolError::Usage)
    }
}

#[derive(Clone, Copy, Default, Deserialize)]
struct OpenAiInputDetails {
    #[serde(default)]
    cached_tokens: u64,
    #[serde(default)]
    cache_write_tokens: u64,
}

#[derive(Clone, Copy, Default, Deserialize)]
struct OpenAiOutputDetails {
    #[serde(default)]
    reasoning_tokens: u64,
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
    #[serde(borrow, default)]
    code: &'a str,
    #[serde(borrow, default, rename = "type")]
    error_type: &'a str,
}

fn parse_event_type(data: &str) -> Result<&str, AgentProviderProtocolError> {
    let event: EventType<'_> = parse(data)?;
    Ok(event.kind)
}

fn openai_protocol_event(event: &SseEvent) -> AgentProviderProtocolEvent {
    if event.data() == "[DONE]" {
        return AgentProviderProtocolEvent::OpenAiDone;
    }
    match event.event() {
        "response.created" => AgentProviderProtocolEvent::OpenAiCreated,
        "response.in_progress" => AgentProviderProtocolEvent::OpenAiInProgress,
        "response.output_item.added" => AgentProviderProtocolEvent::OpenAiOutputItemAdded,
        "response.output_item.done" => AgentProviderProtocolEvent::OpenAiOutputItemDone,
        "response.content_part.added" | "response.content_part.done" => {
            AgentProviderProtocolEvent::OpenAiContentPart
        }
        "response.output_text.delta"
        | "response.output_text.done"
        | "response.refusal.delta"
        | "response.refusal.done" => AgentProviderProtocolEvent::OpenAiText,
        "response.function_call_arguments.delta" => {
            AgentProviderProtocolEvent::OpenAiToolArgumentsDelta
        }
        "response.function_call_arguments.done" => {
            AgentProviderProtocolEvent::OpenAiToolArgumentsDone
        }
        "response.completed" | "response.incomplete" | "response.failed" | "response.cancelled" => {
            AgentProviderProtocolEvent::OpenAiTerminal
        }
        "error" => AgentProviderProtocolEvent::OpenAiError,
        _ => AgentProviderProtocolEvent::OpenAiUnknown,
    }
}

fn parse<'a, T>(data: &'a str) -> Result<T, AgentProviderProtocolError>
where
    T: Deserialize<'a>,
{
    serde_json::from_str(data).map_err(|_| AgentProviderProtocolError::Event)
}

fn validate_response_id(value: &str) -> Result<(), AgentProviderProtocolError> {
    if value.is_empty()
        || value.len() > MAX_OPENAI_RESPONSE_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        Err(AgentProviderProtocolError::Event)
    } else {
        Ok(())
    }
}

fn validate_encrypted_reasoning(value: &str) -> Result<(), AgentProviderProtocolError> {
    if value.is_empty()
        || value.len() > MAX_OPENAI_ENCRYPTED_REASONING_ITEM_BYTES
        || !value.bytes().all(|byte| byte.is_ascii_graphic())
    {
        Err(if value.len() > MAX_OPENAI_ENCRYPTED_REASONING_ITEM_BYTES {
            AgentProviderProtocolError::Limit
        } else {
            AgentProviderProtocolError::UnsupportedOutput
        })
    } else {
        Ok(())
    }
}

fn validate_terminal_output(
    output: &[TerminalOutputItem<'_>],
    streamed_output: &[OutputItemAccumulator],
    content_part: Option<&ContentPartAccumulator>,
    tools: &[ToolAccumulator],
) -> Result<ValidatedTerminalOutput, AgentProviderProtocolError> {
    if output.len() > MAX_OPENAI_TERMINAL_OUTPUT_ITEMS {
        return Err(AgentProviderProtocolError::Limit);
    }
    if streamed_output.len() != output.len() {
        return Err(AgentProviderProtocolError::Sequence);
    }
    if tools.len() > 1 {
        return Err(AgentProviderProtocolError::UnsupportedOutput);
    }
    let mut output_kind = TerminalOutputKind::None;
    let mut tool_index = 0_usize;
    let mut replay_items = Vec::new();
    replay_items
        .try_reserve_exact(output.len())
        .map_err(|_| AgentProviderProtocolError::Limit)?;
    for (output_index, item) in output.iter().enumerate() {
        match item.kind {
            "reasoning" => {
                let item_id = item.id.ok_or(AgentProviderProtocolError::Event)?;
                let encrypted_content = item
                    .encrypted_content
                    .as_deref()
                    .ok_or(AgentProviderProtocolError::UnsupportedOutput)?;
                validate_response_id(item_id)?;
                validate_encrypted_reasoning(encrypted_content)?;
                if !item.content.is_empty()
                    || !matches!(item.status, None | Some("completed"))
                    || item.role.is_some()
                    || item.call_id.is_some()
                    || item.name.is_some()
                    || item.arguments.is_some()
                    || item
                        .summary
                        .as_ref()
                        .is_none_or(|summary| !summary.is_empty())
                {
                    return Err(AgentProviderProtocolError::UnsupportedOutput);
                }
                let Some(OutputItemAccumulator::Reasoning {
                    item_id: streamed_id,
                    encrypted_content: Some(_),
                    done: true,
                }) = streamed_output.get(output_index)
                else {
                    return Err(AgentProviderProtocolError::Sequence);
                };
                if streamed_id != item_id {
                    return Err(AgentProviderProtocolError::Sequence);
                }
                replay_items.push(OpenAiResponseReplayItem::Reasoning {
                    id: item_id.to_owned(),
                    encrypted_content: encrypted_content.to_owned(),
                });
            }
            "message" => {
                let item_id = item.id.ok_or(AgentProviderProtocolError::Event)?;
                validate_response_id(item_id)?;
                if item.content.len() != 1 {
                    return Err(if item.content.len() > MAX_OPENAI_TERMINAL_CONTENT_PARTS {
                        AgentProviderProtocolError::Limit
                    } else {
                        AgentProviderProtocolError::UnsupportedOutput
                    });
                }
                if !tools.is_empty()
                    || item.status != Some("completed")
                    || item.role != Some("assistant")
                    || item.call_id.is_some()
                    || item.name.is_some()
                    || item.arguments.is_some()
                    || item.summary.is_some()
                    || item.encrypted_content.is_some()
                    || item
                        .content
                        .iter()
                        .any(|part| !matches!(part.kind, "output_text" | "refusal"))
                {
                    return Err(AgentProviderProtocolError::UnsupportedOutput);
                }
                for part in &item.content {
                    let part_kind = if part.kind == "refusal" {
                        TerminalOutputKind::Refusal
                    } else {
                        TerminalOutputKind::Text
                    };
                    if output_kind != TerminalOutputKind::None && output_kind != part_kind {
                        return Err(AgentProviderProtocolError::UnsupportedOutput);
                    }
                    output_kind = part_kind;
                }
                if !matches!(
                    streamed_output.get(output_index),
                    Some(OutputItemAccumulator::Message {
                        item_id: streamed_id,
                        done: true,
                    }) if streamed_id == item_id
                ) {
                    return Err(AgentProviderProtocolError::Sequence);
                }
                let expected_kind = if item.content[0].kind == "refusal" {
                    ContentPartKind::Refusal
                } else {
                    ContentPartKind::OutputText
                };
                if content_part.is_none_or(|part| {
                    part.item_id != item_id
                        || part.output_index != output_index
                        || part.content_index != 0
                        || part.kind != expected_kind
                        || !part.output_done
                        || !part.part_done
                }) {
                    return Err(AgentProviderProtocolError::Sequence);
                }
            }
            "function_call" => {
                let tool = tools
                    .get(tool_index)
                    .ok_or(AgentProviderProtocolError::Sequence)?;
                let item_id = item.id.ok_or(AgentProviderProtocolError::Event)?;
                let call_id = item.call_id.ok_or(AgentProviderProtocolError::Event)?;
                let name = item.name.ok_or(AgentProviderProtocolError::Event)?;
                let arguments = item
                    .arguments
                    .as_deref()
                    .ok_or(AgentProviderProtocolError::Event)?;
                let argument_guard = tool
                    .argument_guard
                    .ok_or(AgentProviderProtocolError::Sequence)?;
                let actual_guard: [u8; 32] = Sha256::digest(arguments.as_bytes()).into();
                if !item.content.is_empty()
                    || item.status != Some("completed")
                    || item.role.is_some()
                    || item.summary.is_some()
                    || item.encrypted_content.is_some()
                    || tool.item_id != item_id
                    || tool.call_id != call_id
                    || tool.name.as_str() != name
                    || usize::try_from(tool.argument_bytes).ok() != Some(arguments.len())
                    || argument_guard != actual_guard
                {
                    return Err(AgentProviderProtocolError::Sequence);
                }
                if !matches!(
                    streamed_output.get(output_index),
                    Some(OutputItemAccumulator::FunctionCall {
                        tool_index: streamed_tool_index,
                    }) if *streamed_tool_index == tool_index
                ) {
                    return Err(AgentProviderProtocolError::Sequence);
                }
                replay_items.push(OpenAiResponseReplayItem::FunctionCall);
                tool_index += 1;
            }
            _ => return Err(AgentProviderProtocolError::UnsupportedOutput),
        }
    }
    if tool_index != tools.len() {
        return Err(AgentProviderProtocolError::Sequence);
    }
    if output_kind == TerminalOutputKind::Refusal && !tools.is_empty() {
        return Err(AgentProviderProtocolError::UnsupportedOutput);
    }
    let replay = if tools.is_empty() {
        None
    } else {
        Some(
            OpenAiResponseReplay::try_new(replay_items)
                .ok_or(AgentProviderProtocolError::UnsupportedOutput)?,
        )
    };
    Ok(ValidatedTerminalOutput {
        kind: output_kind,
        replay,
    })
}

struct ValidatedTerminalOutput {
    kind: TerminalOutputKind,
    replay: Option<OpenAiResponseReplay>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum TerminalOutputKind {
    None,
    Text,
    Refusal,
}

impl TerminalOutputKind {
    fn matches_state(self, state: &OutputState) -> bool {
        matches!(
            (self, state),
            (Self::None, OutputState::None)
                | (Self::Text, OutputState::Text { done: true, .. })
                | (Self::Refusal, OutputState::Refusal { done: true, .. })
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AgentModelCallId, AgentPlanLeaseId, AgentPlanNodeId, AgentProviderModelRevision,
        AgentProviderPricingProfile, AgentProviderPricingRevision, AgentProviderPricingSchedule,
        AgentProviderReasoningEffort, AgentProviderResponseRoute, AgentProviderTokenRates,
        AgentRunManifestId, SemanticTokenizerRevision,
    };
    use serde_json::json;

    fn call() -> AgentProviderCallIdentity {
        AgentProviderCallIdentity {
            manifest: AgentRunManifestId::from_raw(1),
            manifest_guard: [0; 32],
            call: AgentModelCallId::new(2).expect("call"),
            lease: AgentPlanLeaseId::from_raw(3),
            node: AgentPlanNodeId::from_raw(4),
        }
    }

    fn config(max_text: u32) -> AgentProviderCallConfig {
        config_with_effective_models(max_text, &["gpt-5.6-terra"])
    }

    fn config_with_effective_models(
        max_text: u32,
        effective_models: &[&str],
    ) -> AgentProviderCallConfig {
        let requested =
            AgentProviderModelRevision::try_new("gpt-5.6-terra".to_owned()).expect("model");
        let allowed = effective_models
            .iter()
            .map(|model| {
                AgentProviderModelRevision::try_new((*model).to_owned()).expect("effective model")
            })
            .collect();
        AgentProviderPricingSchedule::try_new(
            AgentProviderKind::OpenAiResponses,
            requested,
            allowed,
            AgentProviderResponseRoute::OpenAiDefault,
            AgentProviderReasoningEffort::Medium,
            SemanticTokenizerRevision::try_new("openai:gpt-5.6-terra:v1".to_owned())
                .expect("tokenizer"),
            AgentProviderPricingProfile::try_new(
                AgentProviderPricingRevision::new(1).expect("pricing revision"),
                16_384,
            )
            .expect("pricing profile"),
            AgentProviderTokenRates::try_new(1, 1, 1, 1).expect("rates"),
        )
        .expect("schedule")
        .try_call_config(
            512,
            1_024,
            AgentProviderStreamBudget::try_new(64 * 1024, 64, max_text, 1, 1_024).expect("budget"),
        )
        .expect("config")
    }

    fn sse(event: &str, data: &str) -> String {
        format!("event: {event}\ndata: {data}\n\n")
    }

    fn created(id: &str) -> String {
        created_with_identity(id, "gpt-5.6-terra", "default")
    }

    fn created_with_identity(id: &str, model: &str, service_tier: &str) -> String {
        sse(
            "response.created",
            &format!(
                "{{\"type\":\"response.created\",\"response\":{{\"id\":\"{id}\",\"status\":\"in_progress\",\"model\":\"{model}\",\"service_tier\":\"{service_tier}\"}}}}"
            ),
        )
    }

    fn in_progress(id: &str) -> String {
        sse(
            "response.in_progress",
            &format!(
                "{{\"type\":\"response.in_progress\",\"response\":{{\"id\":\"{id}\",\"status\":\"in_progress\",\"model\":\"gpt-5.6-terra\",\"service_tier\":\"default\"}}}}"
            ),
        )
    }

    fn terminal(id: &str, kind: &str, status: &str, output: &str) -> String {
        terminal_with_identity(id, kind, status, output, "gpt-5.6-terra", "default")
    }

    fn terminal_with_identity(
        id: &str,
        kind: &str,
        status: &str,
        output: &str,
        model: &str,
        service_tier: &str,
    ) -> String {
        sse(
            kind,
            &format!(
                "{{\"type\":\"{kind}\",\"response\":{{\"id\":\"{id}\",\"status\":\"{status}\",\"model\":\"{model}\",\"service_tier\":\"{service_tier}\",\"output\":{output},\"usage\":{{\"input_tokens\":17,\"output_tokens\":3,\"total_tokens\":20,\"input_tokens_details\":{{\"cached_tokens\":4}},\"output_tokens_details\":{{\"reasoning_tokens\":1}}}}}}}}"
            ),
        )
    }

    fn output_item_event(kind: &str, output_index: usize, item: Value) -> String {
        sse(
            kind,
            &serde_json::json!({
                "type": kind,
                "output_index": output_index,
                "item": item
            })
            .to_string(),
        )
    }

    fn message_item_event(kind: &str, output_index: usize, item_id: &str) -> String {
        let status = if kind == "response.output_item.added" {
            "in_progress"
        } else {
            "completed"
        };
        output_item_event(
            kind,
            output_index,
            json!({
                "type": "message",
                "id": item_id,
                "status": status,
                "role": "assistant"
            }),
        )
    }

    fn content_part_event(kind: &str, item_id: &str, part_kind: &str) -> String {
        sse(
            kind,
            &json!({
                "type": kind,
                "item_id": item_id,
                "output_index": 0,
                "content_index": 0,
                "part": {"type": part_kind}
            })
            .to_string(),
        )
    }

    fn text_event(kind: &str, item_id: &str, field: &str, value: &str) -> String {
        let mut event = json!({
            "type": kind,
            "item_id": item_id,
            "output_index": 0,
            "content_index": 0
        });
        event[field] = Value::String(value.to_owned());
        sse(kind, &event.to_string())
    }

    #[test]
    fn fragmented_text_stream_normalizes_deltas_and_terminal_usage() {
        let mut decoder =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        let stream = [
            created("resp_1"),
            message_item_event("response.output_item.added", 0, "msg_1"),
            content_part_event("response.content_part.added", "msg_1", "output_text"),
            text_event("response.output_text.delta", "msg_1", "delta", "hello "),
            text_event("response.output_text.delta", "msg_1", "delta", "world"),
            text_event(
                "response.output_text.done",
                "msg_1",
                "text",
                "hello world",
            ),
            content_part_event("response.content_part.done", "msg_1", "output_text"),
            message_item_event("response.output_item.done", 0, "msg_1"),
            terminal(
                "resp_1",
                "response.completed",
                "completed",
                r#"[{"type":"message","id":"msg_1","status":"completed","role":"assistant","content":[{"type":"output_text"}]}]"#,
            ),
            "data: [DONE]\n\n".to_owned(),
        ]
        .concat();
        let split = stream.len() / 3;
        let first = decoder.push(&stream.as_bytes()[..split]).expect("first");
        let second = decoder
            .push(&stream.as_bytes()[split..(split * 2)])
            .expect("second");
        let third = decoder
            .push(&stream.as_bytes()[(split * 2)..])
            .expect("third");
        let text = first
            .into_deltas()
            .into_iter()
            .chain(second.into_deltas())
            .chain(third.into_deltas())
            .map(|delta| delta.as_str().to_owned())
            .collect::<String>();
        assert_eq!(text, "hello world");
        let conclusion = decoder.finish().expect("terminal").conclusion();
        let AgentProviderStreamConclusion::Completed(completion) = conclusion else {
            panic!("expected completion");
        };
        assert_eq!(completion.stop(), AgentProviderStopReason::Completed);
        assert!(!completion.tool_only_output());
        assert_eq!(completion.usage().total_tokens(), 20);
        assert_eq!(completion.usage().cached_input_tokens(), 4);
        assert_eq!(completion.stats().output_text_bytes(), 11);
        assert_eq!(completion.stats().events(), 10);
        assert_eq!(
            completion.stats().wire_bytes(),
            u32::try_from(stream.len()).expect("stream bytes")
        );

        let mut bytewise =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
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
    }

    #[test]
    fn terminal_text_mismatch_and_output_limit_fail_closed() {
        let mut mismatch =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(16)).expect("decoder");
        mismatch
            .push(created("resp_2").as_bytes())
            .expect("created");
        mismatch
            .push(message_item_event("response.output_item.added", 0, "msg_2").as_bytes())
            .expect("message added");
        mismatch
            .push(
                content_part_event("response.content_part.added", "msg_2", "output_text")
                    .as_bytes(),
            )
            .expect("content added");
        mismatch
            .push(text_event("response.output_text.delta", "msg_2", "delta", "abc").as_bytes())
            .expect("delta");
        assert_eq!(
            mismatch
                .push(text_event("response.output_text.done", "msg_2", "text", "xyz").as_bytes(),),
            Err(AgentProviderProtocolError::Sequence)
        );
        assert_eq!(
            mismatch.push(b""),
            Err(AgentProviderProtocolError::Sequence)
        );

        let mut limited =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(3)).expect("decoder");
        limited.push(created("resp_3").as_bytes()).expect("created");
        limited
            .push(message_item_event("response.output_item.added", 0, "msg_3").as_bytes())
            .expect("message added");
        limited
            .push(
                content_part_event("response.content_part.added", "msg_3", "output_text")
                    .as_bytes(),
            )
            .expect("content added");
        assert_eq!(
            limited.push(
                text_event("response.output_text.delta", "msg_3", "delta", "four").as_bytes(),
            ),
            Err(AgentProviderProtocolError::Limit)
        );
    }

    #[test]
    fn hidden_reasoning_output_never_crosses_the_boundary() {
        let mut decoder =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        decoder.push(created("resp_4").as_bytes()).expect("created");
        assert_eq!(
            decoder.push(
                sse(
                    "response.reasoning_text.delta",
                    r#"{"type":"response.reasoning_text.delta","delta":"hidden"}"#,
                )
                .as_bytes(),
            ),
            Err(AgentProviderProtocolError::UnsupportedOutput)
        );
    }

    #[test]
    fn reasoning_status_is_optional_but_cannot_contradict_the_lifecycle() {
        let mut decoder =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        decoder
            .push(created("resp_reasoning_status").as_bytes())
            .expect("created");
        let contradictory = sse(
            "response.output_item.added",
            &serde_json::json!({
                "type": "response.output_item.added",
                "output_index": 0,
                "item": {
                    "type": "reasoning",
                    "id": "rs_contradictory",
                    "summary": [],
                    "status": "completed"
                }
            })
            .to_string(),
        );
        assert_eq!(
            decoder.push(contradictory.as_bytes()),
            Err(AgentProviderProtocolError::Sequence)
        );
    }

    #[test]
    fn encrypted_reasoning_is_terminal_authenticated_and_identity_is_distinct() {
        let effective_model = "gpt-5.6-terra-2026-08-01";
        let config = config_with_effective_models(64, &["gpt-5.6-terra", effective_model]);
        let mut decoder = OpenAiResponsesStreamDecoder::try_new(call(), &config).expect("decoder");
        decoder
            .push(created_with_identity("resp_replay", effective_model, "default").as_bytes())
            .expect("created");
        let streamed_encrypted = "opaque_streamed_reasoning_AQID";
        let terminal_encrypted = "opaque_terminal_reasoning_BAUG";
        let provisional_encrypted = "opaque_provisional_reasoning_AQID";
        let reasoning_added = sse(
            "response.output_item.added",
            &serde_json::json!({
                "type": "response.output_item.added",
                "output_index": 0,
                "item": {
                    "type": "reasoning",
                    "id": "rs_private_1",
                    "summary": [],
                    "encrypted_content": provisional_encrypted
                }
            })
            .to_string(),
        );
        assert!(decoder
            .push(reasoning_added.as_bytes())
            .expect("reasoning added")
            .deltas()
            .is_empty());
        let reasoning_done = sse(
            "response.output_item.done",
            &serde_json::json!({
                "type": "response.output_item.done",
                "output_index": 0,
                "item": {
                    "type": "reasoning",
                    "id": "rs_private_1",
                    "summary": [],
                    "encrypted_content": streamed_encrypted
                }
            })
            .to_string(),
        );
        assert!(decoder
            .push(reasoning_done.as_bytes())
            .expect("reasoning done")
            .deltas()
            .is_empty());

        let arguments = r#"{"url":"https://example.test/replay"}"#;
        let tool_added = sse(
            "response.output_item.added",
            &serde_json::json!({
                "type": "response.output_item.added",
                "output_index": 1,
                "item": {
                    "type": "function_call",
                    "id": "fc_replay_1",
                    "call_id": "call_replay_1",
                    "name": "navigate",
                    "arguments": "",
                    "status": "in_progress"
                }
            })
            .to_string(),
        );
        decoder
            .push(tool_added.as_bytes())
            .expect("tool added before terminal");
        let arguments_done = sse(
            "response.function_call_arguments.done",
            &serde_json::json!({
                "type": "response.function_call_arguments.done",
                "item_id": "fc_replay_1",
                "name": "navigate",
                "arguments": arguments
            })
            .to_string(),
        );
        assert!(decoder
            .push(arguments_done.as_bytes())
            .expect("arguments done")
            .deltas()
            .is_empty());
        let tool_done = sse(
            "response.output_item.done",
            &serde_json::json!({
                "type": "response.output_item.done",
                "output_index": 1,
                "item": {
                    "type": "function_call",
                    "id": "fc_replay_1",
                    "call_id": "call_replay_1",
                    "name": "navigate",
                    "arguments": arguments,
                    "status": "completed"
                }
            })
            .to_string(),
        );
        assert!(decoder
            .push(tool_done.as_bytes())
            .expect("tool done")
            .deltas()
            .is_empty());

        let terminal_output = serde_json::json!([
            {
                "type": "reasoning",
                "id": "rs_private_1",
                "summary": [],
                "encrypted_content": terminal_encrypted
            },
            {
                "type": "function_call",
                "id": "fc_replay_1",
                "call_id": "call_replay_1",
                "name": "navigate",
                "arguments": arguments,
                "status": "completed"
            }
        ])
        .to_string();
        let terminal_batch = decoder
            .push(
                terminal_with_identity(
                    "resp_replay",
                    "response.completed",
                    "completed",
                    &terminal_output,
                    effective_model,
                    "default",
                )
                .as_bytes(),
            )
            .expect("authenticated terminal");
        assert!(terminal_batch.deltas().is_empty());
        let finished = decoder.finish().expect("completion");
        let (conclusion, tool) = finished.into_parts();
        let tool = tool.expect("EOF-private tool");
        let (correlation, _) = tool.into_continuation_parts();
        assert!(!format!("{correlation:?}").contains(streamed_encrypted));
        assert!(!format!("{correlation:?}").contains(terminal_encrypted));
        assert!(!format!("{correlation:?}").contains(provisional_encrypted));

        let AgentProviderStreamConclusion::Completed(completion) = conclusion else {
            panic!("completed response");
        };
        assert!(completion.tool_only_output());
        let identity = completion.response_identity().expect("response identity");
        assert_eq!(identity.requested_model(), "gpt-5.6-terra");
        assert_eq!(identity.effective_model(), effective_model);
        assert_eq!(identity.effective_service_tier(), "default");
        assert_eq!(
            identity.reasoning_effort(),
            AgentProviderReasoningEffort::Medium
        );
        let debug = format!("{identity:?}");
        assert!(!debug.contains("gpt-5.6-terra"));
        assert!(!debug.contains(effective_model));
    }

    #[test]
    fn encrypted_reasoning_is_bounded_unique_and_opaque() {
        let reasoning_added = |index, id: &str| {
            output_item_event(
                "response.output_item.added",
                index,
                serde_json::json!({
                    "type": "reasoning",
                    "id": id,
                    "summary": [],
                    "status": "in_progress"
                }),
            )
        };
        let reasoning_done = |index, id: &str, encrypted: Option<&str>| {
            let mut item = serde_json::json!({
                "type": "reasoning",
                "id": id,
                "summary": [],
                "status": "completed"
            });
            if let Some(encrypted) = encrypted {
                item["encrypted_content"] = Value::String(encrypted.to_owned());
            }
            output_item_event("response.output_item.done", index, item)
        };

        for (encrypted, expected) in [
            (None, AgentProviderProtocolError::UnsupportedOutput),
            (
                Some("opaque with whitespace"),
                AgentProviderProtocolError::UnsupportedOutput,
            ),
        ] {
            let mut decoder =
                OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
            decoder
                .push(created("resp_invalid_reasoning").as_bytes())
                .expect("created");
            decoder
                .push(reasoning_added(0, "rs_invalid").as_bytes())
                .expect("reasoning added");
            assert_eq!(
                decoder.push(reasoning_done(0, "rs_invalid", encrypted).as_bytes()),
                Err(expected)
            );
        }

        let mut summary =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        summary
            .push(created("resp_reasoning_summary").as_bytes())
            .expect("created");
        summary
            .push(reasoning_added(0, "rs_summary").as_bytes())
            .expect("reasoning added");
        let summary_done = output_item_event(
            "response.output_item.done",
            0,
            serde_json::json!({
                "type": "reasoning",
                "id": "rs_summary",
                "summary": [{"type": "summary_text", "text": "must remain hidden"}],
                "encrypted_content": "opaque_summary_ciphertext",
                "status": "completed"
            }),
        );
        assert_eq!(
            summary.push(summary_done.as_bytes()),
            Err(AgentProviderProtocolError::UnsupportedOutput)
        );
        assert!(!format!("{summary:?}").contains("must remain hidden"));

        let mut duplicate_field =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        duplicate_field
            .push(created("resp_duplicate_field").as_bytes())
            .expect("created");
        duplicate_field
            .push(reasoning_added(0, "rs_duplicate_field").as_bytes())
            .expect("reasoning added");
        let duplicate_field_done = sse(
            "response.output_item.done",
            r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"reasoning","id":"rs_duplicate_field","summary":[],"encrypted_content":"opaque_first","encrypted_content":"opaque_second","status":"completed"}}"#,
        );
        assert_eq!(
            duplicate_field.push(duplicate_field_done.as_bytes()),
            Err(AgentProviderProtocolError::Event)
        );

        let oversized = "x".repeat(MAX_OPENAI_ENCRYPTED_REASONING_ITEM_BYTES + 1);
        let mut decoder =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        decoder
            .push(created("resp_oversized_reasoning").as_bytes())
            .expect("created");
        decoder
            .push(reasoning_added(0, "rs_oversized").as_bytes())
            .expect("reasoning added");
        assert_eq!(
            decoder.push(reasoning_done(0, "rs_oversized", Some(&oversized)).as_bytes()),
            Err(AgentProviderProtocolError::Limit)
        );

        let mut duplicate_id =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        duplicate_id
            .push(created("resp_duplicate_id").as_bytes())
            .expect("created");
        duplicate_id
            .push(reasoning_added(0, "rs_duplicate").as_bytes())
            .expect("first reasoning");
        assert_eq!(
            duplicate_id.push(reasoning_added(1, "rs_duplicate").as_bytes()),
            Err(AgentProviderProtocolError::Sequence)
        );

        let mut duplicate_encrypted =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        duplicate_encrypted
            .push(created("resp_duplicate_encrypted").as_bytes())
            .expect("created");
        duplicate_encrypted
            .push(reasoning_added(0, "rs_first").as_bytes())
            .expect("first added");
        duplicate_encrypted
            .push(reasoning_done(0, "rs_first", Some("opaque_duplicate")).as_bytes())
            .expect("first done");
        duplicate_encrypted
            .push(reasoning_added(1, "rs_second").as_bytes())
            .expect("second added");
        assert_eq!(
            duplicate_encrypted
                .push(reasoning_done(1, "rs_second", Some("opaque_duplicate")).as_bytes()),
            Err(AgentProviderProtocolError::Sequence)
        );
        assert!(!format!("{duplicate_encrypted:?}").contains("opaque_duplicate"));
    }

    #[test]
    fn complete_function_call_becomes_only_a_typed_browser_proposal() {
        let mut decoder =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        let arguments = r#"{"url":"https://example.test/path"}"#;
        let output = serde_json::json!([{
            "type": "function_call",
            "id": "fc_1",
            "call_id": "call_tool1",
            "name": "navigate",
            "arguments": arguments,
            "status": "completed"
        }])
        .to_string();
        let stream = [
            created("resp_tool"),
            sse(
                "response.output_item.added",
                &serde_json::json!({
                    "type": "response.output_item.added",
                    "output_index": 0,
                    "item": {
                        "type": "function_call",
                        "id": "fc_1",
                        "call_id": "call_tool1",
                        "name": "navigate",
                        "arguments": "",
                        "status": "in_progress"
                    }
                })
                .to_string(),
            ),
            sse(
                "response.function_call_arguments.delta",
                &serde_json::json!({
                    "type": "response.function_call_arguments.delta",
                    "item_id": "fc_1",
                    "output_index": 0,
                    "delta": "{\"url\":"
                })
                .to_string(),
            ),
            sse(
                "response.function_call_arguments.delta",
                &serde_json::json!({
                    "type": "response.function_call_arguments.delta",
                    "item_id": "fc_1",
                    "output_index": 0,
                    "delta": "\"https://example.test/path\"}"
                })
                .to_string(),
            ),
            sse(
                "response.function_call_arguments.done",
                &serde_json::json!({
                    "type": "response.function_call_arguments.done",
                    "item_id": "fc_1",
                    "output_index": 0,
                    "arguments": arguments
                })
                .to_string(),
            ),
            sse(
                "response.output_item.done",
                &serde_json::json!({
                    "type": "response.output_item.done",
                    "output_index": 0,
                    "item": {
                        "type": "function_call",
                        "id": "fc_1",
                        "call_id": "call_tool1",
                        "name": "navigate",
                        "arguments": arguments,
                        "status": "completed"
                    }
                })
                .to_string(),
            ),
            terminal("resp_tool", "response.completed", "completed", &output),
            "data: [DONE]\n\n".to_owned(),
        ]
        .concat();
        let batch = decoder.push(stream.as_bytes()).expect("tool stream");
        assert!(batch.deltas().is_empty());
        let finished = decoder.finish().expect("terminal");
        let (conclusion, tool) = finished.into_parts();
        let tool = tool.expect("EOF-private tool");
        assert_eq!(tool.id().as_str(), "call_tool1");
        let (_, proposal) = tool.into_parts();
        let crate::AgentBrowserToolProposal::Navigate(target) = proposal else {
            panic!("navigate proposal");
        };
        assert_eq!(target.as_url().as_str(), "https://example.test/path");

        let AgentProviderStreamConclusion::Completed(completion) = conclusion else {
            panic!("completed tool response");
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
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        decoder
            .push(created("resp_bad_tool").as_bytes())
            .expect("created");
        decoder
            .push(
                sse(
                    "response.output_item.added",
                    &serde_json::json!({
                        "type": "response.output_item.added",
                        "output_index": 0,
                        "item": {
                            "type": "function_call",
                            "id": "fc_bad",
                            "call_id": "call_bad",
                            "name": "navigate",
                            "arguments": "",
                            "status": "in_progress"
                        }
                    })
                    .to_string(),
                )
                .as_bytes(),
            )
            .expect("tool start");
        let arguments = r##"{"url":"https://example.test","selector":"#secret"}"##;
        decoder
            .push(
                sse(
                    "response.function_call_arguments.delta",
                    &serde_json::json!({
                        "type": "response.function_call_arguments.delta",
                        "item_id": "fc_bad",
                        "delta": arguments
                    })
                    .to_string(),
                )
                .as_bytes(),
            )
            .expect("arguments");
        let done = sse(
            "response.function_call_arguments.done",
            &serde_json::json!({
                "type": "response.function_call_arguments.done",
                "item_id": "fc_bad",
                "name": "navigate",
                "arguments": arguments
            })
            .to_string(),
        );
        assert_eq!(
            decoder.push(done.as_bytes()),
            Err(AgentProviderProtocolError::ToolCall)
        );
        assert_eq!(decoder.push(b""), Err(AgentProviderProtocolError::ToolCall));
        assert!(!format!("{decoder:?}").contains("#secret"));
    }

    #[test]
    fn second_tool_is_rejected_before_any_proposal_can_escape() {
        let mut decoder =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        decoder
            .push(created("resp_second_tool").as_bytes())
            .expect("created");
        let tool_added = |output_index: usize, item_id: &str, call_id: &str| {
            output_item_event(
                "response.output_item.added",
                output_index,
                json!({
                    "type": "function_call",
                    "id": item_id,
                    "call_id": call_id,
                    "name": "back",
                    "arguments": "",
                    "status": "in_progress"
                }),
            )
        };
        assert!(decoder
            .push(tool_added(0, "fc_first", "call_first").as_bytes())
            .expect("first tool")
            .deltas()
            .is_empty());
        assert_eq!(
            decoder.push(tool_added(1, "fc_second", "call_second").as_bytes()),
            Err(AgentProviderProtocolError::Limit)
        );
        assert_eq!(decoder.push(b""), Err(AgentProviderProtocolError::Limit));
    }

    #[test]
    fn incomplete_and_failed_terminals_are_typed_without_provider_text() {
        let mut incomplete =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        incomplete
            .push(created("resp_5").as_bytes())
            .expect("created");
        let event = sse(
            "response.incomplete",
            r#"{"type":"response.incomplete","response":{"id":"resp_5","status":"incomplete","model":"gpt-5.6-terra","service_tier":"default","output":[],"incomplete_details":{"reason":"max_output_tokens"},"usage":{"input_tokens":5,"output_tokens":7,"total_tokens":12}}}"#,
        );
        incomplete.push(event.as_bytes()).expect("incomplete");
        let AgentProviderStreamConclusion::Completed(completion) =
            incomplete.finish().expect("terminal").conclusion()
        else {
            panic!("expected typed incomplete response");
        };
        assert_eq!(completion.stop(), AgentProviderStopReason::OutputLimit);

        let mut failed =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        failed.push(created("resp_6").as_bytes()).expect("created");
        let event = sse(
            "response.failed",
            r#"{"type":"response.failed","response":{"id":"resp_6","status":"failed","model":"gpt-5.6-terra","service_tier":"default","output":[],"usage":null,"error":{"code":"server_error","message":"must not escape"}}}"#,
        );
        failed.push(event.as_bytes()).expect("failure");
        let conclusion = failed.finish().expect("terminal").conclusion();
        assert_eq!(
            format!("{conclusion:?}"),
            format!(
                "Failed({:?})",
                match conclusion {
                    AgentProviderStreamConclusion::Failed(failure) => failure,
                    AgentProviderStreamConclusion::Completed(_) => panic!("expected failure"),
                }
            )
        );
        assert!(!format!("{conclusion:?}").contains("must not escape"));
        let AgentProviderStreamConclusion::Failed(failure) = conclusion else {
            panic!("expected failure");
        };
        assert_eq!(
            failure.failure().class(),
            AgentProviderFailureClass::Provider
        );
        assert_eq!(failure.usage(), None);
    }

    #[test]
    fn model_response_identity_usage_and_terminal_sequence_are_exact() {
        let mut billing_mismatch =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        assert_eq!(
            billing_mismatch.push(
                created("resp_tier")
                    .replace("\"default\"", "\"priority\"")
                    .as_bytes()
            ),
            Err(AgentProviderProtocolError::Event)
        );

        let oversized_model = "x".repeat(97);
        for invalid_model in ["", "model/unsafe", oversized_model.as_str()] {
            let mut invalid =
                OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
            assert_eq!(
                invalid.push(
                    created_with_identity("resp_invalid", invalid_model, "default").as_bytes()
                ),
                Err(AgentProviderProtocolError::Event)
            );
        }

        let mut model_drift =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        assert_eq!(
            model_drift.push(
                created_with_identity("resp_model_drift", "gpt-5.6-terra-2026-08-01", "default")
                    .as_bytes(),
            ),
            Err(AgentProviderProtocolError::Event)
        );

        let mut tier_drift =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        tier_drift
            .push(created("resp_tier_drift").as_bytes())
            .expect("created");
        assert_eq!(
            tier_drift.push(
                terminal_with_identity(
                    "resp_tier_drift",
                    "response.completed",
                    "completed",
                    "[]",
                    "gpt-5.6-terra",
                    "priority"
                )
                .as_bytes()
            ),
            Err(AgentProviderProtocolError::Event)
        );

        let mut decoder =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        assert_eq!(
            decoder.push(terminal("resp_7", "response.completed", "completed", "[]").as_bytes(),),
            Err(AgentProviderProtocolError::Sequence)
        );

        let mut decoder =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        decoder.push(created("resp_7").as_bytes()).expect("created");
        assert_eq!(
            decoder
                .push(terminal("resp_other", "response.completed", "completed", "[]").as_bytes(),),
            Err(AgentProviderProtocolError::Sequence)
        );

        let mut decoder =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        decoder.push(created("resp_8").as_bytes()).expect("created");
        let bad_usage = sse(
            "response.completed",
            r#"{"type":"response.completed","response":{"id":"resp_8","status":"completed","model":"gpt-5.6-terra","service_tier":"default","output":[],"usage":{"input_tokens":1,"output_tokens":2,"total_tokens":99}}}"#,
        );
        assert_eq!(
            decoder.push(bad_usage.as_bytes()),
            Err(AgentProviderProtocolError::Usage)
        );
    }

    #[test]
    fn terminal_output_rejects_mixed_and_reordered_items() {
        let arguments = "{}";
        let guard: [u8; 32] = Sha256::digest(arguments.as_bytes()).into();
        let tool_call = AgentBrowserToolCall::decode_openai_with_replay(
            call(),
            "fc_terminal_1".to_owned(),
            "call_terminal_1".to_owned(),
            "back",
            arguments.to_owned(),
            None,
        )
        .expect("tool");
        let tools = vec![ToolAccumulator {
            item_id: "fc_terminal_1".to_owned(),
            call_id: "call_terminal_1".to_owned(),
            name: AgentBrowserToolKind::Back,
            arguments: None,
            argument_bytes: 2,
            argument_guard: Some(guard),
            item_done: true,
            call: Some(tool_call),
        }];
        let function = || TerminalOutputItem {
            kind: "function_call",
            content: Vec::new(),
            id: Some("fc_terminal_1"),
            call_id: Some("call_terminal_1"),
            name: Some("back"),
            arguments: Some(Cow::Borrowed(arguments)),
            status: Some("completed"),
            role: None,
            summary: None,
            encrypted_content: None,
        };
        let mixed = vec![
            function(),
            TerminalOutputItem {
                kind: "message",
                content: vec![TerminalContentPart {
                    kind: "output_text",
                }],
                id: Some("msg_mixed_1"),
                call_id: None,
                name: None,
                arguments: None,
                status: Some("completed"),
                role: Some("assistant"),
                summary: None,
                encrypted_content: None,
            },
        ];
        let mixed_stream = vec![
            OutputItemAccumulator::FunctionCall { tool_index: 0 },
            OutputItemAccumulator::Message {
                item_id: "msg_mixed_1".to_owned(),
                done: true,
            },
        ];
        assert!(matches!(
            validate_terminal_output(&mixed, &mixed_stream, None, &tools),
            Err(AgentProviderProtocolError::UnsupportedOutput)
        ));

        let encrypted = "opaque_reordered_ciphertext";
        let reordered = vec![
            function(),
            TerminalOutputItem {
                kind: "reasoning",
                content: Vec::new(),
                id: Some("rs_reordered_1"),
                call_id: None,
                name: None,
                arguments: None,
                status: Some("completed"),
                role: None,
                summary: Some(Vec::new()),
                encrypted_content: Some(Cow::Borrowed(encrypted)),
            },
        ];
        let expected_stream = vec![
            OutputItemAccumulator::Reasoning {
                item_id: "rs_reordered_1".to_owned(),
                encrypted_content: Some(encrypted.to_owned()),
                done: true,
            },
            OutputItemAccumulator::FunctionCall { tool_index: 0 },
        ];
        assert!(matches!(
            validate_terminal_output(&reordered, &expected_stream, None, &tools),
            Err(AgentProviderProtocolError::Sequence)
        ));
    }

    #[test]
    fn live_output_item_cap_precedes_every_added_item_variant() {
        let added_items = [
            json!({
                "type": "message",
                "id": "msg_over_limit",
                "status": "in_progress",
                "role": "assistant"
            }),
            json!({
                "type": "reasoning",
                "id": "rs_over_limit",
                "status": "in_progress",
                "summary": []
            }),
            json!({
                "type": "function_call",
                "id": "fc_over_limit",
                "call_id": "call_over_limit",
                "name": "back",
                "arguments": "",
                "status": "in_progress"
            }),
        ];

        for item in added_items {
            let mut decoder =
                OpenAiResponsesStreamDecoder::try_new(call(), &config(1_024)).expect("decoder");
            decoder
                .push(created("resp_live_item_cap").as_bytes())
                .expect("created");
            decoder
                .output_items
                .extend((0..MAX_OPENAI_TERMINAL_OUTPUT_ITEMS).map(|index| {
                    OutputItemAccumulator::Message {
                        item_id: format!("msg_retained_{index}"),
                        done: false,
                    }
                }));

            let event = output_item_event(
                "response.output_item.added",
                MAX_OPENAI_TERMINAL_OUTPUT_ITEMS,
                item,
            );
            assert!(matches!(
                decoder.push(event.as_bytes()),
                Err(AgentProviderProtocolError::Limit)
            ));
            assert_eq!(decoder.output_items.len(), MAX_OPENAI_TERMINAL_OUTPUT_ITEMS);
            assert!(decoder.tools.is_empty());
            assert_eq!(decoder.encrypted_reasoning_bytes, 0);
        }
    }

    #[test]
    fn terminal_output_deserialization_never_preallocates_past_the_item_cap() {
        let at_limit = (0..MAX_OPENAI_TERMINAL_OUTPUT_ITEMS)
            .map(|index| json!({"type": "unknown", "id": format!("item_{index}")}))
            .collect::<Vec<_>>();
        let at_limit_wire = json!({
            "type": "response.completed",
            "response": {
                "id": "resp_terminal_cap",
                "status": "completed",
                "model": "gpt-5.6-terra",
                "service_tier": "default",
                "output": at_limit,
                "usage": null
            }
        })
        .to_string();
        let decoded: TerminalEnvelope<'_> = parse(&at_limit_wire).expect("bounded terminal output");
        assert_eq!(
            decoded.response.output.len(),
            MAX_OPENAI_TERMINAL_OUTPUT_ITEMS
        );

        let over_limit = (0..=MAX_OPENAI_TERMINAL_OUTPUT_ITEMS)
            .map(|index| json!({"type": "unknown", "id": format!("item_{index}")}))
            .collect::<Vec<_>>();
        let over_limit_wire = json!({
            "type": "response.completed",
            "response": {
                "id": "resp_terminal_over_cap",
                "status": "completed",
                "model": "gpt-5.6-terra",
                "service_tier": "default",
                "output": over_limit,
                "usage": null
            }
        })
        .to_string();
        assert!(matches!(
            parse::<TerminalEnvelope<'_>>(&over_limit_wire),
            Err(AgentProviderProtocolError::Event)
        ));
    }

    #[test]
    fn post_terminal_duplicate_done_unknown_and_trailing_framing_fail_closed() {
        let terminal_event = terminal("resp_eof_gate", "response.completed", "completed", "[]");

        let mut post_terminal =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        post_terminal
            .push(created("resp_eof_gate").as_bytes())
            .expect("created");
        let terminal_batch = post_terminal
            .push(terminal_event.as_bytes())
            .expect("terminal event");
        assert!(terminal_batch.deltas().is_empty());
        assert_eq!(
            post_terminal.push(sse("ping", r#"{"type":"ping"}"#).as_bytes()),
            Err(AgentProviderProtocolError::Sequence)
        );

        let mut duplicate_done =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        duplicate_done
            .push(created("resp_eof_gate").as_bytes())
            .expect("created");
        duplicate_done
            .push(terminal_event.as_bytes())
            .expect("terminal");
        duplicate_done
            .push(b"data: [DONE]\n\n")
            .expect("single done");
        assert_eq!(
            duplicate_done.push(b"data: [DONE]\n\n"),
            Err(AgentProviderProtocolError::Terminal)
        );

        let mut unknown =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        unknown
            .push(created("resp_unknown").as_bytes())
            .expect("created");
        assert_eq!(
            unknown.push(
                sse(
                    "response.future_output",
                    r#"{"type":"response.future_output","opaque":true}"#,
                )
                .as_bytes(),
            ),
            Err(AgentProviderProtocolError::UnsupportedOutput)
        );

        let mut trailing =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        trailing
            .push(created("resp_eof_gate").as_bytes())
            .expect("created");
        trailing.push(terminal_event.as_bytes()).expect("terminal");
        trailing
            .push(b"data: unfinished")
            .expect("buffer bounded trailing bytes");
        assert!(matches!(
            trailing.finish(),
            Err(AgentProviderProtocolError::Framing)
        ));
    }

    #[test]
    fn in_progress_and_content_part_lifecycle_are_one_shot_and_correlated() {
        let mut progress =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        progress
            .push(created("resp_progress_once").as_bytes())
            .expect("created");
        progress
            .push(in_progress("resp_progress_once").as_bytes())
            .expect("in progress");
        assert_eq!(
            progress.push(in_progress("resp_progress_once").as_bytes()),
            Err(AgentProviderProtocolError::Sequence)
        );

        let mut before_added =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        before_added
            .push(created("resp_before_added").as_bytes())
            .expect("created");
        before_added
            .push(message_item_event("response.output_item.added", 0, "msg_before").as_bytes())
            .expect("message");
        assert_eq!(
            before_added.push(
                text_event("response.output_text.delta", "msg_before", "delta", "x").as_bytes()
            ),
            Err(AgentProviderProtocolError::Sequence)
        );

        let mut duplicate_added =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        duplicate_added
            .push(created("resp_duplicate_part").as_bytes())
            .expect("created");
        duplicate_added
            .push(message_item_event("response.output_item.added", 0, "msg_part").as_bytes())
            .expect("message");
        let added = content_part_event("response.content_part.added", "msg_part", "output_text");
        duplicate_added.push(added.as_bytes()).expect("part");
        assert_eq!(
            duplicate_added.push(added.as_bytes()),
            Err(AgentProviderProtocolError::Sequence)
        );

        let mut cross_index =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        cross_index
            .push(created("resp_cross_index").as_bytes())
            .expect("created");
        cross_index
            .push(message_item_event("response.output_item.added", 0, "msg_cross").as_bytes())
            .expect("message");
        cross_index
            .push(
                content_part_event("response.content_part.added", "msg_cross", "output_text")
                    .as_bytes(),
            )
            .expect("part");
        assert_eq!(
            cross_index.push(
                sse(
                    "response.output_text.delta",
                    r#"{"type":"response.output_text.delta","item_id":"msg_cross","output_index":1,"content_index":0,"delta":"x"}"#,
                )
                .as_bytes(),
            ),
            Err(AgentProviderProtocolError::Sequence)
        );

        let mut out_of_order =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        out_of_order
            .push(created("resp_part_order").as_bytes())
            .expect("created");
        out_of_order
            .push(message_item_event("response.output_item.added", 0, "msg_order").as_bytes())
            .expect("message");
        out_of_order
            .push(
                content_part_event("response.content_part.added", "msg_order", "output_text")
                    .as_bytes(),
            )
            .expect("part");
        assert_eq!(
            out_of_order.push(
                content_part_event("response.content_part.done", "msg_order", "output_text")
                    .as_bytes(),
            ),
            Err(AgentProviderProtocolError::Sequence)
        );
    }
}
