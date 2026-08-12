//! Offline admission for the exact Bitwarden Core source reviewed by Zephium.
//!
//! This boundary deliberately performs no acquisition and emits no adapted
//! artifact. Release automation must present an already checked-out, clean
//! repository at the exact reviewed commit and tag. The per-file digests bind
//! the compatibility preimages which later adaptation is allowed to change.

use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;
use sha2::{Digest, Sha256};
use zephium_extension_package::{
    parse_bounded_json, BoundedJsonLimits, CanonicalExtensionTreeIndex, MAX_EXTENSION_TREE_BYTES,
    MAX_EXTENSION_TREE_ENTRIES, MAX_EXTENSION_TREE_FILE_BYTES,
};

const PINNED_COMMIT: &str = "adf0337e4a0f788b895933792fc04fa162669eff";
const PINNED_TAG: &str = "browser-v2026.7.0";
const MAX_REVIEWED_SOURCE_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_GIT_OUTPUT_BYTES: u64 = 16 * 1024;
const EXPECTED_BUILD_FILE_COUNT: usize = 187;
const EXPECTED_FINAL_FILE_COUNT: usize = 169;
const EXPECTED_INSTRUMENTED_FILE_COUNT: usize = EXPECTED_FINAL_FILE_COUNT + 4;
const PROBE_ARTIFACT_EXTENSION_DIRECTORY: &str = "extension";
const PROBE_ARTIFACT_INDEX: &str = "extension-tree.json";
const PROBE_ARTIFACT_METADATA: &str = "ZEPHIUM-PROBE-ARTIFACT.json";
const PROBE_BACKGROUND_WRAPPER: &str = "zephium-probe-background.js";
const PROBE_PAGE_DIAGNOSTICS: &str = "zephium-probe-page-diagnostics.js";
const PROBE_PAGE_CANARY: &str = "zephium-probe-canary.html";
const PROBE_PAGE_CANARY_SCRIPT: &str = "zephium-probe-canary.js";
const SOURCE_POPUP_ENTRYPOINT: &str = "popup/index.html";
const PROBE_BACKGROUND_WRAPPER_SOURCE: &str = r#"(() => {
  'use strict';
  const prefix = 'ZEPHIUM_BACKGROUND_DIAGNOSTIC:';
  const render = value => {
    try {
      if (value instanceof Error) return `${value.name}: ${value.message}\n${value.stack || ''}`;
      if (typeof value === 'string') return value;
      return JSON.stringify(value);
    } catch (_) {
      try { return String(value); } catch (_) { return '<unprintable>'; }
    }
  };
  const publish = (kind, values) => {
    const message = `${prefix}${kind}: ${values.map(render).join(' | ')}`.slice(0, 2048);
    try { chrome.action.setTitle({ title: message }); } catch (_) {}
  };
  const originalError = console.error.bind(console);
  console.error = (...values) => {
    publish('console.error', values);
    originalError(...values);
  };
  addEventListener('error', event => publish('error', [event.message, event.error]));
  addEventListener('unhandledrejection', event => publish('unhandledrejection', [event.reason]));
  try {
    importScripts('background.js');
  } catch (error) {
    publish('importScripts', [error]);
  }
})();
"#;
const PROBE_PAGE_DIAGNOSTICS_SOURCE: &str = r#"(() => {
  'use strict';
  const prefix = 'ZEPHIUM_POPUP_DIAGNOSTIC:';
  const render = value => {
    try {
      if (value instanceof Error) return `${value.name}: ${value.message}\n${value.stack || ''}`;
      if (typeof value === 'string') return value;
      return JSON.stringify(value);
    } catch (_) {
      try { return String(value); } catch (_) { return '<unprintable>'; }
    }
  };
  const publish = (kind, values) => {
    document.title = `${prefix}${kind}: ${values.map(render).join(' | ')}`.slice(0, 2048);
  };
  const scriptStates = [];
  let wasmState = 'idle';
  const publishState = () => {
    document.title = `ZEPHIUM_POPUP_STATE:${JSON.stringify({
      ready: document.readyState,
      loading: !!document.querySelector('#loading'),
      rootChildren: document.querySelector('app-root')?.childElementCount ?? -1,
      chrome: typeof chrome,
      runtime: typeof chrome === 'object' ? typeof chrome.runtime : 'absent',
      initStage: globalThis.__zephiumPopupInitStage ?? 'not-started',
      scripts: scriptStates.map(({ src, loaded }) => ({ src, loaded })),
      wasm: wasmState,
    })}`.slice(0, 2048);
  };
  const trackScript = script => {
    const src = script.getAttribute('src');
    if (!src || scriptStates.some(state => state.script === script)) return;
    const state = { script, src, loaded: false };
    scriptStates.push(state);
    state.script.addEventListener('load', () => { state.loaded = true; publishState(); }, { once: true });
    state.script.addEventListener('error', () => publish('script-error', [state.src]), { once: true });
  };
  for (const script of document.scripts) trackScript(script);
  new MutationObserver(records => {
    for (const record of records) for (const node of record.addedNodes) {
      if (node instanceof HTMLScriptElement) trackScript(node);
      if (node instanceof Element) for (const script of node.querySelectorAll('script')) trackScript(script);
    }
  }).observe(document.documentElement, { childList: true, subtree: true });
  if (typeof WebAssembly?.instantiateStreaming === 'function') {
    const instantiateStreaming = WebAssembly.instantiateStreaming.bind(WebAssembly);
    WebAssembly.instantiateStreaming = (...args) => {
      wasmState = 'instantiate-streaming';
      publishState();
      return instantiateStreaming(...args).then(
        value => { wasmState = 'ready'; publishState(); return value; },
        error => { wasmState = 'streaming-rejected'; publishState(); throw error; },
      );
    };
  }
  const originalError = console.error.bind(console);
  console.error = (...values) => {
    publish('console.error', values);
    originalError(...values);
  };
  addEventListener('error', event => publish('error', [event.message, event.error]));
  addEventListener('unhandledrejection', event => publish('unhandledrejection', [event.reason]));
  const readyTitle = 'zephium-bitwarden-core-popup-ready';
  const observeReadiness = () => {
    const root = document.querySelector('app-root');
    const update = () => {
      if (!document.querySelector('#loading') && root?.childElementCount) {
        document.title = readyTitle;
        return true;
      }
      return false;
    };
    if (!update() && root) new MutationObserver(update).observe(root, { childList: true, subtree: true });
  };
  observeReadiness();
  document.addEventListener('readystatechange', publishState);
  addEventListener('DOMContentLoaded', publishState, { once: true });
  addEventListener('load', publishState, { once: true });
  publishState();
  setTimeout(() => {
    if (document.title !== readyTitle && !document.title.startsWith(prefix)) publishState();
  }, 5000);
})();
"#;
const PROBE_PAGE_CANARY_SOURCE: &str =
    "<!doctype html><meta charset=\"utf-8\"><title>zephium-canary-pending</title><script src=\"zephium-probe-canary.js\"></script>";
const PROBE_PAGE_CANARY_SCRIPT_SOURCE: &str =
    "document.title='zephium-bitwarden-extension-page-ready';";
