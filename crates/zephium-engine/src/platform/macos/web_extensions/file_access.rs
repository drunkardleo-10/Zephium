//! Live post-WebView file-access capability gate.
//!
//! WebKit permission readback is not sufficient evidence for local-file
//! execution. This probe creates fresh contexts only after the product WebView
//! exists and proves the current platform ceiling repeatedly: WebKit accepts
//! the file match-pattern grant but does not execute the declared isolated
//! content script. A future runtime that begins executing fails this gate so
//! product availability must be reviewed intentionally.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use objc2::rc::Weak;
use objc2::runtime::AnyObject;
use objc2_foundation::{MainThreadMarker, NSError, NSRunLoop, NSString, NSURL};
use objc2_web_kit::{WKWebExtension, WKWebExtensionContext, WKWebExtensionController, WKWebView};
use serde_json::{json, Value};

use super::{
    drain_run_loop_once, format_native_error, load_context, new_context, unload_context,
    PROBE_TIMEOUT,
};

const FILE_MATCH_PATTERN: &str = "file:///*";
const CONTEXT_REPETITIONS: usize = 3;
const NEGATIVE_SETTLE: Duration = Duration::from_millis(500);

pub(super) struct FileAccessFixture {
    pub(super) extension_root: PathBuf,
    pub(super) page: PathBuf,
}

pub(super) struct FileAccessEvidence {
    pub(super) contexts: Vec<Weak<WKWebExtensionContext>>,
    pub(super) dynamic_contexts: usize,
}

pub(super) fn write_fixture(root: &Path) -> Result<FileAccessFixture, String> {
    let extension_root = root.join("file-access-extension");
    std::fs::create_dir(&extension_root)
        .map_err(|error| format!("cannot create file-access extension directory: {error}"))?;
    let manifest = json!({
        "manifest_version": 3,
        "name": "Zephium Dynamic File Access Probe",
        "description": "Feature-gated post-WebView file execution fixture.",
        "version": "1.0.0",
        "host_permissions": ["<all_urls>"],
        "content_scripts": [{
            "matches": ["<all_urls>"],
            "js": ["file.js"],
            "run_at": "document_start"
        }]
    });
    std::fs::write(extension_root.join("manifest.json"), manifest.to_string())
        .map_err(|error| format!("cannot write file-access manifest: {error}"))?;
    std::fs::write(
        extension_root.join("file.js"),
        r#"(() => {
          'use strict';
          const root = document.documentElement;
          if (!root) return;
          const current = Number(root.getAttribute('data-zephium-file-access') || '0');
          root.setAttribute('data-zephium-file-access', String(current + 1));
          Object.defineProperty(globalThis, '__zephiumFileAccessWorld', {
            value: 'isolated', configurable: false, enumerable: false, writable: false
          });
        })();"#,
    )
    .map_err(|error| format!("cannot write file-access content script: {error}"))?;
    let page = root.join("file-access-page.html");
    std::fs::write(
        &page,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>file access</title></head><body><main>File access probe</main></body></html>",
    )
    .map_err(|error| format!("cannot write file-access page: {error}"))?;
    Ok(FileAccessFixture {
        extension_root,
        page,
    })
}

pub(super) fn run(
    extension: &WKWebExtension,
    controller: &WKWebExtensionController,
    view: &WKWebView,
    fixture: &FileAccessFixture,
    run_loop: &NSRunLoop,
    _mtm: MainThreadMarker,
) -> Result<FileAccessEvidence, String> {
    let mut contexts = Vec::with_capacity(CONTEXT_REPETITIONS);
    for iteration in 0..CONTEXT_REPETITIONS {
        let identifier = format!("zephium-dynamic-file-access-{iteration}");
        let context = new_context(extension, &identifier)?;
        contexts.push(Weak::from_retained(&context));
        let applied = super::super::extensions::apply_probe_grants(
            &context,
            &[],
            &[FILE_MATCH_PATTERN],
            false,
        )
        .map_err(|error| format!("dynamic file grant application failed: {error}"))?;
        load_context(controller, &context, "dynamic file-access context")?;
        navigate_and_observe(view, &fixture.page, run_loop, false)?;

        unload_context(controller, &context, "dynamic file-access context")?;
        applied
            .clear_and_verify(&context)
            .map_err(|error| format!("dynamic file grant cleanup failed: {error}"))?;
        drop(applied);
        drop(context);
        navigate_and_observe(view, &fixture.page, run_loop, false)?;
    }
    Ok(FileAccessEvidence {
        contexts,
        dynamic_contexts: CONTEXT_REPETITIONS,
    })
}

