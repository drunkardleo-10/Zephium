use std::path::{Path, PathBuf};

const NATIVE_OWNER: &str = "crates/zephium-engine/src/platform/windows/extensions/native.rs";
const REVIEWED_ENVIRONMENT_FORWARDERS: [(&str, usize); 4] = [
    ("crates/zephium-engine/src/host/construction.rs", 1),
    ("vendor/tauri-runtime-wry/src/lib.rs", 1),
    ("vendor/tauri/src/webview/mod.rs", 2),
    ("vendor/tauri/src/webview/webview_window.rs", 3),
];

pub(crate) fn check(repository: &Path) -> Result<(), String> {
    let wry_root = repository.join("vendor/wry/src");
    let constructor_path = wry_root.join("webview2/mod.rs");
    let constructor_source = read_source(&constructor_path)?;
    validate_constructor(&constructor_source)?;

    let native_owner_path = repository.join(NATIVE_OWNER);
    let native_owner_source = read_source(&native_owner_path)?;
    validate_native_owner(&native_owner_source)?;

    let mut wry_sources = Vec::new();
    collect_rust_sources(&wry_root, &mut wry_sources)?;
    wry_sources.sort();
    for path in wry_sources {
        let source = read_source(&path)?;
        if has_native_install_token(&source) {
            return Err(format!(
                "vendored Wry may not install WebView2 extensions: {}",
                relative(repository, &path).display()
            ));
        }
        let native_setter = ["set_are_browser_extensions_", "enabled"].concat();
        if path != constructor_path && compact(&source).contains(&native_setter) {
            return Err(format!(
                "only vendor/wry/src/webview2/mod.rs may configure native WebView2 extension enablement: {}",
                relative(repository, &path).display()
            ));
        }
        reject_direct_enablement(relative(repository, &path), &compact(&source))?;
    }

    let mut shipping_sources = Vec::new();
    for root in [
        "crates",
        "desktop/src",
        "vendor/tauri/src",
        "vendor/tauri-runtime-wry/src",
    ] {
        collect_rust_sources(&repository.join(root), &mut shipping_sources)?;
    }
    shipping_sources.sort();
    for path in shipping_sources {
        let source = read_source(&path)?;
        validate_shipping_source(relative(repository, &path), &source)?;
    }
    Ok(())
}

fn read_source(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))
}

fn collect_rust_sources(directory: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("cannot enumerate {}: {error}", directory.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("cannot enumerate {}: {error}", directory.display()))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("cannot inspect {}: {error}", entry.path().display()))?;
        if is_symlink_or_reparse(&entry, file_type.is_symlink())? {
            return Err(format!(
                "source-gate roots may not contain symlinks: {}",
                entry.path().display()
            ));
        }
        if file_type.is_dir() && is_pruned_artifact_root(&entry.path()) {
            continue;
        }
        if file_type.is_dir() {
            collect_rust_sources(&entry.path(), output)?;
        } else if file_type.is_file() && entry.path().extension().is_some_and(|value| value == "rs")
        {
            output.push(entry.path());
        }
    }
    Ok(())
}

fn is_pruned_artifact_root(path: &Path) -> bool {
    path.ends_with(Path::new("crates/zephium-blocker/fuzz/target"))
}

