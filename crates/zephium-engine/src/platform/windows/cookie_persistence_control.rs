//! Disposable SDK control: no Work host, seeding, metadata, model, or external network.

use std::cell::{Cell, RefCell};
use std::io::Write as _;
use std::os::windows::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2CookieManager, ICoreWebView2_13, ICoreWebView2_2,
};
use windows_core::{Interface as _, HSTRING, PCWSTR, PWSTR};
use wry::{
    WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows as _, WebViewExtWindows as _,
};

use super::super::{
    attest_environment, browser_process_for_environment, install_browser_process_exit_observer,
    pump_browser_exit_callbacks, take_pwstr_bounded, wait_for_browser_process_exit, BrowserProcess,
    BrowserProcessExitObserver,
};
use super::ProbeHostWindow;

const QUERY_TIMEOUT: Duration = Duration::from_secs(5);
const EXIT_TIMEOUT: Duration = Duration::from_secs(15);
const NAMED_PROFILE: &str = "cookie-control";
const COOKIE_URL: &str = "https://cookie-control.invalid/";

#[derive(Clone, Copy)]
enum ControlEnvironment {
    Default,
    VerbatimUserDataFolder,
    OrdinaryLongUserDataFolder,
    WorkBrowserArguments,
    WorkExtensions,
    WorkHttpLoopback,
    WorkHttpResponse(u16),
    WorkHttpMetadata(u16),
    WorkConstructor,
    WorkRetainedEnvironment,
    WorkIdle,
}

#[derive(Default)]
struct ControlBinding {
    profile: Option<(String, PathBuf)>,
    environment: Option<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment>,
}

pub(crate) fn run() -> Result<(), &'static str> {
    let mut arguments = std::env::args_os().skip(1);
    let first_argument = arguments.next();
    if arguments.next().is_none() {
        if first_argument.as_deref() == Some(std::ffi::OsStr::new("--verbatim-udf")) {
            return control(
                Some(NAMED_PROFILE),
                "named-verbatim",
                ControlEnvironment::VerbatimUserDataFolder,
            );
        }
        if first_argument.as_deref() == Some(std::ffi::OsStr::new("--long-udf")) {
            return control(
                Some(NAMED_PROFILE),
                "named-long",
                ControlEnvironment::OrdinaryLongUserDataFolder,
            );
        }
    }
    control(None, "default", ControlEnvironment::Default)?;
    control(Some(NAMED_PROFILE), "named", ControlEnvironment::Default)?;
    let target = zephium_agentic::ContextNavigationTarget::parse("http://127.0.0.1:4242/")
        .map_err(|_| "cookie_control_work_target")?;
    let super::super::agent_context::AgentOwnedProfile::Automation { name } =
        super::super::agent_context::AgentOwnedProfile::work_site(&target)
            .map_err(|_| "cookie_control_work_name")?
    else {
        return Err("cookie_control_work_name");
    };
    // Derive exactly the production scheme/port/site naming shape, while
    // retaining direct SDK construction and only synthetic cookie contents.
    control(Some(&name), "work-named", ControlEnvironment::Default)?;
    control(
        Some(&name),
        "work-args",
        ControlEnvironment::WorkBrowserArguments,
    )?;
    control(
        Some(&name),
        "work-extensions",
        ControlEnvironment::WorkExtensions,
    )?;
    control(
        Some(&name),
        "work-http",
        ControlEnvironment::WorkHttpLoopback,
    )?;
    for (label, metadata) in [("work-http-response", false), ("work-http-metadata", true)] {
        let server = CookieServer::start()?;
        let response_target = zephium_agentic::ContextNavigationTarget::parse(&format!(
            "http://127.0.0.1:{}/seed",
            server.port
        ))
        .map_err(|_| "cookie_control_work_target")?;
        let super::super::agent_context::AgentOwnedProfile::Automation { name } =
            super::super::agent_context::AgentOwnedProfile::work_site(&response_target)
                .map_err(|_| "cookie_control_work_name")?
        else {
            return Err("cookie_control_work_name");
        };
        let options = if metadata {
            ControlEnvironment::WorkHttpMetadata(server.port)
        } else {
            ControlEnvironment::WorkHttpResponse(server.port)
        };
        let outcome = control(Some(&name), label, options);
        let requests = server.finish();
        let _ = writeln!(std::io::stderr().lock(),
            "windows-cookie-control: profile={label} stage=loopback_server seed_requests={requests:?}; content=synthetic");
        outcome?;
        if requests? == 0 {
            return Err("cookie_control_server_no_seed");
        }
    }
    control(
        Some(&name),
        "work-constructor",
        ControlEnvironment::WorkConstructor,
    )?;
    control(
        Some(&name),
        "work-retained-environment",
        ControlEnvironment::WorkRetainedEnvironment,
    )?;
    control(Some(&name), "work-idle", ControlEnvironment::WorkIdle)
}

