//! Bounded WKWebView viewport capture and metadata-free PNG transport.
//!
//! WebKit paints the exact hidden owned view asynchronously. ImageIO writes
//! directly into a Rust-owned capped consumer, so no Foundation-backed or
//! unbounded intermediate encoded buffer can exist. The functional core
//! remains responsible for PNG structure/CRC validation and metadata removal.

use std::cell::RefCell;
use std::ffi::c_void;
use std::panic::AssertUnwindSafe;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use block2::RcBlock;
use objc2_core_foundation::CFString;
use objc2_core_graphics::{CGDataConsumer, CGDataConsumerCallbacks, CGImage};
use objc2_foundation::{MainThreadMarker, NSNumber};
use objc2_web_kit::WKSnapshotConfiguration;
use wry::WebViewExtMacOS as _;
use zephium_agentic::{
    SemanticCaptureInstant, SemanticScreenshotBudget, SemanticScreenshotNativeCapture,
    SemanticScreenshotNativeFailure, SemanticScreenshotNativeRequest,
    SemanticScreenshotPaintEvidence,
};

const PNG_TYPE_IDENTIFIER: &str = "public.png";
const MIN_ENCODER_GROWTH: usize = 64 * 1024;

type NativeCompletion = Box<
    dyn FnOnce(Result<SemanticScreenshotNativeCapture, SemanticScreenshotNativeFailure>) + 'static,
>;
type PanicCallback = Box<dyn Fn() + 'static>;

struct PendingCapture {
    request: SemanticScreenshotNativeRequest,
    completion: NativeCompletion,
    callback_panicked: PanicCallback,
    admitted_at: Instant,
    started_at: SemanticCaptureInstant,
    cancelled: Arc<AtomicBool>,
}

struct BoundedPngSink {
    bytes: Vec<u8>,
    limit: usize,
    failed: bool,
}

impl BoundedPngSink {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            failed: false,
        }
    }

    fn append(&mut self, bytes: &[u8]) -> usize {
        if self.failed {
            return 0;
        }
        let Some(end) = self.bytes.len().checked_add(bytes.len()) else {
            self.failed = true;
            return 0;
        };
        if end > self.limit {
            self.failed = true;
            return 0;
        }
        if end > self.bytes.capacity() {
            let growth = self.bytes.capacity().max(MIN_ENCODER_GROWTH);
            let desired = end
                .max(self.bytes.capacity().saturating_add(growth))
                .min(self.limit);
            if self
                .bytes
                .try_reserve_exact(desired.saturating_sub(self.bytes.capacity()))
                .is_err()
            {
                self.failed = true;
                return 0;
            }
        }
        self.bytes.extend_from_slice(bytes);
        bytes.len()
    }
}

unsafe extern "C-unwind" fn put_png_bytes(
    info: *mut c_void,
    buffer: NonNull<c_void>,
    count: usize,
) -> usize {
    let written = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let Some(state) = (info as *const RefCell<BoundedPngSink>).as_ref() else {
            return 0;
        };
        let Ok(mut state) = state.try_borrow_mut() else {
            return 0;
        };
        // SAFETY: CoreGraphics promises `buffer` addresses `count` readable
        // bytes for the duration of this consumer callback.
        let bytes = unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), count) };
        state.append(bytes)
    }));
    written.unwrap_or(0)
}

struct ImageDestination(NonNull<c_void>);

impl Drop for ImageDestination {
    fn drop(&mut self) {
        // SAFETY: ImageIO returned this destination at +1 under the Create
        // rule and this wrapper is its sole owner.
        unsafe { CFRelease(self.0.as_ptr()) };
    }
}

#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C-unwind" {
    fn CGImageDestinationCreateWithDataConsumer(
        consumer: Option<&CGDataConsumer>,
        type_identifier: Option<&CFString>,
        image_count: usize,
        options: *const c_void,
    ) -> *mut c_void;
    fn CGImageDestinationAddImage(
        destination: *mut c_void,
        image: Option<&CGImage>,
        properties: *const c_void,
    );
    fn CGImageDestinationFinalize(destination: *mut c_void) -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C-unwind" {
    fn CFRelease(value: *mut c_void);
}

