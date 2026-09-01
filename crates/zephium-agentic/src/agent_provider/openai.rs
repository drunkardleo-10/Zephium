//! Bounded OpenAI Responses SSE normalization.
//!
//! The decoder accepts response lifecycle, plain-text/refusal output, the
//! fixed client browser-tool vocabulary, and terminal usage. Function-call
//! JSON remains private, is bounded while streaming, and crosses the public
//! boundary only after the closed browser-tool decoder succeeds. Provider
//! built-in tools and reasoning output are never requested or surfaced.

use std::borrow::Cow;
use std::fmt;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::sse::{SseDecoder, SseEvent};
use super::{
    AgentBrowserToolCall, AgentBrowserToolCallId, AgentBrowserToolKind, AgentProviderCallConfig,
    AgentProviderCallIdentity, AgentProviderCompletion, AgentProviderFailure,
    AgentProviderFailureClass, AgentProviderKind, AgentProviderModelRevision,
    AgentProviderProtocolError, AgentProviderStopReason, AgentProviderStreamBatch,
    AgentProviderStreamBudget, AgentProviderStreamConclusion, AgentProviderStreamEvent,
    AgentProviderStreamStats, AgentProviderTerminalFailure, AgentProviderTextDelta,
    AgentProviderUsage, OPENAI_STANDARD_SERVICE_TIER,
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
}

