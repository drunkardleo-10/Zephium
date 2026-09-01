//! Bounded OpenAI Responses SSE normalization.
//!
//! The decoder accepts only response lifecycle, plain-text/refusal output,
//! and terminal usage today. Function calls are deliberately rejected until
//! the closed browser-tool decoder can consume their arguments without
//! exposing raw JSON. Built-in provider tools and reasoning output are never
//! requested or surfaced.

use std::fmt;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::sse::{SseDecoder, SseEvent};
use super::{
    AgentProviderCallConfig, AgentProviderCallIdentity, AgentProviderCompletion,
    AgentProviderFailure, AgentProviderFailureClass, AgentProviderKind, AgentProviderModelRevision,
    AgentProviderProtocolError, AgentProviderStopReason, AgentProviderStreamBatch,
    AgentProviderStreamBudget, AgentProviderStreamConclusion, AgentProviderStreamEvent,
    AgentProviderStreamStats, AgentProviderTerminalFailure, AgentProviderTextDelta,
    AgentProviderUsage,
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
            "response.completed" | "response.incomplete" => {
                self.handle_success_terminal(event.data(), kind)
            }
            "response.failed" | "response.cancelled" => {
                self.handle_failure_terminal(event.data(), kind)
            }
            "error" => self.handle_stream_error(event.data()),
            "ping" => self.require_in_progress(),
            "response.function_call_arguments.delta"
            | "response.function_call_arguments.done"
            | "response.reasoning_text.delta"
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
        {
            return Err(AgentProviderProtocolError::Event);
        }
        self.validate_response_id(event.response.id)
    }

    fn handle_output_item(&self, data: &str) -> Result<(), AgentProviderProtocolError> {
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
            _ => Err(AgentProviderProtocolError::UnsupportedOutput),
        }
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
                AgentProviderTextDelta::new(event.delta.to_owned()),
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

    fn handle_success_terminal(
        &mut self,
        data: &str,
        kind: &str,
    ) -> Result<(), AgentProviderProtocolError> {
        self.require_in_progress()?;
        if !self.output.is_done() {
            return Err(AgentProviderProtocolError::Sequence);
        }
        let event: TerminalEnvelope<'_> = parse(data)?;
        if event.kind != kind
            || event.response.model != self.model.as_str()
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
        let output_kind = validate_terminal_output(&event.response.output)?;
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
            0,
            0,
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
    delta: &'a str,
}

#[derive(Deserialize)]
struct DoneEnvelope<'a> {
    #[serde(borrow, rename = "type")]
    kind: &'a str,
    #[serde(borrow, alias = "refusal")]
    text: &'a str,
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
) -> Result<TerminalOutputKind, AgentProviderProtocolError> {
    if output.len() > MAX_OPENAI_TERMINAL_OUTPUT_ITEMS {
        return Err(AgentProviderProtocolError::Limit);
    }
    let mut output_kind = TerminalOutputKind::None;
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
            _ => return Err(AgentProviderProtocolError::UnsupportedOutput),
        }
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
    use crate::{AgentModelCallId, AgentPlanLeaseId, AgentPlanNodeId, AgentRunManifestId};

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
                "{{\"type\":\"response.created\",\"response\":{{\"id\":\"{id}\",\"status\":\"in_progress\",\"model\":\"gpt-5.6-sol\"}}}}"
            ),
        )
    }

    fn terminal(id: &str, kind: &str, status: &str, output: &str) -> String {
        sse(
            kind,
            &format!(
                "{{\"type\":\"{kind}\",\"response\":{{\"id\":\"{id}\",\"status\":\"{status}\",\"model\":\"gpt-5.6-sol\",\"output\":{output},\"usage\":{{\"input_tokens\":17,\"output_tokens\":3,\"total_tokens\":20,\"input_tokens_details\":{{\"cached_tokens\":4}},\"output_tokens_details\":{{\"reasoning_tokens\":1}}}}}}}}"
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
    fn unsupported_tool_and_reasoning_output_never_cross_the_boundary() {
        for (name, body) in [
            (
                "response.function_call_arguments.delta",
                r#"{"type":"response.function_call_arguments.delta","delta":"{\\\"x\\\":"}"#,
            ),
            (
                "response.reasoning_text.delta",
                r#"{"type":"response.reasoning_text.delta","delta":"hidden"}"#,
            ),
        ] {
            let mut decoder =
                OpenAiResponsesStreamDecoder::try_new(call(), &config(64)).expect("decoder");
            decoder.push(created("resp_4").as_bytes()).expect("created");
            assert_eq!(
                decoder.push(sse(name, body).as_bytes()),
                Err(AgentProviderProtocolError::UnsupportedOutput)
            );
        }
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
            r#"{"type":"response.incomplete","response":{"id":"resp_5","status":"incomplete","model":"gpt-5.6-sol","output":[],"incomplete_details":{"reason":"max_output_tokens"},"usage":{"input_tokens":5,"output_tokens":7,"total_tokens":12}}}"#,
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
            r#"{"type":"response.failed","response":{"id":"resp_6","status":"failed","model":"gpt-5.6-sol","output":[],"usage":null,"error":{"code":"server_error","message":"must not escape"}}}"#,
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
            r#"{"type":"response.completed","response":{"id":"resp_8","status":"completed","model":"gpt-5.6-sol","output":[],"usage":{"input_tokens":1,"output_tokens":2,"total_tokens":99}}}"#,
        );
        assert_eq!(
            decoder.push(bad_usage.as_bytes()),
            Err(AgentProviderProtocolError::Usage)
        );
    }
}
