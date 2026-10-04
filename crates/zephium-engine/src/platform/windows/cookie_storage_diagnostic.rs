//! Fixed, content-free cookie storage facts in the release-excluded Work qualifier.
#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]

use std::{
    cell::{Cell, RefCell},
    io::Write as _,
    rc::Rc,
    time::{Duration, Instant},
};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2CallDevToolsProtocolMethodCompletedHandler,
    ICoreWebView2CallDevToolsProtocolMethodCompletedHandler_Impl,
};
use windows_core::{HRESULT, HSTRING, PCWSTR};

const NAMES: [&str; 2] = ["Cookie.LoadProblem", "Cookie.CommitProblem"];
type Histogram = (u64, i64, Vec<(i64, i64, u64)>);
#[derive(Debug)]
enum HistogramFailure {
    Native(i32),
    MissingOrOversizedReply,
    Schema,
}
type Response = Rc<RefCell<Option<Result<Histogram, HistogramFailure>>>>;

pub(crate) fn observe(core: &ICoreWebView2) {
    let _ = writeln!(
        std::io::stderr().lock(),
        "windows-work-cookies: stage=host_token restricted_elevated={:?}; content=redacted",
        host_token()
    );
    let deadline = Instant::now() + Duration::from_millis(500);
    for name in NAMES {
        let response = Rc::new(RefCell::new(None));
        let closed = Rc::new(Cell::new(false));
        let handler: ICoreWebView2CallDevToolsProtocolMethodCompletedHandler = Completion {
            name,
            response: response.clone(),
            closed: closed.clone(),
        }
        .into();
        let method = HSTRING::from("Browser.getHistogram");
        let parameters = HSTRING::from(format!("{{\"name\":\"{name}\",\"delta\":false}}"));
        // SAFETY: this is the exact STA-owned controller and a fixed browser diagnostic method;
        // immutable buffers live through the call and COM owns the bounded callback afterward.
        let dispatched =
            unsafe { core.CallDevToolsProtocolMethod(&method, &parameters, &handler) }.is_ok();
        if dispatched {
            while response.borrow().is_none() && Instant::now() < deadline {
                if !super::pump_browser_exit_callbacks(deadline) {
                    break;
                }
            }
        }
        closed.set(true);
        let observed = response.borrow_mut().take().map(|v| {
            v.map_err(|e| match e {
                HistogramFailure::Native(code) => ("native", Some(code)),
                HistogramFailure::MissingOrOversizedReply => ("reply_bound", None),
                HistogramFailure::Schema => ("schema", None),
            })
        });
        let _ = writeln!(std::io::stderr().lock(), "windows-work-cookies: stage=storage_histogram name={name} dispatched={dispatched} facts={observed:?}; content=redacted");
    }
}

