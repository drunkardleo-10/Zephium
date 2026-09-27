//! Public-API, nonpersistent native offscreen qualification. No product
//! admission, private preferences, network requests or user profile is used.
use super::*;
use objc2_foundation::{NSRect, NSSize, NSURLRequest};
use objc2_web_kit::WKWebExtensionContextPermissionStatus;

const SCRIPT: &str = r#"
(async () => {
 const api = globalThis.browser ?? globalThis.chrome;
 const run = ((await api.storage.local.get('probeRun')).probeRun || 0) + 1;
 await api.storage.local.set({probeRun:run, probeResult:null});
 const assert = (value, message) => { if (!value) throw Error(message); };
 try {
  assert(api && api.offscreen, 'offscreen namespace unavailable');
  assert(typeof api.offscreen.createDocument === 'function', 'createDocument unavailable');
  assert(typeof api.offscreen.closeDocument === 'function', 'closeDocument unavailable');
  const contexts = typeof api.runtime.getContexts === 'function';
  const has = async () => typeof api.offscreen.hasDocument === 'function'
    ? await api.offscreen.hasDocument()
    : contexts ? (await api.runtime.getContexts({contextTypes:['OFFSCREEN_DOCUMENT']})).length !== 0
    : (() => {throw Error('offscreen discovery unavailable')})();
  assert(!await has(), 'offscreen document survived unload or existed before creation');
  let rejected = false;
  try { await api.offscreen.createDocument({url:'data:text/html,not-packaged', reasons:['DOM_PARSER'], justification:'Origin rejection probe'}); }
  catch (_) { rejected = true; }
  assert(rejected && !await has(), 'unpackaged document was admitted');
  const parameters = {url:'offscreen.html', reasons:['DOM_PARSER'], justification:'Zephium native DOM and messaging qualification'};
  await api.offscreen.createDocument(parameters);
  assert(await has(), 'created document is not discoverable');
  rejected = false;
  try { await api.offscreen.createDocument(parameters); } catch (_) { rejected = true; }
  assert(rejected, 'second concurrent document was admitted');
  const reply = await api.runtime.sendMessage({zephiumOffscreenProbe:true});
  assert(reply?.text === 'native-dom', 'offscreen DOM/message reply failed');
  assert(reply.openerNull, 'offscreen document has an opener');
  assert(reply.storageAbsent && reply.tabsAbsent && reply.scriptingAbsent, 'offscreen exposes non-runtime extension APIs');
  if (contexts) {
   const items = await api.runtime.getContexts({contextTypes:['OFFSCREEN_DOCUMENT']});
   assert(items.length === 1 && items[0].documentUrl === api.runtime.getURL('offscreen.html'), 'offscreen context identity mismatch');
  }
  await api.offscreen.closeDocument();
  assert(!await has(), 'closed document remains discoverable');
  // Leave one open so the native unload/reload pass must clean it up.
  await api.offscreen.createDocument(parameters);
  await api.storage.local.set({probeResult:{ok:true, run, contexts, runtimeOnly:true, singleton:true}});
 } catch(error) {
  await api.storage.local.set({probeResult:{ok:false,run,error:String(error.message)}});
 }
})();
"#;
const DOCUMENT: &str = r#"
const api = globalThis.browser ?? globalThis.chrome;
api.runtime.onMessage.addListener((message, _sender, respond) => {
 if (!message?.zephiumOffscreenProbe) return;
 respond({text:new DOMParser().parseFromString('<p>native-dom</p>', 'text/html').body.textContent,
  openerNull:window.opener === null, storageAbsent:typeof api.storage === 'undefined',
  tabsAbsent:typeof api.tabs === 'undefined', scriptingAbsent:typeof api.scripting === 'undefined'});
});
"#;