fn navigate_and_observe(
    view: &WKWebView,
    file: &Path,
    run_loop: &NSRunLoop,
    expected: bool,
) -> Result<(), String> {
    let file = file
        .to_str()
        .ok_or_else(|| "file-access probe path is not UTF-8".to_owned())?;
    let root = Path::new(file)
        .parent()
        .and_then(Path::to_str)
        .ok_or_else(|| "file-access probe root is invalid".to_owned())?;
    let file_url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(file), false);
    let root_url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(root), true);
    // SAFETY: the view and file URLs are main-thread-only retained values. The
    // exact temporary fixture directory is the only allowed read scope.
    unsafe {
        view.loadFileURL_allowingReadAccessToURL(&file_url, &root_url);
    }
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut ready_without_execution_since = None;
    let mut last_error = None;
    loop {
        drain_run_loop_once(run_loop);
        match evaluate(view, run_loop, deadline) {
            Ok(state) if state.get("ready").and_then(Value::as_str) == Some("complete") => {
                let marker = state.get("marker").and_then(Value::as_u64).unwrap_or(0);
                if state.get("pageWorldLeak").and_then(Value::as_bool) != Some(false) {
                    return Err(format!("file content world leaked: {state}"));
                }
                if expected && marker > 0 {
                    return Ok(());
                }
                if !expected {
                    if marker > 0 {
                        return Err(format!(
                            "file content executed without effective authority: {state}"
                        ));
                    }
                    let ready_since =
                        ready_without_execution_since.get_or_insert_with(Instant::now);
                    if ready_since.elapsed() >= NEGATIVE_SETTLE {
                        return Ok(());
                    }
                }
            }
            Ok(_) => ready_without_execution_since = None,
            Err(error) => last_error = Some(error),
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "file execution did not settle{}",
                last_error
                    .map(|error| format!(": {error}"))
                    .unwrap_or_default()
            ));
        }
    }
}

fn evaluate(view: &WKWebView, run_loop: &NSRunLoop, deadline: Instant) -> Result<Value, String> {
    let result = Rc::new(RefCell::new(None));
    let callback_result = result.clone();
    let completion = block2::RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
        let value = if let Some(error) = unsafe { error.as_ref() } {
            Err(format_native_error("file execution evaluation", error))
        } else {
            unsafe { value.as_ref() }
                .and_then(AnyObject::downcast_ref::<NSString>)
                .map(ToString::to_string)
                .ok_or_else(|| "file execution evaluation returned a non-string".to_owned())
        };
        callback_result.replace(Some(value));
    });
    let script = NSString::from_str(
        "JSON.stringify({ready:document.readyState,marker:Number(document.documentElement?.getAttribute('data-zephium-file-access')||'0'),pageWorldLeak:typeof globalThis.__zephiumFileAccessWorld!=='undefined'})",
    );
    // SAFETY: main-thread affinity is maintained by the probe. WebKit copies
    // the source and block for its asynchronous completion.
    unsafe {
        view.evaluateJavaScript_completionHandler(&script, Some(&completion));
    }
    loop {
        if let Some(result) = result.borrow_mut().take() {
            return result.and_then(|encoded| {
                serde_json::from_str(&encoded)
                    .map_err(|_| "file execution evaluation returned invalid JSON".to_owned())
            });
        }
        if Instant::now() >= deadline {
            return Err("file execution evaluation did not settle".into());
        }
        drain_run_loop_once(run_loop);
    }
}
