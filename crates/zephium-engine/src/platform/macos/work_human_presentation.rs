//! Exclusive native input owner, retained before exposing the agent page.
#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]
use objc2::rc::Retained;
use objc2_app_kit::{NSView, NSWindow, NSWindowOrderingMode};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use objc2_web_kit::WKWebView;
use std::time::Instant;
use zephium_agentic::WorkBrowserHumanRegion;

pub(crate) struct WorkHumanPresentation {
    page: Retained<WKWebView>,
    parent: Retained<NSView>,
    window: Retained<NSWindow>,
    original: NSRect,
    region: NSRect,
    deadline: Instant,
    presented: bool,
    retired: bool,
}
impl WorkHumanPresentation {
    pub(crate) fn prepare(
        view: &wry::WebView,
        region: WorkBrowserHumanRegion,
        deadline: Instant,
    ) -> Option<Self> {
        MainThreadMarker::new()?;
        let page = super::native_webview(view);
        let window = page.window()?;
        // SAFETY: the view is retained on its owning AppKit thread.
        let parent = unsafe { page.superview() }?;
        if !page.isHidden()
            || !window.isVisible()
            || window.isMiniaturized()
            || Instant::now() >= deadline
        {
            return None;
        }
        let [x, y, width, height] = region.components().map(f64::from);
        let bounds = parent.bounds();
        if x + width > bounds.size.width || y + height > bounds.size.height {
            return None;
        }
        let y = if parent.isFlipped() {
            y
        } else {
            bounds.size.height - y - height
        };
        let region = NSRect::new(
            NSPoint::new(bounds.origin.x + x, bounds.origin.y + y),
            NSSize::new(width, height),
        );
        let original = page.frame();
        Some(Self {
            page,
            parent,
            window,
            original,
            region,
            deadline,
            presented: false,
            retired: false,
        })
    }
    pub(crate) fn present(&mut self) -> bool {
        if self.presented || self.retired || Instant::now() >= self.deadline {
            return false;
        }
        self.parent
            .addSubview_positioned_relativeTo(&self.page, NSWindowOrderingMode::Above, None);
        self.page.setFrame(self.region);
        super::passive_page::unregister(&self.page);
        self.page.setHidden(false);
        self.presented = true;
        self.current()
    }
    pub(crate) fn current(&self) -> bool {
        self.presented && !self.retired && Instant::now() < self.deadline
            && self.window.isVisible() && !self.window.isMiniaturized()
            && !self.page.isHidden() && self.page.frame() == self.region
            && self.page.window().is_some_and(|window| std::ptr::eq(&*window, &*self.window))
            // SAFETY: retained native identities on the main thread.
            && unsafe { self.page.superview() }.is_some_and(|parent| std::ptr::eq(&*parent, &*self.parent))
    }
    pub(crate) fn visible_for_audit(&self) -> bool {
        !self.page.isHidden()
    }
    pub(crate) fn retire(&mut self) -> bool {
        if self.retired {
            return self.page.isHidden();
        }
        let fenced = super::passive_page::register(&self.page);
        self.page.setHidden(true);
        let owns_responder = self.window.firstResponder().is_some_and(|responder| {
            responder.downcast_ref::<NSView>().is_some_and(|view| {
                std::ptr::eq(view, &**self.page) || view.isDescendantOf(&self.page)
            })
        });
        let responder_released = !owns_responder || self.window.makeFirstResponder(None);
        self.page.setFrame(self.original);
        self.parent
            .addSubview_positioned_relativeTo(&self.page, NSWindowOrderingMode::Below, None);
        self.retired = fenced
            && responder_released
            && self.page.isHidden()
            && self.page.frame() == self.original;
        self.retired
    }
}
impl Drop for WorkHumanPresentation {
    fn drop(&mut self) {
        self.retire();
        super::passive_page::unregister(&self.page);
    }
}
