//! Live, source-free contract for APIs used by ordinary major extensions.
//!
//! The fixture is Zephium-owned and intentionally small. Its declaration
//! shape covers browser-owned APIs used by ordinary major extensions without
//! downloading, embedding, or executing third-party code. This gate answers
//! two separate questions: which permission tokens WebKit exposes through its
//! native grant set, and which JavaScript namespaces a granted extension page
//! actually receives. It is platform evidence, not a compatibility claim for
//! any upstream package.

use std::path::{Path, PathBuf};
use std::time::Instant;

use objc2::rc::Weak;
use objc2_foundation::{MainThreadMarker, NSRunLoop};
use objc2_web_kit::{
    WKWebExtension, WKWebExtensionContext, WKWebExtensionController, WKWebView, WKWebsiteDataStore,
};
use serde_json::{json, Value};

const CONTRACT_PRINCIPAL: &str = "cccccccccccccccccccccccccccccccc";
const PENDING_TITLE: &str = "zephium-major-extension-contract-pending";
const DECLARED_PERMISSIONS: [&str; 8] = [
    "bookmarks",
    "favicon",
    "fontSettings",
    "history",
    "search",
    "sessions",
    "storage",
    "webNavigation",
];
const EXPECTED_NATIVE_PERMISSIONS: [&str; 2] = ["storage", "webNavigation"];
const NAMESPACE_NAMES: [&str; 20] = [
    "action",
    "actionSetIcon",
    "bookmarks",
    "fontSettings",
    "history",
    "runtime",
    "search",
    "sessions",
    "storageLocal",
    "storageSession",
    "storageSessionGet",
    "storageSessionSet",
    "storageSessionSetAccessLevel",
    "storageSync",
    "storageSyncGet",
    "storageSyncSet",
    "tabs",
    "webNavigationCommitted",
    "webNavigationHistoryStateUpdated",
    "webNavigationReferenceFragmentUpdated",
];
const EXPECTED_NAMESPACES: [(&str, &str); 20] = [
    ("action", "object"),
    ("actionSetIcon", "function"),
    ("bookmarks", "undefined"),
    ("fontSettings", "undefined"),
    ("history", "undefined"),
    ("runtime", "object"),
    ("search", "undefined"),
    ("sessions", "undefined"),
    ("storageLocal", "object"),
    ("storageSession", "object"),
    ("storageSessionGet", "function"),
    ("storageSessionSet", "function"),
    ("storageSessionSetAccessLevel", "function"),
    ("storageSync", "object"),
    ("storageSyncGet", "function"),
    ("storageSyncSet", "function"),
    ("tabs", "object"),
    ("webNavigationCommitted", "object"),
    ("webNavigationHistoryStateUpdated", "undefined"),
    ("webNavigationReferenceFragmentUpdated", "undefined"),
];

pub(super) struct RuntimeEvidence {
    pub(super) controller: Weak<WKWebExtensionController>,
    pub(super) context: Weak<WKWebExtensionContext>,
    pub(super) view: Weak<WKWebView>,
    pub(super) store: Weak<WKWebsiteDataStore>,
    pub(super) native_permissions: Box<[String]>,
    pub(super) namespace_summary: Box<str>,
}

pub(super) fn write_fixture(root: &Path) -> Result<PathBuf, String> {
    let path = root.join("major-extension-contract");
    std::fs::create_dir(&path)
        .map_err(|error| format!("cannot create major-extension contract fixture: {error}"))?;
    let manifest = json!({
        "manifest_version": 3,
        "name": "Zephium Major Extension API Contract Probe",
        "version": "1.0.0",
        "description": "Zephium-owned source-free ordinary-extension capability fixture.",
        "action": {"default_title": "Zephium contract"},
        "permissions": DECLARED_PERMISSIONS
    });
    write(&path, "manifest.json", &manifest.to_string())?;
    write(
        &path,
        "probe.html",
        "<!doctype html><meta charset=\"utf-8\"><title>zephium-major-extension-contract-pending</title><script src=\"probe.js\"></script>",
    )?;
    write(
        &path,
        "probe.js",
        r#"(() => {
    'use strict';
    const type = (value) => typeof value;
    document.title = JSON.stringify({
        action: type(globalThis.chrome?.action),
        actionSetIcon: type(globalThis.chrome?.action?.setIcon),
        bookmarks: type(globalThis.chrome?.bookmarks),
        fontSettings: type(globalThis.chrome?.fontSettings),
        history: type(globalThis.chrome?.history),
        runtime: type(globalThis.chrome?.runtime),
        search: type(globalThis.chrome?.search),
        sessions: type(globalThis.chrome?.sessions),
        storageLocal: type(globalThis.chrome?.storage?.local),
        storageSession: type(globalThis.chrome?.storage?.session),
        storageSessionGet: type(globalThis.chrome?.storage?.session?.get),
        storageSessionSet: type(globalThis.chrome?.storage?.session?.set),
        storageSessionSetAccessLevel: type(globalThis.chrome?.storage?.session?.setAccessLevel),
        storageSync: type(globalThis.chrome?.storage?.sync),
        storageSyncGet: type(globalThis.chrome?.storage?.sync?.get),
        storageSyncSet: type(globalThis.chrome?.storage?.sync?.set),
        tabs: type(globalThis.chrome?.tabs),
        webNavigationCommitted: type(globalThis.chrome?.webNavigation?.onCommitted),
        webNavigationHistoryStateUpdated: type(globalThis.chrome?.webNavigation?.onHistoryStateUpdated),
        webNavigationReferenceFragmentUpdated: type(globalThis.chrome?.webNavigation?.onReferenceFragmentUpdated)
    });
})()"#,
    )?;
    Ok(path)
}

