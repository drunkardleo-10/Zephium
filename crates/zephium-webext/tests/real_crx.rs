use std::path::Path;

use zephium_webext::archive::{self, Limits};
use zephium_webext::manifest::Manifest;
use zephium_webext::prepare::{prepare, CompatLayer};
use zephium_webext::{crx, permissions, ExtensionId};

const BITWARDEN_CRX: &str = "/private/tmp/bitwarden-current-20260926.crx3";

#[test]
#[ignore = "needs a Chrome Web Store download on disk"]
fn verifies_extracts_and_prepares_bitwarden() {
    let Ok(bytes) = std::fs::read(Path::new(BITWARDEN_CRX)) else {
        eprintln!("skipping: {BITWARDEN_CRX} is absent");
        return;
    };
    let expected = ExtensionId::parse("nngceckbapebfimnlniiiahkandclblb").unwrap();
    let verified = crx::verify(&bytes, Some(&expected)).unwrap();

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("bitwarden");
    let stats = archive::extract(verified.zip, &dir, &Limits::default()).unwrap();
    let manifest = Manifest::load(&dir).unwrap();
    eprintln!(
        "{} {} (MV{}): {} files, {} bytes",
        manifest.name().unwrap_or_default(),
        manifest.version().unwrap_or_default(),
        manifest.manifest_version(),
        stats.files,
        stats.bytes
    );
    eprintln!("background: {:?}", manifest.background());
    eprintln!("warnings: {:#?}", permissions::warnings(&manifest));

    let report = prepare(&dir, &CompatLayer::new("")).unwrap();
    eprintln!(
        "prepared: worker {:?}, {} HTML injected, {} skipped, {} events, manifest rewritten: {}",
        report.worker,
        report.html_injected,
        report.html_skipped.len(),
        report.events.len(),
        report.manifest_rewritten
    );
    eprintln!("events: {:?}", report.events);
    assert!(!manifest.name().unwrap_or_default().starts_with("__MSG_"));
    Manifest::load(&dir).unwrap();
}

/// Verifies, extracts and prepares every package in `ZEPHIUM_WEBEXT_CORPUS`.
#[test]
#[ignore = "needs Chrome Web Store downloads on disk"]
fn prepares_a_corpus_of_real_packages() {
    let Some(dir) = std::env::var_os("ZEPHIUM_WEBEXT_CORPUS") else {
        eprintln!("skipping: ZEPHIUM_WEBEXT_CORPUS is not set");
        return;
    };
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