fn control(
    name: Option<&str>,
    label: &'static str,
    options: ControlEnvironment,
) -> Result<(), &'static str> {
    let directory = tempfile::Builder::new()
        .prefix("zephium-cookie-control-")
        .tempdir()
        .map_err(|_| "cookie_control_tempdir")?;
    let mut udf_directory = directory.path().to_path_buf();
    if matches!(options, ControlEnvironment::OrdinaryLongUserDataFolder) {
        // Only this fresh TempDir owns these fixed synthetic descendants.
        // Preserve ordinary Win32 spelling at the SDK boundary even beyond
        // MAX_PATH; canonical paths still attest the exact returned UDF.
        for index in 0..5 {
            udf_directory.push(format!("long-segment-{index}-{}", "x".repeat(48)));
        }
        std::fs::create_dir_all(&udf_directory).map_err(|_| "cookie_control_long_directory")?;
        let units = udf_directory.as_os_str().encode_wide().count();
        if units <= 260 || units > 600 {
            return Err("cookie_control_long_directory_bound");
        }
        let _ = writeln!(std::io::stderr().lock(),
            "windows-cookie-control: profile={label} stage=long_udf utf16_units={units}; content=synthetic");
    }
    let metadata = if matches!(
        options,
        ControlEnvironment::WorkHttpMetadata(_)
            | ControlEnvironment::WorkConstructor
            | ControlEnvironment::WorkRetainedEnvironment
            | ControlEnvironment::WorkIdle
    ) {
        let canonical = directory
            .path()
            .canonicalize()
            .map_err(|_| "cookie_control_canonical_udf")?;
        Some(Rc::new(
            super::super::work_seed_metadata::WorkSeedMetadata::open(&canonical)
                .map_err(|_| "cookie_control_seed_metadata")?,
        ))
    } else {
        None
    };
    let store = if matches!(
        options,
        ControlEnvironment::WorkConstructor
            | ControlEnvironment::WorkRetainedEnvironment
            | ControlEnvironment::WorkIdle
    ) {
        Some(Rc::new(super::super::agent_context::WorkStoreSeed::new(
            metadata
                .as_ref()
                .ok_or("cookie_control_seed_metadata")?
                .clone(),
            name.ok_or("cookie_control_work_name")?.to_owned(),
        )))
    } else {
        None
    };
    let host = ProbeHostWindow::new().map_err(|_| "cookie_control_window")?;
    let mut binding = ControlBinding::default();
    let outcome = (|| {
        cycle(
            &host,
            &udf_directory,
            name,
            label,
            true,
            &mut binding,
            options,
            store.as_ref(),
        )?;
        cycle(
            &host,
            &udf_directory,
            name,
            label,
            false,
            &mut binding,
            options,
            store.as_ref(),
        )
    })();
    drop(host);
    drop(binding);
    drop(store);
    drop(metadata);
    if let Err(code) = outcome {
        // Retain only this freshly-created fixture after any failure. Never
        // remove a UDF unless its exact native browser exit was proved.
        let retained = directory.keep();
        let _ = writeln!(std::io::stderr().lock(),
            "windows-cookie-control: profile={label} result={code} retained_udf={}; content=synthetic",
            retained.display());
        return Err(code);
    }
    directory.close().map_err(|_| "cookie_control_remove_udf")?;
    let _ =
        writeln!(std::io::stderr().lock(),
        "windows-cookie-control: profile={label} result=pass udf_removed=true; content=synthetic");
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "The native diagnostic keeps exact host, UDF, profile, phase, binding, options and retained store ownership explicit."
)]
fn cycle(
    host: &ProbeHostWindow,
    directory: &Path,
    name: Option<&str>,
    label: &'static str,
    initial: bool,
    binding: &mut ControlBinding,
    options: ControlEnvironment,
    store: Option<&Rc<super::super::agent_context::WorkStoreSeed>>,
) -> Result<(), &'static str> {
    let udf_argument = if matches!(options, ControlEnvironment::VerbatimUserDataFolder) {
        directory
            .canonicalize()
            .map_err(|_| "cookie_control_canonical_udf")?
    } else {
        directory.to_path_buf()
    };
    let verbatim = udf_argument.to_string_lossy().starts_with(r"\\?\");
    let _=writeln!(std::io::stderr().lock(), "windows-cookie-control: profile={label} stage=udf_spelling verbatim={verbatim}; content=synthetic");
    let mut context = WebContext::new(Some(udf_argument));
    // Use regular Wry defaults, apart from keeping the disposable blank view
    // hidden and non-private. No Work settings or browser arguments participate.
    let loaded = Rc::new(Cell::new(false));
    let callback_loaded = loaded.clone();
    let expected_response = match options {
        ControlEnvironment::WorkHttpResponse(port) | ControlEnvironment::WorkHttpMetadata(port) => {
            Some(format!("http://127.0.0.1:{port}/seed"))
        }
        _ => None,
    };
    let failed = Rc::new(Cell::new(false));
    let callback_failed = failed.clone();
    let navigation_expected = expected_response;
    let mut builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_url("about:blank")
        .with_incognito(false)
        .with_visible(false)
        .with_focused(false)
        .with_navigation_event_handler(move |event| {
            if navigation_expected.as_deref() == Some(event.url.as_str()) {
                let _ = writeln!(std::io::stderr().lock(),
                    "windows-cookie-control: profile={label} stage=http_navigation phase={:?}; content=synthetic",
                    event.phase);
                match event.phase {
                    wry::NavigationEventPhase::Finished => callback_loaded.set(true),
                    wry::NavigationEventPhase::Failed | wry::NavigationEventPhase::Cancelled => callback_failed.set(true),
                    _ => {}
                }
            }
        });
    if !matches!(
        options,
        ControlEnvironment::WorkConstructor
            | ControlEnvironment::WorkRetainedEnvironment
            | ControlEnvironment::WorkIdle
    ) {
        if let Some(name) = name {
            builder = builder.with_profile_name(name);
        }
    }
    if !matches!(
        options,
        ControlEnvironment::Default
            | ControlEnvironment::VerbatimUserDataFolder
            | ControlEnvironment::OrdinaryLongUserDataFolder
    ) {
        // Match Work's actual environment argument string, including its
        // preservation of SmartScreen; no security feature is disabled here
        // beyond the same fixed native UI feature exclusions used by Work.
        builder = builder.with_additional_browser_args("--disable-features=msWebOOUI,msPdfOOUI");
    }
    if matches!(
        options,
        ControlEnvironment::WorkExtensions
            | ControlEnvironment::WorkHttpLoopback
            | ControlEnvironment::WorkHttpResponse(_)
            | ControlEnvironment::WorkHttpMetadata(_)
            | ControlEnvironment::WorkConstructor
            | ControlEnvironment::WorkRetainedEnvironment
            | ControlEnvironment::WorkIdle
    ) {
        // A fresh owned UDF has no person extensions. Enable the same native
        // environment capability as Work, and require only its runtime components
        // before initialization; never load or install any extension.
        builder = builder.with_browser_extension_startup_gate(move |_, core| {
            let fail = |stage: &'static str, error: windows_core::Error| {
                let _ = writeln!(std::io::stderr().lock(),
                    "windows-cookie-control: profile={label} stage={stage} hresult={:?}; content=synthetic",
                    error.code());
                error
            };
            let profile = super::super::extensions::profile(core)
                .map_err(|error| fail("extension_profile", error))?;
            let entries = super::super::extensions::list(&profile).map_err(|_| {
                fail("extension_inventory", windows_core::Error::from_hresult(
                    windows::Win32::Foundation::E_ACCESSDENIED))
            })?;
            let count = entries.len();
            for entry in entries {
                let id = super::super::extensions::extension_id(&entry)
                    .map_err(|error| fail("extension_identity", error))?;
                if !super::super::extensions::is_runtime_component(&id) {
                    return Err(fail("extension_unapproved", windows_core::Error::from_hresult(
                        windows::Win32::Foundation::E_ACCESSDENIED)));
                }
            }
            let _ = writeln!(std::io::stderr().lock(),
                "windows-cookie-control: profile={label} stage=extension_inventory count={count} runtime_only=true; content=synthetic");
            Ok(())
        });
    }
    if matches!(options, ControlEnvironment::WorkRetainedEnvironment) {
        if let Some(environment) = binding.environment.as_ref() {
            builder = builder.with_environment(environment.clone());
        }
    }
    let mut view = builder.build_as_child(host).map_err(|_| {
        let _ = writeln!(
            std::io::stderr().lock(),
            "windows-cookie-control: profile={label} stage=construction_refused; content=synthetic"
        );
        "cookie_control_construct"
    })?;
    let environment = view.environment();
    if matches!(options, ControlEnvironment::WorkRetainedEnvironment) {
        if binding
            .environment
            .as_ref()
            .is_some_and(|old| !super::super::same_environment(old, &environment))
        {
            return Err("cookie_control_retained_environment_mismatch");
        }
        binding.environment = Some(environment.clone());
    }
    let mut owned = None;
    let mut process: Option<BrowserProcess> = None;
    let mut observer: Option<BrowserProcessExitObserver> = None;
    let observed = (|| {
        attest_environment(&environment, directory).map_err(|_| "cookie_control_environment")?;
        let browser =
            browser_process_for_environment(&environment).map_err(|_| "cookie_control_browser")?;
        let exit = install_browser_process_exit_observer(&environment, browser.id(), |_| {})
            .map_err(|_| "cookie_control_observer")?;
        process = Some(browser);
        observer = Some(exit);
        if matches!(
            options,
            ControlEnvironment::WorkConstructor
                | ControlEnvironment::WorkRetainedEnvironment
                | ControlEnvironment::WorkIdle
        ) {
            use super::super::agent_context::{AgentOwnedProfile, AgentOwnedViewCallbacks};
            let failed = Rc::new(Cell::new(false));
            let nav = failed.clone();
            let renderer = failed.clone();
            let browser = failed.clone();
            let invariant = failed.clone();
            let panic = failed.clone();
            let deadline = Instant::now() + Duration::from_secs(30);
            let (mut work, _) = super::super::agent_context::build_owned_work_view(
                host,
                zephium_agentic::ContextOwnedViewport::STANDARD,
                &environment,
                AgentOwnedProfile::Automation {
                    name: name.ok_or("cookie_control_work_name")?.to_owned(),
                },
                zephium_agentic::ContextProfileStorageClass::Durable,
                directory,
                deadline,
                true,
                AgentOwnedViewCallbacks::new(
                    move |_| nav.set(true),
                    || {},
                    move || renderer.set(true),
                    move || browser.set(true),
                    move || invariant.set(true),
                    move || panic.set(true),
                ),
            )
            .map_err(|_| "cookie_control_work_construct")?;
            let origin = "http://127.0.0.1:4242".to_owned();
            let scope = zephium_agentic::ContextCookieScope::try_new(vec![
                zephium_agentic::ContextCookieOrigin::parse(&origin)
                    .map_err(|_| "cookie_control_seed_scope")?,
            ])
            .map_err(|_| "cookie_control_seed_scope")?;
            let seeded = Rc::new(Cell::new(None));
            let done = seeded.clone();
            let panic = failed.clone();
            work.seed_work_session(
                None,
                scope,
                origin,
                store.ok_or("cookie_control_seed_store")?.clone(),
                deadline,
                move |success, unproven| done.set(Some(success && !unproven)),
                move || panic.set(true),
            );
            owned = Some(work);
            while seeded.get().is_none() {
                if failed.get()
                    || Instant::now() >= deadline
                    || !pump_browser_exit_callbacks(deadline)
                {
                    return Err("cookie_control_work_seed");
                }
            }
            if seeded.get() != Some(true) || failed.get() {
                return Err("cookie_control_work_seed");
            }
        }
        let content = owned.as_ref().map_or(&view, |work| work.view());
        let core = content.webview();
        let extended = core
            .cast::<ICoreWebView2_13>()
            .map_err(|_| "cookie_control_profile")?;
        // SAFETY: this STA retains the exact live controller and its core.
        let profile = unsafe { extended.Profile() }.map_err(|_| "cookie_control_profile")?;
        let mut native_name = PWSTR::null();
        let mut private = windows_core::BOOL::default();
        // SAFETY: initialized outputs receive this exact live profile's properties.
        unsafe { profile.ProfileName(&mut native_name) }
            .map_err(|_| "cookie_control_profile_properties")?;
        let native_name =
            take_pwstr_bounded(native_name, 128, 128).ok_or("cookie_control_profile_name")?;
        // SAFETY: this same live profile initializes the exact BOOL output.
        unsafe { profile.IsInPrivateModeEnabled(&mut private) }
            .map_err(|_| "cookie_control_profile_properties")?;
        let mut native_path = PWSTR::null();
        // SAFETY: the live profile initializes an allocated string, consumed
        // and released once by the bounded helper immediately below.
        unsafe { profile.ProfilePath(&mut native_path) }
            .map_err(|_| "cookie_control_profile_path")?;
        let native_path =
            take_pwstr_bounded(native_path, 32768, 131072).ok_or("cookie_control_profile_path")?;
        let native_path = std::fs::canonicalize(Path::new(&native_path))
            .map_err(|_| "cookie_control_profile_path")?;
        let root = std::fs::canonicalize(directory).map_err(|_| "cookie_control_profile_path")?;
        let path_owned = native_path != root && native_path.starts_with(&root);
        let binding_matches = binding.profile.as_ref().is_none_or(|(old_name, old_path)| {
            *old_name == native_name && *old_path == native_path
        });
        let name_matches = name.is_none_or(|name| name == native_name);
        // This is exclusively a new synthetic UDF. The bounded, escaped native
        // name cannot reveal an existing person's profile or browsing data.
        let expected_name = name.unwrap_or("captured-sdk-default");
        let _ = writeln!(std::io::stderr().lock(),
            "windows-cookie-control: profile={label} stage=profile_attestation native_name={native_name:?} expected_name={expected_name:?} private={} exact_name={name_matches} exact_initial_binding={binding_matches} path_owned={path_owned}; content=synthetic",
            private.as_bool());
        if private.as_bool() || !name_matches || !binding_matches || !path_owned {
            return Err("cookie_control_profile_mismatch");
        }
        if binding.profile.is_none() {
            // The SDK's default name is captured only from our sole regular
            // controller in an attested fresh UDF, then stays exact on reopen.
            binding.profile = Some((native_name, native_path));
        }
        let core = core
            .cast::<ICoreWebView2_2>()
            .map_err(|_| "cookie_control_manager")?;
        // SAFETY: CookieManager returns an owned COM reference for this controller.
        let manager = unsafe { core.CookieManager() }.map_err(|_| "cookie_control_manager")?;
        if initial {
            if query(&manager, options)? != (0, false, false) {
                return Err("cookie_control_fresh_not_empty");
            }
            if let ControlEnvironment::WorkHttpResponse(port)
            | ControlEnvironment::WorkHttpMetadata(port) = options
            {
                view.load_url(&format!("http://127.0.0.1:{port}/seed"))
                    .map_err(|_| "cookie_control_http_navigation")?;
                let deadline = Instant::now() + QUERY_TIMEOUT;
                while !loaded.get() {
                    if failed.get() {
                        return Err("cookie_control_http_navigation_failed");
                    }
                    if Instant::now() >= deadline || !pump_browser_exit_callbacks(deadline) {
                        return Err("cookie_control_http_load");
                    }
                }
            } else {
                set_cookie(&manager, options)?;
            }
        }
        let (count, persistent, future) = query(&manager, options)?;
        let stage = if initial { "before_close" } else { "reopened" };
        let _ = writeln!(std::io::stderr().lock(),
            "windows-cookie-control: profile={label} stage={stage} count={count} persistent={persistent} future={future} private=false; content=synthetic");
        if count != 1 || !persistent || !future {
            return Err("cookie_control_persistence_failed");
        }
        Ok(())
    })();
    let idle = if matches!(options, ControlEnvironment::WorkIdle) {
        if let Some(work) = owned.as_ref() {
            let admitted = work.set_work_leased(true) && work.set_work_leased(false);
            let deadline = Instant::now() + QUERY_TIMEOUT;
            while !work.work_native_activity_drained() && Instant::now() < deadline {
                if !pump_browser_exit_callbacks(deadline) {
                    break;
                }
            }
            let suspended = work.is_suspended().is_ok_and(|value| value);
            let clean = admitted && work.work_native_activity_drained() && suspended;
            let _ = writeln!(std::io::stderr().lock(),
                "windows-cookie-control: profile={label} stage=native_idle admitted={admitted} drained={} suspended={suspended}; content=synthetic",
                work.work_native_activity_drained());
            clean
        } else {
            false
        }
    } else {
        true
    };
    let work_closed = owned
        .as_mut()
        .is_none_or(|work| close_native_result(work.close()));
    drop(owned);
    let closed = close_view(&mut view) && work_closed;
    drop(view);
    drop(context);
    drop(environment);
    let exited = match (process.as_ref(), observer.as_ref()) {
        (Some(process), Some(observer)) => {
            wait_for_browser_process_exit(process, &observer.proof(), Instant::now() + EXIT_TIMEOUT)
        }
        _ => false,
    };
    let stage = if initial {
        "initial_exit"
    } else {
        "reopened_exit"
    };
    let _ = writeln!(std::io::stderr().lock(),
        "windows-cookie-control: profile={label} stage={stage} controller_closed={closed} exact_browser_exit={exited}; content=synthetic");
    if !closed || !exited {
        return Err("cookie_control_exit_unproven");
    }
    if !idle {
        return Err("cookie_control_idle_unproven");
    }
    observed
}