#[windows_core::implement(ICoreWebView2CallDevToolsProtocolMethodCompletedHandler)]
struct Completion {
    name: &'static str,
    response: Response,
    closed: Rc<Cell<bool>>,
}
impl ICoreWebView2CallDevToolsProtocolMethodCompletedHandler_Impl for Completion_Impl {
    fn Invoke(&self, result: HRESULT, response: &PCWSTR) -> windows_core::Result<()> {
        if self.closed.get() {
            return Ok(());
        }
        let observed = if result.is_err() {
            Err(HistogramFailure::Native(result.0))
        } else {
            bounded(response)
                .ok_or(HistogramFailure::MissingOrOversizedReply)
                .and_then(|s| parse(self.name, &s).map_err(|()| HistogramFailure::Schema))
        };
        if let Ok(mut slot) = self.response.try_borrow_mut() {
            if slot.is_none() {
                *slot = Some(observed);
            }
        }
        Ok(())
    }
}
fn bounded(raw: &PCWSTR) -> Option<String> {
    let p = raw.as_ptr();
    if p.is_null() {
        return None;
    }
    for len in 0..=8192 {
        // SAFETY: WebView2 supplies a terminated UTF16 string valid throughout this callback;
        // the fixed scan ceiling prevents unbounded diagnostic allocations.
        if unsafe { p.add(len).read() } == 0 {
            // SAFETY: the bounded scan proved the initialized prefix of the callback's string.
            let text = String::from_utf16(unsafe { std::slice::from_raw_parts(p, len) }).ok()?;
            return (text.len() <= 8192).then_some(text);
        }
    }
    None
}
fn parse(name: &str, raw: &str) -> Result<Histogram, ()> {
    let root: serde_json::Value = serde_json::from_str(raw).map_err(|_| ())?;
    let h = root.get("histogram").ok_or(())?;
    if h.get("name").and_then(|v| v.as_str()) != Some(name) {
        return Err(());
    }
    let count = h.get("count").and_then(|v| v.as_u64()).ok_or(())?;
    let sum = h.get("sum").and_then(|v| v.as_i64()).ok_or(())?;
    let bins = h.get("buckets").and_then(|v| v.as_array()).ok_or(())?;
    if count > 1_000_000 || sum.unsigned_abs() > 16_000_000 || bins.len() > 16 {
        return Err(());
    }
    let mut buckets = Vec::with_capacity(bins.len());
    for b in bins {
        let low = b.get("low").and_then(|v| v.as_i64()).ok_or(())?;
        let high = b.get("high").and_then(|v| v.as_i64()).ok_or(())?;
        let samples = b.get("count").and_then(|v| v.as_u64()).ok_or(())?;
        if low < -1 || high <= low || samples > count {
            return Err(());
        }
        buckets.push((low, high, samples));
    }
    Ok((count, sum, buckets))
}
fn host_token() -> Result<(bool, bool), ()> {
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{
            GetTokenInformation, IsTokenRestricted, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    let mut token = HANDLE::default();
    // SAFETY: current process is live and the initialized output receives a unique query-only token handle.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.map_err(|_| ())?;
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0;
    // SAFETY: token is queryable and the exact typed buffer/byte length are valid writable outputs.
    let observed = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    // SAFETY: the query-only token handle remains live until CloseHandle below.
    let restricted = unsafe { IsTokenRestricted(token) }.is_ok();
    // SAFETY: successful OpenProcessToken transferred this uniquely owned handle; it is closed exactly once.
    let _ = unsafe { CloseHandle(token) };
    observed.map_err(|_| ())?;
    if returned as usize != std::mem::size_of::<TOKEN_ELEVATION>() {
        return Err(());
    }
    Ok((restricted, elevation.TokenIsElevated != 0))
}

pub(crate) fn seed_initialized() {
    let _ = writeln!(std::io::stderr().lock(), "windows-work-cookies: stage=seed_admission exact_origin_initialized=true; content=redacted");
}
pub(crate) fn observe_profile(core: &ICoreWebView2, expected_udf: &std::path::Path) {
    use std::io::Write as _;
    let observed = (|| -> Result<_, ()> {
        let profile = controller_profile(core)?;
        let mut raw = windows_core::PWSTR::null();
        // SAFETY: this exact owned controller supplies an allocated native
        // profile path; bounded ownership conversion frees it on every path.
        unsafe { profile.ProfilePath(&mut raw) }.map_err(|_| ())?;
        let path = super::take_pwstr_bounded(raw, 32768, 65536).ok_or(())?;
        let path = std::fs::canonicalize(path).map_err(|_| ())?;
        let expected = std::fs::canonicalize(expected_udf).map_err(|_| ())?;
        if !path.starts_with(&expected) {
            return Err(());
        }
        let cookie_file = |relative: &str| -> Result<Option<u64>, std::io::ErrorKind> {
            match std::fs::metadata(path.join(relative)) {
                Ok(metadata) if metadata.is_file() => Ok(Some(metadata.len())),
                Ok(_) => Err(std::io::ErrorKind::InvalidData),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error.kind()),
            }
        };
        use sha2::{Digest, Sha256};
        use std::os::windows::ffi::OsStrExt as _;
        let path_hash = format!("{:x}", Sha256::digest(path.to_string_lossy().as_bytes()));
        let udf_hash = format!(
            "{:x}",
            Sha256::digest(expected.to_string_lossy().as_bytes())
        );
        let lengths = (
            path.as_os_str().encode_wide().count(),
            path.join("Cookies").as_os_str().encode_wide().count(),
            path.join("Network/Cookies")
                .as_os_str()
                .encode_wide()
                .count(),
        );
        Ok((
            path_hash,
            udf_hash,
            lengths,
            profile_is_private(&profile),
            cookie_file("Cookies"),
            cookie_file("Network/Cookies"),
        ))
    })();
    let _ = writeln!(std::io::stderr().lock(), "windows-work-cookies: stage=owned_disk_metadata binding_and_files={observed:?}; content=redacted");
    observe(core);
}
fn controller_profile(
    core: &ICoreWebView2,
) -> Result<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Profile, ()> {
    use windows_core::Interface as _;
    let core = core
        .cast::<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_13>()
        .map_err(|_| ())?;
    // SAFETY: the exact STA-owned core returns an AddRef'd native profile.
    unsafe { core.Profile() }.map_err(|_| ())
}
fn profile_is_private(
    profile: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Profile,
) -> Result<bool, ()> {
    let mut private = windows_core::BOOL::default();
    // SAFETY: the live profile and initialized exact-typed native output belong to this STA.
    unsafe { profile.IsInPrivateModeEnabled(&mut private) }.map_err(|_| ())?;
    Ok(private.as_bool())
}

/// Closed native shutdown facts; no profile path or page content is observed.
pub(crate) fn shutdown_admission(flags: [bool; 10], counts: [Option<usize>; 6]) {
    let _ = writeln!(std::io::stderr().lock(), "windows-work-shutdown: admission_empty_unverifiable_construction_process_environment_cleanup_healthy_accounting_ledger_provenance={flags:?} ledger_tab_spare_debt_agent_extension_transient={counts:?}; content=redacted");
}
pub(crate) fn shutdown_wait(
    process_id: u32,
    proof_exited: bool,
    proof_invalid: bool,
    handle_exited: bool,
) {
    let _ = writeln!(std::io::stderr().lock(), "windows-work-shutdown: process_id={process_id} proof_exited={proof_exited} proof_invalid={proof_invalid} handle_exited={handle_exited}; content=redacted");
}

#[cfg(test)]
mod tests {
    use super::parse;
    #[test]
    fn fixed_histogram_parser_keeps_only_closed_counts_and_rejects_other_names() {
        let raw = r#"{"histogram":{"name":"Cookie.LoadProblem","count":1,"sum":3,"buckets":[{"low":3,"high":4,"count":1}]}}"#;
        assert_eq!(
            parse("Cookie.LoadProblem", raw),
            Ok((1, 3, vec![(3, 4, 1)]))
        );
        assert_eq!(parse("Cookie.CommitProblem", raw), Err(()));
    }
}
