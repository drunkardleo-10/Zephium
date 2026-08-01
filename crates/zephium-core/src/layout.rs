//! Pure window layout in top-left logical coordinates. Chrome and content tile
//! without overlap, so the two webviews never contend for the cursor.

use crate::geometry::{Rect, Size};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Sidebar,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub sidebar_width: f64,
    pub padding: f64,
    pub gap: f64,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            sidebar_width: 240.0,
            padding: 8.0,
            gap: 8.0,
        }
    }
}

/// The sidebar's admitted width range, spanning the compact rail through the
/// widest expanded column.
///
/// This is the single authority. It used to be restated in the shell actor and
/// again in the privileged IPC guard, which let the rail pass the boundary
/// check and then get silently widened back by the actor, so the chrome and
/// the content region disagreed about where the sidebar ended.
pub const MIN_SIDEBAR_WIDTH: f64 = 56.0;
pub const MAX_SIDEBAR_WIDTH: f64 = 420.0;

pub fn clamp_sidebar_width(width: f64) -> f64 {
    if !width.is_finite() {
        return MIN_SIDEBAR_WIDTH;
    }
    width.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub chrome: Rect,
    // None on an empty tab: chrome fills the inner area and renders the NTP.
    pub content: Option<Rect>,
}

pub fn compute(window: Size, mode: Mode, m: Metrics, content_present: bool) -> Layout {
    match mode {
        Mode::Sidebar => sidebar(window, m, content_present),
    }
}

fn sidebar(win: Size, m: Metrics, content_present: bool) -> Layout {
    let p = m.padding;
    let inner_h = win.height - 2.0 * p;

    if !content_present {
        return Layout {
            chrome: Rect::new(p, p, win.width - 2.0 * p, inner_h),
            content: None,
        };
    }

    let chrome = Rect::new(p, p, m.sidebar_width, inner_h);
    let content_x = p + m.sidebar_width + m.gap;
    let content = Rect::new(content_x, p, win.width - content_x - p, inner_h);
    Layout {
        chrome,
        content: Some(content),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sidebar_clamp_admits_the_rail_and_rejects_nonsense() {
        assert_eq!(clamp_sidebar_width(MIN_SIDEBAR_WIDTH), MIN_SIDEBAR_WIDTH);
        assert_eq!(clamp_sidebar_width(MAX_SIDEBAR_WIDTH), MAX_SIDEBAR_WIDTH);
        assert_eq!(clamp_sidebar_width(0.0), MIN_SIDEBAR_WIDTH);
        assert_eq!(clamp_sidebar_width(10_000.0), MAX_SIDEBAR_WIDTH);
        assert_eq!(clamp_sidebar_width(f64::NAN), MIN_SIDEBAR_WIDTH);
        assert_eq!(clamp_sidebar_width(240.0), 240.0);
    }

    const M: Metrics = Metrics {
        sidebar_width: 240.0,
        padding: 8.0,
        gap: 8.0,
    };

    #[test]
    fn sidebar_with_content_tiles_without_overlap() {
        let l = compute(Size::new(1200.0, 800.0), Mode::Sidebar, M, true);
        assert_eq!(l.chrome, Rect::new(8.0, 8.0, 240.0, 784.0));
        let content = l.content.unwrap();
        assert_eq!(content, Rect::new(256.0, 8.0, 936.0, 784.0));
        // chrome right edge + gap == content left edge: no overlap.
        assert_eq!(l.chrome.x + l.chrome.width + M.gap, content.x);
    }

    #[test]
    fn empty_tab_lets_chrome_fill_inner_area() {
        let l = compute(Size::new(1200.0, 800.0), Mode::Sidebar, M, false);
        assert_eq!(l.chrome, Rect::new(8.0, 8.0, 1184.0, 784.0));
        assert!(l.content.is_none());
    }

    #[test]
    fn degenerate_window_clamps_to_nonnegative() {
        let l = compute(Size::new(10.0, 10.0), Mode::Sidebar, M, true);
        let content = l.content.unwrap();
        assert!(content.width >= 0.0 && content.height >= 0.0);
        assert!(l.chrome.height >= 0.0);
    }
}
