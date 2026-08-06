//! Single deterministic entrypoint for the workspace gate: `cargo xtask ci`.

mod adblock_provenance;
mod blocker_seed;

use std::process::{exit, Command};
use std::time::{SystemTime, UNIX_EPOCH};

const NATIVE_ADAPTERS: [(&str, Option<&str>); 3] = [
    ("vendor/wry/Cargo.toml", None),
    (
        "vendor/tauri-runtime-wry/Cargo.toml",
        Some("macos-private-api"),
    ),
    ("vendor/tauri/Cargo.toml", Some("macos-private-api,specta")),
];
const ADBLOCK_MANIFEST: &str = "vendor/adblock/Cargo.toml";
const BLOCKER_FUZZ_MANIFEST: &str = "crates/zephium-blocker/fuzz/Cargo.toml";
const BLOCKER_FEATURE_SETS: [&str; 5] = [
    "runtime",
    "runtime-exact",
    "webkit",
    "runtime,webkit",
    "runtime-exact,webkit",
];
const INTERNAL_REPOSITORY_CFG: &str = "zephium_internal_repository_e2e";
const INTERNAL_AUTHORITY_SHIPPING_REJECTION: &str =
    "the internal repository E2E authority may not link into Zephium application code";

fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    match arguments.first().map(String::as_str) {
        Some("ci") => ci(),
        Some("check-engine-floors") => check_engine_floors(),
        Some("check-release-engine-security") => check_release_engine_security(),
        Some("check-advisory-exceptions") => check_advisory_exceptions(),
        Some("check-security-fork-locks") | Some("check-native-adapter-locks") => {
            check_security_fork_locks()
        }
        Some("check-blocker-security-fork") => check_blocker_security_fork(),
        Some("check-extension-runtime-host-assembler") => {
            check_extension_runtime_host_assembler_call_sites()
        }
        Some("check-blocker-seed") if arguments.len() == 1 => {
            let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
            if let Err(error) = blocker_seed::check(&repository) {
                eprintln!("bundled blocker seed policy failed: {error}");
                exit(1);
            }
        }
        Some("materialize-blocker-seed-webkit")
            if arguments.len() == 3 && arguments[1] == "--output" =>
        {
            let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
            if let Err(error) =
                blocker_seed::materialize_webkit(&repository, std::path::Path::new(&arguments[2]))
            {
                eprintln!("bundled blocker seed materialization failed: {error}");
                exit(1);
            }
        }
        Some("update-blocker-seed") => update_blocker_seed(&arguments[1..]),
        #[cfg(any(feature = "blocker-seed-runtime", feature = "blocker-seed-webkit"))]
        Some("__compile-blocker-seed")
            if arguments.len() == 4 || (arguments.len() == 6 && arguments[4] == "--artifact") =>
        {
            if let Err(error) = blocker_seed::compile_hidden(
                &arguments[1],
                std::path::Path::new(&arguments[2]),
                std::path::Path::new(&arguments[3]),
                arguments.get(5).map(std::path::Path::new),
            ) {
                eprintln!("bundled blocker seed compilation failed: {error}");
                exit(1);
            }
        }
        // Retain the old entrypoint for local automation while making it run
        // every engine-floor deadline, not only Windows.
        Some("check-webview2-floor") => check_engine_floors(),
        _ => {
            eprintln!(
                "usage: cargo xtask <ci|check-engine-floors|check-release-engine-security|check-advisory-exceptions|check-security-fork-locks|check-native-adapter-locks|check-blocker-security-fork|check-extension-runtime-host-assembler|check-blocker-seed|materialize-blocker-seed-webkit --output PATH|update-blocker-seed --easylist PATH --easyprivacy PATH --license PATH|check-webview2-floor>"
            );
            exit(2);
        }
    }
}