const EXPECTED_SOURCE_MAPS: &[&str] = &[
    "719.background.js.map",
    "assets/635.js.map",
    "background.js.map",
    "offscreen-document/offscreen-document.js.map",
    "popup/main.css.map",
    "popup/main.js.map",
    "popup/polyfills.js.map",
    "popup/vendor-angular.js.map",
    "popup/vendor.js.map",
];
const UNSAFE_INLINE_MENU_FILES: &[&str] = &[
    "overlay/menu-button.css",
    "overlay/menu-button.html",
    "overlay/menu-button.js",
    "overlay/menu-list.css",
    "overlay/menu-list.html",
    "overlay/menu-list.js",
    "overlay/menu.css",
    "overlay/menu.html",
    "overlay/menu.js",
];
const REQUIRED_PROBE_ARTIFACT_FILES: &[&str] = &[
    "background.js",
    "background.js.LICENSE.txt",
    "content/bootstrap-autofill-overlay-menu.js",
    "content/content-message-handler.js",
    "content/trigger-autofill-script-injection.js",
    "manifest.json",
    "offscreen-document/offscreen-document.js.LICENSE.txt",
    "popup/index.html",
    "popup/main.js",
    "popup/polyfills.js.LICENSE.txt",
    "popup/vendor-angular.js.LICENSE.txt",
    "popup/vendor.js.LICENSE.txt",
];
const PROBE_OVERLAY_FILES: &[&str] = &[
    "apps/browser/src/background/main.background.ts",
    "apps/browser/src/autofill/fido2/background/fido2.background.ts",
    "apps/browser/src/autofill/overlay/inline-menu/iframe-content/autofill-inline-menu-iframe.service.ts",
    "apps/browser/src/manifest.v3.json",
    "apps/browser/src/popup/services/init.service.ts",
    "apps/browser/src/platform/services/platform-utils/browser-platform-utils.service.ts",
    "apps/browser/src/platform/services/sdk/browser-sdk-load.service.ts",
];

#[derive(Clone, Copy)]
struct ReviewedSourceFile {
    path: &'static str,
    sha256: &'static str,
    markers: &'static [SourceMarker],
}

#[derive(Clone, Copy)]
struct SourceMarker {
    text: &'static str,
    occurrences: usize,
}

const SOURCE_FILES: &[ReviewedSourceFile] = &[
    ReviewedSourceFile {
        path: "apps/browser/src/background/main.background.ts",
        sha256: "a144b3007cd85258778aec702194c7e3ba20ec0799aececd1538ba8f7f1f373f",
        markers: &[
            SourceMarker {
                text: "const localStorageStorageService = BrowserApi.isManifestVersion(3)",
                occurrences: 1,
            },
            SourceMarker {
                text: "new OffscreenStorageService(this.offscreenDocumentService)",
                occurrences: 1,
            },
            SourceMarker {
                text: "new PrimarySecondaryStorageService(this.storageService, localStorageStorageService)",
                occurrences: 1,
            },
            SourceMarker {
                text: "  initNotificationSubscriptions() {",
                occurrences: 1,
            },
        ],
    },
    ReviewedSourceFile {
        path: "apps/browser/src/autofill/fido2/background/fido2.background.ts",
        sha256: "781599c51ca9b24537c82da7b0eeb82c936fe52ae06bc5d387eb448b66169231",
        markers: &[
            SourceMarker {
                text: "world: \"MAIN\"",
                occurrences: 1,
            },
            SourceMarker {
                text: "world: chrome.scripting.ExecutionWorld.MAIN",
                occurrences: 1,
            },
        ],
    },
    ReviewedSourceFile {
        path: "apps/browser/src/manifest.v3.json",
        sha256: "8f97a04018ffccc5bd1ff579662f4086b97dd545036f5d5775398a24f264643f",
        markers: &[
            SourceMarker {
                text: "\"offscreen\"",
                occurrences: 2,
            },
            SourceMarker {
                text: "\"sandbox\"",
                occurrences: 1,
            },
            SourceMarker {
                text: "\"overlay/menu-button.html\"",
                occurrences: 2,
            },
            SourceMarker {
                text: "\"overlay/menu-list.html\"",
                occurrences: 2,
            },
            SourceMarker {
                text: "\"overlay/menu.html\"",
                occurrences: 1,
            },
        ],
    },
    ReviewedSourceFile {
        path: "apps/browser/src/platform/offscreen-document/offscreen-document.service.ts",
        sha256: "2afe5a423a5dec6d1ef92cc35a97f171c4342f540d88fbb2ae077d4252043036",
        markers: &[SourceMarker {
            text: "return typeof chrome.offscreen !== \"undefined\";",
            occurrences: 1,
        }],
    },
    ReviewedSourceFile {
        path: "apps/browser/src/popup/services/init.service.ts",
        sha256: "cac0632fdf525443e5082fa6383fabb618bf6cd535561b4828152e8b52ada9d5",
        markers: &[
            SourceMarker {
                text: "      await this.sdkLoadService.loadAndInit();",
                occurrences: 1,
            },
            SourceMarker {
                text: "      await this.migrationRunner.waitForCompletion(); // Browser background is responsible for migrations",
                occurrences: 1,
            },
            SourceMarker {
                text: "      await this.i18nService.init();",
                occurrences: 1,
            },
            SourceMarker {
                text: "      await this.viewCacheService.init();",
                occurrences: 1,
            },
            SourceMarker {
                text: "      await this.sizeService.init();",
                occurrences: 1,
            },
        ],
    },
    ReviewedSourceFile {
        path: "apps/browser/src/platform/services/platform-utils/browser-platform-utils.service.ts",
        sha256: "335303846f567e751bb6bcc82006fb974d4615b0c4f606ed4728dda5180f38ca",
        markers: &[
            SourceMarker {
                text: "} else if (BrowserPlatformUtilsService.isSafari(globalContext)) {",
                occurrences: 1,
            },
            SourceMarker {
                text: "this.deviceCache = DeviceType.SafariExtension;",
                occurrences: 1,
            },
        ],
    },
    ReviewedSourceFile {
        path: "apps/browser/src/platform/services/sdk/browser-sdk-load.service.ts",
        sha256: "2de5f76834123da44e0cab4329c280b1570d47b7f93a763e1d75220790d333c1",
        markers: &[
            SourceMarker {
                text: "const supported = (() => {",
                occurrences: 1,
            },
            SourceMarker {
                text: "WebAssembly.instantiateStreaming",
                occurrences: 0,
            },
            SourceMarker {
                text: "loadingPromise = import(\"./wasm\");",
                occurrences: 1,
            },
        ],
    },
    ReviewedSourceFile {
        path: "apps/browser/src/autofill/overlay/inline-menu/iframe-content/autofill-inline-menu-iframe.service.ts",
        sha256: "04761700c5b876d122267a2903f50a13eea973b41c1228a7c286d98aaec3890a",
        markers: &[
            SourceMarker {
                text: "BrowserApi.getRuntimeURL(\"overlay/menu.html\")",
                occurrences: 1,
            },
            SourceMarker {
                text: "this.iframe.contentWindow?.postMessage(\n      { portKey: this.portKey, ...message },\n      this.extensionOrigin,\n    );",
                occurrences: 1,
            },
        ],
    },
    ReviewedSourceFile {
        path: "apps/browser/src/autofill/overlay/inline-menu/iframe-content/autofill-inline-menu-iframe-element.ts",
        sha256: "7bc325afdad14744b29e68ecb363c859f79231ae240697303a2591cd012d984b",
        markers: &[SourceMarker {
            text: "this.autofillInlineMenuIframeService.initMenuIframe();",
            occurrences: 1,
        }],
    },
    ReviewedSourceFile {
        path: "apps/browser/src/autofill/overlay/inline-menu/pages/button/button.html",
        sha256: "b1b0a514eb26df079f056e2a0c7c21662b37cd4f24ddfc9fc1476c80760c1c50",
        markers: &[SourceMarker {
            text: "<autofill-inline-menu-button></autofill-inline-menu-button>",
            occurrences: 1,
        }],
    },
    ReviewedSourceFile {
        path: "apps/browser/src/autofill/overlay/inline-menu/pages/list/list.html",
        sha256: "181ca75ab2e5bb01caf3b49caf6a6233f3821a4b8a2f121b3a8a84b6765837a0",
        markers: &[SourceMarker {
            text: "<autofill-inline-menu-list></autofill-inline-menu-list>",
            occurrences: 1,
        }],
    },
    ReviewedSourceFile {
        path: "apps/browser/webpack.base.js",
        sha256: "ae27bd62f8e4d34ae5e6fb94e063cada777de2138e0ffd3f3403dbeae8fda625",
        markers: &[
            SourceMarker {
                text: "filename: \"overlay/menu-button.html\"",
                occurrences: 1,
            },
            SourceMarker {
                text: "filename: \"overlay/menu-list.html\"",
                occurrences: 1,
            },
            SourceMarker {
                text: "filename: \"overlay/menu.html\"",
                occurrences: 1,
            },
        ],
    },
];

