//! Isolated qualification of native manifest glob enforcement. Probe-only.
use super::*;
use objc2_foundation::NSURLRequest;

pub(super) fn run() -> Result<bool, String> {
    let Some(os) = supported_runtime()? else {
        return Ok(false);
    };
    let watchdog = arm_process_watchdog();
    let result = run_inner(&os);
    watchdog.store(true, Ordering::Release);
    result.map(|()| true)
}

fn run_inner(os: &str) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("glob probe requires main thread")?;
    let app = NSApplication::sharedApplication(mtm);
    let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.finishLaunching();
    let directory = tempfile::tempdir().map_err(|e| e.to_string())?;
    let mut scripts = Vec::new();
    for (name, filter) in [
        ("baseline", None),
        ("included", Some("include_globs")),
        ("excluded", Some("exclude_globs")),
    ] {
        let mut script = json!({"matches":[HOST_MATCH_PATTERN],"js":[format!("{name}.js")],"run_at":"document_end"});
        if let Some(filter) = filter {
            script[filter] = json!(["*allowed*"]);
        }
        scripts.push(script);
        write_fixture_file(
            directory.path(),
            &format!("{name}.js"),
            &format!("document.documentElement.setAttribute('data-glob-{name}', '1');"),
        )?;
    }
    write_fixture_file(directory.path(), "manifest.json", &json!({"manifest_version":3,"name":"Glob qualification","version":"1.0","host_permissions":[HOST_MATCH_PATTERN],"content_scripts":scripts}).to_string())?;
    let server = FixtureServer::start(None)?;
    let run_loop = NSRunLoop::mainRunLoop();
    let (context_weak, controller_weak, view_weak, outcome) = objc2::rc::autoreleasepool(|_| {
        let bundle = new_nonpersistent_controller(mtm)?;
        let extension = load_extension(directory.path(), &run_loop, mtm)?;
        let context = new_context(&extension, "zephium-glob-probe")?;
        let applied = super::super::extensions::apply_probe_grants(
            &context,
            &[],
            &[HOST_MATCH_PATTERN],
            true,
        )
        .map_err(|e| format!("glob grants: {e}"))?;
        load_context(&bundle.controller, &context, "glob probe")?;
        unsafe {
            bundle
                .webview_configuration
                .setWebExtensionController(Some(&bundle.controller));
        }
        let view = unsafe {
            WKWebView::initWithFrame_configuration(
                WKWebView::alloc(mtm),
                NSRect::new(NSPoint::new(0., 0.), NSSize::new(320., 200.)),
                &bundle.webview_configuration,
            )
        };
        let mut failures = Vec::new();
        let outcome = (|| {
            for (run, expected) in [
                ("allowed", json!([true, true, false])),
                ("other", json!([true, false, true])),
            ] {
                let url = native_url(&server.url("/theme", run))?;
                unsafe {
                    view.loadRequest(&NSURLRequest::requestWithURL(&url));
                }
                let deadline = Instant::now() + PROBE_TIMEOUT;
                loop {
                    drain_run_loop_once(&run_loop);
                    let state = evaluate(&view, &run_loop, deadline)?;
                    if state["ready"] == "complete"
                        && state["url"] == server.url("/theme", run)
                        && state["markers"][0] == true
                    {
                        println!(
                            "native-globs: {run}; actual={}; expected={expected}",
                            state["markers"]
                        );
                        if state["markers"] != expected {
                            failures.push(run);
                        }
                        break;
                    }
                    if Instant::now() >= deadline {
                        return Err("glob page did not settle".to_owned());
                    }
                }
            }
            if failures.is_empty() {
                Ok(())
            } else {
                Err(format!("native glob filters not enforced: {failures:?}"))
            }
        })();
        unsafe {
            view.stopLoading();
        }
        unload_context(&bundle.controller, &context, "glob probe")?;
        applied
            .clear_and_verify(&context)
            .map_err(|e| format!("glob cleanup: {e}"))?;
        Ok::<_, String>((
            Weak::from_retained(&context),
            Weak::from_retained(&bundle.controller),
            Weak::from_retained(&view),
            outcome,
        ))
    })?;
    let deadline = Instant::now() + TEARDOWN_TIMEOUT;
    while !objc2::rc::autoreleasepool(|_| {
        context_weak.load().is_none()
            && controller_weak.load().is_none()
            && view_weak.load().is_none()
    }) {
        if Instant::now() >= deadline {
            return Err("glob native teardown not proven".into());
        }
        drain_run_loop_once(&run_loop);
    }
    outcome.map_err(|e| format!("{e}; native-objects-released=passed; product-authority=false"))?;
    println!("native-globs: os={os}; include-exclude=passed; native-objects-released=passed; product-authority=false");
    Ok(())
}

fn evaluate(view: &WKWebView, run_loop: &NSRunLoop, deadline: Instant) -> Result<Value, String> {
    let result = Rc::new(RefCell::new(None));
    let callback_result = result.clone();
    let completion = block2::RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
        callback_result.replace(Some(if let Some(error) = unsafe { error.as_ref() } {
            Err(format_native_error("glob evaluation", error))
        } else {
            unsafe { value.as_ref() }
                .and_then(AnyObject::downcast_ref::<NSString>)
                .map(ToString::to_string)
                .ok_or_else(|| "glob evaluation returned a non-string".to_owned())
        }));
    });
    unsafe {
        view.evaluateJavaScript_completionHandler(&NSString::from_str("JSON.stringify({ready:document.readyState,url:location.href,markers:['baseline','included','excluded'].map(n=>document.documentElement?.getAttribute('data-glob-'+n)==='1')})"), Some(&completion));
    }
    loop {
        if let Some(result) = result.borrow_mut().take() {
            return result.and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()));
        }
        if Instant::now() >= deadline {
            return Err("glob evaluation timed out".into());
        }
        drain_run_loop_once(run_loop);
    }
}
