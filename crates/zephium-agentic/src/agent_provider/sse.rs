//! Incremental bounded UTF-8 Server-Sent Events framing shared by provider codecs.

use super::{
    AgentProviderProtocolError, MAX_AGENT_PROVIDER_SSE_EVENT_BYTES,
    MAX_AGENT_PROVIDER_SSE_LINE_BYTES, MAX_AGENT_PROVIDER_STREAM_EVENTS,
    MAX_AGENT_PROVIDER_STREAM_WIRE_BYTES,
};

const MAX_SSE_EVENT_NAME_BYTES: usize = 128;

pub(super) struct SseEvent {
    event: String,
    data: String,
}

impl SseEvent {
    pub(super) fn event(&self) -> &str {
        &self.event
    }

    pub(super) fn data(&self) -> &str {
        &self.data
    }
}

pub(super) struct SseDecoder {
    line: Vec<u8>,
    event: Option<String>,
    data: String,
    has_data: bool,
    skip_lf: bool,
    events: u32,
    max_events: u32,
    wire_bytes: u32,
    max_wire_bytes: u32,
    failure: Option<AgentProviderProtocolError>,
}

impl SseDecoder {
    pub(super) fn new(
        max_events: u32,
        max_wire_bytes: u32,
    ) -> Result<Self, AgentProviderProtocolError> {
        if max_events == 0
            || max_events > MAX_AGENT_PROVIDER_STREAM_EVENTS
            || max_wire_bytes == 0
            || max_wire_bytes > MAX_AGENT_PROVIDER_STREAM_WIRE_BYTES
        {
            return Err(AgentProviderProtocolError::Limit);
        }
        Ok(Self {
            line: Vec::with_capacity(1_024),
            event: None,
            data: String::with_capacity(4_096),
            has_data: false,
            skip_lf: false,
            events: 0,
            max_events,
            wire_bytes: 0,
            max_wire_bytes,
            failure: None,
        })
    }

