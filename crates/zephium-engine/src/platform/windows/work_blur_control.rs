//! Provider-free, test-only native composition feasibility; no shipping host changes.
#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    CapturePreviewCompletedHandler, CreateCoreWebView2CompositionControllerCompletedHandler,
    ExecuteScriptCompletedHandler, NavigationCompletedEventHandler, NavigationStartingEventHandler,
};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Direct2D::Common::D2D1_BORDER_MODE_HARD;
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::Dwm::{
    DwmFlush, DwmSetWindowAttribute, DWMWA_CLOAK, DWMWINDOWATTRIBUTE,
};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::{STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET};
use windows::Win32::UI::HiDpi::{
    SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_core::{IUnknown, Interface as _, HSTRING};
use wry::{WebContext, WebViewBuilder, WebViewBuilderExtWindows as _, WebViewExtWindows as _};

use super::super::{
    attest_environment, browser_process_for_environment, install_browser_process_exit_observer,
    pump_browser_exit_callbacks, wait_for_browser_process_exit,
};
use super::ProbeHostWindow;

const TIMEOUT: Duration = Duration::from_secs(15);
const WIDTH: i32 = 1280;
const HEIGHT: i32 = 800;
const PAGE: &str = r#"<!doctype html><meta charset=utf-8><style>body{margin:0}button{position:absolute;left:600px;top:200px;width:120px;height:60px}</style><canvas id=c width=1280 height=800></canvas><button onclick="document.body.dataset.clicked=event.isTrusted?'trusted':'untrusted'">Native input</button><script>const x=c.getContext('2d');for(let i=0;i<1280;i+=16){x.fillStyle=(i/16)%2?'white':'black';x.fillRect(i,0,16,800)}</script>"#;
const NEXT_PAGE: &str = r#"<!doctype html><style>body{margin:0}</style><canvas id=c width=1280 height=800></canvas><script>const x=c.getContext('2d');for(let i=0;i<1280;i+=16){x.fillStyle=(i/16)%2?'red':'blue';x.fillRect(i,0,16,800)}</script>"#;
const CHROME: &str = r#"<!doctype html><style>html,body{margin:0;background:transparent}#panel{position:absolute;left:256px;top:80px;width:992px;height:688px;background:#182028}#control{position:absolute;left:64px;top:64px;width:32px;height:32px;background:#00ff00}</style><div id=panel></div><div id=control></div>"#;

struct FixtureDpi(DPI_AWARENESS_CONTEXT);
impl Drop for FixtureDpi {
    fn drop(&mut self) {
        // SAFETY: restore the exact context returned by the fixture's same-thread setter.
        unsafe {
            SetThreadDpiAwarenessContext(self.0);
        }
    }
}
struct CompositionChild(HWND);
impl Drop for CompositionChild {
    fn drop(&mut self) {
        // SAFETY: controllers/targets have been closed before this uniquely owned child drops.
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

struct OwnedUdf(Option<tempfile::TempDir>);
impl Drop for OwnedUdf {
    fn drop(&mut self) {
        if let Some(directory) = self.0.take() {
            // Never delete a fresh browser store after an unproven early exit.
            let _ = directory.keep();
        }
    }
}

struct NativeComposition {
    device: IDCompositionDevice3,
    desktop: IDCompositionDesktopDevice,
    target: IDCompositionTarget,
    visual: IDCompositionVisual2,
    blur: IDCompositionGaussianBlurEffect,
}
impl NativeComposition {
    fn new(hwnd: HWND) -> windows_core::Result<Self> {
        // SAFETY: the fixture owns this live target HWND on its STA; returned COM owners remain on it.
        unsafe {
            let desktop: IDCompositionDesktopDevice = composition_stage(
                "create_desktop_device",
                DCompositionCreateDevice3(None::<&IUnknown>),
            )?;
            let device: IDCompositionDevice3 = composition_stage("query_device3", desktop.cast())?;
            let target =
                composition_stage("create_target", desktop.CreateTargetForHwnd(hwnd, false))?;
            let visual = composition_stage("create_visual", device.CreateVisual())?;
            let blur = composition_stage("create_blur", device.CreateGaussianBlurEffect())?;
            composition_stage("blur_radius", blur.SetStandardDeviation2(12.0))?;
            composition_stage("blur_border", blur.SetBorderMode(D2D1_BORDER_MODE_HARD))?;
            composition_stage("set_root", target.SetRoot(&visual))?;
            composition_stage("initial_commit", device.Commit())?;
            Ok(Self {
                device,
                desktop,
                target,
                visual,
                blur,
            })
        }
    }
    fn blurred(&self, enabled: bool) -> windows_core::Result<()> {
        // SAFETY: only this fixture's retained visual/effect are mutated on their STA.
        unsafe {
            if enabled {
                self.visual.SetEffect(&self.blur)?;
            } else {
                self.visual.SetEffect(None::<&IDCompositionEffect>)?;
            }
            self.device.Commit()?;
            self.device.WaitForCommitCompletion()?;
            DwmFlush()
        }
    }
}

fn composition_stage<T>(
    stage: &'static str,
    result: windows_core::Result<T>,
) -> windows_core::Result<T> {
    result.inspect_err(|error| {
        eprintln!(
            "native-blur: composition_stage={stage} hresult={:08x}",
            error.code().0 as u32
        );
    })
}
impl Drop for NativeComposition {
    fn drop(&mut self) {
        // SAFETY: disconnect only this fixture's live target before dropping its exact owned HWND.
        unsafe {
            let _ = self.target.SetRoot(None::<&IDCompositionVisual>);
            let _ = self.device.Commit();
        }
    }
}

fn wait<T>(slot: &Rc<RefCell<Option<T>>>) -> Result<T, &'static str> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(value) = slot.borrow_mut().take() {
            return Ok(value);
        }
        if Instant::now() >= deadline || !pump_browser_exit_callbacks(deadline) {
            return Err("native_callback_deadline");
        }
    }
}

