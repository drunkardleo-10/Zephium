use proptest::prelude::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use zephium_core::extensions::{
    ExtensionCompatibilityLevel, ExtensionCompatibilityTargetId, ExtensionContentScriptRunAt,
    ExtensionContentScriptWorld, ExtensionManifestDeclaration,
};

use super::csp::{DEFAULT_EXTENSION_PAGES_CSP, DEFAULT_SANDBOX_CSP};
use super::*;
use crate::{
    CanonicalExtensionTreeIndex, ExtensionReleaseCatalog, MAX_EXTENSION_METADATA_STRING_BYTES,
    MAX_EXTENSION_RESOLVED_METADATA_RETAINED_BYTES,
};

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
            r#"{{"schema_version":1,"catalog_revision":1,"created_unix":1,"authority_id":"{}","admission_policy_sha256":"{}","packages":[{{"package_key":"{}","revision":1,"payload":{{"kind":"bundled_tree"}},"manifest_sha256":"{}","tree_sha256":"{}","tree_index_sha256":"{}","tree_index_length":{},"tree_file_count":{},"tree_bytes":{},"chromium":{},"provenance":{{"source_url":"https://example.com/releases/v1/source","upstream_version":"1","upstream_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","license_expression":"MPL-2.0","attribution":"Example","redistribution":"Reviewed","legal_notice":{{"target":"licenses/example.txt","kind":"notice_bundle","length":1,"sha256":"{}"}},"corresponding_source":null}}}}]}}"#,
        ),
        hex([1; 32]),
        hex([2; 32]),
        hex([3; 32]),
        hex(tree.manifest_sha256().bytes()),
        hex(tree.tree_sha256().bytes()),
        hex(tree.index_sha256().bytes()),
        tree.index_bytes(),
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

fn bitwarden_2026_7_0_contract_fixture() -> Fixture {
    let manifest = json!({
        "manifest_version": 3,
        "minimum_chrome_version": "102.0",
        "name": "__MSG_extName__",
        "short_name": "Bitwarden",
        "version": "2026.7.0",
        "description": "__MSG_extDesc__",
        "default_locale": "en",
        "author": "Bitwarden Inc.",
        "homepage_url": "https://bitwarden.com",
        "icons": {"16": "images/icon16.png"},
        "content_scripts": [
            {
                "all_frames": false,
                "js": ["content/content-message-handler.js"],
                "matches": ["*://*/*", "file:///*"],
                "exclude_matches": ["*://*/*.xml*", "file:///*.xml*"],
                "run_at": "document_start"
            },
            {
                "all_frames": true,
                "css": ["content/autofill.css"],
                "js": ["content/trigger-autofill-script-injection.js"],
                "matches": ["*://*/*", "file:///*"],
                "exclude_matches": ["*://*/*.xml*", "file:///*.xml*"],
                "run_at": "document_start"
            }
        ],
        "background": {"service_worker": "background.js"},
        "action": {
            "default_icon": {"19": "images/icon19.png"},
            "default_title": "Bitwarden",
            "default_popup": "popup/index.html"
        },
        "permissions": [
            "activeTab", "alarms", "clipboardRead", "clipboardWrite", "contextMenus",
            "idle", "offscreen", "scripting", "sidePanel", "storage", "tabs",
            "unlimitedStorage", "webNavigation", "webRequest", "webRequestAuthProvider",
            "notifications"
        ],
        "optional_permissions": ["nativeMessaging", "privacy"],
        "host_permissions": ["https://*/*", "http://*/*"],
        "content_security_policy": {
            "extension_pages": "script-src 'self' 'wasm-unsafe-eval'; object-src 'self'",
            "sandbox": "sandbox allow-scripts; script-src 'self'"
        },
        "sandbox": {"pages": ["overlay/menu-button.html", "overlay/menu-list.html"]},
        "side_panel": {"default_path": "sidepanel-disabled.html"},
        "commands": {
            "_execute_action": {
                "suggested_key": {"default": "Ctrl+Shift+Y", "linux": "Ctrl+Shift+U"},
                "description": "__MSG_commandOpenPopup__"
            },
            "autofill_login": {
                "suggested_key": {"default": "Ctrl+Shift+L"},
                "description": "__MSG_commandAutofillLoginDesc__"
            },
            "autofill_card": {"description": "__MSG_commandAutofillCardDesc__"},
            "autofill_identity": {"description": "__MSG_commandAutofillIdentityDesc__"},
            "generate_password": {
                "suggested_key": {"default": "Ctrl+Shift+9"},
                "description": "__MSG_commandGeneratePasswordDesc__"
            },
            "lock_vault": {"description": "__MSG_commandLockVaultDesc__"}
        },
        "web_accessible_resources": [{
            "resources": [
                "content/fido2-page-script.js", "notification/bar.html", "images/icon38.png",
                "images/icon38_locked.png", "overlay/menu-button.html", "overlay/menu-list.html",
                "overlay/menu.html", "popup/fonts/*"
            ],
            "matches": ["<all_urls>"],
            "use_dynamic_url": true
        }],
        "storage": {"managed_schema": "managed_schema.json"}
    });
    make_fixture(
        manifest,
        &[
            (
                "_locales/en/messages.json",
                br#"{"extName":{"message":"Bitwarden Password Manager"},"extDesc":{"message":"Password manager"}}"#,
            ),
            ("background.js", b"void 0"),
            ("content/autofill.css", b"body{}"),
            ("content/content-message-handler.js", b"void 0"),
            ("content/fido2-page-script.js", b"void 0"),
            ("content/trigger-autofill-script-injection.js", b"void 0"),
            ("images/icon16.png", b"icon"),
            ("images/icon19.png", b"icon"),
            ("images/icon38.png", b"icon"),
            ("images/icon38_locked.png", b"icon"),
            ("managed_schema.json", b"{}"),
            ("notification/bar.html", b"<main></main>"),
            ("overlay/menu-button.html", b"<main></main>"),
            ("overlay/menu-list.html", b"<main></main>"),
            ("overlay/menu.html", b"<main></main>"),
            ("popup/fonts/font.woff2", b"font"),
            ("popup/index.html", b"<main></main>"),
            ("sidepanel-disabled.html", b"<main></main>"),
        ],
        false,
    )
}