    pub(super) fn push(
        &mut self,
        bytes: &[u8],
        output: &mut Vec<SseEvent>,
    ) -> Result<(), AgentProviderProtocolError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let result = self.push_inner(bytes, output);
        if let Err(error) = result {
            self.failure = Some(error);
        }
        result
    }

    fn push_inner(
        &mut self,
        bytes: &[u8],
        output: &mut Vec<SseEvent>,
    ) -> Result<(), AgentProviderProtocolError> {
        let chunk_bytes =
            u32::try_from(bytes.len()).map_err(|_| AgentProviderProtocolError::Limit)?;
        let next_wire_bytes = self
            .wire_bytes
            .checked_add(chunk_bytes)
            .ok_or(AgentProviderProtocolError::Limit)?;
        if next_wire_bytes > self.max_wire_bytes {
            return Err(AgentProviderProtocolError::Limit);
        }
        self.wire_bytes = next_wire_bytes;
        for &byte in bytes {
            if self.skip_lf {
                self.skip_lf = false;
                if byte == b'\n' {
                    continue;
                }
            }
            match byte {
                b'\r' => {
                    self.finish_line(output)?;
                    self.skip_lf = true;
                }
                b'\n' => self.finish_line(output)?,
                _ => {
                    if self.line.len() >= MAX_AGENT_PROVIDER_SSE_LINE_BYTES {
                        return Err(AgentProviderProtocolError::Limit);
                    }
                    self.line.push(byte);
                }
            }
        }
        Ok(())
    }

    pub(super) fn finish(self) -> Result<(), AgentProviderProtocolError> {
        if let Some(error) = self.failure {
            Err(error)
        } else if self.line.is_empty() && self.event.is_none() && !self.has_data {
            Ok(())
        } else {
            Err(AgentProviderProtocolError::Framing)
        }
    }

    pub(super) const fn events(&self) -> u32 {
        self.events
    }

    pub(super) const fn wire_bytes(&self) -> u32 {
        self.wire_bytes
    }

    fn finish_line(
        &mut self,
        output: &mut Vec<SseEvent>,
    ) -> Result<(), AgentProviderProtocolError> {
        if self.line.is_empty() {
            return self.dispatch(output);
        }
        let line =
            std::str::from_utf8(&self.line).map_err(|_| AgentProviderProtocolError::Framing)?;
        if line.starts_with(':') {
            self.line.clear();
            return Ok(());
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "event" => {
                if value.len() > MAX_SSE_EVENT_NAME_BYTES {
                    return Err(AgentProviderProtocolError::Limit);
                }
                self.event = Some(value.to_owned());
            }
            "data" => {
                let separator = usize::from(self.has_data);
                let next = self
                    .data
                    .len()
                    .checked_add(separator)
                    .and_then(|bytes| bytes.checked_add(value.len()))
                    .ok_or(AgentProviderProtocolError::Limit)?;
                if next > MAX_AGENT_PROVIDER_SSE_EVENT_BYTES {
                    return Err(AgentProviderProtocolError::Limit);
                }
                if self.has_data {
                    self.data.push('\n');
                }
                self.data.push_str(value);
                self.has_data = true;
            }
            _ => {}
        }
        self.line.clear();
        Ok(())
    }

    fn dispatch(&mut self, output: &mut Vec<SseEvent>) -> Result<(), AgentProviderProtocolError> {
        if !self.has_data {
            self.event = None;
            return Ok(());
        }
        self.events = self
            .events
            .checked_add(1)
            .ok_or(AgentProviderProtocolError::Limit)?;
        if self.events > self.max_events {
            return Err(AgentProviderProtocolError::Limit);
        }
        output.push(SseEvent {
            event: self.event.take().unwrap_or_else(|| "message".to_owned()),
            data: std::mem::take(&mut self.data),
        });
        self.has_data = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragmented_crlf_multiline_and_comments_are_canonicalized() {
        let mut decoder = SseDecoder::new(4, 1_024).expect("decoder");
        let mut events = Vec::new();
        decoder
            .push(b": ping\r\nevent: sample\r\ndata: {\"a\":\r", &mut events)
            .expect("first chunk");
        decoder
            .push(b"\ndata: 1}\r\nignored: value\r\n\r\n", &mut events)
            .expect("second chunk");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event(), "sample");
        assert_eq!(events[0].data(), "{\"a\":\n1}");
        decoder.finish().expect("complete stream framing");
    }

    #[test]
    fn bare_lf_default_event_and_empty_data_are_supported() {
        let mut decoder = SseDecoder::new(2, 1_024).expect("decoder");
        let mut events = Vec::new();
        decoder
            .push(b"data\n\ndata: two\n\n", &mut events)
            .expect("events");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event(), "message");
        assert_eq!(events[0].data(), "");
        assert_eq!(events[1].data(), "two");
        decoder.finish().expect("complete stream framing");
    }

    #[test]
    fn invalid_utf8_partial_eof_and_event_overflow_fail_closed() {
        let mut decoder = SseDecoder::new(1, 1_024).expect("decoder");
        let mut events = Vec::new();
        assert_eq!(
            decoder.push(b"data: \xff\n", &mut events),
            Err(AgentProviderProtocolError::Framing)
        );

        let mut decoder = SseDecoder::new(1, 1_024).expect("decoder");
        decoder
            .push(b"event: partial\ndata: {}\n", &mut Vec::new())
            .expect("buffer partial event");
        assert_eq!(decoder.finish(), Err(AgentProviderProtocolError::Framing));

        let mut decoder = SseDecoder::new(1, 1_024).expect("decoder");
        decoder
            .push(b"data: one\n\n", &mut events)
            .expect("first event");
        assert_eq!(
            decoder.push(b"data: two\n\n", &mut events),
            Err(AgentProviderProtocolError::Limit)
        );
    }

    #[test]
    fn line_and_aggregate_event_byte_limits_are_independent() {
        let mut decoder = SseDecoder::new(
            1,
            u32::try_from(MAX_AGENT_PROVIDER_SSE_LINE_BYTES + 1).expect("wire bound"),
        )
        .expect("decoder");
        let mut events = Vec::new();
        let line = vec![b'x'; MAX_AGENT_PROVIDER_SSE_LINE_BYTES + 1];
        assert_eq!(
            decoder.push(&line, &mut events),
            Err(AgentProviderProtocolError::Limit)
        );

        let mut decoder = SseDecoder::new(
            1,
            u32::try_from(MAX_AGENT_PROVIDER_SSE_EVENT_BYTES + 32).expect("wire bound"),
        )
        .expect("decoder");
        let first_part = MAX_AGENT_PROVIDER_SSE_EVENT_BYTES / 2;
        let second_part = MAX_AGENT_PROVIDER_SSE_EVENT_BYTES - first_part - 1;
        let first = format!(
            "data: {}\ndata: {}\n",
            "x".repeat(first_part),
            "x".repeat(second_part),
        );
        decoder
            .push(first.as_bytes(), &mut events)
            .expect("exact event boundary");
        assert_eq!(
            decoder.push(b"data:\n", &mut events),
            Err(AgentProviderProtocolError::Limit)
        );
    }

    #[test]
    fn total_wire_limit_fail_stops_before_processing_the_overflow_chunk() {
        let mut decoder = SseDecoder::new(2, 9).expect("decoder");
        let mut events = Vec::new();
        decoder
            .push(b"data:x\n\n", &mut events)
            .expect("first event");
        assert_eq!(decoder.wire_bytes(), 8);
        assert_eq!(events.len(), 1);
        assert_eq!(
            decoder.push(b"xx", &mut events),
            Err(AgentProviderProtocolError::Limit)
        );
        assert_eq!(decoder.wire_bytes(), 8);
        assert_eq!(
            decoder.push(b"", &mut events),
            Err(AgentProviderProtocolError::Limit)
        );
    }
}
