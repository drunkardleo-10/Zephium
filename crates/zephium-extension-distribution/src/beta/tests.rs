use std::io::{Cursor, Write};

#[cfg(any(target_os = "macos", target_os = "linux"))]
use base64::Engine;
use ring::{
    rand::SystemRandom,
    signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_ASN1_SIGNING},
};
use serde_json::{json, Value};
use zephium_core::extensions::ExtensionManifestDeclaration;
use zephium_extension_acquisition::AcquiredExtensionArchive;
use zephium_extension_package::{ChromiumExtensionId, Crx3SigningRequest};

use super::*;

pub(super) struct Package {
    key: EcdsaKeyPair,
    pub(super) public: Vec<u8>,
    pub(super) crx: Vec<u8>,
    pub(super) id: ChromiumExtensionId,
    pub(super) manifest: Vec<u8>,
}

pub(super) fn basic() -> Value {
    json!({"manifest_version":3,"name":"Beta fixture","version":"1.0",
        "permissions":["storage"],"host_permissions":["https://example.test/*"],
        "content_scripts":[{"matches":["https://example.test/*"],"js":["content.js"]}],
        "background":{"service_worker":"background.js"},"action":{"default_popup":"popup.html"}
    })
}

impl Package {
    pub(super) fn new(manifest: Value) -> Self {
        let random = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &random).unwrap();
        let key =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &random)
                .unwrap();
        let mut public = vec![
            0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06,
            0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
        ];
        public.extend_from_slice(key.public_key().as_ref());
        let mut package = Self {
            key,
            public,
            crx: Vec::new(),
            id: ChromiumExtensionId::parse(&"a".repeat(32)).unwrap(),
            manifest: Vec::new(),
        };
        package.set_manifest(manifest);
        package
    }
    pub(super) fn set_manifest(&mut self, manifest: Value) {
        self.set_manifest_with_extra(manifest, &[]);
    }
    pub(super) fn set_manifest_with_extra(&mut self, manifest: Value, extra: &[(&str, &[u8])]) {
        self.set_manifest_files(manifest, extra, false);
    }
    #[cfg(any(
        target_os = "macos",
        target_os = "linux",
        all(target_os = "windows", feature = "windows-namespace-validation")
    ))]
    pub(super) fn set_manifest_for_compatibility(
        &mut self,
        manifest: Value,
        extra: &[(&str, &[u8])],
    ) {
        self.set_manifest_files(manifest, extra, true);
    }
    fn set_manifest_files(
        &mut self,
        manifest: Value,
        extra: &[(&str, &[u8])],
        explicit_heads: bool,
    ) {
        self.manifest = serde_json::to_vec(&manifest).unwrap();
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in [
            ("manifest.json", self.manifest.as_slice()),
            ("content.js", b"void 0;"),
            ("background.js", b"void 0;"),
            ("popup.html", b"<!doctype html><p>Fixture</p>"),
            ("options.html", b"<!doctype html><p>Options</p>"),
            ("sandbox.html", b"<p>Sandbox</p>"),
        ]
        .into_iter()
        .chain(extra.iter().copied())
        {
            archive
                .start_file(
                    name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored),
                )
                .unwrap();
            let bytes = if explicit_heads && name.ends_with(".html") {
                b"<!doctype html><html><head><title>Fixture</title></head><body>Fixture</body></html>".as_slice()
            } else {
                bytes
            };
            archive.write_all(bytes).unwrap();
        }
        let archive = archive.finish().unwrap().into_inner();
        let request = Crx3SigningRequest::new_ecdsa_p256_sha256(&archive, &self.public).unwrap();
        self.id = request.extension_id().clone();
        let signature = self
            .key
            .sign(
                &SystemRandom::new(),
                &request.signed_message_parts().concat(),
            )
            .unwrap();
        self.crx = request.finish(signature.as_ref()).unwrap();
    }
    pub(super) fn source(&self) -> AcquiredExtensionTreeReceipt {
        let mut archive =
            AcquiredExtensionArchive::authenticate_upstream_crx3(&self.crx, &self.id, None)
                .unwrap();
        let receipts = (0..archive.files().len())
            .map(|index| archive.copy_file(index, &mut std::io::sink()).unwrap())
            .collect::<Vec<_>>();
        archive.finish_tree(receipts).unwrap()
    }
    fn assess(&self, runtime: BetaRuntimeTarget) -> AdmittedExtensionManifest {
        let source = self.source();
        let identity = ExtensionPackageIdentity::new(
            ExtensionAuthorityId::from_bytes([1; 32]),
            ExtensionPackageKey::from_bytes(source.developer_key_sha256().bytes()),
            ExtensionPackageRevision::INITIAL,
            source.payload_identity(),
            source.index().manifest_sha256(),
            source.index().tree_sha256(),
        );
        assess_upstream_extension_manifest(
            &identity,
            source.index(),
            &ExpectedChromiumIdentity::from_manifest_key_digest(source.developer_key_sha256()),
            &self.manifest,
            &compatibility::CompiledBetaPolicy::new(runtime),
        )
        .unwrap()
    }
}

