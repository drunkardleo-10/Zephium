use std::io::{ErrorKind, Read, Write};

use thiserror::Error;

use crate::PrivateFsError;

/// Hard ceiling for one regular file admitted by the streaming interface.
pub const MAX_STREAMING_FILE_BYTES: u64 = 128 * 1024 * 1024;

const STREAMING_BUFFER_BYTES: usize = 64 * 1024;

/// Exact regular-file length accepted by the bounded streaming writer.
///
/// Unlike the in-memory [`crate::ByteLimit`], zero is a valid exact length.
/// The value is always within [`MAX_STREAMING_FILE_BYTES`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StreamingFileLength(u64);

impl StreamingFileLength {
    /// Exact empty-file length.
    pub const ZERO: Self = Self(0);

    /// Validates one exact streaming length.
    pub fn new(bytes: u64) -> Result<Self, PrivateFsError> {
        if bytes > MAX_STREAMING_FILE_BYTES {
            return Err(PrivateFsError::BoundExceeded);
        }
        Ok(Self(bytes))
    }

    /// Returns the exact byte length.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Stable failure from a create-new streaming write.
///
/// Source I/O errors are deliberately erased: this boundary never retains an
/// upstream error object or any source bytes. A source failure is returned only
/// after the created node was durably removed and exact absence was proven.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum StreamingWriteError {
    /// The private-filesystem boundary or durability protocol failed.
    #[error("streaming private-file write failed: {0}")]
    Filesystem(PrivateFsError),
    /// The source reader returned an error, including during the final EOF probe.
    #[error("streaming private-file source returned an error")]
    SourceRead,
    /// The source reached EOF before the declared exact length.
    #[error("streaming private-file source ended before its declared length")]
    SourceTooShort,
    /// The source yielded bytes beyond the declared exact length.
    #[error("streaming private-file source exceeded its declared length")]
    SourceTooLong,
    /// The held destination inode did not retain the declared exact length.
    ///
    /// This is returned only after the mismatched inode was durably removed
    /// and exact absence was proven, so the namespace lease remains usable.
    #[error("streaming private-file sink length did not match its declared length")]
    SinkLengthMismatch,
}