pub(crate) fn check_source(root: &Path) -> Result<(), String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize source root: {error}"))?;
    if !root.is_dir() {
        return Err("source root is not a directory".into());
    }
    if read_git_boolean(&root, "core.sparseCheckout")? {
        return Err("sparse source checkouts are not release-admissible".into());
    }
    let sparse_definition = run_git(
        &root,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "info/sparse-checkout",
        ],
    )?;
    if Path::new(&sparse_definition).exists() {
        return Err("a sparse-checkout definition is not release-admissible".into());
    }

    let commit = run_git(&root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    require_exact_line("commit", &commit, PINNED_COMMIT)?;
    let tags = run_git(&root, &["tag", "--points-at", "HEAD"])?;
    if !tags.lines().any(|tag| tag == PINNED_TAG) {
        return Err(format!(
            "HEAD is not labelled with the exact reviewed tag {PINNED_TAG}"
        ));
    }
    let status = run_git(
        &root,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    if !status.is_empty() {
        return Err("source checkout is not clean".into());
    }

    for reviewed in SOURCE_FILES {
        validate_relative_path(reviewed.path)?;
        let path = root.join(reviewed.path);
        let canonical = path
            .canonicalize()
            .map_err(|error| format!("cannot canonicalize {}: {error}", reviewed.path))?;
        if !canonical.starts_with(&root) {
            return Err(format!("{} escapes the source root", reviewed.path));
        }
        let metadata = fs::metadata(&canonical)
            .map_err(|error| format!("cannot inspect {}: {error}", reviewed.path))?;
        if !metadata.is_file() || metadata.len() > MAX_REVIEWED_SOURCE_FILE_BYTES {
            return Err(format!(
                "{} is not a bounded regular source file",
                reviewed.path
            ));
        }
        let bytes = fs::read(&canonical)
            .map_err(|error| format!("cannot read {}: {error}", reviewed.path))?;
        let digest = sha256_hex(&bytes);
        if digest != reviewed.sha256 {
            return Err(format!(
                "{} digest does not match the reviewed source",
                reviewed.path
            ));
        }
        let source =
            std::str::from_utf8(&bytes).map_err(|_| format!("{} is not UTF-8", reviewed.path))?;
        for marker in reviewed.markers {
            require_occurrences(reviewed.path, source, *marker)?;
        }
    }

    println!(
        "Bitwarden Core source admission passed: commit={PINNED_COMMIT}; tag={PINNED_TAG}; reviewed_files={}",
        SOURCE_FILES.len()
    );
    Ok(())
}

/// Materializes an exact, non-product source overlay for the first native
/// Bitwarden vertical probe. The overlay deliberately disables the inline menu
/// instead of allowing WebKit's ineffective extension-page sandbox. It neither
/// builds nor authenticates an extension package.
pub(crate) fn materialize_macos_probe_overlay(root: &Path, output: &Path) -> Result<(), String> {
    check_source(root)?;
    ensure_absent_output(output)?;
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize overlay parent: {error}"))?;
    let output_name = output
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "overlay output has no final component".to_owned())?;
    let final_output = parent.join(output_name);
    ensure_absent_output(&final_output)?;

    let staging = tempfile::Builder::new()
        .prefix(".zephium-bitwarden-overlay-")
        .tempdir_in(&parent)
        .map_err(|error| format!("cannot create overlay stage: {error}"))?;
    for relative in PROBE_OVERLAY_FILES {
        let source = fs::read_to_string(root.join(relative))
            .map_err(|error| format!("cannot read overlay source {relative}: {error}"))?;
        let adapted = adapt_macos_probe_file(relative, &source)?;
        write_overlay_file(staging.path(), relative, adapted.as_bytes())?;
    }
    let metadata = serde_json::to_vec_pretty(&serde_json::json!({
        "schema": 1,
        "kind": "zephium-bitwarden-core-macos-probe-overlay",
        "product_authority": false,
        "source_commit": PINNED_COMMIT,
        "source_tag": PINNED_TAG,
        "build_target": {
            "browser": "chrome",
            "manifest_version": 3,
            "node_env": "production",
        },
        "compatibility_adaptations": [
            "webkit-extension-device-classification",
            "unsupported-notification-subscription-guard",
            "unsupported-offscreen-storage-fallback",
            "typed-main-world-enum"
        ],
        "probe_diagnostics": ["popup-init-stage", "popup-wasm-state"],
        "limitations": ["inline-menu-disabled", "not-a-product-package"],
        "files": PROBE_OVERLAY_FILES,
    }))
    .map_err(|error| format!("cannot serialize overlay metadata: {error}"))?;
    write_overlay_file(staging.path(), "ZEPHIUM-OVERLAY.json", &metadata)?;

    let staged_path = staging.keep();
    fs::rename(&staged_path, &final_output).map_err(|error| {
        format!(
            "cannot atomically publish overlay (stage retained at {}): {error}",
            staged_path.display()
        )
    })?;
    println!(
        "Bitwarden Core macOS probe overlay materialized: files={}; product_authority=false",
        PROBE_OVERLAY_FILES.len()
    );
    Ok(())
}

#[derive(Clone)]
struct ProbeBuildFile {
    relative: String,
    source: PathBuf,
    length: u64,
    sha256: String,
}

#[derive(Default)]
struct ProbeBuildInventory {
    files: Vec<ProbeBuildFile>,
    source_maps: BTreeSet<String>,
    unsafe_inline_menu_files: BTreeSet<String>,
    entry_count: usize,
    total_bytes: u64,
}

#[derive(Serialize)]
struct ProbeTreeIndex<'a> {
    schema_version: u32,
    files: &'a [ProbeTreeFile],
}

#[derive(Serialize)]
struct ProbeTreeFile {
    path: String,
    length: u64,
    sha256: String,
}