fn close_view(view: &mut WebView) -> bool {
    close_native_result(view.close())
}

fn close_native_result(result: Result<(), wry::WebView2CleanupDebt>) -> bool {
    let deadline = Instant::now() + QUERY_TIMEOUT;
    let mut complete = match result {
        Ok(()) => true,
        Err(mut debt) => {
            while !debt.is_complete() && Instant::now() < deadline {
                let _ = debt.retry();
                if !pump_browser_exit_callbacks(deadline) {
                    break;
                }
            }
            debt.is_complete()
        }
    };
    for mut debt in wry::pending_webview2_cleanup_debts() {
        while !debt.is_complete() && Instant::now() < deadline {
            let _ = debt.retry();
            if !pump_browser_exit_callbacks(deadline) {
                break;
            }
        }
        complete &= debt.is_complete();
    }
    complete && !wry::webview2_cleanup_overflowed()
}

fn set_cookie(
    manager: &ICoreWebView2CookieManager,
    options: ControlEnvironment,
) -> Result<(), &'static str> {
    let loopback = matches!(
        options,
        ControlEnvironment::WorkHttpLoopback
            | ControlEnvironment::WorkConstructor
            | ControlEnvironment::WorkRetainedEnvironment
            | ControlEnvironment::WorkIdle
    );
    let name = HSTRING::from("persistent-control");
    let value = HSTRING::from("synthetic");
    let domain = HSTRING::from(if loopback {
        "127.0.0.1"
    } else {
        "cookie-control.invalid"
    });
    let path = HSTRING::from("/");
    let expires = now()? + 3600.0;
    // SAFETY: initialized immutable strings remain live for synchronous COM calls;
    // this manager belongs only to our new non-private disposable profile.
    let cookie = unsafe { manager.CreateCookie(&name, &value, &domain, &path) }
        .map_err(|_| "cookie_control_set_cookie")?;
    // SAFETY: the new cookie is retained locally until the synchronous manager
    // mutation completes; every property is synthetic and valid for this origin.
    unsafe {
        cookie
            .SetExpires(expires)
            .and_then(|_| cookie.SetIsHttpOnly(true))
            .and_then(|_| cookie.SetIsSecure(!loopback))
            .and_then(|_| manager.AddOrUpdateCookie(&cookie))
    }
    .map_err(|_| "cookie_control_set_cookie")
}