fn is_symlink_or_reparse(_entry: &std::fs::DirEntry, is_symlink: bool) -> Result<bool, String> {
    if is_symlink {
        return Ok(true);
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::MetadataExt;

        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        let metadata = std::fs::symlink_metadata(_entry.path())
            .map_err(|error| format!("cannot inspect {}: {error}", _entry.path().display()))?;
        Ok(metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
    }
    #[cfg(not(target_os = "windows"))]
    Ok(false)
}

fn relative<'a>(repository: &'a Path, path: &'a Path) -> &'a Path {
    path.strip_prefix(repository).unwrap_or(path)
}

fn validate_constructor(source: &str) -> Result<(), String> {
    let policy = source
        .split_once("const fn extension_startup_refusal(")
        .map(|(_, policy)| policy)
        .ok_or_else(|| "WebView2 extension startup-refusal policy is missing".to_owned())?;
    let required_policy = concat!(
        "extension_path_configured:bool,browser_extensions_enabled:bool,",
        ")->Option<WebView2ExtensionStartupRefusal>{",
        "ifextension_path_configured{",
        "Some(WebView2ExtensionStartupRefusal::ExtensionPath)",
        "}elseifbrowser_extensions_enabled{",
        "Some(WebView2ExtensionStartupRefusal::StartupFence)",
        "}else{None}}"
    );
    if !compact(policy).starts_with(required_policy) {
        return Err(
            "WebView2 startup refusal must cover path and enablement with path precedence"
                .to_owned(),
        );
    }

    let function = source
        .split_once("fn new_in_hwnd(")
        .map(|(_, function)| function)
        .ok_or_else(|| "InnerWebView::new_in_hwnd is missing".to_owned())?;
    let body = function
        .split_once(") -> Result<Self> {")
        .map(|(_, body)| body)
        .ok_or_else(|| "InnerWebView::new_in_hwnd has an unrecognized signature".to_owned())?;
    let constructor = compact(body);
    let required_first_statement = concat!(
        "ifletSome(refusal)=extension_startup_refusal(",
        "pl_attrs.extension_path.is_some(),",
        "pl_attrs.browser_extensions_enabled,",
        "){returnErr(matchrefusal{",
        "WebView2ExtensionStartupRefusal::ExtensionPath=>",
        "Error::WebView2ExtensionPathUnsupported,",
        "WebView2ExtensionStartupRefusal::StartupFence=>{",
        "Error::WebView2ExtensionsStartupFenceUnavailable",
        "}});}"
    );
    if !constructor.starts_with(required_first_statement) {
        return Err(
            "InnerWebView::new_in_hwnd must refuse path and enablement before Wry initializes COM or creates its child HWND"
                .to_owned(),
        );
    }

    let removed_loader = ["load_", "extensions("].concat();
    if constructor.contains(&removed_loader) {
        return Err("the removed unmanaged extension-path loader was restored".to_owned());
    }

    let source = compact(source);
    let native_setter = ["set_are_browser_extensions_", "enabled"].concat();
    let closed_setter = ["set_are_browser_extensions_", "enabled(false)"].concat();
    if source.matches(&native_setter).count() != 1 || !source.contains(&closed_setter) {
        return Err(
            "Wry must create WebView2 environments with native extension enablement fixed false"
                .to_owned(),
        );
    }
    Ok(())
}

fn validate_shipping_source(relative: &Path, source: &str) -> Result<(), String> {
    if has_native_install_token(source) && relative != Path::new(NATIVE_OWNER) {
        return Err(format!(
            "{} contains the native extension-install token; only {NATIVE_OWNER} may own it",
            relative.display()
        ));
    }

    let compact = compact(source);
    let native_setter = ["set_are_browser_extensions_", "enabled"].concat();
    if identifier_occurrences(source, &native_setter) != 0 && relative != Path::new(NATIVE_OWNER) {
        return Err(format!(
            "{} configures native WebView2 extension enablement outside {NATIVE_OWNER}",
            relative.display()
        ));
    }

    for creation_token in [
        ["CreateCoreWebView2Environment", "WithOptions"].concat(),
        ["CoreWebView2Environment", "Options"].concat(),
    ] {
        if compact.contains(&creation_token) {
            return Err(format!(
                "{} creates WebView2 environments outside the closed Wry owner",
                relative.display()
            ));
        }
    }

    let forwarding = ["with_", "environment"].concat();
    let observed = identifier_occurrences(source, &forwarding);
    let expected = REVIEWED_ENVIRONMENT_FORWARDERS
        .iter()
        .find_map(|(path, count)| (relative == Path::new(path)).then_some(*count))
        .unwrap_or(0);
    if observed != expected {
        return Err(format!(
            "{} contains {observed} supplied-environment forwards, expected {expected}",
            relative.display()
        ));
    }

    reject_direct_enablement(relative, &compact)
}

fn validate_native_owner(source: &str) -> Result<(), String> {
    let source = compact(source);
    let install_entry = [".AddBrowser", "Extension("].concat();
    if source.matches(&install_entry).count() != 1 {
        return Err(format!(
            "{NATIVE_OWNER} must contain exactly one native extension-install entry"
        ));
    }
    for required in [
        "letresult=completion.and_then(|()|{extension.ok_or_else(",
        "ifletErr(unsent)=sender.send(result)",
        "Ok(Err(_))=>Err(WindowsNativeExtensionFailure::NativeCall(WindowsNativeExtensionCall::InstallCompletion",
        "wait_for_native_callback_until(receiver,deadline)",
        "MsgWaitForMultipleObjectsEx(None,timeout,QS_ALLINPUT,MWMO_INPUTAVAILABLE)",
        "PeekMessageW(&mutmessage,None,0,0,PM_REMOVE)",
        "message.message==WM_QUIT",
        "PostQuitMessage(message.wParam.0asi32)",
        "MAX_MESSAGES_PER_PUMP",
        "MAX_NATIVE_LIFECYCLE_OWNERS",
        "ifInstant::now()>=deadline",
        "NATIVE_CALLBACK_PUMP_ACTIVE.with(Cell::get)",
        "state.reserved_callback_slots+=1",
        "NativeLifecycleReservation::acquire()",
        "current>=MAX_NATIVE_LIFECYCLE_OWNERS",
        "_lifecycle:NativeLifecycleReservation",
        "fnretain_orphaned_lifecycle_owner(owner:OrphanedNativeObject){fail_stop_native_extension_admission();",
        "fnquarantine_unregistered(owner:OrphanedNativeObject){let_retained_until_process_exit=ManuallyDrop::new(owner);}",
        "extension:Option<ICoreWebView2BrowserExtension>",
        "environment:ICoreWebView2Environment",
        "profile:ICoreWebView2Profile7",
        "native_root:ExtensionRuntimeNativeRootLease",
        "super::super::attest_environment(&environment,expected_user_data_folder)",
        "ExtensionRuntimeNativeOwnerId::parse_exact(&owner)",
        "observed_owner!=expected_owner",
        "observed_owner:Option<ExtensionRuntimeNativeOwnerId>",
        "retained.observed_owner=Some(observed_owner)",
        "debug_assert_eq!(retained.observed_owner,Some(retained.expected_owner))",
        "count<=MAX_EXTENSION_INSTALLS_PER_PROFILE",
        "implDropforWindowsNativeExtensionCleanupDebt",
        "retain_orphaned_lifecycle_owner(OrphanedNativeObject::Lifecycle",
        "native_extension_cleanup_invariant_failed()",
        "OrphanedNativeObject::Extension(ManuallyDrop::new(extension))",
        "OrphanedNativeObject::Inventory(ManuallyDrop::new(inventory))",
        "WindowsNativeExtensionActivation::RejectedBeforeNative",
        "if!native_call_entered",
        "IdentityMismatchQuarantined",
        "destroyadifferentauthorizedinstall.Keepbothexact",
        ".field(\"profile\",&\"[redacted]\")",
        ".field(\"owner\",&\"[redacted]\")",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "{NATIVE_OWNER} is missing native install/identity/lifecycle contract: {required}"
            ));
        }
    }
    for (required, count) in [
        ("NativeCallbackReservation::acquire()?", 3),
        ("wait_for_native_callback_until(receiver,deadline)", 3),
        ("ifnative_extension_cleanup_invariant_failed()", 2),
        ("quarantine_unregistered(owner)", 7),
    ] {
        if source.matches(required).count() != count {
            return Err(format!(
                "{NATIVE_OWNER} must contain exactly {count} occurrences of {required}"
            ));
        }
    }
    for forbidden in [
        ["CreateCoreWebView2Environment", "WithOptions"].concat(),
        ["CoreWebView2Environment", "Options"].concat(),
        "with_browser_extensions_enabled(true)".to_owned(),
        "with_webview2_extension_startup_gate".to_owned(),
        "webview2_com::wait_with_pump".to_owned(),
        "std::mem::forget".to_owned(),
    ] {
        if source.contains(&forbidden) {
            return Err(format!(
                "{NATIVE_OWNER} may not create or enable a WebView2 environment in this milestone"
            ));
        }
    }
    Ok(())
}