fn update_blocker_seed(arguments: &[String]) {
    if arguments.len() != 6
        || arguments[0] != "--easylist"
        || arguments[2] != "--easyprivacy"
        || arguments[4] != "--license"
    {
        eprintln!(
            "usage: cargo xtask update-blocker-seed --easylist PATH --easyprivacy PATH --license PATH"
        );
        exit(2);
    }
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    if let Err(error) = blocker_seed::update(
        &repository,
        std::path::Path::new(&arguments[1]),
        std::path::Path::new(&arguments[3]),
        std::path::Path::new(&arguments[5]),
    ) {
        eprintln!("bundled blocker seed update failed: {error}");
        exit(1);
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

fn check_security_fork_locks() {
    const ROOT_NATIVE_ADAPTERS: &[(&str, &str)] = &[
        ("tauri", "2.11.3"),
        ("tauri-runtime-wry", "2.11.3"),
        ("wry", "0.55.1"),
    ];
    const ROOT_FORKS: &[(&str, &str)] = &[
        ("adblock", "0.13.2"),
        ("tauri", "2.11.3"),
        ("tauri-runtime-wry", "2.11.3"),
        ("wry", "0.55.1"),
    ];
    const ADBLOCK_FORK: &[(&str, &str)] = &[("adblock", "0.13.2")];
    const RUNTIME_ADAPTERS: &[(&str, &str)] = &[("tauri-runtime-wry", "2.11.3"), ("wry", "0.55.1")];
    const WRY_ADAPTERS: &[(&str, &str)] = &[("wry", "0.55.1")];

    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for (relative, forks) in [
        ("Cargo.lock", ROOT_FORKS),
        ("vendor/adblock/Cargo.lock", ADBLOCK_FORK),
        ("vendor/tauri/Cargo.lock", ROOT_NATIVE_ADAPTERS),
        ("vendor/tauri-runtime-wry/Cargo.lock", RUNTIME_ADAPTERS),
        ("vendor/wry/Cargo.lock", WRY_ADAPTERS),
    ] {
        let path = repository.join(relative);
        let source = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            eprintln!("cannot read {}: {error}", path.display());
            exit(1);
        });
        if let Err(error) = validate_security_fork_lock(&source, forks) {
            eprintln!("vendored security-fork lock policy failed for {relative}: {error}");
            exit(1);
        }
    }
    if let Err(error) = adblock_provenance::check(&repository) {
        eprintln!("adblock fork provenance policy failed: {error}");
        exit(1);
    }
    check_tauri_fixture_blobs(&repository);
}

fn check_tauri_fixture_blobs(repository: &std::path::Path) {
    const EXPECTED_COMMIT: &str = "6f6ab1207bb3923c2721fbc67d2fdb1c8deb0c7a";
    const EXPECTED_FILES: [(&str, &str); 5] = [
        (
            "test/fixture/src-tauri/tauri.conf.json",
            "f5b75e3eb0554e617a862d78194c02176c811fcd",
        ),
        (
            "test/fixture/dist/index.html",
            "698a3577914d350e1ebcd9279fe325563553ba24",
        ),
        (
            "test/fixture/src-tauri/icons/icon.ico",
            "b3636e4b22ba65db9061cd60a77b02c92022dfd6",
        ),
        (
            "test/fixture/src-tauri/icons/icon.ico~dev",
            "db7fd98204424b6b9b02fa06ad18f05c089f93b5",
        ),
        (
            "test/fixture/src-tauri/icons/icon.png",
            "a437dd51741e9e56e14b5d6024493cb2abfd5259",
        ),
    ];
    let fork_root = repository.join("vendor/tauri");
    let record_path = fork_root.join("TEST_FIXTURE.toml");
    let source = std::fs::read_to_string(&record_path).unwrap_or_else(|error| {
        eprintln!("cannot read {}: {error}", record_path.display());
        exit(1);
    });
    let document = source.parse::<toml::Table>().unwrap_or_else(|error| {
        eprintln!("{} is invalid TOML: {error}", record_path.display());
        exit(1);
    });
    if document
        .get("upstream_commit")
        .and_then(toml::Value::as_str)
        != Some(EXPECTED_COMMIT)
    {
        eprintln!("Tauri fixture record does not match the reviewed upstream commit");
        exit(1);
    }
    let files = document
        .get("files")
        .and_then(toml::Value::as_array)
        .filter(|files| files.len() == EXPECTED_FILES.len())
        .unwrap_or_else(|| {
            eprintln!(
                "Tauri fixture record must contain exactly {} files",
                EXPECTED_FILES.len()
            );
            exit(1);
        });
    let mut seen = std::collections::HashSet::with_capacity(EXPECTED_FILES.len());
    for file in files {
        let file = file.as_table().unwrap_or_else(|| {
            eprintln!("Tauri fixture record contains a non-table file entry");
            exit(1);
        });
        let relative = file
            .get("path")
            .and_then(toml::Value::as_str)
            .map(std::path::Path::new)
            .filter(|path| {
                !path.is_absolute()
                    && path
                        .components()
                        .all(|component| matches!(component, std::path::Component::Normal(_)))
            })
            .unwrap_or_else(|| {
                eprintln!("Tauri fixture record contains an unsafe path");
                exit(1);
            });
        let recorded = file
            .get("git_blob")
            .and_then(toml::Value::as_str)
            .filter(|hash| hash.len() == 40 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .unwrap_or_else(|| {
                eprintln!("Tauri fixture record contains an invalid Git blob identity");
                exit(1);
            });
        let relative_text = relative.to_string_lossy();
        let expected = EXPECTED_FILES
            .iter()
            .find_map(|(path, hash)| (*path == relative_text.as_ref()).then_some(*hash))
            .unwrap_or_else(|| {
                eprintln!("unexpected Tauri fixture path {relative_text}");
                exit(1);
            });
        if !seen.insert(relative_text.into_owned()) {
            eprintln!("duplicate Tauri fixture path {}", relative.display());
            exit(1);
        }
        if recorded != expected {
            eprintln!(
                "Tauri fixture record assigns {recorded} to {}, expected {expected}",
                relative.display()
            );
            exit(1);
        }
        let path = fork_root.join(relative);
        let metadata = std::fs::symlink_metadata(&path).unwrap_or_else(|error| {
            eprintln!("cannot inspect {}: {error}", path.display());
            exit(1);
        });
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            eprintln!("Tauri fixture {} is not a regular file", path.display());
            exit(1);
        }
        let output = Command::new("git")
            .arg("hash-object")
            .arg(&path)
            .output()
            .unwrap_or_else(|error| {
                eprintln!("cannot hash {}: {error}", path.display());
                exit(1);
            });
        let actual = std::str::from_utf8(&output.stdout)
            .ok()
            .map(str::trim)
            .filter(|_| output.status.success())
            .unwrap_or_else(|| {
                eprintln!("git hash-object failed for {}", path.display());
                exit(1);
            });
        if actual != expected {
            eprintln!(
                "Tauri fixture {} has Git blob {actual}, expected {expected}",
                path.display()
            );
            exit(1);
        }
    }
}

