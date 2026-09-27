#![cfg(any(
    target_os = "macos",
    target_os = "linux",
    all(target_os = "windows", feature = "windows-namespace-validation")
))]

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;

use serde_json::{json, Value};
use zephium_core::extensions::ExtensionPackageRevision;
use zephium_extension_package::macos_compatibility as compiler;
use zephium_extension_package::sandbox_withholding::SANDBOX_WITHHELD_HTML;
use zephium_extension_package::ChromiumManifestKey;

use super::*;

#[path = "native_fixture.rs"]
#[cfg(test)]
mod native_fixture;
#[path = "../../../../zephium-extension-repository/src/beta.rs"]
#[allow(
    dead_code,
    unused_imports,
    reason = "the signed fixture includes the full production repository API; its production build is linted separately"
)]
mod repository;
use crate::beta::{
    admit_beta_source,
    tests::{basic, Package},
    BetaRuntimeTarget,
};
use crate::public_policy::{tests::Fixture, AcceptedExtensionPolicy, ExtensionPolicyCache};
use repository::{BetaPackageRepository, BetaRepositoryError};

#[tokio::test]
async fn candidate_capabilities_v2_withholds_sandbox_and_reopens_exact_output() {
    let rig = Rig::new().await;
    let mut manifest = basic();
    manifest["permissions"] = json!([
        "storage",
        "scripting",
        "offscreen",
        "clipboardRead",
        "sidePanel",
        "unlimitedStorage",
        "notifications",
        "webNavigation"
    ]);
    manifest["optional_permissions"] = json!(["nativeMessaging"]);
    manifest["sandbox"] = json!({"pages":["sandbox.html"]});
    manifest["content_security_policy"] = json!({
        "extension_pages":"script-src 'self'; object-src 'self'",
        "sandbox":"sandbox allow-scripts; script-src 'self'"
    });
    manifest["side_panel"] = json!({"default_path":"options.html"});
    manifest["web_accessible_resources"] = json!([{
        "resources":["sandbox.html", "popup.html"], "matches":["<all_urls>"]
    }]);
    let mut package = Package::new(manifest.clone());
    package.set_manifest_for_compatibility(
        manifest,
        &[(
            "offscreen-document/index.html",
            b"<!doctype html><html><head></head><body>Original offscreen</body></html>",
        )],
    );
    assert!(matches!(
        crate::beta::admit_external_source(
            package.source(),
            &package.manifest,
            BetaRuntimeTarget::MacosNative,
            ExtensionPackageRevision::INITIAL,
            None,
        ),
        Err(crate::beta::BetaSourceAdmissionError::Unsupported(_))
    ));
    #[cfg(feature = "capabilities-v2-qa")]
    assert!(matches!(
        crate::beta::admit_signed_bitwarden_capabilities_v2_qa(
            package.source(),
            &package.manifest,
            BetaRuntimeTarget::MacosNative,
            ExtensionPackageRevision::INITIAL,
            None,
        ),
        Err(crate::beta::BetaSourceAdmissionError::SourceMismatch)
    ));
    let source = crate::beta::admit_external_source_candidate_v2(
        package.source(),
        &package.manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::INITIAL,
        None,
    )
    .unwrap();
    assert_eq!(
        source
            .manifest()
            .descriptor()
            .compatibility_target()
            .as_str(),
        zephium_core::extensions::LOCAL_MACOS_CAPABILITIES_V2_COMPATIBILITY_TARGET
    );
    for limitation in [
        crate::beta::BetaCompatibilityLimitation::OffscreenLocalStorageOnly,
        crate::beta::BetaCompatibilityLimitation::SandboxedPagesUnavailable,
        crate::beta::BetaCompatibilityLimitation::ClipboardReadUnavailable,
        crate::beta::BetaCompatibilityLimitation::SidePanelUnavailable,
    ] {
        assert!(source.inner.limitations.contains(&limitation));
    }
    let mut workspace = rig.workspace();
    let artifact = workspace.prepare_external(source, &package.crx).unwrap();
    artifact.verify().unwrap();
    let output: Value =
        serde_json::from_slice(&fs::read(rig.ready().join("extension/manifest.json")).unwrap())
            .unwrap();
    let permissions = output["permissions"].as_array().unwrap();
    for required in ["offscreen", "nativeMessaging"] {
        assert!(permissions.contains(&json!(required)));
    }
    for withheld in ["clipboardRead", "sidePanel", "unlimitedStorage"] {
        assert!(!permissions.contains(&json!(withheld)));
    }
    assert!(output.get("sandbox").is_none());
    assert!(output.get("side_panel").is_none());
    assert_eq!(
        output["background"]["service_worker"],
        compiler::BACKGROUND_WRAPPER_V2
    );
    assert!(output["content_security_policy"].get("sandbox").is_none());
    assert_eq!(
        output["web_accessible_resources"][0]["resources"],
        json!(["popup.html"])
    );
    assert_eq!(
        fs::read(rig.ready().join("extension/sandbox.html")).unwrap(),
        SANDBOX_WITHHELD_HTML
    );
    let popup = fs::read_to_string(rig.ready().join("extension/popup.html")).unwrap();
    assert!(popup.contains(compiler::RUNTIME_MESSAGING_BRIDGE_V2));
    let worker = fs::read_to_string(
        rig.ready()
            .join("extension")
            .join(compiler::BACKGROUND_WRAPPER_V2),
    )
    .unwrap();
    let prelude = worker.find(compiler::API_PRELUDE).unwrap();
    let messaging = worker.find(compiler::RUNTIME_MESSAGING_BRIDGE_V2).unwrap();
    let offscreen = worker.find(compiler::OFFSCREEN_BACKGROUND_BRIDGE).unwrap();
    let publisher = worker.find("background.js").unwrap();
    assert!(prelude < messaging && messaging < offscreen && offscreen < publisher);
    let provenance = artifact
        .provenance(ExtensionProvenanceSource::ChromeWebStore)
        .unwrap();
    assert!(
        matches!(provenance.transform(), ExtensionTransformProvenance::Compiled { target, .. }
        if target.as_str() == "local.webkit-capabilities.v2")
    );
    drop(artifact);
    drop(workspace);
    let workspace = rig.workspace();
    let reopened = workspace
        .reopen_external_bound(
            &provenance,
            provenance.upstream(),
            BetaRuntimeTarget::MacosNative,
        )
        .unwrap();
    reopened.verify().unwrap();
    assert_eq!(
        fs::read(rig.ready().join("extension/sandbox.html")).unwrap(),
        SANDBOX_WITHHELD_HTML
    );
}

#[tokio::test]
#[ignore = "requires ZEPHIUM_SIGNED_BITWARDEN_CRX3 with the authenticated original archive"]
async fn signed_bitwarden_capabilities_v2_candidate_preserves_provenance_and_inert_pages() {
    let fixture = std::env::var("ZEPHIUM_SIGNED_BITWARDEN_CRX3").expect("signed CRX fixture path");
    let crx = fs::read(fixture).unwrap();
    let id =
        zephium_extension_package::ChromiumExtensionId::parse("nngceckbapebfimnlniiiahkandclblb")
            .unwrap();
    let mut archive =
        zephium_extension_acquisition::AcquiredExtensionArchive::authenticate_upstream_crx3(
            &crx, &id, None,
        )
        .unwrap();
    let manifest_slot = archive
        .files()
        .iter()
        .position(|file| file.path().as_str() == "manifest.json")
        .unwrap();
    let mut manifest = Vec::new();
    let _manifest_receipt = archive.copy_file(manifest_slot, &mut manifest).unwrap();
    let receipts = (0..archive.files().len())
        .map(|index| archive.copy_file(index, &mut std::io::sink()).unwrap())
        .collect::<Vec<_>>();
    let source = archive.finish_tree(receipts).unwrap();
    assert!(matches!(
        crate::beta::admit_external_source(
            source,
            &manifest,
            BetaRuntimeTarget::MacosNative,
            ExtensionPackageRevision::INITIAL,
            None,
        ),
        Err(crate::beta::BetaSourceAdmissionError::Unsupported(_))
    ));
    let mut archive =
        zephium_extension_acquisition::AcquiredExtensionArchive::authenticate_upstream_crx3(
            &crx, &id, None,
        )
        .unwrap();
    let receipts = (0..archive.files().len())
        .map(|index| archive.copy_file(index, &mut std::io::sink()).unwrap())
        .collect::<Vec<_>>();
    let source = archive.finish_tree(receipts).unwrap();
    #[cfg(feature = "capabilities-v2-qa")]
    let source = crate::beta::admit_signed_bitwarden_capabilities_v2_qa(
        source,
        &manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::INITIAL,
        None,
    )
    .unwrap();
    #[cfg(not(feature = "capabilities-v2-qa"))]
    let source = crate::beta::admit_external_source_candidate_v2(
        source,
        &manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::INITIAL,
        None,
    )
    .unwrap();
    assert_eq!(
        source
            .manifest()
            .descriptor()
            .compatibility_target()
            .as_str(),
        zephium_core::extensions::LOCAL_MACOS_CAPABILITIES_V2_COMPATIBILITY_TARGET
    );
    #[cfg(feature = "capabilities-v2-qa")]
    {
        let mut legacy_archive =
            zephium_extension_acquisition::AcquiredExtensionArchive::authenticate_upstream_crx3(
                &crx, &id, None,
            )
            .unwrap();
        let legacy = Blueprint::derive(&source.inner, &mut legacy_archive, V2Recipe::Legacy).unwrap();
        assert!(matches!(
            &legacy.transform,
            ExtensionTransformProvenance::Compiled { target, revision, sha256 }
                if target.as_str() == "local.webkit-capabilities.v2"
                    && revision.get() == 4
                    && *sha256 == signed_bitwarden_qa_transform_refresh().0
        ));
        assert_eq!(
            crate::beta::hex(legacy.index.index_sha256().bytes()),
            "edb4abee7419c14aa1ac0f230edb9dc9d8e90e401984c504a923e119f5a70f3e"
        );
        assert_eq!(
            crate::beta::hex(legacy.index.tree_sha256().bytes()),
            "ef3bfd62fd09ff2918d18641ba5b20d655a0ea9d55741cbd3b9d027d7f6f3734"
        );
        assert_eq!(
            crate::beta::hex(sha2::Sha256::digest(&legacy.evidence).into()),
            "ec079f2d21105c0fb706dce56c1db35eb5d0beae674dc68716f163db6adf3ce4"
        );
        assert!(legacy.replacements.contains_key(compiler::BACKGROUND_WRAPPER));
        assert!(!legacy
            .replacements
            .contains_key(compiler::BACKGROUND_WRAPPER_V2));
        assert!(!legacy
            .replacements
            .contains_key(compiler::DISPOSAL_SYMBOLS_BRIDGE));
        let legacy_provenance = ExtensionInstallProvenance::new(
            ExtensionProvenanceSource::ChromeWebStore,
            source.inner.upstream(),
            source.inner.original_tree(),
            legacy.transform.clone(),
            legacy.manifest.descriptor(),
            legacy.index.index_sha256().bytes(),
            source.inner.policy_evidence().unwrap(),
        )
        .unwrap();
        assert!(exact_legacy_bitwarden_v2_reopen(&legacy_provenance));
        let ExtensionTransformProvenance::Compiled { target, revision, .. } = &legacy.transform
        else {
            unreachable!();
        };
        let tampered_transform = ExtensionTransformProvenance::Compiled {
            target: target.clone(),
            revision: *revision,
            sha256: [0; 32],
        };
        let tampered = ExtensionInstallProvenance::new(
            ExtensionProvenanceSource::ChromeWebStore,
            source.inner.upstream(),
            source.inner.original_tree(),
            tampered_transform,
            legacy.manifest.descriptor(),
            legacy.index.index_sha256().bytes(),
            source.inner.policy_evidence().unwrap(),
        )
        .unwrap();
        assert!(!exact_legacy_bitwarden_v2_reopen(&tampered));
        let wrong_output = ExtensionInstallProvenance::new(
            ExtensionProvenanceSource::ChromeWebStore,
            source.inner.upstream(),
            source.inner.original_tree(),
            legacy.transform.clone(),
            legacy.manifest.descriptor(),
            [0; 32],
            source.inner.policy_evidence().unwrap(),
        )
        .unwrap();
        assert!(!exact_legacy_bitwarden_v2_reopen(&wrong_output));

        let mut receipt_archive =
            zephium_extension_acquisition::AcquiredExtensionArchive::authenticate_upstream_crx3(
                &crx, &id, None,
            )
            .unwrap();
        let receipts = (0..receipt_archive.files().len())
            .map(|index| receipt_archive.copy_file(index, &mut std::io::sink()).unwrap())
            .collect::<Vec<_>>();
        let receipt = receipt_archive.finish_tree(receipts).unwrap();
        let disposal_only_source = crate::beta::admit_signed_bitwarden_capabilities_v2_qa(
            receipt,
            &manifest,
            BetaRuntimeTarget::MacosNative,
            ExtensionPackageRevision::new(2).unwrap(),
            Some(source.inner.upstream()),
        )
        .unwrap();
        let mut disposal_archive =
            zephium_extension_acquisition::AcquiredExtensionArchive::authenticate_upstream_crx3(
                &crx, &id, None,
            )
            .unwrap();
        let disposal_only = Blueprint::derive(
            &disposal_only_source.inner,
            &mut disposal_archive,
            V2Recipe::DisposalOnly,
        )
        .unwrap();
        assert!(matches!(
            &disposal_only.transform,
            ExtensionTransformProvenance::Compiled { target, revision, sha256 }
                if target.as_str() == "local.webkit-capabilities.v2"
                    && revision.get() == 5
                    && *sha256 == signed_bitwarden_qa_transform_refresh().1
        ));
        assert_eq!(
            crate::beta::hex(disposal_only.index.index_sha256().bytes()),
            "757239ba97c0d55e57dad9d8fe40afdaebd38480584448c4b412a6d8c5a76228"
        );
        assert_eq!(
            crate::beta::hex(disposal_only.index.tree_sha256().bytes()),
            "0f6cde83b3106882f594a394f8c69d8b8ad866ee20d69039fafcd197ba18d2eb"
        );
        assert_eq!(
            crate::beta::hex(sha2::Sha256::digest(&disposal_only.evidence).into()),
            "a4b6ef9526282792a916161b3bf4ecd754357b52adb8e6b7137df217305952d0"
        );
        let disposal_provenance = ExtensionInstallProvenance::new(
            ExtensionProvenanceSource::ChromeWebStore,
            disposal_only_source.inner.upstream(),
            disposal_only_source.inner.original_tree(),
            disposal_only.transform.clone(),
            disposal_only.manifest.descriptor(),
            disposal_only.index.index_sha256().bytes(),
            disposal_only_source.inner.policy_evidence().unwrap(),
        )
        .unwrap();
        assert!(exact_disposal_only_bitwarden_v2_reopen(&disposal_provenance));
    }
    let rig = Rig::new().await;
    let mut workspace = rig.workspace();
    let artifact = workspace.prepare_external(source, &crx).unwrap();
    artifact.verify().unwrap();
    assert_eq!(
        artifact.resolve_metadata().unwrap().name().as_str(),
        "Bitwarden Password Manager"
    );
    let output: Value =
        serde_json::from_slice(&fs::read(rig.ready().join("extension/manifest.json")).unwrap())
            .unwrap();
    assert!(output.get("sandbox").is_none());
    assert!(output.get("side_panel").is_none());
    assert_eq!(
        output["background"]["service_worker"],
        compiler::BACKGROUND_WRAPPER_V3
    );
    assert!(output["content_security_policy"].get("sandbox").is_none());
    let war = output["web_accessible_resources"].as_array().unwrap();
    for page in ["overlay/menu-button.html", "overlay/menu-list.html"] {
        assert_eq!(
            fs::read(rig.ready().join("extension").join(page)).unwrap(),
            SANDBOX_WITHHELD_HTML
        );
        assert!(!war.iter().any(
            |group| group["resources"]
                .as_array()
                .is_some_and(|resources| resources
                    .iter()
                    .any(|resource| resource.as_str() == Some(page)))
        ));
    }
    let popup = fs::read_to_string(rig.ready().join("extension/popup/index.html")).unwrap();
    assert!(popup.contains(compiler::RUNTIME_MESSAGING_BRIDGE_V2));
    assert!(popup.contains(compiler::UNAVAILABLE_PERMISSIONS_BRIDGE));
    let worker = fs::read_to_string(
        rig.ready()
            .join("extension")
            .join(compiler::BACKGROUND_WRAPPER_V3),
    )
    .unwrap();
    assert!(worker.starts_with(&format!(
        "importScripts(\"/{}\",",
        compiler::DISPOSAL_SYMBOLS_BRIDGE
    )));
    assert!(worker.find(compiler::API_PRELUDE).unwrap()
        < worker.find(compiler::UNAVAILABLE_PERMISSIONS_BRIDGE).unwrap());
    assert!(worker.find(compiler::UNAVAILABLE_PERMISSIONS_BRIDGE).unwrap()
        < worker.find("\"/background.js\"").unwrap());
    assert!(worker.contains("\"/background.js\""));
    assert_eq!(
        fs::read(rig.ready().join("extension").join(compiler::DISPOSAL_SYMBOLS_BRIDGE)).unwrap(),
        compiler::DISPOSAL_SYMBOLS_BRIDGE_SOURCE.as_bytes()
    );
    assert_eq!(
        fs::read(
            rig.ready()
                .join("extension")
                .join(compiler::UNAVAILABLE_PERMISSIONS_BRIDGE)
        )
        .unwrap(),
        compiler::UNAVAILABLE_PERMISSIONS_BRIDGE_SOURCE.as_bytes()
    );
    let offscreen = fs::read(rig.ready().join("extension/offscreen-document/index.html")).unwrap();
    assert!(!offscreen
        .windows(b"__zephium__".len())
        .any(|bytes| bytes == b"__zephium__"));
    let provenance = artifact
        .provenance(ExtensionProvenanceSource::ChromeWebStore)
        .unwrap();
    assert_eq!(
        provenance.upstream().original_crx_sha256(),
        <[u8; 32]>::from(sha2::Sha256::digest(&crx))
    );
    let ExtensionTransformProvenance::Compiled { target, revision, sha256 } =
        provenance.transform()
    else {
        panic!("signed v2 candidate lost its compiled transform");
    };
    assert_eq!(target.as_str(), "local.webkit-capabilities.v2");
    assert_eq!(revision.get(), 6);
    #[cfg(feature = "capabilities-v2-qa")]
    assert_eq!(*sha256, signed_bitwarden_qa_transform_refresh().2);
    drop(artifact);
    drop(workspace);
    let workspace = rig.workspace();
    let reopened = workspace
        .reopen_external_bound(
            &provenance,
            provenance.upstream(),
            BetaRuntimeTarget::MacosNative,
        )
        .unwrap();
    reopened.verify().unwrap();
    assert_eq!(
        reopened.resolve_metadata().unwrap().name().as_str(),
        "Bitwarden Password Manager"
    );
    for page in ["overlay/menu-button.html", "overlay/menu-list.html"] {
        assert_eq!(
            fs::read(rig.ready().join("extension").join(page)).unwrap(),
            SANDBOX_WITHHELD_HTML
        );
    }
}

