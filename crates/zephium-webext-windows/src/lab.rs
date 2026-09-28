use std::collections::BTreeMap;
use std::error::Error;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::num::NonZeroIsize;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use base64::Engine as _;
use raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use webview2_com::take_pwstr;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    CallDevToolsProtocolMethodCompletedHandler, ProfileAddBrowserExtensionCompletedHandler,
    ProfileGetBrowserExtensionsCompletedHandler,
};
use windows::core::{w, Interface, HSTRING, PWSTR};
use windows::Win32::Foundation::{E_ACCESSDENIED, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use wry::{WebView, WebViewBuilder, WebViewBuilderExtWindows, WebViewExtWindows};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

struct Fixture {
    origin: String,
    denied: String,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Fixture {
    fn start() -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        let thread = std::thread::spawn(move || {
            while !signal.load(Ordering::Relaxed) {
                if let Ok((mut stream, _)) = listener.accept() {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                    let mut request = [0; 4096];
                    let _ = stream.read(&mut request);
                    let body = "<!doctype html><title>Zephium lab fixture</title><body><h1>Extension probe</h1><textarea aria-label='Editor'>This are a sentence.</textarea><a href='/second'>Second page</a></body>";
                    let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    let _ = stream.write_all(response.as_bytes());
                } else {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        });
        Ok(Self {
            origin: format!("http://127.0.0.1:{port}"),
            denied: format!("http://localhost:{port}"),
            stop,
            thread: Some(thread),
        })
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Host(HWND);
impl Host {
    fn new() -> Result<Self> {
        // SAFETY: class and callback live for the process; this owner destroys
        // the HWND after every child view has been dropped.
        unsafe {
            let module = GetModuleHandleW(None)?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: module.into(),
                lpszClassName: w!("ZephiumWebextLab"),
                ..Default::default()
            };
            RegisterClassW(&class);
            Ok(Self(CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class.lpszClassName,
                w!("Zephium extension lab"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                1000,
                800,
                None,
                None,
                Some(module.into()),
                None,
            )?))
        }
    }
}
impl HasWindowHandle for Host {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        let raw = NonZeroIsize::new(self.0 .0 as isize).ok_or(HandleError::Unavailable)?;
        // SAFETY: the borrow cannot outlive the owner of this HWND.
        Ok(
            unsafe {
                WindowHandle::borrow_raw(RawWindowHandle::Win32(Win32WindowHandle::new(raw)))
            },
        )
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        // SAFETY: unique HWND owner, dropped after child views.
        let _ = unsafe { DestroyWindow(self.0) };
    }
}
unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: delegate the unchanged Win32 callback tuple.
    unsafe { DefWindowProcW(hwnd, message, wp, lp) }
}

fn pump() {
    // SAFETY: local message storage, dispatch on the owning UI thread.
    unsafe {
        let mut message = MSG::default();
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    std::thread::sleep(Duration::from_millis(5));
}
fn wait<T>(rx: mpsc::Receiver<T>) -> Result<T> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match rx.try_recv() {
            Ok(value) => return Ok(value),
            Err(mpsc::TryRecvError::Disconnected) => return Err("callback disconnected".into()),
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if Instant::now() >= deadline {
            return Err("callback timed out".into());
        }
        pump();
    }
}
fn sleep(ms: u64) {
    let until = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < until {
        pump();
    }
}
fn emit(kind: &str, value: Value) {
    println!("{}", json!({"kind":kind,"value":value}));
}

fn profile(view: &WebView) -> Result<ICoreWebView2Profile7> {
    // SAFETY: owned COM interfaces used only on the UI apartment.
    Ok(unsafe {
        view.webview()
            .cast::<ICoreWebView2_13>()?
            .Profile()?
            .cast()?
    })
}

fn make_view(
    host: &Host,
    root: &Path,
    name: &str,
    environment: Option<&ICoreWebView2Environment>,
    enabled: bool,
) -> Result<WebView> {
    let mut context = wry::WebContext::new(Some(root.to_path_buf()));
    let expected_root = root.canonicalize()?;
    let expected_name = name.to_owned();
    let expected_environment = environment.cloned();
    let gate =
        move |env: &ICoreWebView2Environment, core: &ICoreWebView2| -> windows::core::Result<()> {
            // SAFETY: Wry invokes the gate on the controller's owning apartment,
            // before navigation. Every COM-allocated string is freed by take_pwstr.
            unsafe {
                let mut folder = PWSTR::null();
                env.cast::<ICoreWebView2Environment7>()?
                    .UserDataFolder(&mut folder)?;
                let folder = PathBuf::from(take_pwstr(folder));
                let actual = folder
                    .canonicalize()
                    .map_err(|_| windows::core::Error::from(E_ACCESSDENIED))?;
                let p = core.cast::<ICoreWebView2_13>()?.Profile()?;
                let mut profile_name = PWSTR::null();
                p.ProfileName(&mut profile_name)?;
                let actual_name = take_pwstr(profile_name);
                let mut private = windows::core::BOOL::default();
                p.IsInPrivateModeEnabled(&mut private)?;
                if actual != expected_root || actual_name != expected_name || private.as_bool() {
                    return Err(E_ACCESSDENIED.into());
                }
                if let Some(expected) = &expected_environment {
                    if env.cast::<windows::core::IUnknown>()?
                        != expected.cast::<windows::core::IUnknown>()?
                    {
                        return Err(E_ACCESSDENIED.into());
                    }
                }
            }
            Ok(())
        };
    let mut builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_profile_name(name.to_owned())
        .with_visible(false)
        .with_focused(false)
        .with_devtools(false)
        .with_autoplay(false)
        .with_additional_browser_args("--disable-features=msWebOOUI,msPdfOOUI")
        .with_bounds(wry::Rect {
            position: wry::dpi::LogicalPosition::new(0, 0).into(),
            size: wry::dpi::LogicalSize::new(950, 700).into(),
        });
    if enabled {
        builder = builder.with_browser_extension_startup_gate(gate);
    }
    if let Some(environment) = environment {
        builder = builder.with_environment(environment.clone());
    }
    let view = builder.build_as_child(host)?;
    let label = name.to_owned();
    let handler = webview2_com::ProcessFailedEventHandler::create(Box::new(move |_, args| {
        if let Some(args) = args {
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND(0);
            let mut reason = COREWEBVIEW2_PROCESS_FAILED_REASON(0);
            let mut exit = 0;
            // SAFETY: event arguments are retained by the callback during these reads.
            unsafe {
                args.ProcessFailedKind(&mut kind)?;
                if let Ok(detail) = args.cast::<ICoreWebView2ProcessFailedEventArgs2>() {
                    detail.Reason(&mut reason)?;
                    detail.ExitCode(&mut exit)?;
                }
            }
            emit(
                "process_failed",
                json!({"profile":label,"kind":kind.0,"reason":reason.0,"exit_code":exit}),
            );
        }
        Ok(())
    }));
    // SAFETY: WebView2 retains its event handler until this owned view closes.
    unsafe {
        view.webview().add_ProcessFailed(&handler, &mut 0)?;
    }
    Ok(view)
}

fn cdp(view: &WebView, method: &str, parameters: Value) -> Result<Value> {
    let (tx, rx) = mpsc::channel();
    let handler =
        CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |status, result| {
            let _ = tx.send(status.map(|()| result));
            Ok(())
        }));
    // SAFETY: parameters are retained for the synchronous call; WebView2 owns
    // a reference to the callback until completion.
    unsafe {
        view.webview().CallDevToolsProtocolMethod(
            &HSTRING::from(method),
            &HSTRING::from(parameters.to_string()),
            &handler,
        )?;
    }
    Ok(serde_json::from_str(&wait(rx)??)?)
}
fn eval(view: &WebView, script: &str) -> Result<Value> {
    cdp(
        view,
        "Runtime.evaluate",
        json!({"expression":format!("(async()=>{{{script}}})()"),"awaitPromise":true,"returnByValue":true,"timeout":15000}),
    )
}

