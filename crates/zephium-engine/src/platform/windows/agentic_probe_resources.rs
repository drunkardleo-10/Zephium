#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]

//! Release-excluded, content-free WebView2 process resource sampler.
//!
//! Both physical Windows qualification adapters use this one bounded native
//! implementation. It exposes only the Environment8 process count and
//! aggregate resident working set; process identities, kinds, and handles never
//! cross this module. WebView2 explicitly excludes crashpad from this API
//! cohort, so this is not a whole-process-family measurement.

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Environment, ICoreWebView2Environment8, COREWEBVIEW2_PROCESS_KIND_BROWSER,
};
use windows_core::Interface;
use windows_probe_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_probe_sys::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
};
use windows_probe_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
use zephium_agentic::{MAX_RESOURCE_RESIDENT_BYTES, MAX_RESOURCE_WEBVIEW2_PROCESSES};

/// One bounded content-free resource observation for an exact environment.
#[derive(Clone, Copy)]
pub(crate) struct WebView2ResourceSample {
    pub(crate) processes: u8,
    pub(crate) resident_bytes: u64,
}

type ProcessCohort = [(u32, i32); MAX_RESOURCE_WEBVIEW2_PROCESSES as usize];

struct ProbeProcessHandle(HANDLE);

impl ProbeProcessHandle {
    fn open(process_id: u32) -> Option<Self> {
        // SAFETY: the nonzero PID came from WebView2's bounded process
        // snapshot; inheritance is disabled and the requested right is
        // query-only.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        (!handle.is_null()).then_some(Self(handle))
    }

    const fn raw(&self) -> HANDLE {
        self.0
    }

    fn release(&mut self) -> bool {
        if self.0.is_null() {
            return true;
        }
        let handle = std::mem::replace(&mut self.0, std::ptr::null_mut());
        // SAFETY: `open` acquired this non-null owned process handle and the
        // replacement above ensures every path attempts to close it once.
        unsafe { CloseHandle(handle) != 0 }
    }

    fn close(mut self) -> bool {
        self.release()
    }

    fn resident_bytes(&self) -> Option<u64> {
        let mut counters = PROCESS_MEMORY_COUNTERS::default();
        let counter_bytes = u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS>()).ok()?;
        counters.cb = counter_bytes;
        // SAFETY: this query-only process handle remains live; `counters` is
        // valid writable storage of exactly `counter_bytes` and no pointer is
        // retained.
        if unsafe { GetProcessMemoryInfo(self.raw(), &mut counters, counter_bytes) } == 0 {
            return None;
        }
        u64::try_from(counters.WorkingSetSize).ok()
    }
}

impl Drop for ProbeProcessHandle {
    fn drop(&mut self) {
        let _ = self.release();
    }
}

/// Samples the complete bounded Environment8 cohort for one environment.
///
/// The caller-supplied control check brackets every fallible native/COM
/// acquisition and query. Owned-handle cleanup instead runs unconditionally
/// and is followed by a control check, so cancellation cannot interrupt drain.
/// Native measurement failure is represented as `Ok(None)` so the owning
/// qualifier can emit its own closed failure shape; cancellation/deadline
/// failure is returned without erasing its type.
pub(crate) fn sample_webview2_resources<E>(
    environment: &ICoreWebView2Environment,
    check_control: &mut impl FnMut() -> Result<(), E>,
) -> Result<Option<WebView2ResourceSample>, E> {
    check_control()?;
    let environment = environment.cast::<ICoreWebView2Environment8>();
    check_control()?;
    let Ok(environment) = environment else {
        return Ok(None);
    };
    let Some((process_cohort, process_count, helper_processes)) =
        webview2_process_cohort(&environment, check_control)?
    else {
        return Ok(None);
    };
    let process_count_usize = usize::from(process_count);
    let mut process_handles: [Option<ProbeProcessHandle>;
        MAX_RESOURCE_WEBVIEW2_PROCESSES as usize] = std::array::from_fn(|_| None);
    let Some(active_handles) = process_handles.get_mut(..process_count_usize) else {
        return Ok(None);
    };
    let Some(active_process_cohort) = process_cohort.get(..process_count_usize) else {
        return Ok(None);
    };
    let mut resident_bytes = 0_u64;
    for (slot, process_id) in active_handles
        .iter_mut()
        .zip(active_process_cohort.iter().map(|entry| entry.0))
    {
        check_control()?;
        let opened_handle = ProbeProcessHandle::open(process_id);
        check_control()?;
        let Some(handle) = opened_handle else {
            return Ok(None);
        };
        let process_resident_bytes = handle.resident_bytes();
        check_control()?;
        let Some(process_resident_bytes) = process_resident_bytes else {
            return Ok(None);
        };
        let Some(total_resident_bytes) = resident_bytes.checked_add(process_resident_bytes) else {
            return Ok(None);
        };
        resident_bytes = total_resident_bytes;
        if resident_bytes > MAX_RESOURCE_RESIDENT_BYTES {
            return Ok(None);
        }
        *slot = Some(handle);
    }

    let Some((rejoined_process_cohort, rejoined_count, rejoined_helper_processes)) =
        webview2_process_cohort(&environment, check_control)?
    else {
        return Ok(None);
    };
    if rejoined_count != process_count
        || rejoined_helper_processes != helper_processes
        || rejoined_process_cohort.get(..process_count_usize) != Some(active_process_cohort)
    {
        return Ok(None);
    }
    let all_handles_closed = close_process_handles(active_handles);
    check_control()?;
    if !all_handles_closed {
        return Ok(None);
    }
    Ok((resident_bytes != 0).then_some(WebView2ResourceSample {
        processes: process_count,
        resident_bytes,
    }))
}

