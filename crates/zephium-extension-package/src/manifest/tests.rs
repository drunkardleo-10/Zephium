use proptest::prelude::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use zephium_core::extensions::{
    ExtensionCompatibilityLevel, ExtensionCompatibilityTargetId, ExtensionContentScriptRunAt,
    ExtensionContentScriptWorld, ExtensionManifestDeclaration,
};

use super::csp::{DEFAULT_EXTENSION_PAGES_CSP, DEFAULT_SANDBOX_CSP};
use super::*;
use crate::{CanonicalExtensionTreeIndex, ExtensionReleaseCatalog};

struct Fixture {
    manifest: Vec<u8>,
    tree: CanonicalExtensionTreeIndex,
    catalog: ExtensionReleaseCatalog,
}

impl Fixture {
    fn binding(&self) -> ExtensionReleaseTreeBinding<'_> {
        self.catalog.packages()[0]
            .bind_tree_index(&self.tree)
            .expect("fixture release/tree binding")
    }
}

struct CompletePolicy {
    target: ExtensionCompatibilityTargetId,
    unmodeled: ExtensionCompatibilityLevel,
}

impl CompletePolicy {
    fn new(unmodeled: ExtensionCompatibilityLevel) -> Self {
        Self {
            target: ExtensionCompatibilityTargetId::parse_exact("test.compatibility.v1").unwrap(),
            unmodeled,
        }
    }
}

impl ExtensionManifestCompatibilityPolicy for CompletePolicy {
    fn target(&self) -> &ExtensionCompatibilityTargetId {
        &self.target
    }

    fn classify(
        &self,
        subject: ExtensionManifestCompatibilitySubject<'_>,
    ) -> Option<ExtensionCompatibilityLevel> {
        let level = match subject.declaration() {
            ExtensionManifestDeclaration::UnmodeledAuthority(_) => self.unmodeled,
            ExtensionManifestDeclaration::ContentScript { index, .. } => subject
                .resources()
                .content_scripts()
                .get(usize::from(*index))
                .filter(|resources| {
                    resources.include_globs().is_empty() && resources.exclude_globs().is_empty()
                })
                .map_or(ExtensionCompatibilityLevel::Unsupported, |_| {
                    ExtensionCompatibilityLevel::Compatible
                }),
            ExtensionManifestDeclaration::WebAccessibleResources { index, .. } => subject
                .resources()
                .web_accessible_resources()
                .get(usize::from(*index))
                .filter(|resources| {
                    resources
                        .resources()
                        .iter()
                        .all(|resource| !resource.contains_wildcard())
                })
                .map_or(ExtensionCompatibilityLevel::Unsupported, |_| {
                    ExtensionCompatibilityLevel::Compatible
                }),
            _ => ExtensionCompatibilityLevel::Compatible,
        };
        Some(level)
    }
}

fn hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn make_fixture(manifest: Value, files: &[(&str, &[u8])], chromium: bool) -> Fixture {
    fixture_bytes(serde_json::to_vec(&manifest).unwrap(), files, chromium)
}

