use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSColor, NSEvent, NSView};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};

use zephium_core::geometry::Rect;
use zephium_core::split::{self, Divider, Pane};
use zephium_core::tab::TabId;

#[derive(Default)]
pub struct StageIvars {
    tree: RefCell<Option<Pane>>,
    views: RefCell<HashMap<TabId, Retained<NSView>>>,
    gap: Cell<f64>,
    drag: RefCell<Option<Divider>>,
    indicator: RefCell<Option<Retained<NSView>>>,
    on_ratio: RefCell<Option<Box<dyn Fn(Pane)>>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumContentStage"]
    #[ivars = StageIvars]
    pub struct ContentStage;

    impl ContentStage {
        #[unsafe(method(resizeSubviewsWithOldSize:))]
        fn resize_subviews(&self, _old: NSSize) {
            self.position_panes();
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let (px, py) = self.local_point(event);
            let ivars = self.ivars();
            let hit = ivars
                .tree
                .borrow()
                .as_ref()
                .and_then(|t| split::divider_at(t, self.region(), ivars.gap.get(), px, py));
            *ivars.drag.borrow_mut() = hit;
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let ivars = self.ivars();
            let Some(drag) = ivars.drag.borrow().clone() else {
                return;
            };
            let (px, py) = self.local_point(event);
            let ratio = split::ratio_for(drag.axis, drag.rect, ivars.gap.get(), px, py);
            if let Some(tree) = ivars.tree.borrow_mut().as_mut() {
                tree.set_ratio(&drag.path, ratio);
            }
            self.position_panes();
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            if self.ivars().drag.borrow_mut().take().is_none() {
                return;
            }
            let tree = self.ivars().tree.borrow().clone();
            if let (Some(cb), Some(tree)) = (self.ivars().on_ratio.borrow().as_ref(), tree) {
                cb(tree);
            }
        }
    }
);

impl ContentStage {
    pub fn new(mtm: MainThreadMarker, gap: f64) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(StageIvars {
            gap: Cell::new(gap),
            ..Default::default()
        });
        unsafe { msg_send![super(this), init] }
    }

    pub fn set_tree(&self, tree: Option<Pane>) {
        *self.ivars().tree.borrow_mut() = tree;
        self.position_panes();
    }

    pub fn set_on_ratio(&self, f: Box<dyn Fn(Pane)>) {
        *self.ivars().on_ratio.borrow_mut() = Some(f);
    }

    pub fn insert_view(&self, id: TabId, view: Retained<NSView>) {
        self.addSubview(&view);
        self.ivars().views.borrow_mut().insert(id, view);
    }

    pub fn remove_view(&self, id: TabId) {
        if let Some(view) = self.ivars().views.borrow_mut().remove(&id) {
            view.removeFromSuperview();
        }
    }

    pub fn set_visible(&self, visible: &[TabId]) {
        for (id, view) in self.ivars().views.borrow().iter() {
            view.setHidden(!visible.contains(id));
        }
    }

    pub fn set_drop_indicator(&self, zone: Option<Rect>) {
        let mut indicator = self.ivars().indicator.borrow_mut();
        match zone {
            None => {
                if let Some(view) = indicator.take() {
                    view.removeFromSuperview();
                }
            }
            Some(z) => {
                let view = indicator.get_or_insert_with(|| {
                    let v = self.make_indicator();
                    self.addSubview(&v);
                    v
                });
                let h = self.bounds().size.height;
                view.setFrame(NSRect::new(
                    NSPoint::new(z.x, h - z.y - z.height),
                    NSSize::new(z.width, z.height),
                ));
            }
        }
    }

    fn make_indicator(&self) -> Retained<NSView> {
        let v = NSView::new(self.mtm());
        v.setWantsLayer(true);
        if let Some(layer) = v.layer() {
            let fill = NSColor::colorWithWhite_alpha(1.0, 0.12);
            let border = NSColor::colorWithWhite_alpha(1.0, 0.42);
            layer.setBackgroundColor(Some(&fill.CGColor()));
            layer.setBorderColor(Some(&border.CGColor()));
            layer.setBorderWidth(1.5);
            layer.setCornerRadius(10.0);
        }
        v
    }

    fn region(&self) -> Rect {
        let b = self.bounds();
        Rect::new(0.0, 0.0, b.size.width, b.size.height)
    }

    fn local_point(&self, event: &NSEvent) -> (f64, f64) {
        let win = event.locationInWindow();
        let local = self.convertPoint_fromView(win, None);
        (local.x, self.bounds().size.height - local.y)
    }

    fn position_panes(&self) {
        let ivars = self.ivars();
        let bounds = self.bounds();
        let region = Rect::new(0.0, 0.0, bounds.size.width, bounds.size.height);
        let tree = ivars.tree.borrow();
        let Some(tree) = tree.as_ref() else {
            return;
        };
        let views = ivars.views.borrow();
        for (id, r) in split::layout(tree, region, ivars.gap.get()) {
            if let Some(view) = views.get(&id) {
                view.setFrame(NSRect::new(
                    NSPoint::new(r.x, bounds.size.height - r.y - r.height),
                    NSSize::new(r.width, r.height),
                ));
            }
        }
    }
}
