//! Native-only proof that an admitted top-frame redirect is visible to one
//! exact extension tab before a synthetic OAuth callback can commit.

use super::*;
use crate::navigation_epoch::NavigationEpochTracker;
use crate::platform::macos::{
    ControllerBrowserRequestSettlement, ControllerSurfaceApplication, PersistentControllerRegistry,
    ProbeControllerPreparation,
};
use objc2_foundation::NSNumber;
use zephium_core::extensions::{
    ExtensionBrowserRequestAction, ExtensionBrowserRequestResult,
    ExtensionBrowserRequestSettlement, ExtensionBrowserSurface, ExtensionBrowserSurfaceGeneration,
    ExtensionBrowserTab, ExtensionBrowserWindow,
};
use zephium_core::ids::{ItemId, ProfileId};

const PROFILE: u128 = 0xe9d9_77b2_e813_49d4_bf40_3c56_e7af_1d20;
const WINDOW: u64 = 739;
const TAB: u128 = 740;
const PRINCIPAL: &str = "cccccccccccccccccccccccccccccccc";

pub(super) fn run() -> Result<bool, String> {
    let Some(_) = supported_runtime()? else {
        return Ok(false);
    };
    let watchdog = arm_process_watchdog();
    let result = run_inner(false, false, false, None, false, true);
    watchdog.store(true, Ordering::Release);
    result.map(|()| true)
}

pub(super) fn run_immediate() -> Result<bool, String> {
    let Some(_) = supported_runtime()? else {
        return Ok(false);
    };
    let watchdog = arm_process_watchdog();
    let result = run_inner(true, false, false, None, false, true);
    watchdog.store(true, Ordering::Release);
    result.map(|()| true)
}

pub(super) fn run_clear_immediately() -> Result<bool, String> {
    let Some(_) = supported_runtime()? else {
        return Ok(false);
    };
    let watchdog = arm_process_watchdog();
    let result = run_inner(false, true, false, None, false, true);
    watchdog.store(true, Ordering::Release);
    result.map(|()| true)
}

pub(super) fn run_same_turn_nonresident() -> Result<bool, String> {
    let Some(_) = supported_runtime()? else {
        return Ok(false);
    };
    let watchdog = arm_process_watchdog();
    let result = run_inner(false, false, true, None, false, true);
    watchdog.store(true, Ordering::Release);
    result.map(|()| true)
}

pub(super) fn run_broker_order(settle_first: bool) -> Result<bool, String> {
    let Some(_) = supported_runtime()? else {
        return Ok(false);
    };
    let watchdog = arm_process_watchdog();
    let result = run_inner(false, false, false, Some(settle_first), false, true);
    watchdog.store(true, Ordering::Release);
    result.map(|()| true)
}

pub(super) fn run_worker_json_post() -> Result<bool, String> {
    let Some(_) = supported_runtime()? else {
        return Ok(false);
    };
    let watchdog = arm_process_watchdog();
    let result = run_inner(false, false, false, None, true, true)
        .and_then(|()| run_inner(false, false, false, None, true, false));
    watchdog.store(true, Ordering::Release);
    result.map(|()| true)
}