fn fixture_bytes(manifest: Vec<u8>, files: &[(&str, &[u8])], chromium: bool) -> Fixture {
    let mut all_files = files
        .iter()
        .map(|(path, bytes)| ((*path).to_owned(), bytes.to_vec()))
        .collect::<Vec<_>>();
    all_files.push(("manifest.json".to_owned(), manifest.clone()));
    all_files.sort_unstable_by(|left, right| left.0.cmp(&right.0));

    let mut tree_json = String::from(r#"{"schema_version":1,"files":["#);
    for (index, (path, bytes)) in all_files.iter().enumerate() {
        if index != 0 {
            tree_json.push(',');
        }
        tree_json.push_str(&format!(
            r#"{{"path":{},"length":{},"sha256":"{}"}}"#,
            serde_json::to_string(path).unwrap(),
            bytes.len(),
            hex(Sha256::digest(bytes).into()),
        ));
    }
    tree_json.push_str("]}");
    let tree = CanonicalExtensionTreeIndex::parse_canonical(tree_json.as_bytes()).unwrap();
    let chromium_json = if chromium {
        let key = ChromiumManifestKey::parse_canonical("Xw==").unwrap();
        format!(
            r#"{{"manifest_key_sha256":"{}"}}"#,
            hex(key.digest().bytes())
        )
    } else {
        "null".to_owned()
    };
    let catalog_json = format!(
        concat!(
            r#"{{"schema_version":1,"catalog_revision":1,"created_unix":1,"authority_id":"{}","admission_policy_sha256":"{}","packages":[{{"package_key":"{}","revision":1,"payload":{{"kind":"bundled_tree"}},"manifest_sha256":"{}","tree_sha256":"{}","tree_index_sha256":"{}","tree_file_count":{},"tree_bytes":{},"chromium":{},"provenance":{{"source_url":"https://example.com/releases/v1/source","upstream_version":"1","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Example","redistribution":"Reviewed","legal_notice":{{"target":"licenses/example.txt","kind":"notice_bundle","length":1,"sha256":"{}"}},"corresponding_source":null}}}}]}}"#,
        ),
        hex([1; 32]),
        hex([2; 32]),
        hex([3; 32]),
        hex(tree.manifest_sha256().bytes()),
        hex(tree.tree_sha256().bytes()),
        hex(tree.index_sha256().bytes()),
        tree.files().len(),
        tree.total_bytes(),
        chromium_json,
        hex([4; 32]),
    );
    let catalog = ExtensionReleaseCatalog::parse_canonical(catalog_json.as_bytes()).unwrap();
    Fixture {
        manifest,
        tree,
        catalog,
    }
}

fn full_fixture() -> Fixture {
    let manifest = json!({
        "manifest_version": 3,
        "name": "__MSG_extension_name__",
        "version": "1.2.3",
        "description": "__MSG_extension_description__",
        "short_name": "Zephium Test",
        "default_locale": "en",
        "homepage_url": "https://example.com/project",
        "icons": {"16": "icon.png"},
        "key": "Xw==",
        "permissions": ["offscreen", "storage"],
        "optional_permissions": ["clipboardWrite"],
        "host_permissions": ["https://required.example/*"],
        "optional_host_permissions": ["https://optional.example/*"],
        "background": {"service_worker": "sw.js", "type": "module"},
        "action": {
            "default_popup": "popup.html",
            "default_icon": {"16": "action.png"},
            "default_title": "__MSG_action_title__"
        },
        "chrome_url_overrides": {"newtab": "newtab.html"},
        "content_scripts": [{
            "matches": ["https://content.example/*"],
            "exclude_matches": ["https://content.example/private/*"],
            "include_globs": ["*content.example/*"],
            "exclude_globs": ["*secret*"],
            "js": ["content.js"],
            "css": ["content.css"],
            "run_at": "document_start",
            "all_frames": true,
            "match_about_blank": true,
            "match_origin_as_fallback": false,
            "world": "ISOLATED"
        }],
        "content_security_policy": {
            "extension_pages": DEFAULT_EXTENSION_PAGES_CSP,
            "sandbox": DEFAULT_SANDBOX_CSP
        },
        "sandbox": {"pages": ["sandbox.html"]},
        "web_accessible_resources": [{
            "resources": ["asset.png", "images/*"],
            "matches": ["https://consumer.example/*"],
            "use_dynamic_url": true
        }],
        "commands": {"open": {"suggested_key": {"default": "Alt+Shift+O"}}}
    });
    make_fixture(
        manifest,
        &[
            (
                "_locales/en/messages.json",
                br#"{"extension_name":{"message":"Test"}}"#,
            ),
            ("action.png", b"action"),
            ("asset.png", b"asset"),
            ("content.css", b"body{}"),
            ("content.js", b"void 0"),
            ("icon.png", b"icon"),
            ("images/logo.png", b"logo"),
            ("newtab.html", b"newtab"),
            ("popup.html", b"popup"),
            ("sandbox.html", b"sandbox"),
            ("sw.js", b"worker"),
        ],
        true,
    )
}

#[test]
fn admits_complete_mv3_authority_without_losing_runtime_paths() {
    let fixture = full_fixture();
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    let admitted = admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();

    assert_eq!(admitted.metadata().name(), "__MSG_extension_name__");
    assert_eq!(admitted.metadata().version(), "1.2.3");
    assert_eq!(admitted.metadata().default_locale(), Some("en"));
    assert_eq!(
        admitted.metadata().action_title(),
        Some("__MSG_action_title__")
    );
    assert_eq!(
        admitted
            .metadata()
            .locale_messages()
            .unwrap()
            .path()
            .as_str(),
        "_locales/en/messages.json"
    );
    assert_eq!(
        admitted.metadata().icons()[0].resource().path().as_str(),
        "icon.png"
    );
    assert_eq!(admitted.metadata().icons()[0].size(), Some(16));
    assert_eq!(
        admitted.chromium_key().unwrap().extension_id().as_str(),
        "ncocknphbhhlhkikpnnlmbcnbgdempcd"
    );

    let declarations = admitted.descriptor().declarations();
    assert_eq!(declarations.execution().content_scripts().len(), 1);
    let script = &declarations.execution().content_scripts()[0];
    assert_eq!(script.run_at(), ExtensionContentScriptRunAt::DocumentStart);
    assert_eq!(script.world(), ExtensionContentScriptWorld::Isolated);
    assert!(script.all_frames());
    assert!(script.matches().options().match_about_blank);
    assert_eq!(declarations.unmodeled()[0].as_str(), "commands");
    assert_eq!(
        admitted
            .descriptor()
            .compatibility_for(&ExtensionManifestDeclaration::UnmodeledAuthority(
                ExtensionUnmodeledDeclarationName::parse_exact("commands").unwrap(),
            )),
        Some(ExtensionCompatibilityLevel::Unsupported)
    );
    assert_eq!(
        admitted
            .descriptor()
            .compatibility_for(&ExtensionManifestDeclaration::ContentScript {
                index: 0,
                descriptor_digest: script.descriptor_digest(),
            }),
        Some(ExtensionCompatibilityLevel::Unsupported)
    );
    let web_accessible = &declarations.execution().web_accessible_resources()[0];
    assert_eq!(
        admitted.descriptor().compatibility_for(
            &ExtensionManifestDeclaration::WebAccessibleResources {
                index: 0,
                resources_digest: web_accessible.resources_digest(),
            }
        ),
        Some(ExtensionCompatibilityLevel::Unsupported)
    );

    let resources = admitted.resources();
    assert_eq!(
        resources.background_worker().unwrap().path().as_str(),
        "sw.js"
    );
    assert_eq!(
        resources.action_popup().unwrap().path().as_str(),
        "popup.html"
    );
    assert_eq!(resources.action_icons()[0].size(), Some(16));
    assert_eq!(
        resources.content_scripts()[0].javascript()[0]
            .path()
            .as_str(),
        "content.js"
    );
    assert_eq!(
        resources.content_scripts()[0].css()[0].path().as_str(),
        "content.css"
    );
    assert_eq!(
        resources.content_scripts()[0].include_globs()[0].as_ref(),
        "*content.example/*"
    );
    assert_eq!(resources.extension_pages_csp(), DEFAULT_EXTENSION_PAGES_CSP);
    assert_eq!(resources.sandbox_csp(), Some(DEFAULT_SANDBOX_CSP));
    assert_eq!(
        resources.web_accessible_resources()[0].resources()[1].canonical_pattern(),
        "images/*"
    );
    assert!(admitted.retained_bytes() >= admitted.descriptor().retained_bytes());
    assert!(resources.retained_bytes() <= MAX_EXTENSION_MANIFEST_PLAN_RETAINED_BYTES);
}

#[test]
fn exact_defaults_and_admission_digest_are_deterministic() {
    let fixture = make_fixture(
        json!({"manifest_version":3,"name":"Minimal","version":"1"}),
        &[],
        false,
    );
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    let first = admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
    let second = admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
    assert_eq!(first.admission_digest(), second.admission_digest());
    assert_eq!(
        first.admission_digest().bytes(),
        [
            100, 118, 21, 46, 142, 153, 149, 99, 171, 140, 92, 213, 223, 43, 115, 56, 63, 176, 109,
            213, 72, 189, 83, 197, 18, 31, 179, 173, 185, 49, 61, 90,
        ]
    );
    assert_eq!(
        first.resources().extension_pages_csp(),
        DEFAULT_EXTENSION_PAGES_CSP
    );
    assert_eq!(first.resources().sandbox_csp(), None);

    struct DegradedPolicy(ExtensionCompatibilityTargetId);
    impl ExtensionManifestCompatibilityPolicy for DegradedPolicy {
        fn target(&self) -> &ExtensionCompatibilityTargetId {
            &self.0
        }

        fn classify(
            &self,
            _subject: ExtensionManifestCompatibilitySubject<'_>,
        ) -> Option<ExtensionCompatibilityLevel> {
            Some(ExtensionCompatibilityLevel::Degraded)
        }
    }
    let degraded = admit_extension_manifest(
        fixture.binding(),
        &fixture.manifest,
        &DegradedPolicy(
            ExtensionCompatibilityTargetId::parse_exact("test.compatibility.v1").unwrap(),
        ),
    )
    .unwrap();
    assert_ne!(first.admission_digest(), degraded.admission_digest());
}

#[test]
fn duplicate_keys_binding_changes_and_nested_unknowns_fail_closed() {
    let duplicate =
        br#"{"manifest_version":3,"name":"X","version":"1","permissions":[],"permissions":[]}"#
            .to_vec();
    let fixture = fixture_bytes(duplicate, &[], false);
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    assert_eq!(
        admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy),
        Err(ExtensionManifestAdmissionError::Json(
            BoundedJsonError::DuplicateKey
        ))
    );

    let fixture = make_fixture(
        json!({
            "manifest_version":3,"name":"X","version":"1",
            "action":{"default_title":"x","future_execution_flag":true}
        }),
        &[],
        false,
    );
    assert!(matches!(
        admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy),
        Err(ExtensionManifestAdmissionError::InvalidField(_))
    ));

    let fixture = make_fixture(
        json!({"manifest_version":3,"name":"X","version":"1"}),
        &[],
        false,
    );
    let mut changed = fixture.manifest.clone();
    changed.push(b' ');
    assert_eq!(
        admit_extension_manifest(fixture.binding(), &changed, &policy),
        Err(ExtensionManifestAdmissionError::ManifestBindingMismatch)
    );
}

