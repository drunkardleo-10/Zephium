//! Native pane-size admission shared by all platform stages.
//!
//! Deeply nested, minimum-ratio splits can legitimately collapse below one
//! device pixel while a window is small. Passing a rounded zero extent to a
//! native webview is either rejected (WebView2) or silently promoted to a
//! misleading one-pixel surface (GTK). Treat that geometry as temporarily
//! non-paintable; the next authoritative layout can admit it again after the
//! window grows.

pub(crate) fn rounded_native_size(width: f64, height: f64, scale: f64) -> Option<(i32, i32)> {
    if !width.is_finite()
        || !height.is_finite()
        || !scale.is_finite()
        || width <= 0.0
        || height <= 0.0
        || scale <= 0.0
    {
        return None;
    }

    let width = (width * scale).round();
    let height = (height * scale).round();
    if !width.is_finite()
        || !height.is_finite()
        || width < 1.0
        || height < 1.0
        || width > i32::MAX as f64
        || height > i32::MAX as f64
    {
        return None;
    }

    Some((width as i32, height as i32))
}

#[cfg(test)]
mod tests {
    use super::rounded_native_size;

    #[test]
    fn rounded_zero_extent_is_temporarily_non_paintable() {
        assert_eq!(rounded_native_size(0.49, 20.0, 1.0), None);
        assert_eq!(rounded_native_size(20.0, 0.49, 1.0), None);
        assert_eq!(rounded_native_size(0.24, 20.0, 2.0), None);
    }

    #[test]
    fn a_later_larger_layout_is_admitted_again() {
        assert_eq!(rounded_native_size(0.49, 20.0, 1.0), None);
        assert_eq!(rounded_native_size(0.51, 20.0, 1.0), Some((1, 20)));
        assert_eq!(rounded_native_size(0.26, 20.0, 2.0), Some((1, 40)));
    }

    #[test]
    fn malformed_or_unrepresentable_geometry_fails_closed() {
        assert_eq!(rounded_native_size(f64::NAN, 10.0, 1.0), None);
        assert_eq!(rounded_native_size(10.0, f64::INFINITY, 1.0), None);
        assert_eq!(rounded_native_size(10.0, 10.0, 0.0), None);
        assert_eq!(rounded_native_size(i32::MAX as f64 + 1.0, 10.0, 1.0), None);
    }
}
