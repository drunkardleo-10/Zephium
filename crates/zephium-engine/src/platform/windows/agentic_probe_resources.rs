#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]

//! Release-excluded, content-free WebView2 process resource sampler.
//!
//! Both physical Windows qualification adapters use this one bounded native
//! implementation. It exposes only an aggregate process count and resident
//! working set; process identities and handles never cross this module.

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Environment, ICoreWebView2Environment8,
};
use windows_core::Interface;
use windows_probe_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_probe_sys::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
};
use windows_probe_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
use zephium_agentic::{MAX_RESOURCE_HELPER_PROCESSES, MAX_RESOURCE_RESIDENT_BYTES};

/// One bounded content-free resource observation for an exact environment.
#[derive(Clone, Copy)]
pub(crate) struct WebView2ResourceSample {
    pub(crate) processes: u8,
    pub(crate) resident_bytes: u64,
}

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

/// Samples the complete bounded WebView2 process cohort for one environment.
///
/// The caller-supplied control check runs immediately before and after every
/// native/COM operation. Native measurement failure is represented as
/// `Ok(None)` so the owning qualifier can emit its own closed failure shape;
/// cancellation/deadline failure is returned without erasing its type.
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
    let Some((process_ids, process_count)) = webview2_process_ids(&environment, check_control)?
    else {
        return Ok(None);
    };
    let process_count_usize = usize::from(process_count);
    let mut process_handles: [Option<ProbeProcessHandle>; MAX_RESOURCE_HELPER_PROCESSES as usize] =
        std::array::from_fn(|_| None);
    let Some(active_handles) = process_handles.get_mut(..process_count_usize) else {
        return Ok(None);
    };
    let Some(active_process_ids) = process_ids.get(..process_count_usize) else {
        return Ok(None);
    };
    let mut resident_bytes = 0_u64;
    for (slot, process_id) in active_handles
        .iter_mut()
        .zip(active_process_ids.iter().copied())
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

    let Some((rejoined_process_ids, rejoined_count)) =
        webview2_process_ids(&environment, check_control)?
    else {
        return Ok(None);
    };
    if rejoined_count != process_count
        || rejoined_process_ids.get(..process_count_usize) != Some(active_process_ids)
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

fn webview2_process_ids<E>(
    environment: &ICoreWebView2Environment8,
    check_control: &mut impl FnMut() -> Result<(), E>,
) -> Result<Option<([u32; MAX_RESOURCE_HELPER_PROCESSES as usize], u8)>, E> {
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
        .filter(|count| (1..=MAX_RESOURCE_HELPER_PROCESSES).contains(count))
    else {
        return Ok(None);
    };

    let mut process_ids = [0_u32; MAX_RESOURCE_HELPER_PROCESSES as usize];
    for index in 0..count {
        check_control()?;
        // SAFETY: `index` is below the collection's just-read count; the
        // generated COM smart pointer owns the returned process-info object.
        let process = unsafe { processes.GetValueAtIndex(index) };
        check_control()?;
        let Ok(process) = process else {
            return Ok(None);
        };
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
            .and_then(|index| process_ids.get_mut(index))
        else {
            return Ok(None);
        };
        *slot = process_id;
    }
    let Some(active_process_ids) = process_ids.get_mut(..usize::from(process_count)) else {
        return Ok(None);
    };
    active_process_ids.sort_unstable();
    if active_process_ids.windows(2).any(|pair| pair[0] == pair[1]) {
        return Ok(None);
    }
    Ok(Some((process_ids, process_count)))
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
