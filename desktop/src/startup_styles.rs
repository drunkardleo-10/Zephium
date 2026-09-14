//! Development-only, content-free probes of the trusted main frame's stylesheet state.
//! This is diagnostic evidence only; it does not change the startup admission gate.
use serde::Deserialize;
use std::sync::atomic::{AtomicU8, Ordering};
static STAGES: AtomicU8 = AtomicU8::new(0);
static RESPONSES: AtomicU8 = AtomicU8::new(0);
use tauri::{Manager, WebviewWindow};

pub(crate) const SCRIPT: &str = include_str!("startup_styles.js");

pub(crate) fn stylesheet_response(
    request: &tauri::http::Request<Vec<u8>>,
    response: &tauri::http::Response<std::borrow::Cow<'static, [u8]>>,
) {
    if !request.uri().path().ends_with(".css")
        || RESPONSES
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                (count < 64).then_some(count + 1)
            })
            .is_err()
    {
        return;
    }
    let css_mime = response
        .headers()
        .get(tauri::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(';').next() == Some("text/css"));
    crate::work_diagnostics::record(format_args!(
        "startup-styles response_status={} bytes={} css_mime={css_mime}",
        response.status().as_u16(),
        response.body().len(),
    ));
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sheet {
    ready: bool,
    disabled: bool,
    rules: bool,
    inaccessible: bool,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    loaded: u8,
    failed: u8,
    expected: u8,
    sheets: Vec<Sheet>,
    token: bool,
    flex: bool,
    overflow_hidden: bool,
    sized: bool,
    visible: bool,
    complete: bool,
}

fn decode(value: &str) -> Option<Report> {
    (value.len() <= 16_384)
        .then(|| serde_json::from_str::<String>(value).ok())
        .flatten()
        .and_then(|json| serde_json::from_str::<Report>(&json).ok())
        .filter(|report| report.sheets.len() <= 64)
}

pub(crate) fn capture(window: &WebviewWindow, expected: &tauri::Url, stage: &'static str) {
    if window.label() != "main" || !window.url().is_ok_and(|url| &url == expected) {
        return;
    }
    let bit = match stage {
        "document-loaded" => 1,
        "frontend-ready" => 2,
        "shown" => 4,
        "shown-plus-one-second" => 8,
        _ => return,
    };
    if STAGES.fetch_or(bit, Ordering::AcqRel) & bit != 0 {
        return;
    }
    let result = window.eval_with_callback(
        "window.__zephiumStartupStyles?.() ?? null",
        move |value| {
            let report = decode(&value);
            if let Some(report) = report {
                let ready = report.sheets.iter().filter(|sheet| sheet.ready).count();
                let rules = report.sheets.iter().filter(|sheet| sheet.rules).count();
                let disabled = report.sheets.iter().filter(|sheet| sheet.disabled).count();
                let inaccessible = report.sheets.iter().filter(|sheet| sheet.inaccessible).count();
                crate::work_diagnostics::record(format_args!(
                    "startup-styles stage={stage} expected={} ready={ready} rules={rules} disabled={disabled} inaccessible={inaccessible} loaded={} failed={} token={} flex={} overflow_hidden={} sized={} visible={} complete={}",
                    report.expected, report.loaded, report.failed, report.token, report.flex,
                    report.overflow_hidden, report.sized, report.visible, report.complete,
                ));
            } else {
                crate::work_diagnostics::record(format_args!("startup-styles stage={stage} report=unavailable"));
            }
        },
    );
    if result.is_err() {
        crate::work_diagnostics::record(format_args!(
            "startup-styles stage={stage} dispatch=unavailable"
        ));
    }
}

pub(crate) fn after_show(window: &WebviewWindow, expected: &tauri::Url) {
    capture(window, expected, "shown");
    let app = window.app_handle().clone();
    let expected = expected.clone();
    let _ = std::thread::Builder::new()
        .name("startup-style-observation".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let callback_app = app.clone();
            let _ = app.run_on_main_thread(move || {
                if let Some(window) = callback_app.get_webview_window("main") {
                    capture(&window, &expected, "shown-plus-one-second");
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_bounded_content_free_reports_are_admitted() {
        let mut report = serde_json::json!({"loaded": 1, "failed": 0, "expected": 1,
            "sheets": [{"ready": true, "disabled": false, "rules": true, "inaccessible": false}],
            "token": true, "flex": true, "overflow_hidden": true, "sized": true, "visible": true, "complete": true});
        let encoded =
            |value: &serde_json::Value| serde_json::to_string(&value.to_string()).unwrap();
        assert!(decode(&encoded(&report)).is_some());
        report["title"] = serde_json::json!("must never enter diagnostics");
        assert!(decode(&encoded(&report)).is_none());
        report.as_object_mut().unwrap().remove("title");
        report["sheets"] = serde_json::json!(vec![report["sheets"][0].clone(); 65]);
        assert!(decode(&encoded(&report)).is_none());
    }
}