/// Produces the closed, non-product extension tree consumed by the live native
/// Bitwarden probe. This step intentionally does not authenticate a release:
/// it strips debug maps and the fail-closed privileged inline-menu pages,
/// validates the exact reviewed manifest/adaptation invariants, and emits a
/// canonical resource index beside (not inside) the extension root.
pub(crate) fn finalize_macos_probe_artifact(build: &Path, output: &Path) -> Result<(), String> {
    let build = build
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize probe build root: {error}"))?;
    if !build.is_dir() {
        return Err("probe build root is not a directory".into());
    }
    ensure_absent_output(output)?;
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize artifact parent: {error}"))?;
    let output_name = output
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "artifact output has no final component".to_owned())?;
    let final_output = parent.join(output_name);
    ensure_absent_output(&final_output)?;
    if final_output.starts_with(&build) {
        return Err("probe artifact output may not be nested inside its build input".into());
    }

    let mut inventory = ProbeBuildInventory::default();
    collect_probe_build_tree(&build, Path::new(""), 0, &mut inventory)?;
    inventory
        .files
        .sort_unstable_by(|left, right| left.relative.cmp(&right.relative));
    validate_probe_build_inventory(&inventory)?;
    validate_probe_build_semantics(&inventory)?;

    let staging = tempfile::Builder::new()
        .prefix(".zephium-bitwarden-artifact-")
        .tempdir_in(&parent)
        .map_err(|error| format!("cannot create artifact stage: {error}"))?;
    let extension_root = staging.path().join(PROBE_ARTIFACT_EXTENSION_DIRECTORY);
    fs::create_dir(&extension_root)
        .map_err(|error| format!("cannot create staged extension root: {error}"))?;

    let mut index_files = Vec::with_capacity(EXPECTED_INSTRUMENTED_FILE_COUNT);
    for file in &inventory.files {
        let mut bytes = read_bounded_regular_file(&file.source, file.length, &file.relative)?;
        if sha256_hex(&bytes) != file.sha256 {
            return Err(format!(
                "probe build file changed during finalization: {}",
                file.relative
            ));
        }
        if file.relative == "manifest.json" {
            bytes = instrument_probe_manifest(&bytes)?;
        } else if file.relative == "popup/index.html" {
            bytes = instrument_probe_popup(&bytes)?;
        }
        let output_relative = &file.relative;
        write_overlay_file(&extension_root, output_relative, &bytes)?;
        index_files.push(ProbeTreeFile {
            path: output_relative.to_owned(),
            length: bytes.len() as u64,
            sha256: sha256_hex(&bytes),
        });
    }
    write_overlay_file(
        &extension_root,
        PROBE_BACKGROUND_WRAPPER,
        PROBE_BACKGROUND_WRAPPER_SOURCE.as_bytes(),
    )?;
    index_files.push(ProbeTreeFile {
        path: PROBE_BACKGROUND_WRAPPER.to_owned(),
        length: PROBE_BACKGROUND_WRAPPER_SOURCE.len() as u64,
        sha256: sha256_hex(PROBE_BACKGROUND_WRAPPER_SOURCE.as_bytes()),
    });
    write_overlay_file(
        &extension_root,
        PROBE_PAGE_DIAGNOSTICS,
        PROBE_PAGE_DIAGNOSTICS_SOURCE.as_bytes(),
    )?;
    index_files.push(ProbeTreeFile {
        path: PROBE_PAGE_DIAGNOSTICS.to_owned(),
        length: PROBE_PAGE_DIAGNOSTICS_SOURCE.len() as u64,
        sha256: sha256_hex(PROBE_PAGE_DIAGNOSTICS_SOURCE.as_bytes()),
    });
    for (path, bytes) in [
        (PROBE_PAGE_CANARY, PROBE_PAGE_CANARY_SOURCE.as_bytes()),
        (
            PROBE_PAGE_CANARY_SCRIPT,
            PROBE_PAGE_CANARY_SCRIPT_SOURCE.as_bytes(),
        ),
    ] {
        write_overlay_file(&extension_root, path, bytes)?;
        index_files.push(ProbeTreeFile {
            path: path.to_owned(),
            length: bytes.len() as u64,
            sha256: sha256_hex(bytes),
        });
    }
    index_files.sort_unstable_by(|left, right| left.path.cmp(&right.path));

    let index_bytes = serde_json::to_vec(&ProbeTreeIndex {
        schema_version: 1,
        files: &index_files,
    })
    .map_err(|error| format!("cannot serialize probe tree index: {error}"))?;
    let parsed_index = CanonicalExtensionTreeIndex::parse_canonical(&index_bytes)
        .map_err(|error| format!("finalized probe tree index is invalid: {error}"))?;
    write_overlay_file(staging.path(), PROBE_ARTIFACT_INDEX, &index_bytes)?;

    let metadata = serde_json::to_vec_pretty(&serde_json::json!({
        "schema": 1,
        "kind": "zephium-bitwarden-core-macos-probe-artifact",
        "product_authority": false,
        "source_commit": PINNED_COMMIT,
        "source_tag": PINNED_TAG,
        "build_target": {
            "browser": "chrome",
            "manifest_version": 3,
            "node_env": "production",
        },
        "build_toolchain_attested": false,
        "background_diagnostics": true,
        "popup_diagnostics": true,
        "probe_diagnostics": ["popup-init-stage", "popup-wasm-state"],
        "extension_page_canary": true,
        "compatibility_adaptations": [
            "webkit-extension-device-classification",
            "unsupported-notification-subscription-guard",
            "unsupported-offscreen-storage-fallback",
            "typed-main-world-enum"
        ],
        "extension_root": PROBE_ARTIFACT_EXTENSION_DIRECTORY,
        "tree_index": PROBE_ARTIFACT_INDEX,
        "file_count": parsed_index.files().len(),
        "total_bytes": parsed_index.total_bytes(),
        "manifest_sha256": lower_hex(parsed_index.manifest_sha256().as_bytes()),
        "tree_sha256": lower_hex(parsed_index.tree_sha256().as_bytes()),
        "tree_index_sha256": lower_hex(parsed_index.index_sha256().as_bytes()),
        "stripped": {
            "source_maps": EXPECTED_SOURCE_MAPS.len(),
            "unsafe_inline_menu_files": UNSAFE_INLINE_MENU_FILES.len(),
        },
        "limitations": [
            "inline-menu-disabled",
            "build-toolchain-unattested",
            "probe-background-instrumented",
            "probe-popup-instrumented",
            "not-a-product-package"
        ],
    }))
    .map_err(|error| format!("cannot serialize probe artifact metadata: {error}"))?;
    write_overlay_file(staging.path(), PROBE_ARTIFACT_METADATA, &metadata)?;

    let staged_path = staging.keep();
    fs::rename(&staged_path, &final_output).map_err(|error| {
        format!(
            "cannot atomically publish probe artifact (stage retained at {}): {error}",
            staged_path.display()
        )
    })?;
    println!(
        "Bitwarden Core macOS probe artifact finalized: files={}; bytes={}; product_authority=false",
        parsed_index.files().len(),
        parsed_index.total_bytes(),
    );
    Ok(())
}

fn instrument_probe_manifest(source: &[u8]) -> Result<Vec<u8>, String> {
    let mut manifest = parse_bounded_json(source, BoundedJsonLimits::extension_manifest())
        .map_err(|error| format!("cannot instrument invalid probe manifest: {error}"))?
        .into_value();
    let service_worker = manifest
        .pointer_mut("/background/service_worker")
        .ok_or_else(|| "probe manifest omitted its background worker".to_owned())?;
    if service_worker.as_str() != Some("background.js") {
        return Err("probe manifest background worker drifted before instrumentation".into());
    }
    *service_worker = serde_json::Value::String(PROBE_BACKGROUND_WRAPPER.to_owned());
    let popup = manifest
        .pointer_mut("/action/default_popup")
        .ok_or_else(|| "probe manifest omitted its action popup".to_owned())?;
    if popup.as_str() != Some(SOURCE_POPUP_ENTRYPOINT) {
        return Err("probe manifest action popup drifted before instrumentation".into());
    }
    serde_json::to_vec(&manifest)
        .map_err(|error| format!("cannot serialize instrumented probe manifest: {error}"))
}

fn instrument_probe_popup(source: &[u8]) -> Result<Vec<u8>, String> {
    let source = std::str::from_utf8(source)
        .map_err(|_| "probe popup entrypoint is not UTF-8".to_owned())?;
    let before = "</body>";
    let after = "<script src=\"../zephium-probe-page-diagnostics.js\"></script></body>";
    if !source.contains("../popup/") {
        return Err("probe popup entrypoint contains no generated parent-relative assets".into());
    }
    replace_exact(SOURCE_POPUP_ENTRYPOINT, source, before, after).map(String::into_bytes)
}