fn extensions(view: &WebView) -> Result<Value> {
    let (tx, rx) = mpsc::channel();
    let handler =
        ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |status, list| {
            let result = (|| -> windows::core::Result<Value> {
                status?;
                let list = list.ok_or_else(|| windows::core::Error::from(E_ACCESSDENIED))?;
                let mut count = 0;
                let mut values = Vec::new();
                // SAFETY: list indices are bounded by the COM-provided count.
                unsafe {
                    list.Count(&mut count)?;
                    for i in 0..count {
                        let item = list.GetValueAtIndex(i)?;
                        let mut id = PWSTR::null();
                        let mut enabled = windows::core::BOOL::default();
                        item.Id(&mut id)?;
                        item.IsEnabled(&mut enabled)?;
                        values.push(json!({"id":take_pwstr(id),"enabled":enabled.as_bool()}));
                    }
                }
                Ok(json!(values))
            })();
            let _ = tx.send(result);
            Ok(())
        }));
    // SAFETY: callback and profile belong to the current UI apartment.
    unsafe {
        profile(view)?.GetBrowserExtensions(&handler)?;
    }
    Ok(wait(rx)??)
}

fn prepare(source: &Path, root: &Path) -> Result<PathBuf> {
    if source.is_dir() {
        return Ok(source.canonicalize()?);
    }
    let bytes = std::fs::read(source)?;
    let expected = source
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(zephium_webext::ExtensionId::parse);
    let verified = zephium_webext::crx::verify(&bytes, expected.as_ref())?;
    let destination = root.join(verified.id.as_str());
    zephium_webext::archive::extract(verified.zip, &destination, &Default::default())?;
    let manifest_path = destination.join("manifest.json");
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(&manifest_path)?)?;
    // Preserve the signed developer identity for unpacked loading. Original CRX
    // bytes are not modified, and no compatibility script is added.
    manifest["key"] = json!(base64::engine::general_purpose::STANDARD.encode(verified.public_key));
    std::fs::write(manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
    emit(
        "package",
        json!({"id":verified.id.as_str(),"version":manifest["version"],"manifest_version":manifest["manifest_version"],"sha256":format!("{:x}",Sha256::digest(&bytes)),"source":source}),
    );
    Ok(destination)
}

