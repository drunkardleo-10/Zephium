//! Single deterministic entrypoint for the workspace gate: `cargo xtask ci`.

use std::process::{exit, Command};
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("ci") => ci(),
        Some("check-engine-floors") => check_engine_floors(),
        Some("check-release-engine-security") => check_release_engine_security(),
        Some("check-advisory-exceptions") => check_advisory_exceptions(),
        // Retain the old entrypoint for local automation while making it run
        // every engine-floor deadline, not only Windows.
        Some("check-webview2-floor") => check_engine_floors(),
        _ => {
            eprintln!(
                "usage: cargo xtask <ci|check-engine-floors|check-release-engine-security|check-advisory-exceptions|check-webview2-floor>"
            );
            exit(2);
        }
    }
}

fn check_advisory_exceptions() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|error| {
            eprintln!("advisory-exception check cannot read UTC time: {error}");
            exit(1);
        })
        .as_secs();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../deny.toml");
    let source = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        eprintln!("cannot read {}: {error}", path.display());
        exit(1);
    });
    if let Err(error) = validate_advisory_exceptions(&source, now) {
        eprintln!("cargo-deny advisory exception policy failed: {error}");
        exit(1);
    }
}

fn validate_advisory_exceptions(source: &str, now: u64) -> Result<(), String> {
    const MAX_EXCEPTION_LIFETIME: u64 = 120 * 24 * 60 * 60;
    let document = source
        .parse::<toml::Table>()
        .map_err(|error| format!("deny.toml is not valid TOML: {error}"))?;
    let ignore = document
        .get("advisories")
        .and_then(toml::Value::as_table)
        .and_then(|advisories| advisories.get("ignore"))
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "deny.toml advisories.ignore must be an array".to_owned())?;
    if ignore.is_empty() {
        return Err("deny.toml contains no advisory exception entries".into());
    }

    let mut ids = std::collections::HashSet::with_capacity(ignore.len());
    for (index, exception) in ignore.iter().enumerate() {
        let entry = index + 1;
        let exception = exception.as_table().ok_or_else(|| {
            format!(
                "advisory exception {entry} must be a table with id and reason; bare-string ignores are forbidden"
            )
        })?;
        let id = exception
            .get("id")
            .and_then(toml::Value::as_str)
            .filter(|id| valid_rustsec_id(id))
            .ok_or_else(|| format!("advisory exception {entry} has no valid RUSTSEC id"))?;
        if !ids.insert(id) {
            return Err(format!("advisory exception {entry} duplicates {id}"));
        }
        let reason = exception
            .get("reason")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| format!("advisory exception {entry} has no machine-readable reason"))?;
        let owner = reason
            .split(';')
            .find_map(|part| part.trim().strip_prefix("owner="))
            .filter(|owner| !owner.is_empty())
            .ok_or_else(|| format!("advisory exception {entry} has no owner"))?;
        let expiry = reason
            .split(';')
            .find_map(|part| part.trim().strip_prefix("expires-unix="))
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| format!("advisory exception {entry} has no valid expires-unix"))?;
        let human_expiry = reason
            .split(';')
            .find_map(|part| part.trim().strip_prefix("expires="))
            .filter(|value| valid_iso_date(value))
            .ok_or_else(|| format!("advisory exception {entry} has no valid ISO expiry"))?;
        if expiry <= now {
            return Err(format!(
                "advisory exception {entry} owned by {owner} expired at {human_expiry} ({expiry})"
            ));
        }
        if expiry.saturating_sub(now) > MAX_EXCEPTION_LIFETIME {
            return Err(format!(
                "advisory exception {entry} owned by {owner} exceeds the 120-day review horizon"
            ));
        }
    }
    Ok(())
}

fn valid_rustsec_id(value: &str) -> bool {
    let Some((year, sequence)) = value
        .strip_prefix("RUSTSEC-")
        .and_then(|tail| tail.split_once('-'))
    else {
        return false;
    };
    year.len() == 4
        && sequence.len() == 4
        && year.bytes().all(|byte| byte.is_ascii_digit())
        && sequence.bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_iso_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

fn check_engine_floors() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|error| {
            eprintln!("engine security-floor check cannot read UTC time: {error}");
            exit(1);
        })
        .as_secs();
    if !zephium_core::webview2::security_floor_review_is_current(now) {
        eprintln!(
            "WebView2 security floor {} expired after {}. Review {} and update the version, publication date, and review deadline together.",
            zephium_core::webview2::SECURITY_FLOOR_TEXT,
            zephium_core::webview2::SECURITY_FLOOR_REVIEW_BY,
            zephium_core::webview2::SECURITY_FLOOR_SOURCE_URL,
        );
        exit(1);
    }
    eprintln!(
        "WebView2 security floor {} is reviewed through {}",
        zephium_core::webview2::SECURITY_FLOOR_TEXT,
        zephium_core::webview2::SECURITY_FLOOR_REVIEW_BY,
    );

    if !zephium_core::macos::security_floor_review_is_current(now) {
        eprintln!(
            "macOS/WebKit security floors expired after {}. Review {}, {}, and {} and update the OS/Safari versions, publication date, and review deadline together.",
            zephium_core::macos::SECURITY_FLOOR_REVIEW_BY,
            zephium_core::macos::SECURITY_FLOOR_SOURCE_URL,
            zephium_core::macos::SAFARI_SECURITY_SOURCE_URL,
            zephium_core::macos::TAHOE_SECURITY_SOURCE_URL,
        );
        exit(1);
    }
    eprintln!(
        "macOS/WebKit floors Sonoma {} + Safari {}, Sequoia {} + Safari {}, and Tahoe {} are reviewed through {}",
        zephium_core::macos::SONOMA_SECURITY_FLOOR_TEXT,
        zephium_core::macos::SAFARI_SECURITY_FLOOR_TEXT,
        zephium_core::macos::SEQUOIA_SECURITY_FLOOR_TEXT,
        zephium_core::macos::SAFARI_SECURITY_FLOOR_TEXT,
        zephium_core::macos::TAHOE_SECURITY_FLOOR_TEXT,
        zephium_core::macos::SECURITY_FLOOR_REVIEW_BY,
    );

    if !zephium_core::webkitgtk::security_floor_review_is_current(now) {
        eprintln!(
            "WebKitGTK security floor {} expired after {}. Review {} and {} and update the advisory floor, latest-reviewed release, and deadline together.",
            zephium_core::webkitgtk::SECURITY_FLOOR_TEXT,
            zephium_core::webkitgtk::SECURITY_FLOOR_REVIEW_BY,
            zephium_core::webkitgtk::SECURITY_FLOOR_SOURCE_URL,
            zephium_core::webkitgtk::LATEST_REVIEWED_SOURCE_URL,
        );
        exit(1);
    }
    eprintln!(
        "WebKitGTK security floor {} (latest reviewed {}) is reviewed through {}",
        zephium_core::webkitgtk::SECURITY_FLOOR_TEXT,
        zephium_core::webkitgtk::LATEST_REVIEWED_TEXT,
        zephium_core::webkitgtk::SECURITY_FLOOR_REVIEW_BY,
    );
}