fn now() -> Result<f64, &'static str> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs_f64())
        .map_err(|_| "cookie_control_clock")
}

fn query(
    manager: &ICoreWebView2CookieManager,
    options: ControlEnvironment,
) -> Result<(u32, bool, bool), &'static str> {
    let closed = Rc::new(Cell::new(false));
    let result = Rc::new(RefCell::new(None));
    let callback_closed = closed.clone();
    let callback_result = result.clone();
    let handler =
        webview2_com::GetCookiesCompletedHandler::create(Box::new(move |status, list| {
            if callback_closed.replace(true) {
                return Ok(());
            }
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                status.map_err(|_| "cookie_control_query")?;
                let list = list.ok_or("cookie_control_query_list")?;
                let mut count = 0;
                // SAFETY: this callback retains the supplied list and initialized output.
                unsafe { list.Count(&mut count) }.map_err(|_| "cookie_control_query_count")?;
                if count == 0 {
                    return Ok((0, false, false));
                }
                if count != 1 {
                    return Err("cookie_control_query_count");
                }
                // SAFETY: Count proves index zero exists in this callback-owned list.
                let cookie = unsafe { list.GetValueAtIndex(0) }
                    .map_err(|_| "cookie_control_query_cookie")?;
                let mut session = windows_core::BOOL::default();
                let mut expires = 0.0;
                // SAFETY: the callback retains this cookie; outputs have exact initialized types.
                unsafe {
                    cookie
                        .IsSession(&mut session)
                        .and_then(|_| cookie.Expires(&mut expires))
                }
                .map_err(|_| "cookie_control_query_expiry")?;
                Ok((
                    count,
                    !session.as_bool(),
                    expires.is_finite() && expires > now()?,
                ))
            }))
            .unwrap_or(Err("cookie_control_query_panic"));
            if let Ok(mut slot) = callback_result.try_borrow_mut() {
                *slot = Some(outcome);
            }
            Ok(())
        }));
    let query_url = match options {
        ControlEnvironment::WorkHttpLoopback
        | ControlEnvironment::WorkConstructor
        | ControlEnvironment::WorkRetainedEnvironment
        | ControlEnvironment::WorkIdle => "http://127.0.0.1:4242/".to_owned(),
        ControlEnvironment::WorkHttpResponse(port) | ControlEnvironment::WorkHttpMetadata(port) => {
            format!("http://127.0.0.1:{port}/")
        }
        _ => COOKIE_URL.to_owned(),
    };
    let url = HSTRING::from(query_url);
    // SAFETY: this owning controller stays live throughout the bounded STA query.
    unsafe { manager.GetCookies(PCWSTR(url.as_ptr()), &handler) }
        .map_err(|_| "cookie_control_query_start")?;
    let deadline = Instant::now() + QUERY_TIMEOUT;
    loop {
        if Instant::now() >= deadline {
            closed.set(true);
            return Err("cookie_control_query_timeout");
        }
        if let Some(outcome) = result.borrow_mut().take() {
            return outcome;
        }
        if !pump_browser_exit_callbacks(deadline) {
            closed.set(true);
            return Err("cookie_control_query_pump");
        }
    }
}