fn validate_security_fork_lock(source: &str, required: &[(&str, &str)]) -> Result<(), String> {
    let document = source
        .parse::<toml::Table>()
        .map_err(|error| format!("invalid Cargo.lock TOML: {error}"))?;
    let packages = document
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "Cargo.lock has no package array".to_owned())?;

    for &(name, expected_version) in required {
        let matching = packages
            .iter()
            .filter_map(toml::Value::as_table)
            .filter(|package| package.get("name").and_then(toml::Value::as_str) == Some(name))
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err(format!(
                "expected exactly one `{name}` package, found {}",
                matching.len()
            ));
        }
        let package = matching[0];
        let version = package
            .get("version")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| format!("`{name}` has no version"))?;
        if version != expected_version {
            return Err(format!(
                "`{name}` resolved to {version}, expected {expected_version}"
            ));
        }
        if package.contains_key("source") || package.contains_key("checksum") {
            return Err(format!(
                "`{name}` is registry/git sourced instead of the reviewed local adapter"
            ));
        }
    }
    Ok(())
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
            "WebView2 security review (hard floor {}, latest reviewed {}) expired after {}. Review {} and confirm exact runtime availability at {}, then update the versions, publication dates, and review deadline together.",
            zephium_core::webview2::SECURITY_FLOOR_TEXT,
            zephium_core::webview2::LATEST_REVIEWED_TEXT,
            zephium_core::webview2::SECURITY_FLOOR_REVIEW_BY,
            zephium_core::webview2::SECURITY_FLOOR_SOURCE_URL,
            zephium_core::webview2::RUNTIME_AVAILABILITY_SOURCE_URL,
        );
        exit(1);
    }
    eprintln!(
        "WebView2 hard floor {} and latest reviewed Stable {} are reviewed through {}",
        zephium_core::webview2::SECURITY_FLOOR_TEXT,
        zephium_core::webview2::LATEST_REVIEWED_TEXT,
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
        "macOS/WebKit hard floors Sonoma {} + Safari {}, Sequoia {} + Safari {}, and Tahoe {}; latest recommendations Sonoma {}, Sequoia {}, Tahoe {}, and Safari {}; reviewed through {}",
        zephium_core::macos::SONOMA_SECURITY_FLOOR_TEXT,
        zephium_core::macos::SAFARI_SECURITY_FLOOR_TEXT,
        zephium_core::macos::SEQUOIA_SECURITY_FLOOR_TEXT,
        zephium_core::macos::SAFARI_SECURITY_FLOOR_TEXT,
        zephium_core::macos::TAHOE_SECURITY_FLOOR_TEXT,
        zephium_core::macos::SONOMA_RECOMMENDED_TEXT,
        zephium_core::macos::SEQUOIA_RECOMMENDED_TEXT,
        zephium_core::macos::TAHOE_RECOMMENDED_TEXT,
        zephium_core::macos::SAFARI_RECOMMENDED_TEXT,
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

/// Release-only publication gate. Runtime admission uses the best stable
/// engine that actually exists; publishing additionally requires that no
/// vendor has acknowledged an outstanding stable-channel security fix and
/// that the immutable blocker sources are still within their upstream
/// recommended refresh cadence at the exact publication boundary.
fn check_release_engine_security() {
    check_engine_floors();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|error| {
            eprintln!("release engine-security check cannot read UTC time: {error}");
            exit(1);
        })
        .as_secs();
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    if let Err(error) = blocker_seed::check_release_freshness(&repository, now) {
        eprintln!("production release is blocked by bundled blocker seed freshness: {error}");
        exit(1);
    }
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
    reject_ambient_internal_repository_cfg();
    share_workspace_target_dir();
    check_extension_runtime_host_assembler_call_sites();
    check_engine_floors();
    check_advisory_exceptions();
    check_blocker_security_fork();
    run("cargo", &["fmt", "--all", "--", "--check"]);
    for (manifest, _) in NATIVE_ADAPTERS {
        run(
            "cargo",
            &["fmt", "--manifest-path", manifest, "--", "--check"],
        );
    }
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
    for (manifest, features) in NATIVE_ADAPTERS {
        run_native_adapter_clippy(manifest, features, "--all-targets");
        run_native_adapter_clippy(manifest, features, "--lib");
    }
    // The desktop test suite regenerates frame/src/shared/ipc/bindings.ts, so the
    // frontend typecheck after it doubles as a Rust/TS drift check.
    run("cargo", &["test", "--workspace"]);
    run_internal_extension_repository_gates();
    for (manifest, features) in NATIVE_ADAPTERS {
        run_native_adapter_tests(manifest, features);
    }
    #[cfg(target_os = "macos")]
    run_macos_principal_isolation_probe();
    #[cfg(target_os = "macos")]
    run_macos_web_extension_probe();
    run("pnpm", &["--dir", "frame", "run", "check"]);
}