fn webview2_process_cohort<E>(
    environment: &ICoreWebView2Environment8,
    check_control: &mut impl FnMut() -> Result<(), E>,
) -> Result<Option<(ProcessCohort, u8, u8)>, E> {
    check_control()?;
    // SAFETY: the live COM environment owns the returned snapshot collection
    // and transfers it through the generated smart pointer.
    let processes = unsafe { environment.GetProcessInfos() };
    check_control()?;
    let Ok(processes) = processes else {
        return Ok(None);
    };
    let mut count = 0_u32;
    // SAFETY: `count` is valid writable storage and the live COM collection
    // retains no pointer to it after returning.
    let count_result = unsafe { processes.Count(&mut count) };
    check_control()?;
    if count_result.is_err() {
        return Ok(None);
    }
    let Some(process_count) = u8::try_from(count)
        .ok()
        .filter(|count| (1..=MAX_RESOURCE_WEBVIEW2_PROCESSES).contains(count))
    else {
        return Ok(None);
    };

    let mut process_cohort = [(0_u32, 0_i32); MAX_RESOURCE_WEBVIEW2_PROCESSES as usize];
    let mut browser_processes = 0_u8;
    for index in 0..count {
        check_control()?;
        // SAFETY: `index` is below the collection's just-read count; the
        // generated COM smart pointer owns the returned process-info object.
        let process = unsafe { processes.GetValueAtIndex(index) };
        check_control()?;
        let Ok(process) = process else {
            return Ok(None);
        };
        let mut kind = Default::default();
        check_control()?;
        // SAFETY: `kind` is writable stack storage and the process-info object
        // retains no pointer to it after returning.
        let kind_result = unsafe { process.Kind(&mut kind) };
        check_control()?;
        if kind_result.is_err() {
            return Ok(None);
        }
        if kind == COREWEBVIEW2_PROCESS_KIND_BROWSER {
            let Some(updated) = browser_processes.checked_add(1) else {
                return Ok(None);
            };
            browser_processes = updated;
        }
        let mut process_id = 0_i32;
        // SAFETY: `process_id` is writable stack storage and the process-info
        // object retains no pointer to it after returning.
        let process_id_result = unsafe { process.ProcessId(&mut process_id) };
        check_control()?;
        if process_id_result.is_err() {
            return Ok(None);
        }
        let Some(process_id) = u32::try_from(process_id).ok().filter(|id| *id != 0) else {
            return Ok(None);
        };
        let Some(slot) = usize::try_from(index)
            .ok()
            .and_then(|index| process_cohort.get_mut(index))
        else {
            return Ok(None);
        };
        *slot = (process_id, kind.0);
    }
    let Some(active_process_cohort) = process_cohort.get_mut(..usize::from(process_count)) else {
        return Ok(None);
    };
    active_process_cohort.sort_unstable();
    if active_process_cohort
        .windows(2)
        .any(|pair| pair[0].0 == pair[1].0)
    {
        return Ok(None);
    }
    if browser_processes != 1 {
        return Ok(None);
    }
    let Some(helper_processes) = process_count
        .checked_sub(browser_processes)
        .filter(|count| *count != 0)
    else {
        return Ok(None);
    };
    Ok(Some((process_cohort, process_count, helper_processes)))
}

fn close_process_handles(handles: &mut [Option<ProbeProcessHandle>]) -> bool {
    let mut all_closed = true;
    for handle in handles {
        if let Some(handle) = handle.take() {
            all_closed &= handle.close();
        }
    }
    all_closed
}