fn collect_probe_build_tree(
    root: &Path,
    relative_directory: &Path,
    depth: usize,
    inventory: &mut ProbeBuildInventory,
) -> Result<(), String> {
    if depth > 32 {
        return Err("probe build directory depth exceeds the extension ceiling".into());
    }
    let directory = root.join(relative_directory);
    let mut entries = fs::read_dir(&directory)
        .map_err(|error| format!("cannot enumerate probe build directory: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("cannot enumerate probe build entry: {error}"))?;
    entries.sort_unstable_by_key(|entry| entry.file_name());

    for entry in entries {
        inventory.entry_count = inventory
            .entry_count
            .checked_add(1)
            .ok_or_else(|| "probe build entry accounting overflowed".to_owned())?;
        if inventory.entry_count > MAX_EXTENSION_TREE_ENTRIES {
            return Err("probe build exceeds the extension entry ceiling".into());
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "probe build contains a non-UTF-8 path".to_owned())?;
        let relative = relative_directory.join(name);
        let relative_text = portable_relative_path(&relative)?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
            format!("cannot inspect probe build entry {relative_text}: {error}")
        })?;
        if metadata.is_dir() {
            collect_probe_build_tree(root, &relative, depth + 1, inventory)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(format!(
                "probe build entry is not a regular file or directory: {relative_text}"
            ));
        }
        if metadata.len() > MAX_EXTENSION_TREE_FILE_BYTES {
            return Err(format!(
                "probe build file exceeds the per-file extension ceiling: {relative_text}"
            ));
        }
        inventory.total_bytes = inventory
            .total_bytes
            .checked_add(metadata.len())
            .ok_or_else(|| "probe build byte accounting overflowed".to_owned())?;
        if inventory.total_bytes > MAX_EXTENSION_TREE_BYTES {
            return Err("probe build exceeds the extension tree byte ceiling".into());
        }

        if EXPECTED_SOURCE_MAPS.contains(&relative_text.as_str()) {
            inventory.source_maps.insert(relative_text);
            continue;
        }
        if relative_text.ends_with(".map") {
            return Err(format!(
                "probe build emitted an unreviewed source map: {relative_text}"
            ));
        }
        if UNSAFE_INLINE_MENU_FILES.contains(&relative_text.as_str()) {
            inventory.unsafe_inline_menu_files.insert(relative_text);
            continue;
        }
        if relative_text.starts_with("overlay/") {
            return Err(format!(
                "probe build emitted an unreviewed privileged overlay resource: {relative_text}"
            ));
        }
        let bytes = read_bounded_regular_file(&entry.path(), metadata.len(), &relative_text)?;
        inventory.files.push(ProbeBuildFile {
            relative: relative_text,
            source: entry.path(),
            length: metadata.len(),
            sha256: sha256_hex(&bytes),
        });
    }
    Ok(())
}

fn validate_probe_build_inventory(inventory: &ProbeBuildInventory) -> Result<(), String> {
    let expected_maps = EXPECTED_SOURCE_MAPS
        .iter()
        .map(|path| (*path).to_owned())
        .collect::<BTreeSet<_>>();
    let expected_menu = UNSAFE_INLINE_MENU_FILES
        .iter()
        .map(|path| (*path).to_owned())
        .collect::<BTreeSet<_>>();
    if inventory.source_maps != expected_maps {
        return Err("probe build source-map inventory drifted".into());
    }
    if inventory.unsafe_inline_menu_files != expected_menu {
        return Err("probe build unsafe inline-menu inventory drifted".into());
    }
    let input_files = inventory
        .files
        .len()
        .checked_add(inventory.source_maps.len())
        .and_then(|count| count.checked_add(inventory.unsafe_inline_menu_files.len()))
        .ok_or_else(|| "probe build file accounting overflowed".to_owned())?;
    if input_files != EXPECTED_BUILD_FILE_COUNT
        || inventory.files.len() != EXPECTED_FINAL_FILE_COUNT
    {
        return Err(format!(
            "probe build file inventory drifted: input={input_files}, finalized={}",
            inventory.files.len()
        ));
    }
    let retained = inventory
        .files
        .iter()
        .map(|file| file.relative.as_str())
        .collect::<BTreeSet<_>>();
    for required in REQUIRED_PROBE_ARTIFACT_FILES {
        if !retained.contains(required) {
            return Err(format!(
                "probe build omitted required artifact file {required}"
            ));
        }
    }
    Ok(())
}

fn validate_probe_build_semantics(inventory: &ProbeBuildInventory) -> Result<(), String> {
    let manifest = probe_build_file_bytes(inventory, "manifest.json")?;
    let manifest = parse_bounded_json(&manifest, BoundedJsonLimits::extension_manifest())
        .map_err(|error| format!("probe manifest failed bounded JSON admission: {error}"))?
        .into_value();
    let expected_permissions = serde_json::json!([
        "activeTab",
        "alarms",
        "clipboardRead",
        "clipboardWrite",
        "contextMenus",
        "idle",
        "offscreen",
        "scripting",
        "sidePanel",
        "storage",
        "tabs",
        "unlimitedStorage",
        "webNavigation",
        "webRequest",
        "webRequestAuthProvider",
        "notifications"
    ]);
    let expected_optional_permissions = serde_json::json!(["nativeMessaging", "privacy"]);
    let expected_hosts = serde_json::json!(["https://*/*", "http://*/*"]);
    let expected_content_scripts = serde_json::json!([
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
    ]);
    let expected_web_resources = serde_json::json!([{
        "resources": [
            "content/fido2-page-script.js", "notification/bar.html", "images/icon38.png",
            "images/icon38_locked.png", "popup/fonts/*"
        ],
        "matches": ["<all_urls>"],
        "use_dynamic_url": true
    }]);
    let required = [
        ("/manifest_version", serde_json::json!(3)),
        ("/version", serde_json::json!("2026.7.0")),
        ("/minimum_chrome_version", serde_json::json!("102.0")),
        ("/default_locale", serde_json::json!("en")),
        (
            "/background/service_worker",
            serde_json::json!("background.js"),
        ),
        ("/permissions", expected_permissions),
        ("/optional_permissions", expected_optional_permissions),
        ("/host_permissions", expected_hosts),
        ("/content_scripts", expected_content_scripts),
        ("/web_accessible_resources", expected_web_resources),
        (
            "/action/default_popup",
            serde_json::json!("popup/index.html"),
        ),
        ("/action/default_title", serde_json::json!("Bitwarden")),
        (
            "/storage/managed_schema",
            serde_json::json!("managed_schema.json"),
        ),
    ];
    for (pointer, expected) in required {
        if manifest.pointer(pointer) != Some(&expected) {
            return Err(format!("probe manifest field drifted: {pointer}"));
        }
    }
    let object = manifest
        .as_object()
        .ok_or_else(|| "probe manifest root is not an object".to_owned())?;
    for prohibited in ["sandbox", "key", "update_url"] {
        if object.contains_key(prohibited) {
            return Err(format!(
                "probe manifest contains prohibited field {prohibited}"
            ));
        }
    }

    let background = probe_build_file_bytes(inventory, "background.js")?;
    let background = std::str::from_utf8(&background)
        .map_err(|_| "probe background bundle is not UTF-8".to_owned())?;
    if background.contains("chrome.scripting.ExecutionWorld.MAIN")
        || !background.contains("\"MAIN\"")
        || !background.contains("offscreenApiSupported")
    {
        return Err(
            "probe background bundle does not contain the reviewed macOS adaptations".into(),
        );
    }
    let inline_menu =
        probe_build_file_bytes(inventory, "content/bootstrap-autofill-overlay-menu.js")?;
    let inline_menu = std::str::from_utf8(&inline_menu)
        .map_err(|_| "probe inline-menu bootstrap is not UTF-8".to_owned())?;
    if inline_menu.contains("overlay/menu.html") || !inline_menu.contains("forceCloseInlineMenu") {
        return Err("probe inline-menu bootstrap is not fail-closed".into());
    }
    Ok(())
}

fn probe_build_file_bytes(
    inventory: &ProbeBuildInventory,
    relative: &str,
) -> Result<Vec<u8>, String> {
    let file = inventory
        .files
        .iter()
        .find(|file| file.relative == relative)
        .ok_or_else(|| format!("probe build omitted required file {relative}"))?;
    read_bounded_regular_file(&file.source, file.length, relative)
}