fn write_extension(root: &Path, broker_order: bool, worker_post: bool) -> Result<(), String> {
    let manifest = json!({
        "manifest_version": 3,
        "name": "Synthetic OAuth redirect observation",
        "version": "1.0",
        "permissions": ["storage", "tabs"],
        "host_permissions": ["http://127.0.0.1/*", "https://synthetic-callback.invalid/*"],
        "background": {"service_worker": "worker.js"},
        "action": {"default_popup": "probe.html"}
    });
    write_fixture_file(root, "manifest.json", &manifest.to_string())?;
    let worker = if worker_post {
        r#"
        chrome.runtime.onMessage.addListener((message, _sender, respond) => {
          if (message?.kind !== 'worker-post-probe') return;
          respond({started: true});
          (async () => {
            const results = [];
            for (const [name, url] of [['cors', message.cors], ['no-cors-header', message.noCorsHeader]]) {
              try {
                const response = await fetch(url, {
                  method: 'POST', mode: 'cors', credentials: 'include',
                  headers: {'Content-Type': 'application/json'}, body: '{"fixed":true}'
                });
                const body = await response.json();
                results.push({name, state: 'http', status: response.status,
                  parsed: body?.fixed === true});
              } catch (error) {
                results.push({name, state: 'rejected', error: String(error?.name ?? 'unknown').slice(0, 32)});
              }
            }
            chrome.storage.local.set({zephiumOAuthRedirectObservation: {state:'post', results}}, () => {});
          })();
          return true;
        });
    "#
    } else if broker_order {
        r#"
        let started = false;
        chrome.runtime.onMessage.addListener((message, _sender, respond) => {
          if (message?.kind !== 'start-oauth-probe' || started) return;
          started = true;
          respond({started: true});
          (async () => {
            try {
              const created = await new Promise((resolve, reject) => {
                chrome.tabs.create({url: message.url, active: true}, (tab) => {
                  const error = chrome.runtime.lastError;
                  if (error) reject(new Error(String(error.message ?? error)));
                  else resolve(tab);
                });
              });
              let count = 0;
              let authTabStatus = 'open';
              const redirectPrefix = 'https://synthetic-callback.invalid';
              const listenerMarker = chrome.storage.local.set({zephiumOAuthRedirectObservation: {
                state: 'listener', count: 0, tabId: created.id
              }});
              chrome.tabs.onUpdated.addListener(async (tabId, changeInfo) => {
                const tabMatch = tabId === created.id;
                const urlPresent = !!changeInfo?.url;
                const prefixMatch = urlPresent && changeInfo.url.startsWith(redirectPrefix);
                const cleaningUpBefore = authTabStatus === 'cleaning-up';
                if (cleaningUpBefore || !tabMatch || !urlPresent || !prefixMatch) return;
                const callback = new URL(changeInfo.url);
                const codePresent = callback.searchParams.has('code');
                const statePresent = callback.searchParams.has('state');
                authTabStatus = 'cleaning-up';
                count += 1;
                await listenerMarker;
                await chrome.storage.local.set({zephiumOAuthRedirectObservation: {
                  state: 'event', count, tabId, url: changeInfo.url,
                  exactActiveTab: tabMatch, urlPresent, prefixMatch,
                  cleaningUpBefore, codePresent, statePresent
                }});
              });
              await listenerMarker;
            } catch (error) {
              await chrome.storage.local.set({zephiumOAuthRedirectObservation: {
                state: 'error', detail: String(error?.name ?? error).slice(0, 48)
              }});
            }
          })();
          return true;
        });
    "#
    } else {
        r#"
        let count = 0;
        const observed = new Set();
        chrome.tabs.onUpdated.addListener(async (tabId, changeInfo) => {
          if (typeof changeInfo?.url !== 'string' ||
              !changeInfo.url.includes('/oauth-callback?code=fixed-synthetic')) return;
          if (observed.has(changeInfo.url)) return;
          observed.add(changeInfo.url);
          count += 1;
          const active = await chrome.tabs.query({active:true,currentWindow:true});
          await chrome.storage.local.set({zephiumOAuthRedirectObservation: {
            count, tabId, url: changeInfo.url,
            exactActiveTab: active.length === 1 && active[0].id === tabId
          }});
        });
    "#
    };
    write_fixture_file(root, "worker.js", worker)?;
    write_fixture_file(
        root,
        "probe.html",
        "<!doctype html><meta charset=utf-8><title>OAuth probe</title>",
    )
}

fn surface(
    profile: ProfileId,
    tab: ItemId,
    generation: u64,
    url: &str,
    resident: bool,
) -> Result<ExtensionBrowserSurface, String> {
    let url = url::Url::parse(url).map_err(|error| error.to_string())?;
    ExtensionBrowserSurface::new(
        profile,
        ExtensionBrowserSurfaceGeneration::new(generation).ok_or("invalid probe generation")?,
        Some(WINDOW),
        vec![ExtensionBrowserWindow::new(
            WINDOW,
            false,
            Some(tab),
            vec![ExtensionBrowserTab::from_snapshot(
                None,
                tab,
                resident,
                "Synthetic OAuth tab",
                Some(&url),
                false,
                false,
            )
            .map_err(|error| format!("invalid probe tab: {error:?}"))?],
        )
        .map_err(|error| format!("invalid probe window: {error:?}"))?],
    )
    .map_err(|error| format!("invalid probe surface: {error:?}"))
}

fn empty_surface(profile: ProfileId, generation: u64) -> ExtensionBrowserSurface {
    ExtensionBrowserSurface::new(
        profile,
        ExtensionBrowserSurfaceGeneration::new(generation).expect("static generation"),
        None,
        Vec::new(),
    )
    .expect("static empty surface")
}

fn created_surface(
    profile: ProfileId,
    original: ItemId,
    created: ItemId,
    base: &str,
    target: &str,
) -> Result<ExtensionBrowserSurface, String> {
    let base = url::Url::parse(base).map_err(|error| error.to_string())?;
    let target = url::Url::parse(target).map_err(|error| error.to_string())?;
    let original = ExtensionBrowserTab::from_snapshot(
        None,
        original,
        false,
        "OAuth starter",
        Some(&base),
        false,
        false,
    )
    .map_err(|error| format!("invalid starter tab: {error:?}"))?;
    let created_tab = ExtensionBrowserTab::from_snapshot(
        None,
        created,
        true,
        "OAuth created tab",
        Some(&target),
        true,
        false,
    )
    .map_err(|error| format!("invalid created tab: {error:?}"))?;
    ExtensionBrowserSurface::new(
        profile,
        ExtensionBrowserSurfaceGeneration::new(2).expect("static generation"),
        Some(WINDOW),
        vec![ExtensionBrowserWindow::new(
            WINDOW,
            false,
            Some(created),
            vec![original, created_tab],
        )
        .map_err(|error| format!("invalid OAuth creation window: {error:?}"))?],
    )
    .map_err(|error| format!("invalid OAuth creation surface: {error:?}"))
}

