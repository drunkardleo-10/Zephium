//! Single deterministic entrypoint for the workspace gate: `cargo xtask ci`.

use std::process::{exit, Command};

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("ci") => ci(),
        _ => {
            eprintln!("usage: cargo xtask ci");
            exit(2);
        }
    }
}

fn ci() {
    run("cargo", &["fmt", "--all", "--", "--check"]);
    run("cargo", &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]);
    run("cargo", &["test", "--workspace"]);
}

fn run(cmd: &str, args: &[&str]) {
    eprintln!("> {cmd} {}", args.join(" "));
    let status = Command::new(cmd)
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("failed to spawn {cmd}: {e}"));
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }
}