#[test]
fn mv3_and_content_script_defaults_are_explicit() {
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    for manifest_version in [2, 4] {
        let fixture = make_fixture(
            json!({"manifest_version":manifest_version,"name":"X","version":"1"}),
            &[],
            false,
        );
        assert!(matches!(
            admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy),
            Err(ExtensionManifestAdmissionError::InvalidField(_))
        ));
    }

    let fixture = make_fixture(
        json!({
            "manifest_version":3,"name":"X","version":"1",
            "content_scripts":[{
                "matches":["https://example.com/*"],
                "css":["content.css"]
            }]
        }),
        &[("content.css", b"body{}")],
        false,
    );
    let admitted = admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
    let script = &admitted
        .descriptor()
        .declarations()
        .execution()
        .content_scripts()[0];
    assert_eq!(script.run_at(), ExtensionContentScriptRunAt::DocumentIdle);
    assert_eq!(script.world(), ExtensionContentScriptWorld::Isolated);
    assert!(!script.all_frames());
    assert!(!script.matches().options().match_about_blank);
    assert!(!script.matches().options().match_origin_as_fallback);
    assert!(admitted.resources().content_scripts()[0]
        .javascript()
        .is_empty());
    assert_eq!(admitted.resources().content_scripts()[0].css().len(), 1);
}