fn read_bounded_regular_file(
    path: &Path,
    expected_length: u64,
    relative: &str,
) -> Result<Vec<u8>, String> {
    let file = fs::File::open(path)
        .map_err(|error| format!("cannot open probe build file {relative}: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect open probe build file {relative}: {error}"))?;
    if !metadata.is_file()
        || metadata.len() != expected_length
        || metadata.len() > MAX_EXTENSION_TREE_FILE_BYTES
    {
        return Err(format!(
            "probe build file changed shape while open: {relative}"
        ));
    }
    let capacity = usize::try_from(expected_length)
        .map_err(|_| format!("probe build file length does not fit memory: {relative}"))?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(expected_length + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read probe build file {relative}: {error}"))?;
    if bytes.len() as u64 != expected_length {
        return Err(format!(
            "probe build file changed length while reading: {relative}"
        ));
    }
    Ok(bytes)
}

fn portable_relative_path(path: &Path) -> Result<String, String> {
    let mut encoded = String::new();
    for component in path.components() {
        let Component::Normal(component) = component else {
            return Err("probe build contains a non-portable path".into());
        };
        let component = component
            .to_str()
            .ok_or_else(|| "probe build contains a non-UTF-8 path".to_owned())?;
        if !encoded.is_empty() {
            encoded.push('/');
        }
        encoded.push_str(component);
    }
    validate_relative_path(&encoded)?;
    Ok(encoded)
}

fn adapt_macos_probe_file(path: &str, source: &str) -> Result<String, String> {
    match path {
        "apps/browser/src/autofill/fido2/background/fido2.background.ts" => replace_exact(
            path,
            source,
            "world: chrome.scripting.ExecutionWorld.MAIN",
            "world: \"MAIN\" as chrome.scripting.ExecutionWorld",
        ),
        "apps/browser/src/background/main.background.ts" => {
            const BEFORE: &str = r#"    const localStorageStorageService = BrowserApi.isManifestVersion(3)
      ? new OffscreenStorageService(this.offscreenDocumentService)
      : new WindowStorageService(self.localStorage);

    const storageServiceProvider = new BrowserStorageServiceProvider(
      this.storageService,
      this.memoryStorageForStateProviders,
      this.largeObjectMemoryStorageForStateProviders,
      new PrimarySecondaryStorageService(this.storageService, localStorageStorageService),
    );"#;
            const AFTER: &str = r#"    const diskBackupLocalStorage = BrowserApi.isManifestVersion(3)
      ? this.offscreenDocumentService.offscreenApiSupported()
        ? new PrimarySecondaryStorageService(
            this.storageService,
            new OffscreenStorageService(this.offscreenDocumentService),
          )
        : this.storageService
      : new PrimarySecondaryStorageService(
          this.storageService,
          new WindowStorageService(self.localStorage),
        );

    const storageServiceProvider = new BrowserStorageServiceProvider(
      this.storageService,
      this.memoryStorageForStateProviders,
      this.largeObjectMemoryStorageForStateProviders,
      diskBackupLocalStorage,
    );"#;
            let adapted = replace_exact(path, source, BEFORE, AFTER)?;
            const NOTIFICATION_BEFORE: &str = r#"  initNotificationSubscriptions() {
    const handlers: Array<{"#;
            const NOTIFICATION_AFTER: &str = r#"  initNotificationSubscriptions() {
    if (!this.systemNotificationService.isSupported()) {
      return;
    }

    const handlers: Array<{"#;
            replace_exact(path, &adapted, NOTIFICATION_BEFORE, NOTIFICATION_AFTER)
        }
        "apps/browser/src/autofill/overlay/inline-menu/iframe-content/autofill-inline-menu-iframe.service.ts" => {
            const BEFORE: &str = r#"  initMenuIframe() {
    this.defaultIframeAttributes.src = BrowserApi.getRuntimeURL("overlay/menu.html");
    this.defaultIframeAttributes.title = this.iframeTitle;

    this.iframe = globalThis.document.createElement("iframe");
    for (const [attribute, value] of Object.entries(this.defaultIframeAttributes)) {
      this.iframe.setAttribute(attribute, value);
    }
    this.iframeStyles = { ...this.iframeStyles, ...this.initStyles };
    this.setElementStyles(this.iframe, this.iframeStyles, true);
    this.iframe.addEventListener(EVENTS.LOAD, this.setupPortMessageListener);

    if (this.ariaAlert) {
      this.createAriaAlertElement();
    }

    this.shadow.appendChild(this.iframe);
    this.observeIframe();
  }"#;
            const AFTER: &str = r#"  initMenuIframe() {
    // WKWebExtension does not enforce manifest sandbox pages, including when
    // their extension URL is placed in an explicit sandboxed iframe. The
    // authenticated probe overlay therefore refuses this UI path until the
    // sealed inert-payload renderer is applied and attested end to end.
    this.forceCloseInlineMenu();
  }"#;
            replace_exact(path, source, BEFORE, AFTER)
        }
        "apps/browser/src/manifest.v3.json" => adapt_probe_manifest(path, source),
        "apps/browser/src/popup/services/init.service.ts" => {
            const BEFORE: &str = r#"    return async () => {
      await this.sdkLoadService.loadAndInit();
      await this.migrationRunner.waitForCompletion(); // Browser background is responsible for migrations
      await this.i18nService.init();
      this.twoFactorService.init();
      await this.viewCacheService.init();
      await this.sizeService.init();"#;
            const AFTER: &str = r#"    return async () => {
      const probeGlobal = globalThis as typeof globalThis & {
        __zephiumPopupInitStage?: string;
      };
      probeGlobal.__zephiumPopupInitStage = "sdk";
      await this.sdkLoadService.loadAndInit();
      probeGlobal.__zephiumPopupInitStage = "migrations";
      await this.migrationRunner.waitForCompletion(); // Browser background is responsible for migrations
      probeGlobal.__zephiumPopupInitStage = "i18n";
      await this.i18nService.init();
      probeGlobal.__zephiumPopupInitStage = "two-factor";
      this.twoFactorService.init();
      probeGlobal.__zephiumPopupInitStage = "view-cache";
      await this.viewCacheService.init();
      probeGlobal.__zephiumPopupInitStage = "size";
      await this.sizeService.init();
      probeGlobal.__zephiumPopupInitStage = "complete";"#;
            replace_exact(path, source, BEFORE, AFTER)
        }
        "apps/browser/src/platform/services/platform-utils/browser-platform-utils.service.ts" => {
            const BEFORE: &str = r#"    } else if (BrowserPlatformUtilsService.isSafari(globalContext)) {
      this.deviceCache = DeviceType.SafariExtension;
    }

    return this.deviceCache;"#;
            const AFTER: &str = r#"    } else if (BrowserPlatformUtilsService.isSafari(globalContext)) {
      this.deviceCache = DeviceType.SafariExtension;
    } else {
      // WKWebExtension service workers do not expose a branded Safari user
      // agent. This source is admitted only into Zephium's sealed macOS build,
      // whose native API and lifecycle semantics are Safari-extension shaped.
      this.deviceCache = DeviceType.SafariExtension;
    }

    return this.deviceCache;"#;
            replace_exact(path, source, BEFORE, AFTER)
        }
        "apps/browser/src/platform/services/sdk/browser-sdk-load.service.ts" => {
            let adapted = source.to_owned();
            const LOAD_BEFORE: &str = r#"  async load(): Promise<void> {
    const startTime = performance.now();
    await importModule().then((initSdk) => initSdk());
    const endTime = performance.now();"#;
            const LOAD_AFTER: &str = r#"  async load(): Promise<void> {
    const probeGlobal = globalThis as typeof globalThis & {
      __zephiumPopupInitStage?: string;
    };
    const startTime = performance.now();
    probeGlobal.__zephiumPopupInitStage = "sdk-import";
    const initSdk = await importModule();
    probeGlobal.__zephiumPopupInitStage = "sdk-bindings";
    initSdk();
    probeGlobal.__zephiumPopupInitStage = "sdk-bindings-ready";
    const endTime = performance.now();"#;
            replace_exact(path, &adapted, LOAD_BEFORE, LOAD_AFTER)
        }
        _ => Err(format!("no macOS probe adaptation is defined for {path}")),
    }
}

