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