#[test]
fn unknown_authority_is_preserved_blocking_and_never_defaulted_runnable() {
    let fixture = make_fixture(
        json!({
            "manifest_version":3,"name":"X","version":"1",
            "future_authority":{"can_execute":true}
        }),
        &[],
        false,
    );
    let unsupported = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    let admitted =
        admit_extension_manifest(fixture.binding(), &fixture.manifest, &unsupported).unwrap();
    assert_eq!(
        admitted.descriptor().declarations().unmodeled()[0].as_str(),
        "future_authority"
    );

    let runnable = CompletePolicy::new(ExtensionCompatibilityLevel::Compatible);
    assert!(matches!(
        admit_extension_manifest(fixture.binding(), &fixture.manifest, &runnable),
        Err(ExtensionManifestAdmissionError::RunnableUnmodeledDeclaration(_))
    ));

    let invalid_name = make_fixture(
        json!({"manifest_version":3,"name":"X","version":"1","future authority":true}),
        &[],
        false,
    );
    assert!(matches!(
        admit_extension_manifest(invalid_name.binding(), &invalid_name.manifest, &unsupported),
        Err(ExtensionManifestAdmissionError::InvalidUnmodeledDeclaration(_))
    ));
}

#[test]
fn every_declaration_requires_an_explicit_product_policy_decision() {
    struct MissingBackgroundPolicy(ExtensionCompatibilityTargetId);
    impl ExtensionManifestCompatibilityPolicy for MissingBackgroundPolicy {
        fn target(&self) -> &ExtensionCompatibilityTargetId {
            &self.0
        }

        fn classify(
            &self,
            subject: ExtensionManifestCompatibilitySubject<'_>,
        ) -> Option<ExtensionCompatibilityLevel> {
            (!matches!(
                subject.declaration(),
                ExtensionManifestDeclaration::Background
            ))
            .then_some(ExtensionCompatibilityLevel::Compatible)
        }
    }

    let fixture = make_fixture(
        json!({
            "manifest_version":3,"name":"X","version":"1",
            "background":{"service_worker":"sw.js"}
        }),
        &[("sw.js", b"worker")],
        false,
    );
    let policy = MissingBackgroundPolicy(
        ExtensionCompatibilityTargetId::parse_exact("test.missing.v1").unwrap(),
    );
    assert_eq!(
        admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy),
        Err(ExtensionManifestAdmissionError::UnclassifiedDeclaration(
            ExtensionManifestDeclaration::Background
        ))
    );
}

