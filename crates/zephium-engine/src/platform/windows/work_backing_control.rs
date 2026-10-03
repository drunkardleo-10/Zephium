//! Provider-free native chrome backing and capture-debt qualification.
#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]

use super::super::super::{
    work_presentation::{PresentationState, WorkHumanPresentation, WorkObservationPresentation},
    work_rendering,
};
use super::super::ProbeHostWindow;
use super::{
    attest_environment, browser_process_for_environment, install_browser_process_exit_observer,
    pump_browser_exit_callbacks, wait_for_browser_process_exit, OwnedUdf, TIMEOUT,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Controller2, ICoreWebView2_13, COREWEBVIEW2_COLOR,
    COREWEBVIEW2_PREFERRED_COLOR_SCHEME_AUTO, COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK,
    COREWEBVIEW2_PREFERRED_COLOR_SCHEME_LIGHT,
};
use windows_core::Interface as _;
use wry::{WebContext, WebViewBuilder, WebViewExtWindows as _};

struct OwnedNativeRegion(windows::Win32::Graphics::Gdi::HRGN);
impl Drop for OwnedNativeRegion {
    fn drop(&mut self) {
        // SAFETY: this snapshot region was never transferred to a native HWND.
        let _ = unsafe { windows::Win32::Graphics::Gdi::DeleteObject(self.0.into()) };
    }
}

fn copy_region(
    window: windows::Win32::Foundation::HWND,
) -> Result<Option<OwnedNativeRegion>, &'static str> {
    // SAFETY: allocate an exclusively owned writable snapshot on this fixture STA.
    let region =
        OwnedNativeRegion(unsafe { windows::Win32::Graphics::Gdi::CreateRectRgn(0, 0, 0, 0) });
    if region.0 .0.is_null() {
        return Err("clip_snapshot_allocation");
    }
    // SAFETY: this exact live fixture child and owned snapshot stay on their STA.
    let kind = unsafe { windows::Win32::Graphics::Gdi::GetWindowRgn(window, region.0) }.0;
    Ok((kind != 0).then_some(region))
}

fn same_region(
    window: windows::Win32::Foundation::HWND,
    original: Option<&OwnedNativeRegion>,
) -> Result<bool, &'static str> {
    let current = copy_region(window)?;
    Ok(match (current.as_ref(), original) {
        (None, None) => true,
        // SAFETY: both comparison regions are exclusively owned live snapshots.
        (Some(current), Some(original)) => unsafe {
            windows::Win32::Graphics::Gdi::EqualRgn(current.0, original.0).as_bool()
        },
        _ => false,
    })
}

fn owns_clip(window: windows::Win32::Foundation::HWND) -> bool {
    // SAFETY: inspect only the exact fixture child's opaque owner property.
    !unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetPropW(
            window,
            windows_core::w!("ZephiumWorkObservationClipOwner"),
        )
    }
    .0
    .is_null()
}

fn is_empty_clip(window: windows::Win32::Foundation::HWND) -> Result<bool, &'static str> {
    // SAFETY: allocate an exclusively owned comparison region for this fixture.
    let empty =
        OwnedNativeRegion(unsafe { windows::Win32::Graphics::Gdi::CreateRectRgn(0, 0, 0, 0) });
    if empty.0 .0.is_null() {
        return Err("clip_empty_comparison_allocation");
    }
    same_region(window, Some(&empty))
}

fn color(controller: &ICoreWebView2Controller2) -> Result<[u8; 4], &'static str> {
    let mut color = COREWEBVIEW2_COLOR::default();
    // SAFETY: exact fixture-owned controller and initialized output on its STA.
    unsafe { controller.DefaultBackgroundColor(&mut color) }.map_err(|_| "backing_color_read")?;
    Ok([color.R, color.G, color.B, color.A])
}

fn presentation(view: &wry::WebView) -> Result<WorkObservationPresentation, &'static str> {
    let deadline = Instant::now() + TIMEOUT;
    #[cfg(feature = "native-agentic-work-lifetime-diagnostic")]
    let prepared = WorkObservationPresentation::prepare(view, deadline, |_| {});
    #[cfg(not(feature = "native-agentic-work-lifetime-diagnostic"))]
    let prepared = WorkObservationPresentation::prepare(view, deadline);
    prepared.map_err(|_| "backing_presentation")
}

fn assert_dark_crop(
    host: &ProbeHostWindow,
    path: &std::path::Path,
    phase: &'static str,
) -> Result<(), &'static str> {
    let pixels = super::super::work_blur_control::screen(host.hwnd, path)?;
    let mismatch = pixels
        .enumerate_pixels()
        .find(|(_, _, pixel)| {
            pixel
                .0
                .iter()
                .zip([16u8, 16, 21])
                .any(|(value, expected)| value.abs_diff(expected) > 3)
        })
        .map(|(x, y, pixel)| (x, y, pixel.0));
    if let Some(mismatch) = mismatch {
        eprintln!("native-backing: phase={phase} first_owned_pixel_mismatch={mismatch:?}; fixture_pixels_only=true");
        return Err(phase);
    }
    Ok(())
}

#[test]
fn opaque_chrome_tracks_native_render_owners_and_preserves_sharp_capture() {
    assert_eq!(run(), Ok(()));
}

#[test]
fn canvas_sized_clipped_renderer_captures_fresh_pixels_under_transparent_chrome() {
    assert_eq!(run_canvas(), Ok(()));
}