/// Single-owner incremental decoder for one OpenAI Responses SSE body.
///
/// The HTTP shell supplies arbitrary byte chunks and must call `finish` once
/// EOF is reached. Protocol errors are content-free. A terminal provider
/// failure is returned as a typed conclusion rather than being confused with
/// malformed wire data.
#[must_use]
pub struct OpenAiResponsesStreamDecoder {
    call: AgentProviderCallIdentity,
    model: AgentProviderModelRevision,
    budget: AgentProviderStreamBudget,
    sse: SseDecoder,
    phase: StreamPhase,
    response_id: Option<String>,
    output: OutputState,
    tools: Vec<ToolAccumulator>,
    tool_argument_bytes: u32,
    conclusion: Option<AgentProviderStreamConclusion>,
    failure: Option<AgentProviderProtocolError>,
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
            model: config.model().clone(),
            budget: config.stream_budget(),
            sse: SseDecoder::new(
                config.stream_budget().max_events(),
                config.stream_budget().max_wire_bytes(),
            )?,
            phase: StreamPhase::AwaitCreated,
            response_id: None,
            output: OutputState::None,
            tools: Vec::with_capacity(usize::from(config.stream_budget().max_tool_calls())),
            tool_argument_bytes: 0,
            conclusion: None,
            failure: None,
        })
    }

    /// Decodes one arbitrary transport chunk into bounded normalized events.
    pub fn push(
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
    pub fn finish(self) -> Result<AgentProviderStreamConclusion, AgentProviderProtocolError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        self.sse.finish()?;
        if self.phase != StreamPhase::Terminal {
            return Err(AgentProviderProtocolError::Terminal);
        }
        let conclusion = self
            .conclusion
            .ok_or(AgentProviderProtocolError::Terminal)?;
        if matches!(conclusion, AgentProviderStreamConclusion::Completed(_))
            && !self.output.is_done()
        {
            return Err(AgentProviderProtocolError::Terminal);
        }
        Ok(conclusion)
    }

    fn handle_event(
        &mut self,
        event: SseEvent,
        output: &mut Vec<AgentProviderStreamEvent>,
    ) -> Result<(), AgentProviderProtocolError> {
        if event.data() == "[DONE]" {
            return if self.phase == StreamPhase::Terminal {
                Ok(())
            } else {
                Err(AgentProviderProtocolError::Terminal)
            };
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
                self.handle_tool_arguments_done(event.data(), output)
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

    fn handle_created(&mut self, data: &str) -> Result<(), AgentProviderProtocolError> {
        if self.phase != StreamPhase::AwaitCreated {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let event: ResponseEnvelope<'_> = parse(data)?;
        if event.kind != "response.created"
            || event.response.status != "in_progress"
            || event.response.model != self.model.as_str()
            || event.response.service_tier != OPENAI_STANDARD_SERVICE_TIER
        {
            return Err(AgentProviderProtocolError::Event);
        }
        validate_response_id(event.response.id)?;
        self.response_id = Some(event.response.id.to_owned());
        self.phase = StreamPhase::InProgress;
        Ok(())
    }

    fn handle_in_progress(&self, data: &str) -> Result<(), AgentProviderProtocolError> {
        self.require_in_progress()?;
        let event: ResponseEnvelope<'_> = parse(data)?;
        if event.kind != "response.in_progress"
            || event.response.status != "in_progress"
            || event.response.model != self.model.as_str()
            || event.response.service_tier != OPENAI_STANDARD_SERVICE_TIER
        {
            return Err(AgentProviderProtocolError::Event);
        }
        self.validate_response_id(event.response.id)
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
        match event.item.kind {
            "message" | "reasoning" => Ok(()),
            "function_call" if event.kind == "response.output_item.added" => {
                self.handle_tool_item_added(event.item)
            }
            "function_call" => self.handle_tool_item_done(event.item),
            _ => Err(AgentProviderProtocolError::UnsupportedOutput),
        }
    }

    fn handle_tool_item_added(
        &mut self,
        item: OutputItemHead<'_>,
    ) -> Result<(), AgentProviderProtocolError> {
        if self.tools.len() >= usize::from(self.budget.max_tool_calls()) {
            return Err(AgentProviderProtocolError::Limit);
        }
        let item_id = item.id.ok_or(AgentProviderProtocolError::Event)?;
        let call_id = item.call_id.ok_or(AgentProviderProtocolError::Event)?;
        let name = item.name.ok_or(AgentProviderProtocolError::Event)?;
        let arguments = item.arguments.ok_or(AgentProviderProtocolError::Event)?;
        if item.status != Some("in_progress") || !arguments.is_empty() {
            return Err(AgentProviderProtocolError::Sequence);
        }
        validate_response_id(item_id)?;
        AgentBrowserToolCallId::try_new(call_id.to_owned())
            .map_err(|_| AgentProviderProtocolError::ToolCall)?;
        let name = AgentBrowserToolKind::parse(name).ok_or(AgentProviderProtocolError::ToolCall)?;
        if self
            .tools
            .iter()
            .any(|tool| tool.item_id == item_id || tool.call_id == call_id)
        {
            return Err(AgentProviderProtocolError::Sequence);
        }
        self.tools.push(ToolAccumulator {
            item_id: item_id.to_owned(),
            call_id: call_id.to_owned(),
            name,
            arguments: Some(String::new()),
            argument_bytes: 0,
            argument_guard: None,
            item_done: false,
        });
        Ok(())
    }

    fn handle_tool_item_done(
        &mut self,
        item: OutputItemHead<'_>,
    ) -> Result<(), AgentProviderProtocolError> {
        let item_id = item.id.ok_or(AgentProviderProtocolError::Event)?;
        let call_id = item.call_id.ok_or(AgentProviderProtocolError::Event)?;
        let name = item.name.ok_or(AgentProviderProtocolError::Event)?;
        let arguments = item.arguments.ok_or(AgentProviderProtocolError::Event)?;
        let tool = self
            .tools
            .iter_mut()
            .find(|tool| tool.item_id == item_id)
            .ok_or(AgentProviderProtocolError::Sequence)?;
        let argument_guard = tool
            .argument_guard
            .ok_or(AgentProviderProtocolError::Sequence)?;
        let actual_guard: [u8; 32] = Sha256::digest(arguments.as_bytes()).into();
        if tool.item_done
            || item.status != Some("completed")
            || tool.call_id != call_id
            || tool.name.as_str() != name
            || usize::try_from(tool.argument_bytes).ok() != Some(arguments.len())
            || argument_guard != actual_guard
        {
            return Err(AgentProviderProtocolError::Sequence);
        }
        tool.item_done = true;
        Ok(())
    }

    fn handle_content_part(&self, data: &str) -> Result<(), AgentProviderProtocolError> {
        self.require_in_progress()?;
        let event: ContentPartEnvelope<'_> = parse(data)?;
        if !matches!(
            event.kind,
            "response.content_part.added" | "response.content_part.done"
        ) {
            return Err(AgentProviderProtocolError::Event);
        }
        match event.part.kind {
            "output_text" | "refusal" => Ok(()),
            _ => Err(AgentProviderProtocolError::UnsupportedOutput),
        }
    }

    fn handle_output_delta(
        &mut self,
        data: &str,
        refusal: bool,
        output: &mut Vec<AgentProviderStreamEvent>,
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
            output.push(AgentProviderStreamEvent::TextDelta(
                AgentProviderTextDelta::new(event.delta.into_owned()),
            ));
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
        let tool = self
            .tools
            .iter_mut()
            .find(|tool| tool.item_id == event.item_id)
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

    fn handle_tool_arguments_done(
        &mut self,
        data: &str,
        output: &mut Vec<AgentProviderStreamEvent>,
    ) -> Result<(), AgentProviderProtocolError> {
        self.require_in_progress()?;
        let event: ToolArgumentsDoneEnvelope<'_> = parse(data)?;
        if event.kind != "response.function_call_arguments.done" {
            return Err(AgentProviderProtocolError::Event);
        }
        let tool = self
            .tools
            .iter_mut()
            .find(|tool| tool.item_id == event.item_id)
            .ok_or(AgentProviderProtocolError::Sequence)?;
        if tool.item_done || tool.argument_guard.is_some() || tool.name.as_str() != event.name {
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
        let call =
            AgentBrowserToolCall::decode(tool.call_id.clone(), tool.name.as_str(), &arguments)
                .map_err(|_| AgentProviderProtocolError::ToolCall)?;
        tool.argument_guard = Some(guard);
        output.push(AgentProviderStreamEvent::ToolCall(call));
        Ok(())
    }

    fn handle_success_terminal(
        &mut self,
        data: &str,
        kind: &str,
    ) -> Result<(), AgentProviderProtocolError> {
        self.require_in_progress()?;
        if !self.output.is_done() || self.tools.iter().any(|tool| !tool.item_done) {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let event: TerminalEnvelope<'_> = parse(data)?;
        if event.kind != kind
            || event.response.model != self.model.as_str()
            || event.response.service_tier != OPENAI_STANDARD_SERVICE_TIER
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
        let output_kind = validate_terminal_output(&event.response.output, &self.tools)?;
        if !output_kind.matches_state(&self.output) {
            return Err(AgentProviderProtocolError::Sequence);
        }
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
        let stats = self.stats();
        self.conclusion = Some(AgentProviderStreamConclusion::Completed(
            AgentProviderCompletion::new(self.call, stop, usage, stats),
        ));
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
            || event.response.model != self.model.as_str()
            || event.response.service_tier != OPENAI_STANDARD_SERVICE_TIER
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
        let stats = self.stats();
        self.conclusion = Some(AgentProviderStreamConclusion::Failed(
            AgentProviderTerminalFailure::new(self.call, failure, usage, stats),
        ));
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
        let stats = self.stats();
        self.conclusion = Some(AgentProviderStreamConclusion::Failed(
            AgentProviderTerminalFailure::new(self.call, failure, None, stats),
        ));
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
            .field("model", &self.model)
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
}

#[derive(Deserialize)]
struct ContentPartEnvelope<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
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
    delta: Cow<'a, str>,
}

#[derive(Deserialize)]
struct DoneEnvelope<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    #[serde(borrow, alias = "refusal")]
    text: Cow<'a, str>,
}

#[derive(Deserialize)]
struct ToolArgumentsDeltaEnvelope<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    #[serde(borrow)]
    item_id: &'a str,
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
    name: &'a str,
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
    #[serde(borrow, default)]
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
}

#[derive(Deserialize)]
struct TerminalContentPart<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
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

fn validate_terminal_output(
    output: &[TerminalOutputItem<'_>],
    tools: &[ToolAccumulator],
) -> Result<TerminalOutputKind, AgentProviderProtocolError> {
    if output.len() > MAX_OPENAI_TERMINAL_OUTPUT_ITEMS {
        return Err(AgentProviderProtocolError::Limit);
    }
    let mut output_kind = TerminalOutputKind::None;
    let mut tool_index = 0_usize;
    for item in output {
        match item.kind {
            "reasoning" => {
                if !item.content.is_empty() {
                    return Err(AgentProviderProtocolError::UnsupportedOutput);
                }
            }
            "message" => {
                if item.content.len() > MAX_OPENAI_TERMINAL_CONTENT_PARTS {
                    return Err(AgentProviderProtocolError::Limit);
                }
                if item
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
                    || tool.item_id != item_id
                    || tool.call_id != call_id
                    || tool.name.as_str() != name
                    || usize::try_from(tool.argument_bytes).ok() != Some(arguments.len())
                    || argument_guard != actual_guard
                {
                    return Err(AgentProviderProtocolError::Sequence);
                }
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
    Ok(output_kind)
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
        AgentModelCallId, AgentPlanLeaseId, AgentPlanNodeId, AgentProviderPricingProfile,
        AgentProviderPricingRevision, AgentRunManifestId, SemanticTokenizerRevision,
    };

    fn call() -> AgentProviderCallIdentity {
        AgentProviderCallIdentity {
            manifest: AgentRunManifestId::from_raw(1),
            call: AgentModelCallId::new(2).expect("call"),
            lease: AgentPlanLeaseId::from_raw(3),
            node: AgentPlanNodeId::from_raw(4),
        }
    }

    fn config(max_text: u32) -> AgentProviderCallConfig {
        AgentProviderCallConfig::try_new(
            AgentProviderKind::OpenAiResponses,
            AgentProviderModelRevision::try_new("gpt-5.6-sol".to_owned()).expect("model"),
            SemanticTokenizerRevision::try_new("openai:gpt-5.6-sol:v1".to_owned())
                .expect("tokenizer"),
            AgentProviderPricingProfile::try_new(
                AgentProviderPricingRevision::new(1).expect("pricing revision"),
                16_384,
            )
            .expect("pricing profile"),
            512,
            1_024,
            AgentProviderStreamBudget::try_new(64 * 1024, 64, max_text, 2, 1_024).expect("budget"),
        )
        .expect("config")
    }

    fn sse(event: &str, data: &str) -> String {
        format!("event: {event}\ndata: {data}\n\n")
    }

    fn created(id: &str) -> String {
        sse(
            "response.created",
            &format!(
                "{{\"type\":\"response.created\",\"response\":{{\"id\":\"{id}\",\"status\":\"in_progress\",\"model\":\"gpt-5.6-sol\",\"service_tier\":\"default\"}}}}"
            ),
        )
    }

    fn terminal(id: &str, kind: &str, status: &str, output: &str) -> String {
        sse(
            kind,
            &format!(
                "{{\"type\":\"{kind}\",\"response\":{{\"id\":\"{id}\",\"status\":\"{status}\",\"model\":\"gpt-5.6-sol\",\"service_tier\":\"default\",\"output\":{output},\"usage\":{{\"input_tokens\":17,\"output_tokens\":3,\"total_tokens\":20,\"input_tokens_details\":{{\"cached_tokens\":4}},\"output_tokens_details\":{{\"reasoning_tokens\":1}}}}}}}}"
            ),
        )
    }

    #[test]
    fn fragmented_text_stream_normalizes_deltas_and_terminal_usage() {
        let mut decoder =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        let stream = [
            created("resp_1"),
            sse(
                "response.output_text.delta",
                r#"{"type":"response.output_text.delta","delta":"hello "}"#,
            ),
            sse(
                "response.output_text.delta",
                r#"{"type":"response.output_text.delta","delta":"world"}"#,
            ),
            sse(
                "response.output_text.done",
                r#"{"type":"response.output_text.done","text":"hello world"}"#,
            ),
            terminal(
                "resp_1",
                "response.completed",
                "completed",
                r#"[{"type":"message","content":[{"type":"output_text"}]}]"#,
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
            .into_events()
            .into_iter()
            .chain(second.into_events())
            .chain(third.into_events())
            .map(|event| match event {
                AgentProviderStreamEvent::TextDelta(delta) => delta.as_str().to_owned(),
                AgentProviderStreamEvent::ToolCall(_) => panic!("unexpected tool call"),
            })
            .collect::<String>();
        assert_eq!(text, "hello world");
        let conclusion = decoder.finish().expect("terminal");
        let AgentProviderStreamConclusion::Completed(completion) = conclusion else {
            panic!("expected completion");
        };
        assert_eq!(completion.stop(), AgentProviderStopReason::Completed);
        assert_eq!(completion.usage().total_tokens(), 20);
        assert_eq!(completion.usage().cached_input_tokens(), 4);
        assert_eq!(completion.stats().output_text_bytes(), 11);
        assert_eq!(completion.stats().events(), 6);
        assert_eq!(
            completion.stats().wire_bytes(),
            u32::try_from(stream.len()).expect("stream bytes")
        );
    }

    #[test]
    fn terminal_text_mismatch_and_output_limit_fail_closed() {
        let mut mismatch =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(16)).expect("decoder");
        mismatch
            .push(created("resp_2").as_bytes())
            .expect("created");
        mismatch
            .push(
                sse(
                    "response.output_text.delta",
                    r#"{"type":"response.output_text.delta","delta":"abc"}"#,
                )
                .as_bytes(),
            )
            .expect("delta");
        assert_eq!(
            mismatch.push(
                sse(
                    "response.output_text.done",
                    r#"{"type":"response.output_text.done","text":"xyz"}"#,
                )
                .as_bytes(),
            ),
            Err(AgentProviderProtocolError::Sequence)
        );
        assert_eq!(
            mismatch.push(b""),
            Err(AgentProviderProtocolError::Sequence)
        );

        let mut limited =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(3)).expect("decoder");
        limited.push(created("resp_3").as_bytes()).expect("created");
        assert_eq!(
            limited.push(
                sse(
                    "response.output_text.delta",
                    r#"{"type":"response.output_text.delta","delta":"four"}"#,
                )
                .as_bytes(),
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
                    "delta": "{\"url\":"
                })
                .to_string(),
            ),
            sse(
                "response.function_call_arguments.delta",
                &serde_json::json!({
                    "type": "response.function_call_arguments.delta",
                    "item_id": "fc_1",
                    "delta": "\"https://example.test/path\"}"
                })
                .to_string(),
            ),
            sse(
                "response.function_call_arguments.done",
                &serde_json::json!({
                    "type": "response.function_call_arguments.done",
                    "item_id": "fc_1",
                    "name": "navigate",
                    "arguments": arguments
                })
                .to_string(),
            ),
            sse(
                "response.output_item.done",
                &serde_json::json!({
                    "type": "response.output_item.done",
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
        let mut events = batch.into_events();
        assert_eq!(events.len(), 1);
        let AgentProviderStreamEvent::ToolCall(tool) = events.remove(0) else {
            panic!("typed tool call");
        };
        assert_eq!(tool.id().as_str(), "call_tool1");
        let (_, proposal) = tool.into_parts();
        let crate::AgentBrowserToolProposal::Navigate(target) = proposal else {
            panic!("navigate proposal");
        };
        assert_eq!(target.as_url().as_str(), "https://example.test/path");

        let AgentProviderStreamConclusion::Completed(completion) =
            decoder.finish().expect("terminal")
        else {
            panic!("completed tool response");
        };
        assert_eq!(completion.stop(), AgentProviderStopReason::ToolCalls);
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
    fn incomplete_and_failed_terminals_are_typed_without_provider_text() {
        let mut incomplete =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        incomplete
            .push(created("resp_5").as_bytes())
            .expect("created");
        let event = sse(
            "response.incomplete",
            r#"{"type":"response.incomplete","response":{"id":"resp_5","status":"incomplete","model":"gpt-5.6-sol","service_tier":"default","output":[],"incomplete_details":{"reason":"max_output_tokens"},"usage":{"input_tokens":5,"output_tokens":7,"total_tokens":12}}}"#,
        );
        incomplete.push(event.as_bytes()).expect("incomplete");
        let AgentProviderStreamConclusion::Completed(completion) =
            incomplete.finish().expect("terminal")
        else {
            panic!("expected typed incomplete response");
        };
        assert_eq!(completion.stop(), AgentProviderStopReason::OutputLimit);

        let mut failed =
            OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
        failed.push(created("resp_6").as_bytes()).expect("created");
        let event = sse(
            "response.failed",
            r#"{"type":"response.failed","response":{"id":"resp_6","status":"failed","model":"gpt-5.6-sol","service_tier":"default","output":[],"usage":null,"error":{"code":"server_error","message":"must not escape"}}}"#,
        );
        failed.push(event.as_bytes()).expect("failure");
        let conclusion = failed.finish().expect("terminal");
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
            r#"{"type":"response.completed","response":{"id":"resp_8","status":"completed","model":"gpt-5.6-sol","service_tier":"default","output":[],"usage":{"input_tokens":1,"output_tokens":2,"total_tokens":99}}}"#,
        );
        assert_eq!(
            decoder.push(bad_usage.as_bytes()),
            Err(AgentProviderProtocolError::Usage)
        );
    }
}