fn check_extension_runtime_host_assembler_call_sites() {
    const NEEDLE: &str = "try_from_authenticated_repository(";
    const REPOSITORY_BRIDGE: &str =
        "crates/zephium-extension-repository/src/package_lease/runtime_access.rs";
    const EXPECTED_REPOSITORY_CALLS: usize = 2;

    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut sources = Vec::new();
    collect_rust_sources(&repository.join("crates"), &mut sources);
    sources.sort();

    let mut repository_calls = 0;
    let mut forbidden = Vec::new();
    for path in sources {
        let source = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            eprintln!("cannot read {}: {error}", path.display());
            exit(1);
        });
        let calls = source.matches(NEEDLE).count();
        if calls == 0 {
            continue;
        }
        let relative = path
            .strip_prefix(&repository)
            .unwrap_or(&path)
            .to_string_lossy();
        if relative == REPOSITORY_BRIDGE {
            repository_calls = calls;
        } else if relative == "crates/zephium-extension-runtime-api/src/host.rs" && calls == 1 {
            // The sole constructor definition. Calls remain repository-only.
        } else if relative != "crates/zephium-extension-runtime-api/src/host_tests.rs" {
            forbidden.push((relative.into_owned(), calls));
        }
    }

    if repository_calls != EXPECTED_REPOSITORY_CALLS || !forbidden.is_empty() {
        eprintln!(
            "extension host activation assembly must remain behind the authenticated repository bridge"
        );
        eprintln!(
            "{REPOSITORY_BRIDGE} contains {repository_calls} constructor calls, expected {EXPECTED_REPOSITORY_CALLS}"
        );
        for (path, calls) in forbidden {
            eprintln!("forbidden host activation constructor use: {path} ({calls} calls)");
        }
        exit(1);
    }
}

fn collect_rust_sources(directory: &std::path::Path, output: &mut Vec<std::path::PathBuf>) {
    let entries = std::fs::read_dir(directory).unwrap_or_else(|error| {
        eprintln!("cannot enumerate {}: {error}", directory.display());
        exit(1);
    });
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| {
            eprintln!("cannot enumerate {}: {error}", directory.display());
            exit(1);
        });
        let file_type = entry.file_type().unwrap_or_else(|error| {
            eprintln!("cannot inspect {}: {error}", entry.path().display());
            exit(1);
        });
        if file_type.is_dir() {
            collect_rust_sources(&entry.path(), output);
        } else if file_type.is_file() && entry.path().extension().is_some_and(|value| value == "rs")
        {
            output.push(entry.path());
        }
    }
}