impl From<PrivateFsError> for StreamingWriteError {
    fn from(error: PrivateFsError) -> Self {
        Self::Filesystem(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExactCopyError {
    SourceRead,
    SourceTooShort,
    SourceTooLong,
    SinkWrite,
}

impl ExactCopyError {
    pub(crate) const fn into_public(self) -> StreamingWriteError {
        match self {
            Self::SourceRead => StreamingWriteError::SourceRead,
            Self::SourceTooShort => StreamingWriteError::SourceTooShort,
            Self::SourceTooLong => StreamingWriteError::SourceTooLong,
            Self::SinkWrite => StreamingWriteError::Filesystem(PrivateFsError::Io),
        }
    }
}

/// Copies exactly `expected` bytes and then proves source EOF.
///
/// The single fixed stack buffer is reused for every chunk and the one-byte
/// EOF probe. No source error or source byte escapes this function.
pub(crate) fn copy_exact(
    source: &mut (impl Read + ?Sized),
    sink: &mut (impl Write + ?Sized),
    expected: StreamingFileLength,
) -> Result<(), ExactCopyError> {
    let mut buffer = [0_u8; STREAMING_BUFFER_BYTES];
    let mut remaining = expected.get();

    while remaining != 0 {
        let chunk = usize::try_from(remaining.min(STREAMING_BUFFER_BYTES as u64))
            .unwrap_or(STREAMING_BUFFER_BYTES);
        let read = loop {
            match source.read(&mut buffer[..chunk]) {
                Ok(0) => return Err(ExactCopyError::SourceTooShort),
                Ok(read) if read <= chunk => break read,
                Ok(_) => return Err(ExactCopyError::SourceRead),
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(_) => return Err(ExactCopyError::SourceRead),
            }
        };
        sink.write_all(&buffer[..read])
            .map_err(|_| ExactCopyError::SinkWrite)?;
        remaining -= u64::try_from(read).unwrap_or(u64::MAX);
    }

    loop {
        match source.read(&mut buffer[..1]) {
            Ok(0) => return Ok(()),
            Ok(1) => return Err(ExactCopyError::SourceTooLong),
            Ok(_) => return Err(ExactCopyError::SourceRead),
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => return Err(ExactCopyError::SourceRead),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Error as IoError};

    struct ErrorAfter {
        bytes: Cursor<Vec<u8>>,
        remaining_reads: usize,
    }

    impl Read for ErrorAfter {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.remaining_reads == 0 {
                return Err(IoError::other("injected source error"));
            }
            self.remaining_reads -= 1;
            self.bytes.read(buffer)
        }
    }

    struct FailingSink;

    impl Write for FailingSink {
        fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
            Err(IoError::other("injected sink error"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct OverreportingSource;

    impl Read for OverreportingSource {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            Ok(buffer.len().saturating_add(1))
        }
    }

    #[test]
    fn streaming_length_admits_zero_through_the_exact_hard_boundary() {
        assert_eq!(
            StreamingFileLength::new(0).unwrap(),
            StreamingFileLength::ZERO
        );
        assert_eq!(
            StreamingFileLength::new(MAX_STREAMING_FILE_BYTES)
                .unwrap()
                .get(),
            MAX_STREAMING_FILE_BYTES
        );
        assert_eq!(
            StreamingFileLength::new(MAX_STREAMING_FILE_BYTES + 1),
            Err(PrivateFsError::BoundExceeded)
        );
    }

    #[test]
    fn exact_copy_handles_zero_boundary_and_multiple_fixed_chunks() {
        for length in [
            0,
            STREAMING_BUFFER_BYTES,
            STREAMING_BUFFER_BYTES * 2,
            STREAMING_BUFFER_BYTES * 2 + 1,
        ] {
            let bytes = vec![0x5a; length];
            let mut source = Cursor::new(bytes.clone());
            let mut sink = Vec::new();
            copy_exact(
                &mut source,
                &mut sink,
                StreamingFileLength::new(length as u64).unwrap(),
            )
            .unwrap();
            assert_eq!(sink, bytes);
        }
    }

    #[test]
    fn exact_copy_distinguishes_short_long_source_and_sink_failures() {
        let expected = StreamingFileLength::new(4).unwrap();
        assert_eq!(
            copy_exact(&mut Cursor::new(b"abc"), &mut Vec::new(), expected),
            Err(ExactCopyError::SourceTooShort)
        );
        assert_eq!(
            copy_exact(&mut Cursor::new(b"abcde"), &mut Vec::new(), expected),
            Err(ExactCopyError::SourceTooLong)
        );
        assert_eq!(
            copy_exact(
                &mut ErrorAfter {
                    bytes: Cursor::new(b"abcd".to_vec()),
                    remaining_reads: 0,
                },
                &mut Vec::new(),
                expected,
            ),
            Err(ExactCopyError::SourceRead)
        );
        assert_eq!(
            copy_exact(&mut Cursor::new(b"abcd"), &mut FailingSink, expected),
            Err(ExactCopyError::SinkWrite)
        );
        assert_eq!(
            copy_exact(&mut OverreportingSource, &mut Vec::new(), expected),
            Err(ExactCopyError::SourceRead)
        );
    }

    #[test]
    fn eof_probe_source_error_is_not_misreported_as_exact_eof() {
        let mut source = ErrorAfter {
            bytes: Cursor::new(b"abcd".to_vec()),
            remaining_reads: 1,
        };
        assert_eq!(
            copy_exact(
                &mut source,
                &mut Vec::new(),
                StreamingFileLength::new(4).unwrap(),
            ),
            Err(ExactCopyError::SourceRead)
        );
    }
}