fn run_canvas() -> Result<(), &'static str> {
    use windows::Win32::Graphics::Gdi::{
        CreateRectRgn, CreateRoundRectRgn, DeleteObject, SetWindowRgn,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE, SW_SHOWNOACTIVATE,
    };
    let directory = tempfile::Builder::new()
        .prefix("zephium-native-canvas-")
        .tempdir()
        .map_err(|_| "canvas_directory")?;
    let path = directory.path().to_owned();
    let directory = OwnedUdf(Some(directory));
    let host = ProbeHostWindow::new().map_err(|_| "canvas_host")?;
    // SAFETY: this qualifier owns this top-level HWND and never activates it.
    unsafe {
        SetWindowPos(
            host.hwnd,
            Some(HWND_TOPMOST),
            20,
            20,
            1312,
            864,
            SWP_NOACTIVATE,
        )
        .map_err(|_| "canvas_host_position")?;
        let _ = ShowWindow(host.hwnd, SW_SHOWNOACTIVATE);
    }
    let mut context = WebContext::new(Some(path.clone()));
    let loaded = Rc::new(Cell::new(0));
    let animation_ready = Arc::new(AtomicBool::new(false));
    let source_animation_ready = animation_ready.clone();
    let clicked = Arc::new(AtomicBool::new(false));
    let source_clicked = clicked.clone();
    let source_loaded = loaded.clone();
    let mut source = WebViewBuilder::new_with_web_context(&mut context)
        .with_html("<!doctype html><style>html,body{margin:0;width:100%;height:100%;background:#ff0000}</style>")
        .with_visible(false).with_focused(false)
        .with_ipc_handler(move |message| {
            if message.body()=="canvas-green-frame" {
                source_animation_ready.store(true,Ordering::Release);
            }
            if message.body()=="canvas-trusted-click" {
                source_clicked.store(true,Ordering::Release);
            }
        })
        .with_bounds(wry::Rect {
            position: wry::dpi::LogicalPosition::new(160.0, 32.0).into(),
            size: wry::dpi::LogicalSize::new(1280.0, 800.0).into(),
        })
        .with_navigation_event_handler(move |event| {
            if event.phase == wry::NavigationEventPhase::Finished { source_loaded.set(source_loaded.get()+1); }
        }).build_as_child(&host).map_err(|_| "canvas_source")?;
    let chrome_loaded = loaded.clone();
    let mut chrome = WebViewBuilder::new_with_web_context(&mut context)
        .with_html("<!doctype html><style>:root{color-scheme:dark}html,body{margin:0;background:transparent}.canvas{position:absolute;left:160px;top:32px;width:480px;height:320px;border-radius:12px;background:#1a1a1d}</style><div class=canvas></div>")
        .with_transparent(true).with_background_color((0,0,0,0))
        .with_visible(true).with_focused(false)
        .with_bounds(wry::Rect {
            position: wry::dpi::PhysicalPosition::new(0,0).into(),
            size: wry::dpi::PhysicalSize::new(1280,800).into(),
        })
        .with_navigation_event_handler(move |event| {
            if event.phase == wry::NavigationEventPhase::Finished { chrome_loaded.set(chrome_loaded.get()+1); }
        }).build_as_child(&host).map_err(|_| "canvas_chrome")?;
    let environment = source.environment();
    let process = browser_process_for_environment(&environment).map_err(|_| "canvas_process")?;
    let observer = install_browser_process_exit_observer(&environment, process.id(), |_| {})
        .map_err(|_| "canvas_observer")?;
    let outcome = (|| {
        attest_environment(&environment, &path).map_err(|_| "canvas_environment")?;
        let deadline = Instant::now() + TIMEOUT;
        while loaded.get() < 2 && Instant::now() < deadline {
            if !pump_browser_exit_callbacks(deadline) {
                return Err("canvas_load_pump");
            }
        }
        if loaded.get() < 2 {
            return Err("canvas_load");
        }
        let controller = chrome
            .controller()
            .cast::<ICoreWebView2Controller2>()
            .map_err(|_| "canvas_chrome_controller")?;
        if color(&controller)?[3] != 0 {
            return Err("canvas_chrome_not_transparent");
        }
        let scale = f64::from(
            // SAFETY: exact live fixture HWND on its creating STA.
            unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(host.hwnd) },
        ) / 96.0;
        let width = (480.0 * scale).round() as i32;
        let height = (320.0 * scale).round() as i32;
        let viewport_width = (1280.0 * scale).round() as u32;
        let viewport_height = (800.0 * scale).round() as u32;
        // The native clip sits strictly inside the CSS ground, including its
        // rounded corners; it changes no renderer viewport or source pixels.
        // SAFETY: create and transfer a region only to the exact owned source.
        let region = unsafe {
            CreateRoundRectRgn(
                1,
                1,
                width - 1,
                height - 1,
                (28.0 * scale).round() as i32,
                (28.0 * scale).round() as i32,
            )
        };
        if region.0.is_null() {
            return Err("canvas_region_create");
        }
        // SAFETY: system owns a successfully installed region; failed ownership is released here.
        if unsafe { SetWindowRgn(source.hwnd(), Some(region), true) } == 0 {
            // SAFETY: installation failed, so the fixture still owns this region.
            let _ = unsafe { DeleteObject(region.into()) };
            return Err("canvas_region_apply");
        }
        let original_region = copy_region(source.hwnd())?;
        // Document-loaded is not a native first-paint barrier. Wait for the
        // exact owned ground before measuring later source concealment.
        let paint_deadline = Instant::now() + std::time::Duration::from_secs(2);
        let baseline = loop {
            let pixels = super::super::work_blur_control::screen(
                host.hwnd,
                &path.join("canvas-before.png"),
            )?;
            let interior: Vec<_> = pixels
                .enumerate_pixels()
                .filter(|(x, y, _)| {
                    let x = f64::from(*x + 32);
                    let y = f64::from(*y + 32);
                    x >= (160.0 + 16.0) * scale
                        && x <= (160.0 + 480.0 - 16.0) * scale
                        && y >= (32.0 + 16.0) * scale
                        && y <= (32.0 + 320.0 - 16.0) * scale
                })
                .collect();
            if !interior.is_empty()
                && interior.iter().all(|(_, _, pixel)| {
                    pixel
                        .0
                        .iter()
                        .zip([26u8, 26, 29])
                        .all(|(value, expected)| value.abs_diff(expected) <= 3)
                })
            {
                break pixels;
            }
            if Instant::now() >= paint_deadline {
                eprintln!("native-canvas: phase=ground_first_paint first_interior={:?}; fixture_pixels_only=true",interior.first().map(|(x,y,pixel)|(*x,*y,pixel.0)));
                return Err("canvas_ground_first_paint");
            }
            if !pump_browser_exit_callbacks(paint_deadline) {
                return Err("canvas_ground_first_paint_pump");
            }
        };
        let mut canvas_presentation = presentation(&source)?;
        if canvas_presentation.present() != PresentationState::Ready {
            return Err("canvas_present");
        }
        if !owns_clip(source.hwnd()) || !is_empty_clip(source.hwnd())? {
            return Err("canvas_native_concealment");
        }
        let covered =
            super::super::work_blur_control::screen(host.hwnd, &path.join("canvas-covered.png"))?;
        if baseline.as_raw() != covered.as_raw() {
            eprintln!("native-canvas: phase=initial_reveal first_difference={:?}; fixture_pixels_only=true",baseline.enumerate_pixels().zip(covered.pixels()).find(|((_,_,before),after)|before.0!=after.0).map(|((x,y,before),after)|(x,y,before.0,after.0)));
            return Err("canvas_source_exposed");
        }
        let initial_started = Instant::now();
        let initial = super::super::work_blur_control::preview_with_dimensions(
            &source.webview(),
            &path.join("canvas-initial.png"),
            (viewport_width, viewport_height),
        )?;
        eprintln!(
            "native-canvas: source_bounds={}x{} initial_capture_ms={}; fixture_pixels_only=true",
            viewport_width,
            viewport_height,
            initial_started.elapsed().as_millis()
        );
        if initial.dimensions() != (viewport_width, viewport_height)
            || initial
                .pixels()
                .any(|pixel| pixel.0[0] < 247 || pixel.0[1] > 8 || pixel.0[2] > 8)
        {
            return Err("canvas_capture_initial_size_or_pixels");
        }
        let mutated = Arc::new(AtomicBool::new(false));
        let callback_mutated = mutated.clone();
        source.evaluate_script_with_callback("document.documentElement.style.background='#0000ff';document.body.style.background='#0000ff';true",move|result|callback_mutated.store(result=="true",Ordering::Release))
            .map_err(|_| "canvas_mutation_dispatch")?;
        let deadline = Instant::now() + TIMEOUT;
        while !mutated.load(Ordering::Acquire) && Instant::now() < deadline {
            if !pump_browser_exit_callbacks(deadline) {
                return Err("canvas_mutation_pump");
            }
        }
        if !mutated.load(Ordering::Acquire) {
            return Err("canvas_mutation_terminal");
        }
        let updated_started = Instant::now();
        let updated = super::super::work_blur_control::preview_with_dimensions(
            &source.webview(),
            &path.join("canvas-updated.png"),
            (viewport_width, viewport_height),
        )?;
        eprintln!(
            "native-canvas: source_bounds={}x{} updated_capture_ms={}; fixture_pixels_only=true",
            viewport_width,
            viewport_height,
            updated_started.elapsed().as_millis()
        );
        if updated.dimensions() != (viewport_width, viewport_height)
            || updated
                .pixels()
                .any(|pixel| pixel.0[0] > 8 || pixel.0[1] > 8 || pixel.0[2] < 247)
        {
            return Err("canvas_clipped_capture_stale_or_incomplete");
        }
        let covered = super::super::work_blur_control::screen(
            host.hwnd,
            &path.join("canvas-updated-covered.png"),
        )?;
        if baseline.as_raw() != covered.as_raw() || color(&controller)?[3] != 0 {
            return Err("canvas_mutated_source_exposed");
        }
        // Browse has no Work ground. Keep native visibility and viewport,
        // while an empty owned HWND region permits no source drawing at all.
        // Qualify compositor advancement, rather than only synchronous styles.
        // SAFETY: create an empty region to transfer only to the fixture source.
        let empty = unsafe { CreateRectRgn(0, 0, 0, 0) };
        if empty.0.is_null() {
            return Err("canvas_empty_region_create");
        }
        // SAFETY: exact source HWND; successful installation transfers ownership.
        if unsafe { SetWindowRgn(source.hwnd(), Some(empty), true) } == 0 {
            // SAFETY: failed installation leaves this fixture owning the region.
            let _ = unsafe { DeleteObject(empty.into()) };
            return Err("canvas_empty_region_apply");
        }
        let animation_started = Instant::now();
        source.evaluate_script("requestAnimationFrame(()=>{document.documentElement.style.background='#00ff00';document.body.style.background='#00ff00';requestAnimationFrame(()=>window.ipc.postMessage('canvas-green-frame'));})")
            .map_err(|_| "canvas_empty_animation_dispatch")?;
        let deadline = Instant::now() + TIMEOUT;
        while !animation_ready.load(Ordering::Acquire) && Instant::now() < deadline {
            if !pump_browser_exit_callbacks(deadline) {
                return Err("canvas_empty_animation_pump");
            }
        }
        if !animation_ready.load(Ordering::Acquire) {
            return Err("canvas_empty_animation_frozen");
        }
        let empty_started = Instant::now();
        let empty_capture = super::super::work_blur_control::preview_with_dimensions(
            &source.webview(),
            &path.join("canvas-empty-region.png"),
            (viewport_width, viewport_height),
        )?;
        eprintln!("native-canvas: source_bounds={}x{} empty_region_animation_ms={} empty_region_capture_ms={}; fixture_pixels_only=true",width,height,animation_started.elapsed().as_millis(),empty_started.elapsed().as_millis());
        if empty_capture
            .pixels()
            .any(|pixel| pixel.0[0] > 8 || pixel.0[1] < 247 || pixel.0[2] > 8)
        {
            return Err("canvas_empty_region_capture_stale");
        }
        let concealed = super::super::work_blur_control::screen(
            host.hwnd,
            &path.join("canvas-empty-concealed.png"),
        )?;
        if concealed.as_raw() != baseline.as_raw() {
            return Err("canvas_empty_region_exposed");
        }
        let button_ready = Arc::new(AtomicBool::new(false));
        let callback_button_ready = button_ready.clone();
        source.evaluate_script_with_callback("{const b=document.createElement('button');b.style.cssText='position:absolute;left:40px;top:40px;width:80px;height:40px;background:transparent;border:0;outline:0;padding:0';b.onclick=e=>{if(e.isTrusted){document.documentElement.style.background='#ffff00';document.body.style.background='#ffff00';b.blur();window.ipc.postMessage('canvas-trusted-click');}};document.body.append(b);}true",move|result|callback_button_ready.store(result=="true",Ordering::Release))
            .map_err(|_| "canvas_empty_button_dispatch")?;
        let deadline = Instant::now() + TIMEOUT;
        while !button_ready.load(Ordering::Acquire) && Instant::now() < deadline {
            if !pump_browser_exit_callbacks(deadline) {
                return Err("canvas_empty_button_pump");
            }
        }
        if !button_ready.load(Ordering::Acquire) {
            return Err("canvas_empty_button_terminal");
        }
        // The production action adapter uses CDP viewport coordinates. This
        // fixed fixture input proves native HWND clipping does not prevent
        // trusted input delivery to an owned, nonfocused/disabled renderer.
        for parameters in [
            r#"{"type":"mousePressed","x":80,"y":60,"button":"left","buttons":1,"clickCount":1}"#,
            r#"{"type":"mouseReleased","x":80,"y":60,"button":"left","buttons":0,"clickCount":1}"#,
        ] {
            let result = Rc::new(RefCell::new(None));
            let callback_result = result.clone();
            let callback = webview2_com::CallDevToolsProtocolMethodCompletedHandler::create(
                Box::new(move |status, response| {
                    *callback_result.borrow_mut() = Some(status.is_ok() && response.trim() == "{}");
                    Ok(())
                }),
            );
            // SAFETY: fixed fixture-only input and retained COM completion on this STA; no user page is addressed.
            unsafe {
                source.webview().CallDevToolsProtocolMethod(
                    &windows_core::HSTRING::from("Input.dispatchMouseEvent"),
                    &windows_core::HSTRING::from(parameters),
                    &callback,
                )
            }
            .map_err(|_| "canvas_empty_cdp_dispatch")?;
            let deadline = Instant::now() + TIMEOUT;
            while result.borrow().is_none() && Instant::now() < deadline {
                if !pump_browser_exit_callbacks(deadline) {
                    return Err("canvas_empty_cdp_pump");
                }
            }
            if *result.borrow() != Some(true) {
                return Err("canvas_empty_cdp_terminal");
            }
        }
        let deadline = Instant::now() + TIMEOUT;
        while !clicked.load(Ordering::Acquire) && Instant::now() < deadline {
            if !pump_browser_exit_callbacks(deadline) {
                return Err("canvas_empty_click_pump");
            }
        }
        if !clicked.load(Ordering::Acquire) {
            return Err("canvas_empty_trusted_click_absent");
        }
        let clicked_capture = super::super::work_blur_control::preview_with_dimensions(
            &source.webview(),
            &path.join("canvas-empty-trusted-click.png"),
            (viewport_width, viewport_height),
        )?;
        if clicked_capture
            .pixels()
            .any(|pixel| pixel.0[0] < 247 || pixel.0[1] < 247 || pixel.0[2] > 8)
        {
            return Err("canvas_empty_click_pixels_stale");
        }
        eprintln!("native-canvas: empty_region_trusted_cdp_click=true fresh_action_pixels=true; fixture_pixels_only=true");
        if canvas_presentation.retire() != PresentationState::Retired {
            return Err("canvas_retirement");
        }
        if owns_clip(source.hwnd()) || !same_region(source.hwnd(), original_region.as_ref())? {
            return Err("canvas_rounded_region_restore");
        }
        // The original shape can be rounded, absent, or explicitly empty.
        // Repeated observation on the same HWND must restore each exact case,
        // never confuse empty clipping with absence or leak its owner property.
        for original_kind in ["rounded", "absent", "empty"] {
            if original_kind == "absent" {
                // SAFETY: remove clipping only from this hidden fixture child.
                if unsafe { SetWindowRgn(source.hwnd(), None, true) } == 0 {
                    return Err("clip_absent_original_apply");
                }
            } else if original_kind == "empty" {
                // SAFETY: allocate a region to transfer to this exact hidden child.
                let empty = unsafe { CreateRectRgn(0, 0, 0, 0) };
                if empty.0.is_null() {
                    return Err("clip_empty_original_allocation");
                }
                // SAFETY: success transfers ownership; failure leaves it local.
                if unsafe { SetWindowRgn(source.hwnd(), Some(empty), true) } == 0 {
                    // SAFETY: this unsuccessful transfer remains fixture-owned.
                    let _ = unsafe { DeleteObject(empty.into()) };
                    return Err("clip_empty_original_apply");
                }
            }
            let original = copy_region(source.hwnd())?;
            for _ in 0..3 {
                let mut owner = presentation(&source)?;
                if owner.present() != PresentationState::Ready {
                    return Err("clip_repeat_present");
                }
                let fence = owner.human_fence();
                if !fence() || !owns_clip(source.hwnd()) || !is_empty_clip(source.hwnd())? {
                    return Err("clip_repeat_native_ownership");
                }
                let pending = Arc::new(AtomicBool::new(true));
                let guard = owner.retain_frame_capture(pending.clone());
                if guard.is_none() || owner.retire() != PresentationState::Retiring || fence() {
                    return Err("clip_pending_retirement");
                }
                let region = zephium_agentic::WorkBrowserHumanRegion::try_new(160, 32, 480, 320)
                    .ok_or("clip_human_region")?;
                if WorkHumanPresentation::prepare(&source, region, Instant::now() + TIMEOUT)
                    .is_some()
                    || !owns_clip(source.hwnd())
                    || !is_empty_clip(source.hwnd())?
                {
                    return Err("clip_pending_human_admission");
                }
                drop(guard);
                if owner.retire() != PresentationState::Retiring {
                    return Err("clip_queued_debt_restore");
                }
                pending.store(false, Ordering::Release);
                if owner.retire() != PresentationState::Retired
                    || owns_clip(source.hwnd())
                    || !same_region(source.hwnd(), original.as_ref())?
                {
                    return Err("clip_repeat_exact_restore");
                }
                if WorkHumanPresentation::prepare(&source, region, Instant::now() + TIMEOUT)
                    .is_none()
                {
                    return Err("clip_restored_human_admission");
                }
            }
            if original_kind == "absent" {
                let mut resting_bounds = windows::Win32::Foundation::RECT::default();
                // SAFETY: exact retained fixture controller and writable bounds on its STA.
                unsafe { source.controller().Bounds(&mut resting_bounds) }
                    .map_err(|_| "clip_human_resting_bounds")?;
                let region = zephium_agentic::WorkBrowserHumanRegion::try_new(160, 32, 480, 320)
                    .ok_or("clip_human_region")?;
                let mut human =
                    WorkHumanPresentation::prepare(&source, region, Instant::now() + TIMEOUT)
                        .ok_or("clip_human_prepare")?;
                // SAFETY: capture foreground identity only; the owned fixture must not activate it.
                let foreground =
                    unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
                if !human.present() || !human.current() {
                    return Err("clip_human_present");
                }
                let mut human_bounds = windows::Win32::Foundation::RECT::default();
                // SAFETY: read only the exact fixture child/controller and initialized output on its STA.
                let human_native = unsafe {
                    windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(source.hwnd())
                        .as_bool()
                        && windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(
                            source.hwnd(),
                        )
                        .as_bool()
                        && windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow()
                            == foreground
                        && source.controller().Bounds(&mut human_bounds).is_ok()
                };
                if !human_native
                    || owns_clip(source.hwnd())
                    || is_empty_clip(source.hwnd())?
                    || human_bounds
                        != (windows::Win32::Foundation::RECT {
                            left: 0,
                            top: 0,
                            right: width,
                            bottom: height,
                        })
                {
                    return Err("clip_human_native_surface");
                }
                if !human.retire() || human.current() {
                    return Err("clip_human_retire");
                }
                let mut restored_bounds = windows::Win32::Foundation::RECT::default();
                // SAFETY: verify exact native retirement on the fixture's controller STA.
                let human_retired = unsafe {
                    !windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(source.hwnd())
                        .as_bool()
                        && !windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(
                            source.hwnd(),
                        )
                        .as_bool()
                        && source.controller().Bounds(&mut restored_bounds).is_ok()
                };
                if !human_retired || restored_bounds != resting_bounds {
                    return Err("clip_human_viewport_restore");
                }
                drop(human);
                let mut resumed = presentation(&source)?;
                if resumed.present() != PresentationState::Ready
                    || !owns_clip(source.hwnd())
                    || !is_empty_clip(source.hwnd())?
                {
                    return Err("clip_human_observation_resume");
                }
                animation_ready.store(false, Ordering::Release);
                source.evaluate_script("requestAnimationFrame(()=>{document.documentElement.style.background='#ff00ff';document.body.style.background='#ff00ff';requestAnimationFrame(()=>window.ipc.postMessage('canvas-green-frame'));})")
                    .map_err(|_| "clip_human_resume_animation_dispatch")?;
                let deadline = Instant::now() + TIMEOUT;
                while !animation_ready.load(Ordering::Acquire) && Instant::now() < deadline {
                    if !pump_browser_exit_callbacks(deadline) {
                        return Err("clip_human_resume_animation_pump");
                    }
                }
                if !animation_ready.load(Ordering::Acquire) {
                    return Err("clip_human_resume_animation_terminal");
                }
                let resumed_capture = super::super::work_blur_control::preview_with_dimensions(
                    &source.webview(),
                    &path.join("canvas-human-resumed.png"),
                    (viewport_width, viewport_height),
                )?;
                if resumed_capture
                    .pixels()
                    .any(|pixel| pixel.0[0] < 247 || pixel.0[1] > 8 || pixel.0[2] < 247)
                {
                    return Err("clip_human_resume_capture_stale");
                }
                if resumed.retire() != PresentationState::Retired
                    || owns_clip(source.hwnd())
                    || !same_region(source.hwnd(), None)?
                {
                    return Err("clip_human_resume_retirement");
                }
                eprintln!("native-canvas: human_present_visible_enabled=true human_viewport_restored=true resumed_empty_clip_fresh_capture=true foreground_unchanged=true; fixture_owned_native_state_only=true");
            }
            eprintln!("native-canvas: exact_original_region={original_kind} repeated_cycles=3 pending_human_refused=true owner_property_cleared=true; fixture_owned_native_state_only=true");
        }
        Ok(())
    })();
    let closed = source.close().is_ok() & chrome.close().is_ok();
    drop(source);
    drop(chrome);
    drop(context);
    drop(environment);
    let exited =
        wait_for_browser_process_exit(&process, &observer.proof(), Instant::now() + TIMEOUT);
    drop(observer);
    drop(process);
    drop(host);
    if !closed || !exited {
        return Err("canvas_cleanup");
    }
    directory.close()?;
    outcome
}