#[test]
fn admits_complete_mv3_authority_without_losing_runtime_paths() {
    let fixture = full_fixture();
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    let admitted = admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();

    assert_eq!(
        admitted
            .metadata()
            .name()
            .localized_message_key()
            .unwrap()
            .as_str(),
        "extension_name"
    );
    assert_eq!(admitted.metadata().version(), "1.2.3");
    assert_eq!(admitted.metadata().default_locale(), Some("en"));
    assert_eq!(
        admitted
            .metadata()
            .action_title()
            .and_then(ExtensionUnresolvedDisplayText::localized_message_key)
            .map(ExtensionLocalizedMessageKey::as_str),
        Some("action_title")
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
    assert!(declarations.unmodeled().is_empty());
    let commands = declarations.additional().commands().unwrap();
    assert_eq!(commands.command_count(), 1);
    assert_eq!(
        admitted
            .descriptor()
            .compatibility_for(&ExtensionManifestDeclaration::Commands(commands)),
        Some(ExtensionCompatibilityLevel::Compatible)
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
fn admits_the_pinned_bitwarden_manifest_shape_without_unmodeled_authority() {
    let fixture = bitwarden_2026_7_0_contract_fixture();
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    let admitted = admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
    let declarations = admitted.descriptor().declarations();

    assert!(declarations.unmodeled().is_empty());
    assert_eq!(declarations.required_api().len(), 16);
    assert_eq!(declarations.optional_api().len(), 2);
    assert_eq!(declarations.execution().content_scripts().len(), 2);
    assert_eq!(
        declarations
            .additional()
            .minimum_chromium_version()
            .unwrap()
            .components(),
        &[102, 0]
    );
    assert_eq!(
        declarations
            .additional()
            .commands()
            .unwrap()
            .command_count(),
        6
    );
    assert!(declarations.additional().side_panel_resource().is_some());
    assert!(declarations
        .additional()
        .managed_storage_schema_resource()
        .is_some());
    assert!(admitted
        .resources()
        .auxiliary_resources()
        .iter()
        .any(|resource| resource.path().as_str() == "sidepanel-disabled.html"));
    assert!(admitted
        .resources()
        .auxiliary_resources()
        .iter()
        .any(|resource| resource.path().as_str() == "managed_schema.json"));
}

#[test]
fn browser_declarations_are_strict_and_resource_bound() {
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    for manifest in [
        json!({
            "manifest_version": 3,
            "name": "X",
            "version": "1",
            "minimum_chrome_version": "102.00"
        }),
        json!({
            "manifest_version": 3,
            "name": "X",
            "version": "1",
            "commands": {"open": {"unexpected": true}}
        }),
        json!({
            "manifest_version": 3,
            "name": "X",
            "version": "1",
            "side_panel": {"default_path": "panel.html", "unexpected": true}
        }),
        json!({
            "manifest_version": 3,
            "name": "X",
            "version": "1",
            "storage": {"managed_schema": "schema.json", "unexpected": true}
        }),
    ] {
        let fixture = make_fixture(
            manifest,
            &[("panel.html", b"panel"), ("schema.json", b"{}")],
            false,
        );
        assert!(matches!(
            admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy),
            Err(ExtensionManifestAdmissionError::InvalidField(_))
        ));
    }

    let missing = make_fixture(
        json!({
            "manifest_version": 3,
            "name": "X",
            "version": "1",
            "side_panel": {"default_path": "missing.html"}
        }),
        &[],
        false,
    );
    assert!(matches!(
        admit_extension_manifest(missing.binding(), &missing.manifest, &policy),
        Err(ExtensionManifestAdmissionError::InvalidResource(_))
    ));
}

#[test]
fn command_semantics_change_the_typed_compatibility_identity() {
    let fixture = |shortcut: &str| {
        make_fixture(
            json!({
                "manifest_version": 3,
                "name": "X",
                "version": "1",
                "commands": {
                    "open": {
                        "description": "Open",
                        "suggested_key": {"default": shortcut}
                    }
                }
            }),
            &[],
            false,
        )
    };
    let first = fixture("Ctrl+Shift+Y");
    let second = fixture("Ctrl+Shift+U");
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    let first = admit_extension_manifest(first.binding(), &first.manifest, &policy).unwrap();
    let second = admit_extension_manifest(second.binding(), &second.manifest, &policy).unwrap();

    assert_ne!(
        first
            .descriptor()
            .declarations()
            .additional()
            .commands()
            .unwrap()
            .descriptor_digest(),
        second
            .descriptor()
            .declarations()
            .additional()
            .commands()
            .unwrap()
            .descriptor_digest()
    );
    assert_ne!(
        first.descriptor().compatibility_digest(),
        second.descriptor().compatibility_digest()
    );
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
            223, 211, 119, 169, 150, 190, 92, 169, 77, 221, 228, 20, 48, 14, 136, 33, 211, 199, 83,
            139, 96, 37, 236, 50, 40, 2, 207, 22, 133, 96, 60, 86,
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
fn options_ui_is_typed_resource_bound_and_no_longer_unmodeled_authority() {
    let fixture = make_fixture(
        json!({
            "manifest_version":3,"name":"X","version":"1",
            "options_ui": {
                "page": "options.html",
                "open_in_tab": true,
                "browser_style": false
            }
        }),
        &[("options.html", b"<!doctype html><title>Options</title>")],
        false,
    );
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Compatible);
    let admitted = admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
    assert!(admitted.descriptor().declarations().unmodeled().is_empty());
    assert!(admitted
        .descriptor()
        .compatibility()
        .iter()
        .any(|classification| matches!(
            classification.declaration(),
            ExtensionManifestDeclaration::OptionsPage { .. }
        )));
    assert_eq!(admitted.resources().auxiliary_resources().len(), 1);
    assert_eq!(
        admitted.resources().auxiliary_resources()[0]
            .path()
            .as_str(),
        "options.html"
    );
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

#[test]
fn default_locale_resolution_binds_exact_bytes_and_returns_only_trusted_text() {
    let messages = br#"{
        "extension_name":{"message":"Bitwarden Password Manager","description":"Name"},
        "EXTENSION_DESCRIPTION":{"message":"A secure password manager"},
        "short_name":{"message":"Bitwarden"},
        "version_name":{"message":"Stable"},
        "author":{"message":"Bitwarden Inc."},
        "action_title":{"message":"Open secure vault"},
        "runtime_placeholder":{"message":"Found $COUNT$ items","placeholders":{
            "count":{"content":"$1","example":"3"}
        }}
    }"#;
    let fixture = make_fixture(
        json!({
            "manifest_version":3,
            "name":"__MSG_EXTENSION_NAME__",
            "version":"1",
            "description":"__MSG_extension_description__",
            "short_name":"__MSG_short_name__",
            "version_name":"Stable",
            "author":"Bitwarden Inc.",
            "default_locale":"en",
            "action":{"default_title":"__MSG_action_title__"}
        }),
        &[("_locales/en/messages.json", messages)],
        false,
    );
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    let admitted = admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
    assert_eq!(
        admitted
            .metadata()
            .name()
            .localized_message_key()
            .unwrap()
            .as_str(),
        "extension_name"
    );
    assert_eq!(admitted.metadata().name().literal_text(), None);

    let first = resolve_extension_default_locale(&admitted, Some(messages)).unwrap();
    let second = resolve_extension_default_locale(&admitted, Some(messages)).unwrap();
    let projected =
        resolve_extension_metadata_default_locale(admitted.metadata(), Some(messages)).unwrap();
    assert_eq!(first.name().as_str(), "Bitwarden Password Manager");
    assert_eq!(
        first.description().unwrap().as_str(),
        "A secure password manager"
    );
    assert_eq!(first.short_name().unwrap().as_str(), "Bitwarden");
    assert_eq!(first.version_name().unwrap().as_str(), "Stable");
    assert_eq!(first.author().unwrap().as_str(), "Bitwarden Inc.");
    assert_eq!(first.action_title().unwrap().as_str(), "Open secure vault");
    assert_eq!(first.default_locale(), Some("en"));
    assert_eq!(
        first.locale_messages_path(),
        Some("_locales/en/messages.json")
    );
    assert_eq!(
        first.locale_messages_length(),
        Some(u64::try_from(messages.len()).unwrap())
    );
    assert_eq!(
        first.locale_messages_sha256(),
        Some(Sha256::digest(messages).into())
    );
    assert_eq!(first.digest(), second.digest());
    assert_eq!(
        first.admitted_metadata_sha256(),
        *admitted.metadata().digest()
    );
    assert_eq!(
        first.digest().bytes(),
        [
            159, 33, 175, 239, 47, 145, 160, 182, 85, 158, 84, 188, 161, 144, 246, 108, 185, 85,
            176, 218, 120, 180, 50, 198, 242, 98, 176, 81, 237, 177, 211, 164,
        ]
    );
    assert_eq!(first, second);
    assert_eq!(first, projected);
    assert!(first.retained_bytes() <= MAX_EXTENSION_RESOLVED_METADATA_RETAINED_BYTES);
}

#[test]
fn literal_metadata_resolves_without_locale_and_extra_bytes_are_refused() {
    let fixture = make_fixture(
        json!({
            "manifest_version":3,
            "name":"Literal",
            "version":"1",
            "description":"Description",
            "short_name":"Short"
        }),
        &[],
        false,
    );
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    let admitted = admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
    let resolved = resolve_extension_default_locale(&admitted, None).unwrap();
    assert_eq!(resolved.name().as_str(), "Literal");
    assert_eq!(resolved.description().unwrap().as_str(), "Description");
    assert_eq!(resolved.locale_messages_path(), None);
    assert_eq!(resolved.locale_messages_length(), None);
    assert_eq!(resolved.locale_messages_sha256(), None);
    assert_eq!(
        resolve_extension_default_locale(&admitted, Some(b"{}")),
        Err(ExtensionDefaultLocaleResolutionError::UnexpectedLocaleMessages)
    );
}

#[test]
fn locale_binding_and_duplicate_key_failures_are_distinguished() {
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    for (messages, expected) in [
        (
            b"[1]".as_slice(),
            ExtensionDefaultLocaleResolutionError::RootNotObject,
        ),
        (
            b"{".as_slice(),
            ExtensionDefaultLocaleResolutionError::Json(BoundedJsonError::Malformed),
        ),
    ] {
        let fixture = make_fixture(
            json!({
                "manifest_version":3,"name":"__MSG_name__","version":"1","default_locale":"en"
            }),
            &[("_locales/en/messages.json", messages)],
            false,
        );
        let admitted =
            admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
        assert_eq!(
            resolve_extension_default_locale(&admitted, Some(messages)),
            Err(expected)
        );
    }

    let exact_duplicate = br#"{"name":{"message":"one","message":"two"}}"#;
    let fixture = make_fixture(
        json!({
            "manifest_version":3,"name":"__MSG_name__","version":"1","default_locale":"en"
        }),
        &[("_locales/en/messages.json", exact_duplicate)],
        false,
    );
    let admitted = admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
    assert_eq!(
        resolve_extension_default_locale(&admitted, None),
        Err(ExtensionDefaultLocaleResolutionError::MissingLocaleMessages)
    );
    assert_eq!(
        resolve_extension_default_locale(&admitted, Some(b"{}")),
        Err(ExtensionDefaultLocaleResolutionError::LocaleMessagesBindingMismatch)
    );
    assert_eq!(
        resolve_extension_default_locale(&admitted, Some(exact_duplicate)),
        Err(ExtensionDefaultLocaleResolutionError::Json(
            BoundedJsonError::DuplicateKey
        ))
    );

    let case_collision = br#"{
        "Name":{"message":"one"},
        "name":{"message":"two"}
    }"#;
    let fixture = make_fixture(
        json!({
            "manifest_version":3,"name":"__MSG_name__","version":"1","default_locale":"en"
        }),
        &[("_locales/en/messages.json", case_collision)],
        false,
    );
    let admitted = admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
    assert_eq!(
        resolve_extension_default_locale(&admitted, Some(case_collision)),
        Err(ExtensionDefaultLocaleResolutionError::AmbiguousMessageKey(
            "name".into()
        ))
    );
}

#[test]
fn locale_message_and_placeholder_shapes_fail_closed() {
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    for messages in [
        br#"{"bad-key":{"message":"x"}}"#.as_slice(),
        br#"{"@@ui_locale":{"message":"x"}}"#.as_slice(),
        br#"{"name":{"message":"$missing$"}}"#.as_slice(),
        br#"{"name":{"message":"$value$","placeholders":{"value":{"example":"x"}}}}"#
            .as_slice(),
        br#"{"name":{"message":"$value$","placeholders":{"Value":{"content":"$1"},"value":{"content":"$1"}}}}"#
            .as_slice(),
        br#"{"name":{"message":"$value$","placeholders":{"value":{"content":"${1}"}}}}"#
            .as_slice(),
        br#"{"name":{"message":"x","future":"authority"}}"#.as_slice(),
    ] {
        let fixture = make_fixture(
            json!({
                "manifest_version":3,"name":"__MSG_name__","version":"1","default_locale":"en"
            }),
            &[("_locales/en/messages.json", messages)],
            false,
        );
        let admitted =
            admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
        assert!(matches!(
            resolve_extension_default_locale(&admitted, Some(messages)),
            Err(ExtensionDefaultLocaleResolutionError::InvalidMessageKey(_)
                | ExtensionDefaultLocaleResolutionError::InvalidMessageEntry(_)
                | ExtensionDefaultLocaleResolutionError::InvalidPlaceholder(_))
        ));
    }

    let missing = br#"{"other":{"message":"x"}}"#;
    let fixture = make_fixture(
        json!({
            "manifest_version":3,"name":"__MSG_name__","version":"1","default_locale":"en"
        }),
        &[("_locales/en/messages.json", missing)],
        false,
    );
    let admitted = admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
    assert_eq!(
        resolve_extension_default_locale(&admitted, Some(missing)),
        Err(ExtensionDefaultLocaleResolutionError::MissingMessage(
            "name".into()
        ))
    );
}

#[test]
fn selected_runtime_substitutions_and_unsafe_or_oversized_text_are_refused() {
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    let substitution = br#"{
        "name":{"message":"Hello $USER$","placeholders":{"user":{"content":"$1"}}}
    }"#;
    let fixture = make_fixture(
        json!({
            "manifest_version":3,"name":"__MSG_name__","version":"1","default_locale":"en"
        }),
        &[("_locales/en/messages.json", substitution)],
        false,
    );
    let admitted = admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
    assert_eq!(
        resolve_extension_default_locale(&admitted, Some(substitution)),
        Err(ExtensionDefaultLocaleResolutionError::UnsupportedDisplayDollarSyntax("name".into()))
    );

    for value in ["$$", "$$name$$", "$1$2"] {
        let messages = serde_json::to_vec(&json!({"name":{"message":value}})).unwrap();
        let fixture = make_fixture(
            json!({
                "manifest_version":3,"name":"__MSG_name__","version":"1","default_locale":"en"
            }),
            &[("_locales/en/messages.json", &messages)],
            false,
        );
        let admitted =
            admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
        assert_eq!(
            resolve_extension_default_locale(&admitted, Some(&messages)),
            Err(
                ExtensionDefaultLocaleResolutionError::UnsupportedDisplayDollarSyntax(
                    "name".into()
                )
            )
        );
    }

    for value in [
        String::new(),
        " \u{200c}\u{200d}\u{fe0f}".to_owned(),
        "x".repeat(76),
        "unsafe\u{202e}name".to_owned(),
        "line\nbreak".to_owned(),
    ] {
        let messages = serde_json::to_vec(&json!({"name":{"message":value}})).unwrap();
        let fixture = make_fixture(
            json!({
                "manifest_version":3,"name":"__MSG_name__","version":"1","default_locale":"en"
            }),
            &[("_locales/en/messages.json", &messages)],
            false,
        );
        let admitted =
            admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
        assert_eq!(
            resolve_extension_default_locale(&admitted, Some(&messages)),
            Err(ExtensionDefaultLocaleResolutionError::InvalidResolvedField(
                "name"
            ))
        );
    }

    let long_author = "a".repeat(MAX_EXTENSION_METADATA_STRING_BYTES + 1);
    let fixture = make_fixture(
        json!({
            "manifest_version":3,"name":"X","author":long_author,"version":"1"
        }),
        &[],
        false,
    );
    assert!(matches!(
        admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy),
        Err(ExtensionManifestAdmissionError::InvalidField(_))
    ));
}

