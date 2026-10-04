//! OS-driven residency input. No process sampling or periodic memory polling.
#[cfg(any(target_os = "macos", target_os = "windows"))]
use tauri::Manager;

pub(crate) fn install(app: &tauri::AppHandle) {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    if let Some(shell) = app.try_state::<zephium_app::Handle>() {
        if let Some(monitor) = native::Monitor::new(shell.inner().clone()) {
            app.manage(monitor);
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = app;
}

#[cfg(target_os = "macos")]
mod native {
    use dispatch2::{DispatchObject, DispatchRetained, DispatchSource};
    use zephium_app::{Command, Handle};
    use zephium_core::ports::engine::MemoryPressure;

    pub(super) struct Monitor(DispatchRetained<DispatchSource>);

    extern "C" {
        fn sysctlbyname(
            name: *const std::ffi::c_char,
            oldp: *mut std::ffi::c_void,
            oldlenp: *mut usize,
            newp: *mut std::ffi::c_void,
            newlen: usize,
        ) -> std::ffi::c_int;
    }

    /// The kernel's current level. Source data OR-merges every transition
    /// since the last callback, so a coalesced warn-then-normal would
    /// otherwise read as a warning until the next transition.
    fn current_level() -> Option<MemoryPressure> {
        let mut level: std::ffi::c_int = 0;
        let mut size = std::mem::size_of::<std::ffi::c_int>();
        // SAFETY: a read-only sysctl into a correctly sized integer.
        let status = unsafe {
            sysctlbyname(
                c"kern.memorystatus_vm_pressure_level".as_ptr(),
                (&mut level as *mut std::ffi::c_int).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        (status == 0).then_some(match level {
            4 => MemoryPressure::Critical,
            2 => MemoryPressure::Warning,
            _ => MemoryPressure::Normal,
        })
    }

    impl Monitor {
        pub(super) fn new(shell: Handle) -> Option<Self> {
            // SAFETY: this public source type takes handle 0 and the documented
            // NORMAL/WARN/CRITICAL mask; libdispatch owns its default queue.
            let source = unsafe {
                DispatchSource::new(
                    std::ptr::addr_of!(dispatch2::_dispatch_source_type_memorypressure).cast_mut(),
                    0,
                    1 | 2 | 4,
                    None,
                )
            };
            // The handler must not retain its own source. Libdispatch keeps it
            // alive for an executing handler, including cancellation races.
            let address = (&*source as *const DispatchSource) as usize;
            let initial = shell.clone();
            let handler = block2::RcBlock::new(move || {
                // SAFETY: this block is invoked only by this live source;
                // cancellation prevents future events before source disposal.
                let flags = unsafe { &*(address as *const DispatchSource) }.data();
                let pressure = if let Some(level) = current_level() {
                    level
                } else if flags & 4 != 0 {
                    MemoryPressure::Critical
                } else if flags & 2 != 0 {
                    MemoryPressure::Warning
                } else if flags & 1 != 0 {
                    MemoryPressure::Normal
                } else {
                    return;
                };
                let _ = shell.dispatch(Command::SetMemoryPressure(pressure));
            });
            // SAFETY: the source copies the block before activation. Its only
            // cross-thread state is the Send + Sync Shell handle.
            unsafe { source.set_event_handler_with_block(block2::RcBlock::as_ptr(&handler)) };
            source.activate();
            // The source reports transitions only; launching under pressure
            // must not look like a normal machine until the next change.
            if let Some(level) = current_level().filter(|level| *level != MemoryPressure::Normal) {
                let _ = initial.dispatch(Command::SetMemoryPressure(level));
            }
            Some(Self(source))
        }
    }

    impl Drop for Monitor {
        fn drop(&mut self) {
            self.0.cancel();
        }
    }
}

#[cfg(target_os = "windows")]
mod native {
    use std::sync::{Arc, Mutex};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Memory::{
        CreateMemoryResourceNotification, HighMemoryResourceNotification,
        LowMemoryResourceNotification,
    };
    use windows::Win32::System::Threading::{
        CreateEventW, SetEvent, WaitForMultipleObjects, INFINITE,
    };
    use zephium_app::{Command, Handle};
    use zephium_core::ports::engine::MemoryPressure;

    struct Event(HANDLE);
    impl Event {
        fn handle(&self) -> HANDLE {
            self.0
        }
    }
    // SAFETY: these are owned kernel waitable handles. They are closed only
    // after the last Arc/user has gone; no GUI-thread affinity is involved.
    unsafe impl Send for Event {}
    // SAFETY: concurrent waiting/signaling is explicitly supported by Win32.
    unsafe impl Sync for Event {}
    impl Drop for Event {
        fn drop(&mut self) {
            // SAFETY: this wrapper owns exactly one successful handle creation.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    pub(super) struct Monitor {
        stop: Arc<Event>,
        worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    }

    impl Monitor {
        pub(super) fn new(shell: Handle) -> Option<Self> {
            // SAFETY: unnamed, noninherited handles; all successful creations
            // immediately acquire one RAII owner, including failure paths.
            let (stop, low, high) = unsafe {
                (
                    Arc::new(Event(CreateEventW(None, true, false, PCWSTR::null()).ok()?)),
                    Event(CreateMemoryResourceNotification(LowMemoryResourceNotification).ok()?),
                    Event(CreateMemoryResourceNotification(HighMemoryResourceNotification).ok()?),
                )
            };
            let stopped = stop.clone();
            let worker = std::thread::Builder::new()
                .name("memory-pressure".into())
                .spawn(move || {
                    let mut pressured = false;
                    loop {
                        // Alternate low/high thresholds: a signaled low-memory
                        // event must not produce a busy loop while pressure lasts.
                        let handles = [
                            stopped.handle(),
                            if pressured {
                                high.handle()
                            } else {
                                low.handle()
                            },
                        ];
                        // SAFETY: both handles remain owned across this wait;
                        // shutdown signals index zero before joining this thread.
                        let result = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
                        if result.0 != WAIT_OBJECT_0.0 + 1 {
                            break;
                        }
                        pressured = !pressured;
                        let _ = shell.dispatch(Command::SetMemoryPressure(if pressured {
                            MemoryPressure::Critical
                        } else {
                            MemoryPressure::Normal
                        }));
                    }
                })
                .ok()?;
            Some(Self {
                stop,
                worker: Mutex::new(Some(worker)),
            })
        }
    }

    impl Drop for Monitor {
        fn drop(&mut self) {
            // SAFETY: the stop handle outlives both signaling and worker join.
            let _ = unsafe { SetEvent(self.stop.0) };
            if let Ok(worker) = self.worker.get_mut() {
                if let Some(worker) = worker.take() {
                    let _ = worker.join();
                }
            }
        }
    }
}