fn reject_direct_enablement(relative: &Path, compact: &str) -> Result<(), String> {
    for direct_true in [
        [".with_browser_extensions_", "enabled(true)"].concat(),
        [".browser_extensions_", "enabled(true)"].concat(),
        ["set_are_browser_extensions_", "enabled(true)"].concat(),
        ["browser_extensions_", "enabled=true"].concat(),
        ["browser_extensions_", "enabled:true"].concat(),
    ] {
        if compact.contains(&direct_true) {
            return Err(format!(
                "{} enables WebView2 extensions before the startup inventory fence exists",
                relative.display()
            ));
        }
    }
    Ok(())
}

fn identifier_occurrences(source: &str, identifier: &str) -> usize {
    source
        .match_indices(identifier)
        .filter(|(start, value)| {
            let before = source[..*start].chars().next_back();
            let after = source[*start + value.len()..].chars().next();
            !before.is_some_and(is_identifier_character)
                && !after.is_some_and(is_identifier_character)
        })
        .count()
}

fn is_identifier_character(value: char) -> bool {
    value == '_' || value.is_alphanumeric()
}

fn has_native_install_token(source: &str) -> bool {
    let needle = ["AddBrowser", "Extension"].concat();
    compact(source).contains(&needle)
}

fn compact(source: &str) -> String {
    // This is intentionally a conservative source gate, not a partial Rust
    // parser. A matching comment or string fails closed and must be reworded.
    source
        .chars()
        .filter(|value| !value.is_whitespace())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAFE_WRY_SOURCE: &str = r#"
const fn extension_startup_refusal(
    extension_path_configured: bool,
    browser_extensions_enabled: bool,
) -> Option<WebView2ExtensionStartupRefusal> {
    if extension_path_configured {
        Some(WebView2ExtensionStartupRefusal::ExtensionPath)
    } else if browser_extensions_enabled {
        Some(WebView2ExtensionStartupRefusal::StartupFence)
    } else {
        None
    }
}

fn new_in_hwnd() -> Result<Self> {
    if let Some(refusal) = extension_startup_refusal(
        pl_attrs.extension_path.is_some(),
        pl_attrs.browser_extensions_enabled,
    ) {
        return Err(match refusal {
            WebView2ExtensionStartupRefusal::ExtensionPath =>
                Error::WebView2ExtensionPathUnsupported,
            WebView2ExtensionStartupRefusal::StartupFence => {
                Error::WebView2ExtensionsStartupFenceUnavailable
            }
        });
    }
    initialize_com();
}

fn create_environment() {
    options.set_are_browser_extensions_enabled(false);
}
"#;

    #[test]
    fn constructor_refuses_both_inputs_before_native_work_with_path_precedence() {
        assert!(validate_constructor(SAFE_WRY_SOURCE).is_ok());

        let late = SAFE_WRY_SOURCE.replacen(
            "if let Some(refusal) = extension_startup_refusal(",
            "initialize_com(); if let Some(refusal) = extension_startup_refusal(",
            1,
        );
        assert!(validate_constructor(&late).is_err());

        let missing_enablement =
            SAFE_WRY_SOURCE.replacen("pl_attrs.browser_extensions_enabled,", "false,", 1);
        assert!(validate_constructor(&missing_enablement).is_err());

        let reversed_precedence = SAFE_WRY_SOURCE.replacen(
            "if extension_path_configured {",
            "if browser_extensions_enabled {",
            1,
        );
        assert!(validate_constructor(&reversed_precedence).is_err());

        let dynamic_native_enablement = SAFE_WRY_SOURCE.replace(
            "set_are_browser_extensions_enabled(false)",
            "set_are_browser_extensions_enabled(pl_attrs.browser_extensions_enabled)",
        );
        assert!(validate_constructor(&dynamic_native_enablement).is_err());

        let setter = ["set_are_browser_extensions_", "enabled"].concat();
        let second_setter = SAFE_WRY_SOURCE.replace(
            "initialize_com();",
            &format!("initialize_com(); options.{setter} /* bypass */ (true);"),
        );
        assert!(validate_constructor(&second_setter).is_err());

        let loader = ["load_", "extensions"].concat();
        assert!(validate_constructor(&SAFE_WRY_SOURCE.replace("initialize_com", &loader)).is_err());
    }

    #[test]
    fn native_installation_has_one_owner_and_direct_enablement_stays_forbidden() {
        let install_token = ["AddBrowser", "Extension"].concat();
        let method_reference = format!("let install = Profile::{install_token};");
        assert!(validate_shipping_source(Path::new(NATIVE_OWNER), &method_reference).is_ok());
        assert!(
            validate_shipping_source(Path::new("crates/unowned.rs"), &method_reference).is_err()
        );
        assert!(validate_shipping_source(
            Path::new("crates/unowned.rs"),
            &format!("// forbidden token: {install_token}")
        )
        .is_err());

        let wry_method = ["with_browser_extensions_", "enabled"].concat();
        let environment_method = ["with_", "environment"].concat();
        let one_forward = format!("builder.{environment_method}(environment);");
        let dynamic_forwarding = format!(
            "builder.{wry_method}(webview_attributes.browser_extensions_enabled);{one_forward}"
        );
        assert!(validate_shipping_source(
            Path::new("vendor/tauri-runtime-wry/src/lib.rs"),
            &dynamic_forwarding
        )
        .is_ok());

        for reviewed in [
            "crates/zephium-engine/src/host/construction.rs",
            "vendor/tauri-runtime-wry/src/lib.rs",
        ] {
            assert!(validate_shipping_source(Path::new(reviewed), &one_forward).is_ok());
        }
        let two_forwards = format!("{one_forward}{one_forward}");
        assert!(validate_shipping_source(
            Path::new("vendor/tauri/src/webview/mod.rs"),
            &two_forwards
        )
        .is_ok());
        let three_forwards = format!("{two_forwards}{one_forward}");
        assert!(validate_shipping_source(
            Path::new("vendor/tauri/src/webview/webview_window.rs"),
            &three_forwards
        )
        .is_ok());
        assert!(validate_shipping_source(Path::new("crates/unreviewed.rs"), &one_forward).is_err());
        assert!(validate_shipping_source(
            Path::new("vendor/tauri/src/webview/webview_window.rs"),
            &one_forward
        )
        .is_err());
        assert_eq!(
            identifier_occurrences(
                "builder.with_environment_created_handler(handler);",
                &environment_method
            ),
            0
        );

        for creation_type in [
            ["CreateCoreWebView2Environment", "WithOptions"].concat(),
            ["CoreWebView2Environment", "Options"].concat(),
        ] {
            assert!(validate_shipping_source(
                Path::new("crates/unreviewed.rs"),
                &format!("let forbidden = {creation_type};")
            )
            .is_err());
        }

        let tauri_method = ["browser_extensions_", "enabled"].concat();
        let native_setter = ["set_are_browser_extensions_", "enabled"].concat();
        for direct in [
            format!("builder.{wry_method}(true);"),
            format!("builder.{tauri_method}(true);"),
            format!("options.{native_setter}(true);"),
            format!("attributes.{tauri_method} = true;"),
            format!("Attributes {{ {tauri_method}: true }}"),
        ] {
            assert!(validate_shipping_source(Path::new("crates/unfenced.rs"), &direct).is_err());
        }
    }

    #[test]
    fn native_owner_requires_exact_completion_identity_and_retained_lifecycle() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask has repository parent");
        let source = read_source(&repository.join(NATIVE_OWNER)).expect("native owner source");
        assert!(validate_native_owner(&source).is_ok());

        let dropped_completion = source.replacen(
            "let result = completion.and_then(|()| {",
            "let result = Ok(()).and_then(|()| {",
            1,
        );
        assert!(validate_native_owner(&dropped_completion).is_err());

        let dropped_identity = source.replacen(
            "ExtensionRuntimeNativeOwnerId::parse_exact(&owner)",
            "Ok(ExtensionRuntimeNativeOwnerId::from_encoded_bytes([b'a'; 32]).unwrap())",
            1,
        );
        assert!(validate_native_owner(&dropped_identity).is_err());

        let unbound_environment = source.replacen(
            "super::super::attest_environment(&environment, expected_user_data_folder)",
            "Ok::<(), ()>(())",
            1,
        );
        assert!(validate_native_owner(&unbound_environment).is_err());

        let leaked_drop = source.replacen(
            "retain_orphaned_lifecycle_owner(OrphanedNativeObject::Lifecycle(",
            "drop(OrphanedNativeObject::Lifecycle(",
            1,
        );
        assert!(validate_native_owner(&leaked_drop).is_err());

        let swallowed_quit = source.replacen(
            "unsafe { PostQuitMessage(message.wParam.0 as i32) };",
            "drop(message);",
            1,
        );
        assert!(validate_native_owner(&swallowed_quit).is_err());

        let unbounded_wait = source.replacen(
            "wait_for_native_callback_until(receiver, deadline)",
            "webview2_com::wait_with_pump(receiver)",
            1,
        );
        assert!(validate_native_owner(&unbounded_wait).is_err());

        let unreserved_callback = source.replacen(
            "let reservation = NativeCallbackReservation::acquire()?;",
            "let reservation = fake_unbounded_reservation();",
            1,
        );
        assert!(validate_native_owner(&unreserved_callback).is_err());

        let lost_pre_entry = source.replacen("if !native_call_entered {", "if false {", 1);
        assert!(validate_native_owner(&lost_pre_entry).is_err());

        let unbounded_owners = source.replacen(
            "let lifecycle = match NativeLifecycleReservation::acquire() {",
            "let lifecycle = fake_unbounded_lifecycle_owner(); match Ok(()) {",
            1,
        );
        assert!(validate_native_owner(&unbounded_owners).is_err());
    }

    #[test]
    fn source_collection_prunes_only_the_known_fuzz_artifact_root() {
        let temp = tempfile::tempdir().expect("temporary source root");
        let source_dir = temp.path().join("src");
        let tracked_target = source_dir.join("target");
        let artifact_target = temp
            .path()
            .join("crates/zephium-blocker/fuzz/target/generated");
        std::fs::create_dir_all(&tracked_target).expect("tracked target fixture");
        std::fs::create_dir_all(&artifact_target).expect("artifact target fixture");
        let source = source_dir.join("lib.rs");
        let tracked = tracked_target.join("tracked.rs");
        std::fs::write(&source, "fn admitted() {}").expect("source fixture");
        std::fs::write(&tracked, "fn tracked() {}").expect("tracked source fixture");
        std::fs::write(artifact_target.join("ignored.rs"), "fn ignored() {}")
            .expect("artifact source fixture");

        let mut collected = Vec::new();
        collect_rust_sources(temp.path(), &mut collected).expect("source collection");
        collected.sort();
        let mut expected = vec![source, tracked];
        expected.sort();
        assert_eq!(collected, expected);
    }

    #[cfg(unix)]
    #[test]
    fn source_collection_rejects_symlink_files_and_directories() {
        use std::os::unix::fs::symlink;

        let file_fixture = tempfile::tempdir().expect("temporary file fixture");
        let file = file_fixture.path().join("real.rs");
        std::fs::write(&file, "fn real() {}").expect("real source fixture");
        symlink(&file, file_fixture.path().join("linked.rs")).expect("source symlink");
        let mut collected = Vec::new();
        assert!(collect_rust_sources(file_fixture.path(), &mut collected)
            .unwrap_err()
            .contains("symlinks"));

        let directory_fixture = tempfile::tempdir().expect("temporary directory fixture");
        let directory = directory_fixture.path().join("real");
        std::fs::create_dir(&directory).expect("real directory fixture");
        symlink(&directory, directory_fixture.path().join("linked")).expect("directory symlink");
        let mut collected = Vec::new();
        assert!(
            collect_rust_sources(directory_fixture.path(), &mut collected)
                .unwrap_err()
                .contains("symlinks")
        );
    }
}