fn adapt_probe_manifest(path: &str, source: &str) -> Result<String, String> {
    let without_sandbox = replace_exact(
        path,
        source,
        "  \"sandbox\": {\n    \"pages\": [\"overlay/menu-button.html\", \"overlay/menu-list.html\"]\n  },\n",
        "",
    )?;
    let without_button = replace_exact(
        path,
        &without_sandbox,
        "        \"overlay/menu-button.html\",\n",
        "",
    )?;
    let without_list = replace_exact(
        path,
        &without_button,
        "        \"overlay/menu-list.html\",\n",
        "",
    )?;
    replace_exact(path, &without_list, "        \"overlay/menu.html\",\n", "")
}

fn replace_exact(path: &str, source: &str, before: &str, after: &str) -> Result<String, String> {
    let occurrences = source.match_indices(before).count();
    if occurrences != 1 {
        return Err(format!(
            "{path} adaptation preimage drifted: expected one occurrence, observed {occurrences}"
        ));
    }
    Ok(source.replacen(before, after, 1))
}

fn ensure_absent_output(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(format!("output already exists: {}", path.display())),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot inspect overlay output: {error}")),
    }
}

fn write_overlay_file(root: &Path, relative: &str, bytes: &[u8]) -> Result<(), String> {
    validate_relative_path(relative)?;
    let path = root.join(relative);
    let parent = path
        .parent()
        .ok_or_else(|| format!("overlay file has no parent: {relative}"))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create overlay directory for {relative}: {error}"))?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("cannot create overlay file {relative}: {error}"))?;
    file.write_all(bytes)
        .map_err(|error| format!("cannot write overlay file {relative}: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("cannot sync overlay file {relative}: {error}"))
}

fn run_git(root: &Path, arguments: &[&str]) -> Result<String, String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("cannot execute git: {error}"))?;
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .ok_or_else(|| "cannot capture git output".to_owned())?
        .take(MAX_GIT_OUTPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read git output: {error}"))?;
    if bytes.len() as u64 > MAX_GIT_OUTPUT_BYTES {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!(
            "git {} exceeded the bounded output allowance",
            arguments.join(" ")
        ));
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for git: {error}"))?;
    if !status.success() {
        return Err(format!(
            "git {} failed with status {}",
            arguments.join(" "),
            status
        ));
    }
    String::from_utf8(bytes)
        .map(|value| value.trim_end_matches(['\r', '\n']).to_owned())
        .map_err(|_| format!("git {} emitted non-UTF-8 output", arguments.join(" ")))
}

fn read_git_boolean(root: &Path, key: &str) -> Result<bool, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["config", "--bool", "--get", key])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("cannot read git configuration: {error}"))?;
    match output.status.code() {
        Some(0) => match output.stdout.as_slice() {
            b"true\n" | b"true\r\n" => Ok(true),
            b"false\n" | b"false\r\n" => Ok(false),
            _ => Err(format!(
                "git configuration {key} is not a canonical boolean"
            )),
        },
        Some(1) => Ok(false),
        _ => Err(format!("cannot read git configuration {key}")),
    }
}

fn require_exact_line(subject: &str, actual: &str, expected: &str) -> Result<(), String> {
    if actual == expected && !actual.contains(['\r', '\n']) {
        Ok(())
    } else {
        Err(format!("{subject} does not match the reviewed value"))
    }
}

fn require_occurrences(path: &str, source: &str, marker: SourceMarker) -> Result<(), String> {
    let actual = source.match_indices(marker.text).count();
    if actual == marker.occurrences {
        Ok(())
    } else {
        Err(format!(
            "{path} marker drifted: expected {} occurrence(s), observed {actual}",
            marker.occurrences
        ))
    }
}