/// Release-only native-engine gate. Runtime admission uses the best stable
/// engine that actually exists; publishing additionally requires that no
/// vendor has acknowledged an outstanding stable-channel security fix.
fn check_release_engine_security() {
    check_engine_floors();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|error| {
            eprintln!("release engine-security check cannot read UTC time: {error}");
            exit(1);
        })
        .as_secs();
    if !zephium_core::webview2::production_release_security_is_current(now) {
        eprintln!(
            "production release is blocked: Microsoft acknowledged an outstanding Chromium security fix on {} (status reviewed {}); review {} and publish only after a fixed Stable WebView2 runtime is available and the floor is updated",
            zephium_core::webview2::OUTSTANDING_VENDOR_FIX_NOTICE_ON,
            zephium_core::webview2::OUTSTANDING_VENDOR_FIX_REVIEWED_ON,
            zephium_core::webview2::OUTSTANDING_VENDOR_FIX_SOURCE_URL,
        );
        exit(1);
    }
}

fn ci() {
    check_engine_floors();
    check_advisory_exceptions();
    run("cargo", &["fmt", "--all", "--", "--check"]);
    run(
        "cargo",
        &[
            "fmt",
            "--manifest-path",
            "vendor/wry/Cargo.toml",
            "--",
            "--check",
        ],
    );
    run(
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    );
    // `--all-targets` enables test-only references while linting library
    // artifacts, which can hide dead production paths behind cfg(test).
    run(
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--lib",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    );
    run(
        "cargo",
        &[
            "clippy",
            "--manifest-path",
            "vendor/wry/Cargo.toml",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    );
    run(
        "cargo",
        &[
            "clippy",
            "--manifest-path",
            "vendor/wry/Cargo.toml",
            "--lib",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    );
    // The desktop test suite regenerates frame/src/ipc/bindings.ts, so the
    // frontend typecheck after it doubles as a Rust/TS drift check.
    run("cargo", &["test", "--workspace"]);
    run(
        "cargo",
        &[
            "test",
            "--manifest-path",
            "vendor/wry/Cargo.toml",
            "--all-targets",
            "--locked",
        ],
    );
    run("pnpm", &["--dir", "frame", "run", "check"]);
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

#[cfg(test)]
mod tests {
    use super::validate_advisory_exceptions;

    #[test]
    fn advisory_exceptions_require_owner_and_short_live_expiry() {
        let now = 1_000_000;
        let valid = r#"[advisories]
ignore = [
  { id = "RUSTSEC-2026-0001", reason = "owner=security; expires=2026-01-01; expires-unix=1000100; tracked" },
]"#;
        assert!(validate_advisory_exceptions(valid, now).is_ok());
        assert!(validate_advisory_exceptions(
            r#"[advisories]
ignore = [{ id = "RUSTSEC-2026-0001", reason = "expires=2026-01-01; expires-unix=1000100; tracked" }]"#,
            now,
        )
        .unwrap_err()
        .contains("owner"));
        assert!(validate_advisory_exceptions(
            r#"[advisories]
ignore = [{ id = "RUSTSEC-2026-0001", reason = "owner=security; expires=2026-01-01; expires-unix=999999; tracked" }]"#,
            now,
        )
        .unwrap_err()
        .contains("expired"));
    }

    #[test]
    fn advisory_exception_policy_is_toml_structural_not_format_sensitive() {
        let now = 1_000_000;
        let multiline = r#"
[advisories]
ignore = [
  {
    id = "RUSTSEC-2026-0001",
    reason = "owner=security; expires=2026-01-01; expires-unix=1000100; tracked",
  },
]
"#;
        assert!(validate_advisory_exceptions(multiline, now).is_ok());

        let bare = r#"[advisories]
ignore = ["RUSTSEC-2026-0001"]"#;
        assert!(validate_advisory_exceptions(bare, now)
            .unwrap_err()
            .contains("bare-string"));

        let duplicate = r#"[advisories]
ignore = [
  { id = "RUSTSEC-2026-0001", reason = "owner=security; expires=2026-01-01; expires-unix=1000100; tracked" },
  { id = "RUSTSEC-2026-0001", reason = "owner=security; expires=2026-01-01; expires-unix=1000100; tracked" },
]"#;
        assert!(validate_advisory_exceptions(duplicate, now)
            .unwrap_err()
            .contains("duplicates"));
    }
}
