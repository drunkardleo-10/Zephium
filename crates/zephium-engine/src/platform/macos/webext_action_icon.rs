//! Fixed-cost native action-icon rasterization.
//!
//! WebKit owns extension image decoding and returns an `NSImage`. The browser
//! converts that trusted native result into one exact 32x32 RGBA buffer so the
//! Shell and privileged frame never retain native image graphs or decode an
//! extension-controlled image format.

use std::panic::AssertUnwindSafe;

use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
    CGImageByteOrderInfo, CGInterpolationQuality,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use objc2_web_kit::WKWebExtensionAction;
use zephium_core::extensions::{
    ExtensionActionIcon, EXTENSION_ACTION_ICON_HEIGHT, EXTENSION_ACTION_ICON_RGBA_BYTES,
    EXTENSION_ACTION_ICON_WIDTH,
};

pub(crate) fn rasterize_action_icon(action: &WKWebExtensionAction) -> Option<ExtensionActionIcon> {
    objc2::exception::catch(AssertUnwindSafe(|| rasterize_action_icon_inner(action)))
        .ok()
        .flatten()
}

fn rasterize_action_icon_inner(action: &WKWebExtensionAction) -> Option<ExtensionActionIcon> {
    let size = NSSize::new(
        EXTENSION_ACTION_ICON_WIDTH as f64,
        EXTENSION_ACTION_ICON_HEIGHT as f64,
    );
    let image = unsafe { action.iconForSize(size) }?;
    let mut proposed = NSRect::new(NSPoint::new(0.0, 0.0), size);
    let image = unsafe { image.CGImageForProposedRect_context_hints(&mut proposed, None, None) }?;
    rasterize(&image)
}

fn bitmap(rgba: &mut [u8]) -> Option<CFRetained<CGContext>> {
    let color_space = CGColorSpace::new_device_rgb()?;
    unsafe {
        CGBitmapContextCreate(
            rgba.as_mut_ptr().cast(),
            EXTENSION_ACTION_ICON_WIDTH,
            EXTENSION_ACTION_ICON_HEIGHT,
            8,
            EXTENSION_ACTION_ICON_WIDTH * 4,
            Some(&color_space),
            CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0,
        )
    }
}

/// A bitmap context's memory already starts with the image's top row, which
/// is the order `ImageData` reads, so the image is drawn untransformed.
fn rasterize(image: &CGImage) -> Option<ExtensionActionIcon> {
    let mut rgba = vec![0_u8; EXTENSION_ACTION_ICON_RGBA_BYTES];
    let context = bitmap(&mut rgba)?;
    CGContext::set_interpolation_quality(Some(&context), CGInterpolationQuality::High);
    CGContext::draw_image(
        Some(&context),
        NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(
                EXTENSION_ACTION_ICON_WIDTH as f64,
                EXTENSION_ACTION_ICON_HEIGHT as f64,
            ),
        ),
        Some(image),
    );
    // The bitmap context retains the raw destination pointer. Release that
    // native alias before Rust reads or mutates the backing allocation.
    drop(context);
    unpremultiply_rgba(&mut rgba);
    ExtensionActionIcon::from_rgba(rgba).ok()
}

fn unpremultiply_rgba(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        if alpha == 0 {
            pixel[..3].fill(0);
            continue;
        }
        if alpha == 255 {
            continue;
        }
        for channel in &mut pixel[..3] {
            let straight = (u32::from(*channel) * 255 + alpha / 2) / alpha;
            *channel = straight.min(255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_keep_their_top_row_first() {
        use objc2_core_graphics::CGBitmapContextCreateImage;

        let mut source = vec![0_u8; EXTENSION_ACTION_ICON_RGBA_BYTES];
        let context = bitmap(&mut source).expect("bitmap");
        CGContext::set_rgb_fill_color(Some(&context), 1.0, 0.0, 0.0, 1.0);
        // CoreGraphics' origin is the lower left, so this is the top half.
        CGContext::fill_rect(
            Some(&context),
            NSRect::new(NSPoint::new(0.0, 16.0), NSSize::new(32.0, 16.0)),
        );
        let image = CGBitmapContextCreateImage(Some(&context)).expect("image");
        drop(context);

        let icon = rasterize(&image).expect("icon");
        let rgba = icon.rgba();
        assert_eq!(&rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(rgba[rgba.len() - 1], 0);
    }

    #[test]
    fn rgba_unpremultiplication_is_bounded_and_preserves_alpha() {
        let mut rgba = [64, 32, 16, 128, 7, 8, 9, 0, 10, 20, 30, 255];
        unpremultiply_rgba(&mut rgba);
        assert_eq!(rgba, [128, 64, 32, 128, 0, 0, 0, 0, 10, 20, 30, 255]);

        let mut malformed = [255, 255, 255, 1];
        unpremultiply_rgba(&mut malformed);
        assert_eq!(malformed, [255, 255, 255, 1]);
    }
}