#[test]
fn resources_localization_and_chromium_identity_are_exactly_bound() {
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    let missing = make_fixture(
        json!({
            "manifest_version":3,"name":"X","version":"1",
            "background":{"service_worker":"missing.js"}
        }),
        &[],
        false,
    );
    assert!(matches!(
        admit_extension_manifest(missing.binding(), &missing.manifest, &policy),
        Err(ExtensionManifestAdmissionError::InvalidResource(_))
    ));

    let localized = make_fixture(
        json!({"manifest_version":3,"name":"__MSG_name__","version":"1","default_locale":"en"}),
        &[],
        false,
    );
    assert!(matches!(
        admit_extension_manifest(localized.binding(), &localized.manifest, &policy),
        Err(ExtensionManifestAdmissionError::InvalidResource(_))
    ));

    let localized_action = make_fixture(
        json!({
            "manifest_version":3,"name":"X","version":"1",
            "action":{"default_title":"__MSG_action_title__"}
        }),
        &[],
        false,
    );
    assert!(matches!(
        admit_extension_manifest(
            localized_action.binding(),
            &localized_action.manifest,
            &policy
        ),
        Err(ExtensionManifestAdmissionError::InvalidField(_))
    ));

    let locale_without_default = make_fixture(
        json!({"manifest_version":3,"name":"X","version":"1"}),
        &[("_locales/en/messages.json", b"{}")],
        false,
    );
    assert!(matches!(
        admit_extension_manifest(
            locale_without_default.binding(),
            &locale_without_default.manifest,
            &policy
        ),
        Err(ExtensionManifestAdmissionError::InvalidField(_))
    ));

    let oversized_messages = vec![b'x'; crate::MAX_EXTENSION_LOCALE_MESSAGES_BYTES as usize + 1];
    let oversized_locale = make_fixture(
        json!({
            "manifest_version":3,"name":"__MSG_name__","version":"1","default_locale":"en"
        }),
        &[("_locales/en/messages.json", oversized_messages.as_slice())],
        false,
    );
    assert!(matches!(
        admit_extension_manifest(
            oversized_locale.binding(),
            &oversized_locale.manifest,
            &policy
        ),
        Err(ExtensionManifestAdmissionError::InvalidField(_))
    ));

    let missing_key = make_fixture(
        json!({"manifest_version":3,"name":"X","version":"1"}),
        &[],
        true,
    );
    assert_eq!(
        admit_extension_manifest(missing_key.binding(), &missing_key.manifest, &policy),
        Err(ExtensionManifestAdmissionError::ChromiumKeyMissing)
    );
    let unexpected_key = make_fixture(
        json!({"manifest_version":3,"name":"X","version":"1","key":"Xw=="}),
        &[],
        false,
    );
    assert_eq!(
        admit_extension_manifest(unexpected_key.binding(), &unexpected_key.manifest, &policy),
        Err(ExtensionManifestAdmissionError::ChromiumKeyUnexpected)
    );
    let wrong_key = make_fixture(
        json!({"manifest_version":3,"name":"X","version":"1","key":"WA=="}),
        &[],
        true,
    );
    assert_eq!(
        admit_extension_manifest(wrong_key.binding(), &wrong_key.manifest, &policy),
        Err(ExtensionManifestAdmissionError::ChromiumIdentityMismatch)
    );
}

