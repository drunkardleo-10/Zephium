use std::path::Path;

use zephium_webext::archive::{self, Limits};
use zephium_webext::crx;
use zephium_webext::manifest::Manifest;
use zephium_webext::prepare::{prepare, CompatLayer};

/// Verifies, extracts and prepares every package in `ZEPHIUM_WEBEXT_CORPUS`,
/// or else in the packages `cargo xtask webext-suite` downloaded.
#[test]
#[ignore = "needs Chrome Web Store downloads on disk"]
fn prepares_a_corpus_of_real_packages() {
    let dir = std::env::var_os("ZEPHIUM_WEBEXT_CORPUS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/webext-suite"));
    if !dir.is_dir() {
        eprintln!("skipping: run `cargo xtask webext-suite` first");
        return;
    }
    let mut failures = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "crx") {
            continue;
        }
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let result = (|| -> Result<String, String> {
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            let verified = crx::verify(&bytes, None).map_err(|e| format!("verify: {e}"))?;
            let temp = tempfile::tempdir().map_err(|e| e.to_string())?;
            let root = temp.path().join("package");
            archive::extract(verified.zip, &root, &Limits::default())
                .map_err(|e| format!("extract: {e}"))?;
            let manifest = Manifest::load(&root).map_err(|e| format!("manifest: {e}"))?;
            let report = prepare(
                &root,
                &CompatLayer::new("/* compat */").with_permissions(&["nativeMessaging"]),
            )
            .map_err(|e| format!("prepare: {e}"))?;
            Ok(format!(
                "{} {} MV{} worker={:?} html={} events={}",
                manifest.name().unwrap_or_default(),
                manifest.version().unwrap_or_default(),
                manifest.manifest_version(),
                report.worker,
                report.html_injected,
                report.events.len()
            ))
        })();
        match result {
            Ok(summary) => eprintln!("ok   {name}: {summary}"),
            Err(error) => {
                eprintln!("FAIL {name}: {error}");
                failures.push(name);
            }
        }
    }
    assert!(failures.is_empty(), "failed: {failures:?}");
}
