//! Best-effort redacted diagnostics for native-engine failure boundaries.

use std::io::Write;

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

    struct RefusingWriter;

    impl Write for RefusingWriter {
        fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("injected refusal"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("injected refusal"))
        }
    }

    #[test]
    fn diagnostics_cannot_escape_native_control_flow() {
        write_to(&mut RefusingWriter, format_args!("typed failure"));
    }
}