#[test]
fn both_native_targets_have_closed_source_rules_and_explicit_limitations() {
    let package = Package::new(basic());
    for target in [
        BetaRuntimeTarget::MacosNative,
        BetaRuntimeTarget::WindowsNative,
    ] {
        let native_target = match target {
            BetaRuntimeTarget::MacosNative => {
                zephium_extension_authority::ProductExtensionRuntimeTarget::MacosNative
            }
            BetaRuntimeTarget::WindowsNative => {
                zephium_extension_authority::ProductExtensionRuntimeTarget::WindowsNative
            }
        };
        assert_eq!(target.target_id(), native_target.compatibility_target_id());
        let manifest = package.assess(target);
        assert_eq!(
            manifest.descriptor().compatibility_target().as_str(),
            target.target_id()
        );
        assert!(manifest
            .descriptor()
            .compatibility()
            .iter()
            .all(|row| matches!(
                row.level(),
                ExtensionCompatibilityLevel::Compatible | ExtensionCompatibilityLevel::Degraded
            )));
        let limitations = compatibility::CompiledBetaPolicy::new(target).limitations(&manifest);
        assert!(limitations.contains(&BetaCompatibilityLimitation::CloudStorageSyncUnavailable));
        assert_eq!(
            limitations.contains(&BetaCompatibilityLimitation::PlatformBackgroundLifecycle),
            target == BetaRuntimeTarget::MacosNative
        );
    }
}

#[test]
fn optional_permissions_unknown_authority_native_messaging_and_sandbox_are_not_silently_ignored() {
    for (field, value) in [
        ("permissions", json!(["nativeMessaging"])),
        ("optional_permissions", json!(["cookies"])),
        ("permissions", json!(["webRequestBlocking"])),
        ("permissions", json!(["futureApi"])),
        (
            "externally_connectable",
            json!({"matches":["https://example.test/*"]}),
        ),
        ("sandbox", json!({"pages":["sandbox.html"]})),
        ("minimum_chrome_version", json!("999.0")),
        ("host_permissions", json!(["file:///*"])),
        (
            "background",
            json!({"service_worker":"background.js","type":"module"}),
        ),
    ] {
        let mut manifest = basic();
        manifest[field] = value;
        let package = Package::new(manifest);
        for target in [
            BetaRuntimeTarget::MacosNative,
            BetaRuntimeTarget::WindowsNative,
        ] {
            let assessed = package.assess(target);
            assert!(
                assessed
                    .descriptor()
                    .compatibility()
                    .iter()
                    .any(|row| row.level() == ExtensionCompatibilityLevel::Unsupported),
                "{field} {target:?}"
            );
        }
    }
}

#[test]
fn related_frame_or_main_world_semantics_cannot_hide_under_content_script_tag() {
    for (field, value) in [
        ("world", json!("MAIN")),
        ("match_about_blank", json!(true)),
        ("match_origin_as_fallback", json!(true)),
        ("include_globs", json!(["*example.test/*"])),
    ] {
        let mut manifest = basic();
        manifest["content_scripts"][0][field] = value;
        let package = Package::new(manifest);
        for target in [
            BetaRuntimeTarget::MacosNative,
            BetaRuntimeTarget::WindowsNative,
        ] {
            assert!(package
                .assess(target)
                .descriptor()
                .compatibility()
                .iter()
                .any(|row| matches!(
                    row.declaration(),
                    ExtensionManifestDeclaration::ContentScript { .. }
                ) && row.level() == ExtensionCompatibilityLevel::Unsupported));
        }
    }
}