#[test]
fn every_resolved_metadata_field_uses_its_post_resolution_limit() {
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    let boundary_messages = serde_json::to_vec(&json!({
        "name":{"message":"n".repeat(75)},
        "description":{"message":"d".repeat(132)},
        "short":{"message":"s".repeat(12)},
        "action":{"message":"t".repeat(MAX_EXTENSION_METADATA_STRING_BYTES)}
    }))
    .unwrap();
    let boundary = make_fixture(
        json!({
            "manifest_version":3,"name":"__MSG_name__","version":"1",
            "description":"__MSG_description__","short_name":"__MSG_short__",
            "version_name":"v".repeat(MAX_EXTENSION_METADATA_STRING_BYTES),
            "author":"a".repeat(MAX_EXTENSION_METADATA_STRING_BYTES),
            "action":{"default_title":"__MSG_action__"},"default_locale":"en"
        }),
        &[("_locales/en/messages.json", &boundary_messages)],
        false,
    );
    let admitted =
        admit_extension_manifest(boundary.binding(), &boundary.manifest, &policy).unwrap();
    let resolved = resolve_extension_default_locale(&admitted, Some(&boundary_messages)).unwrap();
    assert_eq!(resolved.name().as_str().chars().count(), 75);
    assert_eq!(
        resolved.description().unwrap().as_str().chars().count(),
        132
    );
    assert_eq!(resolved.short_name().unwrap().as_str().chars().count(), 12);
    assert_eq!(
        resolved.version_name().unwrap().as_str().chars().count(),
        MAX_EXTENSION_METADATA_STRING_BYTES
    );
    assert_eq!(
        resolved.author().unwrap().as_str().chars().count(),
        MAX_EXTENSION_METADATA_STRING_BYTES
    );

    for (manifest, message_key, value, expected_field) in [
        (
            json!({
                "manifest_version":3,"name":"X","version":"1",
                "description":"__MSG_target__","default_locale":"en"
            }),
            "target",
            "d".repeat(133),
            "description",
        ),
        (
            json!({
                "manifest_version":3,"name":"X","version":"1",
                "short_name":"__MSG_target__","default_locale":"en"
            }),
            "target",
            "s".repeat(13),
            "short_name",
        ),
    ] {
        let messages = serde_json::to_vec(&json!({message_key:{"message":value}})).unwrap();
        let fixture = make_fixture(manifest, &[("_locales/en/messages.json", &messages)], false);
        let admitted =
            admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
        assert_eq!(
            resolve_extension_default_locale(&admitted, Some(&messages)),
            Err(ExtensionDefaultLocaleResolutionError::InvalidResolvedField(
                expected_field
            ))
        );
    }
}