fn poll_observation(
    view: &WKWebView,
    run_loop: &NSRunLoop,
    deadline: Instant,
) -> Result<Option<Value>, String> {
    let outcome = Rc::new(RefCell::new(None));
    let slot = outcome.clone();
    let callback = block2::RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
        *slot.borrow_mut() = Some(if let Some(error) = unsafe { error.as_ref() } {
            Err(format!(
                "probe page JavaScript failed: domain={} code={}",
                error.domain(),
                error.code()
            ))
        } else {
            unsafe { value.as_ref() }
                .and_then(AnyObject::downcast_ref::<NSString>)
                .map(|value| value.to_string())
                .ok_or_else(|| "probe page JavaScript returned no string".to_owned())
        });
    });
    let world = unsafe {
        objc2_web_kit::WKContentWorld::pageWorld(MainThreadMarker::new().expect("main thread"))
    };
    unsafe {
        view.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
        &NSString::from_str("return JSON.stringify((await chrome.storage.local.get('zephiumOAuthRedirectObservation')).zephiumOAuthRedirectObservation ?? null)"),
        None, None, &world, Some(&callback),
    )
    };
    while outcome.borrow().is_none() && Instant::now() < deadline {
        drain_run_loop_once(run_loop);
    }
    let captured = outcome.borrow_mut().take();
    captured
        .map(|json| {
            json.and_then(|json| serde_json::from_str(&json).map_err(|error| error.to_string()))
        })
        .transpose()
}

fn wait_for_observation(
    view: &WKWebView,
    run_loop: &NSRunLoop,
    minimum_count: u64,
) -> Result<Value, String> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut last = None;
    while Instant::now() < deadline {
        if let Some(value) = poll_observation(view, run_loop, deadline)? {
            if value["count"]
                .as_u64()
                .is_some_and(|count| count >= minimum_count)
            {
                return Ok(value);
            }
            last = Some(value);
        }
        drain_run_loop_once(run_loop);
    }
    Err(format!("OAuth worker URL event not observed: {last:?}"))
}

fn trigger_broker_create(view: &WKWebView, url: &str, run_loop: &NSRunLoop) -> Result<(), String> {
    trigger_worker_request(
        view,
        json!({"kind":"start-oauth-probe","url":url}),
        run_loop,
    )
}

fn trigger_worker_request(
    view: &WKWebView,
    request: Value,
    run_loop: &NSRunLoop,
) -> Result<(), String> {
    let script = format!("return JSON.stringify(await chrome.runtime.sendMessage({request}))");
    let result = Rc::new(RefCell::new(None));
    let slot = result.clone();
    let callback = block2::RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
        *slot.borrow_mut() = Some(if error.is_null() {
            unsafe { value.as_ref() }
                .and_then(AnyObject::downcast_ref::<NSString>)
                .map(|value| value.to_string())
        } else {
            None
        });
    });
    let world = unsafe {
        objc2_web_kit::WKContentWorld::pageWorld(MainThreadMarker::new().expect("main thread"))
    };
    unsafe {
        view.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
            &NSString::from_str(&script),
            None,
            None,
            &world,
            Some(&callback),
        )
    };
    let deadline = Instant::now() + PROBE_TIMEOUT;
    while result.borrow().is_none() && Instant::now() < deadline {
        drain_run_loop_once(run_loop);
    }
    let actual = result
        .borrow_mut()
        .take()
        .flatten()
        .ok_or("worker create trigger did not respond")?;
    if serde_json::from_str::<Value>(&actual).map_err(|error| error.to_string())?["started"] != true
    {
        return Err(format!("worker refused broker create trigger: {actual}"));
    }
    Ok(())
}

fn wait_for_post_observation(view: &WKWebView, run_loop: &NSRunLoop) -> Result<Value, String> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(value) = poll_observation(view, run_loop, deadline)? {
            if value["state"] == "post" {
                return Ok(value);
            }
        }
        drain_run_loop_once(run_loop);
    }
    Err("worker JSON POST observation timed out".into())
}

fn wait_for_listener(view: &WKWebView, run_loop: &NSRunLoop) -> Result<Value, String> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut last = None;
    while Instant::now() < deadline {
        if let Some(value) = poll_observation(view, run_loop, deadline)? {
            if value["state"] == "listener" || value["state"] == "event" {
                return Ok(value);
            }
            if value["state"] == "error" {
                return Err(format!("worker could not create synthetic tab: {value}"));
            }
            last = Some(value);
        }
        drain_run_loop_once(run_loop);
    }
    Err(format!(
        "worker did not register post-await listener: {last:?}"
    ))
}

