//! Runs the compatibility layer's behaviour tests (`compat.test.mjs`) in Node.

use std::process::Command;

#[test]
fn compat_layer_applies_every_fix() {
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/compat.test.mjs");
    let output = match Command::new("node").args(["--test", script]).output() {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("skipping: node is not installed");
            return;
        }
        Err(error) => panic!("could not run node: {error}"),
    };
    assert!(
        output.status.success(),
        "compat layer tests failed:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}