#[test]
fn identity_fields_require_stable_alphanumeric_unicode_scalars() {
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    let egyptian_format_controls = (0x13430..=0x13455)
        .map(|scalar| char::from_u32(scalar).unwrap())
        .collect::<String>();
    let alphanumeric_hangul_fillers = "\u{115f}\u{1160}\u{3164}\u{ffa0}";
    assert!(alphanumeric_hangul_fillers
        .chars()
        .all(char::is_alphanumeric));
    let invalid_identities = [
        (
            "\u{fff9}\u{fffa}\u{fffb}".to_owned(),
            "\u{fff9}\u{fffa}\u{fffb}",
        ),
        (egyptian_format_controls, "\u{13430}\u{13431}\u{13455}"),
        (
            alphanumeric_hangul_fillers.to_owned(),
            alphanumeric_hangul_fillers,
        ),
        ("😀!?—".to_owned(), "😀!?—"),
        (
            " \u{200c}\u{200d}\u{feff}\u{fe0f}".to_owned(),
            " \u{200c}\u{200d}\u{feff}\u{fe0f}",
        ),
    ];

    for (name, short_name) in &invalid_identities {
        let literal_name = make_fixture(
            json!({"manifest_version":3,"name":name,"version":"1"}),
            &[],
            false,
        );
        assert!(matches!(
            admit_extension_manifest(literal_name.binding(), &literal_name.manifest, &policy),
            Err(ExtensionManifestAdmissionError::InvalidField(_))
        ));

        let literal_short_name = make_fixture(
            json!({
                "manifest_version":3,"name":"Valid","short_name":short_name,"version":"1"
            }),
            &[],
            false,
        );
        assert!(matches!(
            admit_extension_manifest(
                literal_short_name.binding(),
                &literal_short_name.manifest,
                &policy
            ),
            Err(ExtensionManifestAdmissionError::InvalidField(_))
        ));

        for (field, manifest) in [
            (
                "name",
                json!({
                    "manifest_version":3,"name":"__MSG_identity__",
                    "version":"1","default_locale":"en"
                }),
            ),
            (
                "short_name",
                json!({
                    "manifest_version":3,"name":"Valid","short_name":"__MSG_identity__",
                    "version":"1","default_locale":"en"
                }),
            ),
        ] {
            let value = if field == "name" {
                name.as_str()
            } else {
                short_name
            };
            let messages = serde_json::to_vec(&json!({"identity":{"message":value}})).unwrap();
            let fixture =
                make_fixture(manifest, &[("_locales/en/messages.json", &messages)], false);
            let admitted =
                admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
            assert_eq!(
                resolve_extension_default_locale(&admitted, Some(&messages)),
                Err(ExtensionDefaultLocaleResolutionError::InvalidResolvedField(
                    field
                ))
            );
        }
    }

    for identity in [
        "Élodie\u{200c}",
        "密码\u{200d}",
        "१२३\u{fe0f}",
        "A😀!\u{200d}",
    ] {
        let literal = make_fixture(
            json!({
                "manifest_version":3,"name":identity,"short_name":identity,"version":"1"
            }),
            &[],
            false,
        );
        let admitted =
            admit_extension_manifest(literal.binding(), &literal.manifest, &policy).unwrap();
        let resolved = resolve_extension_default_locale(&admitted, None).unwrap();
        assert_eq!(resolved.name().as_str(), identity);
        assert_eq!(resolved.short_name().unwrap().as_str(), identity);

        let messages = serde_json::to_vec(&json!({
            "name":{"message":identity},"short":{"message":identity}
        }))
        .unwrap();
        let localized = make_fixture(
            json!({
                "manifest_version":3,"name":"__MSG_name__","short_name":"__MSG_short__",
                "version":"1","default_locale":"en"
            }),
            &[("_locales/en/messages.json", &messages)],
            false,
        );
        let admitted =
            admit_extension_manifest(localized.binding(), &localized.manifest, &policy).unwrap();
        let resolved = resolve_extension_default_locale(&admitted, Some(&messages)).unwrap();
        assert_eq!(resolved.name().as_str(), identity);
        assert_eq!(resolved.short_name().unwrap().as_str(), identity);
    }

    let non_identity_metadata = make_fixture(
        json!({
            "manifest_version":3,"name":"Valid","version":"1",
            "description":"😀!?","version_name":"...","author":"\u{200c}\u{200d}",
            "action":{"default_title":"😀!?"}
        }),
        &[],
        false,
    );
    let admitted = admit_extension_manifest(
        non_identity_metadata.binding(),
        &non_identity_metadata.manifest,
        &policy,
    )
    .unwrap();
    let resolved = resolve_extension_default_locale(&admitted, None).unwrap();
    assert_eq!(resolved.description().unwrap().as_str(), "😀!?");
    assert_eq!(resolved.version_name().unwrap().as_str(), "...");
    assert_eq!(resolved.author().unwrap().as_str(), "\u{200c}\u{200d}");
    assert_eq!(resolved.action_title().unwrap().as_str(), "😀!?");
}