pub(super) fn preview(core: &ICoreWebView2, path: &Path) -> Result<image::RgbImage, &'static str> {
    preview_with_dimensions(core, path, (WIDTH as u32, HEIGHT as u32))
}

pub(super) fn preview_with_dimensions(
    core: &ICoreWebView2,
    path: &Path,
    expected: (u32, u32),
) -> Result<image::RgbImage, &'static str> {
    let slot = Rc::new(RefCell::new(None));
    let received = slot.clone();
    // SAFETY: a new bounded-by-verification fixture stream and exact live controller stay on the STA.
    let stream = unsafe { SHCreateMemStream(None) }.ok_or("preview_stream")?;
    let callback = CapturePreviewCompletedHandler::create(Box::new(move |result| {
        *received.borrow_mut() = Some(result.is_ok());
        Ok(())
    }));
    // SAFETY: WebView2 AddRefs this fixture's stream and callback until completion; both are retained below.
    unsafe {
        core.CapturePreview(
            COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
            &stream,
            &callback,
        )
    }
    .map_err(|_| "preview_dispatch")?;
    if !wait(&slot)? {
        return Err("preview_terminal");
    }
    let mut stat = STATSTG::default();
    // SAFETY: initialized outputs and the original completed stream are used on the same STA.
    unsafe { stream.Stat(&mut stat, STATFLAG_NONAME) }.map_err(|_| "preview_stat")?;
    if stat.cbSize == 0 || stat.cbSize > 4 * 1024 * 1024 {
        return Err("preview_size");
    }
    let mut bytes = vec![0; stat.cbSize as usize];
    let mut read = 0;
    // SAFETY: bytes has exactly the verified bounded size; the read output is initialized and valid.
    unsafe {
        stream
            .Seek(0, STREAM_SEEK_SET, None)
            .map_err(|_| "preview_seek")?;
        stream
            .Read(
                bytes.as_mut_ptr().cast(),
                bytes.len() as u32,
                Some(&mut read),
            )
            .ok()
            .map_err(|_| "preview_read")?;
    }
    if read as usize != bytes.len() {
        return Err("preview_short_read");
    }
    let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .map_err(|_| "preview_png")?
        .to_rgb8();
    if image.dimensions() != expected {
        eprintln!("native-fixture: capture_dimensions={:?} expected={expected:?}; fixture_pixels_only=true",image.dimensions());
        return Err("preview_viewport");
    }
    std::fs::write(path, bytes).map_err(|_| "preview_artifact")?;
    Ok(image)
}

pub(super) fn screen(hwnd: HWND, path: &Path) -> Result<image::RgbImage, &'static str> {
    // Read only the exact visible owned client crop; DComp output is absent from HWND GDI readback.
    const W: i32 = 384;
    const H: i32 = 192;
    // SAFETY: exact live owned HWND and writable BITMAPINFO/output are confined to this STA.
    unsafe {
        DwmFlush().map_err(|_| "screen_flush")?;
        // Keep a physical interior margin even on themed/bordered fixture windows.
        let mut origin = POINT { x: 32, y: 32 };
        ClientToScreen(hwnd, &mut origin)
            .ok()
            .map_err(|_| "screen_origin")?;
        if !IsWindowVisible(hwnd).as_bool() || origin.x < 0 || origin.y < 0 {
            return Err("screen_owned_client_hidden");
        }
        for x in [0, W / 2, W - 1] {
            for y in [0, H / 2, H - 1] {
                let point = POINT {
                    x: origin.x + x,
                    y: origin.y + y,
                };
                let hit = WindowFromPoint(point);
                if hit != hwnd && !IsChild(hwnd, hit).as_bool() {
                    return Err("screen_owned_client_occluded");
                }
            }
        }
        let source = GetDC(None);
        if source.0.is_null() {
            return Err("screen_dc");
        }
        let memory = CreateCompatibleDC(Some(source));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: W,
                biHeight: -H,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(Some(source), &info, DIB_RGB_COLORS, &mut bits, None, 0);
        let result = (|| {
            let bitmap = bitmap.as_ref().map_err(|_| "screen_bitmap")?;
            if memory.0.is_null() || bits.is_null() {
                return Err("screen_memory");
            }
            let previous = SelectObject(memory, (*bitmap).into());
            let copied = BitBlt(
                memory,
                0,
                0,
                W,
                H,
                Some(source),
                origin.x,
                origin.y,
                SRCCOPY | CAPTUREBLT,
            )
            .is_ok();
            let _ = SelectObject(memory, previous);
            if !copied {
                return Err("screen_copy");
            }
            let raw = std::slice::from_raw_parts(bits.cast::<u8>(), (W * H * 4) as usize);
            let image = image::RgbImage::from_fn(W as u32, H as u32, |x, y| {
                let index = ((y * W as u32 + x) * 4) as usize;
                image::Rgb([raw[index + 2], raw[index + 1], raw[index]])
            });
            image.save(path).map_err(|_| "screen_artifact")?;
            Ok(image)
        })();
        if let Ok(bitmap) = bitmap {
            let _ = DeleteObject(bitmap.into());
        }
        let _ = DeleteDC(memory);
        let _ = ReleaseDC(None, source);
        result
    }
}

