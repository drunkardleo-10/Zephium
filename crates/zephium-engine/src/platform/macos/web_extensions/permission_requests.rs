//! Live semantics gate for extension-originated optional permission requests.
//!
//! This feature-only fixture deliberately exercises WebKit's own
//! `permissions.request()` path under an action user gesture. It records only
//! bounded permission names and native counts; no product prompt or durable
//! grant authority is implemented here.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::Instant;

use objc2::rc::Weak;
use objc2::runtime::ProtocolObject;
use objc2::MainThreadOnly;
use objc2_app_kit::{NSApplication, NSBackingStoreType, NSWindow, NSWindowStyleMask};
use objc2_foundation::{
    MainThreadMarker, NSDate, NSPoint, NSRect, NSRunLoop, NSSet, NSSize, NSString, NSURL,
};
use objc2_web_kit::{
    WKWebExtension, WKWebExtensionContext, WKWebExtensionController, WKWebExtensionMatchPattern,
    WKWebExtensionPermission, WKWebExtensionTab, WKWebExtensionWindow,
};
use serde_json::json;

const OPTIONAL_API_PERMISSION: &str = "clipboardWrite";
const OPTIONAL_HOST_PATTERN: &str = "https://optional.zephium.invalid/*";
const MAX_REQUEST_ENTRIES: usize = 8;
const INTERACTIVE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ResponsePolicy {
    Deny,
    ApiOnly,
    AllowAll,
}

impl ResponsePolicy {
    const fn identifier_suffix(self) -> &'static str {
        match self {
            Self::Deny => "deny",
            Self::ApiOnly => "api-only",
            Self::AllowAll => "allow-all",
        }
    }

    const fn expected_badge(self) -> &'static str {
        match self {
            Self::Deny | Self::ApiOnly => "DENY",
            Self::AllowAll => "ALLOW",
        }
    }
}

#[derive(Default)]
pub(super) struct PermissionRequestProbe {
    policy: Cell<Option<ResponsePolicy>>,
    context_identifier: RefCell<Option<String>>,
    permission_calls: Cell<usize>,
    pattern_calls: Cell<usize>,
    url_calls: Cell<usize>,
    tab_scoped_calls: Cell<usize>,
    permission_names: RefCell<Vec<String>>,
    pattern_names: RefCell<Vec<String>>,
    failure: RefCell<Option<String>>,
}

impl PermissionRequestProbe {
    pub(super) fn begin(&self, context: &WKWebExtensionContext, policy: ResponsePolicy) {
        self.policy.set(Some(policy));
        self.context_identifier
            .replace(Some(unsafe { context.uniqueIdentifier() }.to_string()));
        self.permission_calls.set(0);
        self.pattern_calls.set(0);
        self.url_calls.set(0);
        self.tab_scoped_calls.set(0);
        self.permission_names.borrow_mut().clear();
        self.pattern_names.borrow_mut().clear();
        self.failure.replace(None);
    }

    pub(super) fn complete_permissions(
        &self,
        permissions: &NSSet<WKWebExtensionPermission>,
        tab: Option<&ProtocolObject<dyn WKWebExtensionTab>>,
        context: &WKWebExtensionContext,
        completion: &block2::DynBlock<
            dyn Fn(NonNull<NSSet<WKWebExtensionPermission>>, *mut NSDate),
        >,
    ) {
        eprintln!("native-probe-runtime-permission: api-callback");
        self.observe_context(tab, context);
        self.permission_calls
            .set(self.permission_calls.get().saturating_add(1));
        let names = permission_names(permissions);
        if names.len() > MAX_REQUEST_ENTRIES {
            self.fail("permission request exceeded the probe entry bound");
        }
        self.permission_names.replace(names);

        if matches!(
            self.policy.get(),
            Some(ResponsePolicy::ApiOnly | ResponsePolicy::AllowAll)
        ) {
            completion.call((NonNull::from(permissions), std::ptr::null_mut()));
        } else {
            let empty = NSSet::<WKWebExtensionPermission>::new();
            completion.call((NonNull::from(&*empty), std::ptr::null_mut()));
        }
    }