pub(super) fn capture_viewport(
    view: &wry::WebView,
    request: SemanticScreenshotNativeRequest,
    admitted_at: Instant,
    cancelled: Arc<AtomicBool>,
    completion: impl FnOnce(Result<SemanticScreenshotNativeCapture, SemanticScreenshotNativeFailure>)
        + 'static,
    callback_panicked: impl Fn() + 'static,
) -> Result<(), SemanticScreenshotNativeFailure> {
    if cancelled.load(Ordering::Acquire) {
        return Err(SemanticScreenshotNativeFailure::Cancelled);
    }
    let mtm = MainThreadMarker::new().ok_or(SemanticScreenshotNativeFailure::Transport)?;
    let webview = view.webview();
    let bounds = webview.bounds();
    let window = webview
        .window()
        .ok_or(SemanticScreenshotNativeFailure::NotReady)?;
    let scale = window.backingScaleFactor();
    let snapshot_width = fitted_snapshot_width(
        bounds.size.width,
        bounds.size.height,
        scale,
        request.budget(),
    )
    .ok_or(SemanticScreenshotNativeFailure::NotReady)?;
    let started_at = mapped_instant(request.requested_at(), admitted_at, Instant::now())
        .ok_or(SemanticScreenshotNativeFailure::TimedOut)?;
    if started_at > request.deadline() {
        return Err(SemanticScreenshotNativeFailure::TimedOut);
    }

    let configuration = unsafe { WKSnapshotConfiguration::new(mtm) };
    let width = NSNumber::new_f64(snapshot_width);
    unsafe {
        configuration.setRect(bounds);
        configuration.setSnapshotWidth(Some(&width));
        configuration.setAfterScreenUpdates(true);
    }

    let pending = Rc::new(RefCell::new(Some(PendingCapture {
        request,
        completion: Box::new(completion),
        callback_panicked: Box::new(callback_panicked),
        admitted_at,
        started_at,
        cancelled,
    })));
    let callback_pending = pending.clone();
    let callback: RcBlock<dyn Fn(*mut objc2_app_kit::NSImage, *mut objc2_foundation::NSError)> =
        RcBlock::new(move |image, error| {
            let Some(pending) = callback_pending.borrow_mut().take() else {
                return;
            };
            let PendingCapture {
                request,
                completion,
                callback_panicked,
                admitted_at,
                started_at,
                cancelled,
            } = pending;
            let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
                objc2::exception::catch(AssertUnwindSafe(|| {
                    finish_capture(request, admitted_at, started_at, &cancelled, image, error)
                }))
                .unwrap_or(Err(SemanticScreenshotNativeFailure::Transport))
            }))
            .unwrap_or(Err(SemanticScreenshotNativeFailure::Transport));
            if std::panic::catch_unwind(AssertUnwindSafe(|| completion(outcome))).is_err() {
                let _ = std::panic::catch_unwind(AssertUnwindSafe(callback_panicked));
            }
        });

    let dispatched = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
        webview.takeSnapshotWithConfiguration_completionHandler(Some(&configuration), &callback);
    }));
    if dispatched.is_err() {
        pending.borrow_mut().take();
        return Err(SemanticScreenshotNativeFailure::Transport);
    }
    Ok(())
}

fn finish_capture(
    request: SemanticScreenshotNativeRequest,
    admitted_at: Instant,
    started_at: SemanticCaptureInstant,
    cancelled: &AtomicBool,
    image: *mut objc2_app_kit::NSImage,
    error: *mut objc2_foundation::NSError,
) -> Result<SemanticScreenshotNativeCapture, SemanticScreenshotNativeFailure> {
    if cancelled.load(Ordering::Acquire) {
        return Err(SemanticScreenshotNativeFailure::Cancelled);
    }
    if image.is_null() || !error.is_null() {
        return Err(SemanticScreenshotNativeFailure::Transport);
    }
    // SAFETY: WebKit guarantees a non-null image pointer remains valid for
    // the duration of its completion callback.
    let image = unsafe { &*image };
    let cg_image =
        unsafe { image.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None) }
            .ok_or(SemanticScreenshotNativeFailure::Transport)?;
    let (width, height, png) = encode_bounded_png(&cg_image, request.budget())?;
    if cancelled.load(Ordering::Acquire) {
        return Err(SemanticScreenshotNativeFailure::Cancelled);
    }
    let completed_at = mapped_instant(request.requested_at(), admitted_at, Instant::now())
        .ok_or(SemanticScreenshotNativeFailure::TimedOut)?;
    if completed_at > request.deadline() {
        return Err(SemanticScreenshotNativeFailure::TimedOut);
    }
    Ok(request.complete(
        SemanticScreenshotPaintEvidence::ExactDocumentContentAvailable,
        started_at,
        completed_at,
        width,
        height,
        png,
    ))
}