#[tokio::test]
async fn identity_recipe_binds_authenticated_crx_id_and_reopens_exact_bytes() {
    let rig = Rig::new().await;
    let mut manifest = basic();
    manifest["permissions"] = json!(["storage", "identity"]);
    let mut package = Package::new(manifest.clone());
    package.set_manifest_for_compatibility(manifest, &[]);
    let source = crate::beta::admit_external_source(
        package.source(),
        &package.manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::INITIAL,
        None,
    )
    .unwrap();
    assert_eq!(
        source
            .manifest()
            .descriptor()
            .compatibility_target()
            .as_str(),
        zephium_core::extensions::LOCAL_MACOS_IDENTITY_V1_COMPATIBILITY_TARGET,
    );
    let mut workspace = rig.workspace();
    let artifact = workspace.prepare_external(source, &package.crx).unwrap();
    artifact.verify().unwrap();
    let output: Value =
        serde_json::from_slice(&fs::read(rig.ready().join("extension/manifest.json")).unwrap())
            .unwrap();
    let permissions = output["permissions"].as_array().unwrap();
    assert!(permissions.contains(&json!("identity")));
    assert!(permissions.contains(&json!("nativeMessaging")));
    let bridge = fs::read_to_string(
        rig.ready()
            .join("extension/__zephium__/webkit-identity-v1.js"),
    )
    .unwrap();
    assert!(bridge.contains(package.id.as_str()));
    assert!(!bridge.contains("__ZEPHIUM_CHROMIUM_ID__"));
    assert!(!bridge.contains("getAuthToken"));
    let worker =
        fs::read_to_string(rig.ready().join("extension/__zephium_background_v1.js")).unwrap();
    assert!(worker.contains("webkit-identity-v1.js"));
    let provenance = artifact
        .provenance(ExtensionProvenanceSource::ChromeWebStore)
        .unwrap();
    assert!(
        matches!(provenance.transform(), ExtensionTransformProvenance::Compiled { target, .. } if target.as_str() == "local.webkit-identity.v1")
    );
    drop(artifact);
    drop(workspace);
    let workspace = rig.workspace();
    let restored = workspace
        .reopen_external_bound(
            &provenance,
            provenance.upstream(),
            BetaRuntimeTarget::MacosNative,
        )
        .unwrap();
    restored.verify().unwrap();
}

#[tokio::test]
async fn main_document_glob_recipe_preserves_original_files_and_refuses_behavioral_css() {
    let rig = Rig::new().await;
    let mut manifest = basic();
    manifest["permissions"] = json!(["storage", "scripting", "identity", "sidePanel"]);
    manifest["side_panel"] = json!({"default_path":"side-panel.html"});
    manifest["content_scripts"] = json!([{
        "matches":["https://example.test/*"],
        "exclude_globs":["*blocked*"],
        "all_frames":true,
        "match_about_blank":true,
        "run_at":"document_idle",
        "js":["content.js"],
        "css":["fonts.css"]
    }]);
    let original_js = b"void 0;";
    let fonts = b"@font-face { font-family: Test; src: url(data:font/woff2;base64,AA); }";
    let mut package = Package::new(manifest.clone());
    package.set_manifest_for_compatibility(
        manifest.clone(),
        &[
            ("fonts.css", fonts),
            ("side-panel.html", b"<!doctype html><title>Panel</title>"),
        ],
    );
    let source = crate::beta::admit_external_source(
        package.source(),
        &package.manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::INITIAL,
        None,
    )
    .unwrap();
    assert_eq!(
        source
            .manifest()
            .descriptor()
            .compatibility_target()
            .as_str(),
        zephium_core::extensions::LOCAL_MACOS_MAIN_DOCUMENT_GLOBS_V1_COMPATIBILITY_TARGET
    );
    assert!(source
        .inner
        .limitations
        .contains(&crate::beta::BetaCompatibilityLimitation::MainDocumentContentScriptsOnly));
    assert!(source
        .inner
        .limitations
        .contains(&crate::beta::BetaCompatibilityLimitation::ContentScriptFontsUnavailable));
    assert!(source
        .inner
        .limitations
        .contains(&crate::beta::BetaCompatibilityLimitation::SidePanelUnavailable));
    let mut workspace = rig.workspace();
    let artifact = workspace.prepare_external(source, &package.crx).unwrap();
    artifact.verify().unwrap();
    let output: Value =
        serde_json::from_slice(&fs::read(rig.ready().join("extension/manifest.json")).unwrap())
            .unwrap();
    let group = &output["content_scripts"][0];
    assert_eq!(group["all_frames"], false);
    assert_eq!(group["match_about_blank"], false);
    assert!(group.get("exclude_globs").is_none());
    assert!(group.get("css").is_none());
    assert!(group["js"]
        .as_array()
        .unwrap()
        .iter()
        .any(|path| path.as_str() == Some("__zephium__/webkit-glob-bootstrap-0-v1.js")));
    assert_eq!(output["side_panel"]["default_path"], "side-panel.html");
    assert!(output["permissions"]
        .as_array()
        .unwrap()
        .contains(&json!("sidePanel")));
    assert_eq!(
        fs::read(rig.ready().join("extension/content.js")).unwrap(),
        original_js
    );
    assert!(
        fs::read_to_string(rig.ready().join("extension/__zephium_background_v1.js"))
            .unwrap()
            .contains("webkit-glob-worker-v1.js")
    );
    let provenance = artifact
        .provenance(ExtensionProvenanceSource::ChromeWebStore)
        .unwrap();
    assert!(
        matches!(provenance.transform(), ExtensionTransformProvenance::Compiled { target, .. } if target.as_str() == "local.webkit-main-document-globs.v1")
    );
    drop(artifact);
    drop(workspace);
    rig.workspace()
        .reopen_external_bound(
            &provenance,
            provenance.upstream(),
            BetaRuntimeTarget::MacosNative,
        )
        .unwrap()
        .verify()
        .unwrap();

    let behavioral_rig = Rig::new().await;
    let mut behavioral = Package::new(manifest.clone());
    behavioral.set_manifest_for_compatibility(
        manifest,
        &[
            (
                "fonts.css",
                b"@font-face { font-family: Test; } body { display: none; }",
            ),
            ("side-panel.html", b"<!doctype html><title>Panel</title>"),
        ],
    );
    let source = crate::beta::admit_external_source(
        behavioral.source(),
        &behavioral.manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::INITIAL,
        None,
    )
    .unwrap();
    assert!(matches!(
        behavioral_rig
            .workspace()
            .prepare_external(source, &behavioral.crx),
        Err(BetaArtifactPreparationError::Transform)
    ));
}

struct Rig {
    root: tempfile::TempDir,
    fixture: Fixture,
    cache: ExtensionPolicyCache,
    policy: Arc<AcceptedExtensionPolicy>,
}

impl Rig {
    async fn new() -> Self {
        #[cfg(target_os = "macos")]
        let root = tempfile::tempdir_in("/private/tmp").unwrap();
        #[cfg(target_os = "linux")]
        let root = tempfile::tempdir_in("/tmp").unwrap();
        #[cfg(target_os = "windows")]
        let root = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let mut fixture = Fixture::new();
        fixture.policy["beta_targets"] = json!([
            {"target":"macos.wkwebextension.v1","policy_version":1},
            {"target":"windows.webview2.v1","policy_version":1}
        ]);
        fixture.publish(1);
        let mut cache = ExtensionPolicyCache::open(
            LockedPrivateNamespace::open_or_create(root.path().join("policy")).unwrap(),
            &fixture.client(),
        )
        .unwrap();
        let policy = Arc::new(cache.refresh(&fixture.client()).await.unwrap());
        Self {
            root,
            fixture,
            cache,
            policy,
        }
    }
    fn workspace(&self) -> BetaPreparationWorkspace {
        BetaPreparationWorkspace::open(
            LockedPrivateNamespace::open_or_create(self.root.path().join("artifacts")).unwrap(),
        )
        .unwrap()
    }
    fn source(&self, package: &Package, runtime: BetaRuntimeTarget) -> ProductAdmittedBetaSource {
        admit_beta_source(
            Arc::clone(&self.policy),
            package.source(),
            &package.manifest,
            runtime,
            ExtensionPackageRevision::INITIAL,
            None,
        )
        .unwrap()
    }
    fn ready(&self) -> PathBuf {
        self.root.path().join("artifacts/ready")
    }
}

// Restore permissions only inside this self-authored fixture directory and
// never follow symlinks, including intentionally hostile test entries.
#[cfg(unix)]
impl Drop for Rig {
    fn drop(&mut self) {
        fn writable(path: &Path) {
            let Ok(metadata) = fs::symlink_metadata(path) else {
                return;
            };
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return;
            }
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
            for child in fs::read_dir(path).unwrap() {
                writable(&child.unwrap().path());
            }
        }
        writable(self.root.path());
    }
}