fn contrast(image: &image::RgbImage) -> u64 {
    (64..160)
        .flat_map(|y| (64..320).map(move |x| (x, y)))
        .map(|(x, y)| {
            let first = image.get_pixel(x, y);
            let next = image.get_pixel(x + 1, y);
            first
                .0
                .iter()
                .zip(next.0)
                .map(|(a, b)| u64::from(a.abs_diff(b)))
                .sum::<u64>()
        })
        .sum()
}

fn chrome_matches_baseline(image: &image::RgbImage, baseline: &image::RgbImage) -> bool {
    let difference: u64 = (112..144)
        .flat_map(|y| (112..208).map(move |x| (x, y)))
        .map(|(x, y)| {
            image
                .get_pixel(x, y)
                .0
                .iter()
                .zip(baseline.get_pixel(x, y).0)
                .map(|(actual, expected)| u64::from(actual.abs_diff(expected)))
                .sum::<u64>()
        })
        .sum();
    let variation: u64 = (112..144)
        .flat_map(|y| (112..208).map(move |x| (x, y)))
        .map(|(x, y)| {
            image
                .get_pixel(x, y)
                .0
                .iter()
                .zip(image.get_pixel(x + 1, y).0)
                .map(|(first, next)| u64::from(first.abs_diff(next)))
                .sum::<u64>()
        })
        .sum();
    eprintln!("native-blur: chrome_exposed_baseline_difference={difference} chrome_exposed_variation={variation}");
    image.get_pixel(48, 48).0 == [0, 255, 0]
        && image.get_pixel(300, 100).0 == [24, 32, 40]
        && difference <= 32 * 96 * 3 * 3
        && variation >= 1_000
}

fn layered_child(hwnd: HWND) -> Result<(), &'static str> {
    // SAFETY: the caller retains this exact fixture-owned child; compatibility is proven by the resulting style.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED.0 as isize);
        SetLayeredWindowAttributes(
            hwnd,
            windows::Win32::Foundation::COLORREF(0),
            255,
            LWA_ALPHA,
        )
        .map_err(|error| {
            eprintln!(
                "native-blur: layered_alpha hresult={:08x} layered_style={}",
                error.code().0 as u32,
                GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_LAYERED.0 as isize != 0
            );
            "layered_alpha"
        })?;
        if GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_LAYERED.0 as isize == 0 {
            return Err("layered_style_unapplied");
        }
    }
    Ok(())
}

fn redirection_bitmap_alpha(hwnd: HWND) -> Result<(), &'static str> {
    // Public Windows SDK10.0.26100.0 um/dwmapi.h: the enumerator after
    // DWMWA_SYSTEMBACKDROP_TYPE(38) is DWMWA_REDIRECTIONBITMAP_ALPHA(39).
    // windows0.61 predates its binding. This fixture runs on Windows11 26200;
    // an unsupported API must refuse rather than imply transparent composition.
    const REDIRECTION_BITMAP_ALPHA: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(39);
    let enabled = 1u32;
    // SAFETY: set the documented BOOL attribute only on this retained fixture's exact chrome HWND.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            REDIRECTION_BITMAP_ALPHA,
            (&enabled as *const u32).cast(),
            std::mem::size_of::<u32>() as u32,
        )
    }
    .map_err(|error| {
        eprintln!(
            "native-blur: redirection_alpha_api=false hresult={:08x}",
            error.code().0 as u32
        );
        "redirection_bitmap_alpha"
    })?;
    eprintln!("native-blur: redirection_alpha_api=true");
    Ok(())
}