fn wait_for_commit(
    tracker: &NavigationEpochTracker,
    expected: &str,
    run_loop: &NSRunLoop,
) -> Result<(), String> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    while Instant::now() < deadline {
        if tracker
            .committed_snapshot()
            .is_some_and(|(_, url)| url == expected)
        {
            return Ok(());
        }
        drain_run_loop_once(run_loop);
    }
    Err(format!(
        "initial OAuth fixture page did not commit: {:?}",
        tracker.committed_snapshot()
    ))
}

fn wait_for_failure(
    events: &Rc<RefCell<Vec<wry::NavigationEvent>>>,
    after: usize,
    run_loop: &NSRunLoop,
) -> Result<(), String> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    while Instant::now() < deadline {
        if events
            .borrow()
            .iter()
            .skip(after)
            .any(|event| matches!(event.phase, wry::NavigationEventPhase::Failed))
        {
            return Ok(());
        }
        drain_run_loop_once(run_loop);
    }
    Err("synthetic OAuth callback did not fail provisionally".into())
}

fn run_inner(
    immediate_failure: bool,
    immediate_clear: bool,
    same_turn_nonresident: bool,
    broker_order: Option<bool>,
    worker_post: bool,
    grant_host: bool,
) -> Result<(), String> {
    use super::super::extensions::MacosNativeApiPermission as Permission;
    let mtm = MainThreadMarker::new().ok_or("OAuth redirect probe requires main thread")?;
    let app = NSApplication::sharedApplication(mtm);
    let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.finishLaunching();
    let run_loop = NSRunLoop::mainRunLoop();
    let server = FixtureServer::start(None)?;
    let fixture = tempfile::Builder::new()
        .prefix("zephium-oauth-redirect-")
        .tempdir()
        .map_err(|error| error.to_string())?;
    write_extension(fixture.path(), broker_order.is_some(), worker_post)?;
    let profile = ProfileId::from(PROFILE);
    let tab_id = ItemId::from(TAB);
    let current_tab = Rc::new(Cell::new(tab_id));
    let (request_tx, request_rx) =
        std::sync::mpsc::sync_channel::<zephium_core::ports::engine::EngineEvent>(8);
    let registry = Rc::new(RefCell::new(if broker_order.is_some() {
        PersistentControllerRegistry::with_browser_request_sink(Arc::new(move |ingress| {
            let _ = request_tx.try_send(ingress.event);
        }))
    } else {
        PersistentControllerRegistry::new()
    }));
    match registry
        .borrow_mut()
        .prepare_for_native_probe(profile)
        .map_err(|error| format!("OAuth controller preparation failed: {error}"))?
    {
        ProbeControllerPreparation::Prepared => {}
        ProbeControllerPreparation::RuntimeUnavailable => {
            return Err("OAuth controller unavailable".into())
        }
    }
    let prepared = registry
        .borrow_mut()
        .configuration_for_durable_profile(profile)
        .map_err(|error| format!("OAuth profile configuration failed: {error}"))?
        .ok_or("OAuth controller has no view configuration")?;
    let (configuration, proof) = prepared.into_parts();
    let controller = unsafe { configuration.webExtensionController() }
        .ok_or("OAuth profile configuration lacks controller")?;
    let store = unsafe { configuration.websiteDataStore() };
    let extension = load_extension(fixture.path(), &run_loop, mtm)?;
    let context = new_context(&extension, PRINCIPAL)?;
    let host_grants: &[&str] = if worker_post && !grant_host {
        &[]
    } else {
        &[HOST_MATCH_PATTERN, "https://synthetic-callback.invalid/*"]
    };
    let grants = super::super::extensions::apply_probe_grants(
        &context,
        &[Permission::Storage, Permission::Tabs],
        host_grants,
        false,
    )
    .map_err(|error| format!("OAuth probe grants failed: {error}"))?;
    let window = new_window(mtm)?;
    let host = profile_isolation::host_for_window(&window, "OAuth redirect")?;
    let tracker = NavigationEpochTracker::new();
    let events: Rc<RefCell<Vec<wry::NavigationEvent>>> = Rc::new(RefCell::new(Vec::new()));
    let attempts = Rc::new(RefCell::new(Vec::<String>::new()));
    let errors = Rc::new(RefCell::new(Vec::<String>::new()));
    let bound_view: Rc<RefCell<Option<Weak<WKWebView>>>> = Rc::new(RefCell::new(None));
    let surface_ready = Rc::new(Cell::new(false));
    let attempt_registry = registry.clone();
    let attempt_view = bound_view.clone();
    let attempt_ready = surface_ready.clone();
    let attempt_log = attempts.clone();
    let attempt_errors = errors.clone();
    let attempt_tab = current_tab.clone();
    let event_registry = registry.clone();
    let event_view = bound_view.clone();
    let event_ready = surface_ready.clone();
    let event_log = events.clone();
    let event_tracker = tracker.clone();
    let event_tab = current_tab.clone();
    let base_url = if broker_order.is_some() {
        "about:blank".to_owned()
    } else {
        server.url("/oauth-base", "fixed")
    };
    let base_for_retire = base_url.clone();
    let mut builder = wry::WebViewBuilder::new()
        .with_webview_configuration(configuration)
        .with_navigation_handler(|url| {
            zephium_core::navigation::is_allowed_str(&url) && !url.contains("/oauth-denied")
        })
        .with_main_frame_navigation_attempt_handler(move |target| {
            if !attempt_ready.get() { return; }
            let Some(view) = attempt_view.borrow().as_ref().and_then(Weak::load) else { return; };
            attempt_log.borrow_mut().push(target.clone());
            match attempt_registry.try_borrow_mut() {
                Ok(mut registry) => match registry.observe_browser_tab_url_attempt(profile, attempt_tab.get(), &view, &target) {
                    Ok(true) => {
                        if immediate_clear {
                            registry.clear_browser_tab_url_attempt(profile, attempt_tab.get(), &view);
                        } else if same_turn_nonresident && target.contains("/oauth-callback?code=fixed-synthetic") {
                            if let Err(error) = registry.bind_browser_surface_view(profile, tab_id, None) {
                                attempt_errors.borrow_mut().push(format!("same-turn physical unbind failed: {error}"));
                            } else {
                                match surface(profile, tab_id, 2, &base_for_retire, false)
                                    .and_then(|surface| registry.apply_browser_surface(&surface, |_| None)
                                        .map_err(|error| format!("same-turn nonresident surface failed: {error}"))) {
                                    Ok(ControllerSurfaceApplication::Applied) => {}
                                    result => attempt_errors.borrow_mut().push(format!("same-turn nonresident replacement refused: {result:?}")),
                                }
                            }
                        }
                    }
                    result => attempt_errors.borrow_mut().push(format!("pending URL refused: {result:?}")),
                },
                Err(_) => attempt_errors.borrow_mut().push("registry reentered during URL attempt".into()),
            }
        })
        .with_navigation_event_handler(move |event| {
            event_tracker.observe_navigation(&event);
            if event_ready.get() && (event.phase == wry::NavigationEventPhase::Committed
                || (!same_turn_nonresident && broker_order.is_none() && matches!(event.phase,
                    wry::NavigationEventPhase::Failed | wry::NavigationEventPhase::Cancelled)))
            {
                if let Some(view) = event_view.borrow().as_ref().and_then(Weak::load) {
                    if let Ok(mut registry) = event_registry.try_borrow_mut() {
                        registry.clear_browser_tab_url_attempt(profile, event_tab.get(), &view);
                    }
                }
            }
            event_log.borrow_mut().push(event);
        });
    for (source, all_frames) in crate::host::protected_script_specs_for_native_probe() {
        builder = builder.with_initialization_script_for_main_only(source, !all_frames);
    }
    let view = builder
        .build_as_child(&host)
        .map_err(|error| format!("OAuth Wry view construction failed: {error}"))?;
    registry
        .borrow_mut()
        .attest_built_view(&view, Some(proof))
        .map_err(|error| format!("OAuth view attestation failed: {error}"))?;
    let native_view = super::super::native::webkit(&view);
    *bound_view.borrow_mut() = Some(Weak::from_retained(&native_view));
    profile_isolation::assert_attached_store(&native_view, &store)?;
    let mut loaded = false;
    let mut published = false;
    let result = (|| {
        load_context(&controller, &context, "OAuth redirect")?;
        loaded = true;
        let base = base_url.clone();
        let initial = surface(profile, tab_id, 1, &base, true)?;
        if registry
            .borrow_mut()
            .apply_browser_surface(&initial, |id| (id == tab_id).then(|| native_view.clone()))
            .map_err(|error| format!("OAuth surface publication failed: {error}"))?
            != ControllerSurfaceApplication::Applied
        {
            return Err("OAuth surface was not applied".into());
        }
        published = true;
        surface_ready.set(true);
        persistent_runtime::load_background_content(&context, &run_loop, "OAuth redirect")?;
        window.orderFrontRegardless();
        if broker_order.is_none() {
            view.load_url(&base)
                .map_err(|error| format!("OAuth base navigation failed: {error}"))?;
            wait_for_commit(&tracker, &base, &run_loop)?;
        }
        let (native_window, native_tab) = registry
            .borrow_mut()
            .probe_browser_surface_identity(profile, WINDOW, tab_id)
            .map_err(|error| format!("OAuth tab identity unavailable: {error}"))?
            .ok_or("OAuth tab omitted native identity")?;
        let no_gesture = !unsafe { context.hasActiveUserGestureInTab(&native_tab) };
        if !no_gesture {
            return Err("OAuth fixture unexpectedly began with activeTab gesture".into());
        }
        let observer = observer_view(&context, &native_tab, &run_loop)?;
        reset_observation(&observer, &run_loop)?;
        if worker_post {
            let token_url = server.url("/oauth-token-cors", "fixed");
            let native_token_url = NSURL::URLWithString(&NSString::from_str(&token_url))
                .ok_or("invalid fixed local JSON POST URL")?;
            let effective_grant = unsafe { context.hasAccessToURL(&native_token_url) };
            if effective_grant != grant_host {
                return Err(format!("worker JSON POST host grant mismatch: expected={grant_host} actual={effective_grant}"));
            }
            trigger_worker_request(
                &observer,
                json!({
                    "kind":"worker-post-probe",
                    "cors":token_url,
                    "noCorsHeader":server.url("/oauth-token-no-cors-header", "fixed")
                }),
                &run_loop,
            )?;
            let observed = wait_for_post_observation(&observer, &run_loop)?;
            let cors = &observed["results"][0];
            let no_header = &observed["results"][1];
            println!("native-probe: worker-json-post host_granted={grant_host} cors_state={} cors_status={} cors_json={} without_acao_state={} without_acao_status={} without_acao_error={}",
                cors["state"], cors["status"], cors["parsed"],
                no_header["state"], no_header["status"], no_header["error"]);
            if cors["state"] != "http" || cors["status"] != 200 || cors["parsed"] != true {
                return Err(format!(
                    "worker could not read local CORS JSON POST response: {observed}"
                ));
            }
            if !grant_host && no_header["state"] != "rejected" {
                return Err(format!(
                    "ungranted worker unexpectedly read no-ACAO response: {observed}"
                ));
            }
            return Ok(());
        }
        if let Some(settle_first) = broker_order {
            let start = server.url("/oauth-dns-start", "fixed");
            trigger_broker_create(&observer, &start, &run_loop)?;
            let request = {
                let deadline = Instant::now() + PROBE_TIMEOUT;
                loop {
                    match request_rx.try_recv() {
                        Ok(
                            zephium_core::ports::engine::EngineEvent::ExtensionBrowserRequested {
                                request,
                            },
                        ) => break request,
                        Ok(_) => {}
                        Err(std::sync::mpsc::TryRecvError::Empty) => {}
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            return Err("native browser request sink disconnected".into())
                        }
                    }
                    if Instant::now() >= deadline {
                        return Err("worker tabs.create did not reach native broker".into());
                    }
                    drain_run_loop_once(&run_loop);
                }
            };
            match request.action() {
                ExtensionBrowserRequestAction::CreateTab {
                    url: Some(url),
                    active: true,
                    ..
                } if url.as_ref() == start => {}
                other => {
                    return Err(format!(
                        "worker produced wrong tabs.create request: {other:?}"
                    ))
                }
            }
            let created = ItemId::from(TAB + 1);
            let new_surface = created_surface(profile, tab_id, created, &base, &start)?;
            if registry
                .borrow_mut()
                .apply_browser_surface(&new_surface, |id| {
                    (id == created).then(|| native_view.clone())
                })
                .map_err(|error| format!("created-tab native surface refused: {error}"))?
                != ControllerSurfaceApplication::Applied
            {
                return Err("created-tab surface was not published".into());
            }
            current_tab.set(created);
            let settle = || -> Result<(), String> {
                let outcome = registry
                    .borrow_mut()
                    .settle_browser_request(
                        profile,
                        request.id(),
                        ExtensionBrowserRequestSettlement::Applied(
                            ExtensionBrowserRequestResult::CreatedTab(created),
                        ),
                        None,
                        None,
                    )
                    .map_err(|error| format!("created-tab broker settlement failed: {error}"))?;
                if outcome != ControllerBrowserRequestSettlement::Settled {
                    return Err(format!(
                        "created-tab broker settlement was not terminal: {outcome:?}"
                    ));
                }
                Ok(())
            };
            let before = events.borrow().len();
            if settle_first {
                settle()?;
                let listener = wait_for_listener(&observer, &run_loop)?;
                if listener["state"] != "listener" || listener["count"] != 0 {
                    return Err(format!(
                        "post-await listener was not ready before network: {listener}"
                    ));
                }
                view.load_url(&start)
                    .map_err(|error| format!("settled-first DNS navigation refused: {error}"))?;
                wait_for_failure(&events, before, &run_loop)?;
                let observed = wait_for_observation(&observer, &run_loop, 1).map_err(|error| {
                    format!(
                        "{error}; attempts={:?}; bridge_errors={:?}; phases={:?}",
                        attempts.borrow(),
                        errors.borrow(),
                        events.borrow().iter().skip(before).collect::<Vec<_>>()
                    )
                })?;
                if observed["state"] != "event"
                    || observed["url"]
                        != "https://synthetic-callback.invalid/oauth/cb?code=fixed&state=fixed"
                    || observed["exactActiveTab"] != true
                    || observed["urlPresent"] != true
                    || observed["prefixMatch"] != true
                    || observed["cleaningUpBefore"] != false
                    || observed["codePresent"] != true
                    || observed["statePresent"] != true
                {
                    return Err(format!(
                        "post-await listener missed exact DNS callback: {observed}"
                    ));
                }
            } else {
                view.load_url(&start)
                    .map_err(|error| format!("network-first DNS navigation refused: {error}"))?;
                wait_for_failure(&events, before, &run_loop)?;
                settle()?;
                let first = wait_for_listener(&observer, &run_loop)?;
                let observed = if first["state"] == "event" {
                    first
                } else {
                    wait_for_observation(&observer, &run_loop, 1)?
                };
                if observed["state"] != "event"
                    || observed["url"]
                        != "https://synthetic-callback.invalid/oauth/cb?code=fixed&state=fixed"
                    || observed["exactActiveTab"] != true
                    || observed["urlPresent"] != true
                    || observed["prefixMatch"] != true
                    || observed["cleaningUpBefore"] != false
                    || observed["codePresent"] != true
                    || observed["statePresent"] != true
                {
                    return Err(format!("retained URL was not delivered after late listener registration: {observed}"));
                }
            }
            if tracker
                .committed_snapshot()
                .is_some_and(|(_, url)| url.contains("synthetic-callback.invalid"))
            {
                return Err("DNS-failed callback entered Core committed identity".into());
            }
            if !errors.borrow().is_empty() {
                return Err(format!(
                    "broker-order pending URL bridge failed: {:?}",
                    errors.borrow()
                ));
            }
            return Ok(());
        }
        let callback = server
            .url("/oauth-callback", "fixed")
            .replace("?run=fixed", "?code=fixed-synthetic");
        let before = events.borrow().len();
        if immediate_failure {
            server.release_oauth_callback();
        }
        view.load_url(&server.url("/oauth-start", "fixed"))
            .map_err(|error| format!("OAuth redirect navigation refused: {error}"))?;
        let deadline = Instant::now() + PROBE_TIMEOUT;
        while !server.oauth_callback_entered() && Instant::now() < deadline {
            drain_run_loop_once(&run_loop);
        }
        if !server.oauth_callback_entered() {
            return Err("local OAuth callback request was not reached".into());
        }
        if immediate_failure {
            wait_for_failure(&events, before, &run_loop)?;
        }
        if immediate_clear {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                if poll_observation(&observer, &run_loop, deadline)?
                    .is_some_and(|value| !value.is_null())
                {
                    return Err(
                        "WebKit unexpectedly retained URL after synchronous getter clear".into(),
                    );
                }
                drain_run_loop_once(&run_loop);
            }
        } else {
            let observed = wait_for_observation(&observer, &run_loop, 1)?;
            if observed["url"] != callback
                || observed["exactActiveTab"] != true
                || observed["count"] != 1
            {
                return Err(format!(
                    "OAuth worker observed wrong native tab or URL: {observed}"
                ));
            }
        }
        if events.borrow().iter().skip(before).any(|event| {
            event.phase == wry::NavigationEventPhase::Committed && event.url == callback
        }) {
            return Err("OAuth callback committed before worker observation".into());
        }
        if tracker
            .committed_snapshot()
            .is_some_and(|(_, url)| url == callback)
        {
            return Err("Core navigation tracker promoted uncommitted OAuth callback".into());
        }
        server.release_oauth_callback();
        if !immediate_failure {
            wait_for_failure(&events, before, &run_loop)?;
        }
        if unsafe { context.hasActiveUserGestureInTab(&native_tab) } {
            return Err("OAuth redirect minted an activeTab gesture".into());
        }
        if !immediate_failure && !immediate_clear && !same_turn_nonresident {
            let frame = server.url("/oauth-frame", "fixed");
            let before_frame = attempts.borrow().len();
            view.load_url(&frame)
                .map_err(|error| format!("iframe negative navigation failed: {error}"))?;
            wait_for_commit(&tracker, &frame, &run_loop)?;
            for _ in 0..30 {
                drain_run_loop_once(&run_loop);
            }
            if attempts
                .borrow()
                .iter()
                .skip(before_frame)
                .any(|url| url.contains("/oauth-callback"))
            {
                return Err("subframe OAuth redirect was published as a main-frame tab URL".into());
            }
            if poll_observation(&observer, &run_loop, Instant::now() + PROBE_TIMEOUT)?
                .is_some_and(|value| value["count"] != 1)
            {
                return Err("subframe OAuth redirect reached tabs.onUpdated".into());
            }
            let before_denied = attempts.borrow().len();
            let _ = view.load_url(&server.url("/oauth-denied", "fixed"));
            for _ in 0..20 {
                drain_run_loop_once(&run_loop);
            }
            if attempts.borrow().len() != before_denied {
                return Err("denied OAuth navigation reached admitted-attempt callback".into());
            }
            let replacement_config = unsafe { WKWebViewConfiguration::new(mtm) };
            unsafe {
                replacement_config.setWebsiteDataStore(&store);
                replacement_config.setWebExtensionController(Some(&controller));
            }
            let replacement = unsafe {
                WKWebView::initWithFrame_configuration(
                    WKWebView::alloc(mtm),
                    NSRect::new(NSPoint::new(0., 0.), NSSize::new(1., 1.)),
                    &replacement_config,
                )
            };
            registry
                .borrow_mut()
                .bind_browser_surface_view(profile, tab_id, Some(&replacement))
                .map_err(|error| format!("replacement view binding failed: {error}"))?;
            if registry
                .borrow_mut()
                .observe_browser_tab_url_attempt(profile, tab_id, &native_view, &callback)
                .map_err(|error| format!("stale-view URL refusal failed: {error}"))?
            {
                return Err("stale physical WebView published OAuth callback".into());
            }
            drop(replacement);
        }
        drop((native_window, native_tab));
        if !errors.borrow().is_empty() {
            return Err(format!(
                "OAuth pending URL bridge failed: {:?}",
                errors.borrow()
            ));
        }
        Ok(())
    })();
    server.release_oauth_callback();
    surface_ready.set(false);
    if published {
        let _ = registry.borrow_mut().apply_browser_surface(
            &empty_surface(
                profile,
                if same_turn_nonresident || broker_order.is_some() {
                    3
                } else {
                    2
                },
            ),
            |_| None,
        );
    }
    if loaded {
        let _ = unload_context(&controller, &context, "OAuth redirect");
    }
    let _ = grants.clear_and_verify(&context);
    drop(view);
    window.close();
    drop(window);
    drop(native_view);
    drop(context);
    drop(controller);
    drop(store);
    registry.borrow_mut().seal();
    let _ = registry.borrow_mut().release_all_after_views();
    result?;
    if worker_post {
        println!("native-probe: worker-json-post host_granted={grant_host} local_only=passed");
        return Ok(());
    }
    println!("native-probe: synthetic OAuth redirect mode={}; native_tabs_onUpdated={}; exact_active_tab={}; Core_commit=unchanged; activeTab_gesture=absent; subframe_denied_stale={}; local_only=passed",
        if broker_order == Some(true) { "broker-settle-first" } else if broker_order == Some(false) { "broker-network-first" } else if same_turn_nonresident { "same-turn-nonresident" } else if immediate_clear { "synchronous-clear" } else if immediate_failure { "immediate-failure" } else { "held-open-precommit" },
        if immediate_clear { "suppressed-as-expected" } else { "passed" },
        if immediate_clear { "not-applicable" } else { "passed" },
        if !immediate_failure && !immediate_clear && !same_turn_nonresident && broker_order.is_none() { "passed" } else { "separate-held-open-mode" });
    Ok(())
}

