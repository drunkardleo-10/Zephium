//! Bounded Server-Sent Events framing for the lead wires.

use zephium_core::work::model::WorkModelError;

const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
const MAX_EVENT_BYTES: usize = 4 * 1024 * 1024;
const MAX_STREAM_BYTES: usize = 48 * 1024 * 1024;
const MAX_EVENTS: u32 = 200_000;

pub(crate) struct SseEvent {
    pub(crate) event: String,
    pub(crate) data: String,
}

#[derive(Default)]
pub(crate) struct SseFramer {
    line: Vec<u8>,
    event: String,
    data: String,
    has_data: bool,
    skip_lf: bool,
    bytes: usize,
    events: u32,
}

impl SseFramer {
    pub(crate) fn push(
        &mut self,
        chunk: &[u8],
        out: &mut Vec<SseEvent>,
    ) -> Result<(), WorkModelError> {
        self.bytes = self.bytes.saturating_add(chunk.len());
        if self.bytes > MAX_STREAM_BYTES {
            return Err(WorkModelError::Protocol);
        }
        for &byte in chunk {
            if self.skip_lf {
                self.skip_lf = false;
                if byte == b'\n' {
                    continue;
                }
            }
            match byte {
                b'\r' => {
                    self.line_end(out)?;
                    self.skip_lf = true;
                }
                b'\n' => self.line_end(out)?,
                _ => {
                    if self.line.len() >= MAX_LINE_BYTES {
                        return Err(WorkModelError::Protocol);
                    }
                    self.line.push(byte);
                }
            }
        }
        Ok(())
    }

    /// A stream may end without the blank line after its last event.
    pub(crate) fn finish(&mut self, out: &mut Vec<SseEvent>) -> Result<(), WorkModelError> {
        if !self.line.is_empty() {
            self.line_end(out)?;
        }
        self.dispatch(out)
    }

    fn line_end(&mut self, out: &mut Vec<SseEvent>) -> Result<(), WorkModelError> {
        if self.line.is_empty() {
            return self.dispatch(out);
        }
        let line = std::mem::take(&mut self.line);
        let line = std::str::from_utf8(&line).map_err(|_| WorkModelError::Protocol)?;
        if line.starts_with(':') {
            return Ok(());
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "event" => {
                self.event.clear();
                self.event.push_str(value);
            }
            "data" => {
                if self.has_data {
                    self.data.push('\n');
                }
                if self.data.len() + value.len() > MAX_EVENT_BYTES {
                    return Err(WorkModelError::Protocol);
                }
                self.data.push_str(value);
                self.has_data = true;
            }
            _ => {}
        }
        Ok(())
    }

    fn dispatch(&mut self, out: &mut Vec<SseEvent>) -> Result<(), WorkModelError> {
        if !self.has_data {
            self.event.clear();
            return Ok(());
        }
        self.events += 1;
        if self.events > MAX_EVENTS {
            return Err(WorkModelError::Protocol);
        }
        out.push(SseEvent {
            event: std::mem::take(&mut self.event),
            data: std::mem::take(&mut self.data),
        });
        self.has_data = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(chunks: &[&str]) -> Vec<(String, String)> {
        let mut framer = SseFramer::default();
        let mut out = Vec::new();
        for chunk in chunks {
            framer.push(chunk.as_bytes(), &mut out).unwrap();
        }
        framer.finish(&mut out).unwrap();
        out.into_iter().map(|e| (e.event, e.data)).collect()
    }

    #[test]
    fn events_split_across_chunks_and_line_endings_reassemble() {
        let events = frame(&[
            "event: a\r\ndata: {\"x\"",
            ":1}\r\n\r\n: keepalive\n\ndata: one\ndata: two\n\n",
            "data: tail",
        ]);
        assert_eq!(
            events,
            vec![
                ("a".into(), "{\"x\":1}".into()),
                (String::new(), "one\ntwo".into()),
                (String::new(), "tail".into()),
            ]
        );
    }

    #[test]
    fn an_oversized_line_fails_closed() {
        let mut framer = SseFramer::default();
        let mut out = Vec::new();
        let big = vec![b'a'; MAX_LINE_BYTES + 1];
        assert_eq!(framer.push(&big, &mut out), Err(WorkModelError::Protocol));
    }
}