fn chrome_fixture(
    environment: &ICoreWebView2Environment,
    parent: &ProbeHostWindow,
) -> Result<wry::WebView, &'static str> {
    let loaded = Rc::new(RefCell::new(None));
    let received = loaded.clone();
    let overlay = WebViewBuilder::new()
        .with_environment(environment.clone())
        .with_html(CHROME)
        .with_transparent(true)
        .with_focused(false)
        .with_visible(true)
        .with_navigation_event_handler(move |event| {
            if event.phase == wry::NavigationEventPhase::Finished {
                *received.borrow_mut() = Some(true);
            }
        })
        .with_bounds(wry::Rect {
            position: wry::dpi::PhysicalPosition::new(0, 0).into(),
            size: wry::dpi::PhysicalSize::new(WIDTH as u32, HEIGHT as u32).into(),
        })
        .build_as_child(parent)
        .map_err(|_| "chrome_controller")?;
    wait(&loaded)?;
    let scale: ICoreWebView2Controller3 =
        overlay.controller().cast().map_err(|_| "chrome_scale")?;
    // SAFETY: exact owned chrome and fixed fixture scale on its STA; it remains above the native backdrop.
    unsafe {
        scale
            .SetShouldDetectMonitorScaleChanges(false)
            .map_err(|_| "chrome_scale_policy")?;
        scale
            .SetRasterizationScale(1.0)
            .map_err(|_| "chrome_scale_value")?;
        SetWindowPos(
            overlay.hwnd(),
            Some(HWND_TOP),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
        .map_err(|_| "chrome_z_order")?;
    }
    Ok(overlay)
}

fn navigate(core: &ICoreWebView2, page: &str) -> Result<(), &'static str> {
    let slot = Rc::new(RefCell::new(None));
    let received = slot.clone();
    let navigation = Rc::new(Cell::new(None));
    let starting_navigation = navigation.clone();
    let starting = NavigationStartingEventHandler::create(Box::new(move |_, event| {
        let mut id = 0;
        // SAFETY: read the supplied live event into an initialized scalar on the fixture STA.
        if event.is_some_and(|event| unsafe { event.NavigationId(&mut id) }.is_ok()) {
            starting_navigation.set(Some(id));
        }
        Ok(())
    }));
    let callback = NavigationCompletedEventHandler::create(Box::new(move |_, event| {
        let mut success = windows_core::BOOL::default();
        let mut id = 0;
        if let Some(event) = event {
            // SAFETY: query only this event's native identity with initialized writable output.
            if unsafe { event.NavigationId(&mut id) }.is_ok() && navigation.get() == Some(id) {
                // SAFETY: the same callback-owned event and initialized success output remain valid.
                let valid = unsafe { event.IsSuccess(&mut success) }.is_ok() && success.as_bool();
                *received.borrow_mut() = Some(valid);
            }
        }
        Ok(())
    }));
    let mut token = 0;
    let mut starting_token = 0;
    // SAFETY: retain callback until the exact navigation completes; retire the returned event token before return.
    let outcome = unsafe {
        core.add_NavigationStarting(&starting, &mut starting_token)
            .map_err(|_| "navigation_start_event")?;
        core.add_NavigationCompleted(&callback, &mut token)
            .map_err(|_| "navigation_event")?;
        core.NavigateToString(&HSTRING::from(page))
            .map_err(|_| "navigation_dispatch")
            .and_then(|()| wait(&slot))
            .and_then(|success| success.then_some(()).ok_or("navigation_terminal"))
    };
    // SAFETY: remove only the event token registered immediately above on this same live controller.
    unsafe { core.remove_NavigationCompleted(token) }.map_err(|_| "navigation_event_retire")?;
    // SAFETY: remove only the paired token registered for this fixture navigation.
    unsafe { core.remove_NavigationStarting(starting_token) }
        .map_err(|_| "navigation_start_event_retire")?;
    outcome
}

fn trusted_click(
    composition: &ICoreWebView2CompositionController,
    core: &ICoreWebView2,
) -> Result<(), &'static str> {
    // This isolated prototype has no product input authority: grant only this fixed synthetic button gesture.
    // SAFETY: input is sent only to the exact owned fixture, with fixed bounded viewport coordinates.
    unsafe {
        composition
            .SendMouseInput(
                COREWEBVIEW2_MOUSE_EVENT_KIND_LEFT_BUTTON_DOWN,
                COREWEBVIEW2_MOUSE_EVENT_VIRTUAL_KEYS_LEFT_BUTTON,
                0,
                POINT { x: 660, y: 230 },
            )
            .map_err(|_| "input_down")?;
        composition
            .SendMouseInput(
                COREWEBVIEW2_MOUSE_EVENT_KIND_LEFT_BUTTON_UP,
                COREWEBVIEW2_MOUSE_EVENT_VIRTUAL_KEYS_NONE,
                0,
                POINT { x: 660, y: 230 },
            )
            .map_err(|_| "input_up")?;
    }
    let slot = Rc::new(RefCell::new(None));
    let received = slot.clone();
    let callback = ExecuteScriptCompletedHandler::create(Box::new(move |result, value| {
        *received.borrow_mut() = Some(result.is_ok() && value == "\"trusted\"");
        Ok(())
    }));
    // SAFETY: fixed synthetic fixture expression; no external document or interpolated page data.
    unsafe { core.ExecuteScript(windows_core::w!("document.body.dataset.clicked"), &callback) }
        .map_err(|_| "input_verify_dispatch")?;
    if !wait(&slot)? {
        return Err("input_not_trusted");
    }
    Ok(())
}