const SANDBOX_SCRIPT: &str = r#"
const frame = document.createElement('iframe');
frame.sandbox = 'allow-scripts';
addEventListener('message', event => {
 if (event.source !== frame.contentWindow || !event.data?.sandboxProbe) return;
 const result = {...event.data, ok:event.origin === 'null' && event.data.apiAbsent && event.data.storageBlocked};
 frame.remove();
 document.title = 'ZEPHIUM_OFFSCREEN:' + JSON.stringify(result);
}, {once:true});
frame.src = 'sandbox.html';
document.documentElement.append(frame);
"#;
const SANDBOX_DOCUMENT: &str = r#"
(async () => {
 let storageBlocked = false;
 try { localStorage.getItem('test'); } catch (_) { storageBlocked = true; }
 const api = globalThis.browser ?? globalThis.chrome;
 let extensionStorageWritable = false;
 try {
  await api.storage.local.set({sandboxProbeValue:123});
  extensionStorageWritable = (await api.storage.local.get('sandboxProbeValue')).sandboxProbeValue === 123;
  await api.storage.local.remove('sandboxProbeValue');
 } catch (_) {}
 parent.postMessage({sandboxProbe:true, tabs:typeof api?.tabs?.create, storage:typeof api?.storage?.local?.get, runtime:typeof api?.runtime?.sendMessage, apiAbsent:typeof globalThis.chrome === 'undefined' && typeof globalThis.browser === 'undefined', storageBlocked, extensionStorageWritable, dom:new DOMParser().parseFromString('<p>dom</p>', 'text/html').body.textContent}, '*');
})();
"#;