#[test]
fn broad_host_patterns_get_one_explicit_file_access_limitation() {
    let mut manifest = basic();
    manifest["host_permissions"] = json!(["<all_urls>"]);
    manifest["content_scripts"][0]["matches"] = json!(["<all_urls>"]);
    let package = Package::new(manifest);
    for target in [
        BetaRuntimeTarget::MacosNative,
        BetaRuntimeTarget::WindowsNative,
    ] {
        let rules = compatibility::CompiledBetaPolicy::new(target);
        let limitations = rules.limitations(&package.assess(target));
        assert_eq!(
            limitations
                .iter()
                .filter(|value| **value == BetaCompatibilityLimitation::LocalFileAccessExcluded)
                .count(),
            1
        );
    }
}

// Filesystem integration is executed on adapters supported by private-fs.
// Both backend policy targets are exercised here on every supported host.
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod integration {
    use super::*;
    use crate::public_policy::{tests::Fixture, ExtensionPolicyCache};
    use std::os::unix::fs::PermissionsExt;
    use zephium_private_fs::LockedPrivateNamespace;

    fn setup() -> (tempfile::TempDir, Fixture, ExtensionPolicyCache) {
        #[cfg(target_os = "macos")]
        let directory = tempfile::tempdir_in("/private/tmp").unwrap();
        #[cfg(target_os = "linux")]
        let directory = tempfile::tempdir_in("/tmp").unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut fixture = Fixture::new();
        fixture.policy["beta_targets"] = json!([
            {"target":"macos.wkwebextension.v1","policy_version":1},
            {"target":"windows.webview2.v1","policy_version":1}
        ]);
        fixture.publish(1);
        let cache = ExtensionPolicyCache::open(
            LockedPrivateNamespace::open_or_create(directory.path().join("policy")).unwrap(),
            &fixture.client(),
        )
        .unwrap();
        (directory, fixture, cache)
    }

    #[tokio::test]
    async fn real_crx_and_signed_policy_mint_only_the_distinct_beta_source_witness() {
        let (_directory, fixture, mut cache) = setup();
        let policy = Arc::new(cache.refresh(&fixture.client()).await.unwrap());
        let package = Package::new(basic());
        let mut authorities = Vec::new();
        for target in [
            BetaRuntimeTarget::MacosNative,
            BetaRuntimeTarget::WindowsNative,
        ] {
            let source = package.source();
            let expected = source.upstream_checkpoint(&package.manifest).unwrap();
            let admitted = admit_beta_source(
                Arc::clone(&policy),
                source,
                &package.manifest,
                target,
                ExtensionPackageRevision::INITIAL,
                None,
            )
            .unwrap();
            assert_eq!(admitted.upstream(), expected);
            assert!(admitted.requires_manifest_key_transform());
            assert_eq!(
                admitted.policy_evidence().unwrap().sha256,
                policy.policy().unwrap().sha256()
            );
            assert_eq!(
                admitted.original_tree().manifest,
                <[u8; 32]>::from(Sha256::digest(&package.manifest))
            );
            assert!(admitted.retained_bytes() >= admitted.manifest().unwrap().retained_bytes());
            assert!(admitted.retained_bytes() <= MAX_BETA_SOURCE_RETAINED_BYTES);
            authorities.push(
                admitted
                    .manifest()
                    .unwrap()
                    .descriptor()
                    .package()
                    .authority(),
            );
            assert_ne!(
                std::any::TypeId::of::<ProductAdmittedBetaSource>(),
                std::any::TypeId::of::<zephium_extension_authority::ProductAdmittedExtensionManifest>(
                )
            );
        }
        assert_ne!(authorities[0], authorities[1]);
    }

    #[tokio::test]
    async fn missing_or_future_policy_opt_in_never_uses_recommendations_as_authority() {
        for targets in [
            json!([]),
            json!([{"target":"macos.wkwebextension.v1","policy_version":2}]),
            json!([{"target":"windows.webview2.v1","policy_version":1}]),
        ] {
            let (_directory, mut fixture, mut cache) = setup();
            let package = Package::new(basic());
            let source = package.source();
            fixture.policy["beta_targets"] = targets;
            fixture.policy["recommendations"] = json!([{
                "extension_id":source.extension_id().as_str(),
                "developer_key_sha256":hex(source.developer_key_sha256().bytes()),
                "tested_versions":[{
                    "upstream_version":"1.0", "original_crx_sha256":hex(source.original_crx_sha256()),
                    "runtime_target":"macos.wkwebextension.v1", "transform_id":"identity.v1",
                    "transformed_tree_sha256":hex(source.index().tree_sha256().bytes()),
                    "zephium_version":"0.1.0", "platform_runtime":"Fixture only",
                    "tested_unix":fixture.policy["issued_unix"],
                    "workflows":["Fixture workflow"], "limitations":[]
                }]
            }]);
            fixture.publish(1);
            let policy = Arc::new(cache.refresh(&fixture.client()).await.unwrap());
            assert_eq!(
                admit_beta_source(
                    policy,
                    package.source(),
                    &package.manifest,
                    BetaRuntimeTarget::MacosNative,
                    ExtensionPackageRevision::INITIAL,
                    None
                )
                .unwrap_err(),
                BetaSourceAdmissionError::TargetNotEnabled
            );
        }
    }

    #[tokio::test]
    async fn exact_crx_and_publisher_revocations_override_beta_opt_in() {
        for publisher_wide in [false, true] {
            let (_directory, mut fixture, mut cache) = setup();
            let package = Package::new(basic());
            let source = package.source();
            fixture.policy["revocations"] = json!([{
                "extension_id":source.extension_id().as_str(),"developer_key_sha256":hex(source.developer_key_sha256().bytes()),
                "original_crx_sha256":if publisher_wide { Value::Null } else { json!(hex(source.original_crx_sha256())) },"reason":"Fixture incident"
            }]);
            fixture.publish(1);
            let policy = Arc::new(cache.refresh(&fixture.client()).await.unwrap());
            assert_eq!(
                admit_beta_source(
                    policy,
                    source,
                    &package.manifest,
                    BetaRuntimeTarget::MacosNative,
                    ExtensionPackageRevision::INITIAL,
                    None
                )
                .unwrap_err(),
                BetaSourceAdmissionError::Revoked
            );
        }
    }

    #[tokio::test]
    async fn superseded_policy_and_owner_teardown_invalidate_already_admitted_sources() {
        let (_directory, mut fixture, mut cache) = setup();
        let package = Package::new(basic());
        let policy = Arc::new(cache.refresh(&fixture.client()).await.unwrap());
        let old = admit_beta_source(
            policy,
            package.source(),
            &package.manifest,
            BetaRuntimeTarget::MacosNative,
            ExtensionPackageRevision::INITIAL,
            None,
        )
        .unwrap();
        fixture.policy["policy_revision"] = json!(2);
        fixture.publish(2);
        let policy = Arc::new(cache.refresh(&fixture.client()).await.unwrap());
        assert_eq!(
            old.manifest().unwrap_err(),
            BetaSourceAdmissionError::PolicyUnavailable
        );
        let current = admit_beta_source(
            policy,
            package.source(),
            &package.manifest,
            BetaRuntimeTarget::MacosNative,
            ExtensionPackageRevision::INITIAL,
            None,
        )
        .unwrap();
        drop(cache);
        assert_eq!(
            current.revalidate(),
            Err(BetaSourceAdmissionError::PolicyUnavailable)
        );
    }

    #[tokio::test]
    async fn publisher_version_and_original_bytes_must_match_the_store_high_water() {
        let (_directory, fixture, mut cache) = setup();
        let policy = Arc::new(cache.refresh(&fixture.client()).await.unwrap());
        let mut package = Package::new(basic());
        let checkpoint = package
            .source()
            .upstream_checkpoint(&package.manifest)
            .unwrap();
        let admit = |package: &Package, previous| {
            admit_beta_source(
                Arc::clone(&policy),
                package.source(),
                &package.manifest,
                BetaRuntimeTarget::MacosNative,
                ExtensionPackageRevision::new(2).unwrap(),
                Some(previous),
            )
        };
        assert!(admit(&package, checkpoint).is_ok());
        let mut next = basic();
        next["version"] = json!("1.10");
        package.set_manifest(next);
        let advanced = admit(&package, checkpoint).unwrap();
        assert_eq!(advanced.previous_upstream(), Some(checkpoint));
        let mut lower = basic();
        lower["version"] = json!("1.9");
        package.set_manifest(lower);
        assert_eq!(
            admit(&package, advanced.upstream()).unwrap_err(),
            BetaSourceAdmissionError::UpstreamRollback
        );
        package.set_manifest(basic()); // Same version, newly signed envelope.
        assert_eq!(
            admit(&package, checkpoint).unwrap_err(),
            BetaSourceAdmissionError::UpstreamRollback
        );
        let foreign = Package::new(basic());
        assert_eq!(
            admit(&foreign, checkpoint).unwrap_err(),
            BetaSourceAdmissionError::UpstreamRollback
        );
    }

    #[tokio::test]
    async fn manifest_substitution_and_mismatching_manifest_key_never_get_a_witness() {
        let (_directory, fixture, mut cache) = setup();
        let policy = Arc::new(cache.refresh(&fixture.client()).await.unwrap());
        let mut package = Package::new(basic());
        assert_eq!(
            admit_beta_source(
                Arc::clone(&policy),
                package.source(),
                b"{}",
                BetaRuntimeTarget::MacosNative,
                ExtensionPackageRevision::INITIAL,
                None
            )
            .unwrap_err(),
            BetaSourceAdmissionError::SourceMismatch
        );
        let foreign = Package::new(basic());
        let mut manifest = basic();
        manifest["key"] = json!(base64::engine::general_purpose::STANDARD.encode(&foreign.public));
        package.set_manifest(manifest);
        assert_eq!(
            admit_beta_source(
                Arc::clone(&policy),
                package.source(),
                &package.manifest,
                BetaRuntimeTarget::MacosNative,
                ExtensionPackageRevision::INITIAL,
                None
            )
            .unwrap_err(),
            BetaSourceAdmissionError::SourceMismatch
        );
        let mut manifest = basic();
        manifest["key"] = json!(base64::engine::general_purpose::STANDARD.encode(&package.public));
        package.set_manifest(manifest);
        let admitted = admit_beta_source(
            policy,
            package.source(),
            &package.manifest,
            BetaRuntimeTarget::WindowsNative,
            ExtensionPackageRevision::INITIAL,
            None,
        )
        .unwrap();
        assert!(!admitted.requires_manifest_key_transform());
    }

    #[tokio::test]
    async fn a_valid_signature_does_not_override_the_compiled_unsupported_decision() {
        let (_directory, fixture, mut cache) = setup();
        let policy = Arc::new(cache.refresh(&fixture.client()).await.unwrap());
        let mut manifest = basic();
        manifest["permissions"] = json!(["nativeMessaging"]);
        let package = Package::new(manifest);
        assert!(matches!(
            admit_beta_source(
                policy,
                package.source(),
                &package.manifest,
                BetaRuntimeTarget::MacosNative,
                ExtensionPackageRevision::INITIAL,
                None
            ),
            Err(BetaSourceAdmissionError::Unsupported(_))
        ));
    }
}

#[test]
fn external_rejection_reports_multiple_missing_capabilities_without_admission() {
    let mut manifest = basic();
    manifest["permissions"] = json!([
        "storage",
        "offscreen",
        "identity",
        "idle",
        "sidePanel",
        "webRequestBlocking"
    ]);
    let package = Package::new(manifest);
    let Err(BetaSourceAdmissionError::Unsupported(report)) = admit_external_source(
        package.source(),
        &package.manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::INITIAL,
        None,
    ) else {
        panic!("unsupported source was admitted or lost its capability report");
    };
    let names: Vec<_> = report
        .declarations()
        .iter()
        .filter_map(|declaration| match declaration {
            zephium_core::extensions::ExtensionManifestDeclaration::RequiredApiPermission(name) => {
                Some(name.as_str())
            }
            _ => None,
        })
        .collect();
    for name in ["offscreen", "sidePanel", "webRequestBlocking"] {
        assert!(names.contains(&name));
    }
    assert!(!names.contains(&"identity"));
    // The adapted profile now discloses idle detection as unavailable instead
    // of rejecting the entire package for that declaration alone.
    assert!(!names.contains(&"idle"));
}
