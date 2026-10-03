//! Provider-free cookie persistence qualifier for disposable Windows SDK profiles.
#![forbid(unsafe_code)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]

#[cfg(not(debug_assertions))]
compile_error!("Windows cookie persistence qualification is forbidden in optimized builds");

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    use std::io::Write as _;
    let mut arguments = std::env::args_os().skip(1);
    let first = arguments.next();
    if arguments.next().is_some()
        || first
            .as_deref()
            .is_some_and(|flag| flag != std::ffi::OsStr::new("--verbatim-udf"))
    {
        let _ = writeln!(
            std::io::stderr().lock(),
            "windows-cookie-control: invalid_arguments; content=redacted"
        );
        return std::process::ExitCode::FAILURE;
    }
    match zephium_engine::run_windows_cookie_persistence_control() {
        Ok(()) => {
            let _ = writeln!(
                std::io::stderr().lock(),
                "windows-cookie-control: passed; content=synthetic"
            );
            std::process::ExitCode::SUCCESS
        }
        Err(reason) => {
            let _ = writeln!(
                std::io::stderr().lock(),
                "windows-cookie-control: failure={reason}; content=redacted"
            );
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn main() -> std::process::ExitCode {
    std::process::ExitCode::FAILURE
}