pub(super) fn run(
    extension: &WKWebExtension,
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
) -> Result<RuntimeEvidence, String> {
    let native_permissions = inspect_parse_contract(extension)?;
    let bundle = super::new_nonpersistent_controller(mtm)?;
    let controller = bundle.controller.clone();
    let store = bundle._data_store.clone();
    let context = super::new_context(extension, CONTRACT_PRINCIPAL)?;
    let applied = super::super::extensions::apply_probe_grants(
        &context,
        &[
            super::super::extensions::MacosNativeApiPermission::Storage,
            super::super::extensions::MacosNativeApiPermission::WebNavigation,
        ],
        &[],
        false,
    )
    .map_err(|error| format!("major-extension storage grant application failed: {error}"))?;

    let controller_weak = Weak::from_retained(&controller);
    let context_weak = Weak::from_retained(&context);
    let store_weak = Weak::from_retained(&store);
    let mut window = None;
    let mut view = None;
    let mut view_weak = None;
    let mut loaded = false;
    let gate = (|| {
        super::load_context(&controller, &context, "major-extension contract")?;
        loaded = true;
        let configuration = unsafe { context.webViewConfiguration() }.ok_or_else(|| {
            "loaded major-extension contract returned no extension-page configuration".to_owned()
        })?;
        let probe_window = super::new_window(mtm)?;
        let host =
            super::profile_isolation::host_for_window(&probe_window, "major-extension contract")?;
        let probe_view = super::profile_isolation::build_profile_view(&host, configuration)?;
        probe_window.orderFrontRegardless();
        let native_view = super::super::native::webkit(&probe_view);
        super::assert_attached_controller(&native_view, &controller)?;
        super::profile_isolation::assert_attached_store(&native_view, &store)?;
        view_weak = Some(Weak::from_retained(&native_view));
        drop(native_view);

        let page = unsafe { context.baseURL() }
            .URLByAppendingPathComponent(&objc2_foundation::NSString::from_str("probe.html"))
            .and_then(|url| url.absoluteString())
            .ok_or_else(|| "major-extension contract produced no probe URL".to_owned())?
            .to_string();
        probe_view
            .load_url(&page)
            .map_err(|error| format!("cannot navigate major-extension contract: {error}"))?;
        view = Some(probe_view);
        window = Some(probe_window);
        read_namespace_evidence(
            view.as_ref().expect("major-extension view was stored"),
            &context,
            run_loop,
        )
    })();

    let mut cleanup_failures = Vec::new();
    if loaded {
        if let Err(error) = super::unload_context(&controller, &context, "major-extension contract")
        {
            cleanup_failures.push(error);
        }
    }
    if let Err(error) = applied.clear_and_verify(&context) {
        cleanup_failures.push(format!("major-extension grant cleanup failed: {error}"));
    }
    drop(view.take());
    if let Some(window) = window.take() {
        window.close();
        drop(window);
    }
    drop(context);
    drop(controller);
    drop(store);
    drop(bundle);

    let cleanup = if cleanup_failures.is_empty() {
        Ok(())
    } else {
        Err(cleanup_failures.join("; "))
    };
    match (gate, cleanup) {
        (Ok(namespace_summary), Ok(())) => Ok(RuntimeEvidence {
            controller: controller_weak,
            context: context_weak,
            view: view_weak.expect("successful major-extension gate constructed a view"),
            store: store_weak,
            native_permissions,
            namespace_summary,
        }),
        (Err(gate), Ok(())) => Err(gate),
        (Ok(_), Err(cleanup)) => Err(format!("major-extension native cleanup failed: {cleanup}")),
        (Err(gate), Err(cleanup)) => Err(format!(
            "{gate}; major-extension native cleanup also failed: {cleanup}"
        )),
    }
}

