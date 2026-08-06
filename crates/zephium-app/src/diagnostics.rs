//! Best-effort diagnostics for actor and lifecycle failure paths.

use std::io::Write;

/// Writes one diagnostic without allowing a closed or failing stderr stream to
/// alter actor, shutdown, or panic-unwind control flow.
pub(crate) fn write(arguments: std::fmt::Arguments<'_>) {
    let stderr = std::io::stderr();
    let mut stderr = stderr.lock();
    write_to(&mut stderr, arguments);
}

fn write_to(writer: &mut dyn Write, arguments: std::fmt::Arguments<'_>) {
    let _ = writer.write_fmt(arguments);
    let _ = writer.write_all(b"\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("injected diagnostic failure"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("injected diagnostic failure"))
        }
    }

    #[test]
    fn failed_diagnostic_sink_cannot_escape_into_lifecycle_control_flow() {
        write_to(
            &mut FailingWriter,
            format_args!("terminal diagnostic {}", 17),
        );
    }
}