fn encode_bounded_png(
    image: &CGImage,
    budget: SemanticScreenshotBudget,
) -> Result<(u32, u32, Vec<u8>), SemanticScreenshotNativeFailure> {
    let width = u32::try_from(CGImage::width(Some(image)))
        .map_err(|_| SemanticScreenshotNativeFailure::ResourceExhausted)?;
    let height = u32::try_from(CGImage::height(Some(image)))
        .map_err(|_| SemanticScreenshotNativeFailure::ResourceExhausted)?;
    let pixels = width
        .checked_mul(height)
        .ok_or(SemanticScreenshotNativeFailure::ResourceExhausted)?;
    if width == 0
        || height == 0
        || width > u32::from(budget.max_width())
        || height > u32::from(budget.max_height())
        || pixels > budget.max_pixels()
    {
        return Err(SemanticScreenshotNativeFailure::ResourceExhausted);
    }
    if CGImage::bits_per_component(Some(image)) != 8 {
        return Err(SemanticScreenshotNativeFailure::Transport);
    }

    let sink = RefCell::new(BoundedPngSink::new(
        usize::try_from(budget.max_png_bytes())
            .map_err(|_| SemanticScreenshotNativeFailure::ResourceExhausted)?,
    ));
    let callbacks = CGDataConsumerCallbacks {
        putBytes: Some(put_png_bytes),
        releaseConsumer: None,
    };
    let consumer = unsafe {
        CGDataConsumer::new(
            std::ptr::from_ref(&sink).cast_mut().cast::<c_void>(),
            &callbacks,
        )
    }
    .ok_or(SemanticScreenshotNativeFailure::Transport)?;
    let type_identifier = CFString::from_static_str(PNG_TYPE_IDENTIFIER);
    let destination = unsafe {
        CGImageDestinationCreateWithDataConsumer(
            Some(&consumer),
            Some(&type_identifier),
            1,
            std::ptr::null(),
        )
    };
    let destination = NonNull::new(destination)
        .map(ImageDestination)
        .ok_or(SemanticScreenshotNativeFailure::Transport)?;
    unsafe {
        CGImageDestinationAddImage(destination.0.as_ptr(), Some(image), std::ptr::null());
    }
    let finalized = unsafe { CGImageDestinationFinalize(destination.0.as_ptr()) };
    drop(destination);
    drop(consumer);
    let mut sink = sink.into_inner();
    if sink.failed {
        return Err(SemanticScreenshotNativeFailure::ResourceExhausted);
    }
    if !finalized || sink.bytes.is_empty() {
        return Err(SemanticScreenshotNativeFailure::Transport);
    }
    Ok((width, height, std::mem::take(&mut sink.bytes)))
}

fn mapped_instant(
    requested_at: SemanticCaptureInstant,
    admitted_at: Instant,
    now: Instant,
) -> Option<SemanticCaptureInstant> {
    let elapsed = u64::try_from(now.saturating_duration_since(admitted_at).as_millis()).ok()?;
    requested_at
        .millis()
        .checked_add(elapsed)
        .map(SemanticCaptureInstant::from_millis)
}