#[test]
fn platform_variant_format_controls_are_unsafe_even_beside_visible_identity() {
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    let unsafe_controls = ['\u{fff9}', '\u{fffa}', '\u{fffb}']
        .into_iter()
        .chain((0x13430..=0x13455).map(|scalar| char::from_u32(scalar).unwrap()))
        .collect::<Vec<_>>();
    assert!(unsafe_controls
        .iter()
        .copied()
        .all(super::metadata::is_unsafe_display_character));

    for control in ['\u{fff9}', '\u{13430}'] {
        let mixed = format!("PayPal{control}");
        for manifest in [
            json!({"manifest_version":3,"name":mixed,"version":"1"}),
            json!({
                "manifest_version":3,"name":"PayPal","short_name":mixed,"version":"1"
            }),
            json!({
                "manifest_version":3,"name":"PayPal","description":mixed,"version":"1"
            }),
            json!({
                "manifest_version":3,"name":"PayPal","version_name":mixed,"version":"1"
            }),
            json!({
                "manifest_version":3,"name":"PayPal","author":mixed,"version":"1"
            }),
            json!({
                "manifest_version":3,"name":"PayPal","version":"1",
                "action":{"default_title":mixed}
            }),
        ] {
            let fixture = make_fixture(manifest, &[], false);
            assert!(matches!(
                admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy),
                Err(ExtensionManifestAdmissionError::InvalidField(_))
            ));
        }

        let messages = serde_json::to_vec(&json!({"target":{"message":mixed}})).unwrap();
        for (field, manifest) in [
            (
                "name",
                json!({
                    "manifest_version":3,"name":"__MSG_target__","version":"1",
                    "default_locale":"en"
                }),
            ),
            (
                "short_name",
                json!({
                    "manifest_version":3,"name":"PayPal","short_name":"__MSG_target__",
                    "version":"1","default_locale":"en"
                }),
            ),
            (
                "description",
                json!({
                    "manifest_version":3,"name":"PayPal","description":"__MSG_target__",
                    "version":"1","default_locale":"en"
                }),
            ),
            (
                "action.default_title",
                json!({
                    "manifest_version":3,"name":"PayPal","version":"1",
                    "default_locale":"en","action":{"default_title":"__MSG_target__"}
                }),
            ),
        ] {
            let fixture =
                make_fixture(manifest, &[("_locales/en/messages.json", &messages)], false);
            let admitted =
                admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
            assert_eq!(
                resolve_extension_default_locale(&admitted, Some(&messages)),
                Err(ExtensionDefaultLocaleResolutionError::InvalidResolvedField(
                    field
                ))
            );
        }
    }
}