pub fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() == 3 && args[0] == "--prepare" {
        let root = PathBuf::from(&args[2]);
        std::fs::create_dir(&root)?;
        let folder = prepare(Path::new(&args[1]), &root)?;
        emit("prepared", json!({"folder":folder}));
        return Ok(());
    }
    if args.len() != 2 {
        return Err("usage: webext-lab-windows <scenario.json> <NEW-data-directory>".into());
    }
    let steps: Vec<Value> = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let root = PathBuf::from(&args[1]);
    std::fs::create_dir(&root)?;
    std::fs::write(
        root.join("LAB-ONLY.txt"),
        "Disposable extension probe data; never a product profile",
    )?;
    let udf = root.join("webview2");
    std::fs::create_dir(&udf)?;
    let packages = root.join("packages");
    std::fs::create_dir(&packages)?;
    let fixture = Fixture::start()?;
    let host = Host::new()?;
    let enabled = !steps.iter().any(|step| step["extensions_enabled"] == false);
    // Wry pumps construction callbacks without a deadline. Bound startup in
    // this standalone process so an unavailable desktop cannot hang the lab.
    let (startup_tx, startup_rx) = mpsc::channel();
    let startup_watchdog = std::thread::spawn(move || {
        if startup_rx.recv_timeout(Duration::from_secs(45)).is_err() {
            eprintln!("WebView2 startup timed out before the lab obtained a verified environment");
            std::process::exit(2);
        }
    });
    eprintln!(
        "Creating WebView2 environment for disposable data at {}",
        udf.display()
    );
    let first = make_view(&host, &udf, "LabHuman", None, enabled)?;
    startup_tx.send(())?;
    let _ = startup_watchdog.join();
    let environment = first.environment();
    let mut version = PWSTR::null();
    let mut pid = 0;
    // SAFETY: live environment and valid output storage.
    unsafe {
        environment.BrowserVersionString(&mut version)?;
        first.webview().BrowserProcessId(&mut pid)?;
    }
    emit(
        "environment",
        json!({"runtime":take_pwstr(version),"browser_pid":pid,"lab_pid":std::process::id(),"data":root,"extensions_enabled":enabled,"fixture":fixture.origin}),
    );
    let mut views = BTreeMap::from([("tab1".to_owned(), first)]);
    let mut last_id = String::new();
    let mut last_popup = String::new();
    let started = Instant::now();
    for (index, step) in steps.iter().enumerate() {
        let operation = (|| -> Result<Value> {
            let name = step["in"].as_str().unwrap_or("tab1");
            if let Some(ms) = step["sleep"].as_u64() {
                sleep(ms);
                return Ok(json!({"slept_ms":ms}));
            }
            if let Some(source) = step["load"].as_str() {
                let folder = prepare(Path::new(source), &packages)?;
                let manifest: Value =
                    serde_json::from_slice(&std::fs::read(folder.join("manifest.json"))?)?;
                last_popup = manifest
                    .pointer("/action/default_popup")
                    .or_else(|| manifest.pointer("/browser_action/default_popup"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                let (tx, rx) = mpsc::channel();
                let handler = ProfileAddBrowserExtensionCompletedHandler::create(Box::new(
                    move |status, item| {
                        let result = (|| -> windows::core::Result<String> {
                            status?;
                            let item =
                                item.ok_or_else(|| windows::core::Error::from(E_ACCESSDENIED))?;
                            let mut id = PWSTR::null();
                            // SAFETY: a successful callback owns the extension interface.
                            unsafe {
                                item.Id(&mut id)?;
                            }
                            Ok(take_pwstr(id))
                        })();
                        let _ = tx.send(result);
                        Ok(())
                    },
                ));
                // SAFETY: folder remains present for the entire lab run.
                unsafe {
                    profile(views.get(name).ok_or("unknown view")?)?.AddBrowserExtension(
                        &HSTRING::from(folder.to_string_lossy().as_ref()),
                        &handler,
                    )?;
                }
                last_id = wait(rx)??;
                if !Path::new(source).is_dir()
                    && folder.file_name().and_then(|s| s.to_str()) != Some(last_id.as_str())
                {
                    return Err("WebView2 ID differs from the verified CRX identity".into());
                }
                return Ok(
                    json!({"id":last_id,"name":manifest["name"],"version":manifest["version"],"manifest_version":manifest["manifest_version"],"background":manifest["background"],"permissions":manifest["permissions"],"host_permissions":manifest["host_permissions"],"popup":last_popup,"folder":folder}),
                );
            }
            if let Some(new_name) = step["view"].as_str() {
                if views.contains_key(new_name) {
                    return Err("view already exists".into());
                }
                let p = step["profile"].as_str().unwrap_or("LabHuman");
                views.insert(
                    new_name.to_owned(),
                    make_view(&host, &udf, p, Some(&environment), enabled)?,
                );
                return Ok(json!({"view":new_name,"profile":p}));
            }
            let view = views.get(name).ok_or("unknown view")?;
            let substitute = |s: &str| {
                s.replace("$ID", &last_id)
                    .replace("$ORIGIN", &fixture.origin)
                    .replace("$DENIED", &fixture.denied)
            };
            if let Some(url) = step["navigate"].as_str() {
                view.load_url(&substitute(url))?;
                return Ok(json!({"navigating":url}));
            }
            if step["popup"] == true {
                if last_popup.is_empty() {
                    return Err("extension has no default popup".into());
                }
                view.load_url(&format!("chrome-extension://{last_id}/{last_popup}"))?;
                return Ok(json!({"popup":last_popup}));
            }
            if let Some(script) = step["eval"].as_str() {
                return eval(view, &substitute(script));
            }
            if step["list"] == true {
                return extensions(view);
            }
            if let Some(filename) = step["screenshot"].as_str() {
                if Path::new(filename).file_name().and_then(|s| s.to_str()) != Some(filename) {
                    return Err("screenshot must be a filename".into());
                }
                let screenshot = cdp(view, "Page.captureScreenshot", json!({"format":"png"}))?;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(screenshot["data"].as_str().ok_or("no screenshot data")?)?;
                let path = root.join(filename);
                std::fs::write(&path, &bytes)?;
                return Ok(json!({"screenshot":path,"bytes":bytes.len()}));
            }
            if let Some(method) = step["cdp"].as_str() {
                return cdp(
                    view,
                    method,
                    step.get("params").cloned().unwrap_or(json!({})),
                );
            }
            if step.get("extensions_enabled").is_some() {
                return Ok(json!({"extensions_enabled":enabled}));
            }
            Err("unknown step".into())
        })();
        match operation {
            Ok(value) => emit(
                "step",
                json!({"index":index,"label":step["label"],"elapsed_ms":started.elapsed().as_millis(),"ok":true,"result":value}),
            ),
            Err(error) => emit(
                "step",
                json!({"index":index,"label":step["label"],"elapsed_ms":started.elapsed().as_millis(),"ok":false,"error":error.to_string()}),
            ),
        }
    }
    drop(views);
    drop(environment);
    sleep(1000);
    emit(
        "complete",
        json!({"elapsed_ms":started.elapsed().as_millis()}),
    );
    Ok(())
}