fn fitted_snapshot_width(
    width_points: f64,
    height_points: f64,
    backing_scale: f64,
    budget: SemanticScreenshotBudget,
) -> Option<f64> {
    if !width_points.is_finite()
        || !height_points.is_finite()
        || !backing_scale.is_finite()
        || width_points <= 0.0
        || height_points <= 0.0
        || backing_scale <= 0.0
    {
        return None;
    }
    let width_pixels = width_points * backing_scale;
    let height_pixels = height_points * backing_scale;
    let area = width_pixels * height_pixels;
    if !width_pixels.is_finite() || !height_pixels.is_finite() || !area.is_finite() || area <= 0.0 {
        return None;
    }
    // Leave one physical pixel on each dimension for native rounding.
    let max_width = f64::from(budget.max_width().saturating_sub(1).max(1));
    let max_height = f64::from(budget.max_height().saturating_sub(1).max(1));
    let pixel_margin = u32::from(budget.max_width())
        .saturating_add(u32::from(budget.max_height()))
        .min(budget.max_pixels().saturating_sub(1));
    let max_pixels = f64::from(budget.max_pixels().saturating_sub(pixel_margin).max(1));
    let fit = 1.0_f64
        .min(max_width / width_pixels)
        .min(max_height / height_pixels)
        .min((max_pixels / area).sqrt());
    let target_physical_width = (width_pixels * fit).floor().max(1.0);
    let snapshot_width = (target_physical_width / backing_scale).min(width_points);
    (snapshot_width.is_finite() && snapshot_width > 0.0).then_some(snapshot_width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_core_graphics::{
        CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGImageAlphaInfo,
        CGImageByteOrderInfo,
    };

    #[test]
    fn bounded_sink_never_grows_past_the_encoded_limit() {
        let mut sink = BoundedPngSink::new(8);
        assert_eq!(sink.append(&[1, 2, 3, 4]), 4);
        assert_eq!(sink.append(&[5, 6, 7, 8]), 4);
        assert_eq!(sink.bytes.len(), 8);
        assert!(sink.bytes.capacity() <= 8);
        assert_eq!(sink.append(&[9]), 0);
        assert!(sink.failed);
        assert_eq!(sink.bytes.len(), 8);
    }

    #[test]
    fn sizing_is_finite_aspect_preserving_and_conservative() {
        let width = fitted_snapshot_width(3840.0, 2160.0, 2.0, SemanticScreenshotBudget::STANDARD)
            .expect("sizing");
        let output_width = width * 2.0;
        let output_height = output_width * 2160.0 / 3840.0;
        assert!(output_width < f64::from(SemanticScreenshotBudget::STANDARD.max_width()));
        assert!(output_height < f64::from(SemanticScreenshotBudget::STANDARD.max_height()));
        assert!(
            output_width * output_height
                < f64::from(SemanticScreenshotBudget::STANDARD.max_pixels())
        );
        assert!(
            fitted_snapshot_width(f64::NAN, 800.0, 2.0, SemanticScreenshotBudget::STANDARD)
                .is_none()
        );
    }

    #[test]
    fn image_io_streams_synthetic_pixels_and_refuses_before_byte_overrun() {
        let mut rgba = [0x20_u8, 0x40, 0x80, 0xff];
        let color_space = CGColorSpace::new_device_rgb().expect("color space");
        let context = unsafe {
            CGBitmapContextCreate(
                rgba.as_mut_ptr().cast(),
                1,
                1,
                8,
                4,
                Some(&color_space),
                CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0,
            )
        }
        .expect("bitmap context");
        let image = CGBitmapContextCreateImage(Some(&context)).expect("image");
        drop(context);

        let (width, height, png) =
            encode_bounded_png(&image, SemanticScreenshotBudget::STANDARD).expect("PNG encode");
        assert_eq!((width, height), (1, 1));
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(png.len() <= SemanticScreenshotBudget::STANDARD.max_png_bytes() as usize);
        assert_eq!(png.get(8..16), Some(b"\0\0\0\rIHDR".as_slice()));
        assert_eq!(png.get(16..24), Some([0, 0, 0, 1, 0, 0, 0, 1].as_slice()));
        assert_eq!(png.get(24), Some(&8));
        assert!(matches!(png.get(25), Some(2 | 6)));
        assert_eq!(png.get(26..29), Some([0, 0, 0].as_slice()));

        let tiny = SemanticScreenshotBudget::try_new(1, 1, 1, 57).expect("tiny budget");
        assert!(matches!(
            encode_bounded_png(&image, tiny),
            Err(SemanticScreenshotNativeFailure::ResourceExhausted)
        ));
    }
}