fn run_internal_extension_repository_gates() {
    verify_internal_authority_cannot_link_into_shipping_code();
    for target in ["--all-targets", "--lib"] {
        run_with_internal_repository_cfg(&[
            "clippy",
            "--locked",
            "-p",
            "zephium-extension-authority",
            "-p",
            "zephium-extension-repository",
            target,
            "--",
            "-D",
            "warnings",
        ]);
    }
    run_with_internal_repository_cfg(&[
        "test",
        "--locked",
        "-p",
        "zephium-extension-authority",
        "--lib",
    ]);
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    run_internal_repository_e2e_tests();
    #[cfg(target_os = "windows")]
    eprintln!(
        "internal repository writer E2E is unavailable on Windows until the private namespace primitive is implemented; custom authority lint/tests remain mandatory"
    );
}

fn run_with_internal_repository_cfg(args: &[&str]) {
    eprintln!("> [internal repository authority] cargo {}", args.join(" "));
    let status = internal_repository_command(args)
        .status()
        .unwrap_or_else(|error| panic!("failed to spawn cargo: {error}"));
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }
}

fn internal_repository_command(args: &[&str]) -> Command {
    let mut command = Command::new("cargo");
    command.args(args);
    if let Some(mut encoded) = std::env::var_os("CARGO_ENCODED_RUSTFLAGS") {
        if !encoded.is_empty() {
            encoded.push("\u{1f}");
        }
        encoded.push("--cfg\u{1f}");
        encoded.push(INTERNAL_REPOSITORY_CFG);
        command.env("CARGO_ENCODED_RUSTFLAGS", encoded);
    } else {
        let mut flags = std::env::var_os("RUSTFLAGS").unwrap_or_default();
        if !flags.is_empty() {
            flags.push(" ");
        }
        flags.push("--cfg ");
        flags.push(INTERNAL_REPOSITORY_CFG);
        command.env("RUSTFLAGS", flags);
    }
    command
}

fn reject_ambient_internal_repository_cfg() {
    let plain = std::env::var_os("RUSTFLAGS")
        .map(|value| value.to_string_lossy().into_owned())
        .is_some_and(|value| rustflags_enable_internal_repository_cfg(value.split_whitespace()));
    let encoded = std::env::var_os("CARGO_ENCODED_RUSTFLAGS")
        .map(|value| value.to_string_lossy().into_owned())
        .is_some_and(|value| rustflags_enable_internal_repository_cfg(value.split('\u{1f}')));
    if plain || encoded {
        eprintln!(
            "cargo xtask ci refuses an ambient internal repository E2E authority; remove the custom cfg from compiler flags"
        );
        exit(2);
    }
}

fn rustflags_enable_internal_repository_cfg<'flag>(
    flags: impl IntoIterator<Item = &'flag str>,
) -> bool {
    let mut expects_cfg_value = false;
    for raw in flags {
        let flag = raw.trim_matches(['\'', '"']);
        if expects_cfg_value {
            if flag == INTERNAL_REPOSITORY_CFG {
                return true;
            }
            expects_cfg_value = false;
        }
        if flag == "--cfg" {
            expects_cfg_value = true;
        } else if flag.strip_prefix("--cfg=") == Some(INTERNAL_REPOSITORY_CFG) {
            return true;
        }
    }
    false
}