fn inspect_parse_contract(extension: &WKWebExtension) -> Result<Box<[String]>, String> {
    if unsafe { extension.manifestVersion() } != 3.0 {
        return Err("major-extension contract was not parsed as manifest v3".into());
    }
    let errors = unsafe { extension.errors() };
    if errors.count() != 0 {
        return Err(format!(
            "major-extension contract parsed with {} error(s): {}",
            errors.count(),
            super::describe_native_errors(&errors),
        ));
    }
    let permissions = unsafe { extension.requestedPermissions() };
    let objects = permissions.allObjects();
    let mut names = (0..objects.count())
        .map(|index| objects.objectAtIndex(index).to_string())
        .collect::<Vec<_>>();
    names.sort_unstable();
    if !names
        .iter()
        .map(String::as_str)
        .eq(EXPECTED_NATIVE_PERMISSIONS)
    {
        return Err(format!(
            "major-extension native permission projection drifted: expected {EXPECTED_NATIVE_PERMISSIONS:?}, got {names:?}"
        ));
    }
    Ok(names.into_boxed_slice())
}

fn read_namespace_evidence(
    view: &wry::WebView,
    context: &WKWebExtensionContext,
    run_loop: &NSRunLoop,
) -> Result<Box<str>, String> {
    let deadline = Instant::now() + super::PROBE_TIMEOUT;
    let evidence: Value = loop {
        let title = view
            .document_title()
            .map_err(|error| format!("cannot inspect major-extension probe title: {error}"))?;
        if let Some(title) = title
            .as_deref()
            .filter(|title| !title.is_empty() && *title != PENDING_TITLE)
        {
            break serde_json::from_str(title).map_err(|error| {
                format!("major-extension probe returned invalid evidence {title:?}: {error}")
            })?;
        }
        super::validate_context_errors(context, "major-extension namespace probe")?;
        if Instant::now() >= deadline {
            return Err(format!(
                "major-extension namespace probe timed out at {:?}",
                view.url().ok()
            ));
        }
        super::drain_run_loop_once(run_loop);
    };
    let object = evidence.as_object().ok_or_else(|| {
        format!("major-extension namespace evidence is not an object: {evidence}")
    })?;
    if object.len() != EXPECTED_NAMESPACES.len()
        || EXPECTED_NAMESPACES
            .iter()
            .any(|(name, expected)| object.get(*name).and_then(Value::as_str) != Some(*expected))
    {
        return Err(format!(
            "major-extension namespace evidence drifted from the closed runtime contract: {evidence}"
        ));
    }
    let summary = NAMESPACE_NAMES
        .iter()
        .map(|name| {
            format!(
                "{name}:{}",
                object
                    .get(*name)
                    .and_then(Value::as_str)
                    .expect("schema checked above")
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    Ok(summary.into_boxed_str())
}

fn write(directory: &Path, name: &str, contents: &str) -> Result<(), String> {
    std::fs::write(directory.join(name), contents)
        .map_err(|error| format!("cannot write major-extension contract fixture {name}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_is_source_free_and_keeps_the_browser_owned_api_delta_explicit() {
        let temp = tempfile::tempdir().expect("temporary contract root");
        let fixture = write_fixture(temp.path()).expect("major-extension contract fixture");
        let manifest: Value = serde_json::from_slice(
            &std::fs::read(fixture.join("manifest.json")).expect("manifest bytes"),
        )
        .expect("manifest JSON");
        assert_eq!(manifest["permissions"], json!(DECLARED_PERMISSIONS));
        assert_eq!(EXPECTED_NATIVE_PERMISSIONS, ["storage", "webNavigation"]);
        assert_eq!(manifest["manifest_version"], 3);
        assert_eq!(manifest["action"]["default_title"], "Zephium contract");
        assert_eq!(
            std::fs::read_dir(&fixture)
                .expect("fixture directory")
                .count(),
            3
        );
    }

    #[test]
    fn namespace_probe_has_exact_bounded_inventory() {
        let temp = tempfile::tempdir().expect("temporary contract root");
        let fixture = write_fixture(temp.path()).expect("major-extension contract fixture");
        let script = std::fs::read_to_string(fixture.join("probe.js")).expect("probe script");
        for namespace in NAMESPACE_NAMES {
            assert!(script.contains(namespace));
        }
        assert_eq!(EXPECTED_NAMESPACES.map(|(name, _)| name), NAMESPACE_NAMES);
        assert!(!script.contains("fetch("));
        assert!(!script.contains("XMLHttpRequest"));
    }
}