#[test]
#[ignore = "interactive feasibility: requires an owned Win10-manifested EXE; transparent-chrome qualification currently fails"]
fn native_work_blur_hwnd_bridge_and_composition_control() {
    assert_eq!(run(), Ok(()));
}

fn run() -> Result<(), &'static str> {
    // SAFETY: this fixture changes only its STA's DPI projection and restores it after all HWNDs drop.
    let previous_dpi =
        unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    if previous_dpi.0.is_null() {
        return Err("fixture_dpi_context");
    }
    let _dpi = FixtureDpi(previous_dpi);
    let directory = tempfile::Builder::new()
        .prefix("zephium-native-blur-")
        .tempdir()
        .map_err(|_| "udf")?;
    let mut udf = OwnedUdf(Some(directory));
    let path = udf.0.as_ref().ok_or("udf_owner")?.path().to_owned();
    let artifacts =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/windows-work-blur-prototype");
    std::fs::create_dir_all(&artifacts).map_err(|_| "artifacts")?;
    let source = ProbeHostWindow::new().map_err(|_| "source_host")?;
    let receiver = ProbeHostWindow::new().map_err(|_| "receiver_host")?;
    // SAFETY: only these two fresh fixture windows are positioned; the receiver's client is used for pixel proof.
    unsafe {
        SetWindowPos(
            source.hwnd,
            Some(HWND_TOPMOST),
            20,
            20,
            WIDTH + 32,
            HEIGHT + 64,
            SWP_NOACTIVATE,
        )
        .map_err(|_| "source_position")?;
        let _ = ShowWindow(source.hwnd, SW_SHOWNOACTIVATE);
    }
    let mut context = WebContext::new(Some(path.clone()));
    let initial_loaded = Rc::new(RefCell::new(None));
    let initial_callback = initial_loaded.clone();
    let mut view = WebViewBuilder::new_with_web_context(&mut context)
        .with_html(PAGE)
        .with_navigation_event_handler(move |event| {
            if event.phase == wry::NavigationEventPhase::Finished {
                *initial_callback.borrow_mut() = Some(true);
            }
        })
        .with_focused(false)
        .with_visible(true)
        .with_bounds(wry::Rect {
            position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
            size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                WIDTH as u32,
                HEIGHT as u32,
            )),
        })
        .build_as_child(&source)
        .map_err(|_| "hwnd_view")?;
    let environment = view.environment();
    attest_environment(&environment, &path).map_err(|_| "environment")?;
    let process = browser_process_for_environment(&environment).map_err(|_| "process")?;
    let observer = install_browser_process_exit_observer(&environment, process.id(), |_| {})
        .map_err(|_| "observer")?;
    let mut composed = None;
    let mut composition_tree = None;
    let mut composition_child = None;
    let mut chrome = None;
    let mut bridge_chrome_proven = false;
    let mut all_chrome_closed = true;
    let outcome = (|| {
        wait(&initial_loaded)?;
        let core = view.webview();
        let scale: ICoreWebView2Controller3 = view.controller().cast().map_err(|_| "hwnd_scale")?;
        // SAFETY: set a fixed fixture-only CSS/physical ratio before loading its bounded page.
        unsafe {
            scale
                .SetShouldDetectMonitorScaleChanges(false)
                .map_err(|_| "hwnd_scale_policy")?;
            scale
                .SetRasterizationScale(1.0)
                .map_err(|_| "hwnd_scale_value")?;
        }
        navigate(&core, PAGE)?;
        let sharp = preview(&core, &artifacts.join("hwnd-sharp-preview.png"))?;
        if contrast(&sharp) < 100_000 {
            return Err("hwnd_fixture_not_sharp");
        }
        let screen_baseline = screen(source.hwnd, &artifacts.join("hwnd-screen-baseline.png"))?;
        if contrast(&screen_baseline) < 100_000 {
            return Err("owned_screen_acquisition_not_sharp");
        }
        let source_hwnd = view.hwnd();
        // SAFETY: retain the exact owned child's original style for every bridge exit.
        let style = unsafe { GetWindowLongPtrW(source_hwnd, GWL_EXSTYLE) };
        let bridge_result = (|| {
            // SAFETY: documented layered-window APIs affect only this owned Wry container, preserving full bounds.
            unsafe {
                SetWindowLongPtrW(source_hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED.0 as isize);
                SetLayeredWindowAttributes(
                    source_hwnd,
                    windows::Win32::Foundation::COLORREF(0),
                    255,
                    LWA_ALPHA,
                )
                .map_err(|error| {
                    eprintln!(
                        "native-blur: layered_alpha hresult={:08x} layered_style={}",
                        error.code().0 as u32,
                        GetWindowLongPtrW(source_hwnd, GWL_EXSTYLE) & WS_EX_LAYERED.0 as isize != 0
                    );
                    "layered_alpha"
                })?;
                SetWindowPos(
                    receiver.hwnd,
                    Some(HWND_TOPMOST),
                    20,
                    20,
                    WIDTH + 32,
                    HEIGHT + 64,
                    SWP_NOACTIVATE,
                )
                .map_err(|_| "receiver_position")?;
                let _ = ShowWindow(receiver.hwnd, SW_SHOWNOACTIVATE);
            }
            let bridge = NativeComposition::new(receiver.hwnd).map_err(|_| "bridge_device")?;
            // SAFETY: redirect only this exact layered fixture child into its owned target visual.
            let surface = unsafe { bridge.desktop.CreateSurfaceFromHwnd(source_hwnd) };
            match surface {
                Ok(surface) => {
                    // SAFETY: the source and composition target are retained; cloak uses a correctly sized BOOL scalar.
                    unsafe {
                        bridge
                            .visual
                            .SetContent(&surface)
                            .map_err(|_| "bridge_content")?;
                        let cloak = 1u32;
                        DwmSetWindowAttribute(
                            source_hwnd,
                            DWMWA_CLOAK,
                            (&cloak as *const u32).cast(),
                            std::mem::size_of::<u32>() as u32,
                        )
                        .map_err(|_| "source_cloak")?;
                    }
                    bridge.blurred(false).map_err(|_| "bridge_commit")?;
                    let raw = screen(receiver.hwnd, &artifacts.join("hwnd-bridge-raw.png"))?;
                    bridge.blurred(true).map_err(|_| "bridge_effect")?;
                    let blurred =
                        screen(receiver.hwnd, &artifacts.join("hwnd-bridge-blurred.png"))?;
                    let sharp_while_cloaked =
                        preview(&core, &artifacts.join("hwnd-cloaked-preview.png"))?;
                    eprintln!("native-blur: hwnd_bridge_api=true raw_contrast={} blurred_contrast={} capture_contrast={}",
                    contrast(&raw), contrast(&blurred), contrast(&sharp_while_cloaked));
                    if contrast(&raw) < 100_000
                        || contrast(&blurred) >= contrast(&raw) / 3
                        || contrast(&sharp_while_cloaked) < 100_000
                    {
                        return Err("hwnd_bridge_pixel_proof");
                    }
                    chrome = Some(chrome_fixture(&environment, &receiver)?);
                    layered_child(chrome.as_ref().ok_or("bridge_chrome_owner")?.hwnd())?;
                    redirection_bitmap_alpha(chrome.as_ref().ok_or("bridge_chrome_owner")?.hwnd())?;
                    bridge.blurred(true).map_err(|_| "bridge_chrome_commit")?;
                    let with_chrome = screen(
                        receiver.hwnd,
                        &artifacts.join("hwnd-bridge-layered-chrome.png"),
                    )?;
                    if !chrome_matches_baseline(&with_chrome, &blurred) {
                        return Err("hwnd_bridge_layered_chrome_proof");
                    }
                    navigate(&core, NEXT_PAGE)?;
                    let changed_preview = preview(
                        &core,
                        &artifacts.join("hwnd-layered-chrome-navigation-preview.png"),
                    )?;
                    bridge
                        .blurred(true)
                        .map_err(|_| "bridge_chrome_changed_commit")?;
                    let changed = screen(
                        receiver.hwnd,
                        &artifacts.join("hwnd-bridge-layered-chrome-changed.png"),
                    )?;
                    let color = changed.get_pixel(128, 128).0;
                    if color[1] > 8
                        || u16::from(color[0]) + u16::from(color[2]) < 200
                        || changed_preview.get_pixel(8, 100).0 != [0, 0, 255]
                        || changed.get_pixel(48, 48).0 != [0, 255, 0]
                        || changed.get_pixel(300, 100).0 != [24, 32, 40]
                    {
                        return Err("hwnd_bridge_source_change_proof");
                    }
                    bridge_chrome_proven = true;
                    eprintln!("native-blur: hwnd_bridge_layered_chrome=true differential_source_change=true sharp_capture=true sharp_controls=true");
                }
                Err(error) => eprintln!(
                    "native-blur: hwnd_bridge_api=false hresult={:08x}",
                    error.code().0 as u32
                ),
            }
            drop(bridge);
            Ok::<(), &'static str>(())
        })();
        if let Some(mut overlay) = chrome.take() {
            all_chrome_closed &= overlay.close().is_ok();
            drop(overlay);
            if !all_chrome_closed {
                return Err("bridge_chrome_close");
            }
        }
        // SAFETY: restore only the exact owned source even when the optional HWND bridge fails.
        unsafe {
            let cloak = 0u32;
            let _ = DwmSetWindowAttribute(
                source_hwnd,
                DWMWA_CLOAK,
                (&cloak as *const u32).cast(),
                std::mem::size_of::<u32>() as u32,
            );
            SetWindowLongPtrW(source_hwnd, GWL_EXSTYLE, style);
        }
        eprintln!("native-blur: hwnd_bridge_result={bridge_result:?}; positive_control_still_required=true");
        // Positive control always runs; wrapper success cannot stand in for actual webpage pixels.
        // SAFETY: show this exact fixture receiver independently of the optional bridge result.
        unsafe {
            SetWindowPos(
                receiver.hwnd,
                Some(HWND_TOPMOST),
                20,
                20,
                WIDTH + 32,
                HEIGHT + 64,
                SWP_NOACTIVATE,
            )
            .map_err(|_| "composition_host_position")?;
            let _ = ShowWindow(receiver.hwnd, SW_SHOWNOACTIVATE);
        }
        // SAFETY: create only a fixed full-viewport child of this owned fixture receiver.
        let child = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows_core::w!("STATIC"),
                windows_core::w!(""),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                0,
                0,
                WIDTH,
                HEIGHT,
                Some(receiver.hwnd),
                None,
                None,
                None,
            )
        }
        .map_err(|_| "composition_child")?;
        composition_child = Some(CompositionChild(child));
        let tree = NativeComposition::new(child).map_err(|_| "composition_device")?;
        let slot = Rc::new(RefCell::new(None));
        let received = slot.clone();
        let callback = CreateCoreWebView2CompositionControllerCompletedHandler::create(Box::new(
            move |result, controller| {
                *received.borrow_mut() = Some(result.and_then(|()| {
                    controller.ok_or_else(|| {
                        windows_core::Error::from_hresult(windows::Win32::Foundation::E_POINTER)
                    })
                }));
                Ok(())
            },
        ));
        let env3: ICoreWebView2Environment3 =
            environment.cast().map_err(|_| "composition_environment")?;
        // SAFETY: callback/output controller and owned receiver stay on the same STA.
        unsafe { env3.CreateCoreWebView2CompositionController(child, &callback) }
            .map_err(|_| "composition_dispatch")?;
        let composition = wait(&slot)?.map_err(|_| "composition_construct")?;
        let controller: ICoreWebView2Controller =
            composition.cast().map_err(|_| "composition_controller")?;
        composed = Some((composition.clone(), controller.clone()));
        let scale: ICoreWebView2Controller3 = controller.cast().map_err(|_| "composition_scale")?;
        // SAFETY: attach only the fixture's retained visual and fixed full viewport to its exact composition controller.
        unsafe {
            scale
                .SetShouldDetectMonitorScaleChanges(false)
                .map_err(|_| "composition_scale_policy")?;
            scale
                .SetRasterizationScale(1.0)
                .map_err(|_| "composition_scale_value")?;
            composition
                .SetRootVisualTarget(&tree.visual)
                .map_err(|_| "composition_root")?;
            controller
                .SetBounds(RECT {
                    left: 0,
                    top: 0,
                    right: WIDTH,
                    bottom: HEIGHT,
                })
                .map_err(|_| "composition_bounds")?;
            controller
                .SetIsVisible(true)
                .map_err(|_| "composition_visible")?;
            tree.device.Commit().map_err(|_| "composition_commit")?;
        }
        composition_tree = Some(tree);
        let tree = composition_tree.as_ref().ok_or("composition_owner")?;
        // SAFETY: this retained controller is live on its owning STA.
        let core = unsafe { controller.CoreWebView2() }.map_err(|_| "composition_core")?;
        navigate(&core, PAGE)?;
        let sharp = preview(&core, &artifacts.join("composition-sharp-preview.png"))?;
        tree.blurred(false).map_err(|_| "composition_raw_commit")?;
        let raw = screen(receiver.hwnd, &artifacts.join("composition-raw.png"))?;
        tree.blurred(true).map_err(|_| "composition_blur_commit")?;
        let blurred = screen(receiver.hwnd, &artifacts.join("composition-blurred.png"))?;
        let still_sharp = preview(
            &core,
            &artifacts.join("composition-blurred-sharp-preview.png"),
        )?;
        eprintln!("native-blur: composition raw_contrast={} blurred_contrast={} capture_contrast={} viewport={}x{}",
            contrast(&raw), contrast(&blurred), contrast(&still_sharp), sharp.width(), sharp.height());
        if contrast(&raw) < 100_000
            || contrast(&blurred) >= contrast(&raw) / 3
            || contrast(&still_sharp) < 100_000
        {
            return Err("composition_pixel_proof");
        }
        let loaded = Rc::new(RefCell::new(None));
        let received = loaded.clone();
        let overlay = WebViewBuilder::new()
            .with_environment(environment.clone())
            .with_html(CHROME)
            .with_transparent(true)
            .with_focused(false)
            .with_visible(true)
            .with_navigation_event_handler(move |event| {
                if event.phase == wry::NavigationEventPhase::Finished {
                    *received.borrow_mut() = Some(true);
                }
            })
            .with_bounds(wry::Rect {
                position: wry::dpi::PhysicalPosition::new(0, 0).into(),
                size: wry::dpi::PhysicalSize::new(WIDTH as u32, HEIGHT as u32).into(),
            })
            .build_as_child(&receiver)
            .map_err(|_| "chrome_controller")?;
        chrome = Some(overlay);
        wait(&loaded)?;
        let overlay = chrome.as_ref().ok_or("chrome_owner")?;
        layered_child(overlay.hwnd())?;
        redirection_bitmap_alpha(overlay.hwnd())?;
        let overlay_scale: ICoreWebView2Controller3 =
            overlay.controller().cast().map_err(|_| "chrome_scale")?;
        // SAFETY: fixed physical fixture coordinates and this exact chrome sibling are owned on the STA.
        unsafe {
            overlay_scale
                .SetShouldDetectMonitorScaleChanges(false)
                .map_err(|_| "chrome_scale_policy")?;
            overlay_scale
                .SetRasterizationScale(1.0)
                .map_err(|_| "chrome_scale_value")?;
            SetWindowPos(
                overlay.hwnd(),
                Some(HWND_TOP),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
            .map_err(|_| "chrome_z_order")?;
        }
        let chrome_image = screen(
            receiver.hwnd,
            &artifacts.join("composition-chrome-sibling.png"),
        )?;
        let exposed_difference: u64 = (112..144)
            .flat_map(|y| (112..208).map(move |x| (x, y)))
            .map(|(x, y)| {
                chrome_image
                    .get_pixel(x, y)
                    .0
                    .iter()
                    .zip(blurred.get_pixel(x, y).0)
                    .map(|(actual, expected)| u64::from(actual.abs_diff(expected)))
                    .sum::<u64>()
            })
            .sum();
        let exposed_variation: u64 = (112..144)
            .flat_map(|y| (112..208).map(move |x| (x, y)))
            .map(|(x, y)| {
                chrome_image
                    .get_pixel(x, y)
                    .0
                    .iter()
                    .zip(chrome_image.get_pixel(x + 1, y).0)
                    .map(|(first, next)| u64::from(first.abs_diff(next)))
                    .sum::<u64>()
            })
            .sum();
        eprintln!("native-blur: chrome_exposed_baseline_difference={exposed_difference} chrome_exposed_variation={exposed_variation}");
        let composition_chrome_proven = !(chrome_image.get_pixel(48, 48).0 != [0, 255, 0]
            || chrome_image.get_pixel(300, 100).0 != [24, 32, 40]
            || exposed_difference > 32 * 96 * 3 * 3
            || exposed_variation < 1_000);
        if !composition_chrome_proven && !bridge_chrome_proven {
            return Err("composition_chrome_pixel_proof");
        }
        let sharp_under_chrome = preview(
            &core,
            &artifacts.join("composition-chrome-sharp-preview.png"),
        )?;
        if contrast(&sharp_under_chrome) < 100_000 {
            return Err("composition_chrome_capture");
        }
        eprintln!("native-blur: composition_child_layered_chrome={composition_chrome_proven} hwnd_bridge_layered_chrome={bridge_chrome_proven} capture_sharp=true");
        if chrome.as_mut().is_none_or(|view| view.close().is_err()) {
            return Err("chrome_close");
        }
        drop(chrome.take());
        tree.blurred(false).map_err(|_| "human_effect_remove")?;
        trusted_click(&composition, &core)?;
        tree.blurred(true).map_err(|_| "passive_effect_restore")?;
        navigate(&core, NEXT_PAGE)?;
        let next = preview(
            &core,
            &artifacts.join("composition-navigation-sharp-preview.png"),
        )?;
        if contrast(&next) < 100_000 || next.get_pixel(8, 100).0 != [0, 0, 255] {
            return Err("composition_navigation_capture");
        }
        eprintln!("native-blur: positive_control screen_blurred=true capture_sharp=true navigation_capture=true fixed_native_input_trusted=true");
        Ok(())
    })();
    let chrome_closed =
        all_chrome_closed && chrome.as_mut().is_none_or(|view| view.close().is_ok());
    drop(chrome);
    let composition_closed = composed.take().is_none_or(|(composition, controller)| {
        // SAFETY: detach/close only this exact created controller before releasing its composition tree.
        unsafe {
            let _ = composition.SetRootVisualTarget(None::<&IUnknown>);
            controller.Close().is_ok()
        }
    });
    drop(composition_tree);
    drop(composition_child);
    let view_closed = view.close().is_ok();
    drop(view);
    drop(context);
    drop(environment);
    let exited =
        wait_for_browser_process_exit(&process, &observer.proof(), Instant::now() + TIMEOUT);
    drop(observer);
    drop(process);
    drop(receiver);
    drop(source);
    eprintln!("native-blur: cleanup hwnd_closed={view_closed} composition_closed={composition_closed} chrome_closed={chrome_closed} process_exited={exited}");
    if view_closed && composition_closed && chrome_closed && exited {
        udf.0
            .take()
            .ok_or("udf_cleanup_owner")?
            .close()
            .map_err(|_| "udf_cleanup")?;
    } else {
        return outcome.and(Err("native_cleanup"));
    }
    outcome
}