#[tokio::test]
async fn both_targets_prepare_exact_sealed_output_and_complete_structural_provenance() {
    for runtime in [
        BetaRuntimeTarget::MacosNative,
        BetaRuntimeTarget::WindowsNative,
    ] {
        let rig = Rig::new().await;
        let mut package = Package::new(basic());
        package.set_manifest_with_extra(
            basic(),
            &[("Assets/MixedCase.txt", b"exact bytes"), ("empty.dat", b"")],
        );
        let source = rig.source(&package, runtime);
        let original = source.original_tree();
        let mut workspace = rig.workspace();
        let artifact = workspace.prepare(source, &package.crx).unwrap();
        artifact.verify().unwrap();
        assert!(artifact.retained_bytes() <= MAX_PREPARED_BETA_ARTIFACT_RETAINED_BYTES);
        assert_eq!(
            fs::read(rig.ready().join("original.crx")).unwrap(),
            package.crx
        );
        let output = fs::read(rig.ready().join("extension/manifest.json")).unwrap();
        let mut parsed: Value = serde_json::from_slice(&output).unwrap();
        let key = parsed.as_object_mut().unwrap().remove("key").unwrap();
        assert_eq!(
            parsed,
            serde_json::from_slice::<Value>(&package.manifest).unwrap()
        );
        assert_eq!(
            key,
            json!(base64::engine::general_purpose::STANDARD.encode(&package.public))
        );
        let native_key = ChromiumManifestKey::parse_canonical(key.as_str().unwrap()).unwrap();
        assert_eq!(native_key.extension_id(), &package.id);
        assert_eq!(
            fs::read(rig.ready().join("extension/Assets/MixedCase.txt")).unwrap(),
            b"exact bytes"
        );
        assert!(fs::read(rig.ready().join("extension/empty.dat"))
            .unwrap()
            .is_empty());
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(rig.ready()).unwrap().permissions().mode() & 0o777,
            0o500
        );
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(rig.ready().join("extension/manifest.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o400
        );
        let provenance = artifact
            .provenance(ExtensionProvenanceSource::ChromeWebStore)
            .unwrap();
        assert_eq!(provenance.original(), original);
        assert_eq!(
            provenance.output_index(),
            artifact.index().index_sha256().bytes()
        );
        assert_eq!(
            provenance.upstream().original_crx_sha256(),
            <[u8; 32]>::from(Sha256::digest(&package.crx))
        );
        assert_ne!(
            provenance.original().manifest,
            provenance.package().manifest_sha256().bytes()
        );
        assert!(
            matches!(provenance.transform(), ExtensionTransformProvenance::Compiled { target, revision, sha256 }
            if target.as_str() == TRANSFORM_ID && revision.get() == 1 && *sha256 == <[u8;32]>::from(Sha256::digest(TRANSFORM_DESCRIPTOR)))
        );
        workspace.discard().unwrap();
        assert_eq!(artifact.verify(), Err(BetaArtifactPreparationError::Stale));
    }
}

#[tokio::test]
async fn existing_key_preserves_exact_manifest_bytes_and_identity_provenance() {
    let rig = Rig::new().await;
    let mut package = Package::new(basic());
    let mut manifest = basic();
    manifest["key"] = json!(base64::engine::general_purpose::STANDARD.encode(&package.public));
    package.set_manifest(manifest);
    let mut workspace = rig.workspace();
    let artifact = workspace
        .prepare(
            rig.source(&package, BetaRuntimeTarget::WindowsNative),
            &package.crx,
        )
        .unwrap();
    assert_eq!(
        fs::read(rig.ready().join("extension/manifest.json")).unwrap(),
        package.manifest
    );
    assert_eq!(
        artifact.transformation(),
        &ExtensionTransformProvenance::Identity
    );
    let provenance = artifact
        .provenance(ExtensionProvenanceSource::ChromeWebStore)
        .unwrap();
    assert_eq!(
        provenance.original().tree,
        provenance.package().tree_sha256().bytes()
    );
    workspace.discard().unwrap();
}

#[tokio::test]
async fn ready_output_cannot_be_rebound_to_another_runtime_or_local_revision() {
    let rig = Rig::new().await;
    let package = Package::new(basic());
    let mut workspace = rig.workspace();
    let artifact = workspace
        .prepare(
            rig.source(&package, BetaRuntimeTarget::MacosNative),
            &package.crx,
        )
        .unwrap();
    assert!(workspace
        .reopen(rig.source(&package, BetaRuntimeTarget::WindowsNative))
        .is_err());
    let next = admit_beta_source(
        Arc::clone(&rig.policy),
        package.source(),
        &package.manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::new(2).unwrap(),
        None,
    )
    .unwrap();
    assert!(workspace.reopen(next).is_err());
    artifact.verify().unwrap();
    workspace.discard().unwrap();
}

#[tokio::test]
async fn stale_policy_is_rejected_before_any_original_or_output_write() {
    let mut rig = Rig::new().await;
    let package = Package::new(basic());
    let source = rig.source(&package, BetaRuntimeTarget::MacosNative);
    rig.fixture.policy["policy_revision"] = json!(2);
    rig.fixture.publish(2);
    rig.policy = Arc::new(rig.cache.refresh(&rig.fixture.client()).await.unwrap());
    let mut workspace = rig.workspace();
    assert_eq!(
        workspace.prepare(source, &package.crx).unwrap_err(),
        BetaArtifactPreparationError::Policy
    );
    assert!(!rig.root.path().join("artifacts/incoming").exists());
    assert!(!rig.ready().exists());
}

#[tokio::test]
async fn changed_original_bytes_are_refused_before_any_staging_write() {
    let rig = Rig::new().await;
    let package = Package::new(basic());
    let mut workspace = rig.workspace();
    let mut changed = package.crx.clone();
    let last = changed.len() - 1;
    changed[last] ^= 1;
    assert_eq!(
        workspace
            .prepare(
                rig.source(&package, BetaRuntimeTarget::MacosNative),
                &changed
            )
            .unwrap_err(),
        BetaArtifactPreparationError::Source
    );
    assert!(!rig.root.path().join("artifacts/incoming").exists());
    assert!(!rig.ready().exists());
}

#[tokio::test]
async fn reopen_recomputes_transformation_after_policy_refresh_and_invalidates_old_receipts() {
    let mut rig = Rig::new().await;
    let package = Package::new(basic());
    let mut workspace = rig.workspace();
    let old = workspace
        .prepare(
            rig.source(&package, BetaRuntimeTarget::MacosNative),
            &package.crx,
        )
        .unwrap();
    let output_index = old.index().index_sha256();
    let old_policy = old
        .provenance(ExtensionProvenanceSource::ChromeWebStore)
        .unwrap()
        .policy();
    rig.fixture.policy["policy_revision"] = json!(2);
    rig.fixture.publish(2);
    rig.policy = Arc::new(rig.cache.refresh(&rig.fixture.client()).await.unwrap());
    assert_eq!(old.verify(), Err(BetaArtifactPreparationError::Policy));
    let fresh = workspace
        .reopen(rig.source(&package, BetaRuntimeTarget::MacosNative))
        .unwrap();
    assert_eq!(fresh.index().index_sha256(), output_index);
    assert_ne!(
        fresh
            .provenance(ExtensionProvenanceSource::ChromeWebStore)
            .unwrap()
            .policy(),
        old_policy
    );
    drop(old);
    drop(fresh);
    drop(workspace);
    workspace = rig.workspace();
    let reopened = workspace
        .reopen(rig.source(&package, BetaRuntimeTarget::MacosNative))
        .unwrap();
    reopened.verify().unwrap();
    workspace.discard().unwrap();
}