    pub(super) fn complete_patterns(
        &self,
        patterns: &NSSet<WKWebExtensionMatchPattern>,
        tab: Option<&ProtocolObject<dyn WKWebExtensionTab>>,
        context: &WKWebExtensionContext,
        completion: &block2::DynBlock<
            dyn Fn(NonNull<NSSet<WKWebExtensionMatchPattern>>, *mut NSDate),
        >,
    ) {
        eprintln!("native-probe-runtime-permission: pattern-callback");
        self.observe_context(tab, context);
        self.pattern_calls
            .set(self.pattern_calls.get().saturating_add(1));
        let names = pattern_names(patterns);
        if names.len() > MAX_REQUEST_ENTRIES {
            self.fail("host-pattern request exceeded the probe entry bound");
        }
        self.pattern_names.replace(names);

        if self.policy.get() == Some(ResponsePolicy::AllowAll) {
            completion.call((NonNull::from(patterns), std::ptr::null_mut()));
        } else {
            let empty = NSSet::<WKWebExtensionMatchPattern>::new();
            completion.call((NonNull::from(&*empty), std::ptr::null_mut()));
        }
    }

    pub(super) fn reject_urls(
        &self,
        _urls: &NSSet<NSURL>,
        tab: Option<&ProtocolObject<dyn WKWebExtensionTab>>,
        context: &WKWebExtensionContext,
        completion: &block2::DynBlock<dyn Fn(NonNull<NSSet<NSURL>>, *mut NSDate)>,
    ) {
        self.observe_context(tab, context);
        self.url_calls.set(self.url_calls.get().saturating_add(1));
        let empty = NSSet::<NSURL>::new();
        completion.call((NonNull::from(&*empty), std::ptr::null_mut()));
    }

    fn observe_context(
        &self,
        tab: Option<&ProtocolObject<dyn WKWebExtensionTab>>,
        context: &WKWebExtensionContext,
    ) {
        if tab.is_some() {
            self.tab_scoped_calls
                .set(self.tab_scoped_calls.get().saturating_add(1));
        }
        let actual = unsafe { context.uniqueIdentifier() }.to_string();
        if self.context_identifier.borrow().as_deref() != Some(actual.as_str()) {
            self.fail("permission callback crossed its bound extension context");
        }
    }

    fn fail(&self, message: &str) {
        if self.failure.borrow().is_none() {
            self.failure.replace(Some(message.to_owned()));
        }
    }

    fn validate_callbacks(&self, policy: ResponsePolicy) -> Result<(), String> {
        if let Some(failure) = self.failure.borrow().as_deref() {
            return Err(failure.to_owned());
        }
        let identity_matches = self.policy.get() == Some(policy);
        let counts_match = self.permission_calls.get() == 1
            && self.pattern_calls.get() == 1
            && self.url_calls.get() == 0
            // Requests from an extension page are context-scoped. WebKit
            // supplies `nil` for the tab even though the host has an active
            // tab; product authority must never be inferred from tab presence.
            && self.tab_scoped_calls.get() == 0;
        let permission_matches =
            self.permission_names.borrow().as_slice() == [OPTIONAL_API_PERMISSION];
        let pattern_matches = self.pattern_names.borrow().as_slice() == [OPTIONAL_HOST_PATTERN];
        if !identity_matches || !counts_match || !permission_matches || !pattern_matches {
            return Err(format!(
                "optional-permission callback drift: identity_matches={identity_matches}, counts_match={counts_match}, permission_matches={permission_matches}, pattern_matches={pattern_matches}, policy={:?}, permission_calls={}, pattern_calls={}, url_calls={}, tab_scoped_calls={}, permissions={:?}, patterns={:?}",
                self.policy.get(),
                self.permission_calls.get(),
                self.pattern_calls.get(),
                self.url_calls.get(),
                self.tab_scoped_calls.get(),
                self.permission_names.borrow(),
                self.pattern_names.borrow(),
            ));
        }
        Ok(())
    }
}