fn run() -> Result<(), &'static str> {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE, SW_SHOWNOACTIVATE,
    };
    let directory = tempfile::Builder::new()
        .prefix("zephium-native-backing-")
        .tempdir()
        .map_err(|_| "directory")?;
    let path = directory.path().to_owned();
    let directory = OwnedUdf(Some(directory));
    let host = ProbeHostWindow::new().map_err(|_| "host")?;
    // SAFETY: this fixture uniquely owns this top-level HWND. Match the existing
    // native blur fixture's visible topmost placement without taking activation.
    unsafe {
        SetWindowPos(
            host.hwnd,
            Some(HWND_TOPMOST),
            20,
            20,
            1280 + 32,
            800 + 64,
            SWP_NOACTIVATE,
        )
        .map_err(|_| "backing_host_position")?;
        let _ = ShowWindow(host.hwnd, SW_SHOWNOACTIVATE);
    }
    let mut context = WebContext::new(Some(path.clone()));
    let loaded = Rc::new(Cell::new(0));
    let mut build = |page: &'static str, visible| {
        let loaded = loaded.clone();
        WebViewBuilder::new_with_web_context(&mut context)
            .with_html(page)
            .with_incognito(false)
            .with_focused(false)
            .with_visible(visible)
            // Mirror actual Tauri chrome and opaque owned Work views.
            .with_transparent(visible)
            .with_background_color(if visible {
                (0, 0, 0, 0)
            } else {
                (255, 255, 255, 255)
            })
            .with_bounds(wry::Rect {
                position: wry::dpi::PhysicalPosition::new(0, 0).into(),
                size: wry::dpi::PhysicalSize::new(1280, 800).into(),
            })
            .with_navigation_event_handler(move |event| {
                if event.phase == wry::NavigationEventPhase::Finished {
                    loaded.set(loaded.get() + 1);
                }
            })
            .build_as_child(&host)
            .map_err(|_| "backing_view")
    };
    // Keep stripes 16 physical pixels wide at 100%, 125%, and 150% DPI.
    const STRIPES: &str = "<!doctype html><style>html,body{margin:0;width:100%;height:100%;background:transparent}</style><script>const w=16/devicePixelRatio;document.documentElement.style.background='repeating-linear-gradient(90deg,#000 0,#000 '+w+'px,#fff '+w+'px,#fff '+(2*w)+'px)';</script>";
    let mut first = build(STRIPES, false)?;
    let mut second = build(STRIPES, false)?;
    let mut chrome = build(
        "<!doctype html><style>:root{color-scheme:dark}:root[data-theme=light]{color-scheme:light}html,body{margin:0;background:transparent}</style>",
        true,
    )?;
    let controller = chrome
        .controller()
        .cast::<ICoreWebView2Controller2>()
        .map_err(|_| "backing_controller2")?;
    let environment = first.environment();
    let process = browser_process_for_environment(&environment).map_err(|_| "process")?;
    let observer = install_browser_process_exit_observer(&environment, process.id(), |_| {})
        .map_err(|_| "observer")?;
    let parent = host.hwnd.0 as usize;
    let original = color(&controller)?;
    let profile = chrome
        .webview()
        .cast::<ICoreWebView2_13>()
        .map_err(|_| "backing_profile13")?;
    // SAFETY: this fixture's exact WebView2 profile is retained on its STA.
    let profile = unsafe { profile.Profile() }.map_err(|_| "backing_profile")?;
    let mut original_scheme = COREWEBVIEW2_PREFERRED_COLOR_SCHEME_AUTO;
    // SAFETY: query this fixture's profile into initialized output on its STA.
    unsafe { profile.PreferredColorScheme(&mut original_scheme) }
        .map_err(|_| "backing_original_scheme")?;
    let active = Rc::new(Cell::new(false));
    let early_restore = Rc::new(Cell::new(false));
    let callback_early_restore = early_restore.clone();
    let first_window = first.hwnd();
    let second_window = second.hwnd();
    let callback_active = active.clone();
    let callback_controller = controller.clone();
    if !work_rendering::install_work_rendering_backing(parent, move |enabled| {
        // SAFETY: these exact fixture-owned native children remain on this same STA.
        if !enabled
            && unsafe {
                windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(first_window).as_bool()
                    || windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(second_window)
                        .as_bool()
            }
        {
            callback_early_restore.set(true);
        }
        let color = if enabled {
            COREWEBVIEW2_COLOR {
                R: 16,
                G: 16,
                B: 21,
                A: 255,
            }
        } else {
            COREWEBVIEW2_COLOR {
                R: original[0],
                G: original[1],
                B: original[2],
                A: original[3],
            }
        };
        // SAFETY: the callback runs synchronously on the exact native fixture STA.
        let applied = unsafe { callback_controller.SetDefaultBackgroundColor(color) }.is_ok();
        if applied {
            callback_active.set(enabled);
        }
        applied
    }) {
        return Err("backing_registration");
    }
    let outcome = (|| {
        attest_environment(&environment, &path).map_err(|_| "attestation")?;
        let deadline = Instant::now() + TIMEOUT;
        while loaded.get() < 3 && Instant::now() < deadline {
            if !pump_browser_exit_callbacks(deadline) {
                return Err("backing_load_pump");
            }
        }
        if loaded.get() < 3 || original[3] != 0 || active.get() {
            return Err("backing_idle");
        }
        // SAFETY: mirror the production Wry theme adapter using only this fresh fixture profile.
        unsafe { profile.SetPreferredColorScheme(COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK) }
            .map_err(|_| "backing_dark_scheme")?;
        let mut one = presentation(&first)?;
        let mut two = presentation(&second)?;
        if one.present() != PresentationState::Ready
            || two.present() != PresentationState::Ready
            || color(&controller)? != [16, 16, 21, 255]
        {
            return Err("backing_dark");
        }
        // HRESULT admission is not a painted-frame barrier. Check the first
        // reveal immediately, before pumping any later Chromium frame.
        let immediate = super::super::work_blur_control::screen(
            host.hwnd,
            &path.join("backing-first-reveal.png"),
        )?;
        let first_mismatch = immediate
            .enumerate_pixels()
            .find(|(_, _, pixel)| {
                pixel
                    .0
                    .iter()
                    .zip([16u8, 16, 21])
                    .any(|(value, expected)| value.abs_diff(expected) > 3)
            })
            .map(|(x, y, pixel)| (x, y, pixel.0));
        if let Some(mismatch) = first_mismatch {
            eprintln!("native-backing: phase=immediate_first_reveal color={:?} first_owned_pixel_mismatch={:?}; fixture_pixels_only=true",color(&controller)?,mismatch);
            return Err("backing_first_reveal_flash");
        }
        // An action can replace a document while retaining its reading owner.
        // Check both native dispatch and committed navigation, not just a late
        // steady-state frame after an arbitrary delay.
        let previous_loaded = loaded.get();
        first
            .load_html(STRIPES)
            .map_err(|_| "backing_action_navigation_dispatch")?;
        assert_dark_crop(
            &host,
            &path.join("backing-action-dispatch.png"),
            "backing_action_dispatch_flash",
        )?;
        let navigation_deadline = Instant::now() + TIMEOUT;
        while loaded.get() == previous_loaded && Instant::now() < navigation_deadline {
            if !pump_browser_exit_callbacks(navigation_deadline) {
                return Err("backing_action_navigation_pump");
            }
        }
        if loaded.get() == previous_loaded {
            return Err("backing_action_navigation_terminal");
        }
        assert_dark_crop(
            &host,
            &path.join("backing-action-commit.png"),
            "backing_action_commit_flash",
        )?;
        if two.retire() != PresentationState::Retired {
            return Err("backing_overlap_retire");
        }
        assert_dark_crop(
            &host,
            &path.join("backing-overlap-retire.png"),
            "backing_overlap_retire_flash",
        )?;
        if one.retire() != PresentationState::Retired
            || active.get()
            || color(&controller)? != original
            || early_restore.get()
        {
            return Err("backing_transition_hidden_restore");
        }
        // Construction on a different page reacquires protection after the
        // last owner hid; this is the exact restore/reveal transition, with no
        // inserted sleep, extra controller, or production polling.
        two = presentation(&second)?;
        if two.present() != PresentationState::Ready {
            return Err("backing_page_successor");
        }
        assert_dark_crop(
            &host,
            &path.join("backing-page-successor.png"),
            "backing_page_successor_flash",
        )?;
        one = presentation(&first)?;
        if one.present() != PresentationState::Ready {
            return Err("backing_page_overlap");
        }
        // SAFETY: change only the owned chrome native color, matching a light theme event while render owners remain live.
        unsafe {
            profile
                .SetPreferredColorScheme(COREWEBVIEW2_PREFERRED_COLOR_SCHEME_LIGHT)
                .map_err(|_| "backing_light_scheme")?;
            controller.SetDefaultBackgroundColor(COREWEBVIEW2_COLOR {
                R: 245,
                G: 245,
                B: 249,
                A: 255,
            })
        }
        .map_err(|_| "backing_theme")?;
        if color(&controller)? != [245, 245, 249, 255] {
            return Err("backing_light");
        }
        let theme_ready = Arc::new(AtomicBool::new(false));
        let callback_theme_ready = theme_ready.clone();
        chrome
            .evaluate_script_with_callback(
                "document.documentElement.dataset.theme='light';true",
                move |result| callback_theme_ready.store(result == "true", Ordering::Release),
            )
            .map_err(|_| "backing_light_document")?;
        let theme_deadline = Instant::now() + TIMEOUT;
        while !theme_ready.load(Ordering::Acquire) && Instant::now() < theme_deadline {
            if !pump_browser_exit_callbacks(theme_deadline) {
                return Err("backing_light_document_pump");
            }
        }
        if !theme_ready.load(Ordering::Acquire) {
            return Err("backing_light_document_terminal");
        }
        // The color setter's success precedes Chromium/DWM frame publication.
        // Pump this isolated STA for a bounded interval while retaining the
        // exact same crop and strict assertion of every owned pixel.
        let paint_deadline = Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let pixels = super::super::work_blur_control::screen(
                host.hwnd,
                &path.join("backing-client.png"),
            )?;
            let mismatches: Vec<_> = pixels
                .enumerate_pixels()
                .filter_map(|(x, y, pixel)| {
                    pixel
                        .0
                        .iter()
                        .zip([245u8, 245, 249])
                        .any(|(value, expected)| value.abs_diff(expected) > 3)
                        .then_some((x, y, pixel.0))
                })
                .collect();
            if mismatches.is_empty() {
                break;
            }
            if Instant::now() >= paint_deadline {
                use windows::Win32::Foundation::RECT;
                use windows::Win32::UI::WindowsAndMessaging::{
                    GetClientRect, GetTopWindow, GetWindowRect, IsWindowVisible,
                };
                let mut host_bounds = RECT::default();
                let mut chrome_bounds = RECT::default();
                let mut first_bounds = RECT::default();
                let mut second_bounds = RECT::default();
                // SAFETY: diagnostics query only exact fixture-owned HWNDs and initialized writable output on their STA.
                let (chrome_top, chrome_visible, dpi) = unsafe {
                    let _ = GetClientRect(host.hwnd, &mut host_bounds);
                    let _ = GetWindowRect(chrome.hwnd(), &mut chrome_bounds);
                    let _ = GetWindowRect(first.hwnd(), &mut first_bounds);
                    let _ = GetWindowRect(second.hwnd(), &mut second_bounds);
                    (
                        GetTopWindow(Some(host.hwnd)).ok() == Some(chrome.hwnd()),
                        IsWindowVisible(chrome.hwnd()).as_bool(),
                        windows::Win32::UI::HiDpi::GetDpiForWindow(host.hwnd),
                    )
                };
                let chrome_capture = super::super::work_blur_control::preview(
                    &chrome.webview(),
                    &path.join("backing-chrome.png"),
                )
                .map(|image| [image.get_pixel(32, 32).0, image.get_pixel(128, 128).0]);
                let document = Arc::new(std::sync::Mutex::new(None::<String>));
                let callback_document = document.clone();
                let queried=chrome.evaluate_script_with_callback("JSON.stringify({htmlBackground:getComputedStyle(document.documentElement).backgroundColor,bodyBackground:getComputedStyle(document.body).backgroundColor,htmlScheme:getComputedStyle(document.documentElement).colorScheme,bodyScheme:getComputedStyle(document.body).colorScheme,dpr:devicePixelRatio,width:innerWidth,height:innerHeight})",move|value|{
                    if let Ok(mut document)=callback_document.lock(){*document=Some(value);}
                }).is_ok();
                let document_deadline = Instant::now() + std::time::Duration::from_secs(2);
                while queried
                    && document
                        .lock()
                        .ok()
                        .is_some_and(|document| document.is_none())
                    && Instant::now() < document_deadline
                {
                    let _ = pump_browser_exit_callbacks(document_deadline);
                }
                let document = document.lock().ok().and_then(|document| document.clone());
                eprintln!("native-backing: color={:?} chrome_capture={:?} chrome_document={:?} mismatch_count={} crop=[32,32,384,192] first_mismatch={:?} stripe_samples={:?} dpi={} chrome_top={} chrome_visible={} host_client={:?} chrome_rect={:?} first_rect={:?} second_rect={:?}; fixture_pixels_only=true",
                    color(&controller)?,chrome_capture,document,mismatches.len(),mismatches.first(),[pixels.get_pixel(4,64).0,pixels.get_pixel(12,64).0,pixels.get_pixel(20,64).0,pixels.get_pixel(28,64).0],dpi,chrome_top,chrome_visible,host_bounds,chrome_bounds,first_bounds,second_bounds);
                return Err("backing_raw_underlay");
            }
            let _ = pump_browser_exit_callbacks(paint_deadline);
        }
        let pending = Arc::new(AtomicBool::new(true));
        let guard = one.retain_frame_capture(pending.clone());
        let received = Rc::new(RefCell::new(None));
        let callback_received = received.clone();
        let callback_pending = pending.clone();
        if !super::super::super::semantic_screenshot::capture_work_frame(&first, move |frame| {
            let sharp = frame
                .and_then(|(width, height, png)| {
                    if (width, height) != (640, 400) {
                        return None;
                    }
                    let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
                        .ok()?
                        .to_rgb8();
                    Some((0..80).all(|stripe| {
                        let value = image.get_pixel(stripe * 8 + 4, 200).0;
                        if stripe % 2 == 0 {
                            value.iter().all(|v| *v < 8)
                        } else {
                            value.iter().all(|v| *v > 247)
                        }
                    }))
                })
                .unwrap_or(false);
            drop(guard);
            callback_pending.store(false, Ordering::Release);
            *callback_received.borrow_mut() = Some(sharp);
        }) {
            return Err("backing_capture_dispatch");
        }
        if one.retire() != PresentationState::Retiring {
            return Err("backing_capture_retire");
        }
        drop(one);
        if !owns_clip(first.hwnd()) || !is_empty_clip(first.hwnd())? {
            return Err("clip_original_capture_owner_lost");
        }
        if two.retire() != PresentationState::Retired
            || !active.get()
            || color(&controller)? != [245, 245, 249, 255]
        {
            return Err("backing_capture_debt");
        }
        drop(two);
        let deadline = Instant::now() + TIMEOUT;
        while received.borrow().is_none() && Instant::now() < deadline {
            if !pump_browser_exit_callbacks(deadline) {
                return Err("backing_capture_pump");
            }
        }
        if *received.borrow() != Some(true)
            || pending.load(Ordering::Acquire)
            || active.get()
            || color(&controller)? != original
        {
            return Err("backing_capture_restore");
        }
        if owns_clip(first.hwnd()) || !same_region(first.hwnd(), None)? {
            return Err("clip_original_capture_restore");
        }
        let mut successor = presentation(&first)?;
        // SAFETY: mirror a second theme switch using only the fixture's owned profile.
        unsafe { profile.SetPreferredColorScheme(COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK) }
            .map_err(|_| "backing_successor_scheme")?;
        let theme_ready = Arc::new(AtomicBool::new(false));
        let callback_theme_ready = theme_ready.clone();
        chrome
            .evaluate_script_with_callback(
                "document.documentElement.dataset.theme='dark';true",
                move |result| callback_theme_ready.store(result == "true", Ordering::Release),
            )
            .map_err(|_| "backing_dark_document")?;
        let theme_deadline = Instant::now() + TIMEOUT;
        while !theme_ready.load(Ordering::Acquire) && Instant::now() < theme_deadline {
            if !pump_browser_exit_callbacks(theme_deadline) {
                return Err("backing_dark_document_pump");
            }
        }
        if !theme_ready.load(Ordering::Acquire) {
            return Err("backing_dark_document_terminal");
        }
        if successor.present() != PresentationState::Ready
            || color(&controller)? != [16, 16, 21, 255]
        {
            return Err("backing_successor");
        }
        // Change real document pixels after opaque chrome fully covers the
        // renderer. A stale but sharp initial screenshot must fail this check.
        let mutated = Arc::new(AtomicBool::new(false));
        let callback_mutated = mutated.clone();
        first.evaluate_script_with_callback(
            "{const w=16/devicePixelRatio;document.documentElement.style.background='repeating-linear-gradient(90deg,#0000ff 0,#0000ff '+w+'px,#ff0000 '+w+'px,#ff0000 '+(2*w)+'px)';document.body.style.background='transparent';} true",
            move |result| { callback_mutated.store(result == "true", Ordering::Release); },
        ).map_err(|_| "backing_mutation_dispatch")?;
        let deadline = Instant::now() + TIMEOUT;
        while !mutated.load(Ordering::Acquire) && Instant::now() < deadline {
            if !pump_browser_exit_callbacks(deadline) {
                return Err("backing_mutation_pump");
            }
        }
        if !mutated.load(Ordering::Acquire) {
            return Err("backing_mutation_terminal");
        }
        let pending = Arc::new(AtomicBool::new(true));
        let guard = successor.retain_frame_capture(pending.clone());
        let updated = Rc::new(RefCell::new(None));
        let callback_updated = updated.clone();
        let callback_pending = pending.clone();
        if !super::super::super::semantic_screenshot::capture_work_frame(&first, move |frame| {
            let fresh = frame
                .and_then(|(_, _, png)| {
                    let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
                        .ok()?
                        .to_rgb8();
                    Some((0..80).all(|stripe| {
                        let value = image.get_pixel(stripe * 8 + 4, 200).0;
                        if stripe % 2 == 0 {
                            value[0] < 8 && value[1] < 8 && value[2] > 247
                        } else {
                            value[0] > 247 && value[1] < 8 && value[2] < 8
                        }
                    }))
                })
                .unwrap_or(false);
            drop(guard);
            callback_pending.store(false, Ordering::Release);
            *callback_updated.borrow_mut() = Some(fresh);
        }) {
            return Err("backing_updated_capture_dispatch");
        }
        let deadline = Instant::now() + TIMEOUT;
        while updated.borrow().is_none() && Instant::now() < deadline {
            if !pump_browser_exit_callbacks(deadline) {
                return Err("backing_updated_capture_pump");
            }
        }
        if *updated.borrow() != Some(true)
            || successor.retire() != PresentationState::Retired
            || active.get()
            || color(&controller)? != original
        {
            return Err("backing_occluded_pixels_stale");
        }
        if early_restore.get() {
            return Err("backing_restored_before_native_hide");
        }
        Ok(())
    })();
    work_rendering::remove_work_rendering_backing(parent);
    // SAFETY: restore this same fixture profile's exact original preference before closing its owned views.
    let scheme_restored = unsafe { profile.SetPreferredColorScheme(original_scheme) }.is_ok();
    drop(profile);
    drop(controller);
    let closed = first.close().is_ok() & second.close().is_ok() & chrome.close().is_ok();
    drop(first);
    drop(second);
    drop(chrome);
    drop(context);
    drop(environment);
    let exited =
        wait_for_browser_process_exit(&process, &observer.proof(), Instant::now() + TIMEOUT);
    drop(observer);
    drop(process);
    drop(host);
    if !closed || !exited || !scheme_restored {
        return Err("backing_native_cleanup");
    }
    directory.close()?;
    outcome
}