fn validate_relative_path(path: &str) -> Result<(), String> {
    let parsed = Path::new(path);
    if path.is_empty()
        || parsed.is_absolute()
        || parsed
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        Err(format!(
            "reviewed path is not portable and relative: {path}"
        ))
    } else {
        Ok(())
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    lower_hex(&Sha256::digest(bytes))
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing into a String cannot fail");
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe_manifest() -> serde_json::Value {
        serde_json::json!({
            "manifest_version": 3,
            "minimum_chrome_version": "102.0",
            "name": "__MSG_extName__",
            "version": "2026.7.0",
            "default_locale": "en",
            "background": {"service_worker": "background.js"},
            "permissions": [
                "activeTab", "alarms", "clipboardRead", "clipboardWrite", "contextMenus",
                "idle", "offscreen", "scripting", "sidePanel", "storage", "tabs",
                "unlimitedStorage", "webNavigation", "webRequest", "webRequestAuthProvider",
                "notifications"
            ],
            "optional_permissions": ["nativeMessaging", "privacy"],
            "host_permissions": ["https://*/*", "http://*/*"],
            "content_security_policy": {
                "extension_pages": "script-src 'self' 'wasm-unsafe-eval'; object-src 'self'"
            },
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
            "web_accessible_resources": [{
                "resources": [
                    "content/fido2-page-script.js", "notification/bar.html", "images/icon38.png",
                    "images/icon38_locked.png", "popup/fonts/*"
                ],
                "matches": ["<all_urls>"],
                "use_dynamic_url": true
            }],
            "action": {
                "default_popup": "popup/index.html",
                "default_title": "Bitwarden"
            },
            "storage": {"managed_schema": "managed_schema.json"}
        })
    }

    fn write_probe_build_fixture(root: &Path) {
        let manifest = serde_json::to_vec(&probe_manifest()).unwrap();
        for required in REQUIRED_PROBE_ARTIFACT_FILES {
            let bytes: &[u8] = match *required {
                "manifest.json" => &manifest,
                "background.js" => b"const world=\"MAIN\";function offscreenApiSupported(){}",
                "content/bootstrap-autofill-overlay-menu.js" => {
                    b"function forceCloseInlineMenu(){}"
                }
                SOURCE_POPUP_ENTRYPOINT => {
                    b"<!doctype html><base href=\"\"/><script defer=\"defer\" src=\"../popup/polyfills.js\"></script><script defer=\"defer\" src=\"../popup/vendor.js\"></script><script defer=\"defer\" src=\"../popup/vendor-angular.js\"></script><script defer=\"defer\" src=\"../popup/main.js\"></script><body><app-root></app-root></body>"
                }
                _ => b"fixture",
            };
            write_overlay_file(root, required, bytes).unwrap();
        }
        for path in EXPECTED_SOURCE_MAPS {
            write_overlay_file(root, path, b"source map").unwrap();
        }
        for path in UNSAFE_INLINE_MENU_FILES {
            write_overlay_file(root, path, b"unsafe menu").unwrap();
        }

        let required_count = REQUIRED_PROBE_ARTIFACT_FILES.len();
        for index in 0..(EXPECTED_FINAL_FILE_COUNT - required_count) {
            write_overlay_file(
                root,
                &format!("fixture/resource-{index:03}.bin"),
                b"fixture",
            )
            .unwrap();
        }
    }

    #[test]
    fn reviewed_inventory_is_unique_portable_and_bounded() {
        let mut paths = SOURCE_FILES
            .iter()
            .map(|file| file.path)
            .collect::<Vec<_>>();
        for path in &paths {
            validate_relative_path(path).unwrap();
        }
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), SOURCE_FILES.len());
        assert!(SOURCE_FILES.iter().all(|file| file.sha256.len() == 64));
    }

    #[test]
    fn exact_marker_cardinality_fails_closed() {
        let marker = SourceMarker {
            text: "needle",
            occurrences: 1,
        };
        assert!(require_occurrences("fixture", "before needle after", marker).is_ok());
        assert!(require_occurrences("fixture", "no match", marker).is_err());
        assert!(require_occurrences("fixture", "needle needle", marker).is_err());
    }

    #[test]
    fn digest_and_exact_line_checks_are_stable() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(require_exact_line("fixture", "value", "value").is_ok());
        assert!(require_exact_line("fixture", "value\nother", "value").is_err());
    }

    #[test]
    fn main_world_and_offscreen_adaptations_are_exact_and_fail_closed() {
        let fido = "before world: chrome.scripting.ExecutionWorld.MAIN after";
        let adapted = adapt_macos_probe_file(
            "apps/browser/src/autofill/fido2/background/fido2.background.ts",
            fido,
        )
        .unwrap();
        assert_eq!(
            adapted,
            "before world: \"MAIN\" as chrome.scripting.ExecutionWorld after"
        );
        assert!(adapt_macos_probe_file(
            "apps/browser/src/autofill/fido2/background/fido2.background.ts",
            &format!("{fido} {fido}"),
        )
        .is_err());

        let storage = r#"    const localStorageStorageService = BrowserApi.isManifestVersion(3)
      ? new OffscreenStorageService(this.offscreenDocumentService)
      : new WindowStorageService(self.localStorage);

    const storageServiceProvider = new BrowserStorageServiceProvider(
      this.storageService,
      this.memoryStorageForStateProviders,
      this.largeObjectMemoryStorageForStateProviders,
      new PrimarySecondaryStorageService(this.storageService, localStorageStorageService),
    );

  initNotificationSubscriptions() {
    const handlers: Array<{"#;
        let adapted =
            adapt_macos_probe_file("apps/browser/src/background/main.background.ts", storage)
                .unwrap();
        assert!(adapted.contains("offscreenApiSupported()"));
        assert!(adapted.contains(": this.storageService"));
        assert!(adapted.contains("diskBackupLocalStorage,"));
        assert!(!adapted.contains("const localStorageStorageService"));
        assert!(adapted.contains("!this.systemNotificationService.isSupported()"));

        let platform = r#"    } else if (BrowserPlatformUtilsService.isSafari(globalContext)) {
      this.deviceCache = DeviceType.SafariExtension;
    }

    return this.deviceCache;"#;
        let adapted = adapt_macos_probe_file(
            "apps/browser/src/platform/services/platform-utils/browser-platform-utils.service.ts",
            platform,
        )
        .unwrap();
        assert!(adapted.contains("WKWebExtension service workers"));
        assert_eq!(
            adapted
                .match_indices("this.deviceCache = DeviceType.SafariExtension;")
                .count(),
            2
        );

        let sdk = r#"before
// https://stackoverflow.com/a/47880734
const supported = (() => {
after
  async load(): Promise<void> {
    const startTime = performance.now();
    await importModule().then((initSdk) => initSdk());
    const endTime = performance.now();"#;
        let adapted = adapt_macos_probe_file(
            "apps/browser/src/platform/services/sdk/browser-sdk-load.service.ts",
            sdk,
        )
        .unwrap();
        assert!(!adapted.contains("__zephiumWasmCompatibilityStage"));
        assert_eq!(adapted.matches("const supported = (() => {").count(), 1);
        assert!(adapted.contains("__zephiumPopupInitStage = \"sdk-import\""));
        assert!(adapted.contains("__zephiumPopupInitStage = \"sdk-bindings-ready\""));

        let init = r#"    return async () => {
      await this.sdkLoadService.loadAndInit();
      await this.migrationRunner.waitForCompletion(); // Browser background is responsible for migrations
      await this.i18nService.init();
      this.twoFactorService.init();
      await this.viewCacheService.init();
      await this.sizeService.init();"#;
        let adapted =
            adapt_macos_probe_file("apps/browser/src/popup/services/init.service.ts", init)
                .unwrap();
        assert!(adapted.contains("__zephiumPopupInitStage = \"sdk\""));
        assert!(adapted.contains("__zephiumPopupInitStage = \"migrations\""));
        assert!(adapted.contains("__zephiumPopupInitStage = \"complete\""));
    }

    #[test]
    fn probe_overlay_removes_every_unsafe_inline_menu_entrypoint() {
        let manifest = r#"{
  "sandbox": {
    "pages": ["overlay/menu-button.html", "overlay/menu-list.html"]
  },
  "resources": [
        "overlay/menu-button.html",
        "overlay/menu-list.html",
        "overlay/menu.html",
        "kept.html"
  ]
}"#;
        let adapted = adapt_probe_manifest("manifest", manifest).unwrap();
        assert!(!adapted.contains("\"sandbox\""));
        assert!(!adapted.contains("overlay/menu-button.html"));
        assert!(!adapted.contains("overlay/menu-list.html"));
        assert!(!adapted.contains("overlay/menu.html"));
        assert!(adapted.contains("kept.html"));

        let service = r#"  initMenuIframe() {
    this.defaultIframeAttributes.src = BrowserApi.getRuntimeURL("overlay/menu.html");
    this.defaultIframeAttributes.title = this.iframeTitle;

    this.iframe = globalThis.document.createElement("iframe");
    for (const [attribute, value] of Object.entries(this.defaultIframeAttributes)) {
      this.iframe.setAttribute(attribute, value);
    }
    this.iframeStyles = { ...this.iframeStyles, ...this.initStyles };
    this.setElementStyles(this.iframe, this.iframeStyles, true);
    this.iframe.addEventListener(EVENTS.LOAD, this.setupPortMessageListener);

    if (this.ariaAlert) {
      this.createAriaAlertElement();
    }

    this.shadow.appendChild(this.iframe);
    this.observeIframe();
  }"#;
        let adapted = adapt_macos_probe_file(
            "apps/browser/src/autofill/overlay/inline-menu/iframe-content/autofill-inline-menu-iframe.service.ts",
            service,
        )
        .unwrap();
        assert!(adapted.contains("this.forceCloseInlineMenu();"));
        assert!(!adapted.contains("createElement(\"iframe\")"));
        assert!(!adapted.contains("BrowserApi.getRuntimeURL"));
    }

    #[test]
    fn overlay_writer_never_replaces_an_existing_file() {
        let temp = tempfile::tempdir().unwrap();
        write_overlay_file(temp.path(), "nested/file", b"first").unwrap();
        assert!(write_overlay_file(temp.path(), "nested/file", b"second").is_err());
        assert_eq!(fs::read(temp.path().join("nested/file")).unwrap(), b"first");
    }

    #[test]
    fn probe_artifact_is_closed_canonical_non_product_and_no_replace() {
        let temp = tempfile::tempdir().unwrap();
        let build = temp.path().join("build");
        fs::create_dir(&build).unwrap();
        write_probe_build_fixture(&build);
        let output = temp.path().join("artifact");

        finalize_macos_probe_artifact(&build, &output).unwrap();
        assert!(!output
            .join(PROBE_ARTIFACT_EXTENSION_DIRECTORY)
            .join("background.js.map")
            .exists());
        assert!(!output
            .join(PROBE_ARTIFACT_EXTENSION_DIRECTORY)
            .join("overlay/menu.html")
            .exists());
        assert!(output
            .join(PROBE_ARTIFACT_EXTENSION_DIRECTORY)
            .join(SOURCE_POPUP_ENTRYPOINT)
            .exists());
        let index = fs::read(output.join(PROBE_ARTIFACT_INDEX)).unwrap();
        let index = CanonicalExtensionTreeIndex::parse_canonical(&index).unwrap();
        assert_eq!(index.files().len(), EXPECTED_INSTRUMENTED_FILE_COUNT);
        let metadata: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join(PROBE_ARTIFACT_METADATA)).unwrap())
                .unwrap();
        assert_eq!(metadata["product_authority"], false);
        assert_eq!(metadata["file_count"], EXPECTED_INSTRUMENTED_FILE_COUNT);
        assert_eq!(metadata["background_diagnostics"], true);
        assert_eq!(metadata["popup_diagnostics"], true);
        assert_eq!(
            metadata["compatibility_adaptations"][0],
            "webkit-extension-device-classification"
        );
        assert_eq!(
            metadata["tree_sha256"],
            lower_hex(index.tree_sha256().as_bytes())
        );
        assert_eq!(
            metadata["tree_index_sha256"],
            lower_hex(index.index_sha256().as_bytes())
        );
        assert_eq!(
            metadata["manifest_sha256"],
            lower_hex(index.manifest_sha256().as_bytes())
        );
        assert!(finalize_macos_probe_artifact(&build, &output).is_err());
    }
}