#[test]
fn malformed_and_predefined_manifest_localization_tokens_are_rejected_at_admission() {
    let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
    for name in [
        "prefix __MSG_name__",
        "__MSG_bad-key__",
        "__MSG_@@ui_locale__",
        "__MSG___",
        "__MSG_name__suffix",
        "__MSG_name___",
        "__MSG_name____",
        "__MSG_first____MSG_second__",
        "__MSG_first____MSG_second____",
    ] {
        let fixture = make_fixture(
            json!({"manifest_version":3,"name":name,"version":"1","default_locale":"en"}),
            &[("_locales/en/messages.json", b"{}")],
            false,
        );
        assert!(matches!(
            admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy),
            Err(ExtensionManifestAdmissionError::InvalidField(_))
        ));
    }

    for manifest in [
        json!({
            "manifest_version":3,"name":"X","version":"1","author":"__MSG_author__"
        }),
        json!({
            "manifest_version":3,"name":"X","version":"1",
            "version_name":"prefix __MSG_version__"
        }),
    ] {
        let fixture = make_fixture(manifest, &[], false);
        assert!(matches!(
            admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy),
            Err(ExtensionManifestAdmissionError::InvalidField(_))
        ));
    }

    let boundary_key = "k".repeat(crate::MAX_EXTENSION_LOCALE_MESSAGE_KEY_BYTES);
    let boundary_token = format!("__MSG_{boundary_key}__");
    let boundary_messages =
        serde_json::to_vec(&json!({boundary_key.clone():{"message":"X"}})).unwrap();
    let fixture = make_fixture(
        json!({
            "manifest_version":3,"name":boundary_token,"version":"1","default_locale":"en"
        }),
        &[("_locales/en/messages.json", &boundary_messages)],
        false,
    );
    let admitted = admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
    assert_eq!(
        resolve_extension_default_locale(&admitted, Some(&boundary_messages))
            .unwrap()
            .name()
            .as_str(),
        "X"
    );

    let oversized_key = "k".repeat(crate::MAX_EXTENSION_LOCALE_MESSAGE_KEY_BYTES + 1);
    let oversized_token = format!("__MSG_{oversized_key}__");
    let fixture = make_fixture(
        json!({
            "manifest_version":3,"name":oversized_token,"version":"1","default_locale":"en"
        }),
        &[("_locales/en/messages.json", b"{}")],
        false,
    );
    assert!(matches!(
        admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy),
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
        prop_assert_eq!(first.metadata().name().literal_text(), Some(name.as_str()));
        prop_assert_eq!(first.metadata().version(), version.as_str());
        prop_assert_eq!(first.admission_digest(), second.admission_digest());
        prop_assert!(first.retained_bytes() >= first.metadata().retained_bytes());
    }

    #[test]
    fn default_locale_resolution_is_case_insensitive_and_deterministic(
        key in "[A-Za-z][A-Za-z0-9]{0,31}",
        value in "[A-Za-z0-9][A-Za-z0-9 ._-]{0,59}",
    ) {
        let manifest_key = format!("__MSG_{}__", key.to_ascii_uppercase());
        let messages = serde_json::to_vec(&json!({key.clone(): {"message": value.clone()}})).unwrap();
        let fixture = make_fixture(
            json!({
                "manifest_version":3,"name":manifest_key,"version":"1","default_locale":"en"
            }),
            &[("_locales/en/messages.json", &messages)],
            false,
        );
        let policy = CompletePolicy::new(ExtensionCompatibilityLevel::Unsupported);
        let admitted = admit_extension_manifest(fixture.binding(), &fixture.manifest, &policy).unwrap();
        let first = resolve_extension_default_locale(&admitted, Some(&messages)).unwrap();
        let second = resolve_extension_default_locale(&admitted, Some(&messages)).unwrap();
        prop_assert_eq!(first.name().as_str(), value.as_str());
        prop_assert_eq!(first.digest(), second.digest());
        prop_assert_eq!(first.retained_bytes(), second.retained_bytes());
    }
}