pub(super) struct PermissionRequestEvidence {
    pub(super) contexts: Vec<Weak<WKWebExtensionContext>>,
    pub(super) extension_views: Vec<Weak<objc2_web_kit::WKWebView>>,
    pub(super) readback: String,
}

pub(super) fn write_fixture(path: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir(path)
        .map_err(|error| format!("cannot create optional-permission fixture: {error}"))?;
    let manifest = json!({
        "manifest_version": 3,
        "name": "Zephium Optional Permission Probe",
        "description": "Feature-gated runtime permission semantics fixture.",
        "version": "1.0.0",
        "action": {
            "default_title": "Optional permission probe",
            "default_popup": "request.html"
        },
        "optional_permissions": [OPTIONAL_API_PERMISSION],
        "optional_host_permissions": [OPTIONAL_HOST_PATTERN]
    });
    std::fs::write(path.join("manifest.json"), manifest.to_string())
        .map_err(|error| format!("cannot write optional-permission manifest: {error}"))?;
    std::fs::write(
        path.join("request.html"),
        "<!doctype html><meta charset=\"utf-8\"><title>Permission request</title><style>html,body,button{box-sizing:border-box;width:100%;height:100%;margin:0}</style><button id=\"request\" type=\"button\" autofocus>Request</button><script src=\"request.js\"></script>",
    )
    .map_err(|error| format!("cannot write optional-permission popup: {error}"))?;
    std::fs::write(
        path.join("request.js"),
        format!(
            r#"const button = document.getElementById('request');
            button.focus();
            button.addEventListener('pointerdown', () => {{ document.title = 'DOWN'; }}, {{ once: true }});
            button.addEventListener('click', async () => {{
                'use strict';
                document.title = 'CLICK';
                try {{
                    const request = {{
                        permissions: [{api:?}],
                        origins: [{host:?}]
                    }};
                    const granted = await browser.permissions.request(request);
                    const contains = await browser.permissions.contains(request);
                    const badge = granted && contains ? 'ALLOW' :
                        (!granted && !contains ? 'DENY' : 'MISM');
                    document.title = badge;
                    await browser.action.setBadgeText({{ text: badge }});
                }} catch (error) {{
                    document.title = 'ERR:' + String(error?.message || error);
                    await browser.action.setBadgeText({{ text: 'ERR' }});
                }}
            }}, {{ once: true }});
            browser.action.setBadgeText({{ text: 'READY' }});"#,
            api = OPTIONAL_API_PERMISSION,
            host = OPTIONAL_HOST_PATTERN,
        ),
    )
    .map_err(|error| format!("cannot write optional-permission popup script: {error}"))
}

pub(super) fn validate_declaration(extension: &WKWebExtension) -> Result<(), String> {
    let errors = unsafe { extension.errors() };
    if errors.count() != 0 {
        return Err(format!(
            "optional-permission fixture parsed with {} error(s): {}",
            errors.count(),
            super::describe_native_errors(&errors),
        ));
    }
    if unsafe { extension.manifestVersion() } != 3.0 {
        return Err("optional-permission fixture was not parsed as MV3".into());
    }
    let optional_permissions = unsafe { extension.optionalPermissions() };
    let optional_patterns = unsafe { extension.optionalPermissionMatchPatterns() };
    let permissions = permission_names(&optional_permissions);
    let patterns = pattern_names(&optional_patterns);
    if permissions.as_slice() != [OPTIONAL_API_PERMISSION]
        || patterns.as_slice() != [OPTIONAL_HOST_PATTERN]
    {
        return Err(format!(
            "optional-permission declaration drift: permissions={permissions:?}, patterns={patterns:?}"
        ));
    }
    Ok(())
}