fn verify_internal_authority_cannot_link_into_shipping_code() {
    let output = internal_repository_command(&["check", "--locked", "-p", "zephium-app", "--lib"])
        .output()
        .unwrap_or_else(|error| panic!("failed to spawn negative authority-link gate: {error}"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() || !stderr.contains(INTERNAL_AUTHORITY_SHIPPING_REJECTION) {
        eprintln!(
            "internal repository authority shipping rejection did not fail for the expected reason"
        );
        eprintln!("{}", String::from_utf8_lossy(&output.stdout));
        eprintln!("{stderr}");
        exit(output.status.code().filter(|code| *code != 0).unwrap_or(1));
    }
    eprintln!("> internal repository authority is compile-time rejected by shipping code");
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn run_internal_repository_e2e_tests() {
    // Prefix selections cover modules that exist only under the internal
    // authority cfg. Exact selections cover cfg-only cases embedded in modules
    // whose regular-build tests already ran in the workspace suite.
    const TEST_SELECTIONS: [(&str, usize, bool); 8] = [
        ("writer::repository_e2e_tests::", 22, false),
        ("package_lease::repository_e2e::", 14, false),
        ("garbage_collection::tests::", 10, false),
        ("materialization::measurement::tests::", 3, false),
        (
            "admission::tests::production_seeded_catalog_can_be_forgotten_and_reseeded_with_a_new_identity",
            1,
            true,
        ),
        (
            "admission::tests::catalog_inventory_revalidates_bytes_without_reparsing_catalogs",
            1,
            true,
        ),
        (
            "admission::tests::active_catalog_is_structurally_parsed_once_during_open",
            1,
            true,
        ),
        (
            "catalog_cache::tests::nonforgeable_seed_paths_and_authority_charge_clear_together",
            1,
            true,
        ),
    ];
    let regular_inventory = list_extension_repository_tests(false);
    let internal_inventory = list_extension_repository_tests(true);
    let internal_only = internal_inventory
        .difference(&regular_inventory)
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let mut selected = std::collections::BTreeSet::new();
    let mut commands = Vec::with_capacity(TEST_SELECTIONS.len());
    for (filter, expected, exact) in TEST_SELECTIONS {
        let matches = internal_inventory
            .iter()
            .filter(|name| {
                if exact {
                    name.as_str() == filter
                } else {
                    name.starts_with(filter)
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        if matches.len() != expected {
            eprintln!(
                "internal repository E2E selection {filter} contains {} tests, expected {expected}",
                matches.len()
            );
            exit(1);
        }
        for name in matches {
            if !selected.insert(name.clone()) {
                eprintln!("internal repository E2E test is selected more than once: {name}");
                exit(1);
            }
        }

        let mut base = vec![
            "test",
            "--locked",
            "-p",
            "zephium-extension-repository",
            "--lib",
            filter,
            "--",
        ];
        if exact {
            base.push("--exact");
        }
        // APFS durability cases intentionally quarantine ambiguous concurrent
        // settlements. Serialize this crash matrix so the gate is deterministic.
        base.push("--test-threads=1");
        commands.push(base);
    }
    if selected != internal_only {
        eprintln!("internal repository E2E selections must cover every cfg-only test exactly once");
        for name in internal_only.difference(&selected) {
            eprintln!("unselected internal-only test: {name}");
        }
        for name in selected.difference(&internal_only) {
            eprintln!("redundant regular-build test selection: {name}");
        }
        exit(1);
    }
    for command in commands {
        run_with_internal_repository_cfg(&command);
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn list_extension_repository_tests(internal: bool) -> std::collections::BTreeSet<String> {
    let args = [
        "test",
        "--locked",
        "-p",
        "zephium-extension-repository",
        "--lib",
        "--",
        "--list",
    ];
    let output = if internal {
        internal_repository_command(&args).output()
    } else {
        Command::new("cargo").args(args).output()
    }
    .unwrap_or_else(|error| panic!("failed to list extension repository tests: {error}"));
    if !output.status.success() {
        eprintln!("extension repository test inventory failed to compile");
        eprintln!("{}", String::from_utf8_lossy(&output.stdout));
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        exit(output.status.code().unwrap_or(1));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .map(str::to_owned)
        .collect()
}

#[cfg(target_os = "macos")]
fn run_macos_principal_isolation_probe() {
    const COMMON: [&str; 7] = [
        "--locked",
        "-p",
        "zephium-engine",
        "--features",
        "native-isolation-probes",
        "--bin",
        "macos-principal-isolation-probe",
    ];

    let mut clippy = vec!["clippy"];
    clippy.extend(COMMON);
    clippy.extend(["--", "-D", "warnings"]);
    run("cargo", &clippy);

    let mut execute = vec!["run"];
    execute.extend(COMMON);
    run("cargo", &execute);
}

#[cfg(target_os = "macos")]
fn run_macos_web_extension_probe() {
    const COMMON: [&str; 7] = [
        "--locked",
        "-p",
        "zephium-engine",
        "--features",
        "native-web-extension-probes",
        "--bin",
        "macos-web-extension-probe",
    ];

    let mut clippy = vec!["clippy"];
    clippy.extend(COMMON);
    clippy.extend(["--", "-D", "warnings"]);
    run("cargo", &clippy);

    let mut execute = vec!["run"];
    execute.extend(COMMON);
    run("cargo", &execute);
}

fn check_blocker_security_fork() {
    share_workspace_target_dir();
    check_security_fork_locks();
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    if let Err(error) = blocker_seed::check(&repository) {
        eprintln!("bundled blocker seed policy failed: {error}");
        exit(1);
    }
    for package in [
        "zephium-blocker",
        "zephium-blocker-service",
        "zephium-blocker-update",
    ] {
        run("cargo", &["fmt", "--package", package, "--", "--check"]);
    }
    run(
        "cargo",
        &["fmt", "--manifest-path", ADBLOCK_MANIFEST, "--", "--check"],
    );
    run(
        "cargo",
        &[
            "fmt",
            "--manifest-path",
            BLOCKER_FUZZ_MANIFEST,
            "--",
            "--check",
        ],
    );
    run_blocker_feature_gates();
    run_blocker_product_gates();
    check_blocker_dependency_graphs();
    run_adblock_fork_gates();
}

fn share_workspace_target_dir() {
    // Excluded fork manifests otherwise create independent multi-gigabyte
    // target trees. Preserve an explicit caller override, but make local gates
    // share Cargo's fingerprinted workspace output.
    if std::env::var_os("CARGO_TARGET_DIR").is_none() {
        let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target");
        std::env::set_var("CARGO_TARGET_DIR", target);
    }
}

fn run_blocker_feature_gates() {
    for features in BLOCKER_FEATURE_SETS {
        run(
            "cargo",
            &[
                "check",
                "-p",
                "zephium-blocker",
                "--lib",
                "--locked",
                "--no-default-features",
                "--features",
                features,
            ],
        );
        run(
            "cargo",
            &[
                "clippy",
                "-p",
                "zephium-blocker",
                "--lib",
                "--tests",
                "--locked",
                "--no-default-features",
                "--features",
                features,
                "--",
                "-D",
                "warnings",
            ],
        );
        run(
            "cargo",
            &[
                "test",
                "-p",
                "zephium-blocker",
                "--lib",
                "--locked",
                "--no-default-features",
                "--features",
                features,
            ],
        );
    }
}

fn run_blocker_product_gates() {
    for (package, features) in [
        ("zephium-blocker-update", None),
        ("zephium-blocker-update", Some("tuf")),
        ("zephium-blocker-service", None),
        ("zephium-blocker-service", Some("tuf")),
    ] {
        let mut common = vec![
            "-p",
            package,
            "--all-targets",
            "--locked",
            "--no-default-features",
        ];
        if let Some(features) = features {
            common.extend(["--features", features]);
        }

        let mut check = vec!["check"];
        check.extend(common.iter().copied());
        run("cargo", &check);

        let mut clippy = vec!["clippy"];
        clippy.extend(common.iter().copied());
        clippy.extend(["--", "-D", "warnings"]);
        run("cargo", &clippy);

        let mut test = vec!["test"];
        test.extend(common.iter().copied());
        run("cargo", &test);
    }
    run(
        "cargo",
        &[
            "test",
            "-p",
            "zephium-blocker",
            "--test",
            "synthetic_quality",
            "--locked",
        ],
    );
    run(
        "cargo",
        &[
            "check",
            "--manifest-path",
            BLOCKER_FUZZ_MANIFEST,
            "--locked",
            "--bins",
        ],
    );
    run(
        "cargo",
        &[
            "clippy",
            "--manifest-path",
            BLOCKER_FUZZ_MANIFEST,
            "--locked",
            "--bins",
            "--",
            "-D",
            "warnings",
        ],
    );
    run(
        "cargo",
        &[
            "run",
            "-p",
            "zephium-blocker",
            "--example",
            "synthetic_blocker_lab",
            "--locked",
            "--",
            "--rules",
            "256",
            "--requests",
            "1024",
            "--target",
            "all",
        ],
    );
}

fn check_blocker_dependency_graphs() {
    let bundled = cargo_tree(&[
        "-p",
        "zephium-desktop",
        "--no-default-features",
        "--locked",
        "-e",
        "features",
        "--prefix",
        "none",
    ]);
    for forbidden in [
        "zephium-blocker-update feature \"tuf\"",
        "tough v",
        "reqwest v",
        "rustls-platform-verifier v",
        "aws-lc-rs v",
    ] {
        if bundled.lines().any(|line| line.starts_with(forbidden)) {
            eprintln!("bundled desktop dependency graph unexpectedly contains `{forbidden}`");
            exit(1);
        }
    }
    if !bundled
        .lines()
        .any(|line| line.starts_with("zephium-blocker-update v"))
    {
        eprintln!("bundled desktop graph lost canonical blocker package validation");
        exit(1);
    }

    let tuf = cargo_tree(&[
        "-p",
        "zephium-blocker-update",
        "--no-default-features",
        "--features",
        "tuf",
        "--locked",
        "-e",
        "features",
        "--prefix",
        "none",
    ]);
    for required in [
        "zephium-blocker-update v",
        "tough v",
        "reqwest v",
        "rustls-platform-verifier v",
        "aws-lc-rs v",
    ] {
        if !tuf.lines().any(|line| line.starts_with(required)) {
            eprintln!("TUF verification graph is missing `{required}`");
            exit(1);
        }
    }
}

fn cargo_tree(arguments: &[&str]) -> String {
    let output = Command::new("cargo")
        .arg("tree")
        .args(arguments)
        .output()
        .unwrap_or_else(|error| {
            eprintln!("failed to execute cargo tree: {error}");
            exit(1);
        });
    if !output.status.success() {
        eprintln!(
            "cargo tree failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        exit(1);
    }
    String::from_utf8(output.stdout).unwrap_or_else(|error| {
        eprintln!("cargo tree emitted non-UTF-8 output: {error}");
        exit(1);
    })
}

fn run_adblock_fork_gates() {
    for features in adblock_provenance::SHIPPING_FEATURE_SETS {
        run(
            "cargo",
            &[
                "check",
                "--manifest-path",
                ADBLOCK_MANIFEST,
                "--lib",
                "--locked",
                "--no-default-features",
                "--features",
                features,
            ],
        );
        run(
            "cargo",
            &[
                "clippy",
                "--manifest-path",
                ADBLOCK_MANIFEST,
                "--lib",
                "--locked",
                "--no-default-features",
                "--features",
                features,
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
                ADBLOCK_MANIFEST,
                "--lib",
                "--tests",
                "--locked",
                "--no-default-features",
                "--features",
                features,
                "--",
                "-D",
                "warnings",
            ],
        );
        run(
            "cargo",
            &[
                "test",
                "--manifest-path",
                ADBLOCK_MANIFEST,
                "--lib",
                "--test",
                "fork_contract",
                "--locked",
                "--no-default-features",
                "--features",
                features,
            ],
        );
    }
    let exact = adblock_provenance::OPTIONAL_EXACT_FEATURES;
    run(
        "cargo",
        &[
            "check",
            "--manifest-path",
            ADBLOCK_MANIFEST,
            "--lib",
            "--locked",
            "--no-default-features",
            "--features",
            exact,
        ],
    );
    run(
        "cargo",
        &[
            "clippy",
            "--manifest-path",
            ADBLOCK_MANIFEST,
            "--lib",
            "--tests",
            "--locked",
            "--no-default-features",
            "--features",
            exact,
            "--",
            "-D",
            "warnings",
        ],
    );
    run(
        "cargo",
        &[
            "test",
            "--manifest-path",
            ADBLOCK_MANIFEST,
            "--lib",
            "--test",
            "fork_contract",
            "--locked",
            "--no-default-features",
            "--features",
            exact,
        ],
    );
    // Retain an upstream-default compatibility run, but never let its
    // single-thread/embedded-resolver graph substitute for the exact shipped
    // graph tests above.
    run(
        "cargo",
        &[
            "test",
            "--manifest-path",
            ADBLOCK_MANIFEST,
            "--lib",
            "--test",
            "fork_contract",
            "--locked",
        ],
    );
}

fn run_native_adapter_clippy(manifest: &str, features: Option<&str>, target: &str) {
    let mut args = vec!["clippy", "--manifest-path", manifest, target, "--locked"];
    if let Some(features) = features {
        args.extend(["--features", features]);
    }
    args.extend(["--", "-D", "warnings"]);
    run("cargo", &args);
}

fn run_native_adapter_tests(manifest: &str, features: Option<&str>) {
    let mut args = vec![
        "test",
        "--manifest-path",
        manifest,
        "--all-targets",
        "--locked",
    ];
    if let Some(features) = features {
        args.extend(["--features", features]);
    }
    run("cargo", &args);
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
    use super::{
        rustflags_enable_internal_repository_cfg, validate_advisory_exceptions,
        validate_security_fork_lock,
    };

    #[test]
    fn internal_repository_cfg_detection_is_exact_across_rustflag_encodings() {
        assert!(rustflags_enable_internal_repository_cfg([
            "-C",
            "debuginfo=2",
            "--cfg",
            "zephium_internal_repository_e2e",
        ]));
        assert!(rustflags_enable_internal_repository_cfg([
            "--cfg=zephium_internal_repository_e2e"
        ]));
        assert!(rustflags_enable_internal_repository_cfg([
            "--cfg",
            "'zephium_internal_repository_e2e'",
        ]));
        assert!(!rustflags_enable_internal_repository_cfg([
            "--check-cfg=cfg(zephium_internal_repository_e2e)",
            "--cfg",
            "another_cfg",
        ]));
    }

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
    fn security_fork_locks_require_exact_local_packages() {
        let local = r#"
version = 4

[[package]]
name = "tauri"
version = "2.11.3"

[[package]]
name = "tauri-runtime-wry"
version = "2.11.3"
"#;
        let required = &[("tauri", "2.11.3"), ("tauri-runtime-wry", "2.11.3")];
        assert!(validate_security_fork_lock(local, required).is_ok());

        let registry = format!(
            "{local}\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n"
        );
        assert!(validate_security_fork_lock(&registry, required)
            .unwrap_err()
            .contains("registry/git sourced"));

        let duplicate = format!("{local}\n[[package]]\nname = \"tauri\"\nversion = \"2.11.3\"\n");
        assert!(validate_security_fork_lock(&duplicate, required)
            .unwrap_err()
            .contains("exactly one"));
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