#[tokio::test]
async fn every_prepublication_frontier_is_discarded_and_only_committed_ready_survives() {
    for frontier in [
        TestFrontier::Original,
        TestFrontier::Tree,
        TestFrontier::Index,
        TestFrontier::Evidence,
        TestFrontier::Sealed,
        TestFrontier::Published,
    ] {
        let rig = Rig::new().await;
        let package = Package::new(basic());
        let mut workspace = rig.workspace();
        workspace.stop_at = Some(frontier);
        assert_eq!(
            workspace
                .prepare(
                    rig.source(&package, BetaRuntimeTarget::MacosNative),
                    &package.crx
                )
                .unwrap_err(),
            BetaArtifactPreparationError::Storage,
            "{frontier:?}"
        );
        assert_eq!(
            workspace.discard(),
            Err(BetaArtifactPreparationError::Stale)
        );
        drop(workspace);
        let mut workspace = rig.workspace();
        assert!(!rig.root.path().join("artifacts/incoming").exists());
        if frontier == TestFrontier::Published {
            workspace
                .reopen(rig.source(&package, BetaRuntimeTarget::MacosNative))
                .unwrap()
                .verify()
                .unwrap();
        } else {
            assert!(!rig.ready().exists());
            workspace
                .prepare(
                    rig.source(&package, BetaRuntimeTarget::MacosNative),
                    &package.crx,
                )
                .unwrap()
                .verify()
                .unwrap();
        }
        workspace.discard().unwrap();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn modified_output_original_and_evidence_are_detected_on_verify_and_reopen() {
    for relative in [
        "extension/content.js",
        "extension/manifest.json",
        "original.crx",
        "tree-index.json",
        "evidence.json",
    ] {
        let rig = Rig::new().await;
        let package = Package::new(basic());
        let mut workspace = rig.workspace();
        let artifact = workspace
            .prepare(
                rig.source(&package, BetaRuntimeTarget::MacosNative),
                &package.crx,
            )
            .unwrap();
        let path = rig.ready().join(relative);
        let mut bytes = fs::read(&path).unwrap();
        bytes[0] ^= 1;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&path, bytes).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
        assert!(artifact.verify().is_err(), "{relative}");
        assert!(
            workspace
                .reopen(rig.source(&package, BetaRuntimeTarget::MacosNative))
                .is_err(),
            "{relative}"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn extra_members_symlinks_and_unsafe_modes_never_form_a_verified_tree() {
    for attack in 0..3 {
        let rig = Rig::new().await;
        let package = Package::new(basic());
        let mut workspace = rig.workspace();
        let artifact = workspace
            .prepare(
                rig.source(&package, BetaRuntimeTarget::MacosNative),
                &package.crx,
            )
            .unwrap();
        let tree = rig.ready().join("extension");
        fs::set_permissions(&tree, fs::Permissions::from_mode(0o700)).unwrap();
        match attack {
            0 => {
                let extra = tree.join("unexpected.js");
                fs::write(&extra, b"unexpected").unwrap();
                fs::set_permissions(extra, fs::Permissions::from_mode(0o400)).unwrap();
            }
            1 => {
                fs::remove_file(tree.join("content.js")).unwrap();
                std::os::unix::fs::symlink(
                    rig.root.path().join("outside"),
                    tree.join("content.js"),
                )
                .unwrap();
            }
            _ => fs::set_permissions(tree.join("content.js"), fs::Permissions::from_mode(0o644))
                .unwrap(),
        }
        fs::set_permissions(&tree, fs::Permissions::from_mode(0o500)).unwrap();
        assert!(artifact.verify().is_err());
    }
}

#[tokio::test]
async fn verification_and_discard_are_single_flight_and_owner_drop_revokes_receipts() {
    let rig = Rig::new().await;
    let package = Package::new(basic());
    let mut workspace = rig.workspace();
    let artifact = workspace
        .prepare(
            rig.source(&package, BetaRuntimeTarget::MacosNative),
            &package.crx,
        )
        .unwrap();
    let gate = Arc::clone(&workspace.gate);
    let held = gate.lock().unwrap();
    assert_eq!(artifact.verify(), Err(BetaArtifactPreparationError::Busy));
    assert_eq!(workspace.discard(), Err(BetaArtifactPreparationError::Busy));
    drop(held);
    artifact.verify().unwrap();
    assert_eq!(
        workspace
            .prepare(
                rig.source(&package, BetaRuntimeTarget::MacosNative),
                &package.crx
            )
            .unwrap_err(),
        BetaArtifactPreparationError::Occupied
    );
    drop(workspace);
    assert_eq!(artifact.verify(), Err(BetaArtifactPreparationError::Stale));
}

#[tokio::test]
async fn maximum_portable_depth_can_be_prepared_and_recovered_without_widening_fs_limits() {
    let rig = Rig::new().await;
    let mut package = Package::new(basic());
    let path = format!(
        "{}deep.txt",
        "d/".repeat(zephium_extension_package::MAX_EXTENSION_RELATIVE_PATH_DEPTH - 1)
    );
    package.set_manifest_with_extra(basic(), &[(&path, b"bounded depth")]);
    let mut workspace = rig.workspace();
    workspace.stop_at = Some(TestFrontier::Sealed);
    assert!(workspace
        .prepare(
            rig.source(&package, BetaRuntimeTarget::MacosNative),
            &package.crx
        )
        .is_err());
    drop(workspace);
    let mut workspace = rig.workspace();
    let artifact = workspace
        .prepare(
            rig.source(&package, BetaRuntimeTarget::MacosNative),
            &package.crx,
        )
        .unwrap();
    artifact.verify().unwrap();
    workspace.discard().unwrap();
}

fn repository(rig: &Rig) -> BetaPackageRepository {
    BetaPackageRepository::open(
        LockedPrivateNamespace::open_or_create(rig.root.path().join("repository")).unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn repository_custody_survives_scratch_teardown_and_reauthenticates_after_restart() {
    for runtime in [
        BetaRuntimeTarget::MacosNative,
        BetaRuntimeTarget::WindowsNative,
    ] {
        let rig = Rig::new().await;
        let package = Package::new(basic());
        let mut scratch = rig.workspace();
        let prepared = scratch
            .prepare(rig.source(&package, runtime), &package.crx)
            .unwrap();
        let mut repo = repository(&rig);
        let stored = repo.materialize(prepared).unwrap();
        let id = stored.id();
        assert_ne!(id.bytes(), [0; 32]);
        let provenance = stored
            .provenance(ExtensionProvenanceSource::ChromeWebStore)
            .unwrap();
        assert_eq!(
            repository::BetaPackageObjectId::from_provenance(&provenance),
            id
        );
        assert!(provenance.matches_manifest(stored.manifest().unwrap().descriptor()));
        assert_eq!(stored.previous_upstream().unwrap(), None);
        assert!(!stored.limitations().unwrap().is_empty());
        assert!(stored.retained_bytes() <= MAX_PREPARED_BETA_ARTIFACT_RETAINED_BYTES + 1024);
        scratch.discard().unwrap();
        drop(scratch);
        stored.verify().unwrap();
        assert_eq!(repo.inventory().unwrap(), vec![id]);
        drop(repo);
        assert_eq!(
            stored.verify(),
            Err(BetaRepositoryError::Admission(
                BetaArtifactPreparationError::Stale
            ))
        );
        drop(stored);
        let mut reopened = repository(&rig);
        let stored = reopened.reopen(id, rig.source(&package, runtime)).unwrap();
        stored.verify().unwrap();
        assert_eq!(
            stored
                .provenance(ExtensionProvenanceSource::ChromeWebStore)
                .unwrap(),
            provenance
        );
    }
}

#[tokio::test]
async fn repository_replay_preserves_the_existing_object_and_live_receipts() {
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    let rig = Rig::new().await;
    let package = Package::new(basic());
    let mut scratch = rig.workspace();
    let prepared = scratch
        .prepare(
            rig.source(&package, BetaRuntimeTarget::MacosNative),
            &package.crx,
        )
        .unwrap();
    let mut repo = repository(&rig);
    let first = repo.materialize(prepared).unwrap();
    #[cfg(unix)]
    let slot = fs::read_dir(rig.root.path().join("repository"))
        .unwrap()
        .filter_map(Result::ok)
        .find(|entry| entry.file_name().to_string_lossy().starts_with("beta-"))
        .unwrap()
        .path();
    #[cfg(unix)]
    let before = fs::metadata(slot.join("ready/extension/manifest.json"))
        .unwrap()
        .ino();
    let second = repo
        .materialize(
            scratch
                .reopen(rig.source(&package, BetaRuntimeTarget::MacosNative))
                .unwrap(),
        )
        .unwrap();
    assert_eq!(first.id(), second.id());
    first.verify().unwrap();
    second.verify().unwrap();
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(slot.join("ready/extension/manifest.json"))
            .unwrap()
            .ino(),
        before
    );
    assert_eq!(repo.inventory().unwrap().len(), 1);
}

#[tokio::test]
async fn custody_transfer_recovers_all_preparation_frontiers_without_promoting_partial_output() {
    for frontier in [
        TestFrontier::Original,
        TestFrontier::Tree,
        TestFrontier::Index,
        TestFrontier::Evidence,
        TestFrontier::Sealed,
        TestFrontier::Published,
    ] {
        let rig = Rig::new().await;
        let package = Package::new(basic());
        let mut scratch = rig.workspace();
        let prepared = scratch
            .prepare(
                rig.source(&package, BetaRuntimeTarget::MacosNative),
                &package.crx,
            )
            .unwrap();
        let destination =
            LockedPrivateNamespace::open_or_create(rig.root.path().join("destination")).unwrap();
        let child = destination
            .directory()
            .create_new_private_child(&component("slot").unwrap())
            .unwrap();
        let mut target = BetaPreparationWorkspace::open_in(child).unwrap();
        target.stop_at = Some(frontier);
        assert!(matches!(
            prepared.materialize_into(&mut target),
            Err(BetaArtifactPreparationError::Storage)
        ));
        drop(target);
        let child = destination
            .directory()
            .open_private_child(&component("slot").unwrap())
            .unwrap();
        let mut target = BetaPreparationWorkspace::open_in(child).unwrap();
        assert_eq!(
            target.exists("ready").unwrap(),
            frontier == TestFrontier::Published
        );
        let prepared = scratch
            .reopen(rig.source(&package, BetaRuntimeTarget::MacosNative))
            .unwrap();
        let owned = prepared.materialize_into(&mut target).unwrap();
        scratch.discard().unwrap();
        owned.verify().unwrap();
    }
}

#[tokio::test]
async fn repository_refuses_stale_source_without_allocating_a_slot() {
    let rig = Rig::new().await;
    let package = Package::new(basic());
    let mut scratch = rig.workspace();
    let prepared = scratch
        .prepare(
            rig.source(&package, BetaRuntimeTarget::MacosNative),
            &package.crx,
        )
        .unwrap();
    let mut repo = repository(&rig);
    drop(scratch);
    assert!(matches!(
        repo.materialize(prepared),
        Err(BetaRepositoryError::Admission(
            BetaArtifactPreparationError::Stale
        ))
    ));
    assert!(repo.inventory().unwrap().is_empty());
}

#[tokio::test]
async fn repository_unknown_inventory_quarantines_outstanding_receipts() {
    let rig = Rig::new().await;
    let package = Package::new(basic());
    let mut scratch = rig.workspace();
    let mut repo = repository(&rig);
    let stored = repo
        .materialize(
            scratch
                .prepare(
                    rig.source(&package, BetaRuntimeTarget::MacosNative),
                    &package.crx,
                )
                .unwrap(),
        )
        .unwrap();
    fs::write(rig.root.path().join("repository/unknown"), b"not an object").unwrap();
    assert_eq!(repo.inventory(), Err(BetaRepositoryError::Integrity));
    assert_eq!(repo.inventory(), Err(BetaRepositoryError::Quarantined));
    assert_eq!(
        stored.verify(),
        Err(BetaRepositoryError::Admission(
            BetaArtifactPreparationError::Stale
        ))
    );
}

#[cfg(unix)]
#[tokio::test]
async fn repository_restart_does_not_trust_or_replace_corrupted_ready_evidence() {
    let rig = Rig::new().await;
    let package = Package::new(basic());
    let mut scratch = rig.workspace();
    let mut repo = repository(&rig);
    let stored = repo
        .materialize(
            scratch
                .prepare(
                    rig.source(&package, BetaRuntimeTarget::MacosNative),
                    &package.crx,
                )
                .unwrap(),
        )
        .unwrap();
    let id = stored.id();
    drop(stored);
    drop(repo);
    let slot = fs::read_dir(rig.root.path().join("repository"))
        .unwrap()
        .filter_map(Result::ok)
        .find(|entry| entry.file_name().to_string_lossy().starts_with("beta-"))
        .unwrap()
        .path();
    let evidence = slot.join("ready/evidence.json");
    fs::set_permissions(&evidence, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&evidence, b"corrupt").unwrap();
    fs::set_permissions(&evidence, fs::Permissions::from_mode(0o400)).unwrap();
    let mut repo = repository(&rig);
    assert!(matches!(
        repo.reopen(id, rig.source(&package, BetaRuntimeTarget::MacosNative)),
        Err(BetaRepositoryError::Admission(_))
    ));
    assert_eq!(repo.inventory(), Err(BetaRepositoryError::Quarantined));
    assert_eq!(fs::read(evidence).unwrap(), b"corrupt");
}

#[tokio::test]
async fn repository_slot_bound_counts_inert_residue_before_writing() {
    let rig = Rig::new().await;
    let namespace =
        LockedPrivateNamespace::open_or_create(rig.root.path().join("repository")).unwrap();
    for index in 0..repository::MAX_BETA_REPOSITORY_SLOTS {
        namespace
            .directory()
            .create_new_private_child(&component(&format!("beta-{index:064x}")).unwrap())
            .unwrap();
    }
    let mut repo = BetaPackageRepository::open(namespace).unwrap();
    let package = Package::new(basic());
    let mut scratch = rig.workspace();
    let prepared = scratch
        .prepare(
            rig.source(&package, BetaRuntimeTarget::MacosNative),
            &package.crx,
        )
        .unwrap();
    assert!(matches!(
        repo.materialize(prepared),
        Err(BetaRepositoryError::Capacity)
    ));
    assert_eq!(
        repo.inventory().unwrap().len(),
        repository::MAX_BETA_REPOSITORY_SLOTS
    );
}

#[tokio::test]
async fn real_beta_package_provenance_and_grants_commit_together_and_reload_from_store() {
    use zephium_core::extensions::{
        ExtensionGrantAuthority, ExtensionGrantManifestBinding, ExtensionGrantManifestBindings,
        ExtensionInstall,
    };
    use zephium_core::ids::{ExtensionInstallId, ProfileId, SpaceId};
    use zephium_core::ports::store::{
        ExtensionGrantCohortLoadOutcome, ExtensionInstallCatalogLoadOutcome,
        ExtensionInstallProvisionOutcome, Store,
    };
    use zephium_core::session::{PersistedProfile, PersistedSpace, SessionState};
    use zephium_store::{ExtensionServiceStoreCallOutcome as Call, SqliteStore};
    let deadline = || std::time::Instant::now() + std::time::Duration::from_secs(10);
    let rig = Rig::new().await;
    let package = Package::new(basic());
    let mut scratch = rig.workspace();
    let mut repo = repository(&rig);
    let stored = repo
        .materialize(
            scratch
                .prepare(
                    rig.source(&package, BetaRuntimeTarget::MacosNative),
                    &package.crx,
                )
                .unwrap(),
        )
        .unwrap();
    let manifest = Arc::new(stored.manifest().unwrap().descriptor().clone());
    let provenance = Arc::new(
        stored
            .provenance(ExtensionProvenanceSource::ChromeWebStore)
            .unwrap(),
    );
    let profile = ProfileId::from(1);
    let store_path = rig.root.path().join("store");
    fs::create_dir(&store_path).unwrap();
    #[cfg(unix)]
    fs::set_permissions(&store_path, fs::Permissions::from_mode(0o700)).unwrap();
    let store = Arc::new(SqliteStore::open(&store_path).unwrap());
    store.save_session(SessionState {
        profiles: vec![PersistedProfile {
            id: profile,
            name: "Fixture".into(),
            kind: zephium_core::profiles::ProfileKind::Default,
        }],
        spaces: vec![PersistedSpace {
            id: SpaceId::from(2),
            profile,
            name: "Fixture".into(),
        }],
        ..SessionState::default()
    });
    assert!(store.flush_until(deadline()));
    let authority = store.claim_extension_service_store_authority().unwrap();
    let Call::Completed(ExtensionInstallCatalogLoadOutcome::Loaded(catalog)) =
        authority.load_install_catalog_until(profile, deadline())
    else {
        panic!("catalog unavailable");
    };
    let install_id = ExtensionInstallId::generate_after(catalog.install_id_high_water()).unwrap();
    // Explicit self-authored consent fixture: required declarations only.
    let grants = ExtensionGrantAuthority::initialize(
        &ExtensionInstall::new(install_id, manifest.package().clone()),
        manifest.declarations().required_api().names().to_vec(),
        manifest
            .declarations()
            .required_host_authorities()
            .into_iter()
            .cloned()
            .collect(),
        false,
        false,
        &manifest,
    )
    .unwrap();
    use zephium_core::ports::store::ExtensionUpstreamCheckpointLoadOutcome as Upstream;
    assert_eq!(
        authority.load_upstream_checkpoint_until(
            profile,
            provenance.upstream().publisher(),
            deadline()
        ),
        Call::Completed(Upstream::Loaded(None))
    );
    assert_eq!(
        authority.load_upstream_checkpoint_until(
            ProfileId::from(9999),
            provenance.upstream().publisher(),
            deadline()
        ),
        Call::Completed(Upstream::NotRegistered)
    );
    assert_eq!(
        authority.load_upstream_checkpoint_until(
            profile,
            provenance.upstream().publisher(),
            std::time::Instant::now()
        ),
        Call::NotAdmitted
    );
    stored.verify().unwrap();
    let Call::Completed(ExtensionInstallProvisionOutcome::Applied(applied)) = authority
        .provision_install_with_provenance_until(
            profile,
            catalog.revision(),
            install_id,
            manifest.clone(),
            Box::new(grants),
            Some(Box::new((*provenance).clone())),
            deadline(),
        )
    else {
        panic!("provision was not applied");
    };
    assert!(!applied.install.desired_enabled());
    assert_eq!(
        authority.load_upstream_checkpoint_until(
            profile,
            provenance.upstream().publisher(),
            deadline()
        ),
        Call::Completed(Upstream::Loaded(Some(provenance.upstream())))
    );
    assert!(
        ExtensionGrantManifestBindings::new(vec![ExtensionGrantManifestBinding::new(
            install_id,
            manifest.clone()
        )])
        .is_err()
    );
    let bindings = || {
        ExtensionGrantManifestBindings::new(vec![ExtensionGrantManifestBinding::with_provenance(
            install_id,
            manifest.clone(),
            provenance.clone(),
        )
        .unwrap()])
        .unwrap()
    };
    let Call::Completed(ExtensionGrantCohortLoadOutcome::Loaded(cohort)) =
        authority.load_grant_cohort_until(profile, bindings(), deadline())
    else {
        panic!("cohort unavailable");
    };
    assert_eq!(
        cohort.resolve_entry(install_id).unwrap().provenance(),
        Some(&*provenance)
    );
    drop(cohort);
    drop(authority);
    assert_eq!(
        store.shutdown_until(deadline()),
        zephium_core::ports::store::StoreShutdownOutcome::Clean
    );
    drop(store);
    let store = Arc::new(SqliteStore::open(&store_path).unwrap());
    let authority = store.claim_extension_service_store_authority().unwrap();
    assert_eq!(
        authority.load_upstream_checkpoint_until(
            profile,
            provenance.upstream().publisher(),
            deadline()
        ),
        Call::Completed(Upstream::Loaded(Some(provenance.upstream())))
    );
    let Call::Completed(ExtensionGrantCohortLoadOutcome::Loaded(cohort)) =
        authority.load_grant_cohort_until(profile, bindings(), deadline())
    else {
        panic!("persisted cohort unavailable");
    };
    assert_eq!(
        cohort.resolve_entry(install_id).unwrap().provenance(),
        Some(&*provenance)
    );
    // No native API is called in this fixture: this exercises the durable
    // pre-native protocol and the Beta/catalog separation, not execution.
    use zephium_core::extensions::{
        ExtensionBetaObjectDigest, ExtensionExpectedNativeOwnershipIdentity,
        ExtensionNativeOwnershipIntent as Intent,
        ExtensionNativeOwnershipJournalMutation as Mutation,
        ExtensionNativeOwnershipJournalRevision, ExtensionNativeOwnershipPhase as Phase,
        ExtensionNativeOwnershipPreparation, ExtensionNativePackageSource,
        ExtensionRuntimeBackendTarget as Backend,
    };
    use zephium_core::ports::store::{
        ExtensionInstallCatalogMutationOutcome,
        ExtensionNativeOwnershipActivationOutcome as Activation,
        ExtensionNativeOwnershipJournalLoadOutcome as JournalLoad,
        ExtensionNativeOwnershipJournalMutationOutcome as JournalMutation,
    };
    drop(cohort);
    assert!(matches!(
        authority.set_install_enabled_until(
            profile,
            applied.catalog_revision,
            install_id,
            applied.install.revision(),
            true,
            deadline()
        ),
        Call::Completed(ExtensionInstallCatalogMutationOutcome::Applied(_))
    ));
    let Call::Completed(ExtensionGrantCohortLoadOutcome::Loaded(cohort)) =
        authority.load_grant_cohort_until(profile, bindings(), deadline())
    else {
        panic!("enabled cohort unavailable");
    };
    let wrong_backend_package = repo
        .reopen_bound(
            rig.policy.clone(),
            &provenance,
            provenance.upstream(),
            BetaRuntimeTarget::MacosNative,
        )
        .unwrap();
    assert!(matches!(
        repo.prepare_native_ownership(
            wrong_backend_package,
            &cohort,
            install_id,
            Backend::WindowsNative
        ),
        Err(repository::BetaNativeAdmissionError::Backend)
    ));
    let admission: repository::BetaNativeOwnershipAdmission = repo
        .prepare_native_ownership(stored, &cohort, install_id, Backend::MacosNative)
        .unwrap();
    let preparation = admission.preparation().clone();
    assert_eq!(
        preparation.source(),
        ExtensionNativePackageSource::BetaObject(ExtensionBetaObjectDigest::from_provenance(
            &provenance
        ))
    );
    let forged = ExtensionNativeOwnershipPreparation::beta(
        preparation.key(),
        preparation.package().clone(),
        ExtensionBetaObjectDigest::from_bytes([0; 32]),
        preparation.store_catalog_revision(),
        preparation.store_install_revision(),
        preparation.store_grant_revision(),
        preparation.grant_digest(),
        preparation.runtime_backend(),
    )
    .unwrap();
    assert_eq!(
        authority.begin_native_ownership_until(
            ExtensionNativeOwnershipJournalRevision::INITIAL,
            Mutation::begin(forged),
            manifest.clone(),
            deadline()
        ),
        Call::Completed(Activation::Invalid)
    );
    let Call::Completed(Activation::Applied(preparing)) = authority.begin_native_ownership_until(
        ExtensionNativeOwnershipJournalRevision::INITIAL,
        Mutation::begin(preparation),
        manifest.clone(),
        deadline(),
    ) else {
        panic!("Beta Begin unavailable");
    };
    let preparing_entry = preparing.entry.as_deref().unwrap();
    let pin: repository::BetaNativePackagePin = admission.bind_preparing(preparing_entry).unwrap();
    assert_eq!(
        repo.collect_unreferenced(&[], deadline())
            .unwrap()
            .removed(),
        0,
        "live native reservations must independently retain their source"
    );
    let duplicate_package = repo
        .reopen_bound(
            rig.policy.clone(),
            &provenance,
            provenance.upstream(),
            BetaRuntimeTarget::MacosNative,
        )
        .unwrap();
    let duplicate = repo
        .prepare_native_ownership(duplicate_package, &cohort, install_id, Backend::MacosNative)
        .unwrap();
    assert!(matches!(
        duplicate.bind_preparing(preparing_entry),
        Err(repository::BetaNativeAdmissionError::InUse)
    ));
    assert_eq!(
        pin.object(),
        ExtensionBetaObjectDigest::from_provenance(&provenance)
    );
    let mut wrong_identity = pin.expected_native_identity().bytes();
    wrong_identity[0] = if wrong_identity[0] == b'a' {
        b'b'
    } else {
        b'a'
    };
    let wrong_identity = ExtensionExpectedNativeOwnershipIdentity::from_encoded_bytes(
        Backend::MacosNative,
        wrong_identity,
    )
    .unwrap();
    assert_eq!(
        authority.transition_native_ownership_to_may_own_until(
            preparing.journal_revision,
            preparing_entry.cas(),
            Some(wrong_identity),
            manifest.clone(),
            deadline()
        ),
        Call::Completed(Activation::Invalid)
    );
    let Call::Completed(Activation::Applied(may_own)) = authority
        .transition_native_ownership_to_may_own_until(
            preparing.journal_revision,
            preparing_entry.cas(),
            Some(pin.expected_native_identity()),
            manifest.clone(),
            deadline(),
        )
    else {
        panic!("Beta MayOwn unavailable");
    };
    pin.verify_may_own(may_own.entry.as_deref().unwrap())
        .unwrap();
    drop(cohort);
    drop(authority);
    assert_eq!(
        store.shutdown_until(deadline()),
        zephium_core::ports::store::StoreShutdownOutcome::Clean
    );
    drop(store);
    let store = Arc::new(SqliteStore::open(&store_path).unwrap());
    let authority = store.claim_extension_service_store_authority().unwrap();
    assert_eq!(authority.startup_requirement(),zephium_store::ExtensionServiceStoreStartupRequirement::NativeOwnershipReconciliationRequired);
    let Call::Completed(JournalLoad::Loaded(journal)) =
        authority.load_native_ownership_until(deadline())
    else {
        panic!("Beta journal reload unavailable");
    };
    let row = journal.entries().first().unwrap();
    assert_eq!(row, may_own.entry.as_deref().unwrap());
    pin.verify_may_own(row).unwrap();
    use zephium_extension_runtime_api::{
        ExtensionRuntimeActivationSettlement, ExtensionRuntimeHostFactory,
        ExtensionRuntimeVisitorError, MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES,
    };
    let mut access = pin.into_runtime_access().unwrap();
    assert!(access.retained_bytes() < MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);
    assert_eq!(
        Some(access.expected_native_identity()),
        row.expected_native_identity()
    );
    let mut partial = |reader: &mut dyn std::io::Read| {
        let mut first = [0];
        reader.read_exact(&mut first).unwrap();
        assert_eq!(
            repo.inventory().unwrap_err(),
            BetaRepositoryError::CallbackReentry
        );
        Err(ExtensionRuntimeVisitorError::ConsumerUnavailable)
    };
    assert_eq!(
        access.test_access().visit_manifest(&mut partial).unwrap(),
        Err(ExtensionRuntimeVisitorError::ConsumerUnavailable)
    );
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut refusing_factory =
        ExtensionRuntimeHostFactory::from_trusted_port(Box::new(native_fixture::Factory {
            calls: Arc::clone(&calls),
            refuse: true,
        }));
    let generation = zephium_core::extensions::ExtensionRuntimeGeneration::new(1).unwrap();
    let refusal = access
        .try_into_host_activation(
            row.clone(),
            generation,
            &mut refusing_factory,
            usize::MAX,
            0,
        )
        .unwrap_err();
    assert_eq!(
        refusal.reason(),
        repository::BetaNativeAdmissionError::Capacity
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 0);
    let (pin, _) = refusal.try_into_pin().unwrap();
    let refusal = pin
        .into_runtime_access()
        .unwrap()
        .try_into_host_activation(row.clone(), generation, &mut refusing_factory, 0, 0)
        .unwrap_err();
    assert_eq!(
        refusal.reason(),
        repository::BetaNativeAdmissionError::Ownership
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    let (pin, _) = refusal.try_into_pin().unwrap();
    let mut factory =
        ExtensionRuntimeHostFactory::from_trusted_port(Box::new(native_fixture::Factory {
            calls: Arc::clone(&calls),
            refuse: false,
        }));
    let host = pin
        .into_runtime_access()
        .unwrap()
        .try_into_host_activation(row.clone(), generation, &mut factory, 0, 0)
        .unwrap();
    assert!(host.retained_bytes() < MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES);
    let (activation, recovery) = host.into_parts();
    let (request, pending) = activation.into_parts();
    let ExtensionRuntimeActivationSettlement::Rejected { access, .. } =
        request.settle_until(deadline())
    else {
        panic!("root-only fixture must refuse before OS-native execution");
    };
    // Self-authored fixture evidence: no native call occurred anywhere above.
    let Call::Completed(JournalMutation::Applied(released)) = authority
        .mutate_native_ownership_until(
            journal.revision(),
            Mutation::Transition {
                expected: row.cas(),
                intent: Intent::Release,
                phase: Phase::NativeAbsentReleasePending,
                attach_expected_native_identity: None,
                attach_native_identity: None,
            },
            deadline(),
        )
    else {
        panic!("Beta release frontier unavailable");
    };
    let operation = pending
        .recover_after_absence(released.entry.as_deref().unwrap())
        .unwrap();
    let pin = recovery.rejoin(access, operation).unwrap();
    assert!(matches!(
        pin.verify_may_own(released.entry.as_deref().unwrap()),
        Err(repository::BetaNativeAdmissionError::Ownership)
    ));
    let release_binding = zephium_core::extensions::ExtensionPackagePinReleaseBinding::mint(
        released.entry.as_deref().unwrap(),
    )
    .unwrap();
    repo.release_native_pin(pin, &release_binding).unwrap();
    // A same-open settlement is a tombstone, not permission to reuse a copied
    // old preparing row. Reconstruct fresh bytes/cohort to isolate this guard.
    let Call::Completed(ExtensionGrantCohortLoadOutcome::Loaded(fresh_cohort)) =
        authority.load_grant_cohort_until(profile, bindings(), deadline())
    else {
        panic!("fresh cohort unavailable");
    };
    let replay_package = repo
        .reopen_bound(
            rig.policy.clone(),
            &provenance,
            provenance.upstream(),
            BetaRuntimeTarget::MacosNative,
        )
        .unwrap();
    let replay = repo
        .prepare_native_ownership(
            replay_package,
            &fresh_cohort,
            install_id,
            Backend::MacosNative,
        )
        .unwrap();
    assert!(matches!(
        replay.bind_preparing(preparing_entry),
        Err(repository::BetaNativeAdmissionError::Ownership)
    ));
    let Call::Completed(JournalMutation::Applied(cleared)) = authority
        .mutate_native_ownership_until(
            released.journal_revision,
            Mutation::Clear {
                expected: released.entry.as_deref().unwrap().cas(),
            },
            deadline(),
        )
    else {
        panic!("Beta clear unavailable");
    };
    assert!(cleared.entry.is_none());
    drop(authority);
    assert_eq!(
        store.shutdown_until(deadline()),
        zephium_core::ports::store::StoreShutdownOutcome::Clean
    );
}

#[tokio::test]
async fn repository_reconstructs_from_saved_crx_and_preserves_historical_store_binding() {
    let mut rig = Rig::new().await;
    let package = Package::new(basic());
    let mut scratch = rig.workspace();
    let mut repo = repository(&rig);
    let stored = repo
        .materialize(
            scratch
                .prepare(
                    rig.source(&package, BetaRuntimeTarget::MacosNative),
                    &package.crx,
                )
                .unwrap(),
        )
        .unwrap();
    let expected = stored
        .provenance(ExtensionProvenanceSource::ChromeWebStore)
        .unwrap();
    assert!(stored.persisted_provenance().unwrap().is_none());
    drop(stored);
    drop(repo);
    scratch.discard().unwrap();
    drop(scratch);
    drop(package);
    rig.fixture.policy["policy_revision"] = json!(2);
    rig.fixture.publish(2);
    rig.policy = Arc::new(rig.cache.refresh(&rig.fixture.client()).await.unwrap());
    let mut repo = repository(&rig);
    let restored = repo
        .reopen_bound(
            rig.policy.clone(),
            &expected,
            expected.upstream(),
            BetaRuntimeTarget::MacosNative,
        )
        .unwrap();
    restored.verify().unwrap();
    assert_eq!(restored.persisted_provenance().unwrap(), Some(&expected));
    let current = restored
        .provenance(ExtensionProvenanceSource::ChromeWebStore)
        .unwrap();
    assert_eq!(current.policy().revision.get(), 2);
    assert_eq!(expected.policy().revision.get(), 1);
    assert_eq!(current.package(), expected.package());
    assert_eq!(current.upstream(), expected.upstream());
}

#[tokio::test]
async fn repository_saved_source_cannot_change_runtime_or_rollback_high_water() {
    for wrong_runtime in [false, true] {
        let rig = Rig::new().await;
        let package = Package::new(basic());
        let mut scratch = rig.workspace();
        let mut repo = repository(&rig);
        let stored = repo
            .materialize(
                scratch
                    .prepare(
                        rig.source(&package, BetaRuntimeTarget::MacosNative),
                        &package.crx,
                    )
                    .unwrap(),
            )
            .unwrap();
        let expected = stored
            .provenance(ExtensionProvenanceSource::ChromeWebStore)
            .unwrap();
        drop(stored);
        drop(repo);
        let mut repo = repository(&rig);
        let high_water = if wrong_runtime {
            expected.upstream()
        } else {
            zephium_core::extensions::ExtensionUpstreamCheckpoint::from_parts(
                expected.upstream().publisher(),
                zephium_core::extensions::ExtensionUpstreamVersion::parse("2").unwrap(),
                [9; 32],
                [10; 32],
            )
        };
        let runtime = if wrong_runtime {
            BetaRuntimeTarget::WindowsNative
        } else {
            BetaRuntimeTarget::MacosNative
        };
        assert!(repo
            .reopen_bound(rig.policy.clone(), &expected, high_water, runtime)
            .is_err());
        assert_eq!(repo.inventory(), Err(BetaRepositoryError::Quarantined));
    }
}

#[tokio::test]
async fn repository_object_address_distinguishes_original_crx_envelopes_with_the_same_zip() {
    let rig = Rig::new().await;
    let mut package = Package::new(basic());
    let mut scratch = rig.workspace();
    let mut repo = repository(&rig);
    let first = repo
        .materialize(
            scratch
                .prepare(
                    rig.source(&package, BetaRuntimeTarget::MacosNative),
                    &package.crx,
                )
                .unwrap(),
        )
        .unwrap();
    let before = first
        .provenance(ExtensionProvenanceSource::ChromeWebStore)
        .unwrap();
    scratch.discard().unwrap();
    package.set_manifest(basic()); // same ZIP, fresh randomized ECDSA signature
    let second = repo
        .materialize(
            scratch
                .prepare(
                    rig.source(&package, BetaRuntimeTarget::MacosNative),
                    &package.crx,
                )
                .unwrap(),
        )
        .unwrap();
    let after = second
        .provenance(ExtensionProvenanceSource::ChromeWebStore)
        .unwrap();
    assert_eq!(before.package(), after.package());
    assert_eq!(
        before.upstream().archive_sha256(),
        after.upstream().archive_sha256()
    );
    assert_ne!(
        before.upstream().original_crx_sha256(),
        after.upstream().original_crx_sha256()
    );
    assert_ne!(first.id(), second.id());
    assert_eq!(
        repository::BetaPackageObjectId::from_provenance(&before),
        first.id()
    );
    assert_eq!(
        repository::BetaPackageObjectId::from_provenance(&after),
        second.id()
    );
    first.verify().unwrap();
    second.verify().unwrap();
    assert_eq!(repo.inventory().unwrap().len(), 2);
}

#[tokio::test]
async fn local_brokered_source_compiles_and_reopens_without_verified_or_signed_admission() {
    let rig = Rig::new().await;
    let mut manifest = basic();
    manifest["permissions"] = json!([
        "storage",
        "history",
        "bookmarks",
        "tabs",
        "sessions",
        "search",
        "favicon",
        "notifications",
        "webNavigation"
    ]);
    manifest["background"]["type"] = json!("module");
    manifest["action"]["default_popup"] = json!("real-popup.html");
    let mut package = Package::new(manifest.clone());
    package.set_manifest_for_compatibility(
        manifest,
        &[(
            "real-popup.html",
            b"<!doctype html><html><head><title>Fixture</title></head><body>Fixture</body></html>",
        )],
    );
    assert!(admit_beta_source(
        Arc::clone(&rig.policy),
        package.source(),
        &package.manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::INITIAL,
        None
    )
    .is_err());
    let source = crate::beta::admit_external_source(
        package.source(),
        &package.manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::INITIAL,
        None,
    )
    .unwrap();
    let mut workspace = rig.workspace();
    let artifact = workspace.prepare_external(source, &package.crx).unwrap();
    artifact.verify().unwrap();
    assert!(artifact.blueprint.replacements.is_empty());
    let provenance = artifact
        .provenance(ExtensionProvenanceSource::ChromeWebStore)
        .unwrap();
    assert_eq!(
        provenance.runtime_target().as_str(),
        zephium_core::extensions::LOCAL_MACOS_HISTORY_V3_COMPATIBILITY_TARGET
    );
    assert!(
        matches!(provenance.transform(), ExtensionTransformProvenance::Compiled { target, .. } if target.as_str() == "local.webkit-history.v3")
    );
    assert_eq!(
        ExtensionInstallProvenance::decode(&provenance.encode()),
        Some(provenance.clone())
    );
    assert_eq!(
        fs::read_to_string(
            rig.ready()
                .join("extension/__zephium__/webkit-history-v1.js")
        )
        .unwrap(),
        HISTORY_V2,
        "the prepared extension must execute the query-aware adapter, not merely hash it"
    );
    assert!(
        fs::read_to_string(rig.ready().join("extension/real-popup.html"))
            .unwrap()
            .contains("webkit-runtime-messaging-v1.js")
    );
    assert_eq!(
        fs::read_to_string(
            rig.ready()
                .join("extension")
                .join(compiler::SESSIONS_BRIDGE)
        )
        .unwrap(),
        compiler::SESSIONS_BRIDGE_SOURCE
    );
    assert!(!rig
        .ready()
        .join("extension")
        .join(compiler::SESSIONS_BRIDGE_V2)
        .exists());
    assert_eq!(
        fs::read(rig.ready().join("extension/background.js")).unwrap(),
        b"void 0;"
    );
    drop(artifact);
    drop(workspace);
    let workspace = rig.workspace();
    let reopened = workspace
        .reopen_external_bound(
            &provenance,
            provenance.upstream(),
            BetaRuntimeTarget::MacosNative,
        )
        .unwrap();
    reopened.verify().unwrap();
    assert_eq!(
        fs::read_to_string(
            rig.ready()
                .join("extension/__zephium__/webkit-history-v1.js")
        )
        .unwrap(),
        HISTORY_V2,
    );
    assert!(workspace
        .reopen_external_bound(
            &provenance,
            provenance.upstream(),
            BetaRuntimeTarget::WindowsNative
        )
        .is_err());
}

#[tokio::test]
async fn declared_search_and_sessions_compile_without_history_storage_or_background() {
    let rig = Rig::new().await;
    let mut manifest = basic();
    manifest["permissions"] = json!(["search", "sessions"]);
    manifest.as_object_mut().unwrap().remove("background");
    let mut package = Package::new(manifest.clone());
    package.set_manifest_for_compatibility(manifest, &[]);
    let source = crate::beta::admit_external_source(
        package.source(),
        &package.manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::INITIAL,
        None,
    )
    .unwrap();
    assert_eq!(
        source
            .manifest()
            .descriptor()
            .compatibility_target()
            .as_str(),
        zephium_core::extensions::LOCAL_MACOS_CAPABILITIES_V1_COMPATIBILITY_TARGET
    );
    let mut workspace = rig.workspace();
    let artifact = workspace.prepare_external(source, &package.crx).unwrap();
    artifact.verify().unwrap();
    let output: Value =
        serde_json::from_slice(&fs::read(rig.ready().join("extension/manifest.json")).unwrap())
            .unwrap();
    assert_eq!(
        output["permissions"],
        json!(["search", "sessions", "nativeMessaging"])
    );
    assert!(output.get("background").is_none());
    assert!(!rig
        .ready()
        .join("extension/__zephium_background_v1.js")
        .exists());
    for bridge in [compiler::SEARCH_BRIDGE, compiler::SESSIONS_BRIDGE_V2] {
        assert!(rig.ready().join("extension").join(bridge).is_file());
        assert!(fs::read_to_string(rig.ready().join("extension/popup.html"))
            .unwrap()
            .contains(bridge));
    }
    assert_eq!(
        fs::read_to_string(
            rig.ready()
                .join("extension")
                .join(compiler::SESSIONS_BRIDGE_V2)
        )
        .unwrap(),
        compiler::SESSIONS_BRIDGE_V2_SOURCE
    );
    for bridge in [
        compiler::HISTORY_BRIDGE,
        compiler::RUNTIME_MESSAGING_BRIDGE,
        compiler::SESSIONS_BRIDGE,
    ] {
        assert!(!rig.ready().join("extension").join(bridge).exists());
    }
    let provenance = artifact
        .provenance(ExtensionProvenanceSource::ChromeWebStore)
        .unwrap();
    assert!(
        matches!(provenance.transform(), ExtensionTransformProvenance::Compiled { target, .. } if target.as_str() == "local.webkit-capabilities.v1")
    );
    drop(artifact);
    drop(workspace);
    rig.workspace()
        .reopen_external_bound(
            &provenance,
            provenance.upstream(),
            BetaRuntimeTarget::MacosNative,
        )
        .unwrap()
        .verify()
        .unwrap();
}

#[tokio::test]
async fn history_without_storage_or_worker_uses_only_the_real_history_bridge() {
    let rig = Rig::new().await;
    let mut manifest = basic();
    manifest["permissions"] = json!(["history"]);
    manifest.as_object_mut().unwrap().remove("background");
    let mut package = Package::new(manifest.clone());
    package.set_manifest_for_compatibility(manifest, &[]);
    let source = crate::beta::admit_external_source(
        package.source(),
        &package.manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::INITIAL,
        None,
    )
    .unwrap();
    let mut workspace = rig.workspace();
    let artifact = workspace.prepare_external(source, &package.crx).unwrap();
    artifact.verify().unwrap();
    let output: Value =
        serde_json::from_slice(&fs::read(rig.ready().join("extension/manifest.json")).unwrap())
            .unwrap();
    assert_eq!(output["permissions"], json!(["history", "nativeMessaging"]));
    assert!(output.get("background").is_none());
    assert_eq!(
        fs::read_to_string(rig.ready().join("extension").join(compiler::HISTORY_BRIDGE)).unwrap(),
        HISTORY_V2
    );
    for bridge in [
        compiler::RUNTIME_MESSAGING_BRIDGE,
        compiler::SEARCH_BRIDGE,
        compiler::SESSIONS_BRIDGE,
        compiler::SESSIONS_BRIDGE_V2,
        compiler::BOOKMARKS_BRIDGE,
        compiler::FAVICON_BRIDGE,
    ] {
        assert!(!rig.ready().join("extension").join(bridge).exists());
    }
    let provenance = artifact
        .provenance(ExtensionProvenanceSource::ChromeWebStore)
        .unwrap();
    drop(artifact);
    drop(workspace);
    rig.workspace()
        .reopen_external_bound(
            &provenance,
            provenance.upstream(),
            BetaRuntimeTarget::MacosNative,
        )
        .unwrap()
        .verify()
        .unwrap();
}

#[tokio::test]
async fn new_capability_profile_withholds_optional_facades_and_rejects_required_ones() {
    let rig = Rig::new().await;
    let mut manifest = basic();
    manifest["permissions"] = json!(["search"]);
    manifest["optional_permissions"] = json!(["bookmarks", "favicon", "sessions"]);
    let mut package = Package::new(manifest.clone());
    package.set_manifest_for_compatibility(manifest.clone(), &[]);
    let source = crate::beta::admit_external_source(
        package.source(),
        &package.manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::INITIAL,
        None,
    )
    .unwrap();
    let mut workspace = rig.workspace();
    let artifact = workspace.prepare_external(source, &package.crx).unwrap();
    let withheld: Vec<_> = artifact
        .withheld_optional_permissions()
        .iter()
        .map(|name| name.as_str())
        .collect();
    assert!(withheld.contains(&"bookmarks"));
    assert!(withheld.contains(&"favicon"));
    assert!(withheld.contains(&"sessions"));
    let output: Value =
        serde_json::from_slice(&fs::read(rig.ready().join("extension/manifest.json")).unwrap())
            .unwrap();
    assert_eq!(output["optional_permissions"], json!([]));
    for bridge in [
        compiler::BOOKMARKS_BRIDGE,
        compiler::FAVICON_BRIDGE,
        compiler::SESSIONS_BRIDGE_V2,
    ] {
        assert!(!rig.ready().join("extension").join(bridge).exists());
    }
    drop(artifact);
    drop(workspace);

    manifest["permissions"] = json!(["search", "bookmarks"]);
    manifest
        .as_object_mut()
        .unwrap()
        .remove("optional_permissions");
    package.set_manifest_for_compatibility(manifest, &[]);
    assert!(matches!(
        crate::beta::admit_external_source(
            package.source(),
            &package.manifest,
            BetaRuntimeTarget::MacosNative,
            ExtensionPackageRevision::INITIAL,
            None,
        ),
        Err(crate::beta::BetaSourceAdmissionError::Unsupported(_))
    ));
}

#[tokio::test]
async fn unicode_web_accessible_resource_keeps_its_exact_declared_spelling() {
    let rig = Rig::new().await;
    let path = "src/js/сlickableCard.common.chunk.js";
    let mut manifest = basic();
    manifest["permissions"] = json!(["search"]);
    manifest["web_accessible_resources"] = json!([{
        "resources": [path],
        "matches": ["https://example.test/*"]
    }]);
    let mut package = Package::new(manifest.clone());
    package.set_manifest_for_compatibility(manifest, &[(path, b"void 0;")]);
    let source = crate::beta::admit_external_source(
        package.source(),
        &package.manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::INITIAL,
        None,
    )
    .unwrap();
    let mut workspace = rig.workspace();
    let artifact = workspace.prepare_external(source, &package.crx).unwrap();
    artifact.verify().unwrap();
    let output: Value =
        serde_json::from_slice(&fs::read(rig.ready().join("extension/manifest.json")).unwrap())
            .unwrap();
    assert_eq!(output["web_accessible_resources"][0]["resources"][0], path);
    assert_eq!(
        fs::read(rig.ready().join("extension").join(path)).unwrap(),
        b"void 0;"
    );
}

#[tokio::test]
async fn capability_broker_requires_a_source_realm_and_refuses_source_native_messaging() {
    let rig = Rig::new().await;
    let mut manifest = basic();
    manifest["permissions"] = json!(["search"]);
    manifest.as_object_mut().unwrap().remove("background");
    manifest.as_object_mut().unwrap().remove("action");
    let mut package = Package::new(manifest.clone());
    package.set_manifest_for_compatibility(manifest.clone(), &[]);
    let source = crate::beta::admit_external_source(
        package.source(),
        &package.manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::INITIAL,
        None,
    )
    .unwrap();
    let mut workspace = rig.workspace();
    assert!(matches!(
        workspace.prepare_external(source, &package.crx),
        Err(BetaArtifactPreparationError::Transform)
    ));
    assert!(!rig.ready().exists());

    manifest["permissions"] = json!(["search", "nativeMessaging"]);
    package.set_manifest_for_compatibility(manifest, &[]);
    assert!(matches!(
        crate::beta::admit_external_source(
            package.source(),
            &package.manifest,
            BetaRuntimeTarget::MacosNative,
            ExtensionPackageRevision::INITIAL,
            None,
        ),
        Err(crate::beta::BetaSourceAdmissionError::Unsupported(_))
    ));
}

#[tokio::test]
async fn local_brokered_transform_refuses_reserved_source_resources() {
    let rig = Rig::new().await;
    let mut manifest = basic();
    manifest["permissions"] = json!(["storage", "history"]);
    let mut package = Package::new(manifest.clone());
    package.set_manifest_with_extra(
        manifest,
        &[("__zephium__/webkit-history-v1.js", b"hostile replacement")],
    );
    let source = crate::beta::admit_external_source(
        package.source(),
        &package.manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::INITIAL,
        None,
    )
    .unwrap();
    assert_eq!(
        rig.workspace()
            .prepare_external(source, &package.crx)
            .unwrap_err(),
        BetaArtifactPreparationError::Transform
    );
}

#[tokio::test]
async fn sparse_orphan_storage_is_bounded_and_can_be_reclaimed_without_payload_reads() {
    let rig = Rig::new().await;
    let root = rig.root.path().join("quota-repository");
    fs::create_dir(&root).unwrap();
    #[cfg(unix)]
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let orphan = root.join(format!("beta-{}", "a".repeat(64)));
    fs::create_dir(&orphan).unwrap();
    #[cfg(unix)]
    fs::set_permissions(&orphan, fs::Permissions::from_mode(0o700)).unwrap();
    for index in 0..9 {
        let path = orphan.join(format!("part-{index}"));
        let file = fs::File::create(&path).unwrap();
        file.set_len(64 * 1024 * 1024).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut repo =
        BetaPackageRepository::open(LockedPrivateNamespace::open_or_create(root).unwrap()).unwrap();
    assert_eq!(repo.storage_usage(), Err(BetaRepositoryError::Capacity));
    let package = Package::new(basic());
    let mut workspace = rig.workspace();
    let artifact = workspace
        .prepare(
            rig.source(&package, BetaRuntimeTarget::MacosNative),
            &package.crx,
        )
        .unwrap();
    assert!(matches!(
        repo.materialize(artifact),
        Err(BetaRepositoryError::Capacity)
    ));
    assert_eq!(
        repo.collect_unreferenced(
            &[],
            std::time::Instant::now() + std::time::Duration::from_secs(10)
        )
        .unwrap()
        .removed(),
        1
    );
    assert_eq!(repo.storage_usage().unwrap().bytes(), 0);
    assert_eq!(repo.storage_usage().unwrap().entries(), 0);
}

#[tokio::test]
async fn collection_handles_the_deepest_admitted_tree_without_widening_filesystem_limits() {
    let rig = Rig::new().await;
    let path = format!("{}leaf.txt", "d/".repeat(31));
    let mut package = Package::new(basic());
    package.set_manifest_with_extra(basic(), &[(path.as_str(), b"deep resource")]);
    let mut workspace = rig.workspace();
    let artifact = workspace
        .prepare(
            rig.source(&package, BetaRuntimeTarget::MacosNative),
            &package.crx,
        )
        .unwrap();
    let mut repo = BetaPackageRepository::open(
        LockedPrivateNamespace::open_or_create(rig.root.path().join("deep-repository")).unwrap(),
    )
    .unwrap();
    let stored = repo.materialize(artifact).unwrap();
    let result = repo
        .collect_unreferenced(
            &[],
            std::time::Instant::now() + std::time::Duration::from_secs(10),
        )
        .unwrap();
    assert_eq!(result.removed(), 1);
    assert!(repo.inventory().unwrap().is_empty());
    assert!(
        stored.verify().is_err(),
        "an uninstalled stale receipt cannot survive collection"
    );
}

#[tokio::test]
async fn local_capabilities_select_generic_adapters_and_reopen_without_native_host_authority() {
    let rig = Rig::new().await;
    let mut manifest = basic();
    manifest["permissions"] = json!([
        "storage",
        "cookies",
        "privacy",
        "notifications",
        "webNavigation"
    ]);
    manifest["background"]["type"] = json!("module");
    manifest["content_security_policy"] =
        json!({"extension_pages":"script-src 'self' 'wasm-unsafe-eval'; object-src 'self'"});
    let mut package = Package::new(manifest.clone());
    package.set_manifest_for_compatibility(manifest, &[]);
    let source = crate::beta::admit_external_source(
        package.source(),
        &package.manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::INITIAL,
        None,
    )
    .unwrap();
    assert_eq!(
        source
            .manifest()
            .descriptor()
            .compatibility_target()
            .as_str(),
        zephium_core::extensions::LOCAL_MACOS_ADAPTED_COMPATIBILITY_TARGET
    );
    let mut workspace = rig.workspace();
    let artifact = workspace.prepare_external(source, &package.crx).unwrap();
    artifact.verify().unwrap();
    assert!(artifact.publisher_native_host().is_none());
    assert_eq!(
        artifact
            .manifest()
            .unwrap()
            .descriptor()
            .declarations()
            .background()
            .unwrap()
            .environment(),
        zephium_core::extensions::ExtensionBackgroundEnvironment::Document
    );
    assert!(rig
        .ready()
        .join("extension/__zephium__/webkit-privacy-services-v1.js")
        .is_file());
    let provenance = artifact
        .provenance(ExtensionProvenanceSource::ChromeWebStore)
        .unwrap();
    assert_eq!(
        ExtensionInstallProvenance::decode(&provenance.encode()),
        Some(provenance.clone())
    );
    drop(artifact);
    drop(workspace);
    let workspace = rig.workspace();
    let reopened = workspace
        .reopen_external_bound(
            &provenance,
            provenance.upstream(),
            BetaRuntimeTarget::MacosNative,
        )
        .unwrap();
    reopened.verify().unwrap();
    assert!(reopened.publisher_native_host().is_none());
    assert!(workspace
        .reopen_external_bound(
            &provenance,
            provenance.upstream(),
            BetaRuntimeTarget::WindowsNative
        )
        .is_err());
}

#[tokio::test]
async fn cross_browser_background_and_implicit_html_reopen_with_exact_source_on_both_targets() {
    for target in [
        BetaRuntimeTarget::MacosNative,
        BetaRuntimeTarget::WindowsNative,
    ] {
        let rig = Rig::new().await;
        let mut manifest = basic();
        manifest["background"] =
            json!({"type":"module","service_worker":"background.js","scripts":["background.js"]});
        manifest["$schema"] = json!("https://json.schemastore.org/chrome-manifest");
        manifest["browser_specific_settings"] =
            json!({"gecko":{"id":"fixture@example.test","strict_min_version":"128.0"}});
        let package = Package::new(manifest);
        assert!(admit_beta_source(
            Arc::clone(&rig.policy),
            package.source(),
            &package.manifest,
            target,
            ExtensionPackageRevision::INITIAL,
            None
        )
        .is_err());
        let source = crate::beta::admit_external_source(
            package.source(),
            &package.manifest,
            target,
            ExtensionPackageRevision::INITIAL,
            None,
        )
        .unwrap();
        let mut workspace = rig.workspace();
        let artifact = workspace.prepare_external(source, &package.crx).unwrap();
        artifact.verify().unwrap();
        let output: serde_json::Value =
            serde_json::from_slice(&fs::read(rig.ready().join("extension/manifest.json")).unwrap())
                .unwrap();
        if target == BetaRuntimeTarget::MacosNative {
            assert_eq!(
                output["background"]["preferred_environment"],
                json!(["document", "service_worker"])
            );
            assert!(fs::read_to_string(rig.ready().join("extension/popup.html"))
                .unwrap()
                .contains("<head><script src=\"/__zephium__/webkit-api-v1.js\""));
            assert!(
                matches!(artifact.transformation(), ExtensionTransformProvenance::Compiled { target, revision, .. } if target.as_str() == "local.webkit-adapted.v1" && revision.get() == 3)
            );
        } else {
            assert_eq!(
                output["background"],
                json!({"type":"module","service_worker":"background.js"})
            );
            assert_eq!(
                fs::read(rig.ready().join("extension/popup.html")).unwrap(),
                b"<!doctype html><p>Fixture</p>"
            );
            assert!(
                matches!(artifact.transformation(), ExtensionTransformProvenance::Compiled { target, revision, .. } if target.as_str() == "local.cross-browser-background.v1" && revision.get() == 1)
            );
        }
        assert_eq!(
            fs::read(rig.ready().join("original.crx")).unwrap(),
            package.crx
        );
        assert_eq!(
            fs::read(rig.ready().join("extension/background.js")).unwrap(),
            b"void 0;"
        );
        let provenance = artifact
            .provenance(ExtensionProvenanceSource::ChromeWebStore)
            .unwrap();
        assert_eq!(
            ExtensionInstallProvenance::decode(&provenance.encode()),
            Some(provenance.clone())
        );
        drop(artifact);
        drop(workspace);
        rig.workspace()
            .reopen_external_bound(&provenance, provenance.upstream(), target)
            .unwrap()
            .verify()
            .unwrap();
    }
}

#[test]
fn local_native_messaging_requires_publisher_binding_not_a_manifest_name() {
    let mut manifest = basic();
    manifest["name"] = json!("1Password");
    manifest["permissions"] = json!(["storage", "nativeMessaging", "privacy"]);
    let package = Package::new(manifest);
    assert!(matches!(
        crate::beta::admit_external_source(
            package.source(),
            &package.manifest,
            BetaRuntimeTarget::MacosNative,
            ExtensionPackageRevision::INITIAL,
            None,
        ),
        Err(crate::beta::BetaSourceAdmissionError::Unsupported(_))
    ));
}

#[tokio::test]
async fn local_bounded_storage_preserves_source_and_reopens_on_each_native_target() {
    for target in [
        BetaRuntimeTarget::MacosNative,
        BetaRuntimeTarget::WindowsNative,
    ] {
        let rig = Rig::new().await;
        let mut manifest = basic();
        manifest["permissions"] = json!(["storage", "unlimitedStorage"]);
        manifest["background"]["service_worker"] = json!("./background.js");
        manifest["content_scripts"][0]["matches"] = json!(["https://video.example.test/embed/*"]);
        let package = Package::new(manifest);
        assert!(admit_beta_source(
            Arc::clone(&rig.policy),
            package.source(),
            &package.manifest,
            target,
            ExtensionPackageRevision::INITIAL,
            None
        )
        .is_err());
        let source = crate::beta::admit_external_source(
            package.source(),
            &package.manifest,
            target,
            ExtensionPackageRevision::INITIAL,
            None,
        )
        .unwrap();
        let mut workspace = rig.workspace();
        let artifact = workspace.prepare_external(source, &package.crx).unwrap();
        artifact.verify().unwrap();
        assert!(artifact
            .limitations()
            .unwrap()
            .contains(&crate::beta::BetaCompatibilityLimitation::BoundedStorageQuota));
        assert!(!artifact
            .manifest()
            .unwrap()
            .descriptor()
            .declarations()
            .required_api()
            .names()
            .iter()
            .any(|name| name.as_str() == "unlimitedStorage"));
        let output: serde_json::Value =
            serde_json::from_slice(&fs::read(rig.ready().join("extension/manifest.json")).unwrap())
                .unwrap();
        assert_eq!(output["background"]["service_worker"], "./background.js");
        assert!(output["host_permissions"]
            .as_array()
            .unwrap()
            .contains(&json!("https://video.example.test/*")));
        assert_eq!(
            output["content_scripts"][0]["matches"],
            json!(["https://video.example.test/embed/*"])
        );
        assert_eq!(
            fs::read(rig.ready().join("extension/background.js")).unwrap(),
            b"void 0;"
        );
        assert_eq!(
            fs::read(rig.ready().join("original.crx")).unwrap(),
            package.crx
        );
        let provenance = artifact
            .provenance(ExtensionProvenanceSource::ChromeWebStore)
            .unwrap();
        assert_eq!(
            provenance.runtime_target().as_str(),
            target.bounded_storage_target_id()
        );
        assert_eq!(
            ExtensionInstallProvenance::decode(&provenance.encode()),
            Some(provenance.clone())
        );
        drop(artifact);
        drop(workspace);
        let workspace = rig.workspace();
        let reopened = workspace
            .reopen_external_bound(&provenance, provenance.upstream(), target)
            .unwrap();
        reopened.verify().unwrap();
        assert!(reopened
            .limitations()
            .unwrap()
            .contains(&crate::beta::BetaCompatibilityLimitation::BoundedStorageQuota));
    }
}

#[tokio::test]
async fn bounded_storage_composes_with_native_adapters_without_new_publisher_authority() {
    for brokered in [false, true] {
        let rig = Rig::new().await;
        let mut manifest = basic();
        manifest["permissions"] = if brokered {
            json!(["storage", "unlimitedStorage", "history"])
        } else {
            json!(["storage", "unlimitedStorage"])
        };
        if !brokered {
            manifest["background"]["type"] = json!("module");
        }
        let mut package = Package::new(manifest.clone());
        package.set_manifest_for_compatibility(manifest, &[]);
        assert!(admit_beta_source(
            Arc::clone(&rig.policy),
            package.source(),
            &package.manifest,
            BetaRuntimeTarget::MacosNative,
            ExtensionPackageRevision::INITIAL,
            None
        )
        .is_err());
        let source = crate::beta::admit_external_source(
            package.source(),
            &package.manifest,
            BetaRuntimeTarget::MacosNative,
            ExtensionPackageRevision::INITIAL,
            None,
        )
        .unwrap();
        let mut workspace = rig.workspace();
        let artifact = workspace.prepare_external(source, &package.crx).unwrap();
        artifact.verify().unwrap();
        assert!(artifact.publisher_native_host().is_none());
        assert!(artifact
            .limitations()
            .unwrap()
            .contains(&crate::beta::BetaCompatibilityLimitation::BoundedStorageQuota));
        let output = artifact.manifest().unwrap();
        assert!(!output
            .descriptor()
            .declarations()
            .required_api()
            .contains_exact("unlimitedStorage"));
        assert_eq!(
            output.descriptor().compatibility_target().as_str(),
            if brokered {
                zephium_core::extensions::LOCAL_MACOS_HISTORY_V3_COMPATIBILITY_TARGET
            } else {
                zephium_core::extensions::LOCAL_MACOS_ADAPTED_COMPATIBILITY_TARGET
            }
        );
        assert!(
            matches!(artifact.transformation(), ExtensionTransformProvenance::Compiled { revision, .. } if revision.get() == 2)
        );
        let provenance = artifact
            .provenance(ExtensionProvenanceSource::ChromeWebStore)
            .unwrap();
        assert_eq!(
            ExtensionInstallProvenance::decode(&provenance.encode()),
            Some(provenance.clone())
        );
        drop(artifact);
        drop(workspace);
        rig.workspace()
            .reopen_external_bound(
                &provenance,
                provenance.upstream(),
                BetaRuntimeTarget::MacosNative,
            )
            .unwrap()
            .verify()
            .unwrap();
    }
}

#[tokio::test]
async fn managed_schema_uses_a_separate_readonly_adapter_and_preserves_original_resources() {
    let rig = Rig::new().await;
    let mut manifest = basic();
    manifest["permissions"] = json!(["storage", "unlimitedStorage"]);
    manifest["storage"] = json!({"managed_schema":"managed.json"});
    manifest["background"]["type"] = json!("module");
    let schema = br#"{"type":"object","properties":{"enabled":{"type":"boolean","default":true}}}"#;
    let mut package = Package::new(manifest.clone());
    package.set_manifest_for_compatibility(manifest, &[("managed.json", schema)]);
    assert!(admit_beta_source(
        Arc::clone(&rig.policy),
        package.source(),
        &package.manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::INITIAL,
        None
    )
    .is_err());
    let source = crate::beta::admit_external_source(
        package.source(),
        &package.manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::INITIAL,
        None,
    )
    .unwrap();
    let mut workspace = rig.workspace();
    let artifact = workspace.prepare_external(source, &package.crx).unwrap();
    artifact.verify().unwrap();
    assert_eq!(
        fs::read(rig.ready().join("extension/managed.json")).unwrap(),
        schema
    );
    assert_eq!(
        fs::read(rig.ready().join("original.crx")).unwrap(),
        package.crx
    );
    assert_eq!(
        fs::read_to_string(
            rig.ready()
                .join("extension/__zephium__/webkit-managed-storage-v1.js")
        )
        .unwrap(),
        MANAGED_STORAGE_V2
    );
    let output: serde_json::Value =
        serde_json::from_slice(&fs::read(rig.ready().join("extension/manifest.json")).unwrap())
            .unwrap();
    assert_eq!(output["storage"]["managed_schema"], "managed.json");
    assert_eq!(
        output["content_scripts"][0]["js"][0],
        "__zephium__/webkit-managed-storage-v1.js"
    );
    let provenance = artifact
        .provenance(ExtensionProvenanceSource::ChromeWebStore)
        .unwrap();
    drop(artifact);
    drop(workspace);
    rig.workspace()
        .reopen_external_bound(
            &provenance,
            provenance.upstream(),
            BetaRuntimeTarget::MacosNative,
        )
        .unwrap()
        .verify()
        .unwrap();
}

#[tokio::test]
async fn malformed_managed_schema_never_produces_a_prepared_artifact() {
    for schema in [
        b"not json".as_slice(),
        br#"{"type":"array","properties":{}}"#,
        br#"{"type":"object","properties":true}"#,
    ] {
        let rig = Rig::new().await;
        let mut manifest = basic();
        manifest["storage"] = json!({"managed_schema":"managed.json"});
        let mut package = Package::new(manifest.clone());
        package.set_manifest_for_compatibility(manifest, &[("managed.json", schema)]);
        let source = crate::beta::admit_external_source(
            package.source(),
            &package.manifest,
            BetaRuntimeTarget::MacosNative,
            ExtensionPackageRevision::INITIAL,
            None,
        )
        .unwrap();
        assert!(matches!(
            rig.workspace().prepare_external(source, &package.crx),
            Err(BetaArtifactPreparationError::Transform)
        ));
    }
}

#[tokio::test]
async fn unavailable_optional_features_are_withheld_without_promoting_required_or_unknown_authority(
) {
    for target in [
        BetaRuntimeTarget::MacosNative,
        BetaRuntimeTarget::WindowsNative,
    ] {
        let rig = Rig::new().await;
        let mut manifest = basic();
        manifest["optional_permissions"] = json!([
            "identity",
            "nativeMessaging",
            "futureOptional",
            "offscreen",
            "sidePanel",
            "tabs"
        ]);
        manifest["optional_host_permissions"] =
            json!(["file:///*", "https://optional.example.test/*"]);
        manifest["externally_connectable"] =
            json!({"matches":["https://example.test/*"],"ids":["*"],"accepts_tls_channel_id":true});
        manifest["web_accessible_resources"] =
            json!([{"resources":["absent.js", "content.js"],"matches":["https://example.test/*"]}]);
        manifest["side_panel"] = json!({"default_path":"popup.html"});
        let package = Package::new(manifest.clone());
        assert!(admit_beta_source(
            Arc::clone(&rig.policy),
            package.source(),
            &package.manifest,
            target,
            ExtensionPackageRevision::INITIAL,
            None
        )
        .is_err());
        let source = crate::beta::admit_external_source(
            package.source(),
            &package.manifest,
            target,
            ExtensionPackageRevision::INITIAL,
            None,
        )
        .unwrap();
        let mut workspace = rig.workspace();
        let artifact = workspace.prepare_external(source, &package.crx).unwrap();
        artifact.verify().unwrap();
        let output: serde_json::Value =
            serde_json::from_slice(&fs::read(rig.ready().join("extension/manifest.json")).unwrap())
                .unwrap();
        assert_eq!(output["optional_permissions"], json!(["tabs"]));
        assert_eq!(
            output["optional_host_permissions"],
            json!(["https://optional.example.test/*"])
        );
        assert_eq!(
            output["externally_connectable"],
            json!({"ids":[package.id.as_str()], "matches":[]})
        );
        assert_eq!(artifact.withheld_optional_permissions().len(), 5);
        assert!(output.get("side_panel").is_none());
        assert!(artifact.external_messaging_withheld());
        let descriptor = artifact.manifest().unwrap().descriptor();
        let install = zephium_core::extensions::ExtensionInstall::new(
            zephium_core::ids::ExtensionInstallId::from(1),
            descriptor.package().clone(),
        );
        assert!(
            zephium_core::extensions::ExtensionGrantAuthority::initialize(
                &install,
                vec![
                    zephium_core::extensions::ApiPermissionName::parse_exact("storage").unwrap(),
                    zephium_core::extensions::ApiPermissionName::parse_exact("identity").unwrap()
                ],
                descriptor
                    .declarations()
                    .required_host_authorities()
                    .into_iter()
                    .cloned()
                    .collect(),
                false,
                false,
                descriptor,
            )
            .is_err()
        );
        assert_eq!(
            artifact.withheld_optional_hosts(),
            &[Box::<str>::from("file:///*")]
        );
        assert!(!rig.ready().join("extension/absent.js").exists());
        assert_eq!(
            fs::read(rig.ready().join("extension/content.js")).unwrap(),
            b"void 0;"
        );
        assert_eq!(
            fs::read(rig.ready().join("original.crx")).unwrap(),
            package.crx
        );
        let provenance = artifact
            .provenance(ExtensionProvenanceSource::ChromeWebStore)
            .unwrap();
        assert_eq!(
            ExtensionInstallProvenance::decode(&provenance.encode()),
            Some(provenance.clone())
        );
        drop(artifact);
        drop(workspace);
        let workspace = rig.workspace();
        let restored = workspace
            .reopen_external_bound(&provenance, provenance.upstream(), target)
            .unwrap();
        restored.verify().unwrap();
        assert_eq!(restored.withheld_optional_permissions().len(), 5);
        assert!(restored.external_messaging_withheld());
        for required in ["futureRequired"] {
            let mut required_manifest = basic();
            required_manifest["permissions"] = json!(["storage", required]);
            let package = Package::new(required_manifest);
            assert!(crate::beta::admit_external_source(
                package.source(),
                &package.manifest,
                target,
                ExtensionPackageRevision::INITIAL,
                None
            )
            .is_err());
        }
        manifest["unknown_privilege"] = json!({"enabled":true});
        let package = Package::new(manifest);
        assert!(crate::beta::admit_external_source(
            package.source(),
            &package.manifest,
            target,
            ExtensionPackageRevision::INITIAL,
            None
        )
        .is_err());
    }
}

#[tokio::test]
async fn optional_withholding_composes_with_existing_broker_without_publisher_host_authority() {
    let rig = Rig::new().await;
    let mut manifest = basic();
    manifest["permissions"] = json!(["storage", "history"]);
    manifest["optional_permissions"] = json!(["identity", "nativeMessaging"]);
    let mut package = Package::new(manifest.clone());
    package.set_manifest_for_compatibility(manifest, &[]);
    let source = crate::beta::admit_external_source(
        package.source(),
        &package.manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::INITIAL,
        None,
    )
    .unwrap();
    let mut workspace = rig.workspace();
    let artifact = workspace.prepare_external(source, &package.crx).unwrap();
    artifact.verify().unwrap();
    assert!(artifact
        .manifest()
        .unwrap()
        .descriptor()
        .declarations()
        .required_api()
        .contains_exact("nativeMessaging"));
    assert!(artifact.publisher_native_host().is_none());
    assert!(artifact
        .withheld_optional_permissions()
        .iter()
        .any(|name| name.as_str() == "nativeMessaging"));
    let provenance = artifact
        .provenance(ExtensionProvenanceSource::ChromeWebStore)
        .unwrap();
    assert!(
        matches!(provenance.transform(), ExtensionTransformProvenance::Compiled { target, revision, .. }
        if target.as_str() == "local.webkit-history.v3" && revision.get() == 2)
    );
    assert_eq!(
        ExtensionInstallProvenance::decode(&provenance.encode()),
        Some(provenance.clone())
    );
    drop(artifact);
    drop(workspace);
    rig.workspace()
        .reopen_external_bound(
            &provenance,
            provenance.upstream(),
            BetaRuntimeTarget::MacosNative,
        )
        .unwrap()
        .verify()
        .unwrap();
}

#[test]
fn supported_native_optional_apis_remain_available_on_their_qualified_platform() {
    let mut manifest = basic();
    manifest["optional_permissions"] = json!([
        "cookies",
        "clipboardWrite",
        "declarativeNetRequest",
        "webRequest"
    ]);
    let package = Package::new(manifest);
    let source = crate::beta::admit_external_source(
        package.source(),
        &package.manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::INITIAL,
        None,
    )
    .unwrap();
    assert!(source.inner.withheld.optional_api.is_empty());
}

#[tokio::test]
async fn legacy_history_artifacts_reopen_with_their_original_bytes_after_delivery_fix() {
    for (policy, target) in [
        (
            crate::beta::SourcePolicy::LocalLegacyHistory,
            zephium_core::extensions::MACOS_NATIVE_BROKERED_COMPATIBILITY_TARGET,
        ),
        (
            crate::beta::SourcePolicy::LocalHistoryV2,
            zephium_core::extensions::LOCAL_MACOS_HISTORY_V2_COMPATIBILITY_TARGET,
        ),
    ] {
        let rig = Rig::new().await;
        let mut manifest = basic();
        manifest["permissions"] = json!(["storage", "history"]);
        let mut package = Package::new(manifest.clone());
        package.set_manifest_for_compatibility(manifest, &[]);
        let inner = crate::beta::admit_source(
            policy,
            package.source(),
            &package.manifest,
            BetaRuntimeTarget::MacosNative,
            ExtensionPackageRevision::INITIAL,
            None,
        )
        .unwrap();
        let source = crate::beta::ProductAdmittedExternalSource { inner };
        let mut workspace = rig.workspace();
        let artifact = workspace.prepare_external(source, &package.crx).unwrap();
        assert_eq!(
            artifact
                .manifest()
                .unwrap()
                .descriptor()
                .compatibility_target()
                .as_str(),
            target
        );
        let path = rig
            .ready()
            .join("extension/__zephium__/webkit-history-v1.js");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            zephium_extension_package::macos_compatibility::HISTORY_BRIDGE_SOURCE
        );
        let provenance = artifact
            .provenance(ExtensionProvenanceSource::ChromeWebStore)
            .unwrap();
        assert_eq!(
            ExtensionInstallProvenance::decode(&provenance.encode()),
            Some(provenance.clone())
        );
        drop(artifact);
        drop(workspace);
        let workspace = rig.workspace();
        let reopened = workspace
            .reopen_external_bound(
                &provenance,
                provenance.upstream(),
                BetaRuntimeTarget::MacosNative,
            )
            .unwrap();
        reopened.verify().unwrap();
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            zephium_extension_package::macos_compatibility::HISTORY_BRIDGE_SOURCE
        );
        assert_eq!(
            reopened
                .provenance(ExtensionProvenanceSource::ChromeWebStore)
                .unwrap(),
            provenance
        );
    }
}

#[test]
fn local_native_admission_still_refuses_unenforced_content_script_globs() {
    for field in ["include_globs", "exclude_globs"] {
        let mut manifest = basic();
        manifest["content_scripts"][0][field] = json!(["*example.test/private*"]);
        let package = Package::new(manifest);
        assert!(matches!(
            crate::beta::admit_external_source(
                package.source(),
                &package.manifest,
                BetaRuntimeTarget::MacosNative,
                ExtensionPackageRevision::INITIAL,
                None,
            ),
            Err(crate::beta::BetaSourceAdmissionError::Unsupported(_))
        ));
    }
}