pub(super) fn sandbox() -> Result<bool, String> {
    let Some(os) = supported_runtime()? else {
        return Ok(false);
    };
    let watchdog = arm_process_watchdog();
    let result = run_inner(&os, true);
    watchdog.store(true, Ordering::Release);
    result.map(|()| true)
}
pub(super) fn run() -> Result<bool, String> {
    let Some(os) = supported_runtime()? else {
        return Ok(false);
    };
    let watchdog = arm_process_watchdog();
    let result = run_inner(&os, false);
    watchdog.store(true, Ordering::Release);
    result.map(|()| true)
}
fn run_inner(os: &str, sandbox: bool) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("offscreen probe requires the main thread")?;
    let app = NSApplication::sharedApplication(mtm);
    let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.finishLaunching();
    let directory = tempfile::Builder::new()
        .prefix("zephium-offscreen-")
        .tempdir()
        .map_err(|error| error.to_string())?;
    for (name, body) in [
        (
            "manifest.json",
            r#"{"manifest_version":3,"name":"Zephium offscreen probe","description":"Public API qualification using synthetic data.","version":"1.0","permissions":["offscreen","storage"],"background":{"service_worker":"background.js"}}"#,
        ),
        (
            "probe.html",
            "<!doctype html><head><script src='probe.js'></script></head>",
        ),
        ("background.js", SCRIPT),
        (
            "probe.js",
            r#"(async()=>{const api=globalThis.browser??globalThis.chrome;const expected=Number(new URL(location.href).searchParams.get('pass'))+1;for(let i=0;i<200;i++){const {probeResult:r}=await api.storage.local.get('probeResult');if(r?.run===expected){document.title='ZEPHIUM_OFFSCREEN:'+JSON.stringify(r);return;}await new Promise(resolve=>setTimeout(resolve,25));}document.title='ZEPHIUM_OFFSCREEN:'+JSON.stringify({ok:false,error:'worker result unavailable'});})();"#,
        ),
        (
            "offscreen.html",
            "<!doctype html><head><script src='offscreen.js'></script></head>",
        ),
        ("offscreen.js", DOCUMENT),
    ] {
        std::fs::write(directory.path().join(name), body).map_err(|error| error.to_string())?;
    }
    if sandbox {
        std::fs::write(directory.path().join("probe.js"), SANDBOX_SCRIPT)
            .map_err(|error| error.to_string())?;
        std::fs::write(
            directory.path().join("sandbox.html"),
            "<!doctype html><head><script src='sandbox.js'></script></head>",
        )
        .map_err(|error| error.to_string())?;
        std::fs::write(directory.path().join("sandbox.js"), SANDBOX_DOCUMENT)
            .map_err(|error| error.to_string())?;
    }
    let run_loop = NSRunLoop::mainRunLoop();
    let (context_weak, controller_weak, views, outcome) = objc2::rc::autoreleasepool(|_| {
        let bundle = new_nonpersistent_controller(mtm)?;
        let extension = load_extension(directory.path(), &run_loop, mtm)?;
        let errors = unsafe { extension.errors() };
        if errors.count() != 0 {
            return Err(format!(
                "offscreen manifest: {}",
                describe_native_errors(&errors)
            ));
        }
        let context = new_context(&extension, "zephium-native-offscreen-probe")?;
        unsafe {
            context.setHasAccessToPrivateData(true);
            context.setPermissionStatus_forPermission(
                WKWebExtensionContextPermissionStatus::GrantedExplicitly,
                &NSString::from_str("offscreen"),
            );
            context.setPermissionStatus_forPermission(
                WKWebExtensionContextPermissionStatus::GrantedExplicitly,
                &NSString::from_str("storage"),
            );
        }
        let mut views = Vec::with_capacity(2);
        let mut outcome = Ok(());
        for pass in 0..2 {
            load_context(&bundle.controller, &context, "offscreen probe")?;
            if !sandbox
                && unsafe {
                    context.permissionStatusForPermission(&NSString::from_str("offscreen"))
                } != WKWebExtensionContextPermissionStatus::GrantedExplicitly
            {
                outcome = Err("offscreen native permission is unavailable on this runtime".into());
                let _ = unsafe { bundle.controller.unloadExtensionContext_error(&context) };
                break;
            }
            if pass == 0 {
                println!(
                    "native-offscreen: webkit-extension-built-in-scheme={}",
                    unsafe {
                        WKWebView::handlesURLScheme(&NSString::from_str("webkit-extension"), mtm)
                    }
                );
            }
            if !sandbox {
                super::persistent_runtime::load_background_content(
                    &context,
                    &run_loop,
                    "offscreen worker",
                )?;
            }
            let configuration = unsafe { context.webViewConfiguration() }
                .ok_or("missing extension-page configuration")?;
            let view = unsafe {
                WKWebView::initWithFrame_configuration(
                    WKWebView::alloc(mtm),
                    NSRect::new(NSPoint::new(0., 0.), NSSize::new(320., 200.)),
                    &configuration,
                )
            };
            views.push(Weak::from_retained(&view));
            let base = unsafe { context.baseURL() }
                .absoluteString()
                .ok_or("missing base URL")?
                .to_string();
            let url = NSURL::URLWithString(&NSString::from_str(&format!(
                "{base}probe.html?pass={pass}"
            )))
            .ok_or("invalid probe URL")?;
            unsafe {
                view.loadRequest(&NSURLRequest::requestWithURL(&url));
            }
            outcome = wait_for_title(&view, &run_loop).and_then(|value| {
                let result: Value =
                    serde_json::from_str(&value).map_err(|error| error.to_string())?;
                if result["ok"] != true {
                    return Err(format!("native offscreen pass {pass}: {result}"));
                }
                println!("native-offscreen: pass={pass}; {result}");
                Ok(())
            });
            unsafe {
                view.stopLoading();
            }
            let unloaded = unsafe { bundle.controller.unloadExtensionContext_error(&context) }
                .map_err(|error| format_native_error("offscreen unload", &error));
            if outcome.is_ok() {
                outcome = unloaded;
            }
            drop(view);
            if outcome.is_err() {
                break;
            }
        }
        Ok::<_, String>((
            Weak::from_retained(&context),
            Weak::from_retained(&bundle.controller),
            views,
            outcome,
        ))
    })?;
    let deadline = Instant::now() + TEARDOWN_TIMEOUT;
    loop {
        let released = objc2::rc::autoreleasepool(|_| {
            context_weak.load().is_none()
                && controller_weak.load().is_none()
                && views.iter().all(|view| view.load().is_none())
        });
        if released {
            break;
        }
        if Instant::now() >= deadline {
            return Err("offscreen probe native teardown was not proven".into());
        }
        drain_run_loop_once(&run_loop);
    }
    if let Err(error) = outcome {
        return Err(format!(
            "{error}; native-objects-released=passed; product-authority=false"
        ));
    }
    println!("native-offscreen: os={os}; sandbox={sandbox}; scenario-and-unload-reload=passed; native-objects-released=passed; product-authority=false");
    Ok(())
}
fn wait_for_title(view: &WKWebView, run_loop: &NSRunLoop) -> Result<String, String> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        if let Some(title) = unsafe { view.title() } {
            if let Some(result) = title.to_string().strip_prefix("ZEPHIUM_OFFSCREEN:") {
                return Ok(result.to_owned());
            }
        }
        if Instant::now() >= deadline {
            return Err("offscreen native script did not settle".into());
        }
        drain_run_loop_once(run_loop);
    }
}