#[test]
fn version_paths_display_text_and_homepage_are_unambiguous() {
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    for version in ["0", "0.0", "0.0.0.0", "01", "1.0.0.0.0", "65536"] {
        let fixture = make_fixture(
            json!({"manifest_version":3,"name":"X","version":version}),
            &[],
            false,
        );
        assert!(matches!(
            admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy),
            Err(ExtensionManifestAdmissionError::InvalidField(_))
        ));
    }

    for name in ["line\nbreak", "spoof\u{202e}txt", "isolate\u{2067}text"] {
        let fixture = make_fixture(
            json!({"manifest_version":3,"name":name,"version":"1"}),
            &[],
            false,
        );
        assert!(matches!(
            admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy),
            Err(ExtensionManifestAdmissionError::InvalidField(_))
        ));
    }

    let double_root = make_fixture(
        json!({
            "manifest_version":3,"name":"X","version":"1",
            "background":{"service_worker":"//sw.js"}
        }),
        &[("sw.js", b"worker")],
        false,
    );
    assert!(matches!(
        admit_extension_manifest(double_root.binding(), &double_root.manifest, &policy),
        Err(ExtensionManifestAdmissionError::InvalidResource(_))
    ));

    let canonical_homepage = make_fixture(
        json!({
            "manifest_version":3,"name":"X","version":"1",
            "homepage_url":"HTTPS://EXAMPLE.COM:443/project"
        }),
        &[],
        false,
    );
    let admitted = admit_extension_manifest(
        canonical_homepage.binding(),
        &canonical_homepage.manifest,
        &policy,
    )
    .unwrap();
    assert_eq!(
        admitted.metadata().homepage_url(),
        Some("https://example.com/project")
    );

    for homepage_url in [
        format!(
            "https://example.com/{}",
            "a".repeat(crate::MAX_EXTENSION_METADATA_STRING_BYTES)
        ),
        "https://example.com/line\nbreak".to_owned(),
        format!("https://example.com/{}", "é".repeat(1_000)),
    ] {
        let fixture = make_fixture(
            json!({
                "manifest_version":3,"name":"X","version":"1",
                "homepage_url": homepage_url
            }),
            &[],
            false,
        );
        assert!(matches!(
            admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy),
            Err(ExtensionManifestAdmissionError::InvalidField(_))
        ));
    }
}