// Every socket, worker and lifetime belongs solely to this fixed loopback case.
struct CookieServer {
    port: u16,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<Result<u32, &'static str>>>,
}
impl CookieServer {
    fn start() -> Result<Self, &'static str> {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .map_err(|_| "cookie_control_server_bind")?;
        let port = listener
            .local_addr()
            .map_err(|_| "cookie_control_server_address")?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|_| "cookie_control_server_mode")?;
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopped = stop.clone();
        let worker = std::thread::spawn(move || {
            use std::io::{Read as _, Write as _};
            let deadline = Instant::now() + Duration::from_secs(30);
            let mut seeds = 0_u32;
            let mut requests = 0_u32;
            while !stopped.load(std::sync::atomic::Ordering::Acquire) && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, peer)) => {
                        requests += 1;
                        if !peer.ip().is_loopback() || requests > 8 {
                            return Err("cookie_control_server_bound");
                        }
                        stream
                            .set_nonblocking(false)
                            .map_err(|_| "cookie_control_server_stream_mode")?;
                        stream
                            .set_read_timeout(Some(Duration::from_millis(500)))
                            .map_err(|_| "cookie_control_server_read_timeout")?;
                        stream
                            .set_write_timeout(Some(Duration::from_millis(500)))
                            .map_err(|_| "cookie_control_server_write_timeout")?;
                        let mut request = [0_u8; 2048];
                        let mut read = 0_usize;
                        while read < request.len()
                            && !request[..read].windows(4).any(|part| part == b"\r\n\r\n")
                        {
                            match stream.read(&mut request[read..]) {
                                Ok(0) => break,
                                Ok(count) => read += count,
                                Err(error)
                                    if matches!(
                                        error.kind(),
                                        std::io::ErrorKind::WouldBlock
                                            | std::io::ErrorKind::TimedOut
                                    ) =>
                                {
                                    break
                                }
                                Err(_) => return Err("cookie_control_server_read"),
                            }
                        }
                        if read == 0 {
                            continue;
                        }
                        let seed = request[..read].starts_with(b"GET /seed ");
                        let _ = writeln!(std::io::stderr().lock(),
                            "windows-cookie-control: stage=loopback_request seed={seed} is_get={} complete_headers={} requests={requests}; content=synthetic",
                            request[..read].starts_with(b"GET "),
                            request[..read].windows(4).any(|part| part == b"\r\n\r\n"));
                        let response = if seed {
                            seeds += 1;
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nSet-Cookie: persistent-control=synthetic; Path=/; Max-Age=3600; HttpOnly\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        } else {
                            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        };
                        stream
                            .write_all(response.as_bytes())
                            .map_err(|_| "cookie_control_server_write")?;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => return Err("cookie_control_server_accept"),
                }
            }
            Ok(seeds)
        });
        Ok(Self {
            port,
            stop,
            worker: Some(worker),
        })
    }
    fn finish(mut self) -> Result<u32, &'static str> {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        self.worker
            .take()
            .ok_or("cookie_control_server_owner")?
            .join()
            .map_err(|_| "cookie_control_server_panic")?
    }
}
impl Drop for CookieServer {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