pub(super) fn run(
    extension: &WKWebExtension,
    controller: &WKWebExtensionController,
    window: &ProtocolObject<dyn WKWebExtensionWindow>,
    tab: &ProtocolObject<dyn WKWebExtensionTab>,
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
    state: &Rc<PermissionRequestProbe>,
) -> Result<PermissionRequestEvidence, String> {
    validate_declaration(extension)?;
    let mut contexts = Vec::with_capacity(3);
    let mut extension_views = Vec::with_capacity(3);
    let mut readbacks = Vec::with_capacity(3);

    for policy in [
        ResponsePolicy::Deny,
        ResponsePolicy::ApiOnly,
        ResponsePolicy::AllowAll,
    ] {
        let identifier = format!(
            "zephium-probe-runtime-permission-{}",
            policy.identifier_suffix()
        );
        let context = super::new_context(extension, &identifier)?;
        state.begin(&context, policy);
        super::load_context(controller, &context, &identifier)?;
        super::assert_context_surface(
            &context,
            window,
            tab,
            true,
            &format!("runtime permission {} surface", policy.identifier_suffix()),
        )?;
        let action = unsafe { context.actionForTab(Some(tab)) }.ok_or_else(|| {
            format!(
                "runtime permission {} context exposed no action",
                policy.identifier_suffix()
            )
        })?;
        if unsafe { action.badgeText() }.length() != 0
            || !unsafe { action.isEnabled() }
            || !unsafe { action.presentsPopup() }
        {
            return Err(format!(
                "runtime permission {} action did not start enabled with an empty-badge popup",
                policy.identifier_suffix()
            ));
        }

        let configuration = unsafe { context.webViewConfiguration() }.ok_or_else(|| {
            "runtime permission context omitted its extension-page configuration".to_owned()
        })?;
        let window = interactive_window(mtm)?;
        let host =
            super::profile_isolation::host_for_window(&window, "Zephium runtime permission probe")?;
        let page = super::profile_isolation::build_profile_view(&host, configuration)?;
        let native_page = crate::platform::macos::native_webview(&page);
        let request_url = unsafe { context.baseURL() }
            .URLByAppendingPathComponent(&NSString::from_str("request.html"))
            .and_then(|url| url.absoluteString())
            .ok_or_else(|| "runtime permission context produced no request URL".to_owned())?
            .to_string();
        page.load_url(&request_url)
            .map_err(|error| format!("cannot load runtime permission extension page: {error}"))?;
        NSApplication::sharedApplication(mtm).activate();
        window.makeKeyAndOrderFront(None);
        wait_for_badge(
            &context,
            &action,
            &native_page,
            "READY",
            run_loop,
            super::PROBE_TIMEOUT,
        )?;
        // Loading can replace WebKit's responder chain. Establish foreground
        // and keyboard authority only after the extension page has completed
        // its bootstrap so the manual gate cannot accidentally target a
        // provisional document.
        NSApplication::sharedApplication(mtm).activate();
        window.makeKeyAndOrderFront(None);
        if !window.makeFirstResponder(Some(&native_page)) {
            return Err(
                "runtime permission extension page could not become first responder".into(),
            );
        }
        extension_views.push(Weak::from_retained(&native_page));
        eprintln!(
            "native-probe-runtime-permission: click Request for policy={}",
            policy.identifier_suffix()
        );
        wait_for_badge(
            &context,
            &action,
            &native_page,
            policy.expected_badge(),
            run_loop,
            INTERACTIVE_TIMEOUT,
        )?;
        state.validate_callbacks(policy)?;
        readbacks.push(validate_readback(&context, policy)?);
        super::validate_context_errors(&context, "runtime permission probe")?;

        drop(native_page);
        drop(page);
        window.close();
        drop(window);
        super::unload_context(controller, &context, &identifier)?;
        crate::platform::macos::extensions::clear_all_probe_grants(&context).map_err(|error| {
            format!(
                "runtime permission {} cleanup failed: {error}",
                policy.identifier_suffix()
            )
        })?;
        contexts.push(Weak::from_retained(&context));
        drop(action);
        drop(context);
        super::drain_run_loop_once(run_loop);
    }

    Ok(PermissionRequestEvidence {
        contexts,
        extension_views,
        readback: readbacks.join(","),
    })
}