fn observer_view(
    context: &WKWebExtensionContext,
    tab: &ProtocolObject<dyn WKWebExtensionTab>,
    run_loop: &NSRunLoop,
) -> Result<Retained<WKWebView>, String> {
    let action =
        unsafe { context.actionForTab(Some(tab)) }.ok_or("OAuth fixture has no tab action")?;
    let view =
        unsafe { action.popupWebView() }.ok_or("OAuth fixture action has no popup WebView")?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut last = None;
    while Instant::now() < deadline {
        match poll_observation(&view, run_loop, deadline) {
            Ok(Some(_)) => return Ok(view),
            Ok(None) => {}
            Err(error) => last = Some(error),
        }
        drain_run_loop_once(run_loop);
    }
    Err(format!("OAuth probe page did not load extension storage API: last={last:?}; loaded_url={:?}; context_errors={}",
        unsafe { view.URL() }.and_then(|url| url.absoluteString()).map(|url| url.to_string()),
        unsafe { context.errors() }.count()))
}

fn reset_observation(view: &WKWebView, run_loop: &NSRunLoop) -> Result<(), String> {
    let result = Rc::new(RefCell::new(None));
    let slot = result.clone();
    let callback = block2::RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
        *slot.borrow_mut() = Some(
            error.is_null()
                && unsafe { value.as_ref() }
                    .and_then(AnyObject::downcast_ref::<NSNumber>)
                    .is_some_and(|value| value.boolValue()),
        );
    });
    let world = unsafe {
        objc2_web_kit::WKContentWorld::pageWorld(MainThreadMarker::new().expect("main thread"))
    };
    unsafe {
        view.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
            &NSString::from_str(
                "await chrome.storage.local.remove('zephiumOAuthRedirectObservation'); return true",
            ),
            None,
            None,
            &world,
            Some(&callback),
        )
    };
    let deadline = Instant::now() + PROBE_TIMEOUT;
    while result.borrow().is_none() && Instant::now() < deadline {
        drain_run_loop_once(run_loop);
    }
    if result.borrow_mut().take() != Some(true) {
        return Err("OAuth fixture observation storage did not reset".into());
    }
    Ok(())
}