#[test]
fn icon_formats_sizes_and_canonical_paths_are_unambiguous() {
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    for extension in ["svg", "webp", "txt"] {
        let path = format!("icon.{extension}");
        let manifest = json!({
            "manifest_version":3,"name":"X","version":"1","icons":{"16":path}
        });
        let fixture = make_fixture(manifest, &[(path.as_str(), b"icon")], false);
        assert!(matches!(
            admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy),
            Err(ExtensionManifestAdmissionError::InvalidField(_))
        ));
    }

    let duplicate = make_fixture(
        json!({
            "manifest_version":3,"name":"X","version":"1",
            "icons":{"16":"icon.png","32":"icon.png"}
        }),
        &[("icon.png", b"icon")],
        false,
    );
    assert!(matches!(
        admit_extension_manifest(duplicate.binding(), &duplicate.manifest, &policy),
        Err(ExtensionManifestAdmissionError::InvalidField(_))
    ));

    let admitted = make_fixture(
        json!({
            "manifest_version":3,"name":"X","version":"1",
            "icons":{"16":"icon16.png","32":"icon32.jpg"}
        }),
        &[("icon16.png", b"16"), ("icon32.jpg", b"32")],
        false,
    );
    let admitted =
        admit_extension_manifest(admitted.binding(), &admitted.manifest, &policy).unwrap();
    assert_eq!(admitted.metadata().icons()[0].size(), Some(16));
    assert_eq!(admitted.metadata().icons()[1].size(), Some(32));
}

#[test]
fn hostile_collection_and_resource_pattern_shapes_are_rejected() {
    let permissions = (0..=MAX_EXTENSION_API_PERMISSIONS)
        .map(|index| format!("permission{index}"))
        .collect::<Vec<_>>();
    let fixture = make_fixture(
        json!({"manifest_version":3,"name":"X","version":"1","permissions":permissions}),
        &[],
        false,
    );
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    assert!(matches!(
        admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy),
        Err(ExtensionManifestAdmissionError::InvalidField(_))
    ));

    for resource in [
        "../secret",
        "images\\*",
        "images/**/../secret",
        "//images/*",
    ] {
        let fixture = make_fixture(
            json!({
                "manifest_version":3,"name":"X","version":"1",
                "web_accessible_resources":[{
                    "resources":[resource],"matches":["https://example.com/*"]
                }]
            }),
            &[],
            false,
        );
        assert!(matches!(
            admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy),
            Err(ExtensionManifestAdmissionError::InvalidField(_))
        ));
    }

    let non_origin_web_access = make_fixture(
        json!({
            "manifest_version":3,"name":"X","version":"1",
            "web_accessible_resources":[{
                "resources":["asset.png"],
                "matches":["https://example.com/private/*"]
            }]
        }),
        &[("asset.png", b"asset")],
        false,
    );
    assert!(matches!(
        admit_extension_manifest(
            non_origin_web_access.binding(),
            &non_origin_web_access.manifest,
            &policy
        ),
        Err(ExtensionManifestAdmissionError::InvalidField(_))
    ));

    let aliased_web_resource = make_fixture(
        json!({
            "manifest_version":3,"name":"X","version":"1",
            "web_accessible_resources":[{
                "resources":["asset.png","/asset.png"],
                "matches":["https://example.com/*"]
            }]
        }),
        &[("asset.png", b"asset")],
        false,
    );
    assert!(matches!(
        admit_extension_manifest(
            aliased_web_resource.binding(),
            &aliased_web_resource.manifest,
            &policy
        ),
        Err(ExtensionManifestAdmissionError::InvalidField(_))
    ));
}

proptest! {
    #[test]
    fn admitted_metadata_and_digest_are_stable_for_bounded_inputs(
        name in "[A-Za-z][A-Za-z0-9 _-]{0,30}",
        major in 0_u16..=999,
        minor in 0_u16..=999,
    ) {
        prop_assume!(!name.contains("__MSG_"));
        let version = format!("{major}.{minor}");
        let fixture = make_fixture(
            json!({"manifest_version":3,"name":name,"version":version}),
            &[],
            false,
        );
        let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
        let first = admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
        let second = admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
        prop_assert_eq!(first.metadata().name(), name.as_str());
        prop_assert_eq!(first.metadata().version(), version.as_str());
        prop_assert_eq!(first.admission_digest(), second.admission_digest());
        prop_assert!(first.retained_bytes() >= first.metadata().retained_bytes());
    }
}