fn interactive_window(mtm: MainThreadMarker) -> Result<objc2::rc::Retained<NSWindow>, String> {
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(120.0, 120.0), NSSize::new(420.0, 220.0)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe {
        window.setReleasedWhenClosed(false);
        window.setTitle(&NSString::from_str("Zephium Optional Permission Probe"));
    }
    Ok(window)
}

fn wait_for_badge(
    context: &WKWebExtensionContext,
    action: &objc2_web_kit::WKWebExtensionAction,
    extension_page: &objc2_web_kit::WKWebView,
    expected: &str,
    run_loop: &NSRunLoop,
    timeout: std::time::Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        let badge = unsafe { action.badgeText() }.to_string();
        if badge == expected {
            return Ok(());
        }
        if badge == "ERR" || badge == "MISM" {
            let extension_page_title =
                unsafe { extension_page.title() }.map(|title| title.to_string());
            return Err(format!(
                "runtime permission JavaScript reported {badge} instead of {expected}: extension_page_title={extension_page_title:?}"
            ));
        }
        super::validate_context_errors(context, "runtime permission request")?;
        if Instant::now() >= deadline {
            let extension_page_title =
                unsafe { extension_page.title() }.map(|title| title.to_string());
            return Err(format!(
                "runtime permission request did not settle: expected={expected}, badge={badge:?}, extension_page_title={extension_page_title:?}"
            ));
        }
        super::drain_run_loop_once(run_loop);
    }
}

fn validate_readback(
    context: &WKWebExtensionContext,
    policy: ResponsePolicy,
) -> Result<String, String> {
    let granted_permissions = unsafe { context.grantedPermissions() };
    let denied_permissions = unsafe { context.deniedPermissions() };
    let granted_patterns = unsafe { context.grantedPermissionMatchPatterns() };
    let denied_patterns = unsafe { context.deniedPermissionMatchPatterns() };
    let granted_permission_names = granted_permissions
        .allKeys()
        .iter()
        .map(|permission| permission.to_string())
        .collect::<Vec<_>>();
    let granted_pattern_names = granted_patterns
        .allKeys()
        .iter()
        .map(|pattern| unsafe { pattern.string() }.to_string())
        .collect::<Vec<_>>();
    // WebKit settles one JavaScript request atomically across the separate
    // permission and match-pattern delegate callbacks. A partially approved
    // response returns `false` and retains neither subset.
    let expected_api = usize::from(policy == ResponsePolicy::AllowAll);
    let expected_host = usize::from(policy == ResponsePolicy::AllowAll);
    if granted_permission_names.len() != expected_api
        || granted_pattern_names.len() != expected_host
        || denied_permissions.count() != 0
        || denied_patterns.count() != 0
        || (expected_api == 1 && granted_permission_names != [OPTIONAL_API_PERMISSION])
        || (expected_host == 1 && granted_pattern_names != [OPTIONAL_HOST_PATTERN])
    {
        return Err(format!(
            "runtime permission native grant readback drift: policy={policy:?}, granted_permissions={granted_permission_names:?}, granted_patterns={granted_pattern_names:?}, denied_permissions={}, denied_patterns={}",
            denied_permissions.count(),
            denied_patterns.count(),
        ));
    }
    Ok(format!(
        "{}:gp{}-dp{}-gh{}-dh{}",
        policy.identifier_suffix(),
        granted_permissions.count(),
        denied_permissions.count(),
        granted_patterns.count(),
        denied_patterns.count(),
    ))
}

fn permission_names(permissions: &NSSet<WKWebExtensionPermission>) -> Vec<String> {
    let objects = permissions.allObjects();
    let mut names = (0..objects.count())
        .map(|index| objects.objectAtIndex(index).to_string())
        .collect::<Vec<_>>();
    names.sort_unstable();
    names
}

fn pattern_names(patterns: &NSSet<WKWebExtensionMatchPattern>) -> Vec<String> {
    let objects = patterns.allObjects();
    let mut names = (0..objects.count())
        .map(|index| unsafe { objects.objectAtIndex(index).string() }.to_string())
        .collect::<Vec<_>>();
    names.sort_unstable();
    names
}
