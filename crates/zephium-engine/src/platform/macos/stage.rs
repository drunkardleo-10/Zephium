use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSColor, NSEvent, NSView};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};

use zephium_core::geometry::Rect;
use zephium_core::ids::ItemId;
use zephium_core::split::{self, Divider, Pane};

type RatioCallback = Rc<dyn Fn(Pane)>;

#[derive(Default)]
pub struct StageIvars {
    // Boxed: ItemId is a u128 (align 16) and the ObjC runtime caps ivar
    // alignment at 8, so the tree must live behind a pointer.
    tree: RefCell<Option<Box<Pane>>>,
    views: RefCell<HashMap<ItemId, Retained<NSView>>>,
    layout_epoch: Cell<u64>,
    gap: Cell<f64>,
    drag: RefCell<Option<Divider>>,
    indicator: RefCell<Option<Retained<NSView>>>,
    on_ratio: RefCell<Option<RatioCallback>>,
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
            let tree = ivars
                .tree
                .try_borrow()
                .ok()
                .and_then(|tree| tree.as_deref().cloned());
            let region = self.region();
            let hit = tree
                .as_ref()
                .and_then(|tree| split::divider_at(tree, region, ivars.gap.get(), px, py));
            if let Ok(mut drag) = ivars.drag.try_borrow_mut() {
                *drag = hit;
            }
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let ivars = self.ivars();
            let Some(drag) = ivars.drag.try_borrow().ok().and_then(|drag| drag.clone()) else {
                return;
            };
            let (px, py) = self.local_point(event);
            let ratio = split::ratio_for(drag.axis, drag.rect, ivars.gap.get(), px, py);
            let changed = if let Ok(mut tree) = ivars.tree.try_borrow_mut() {
                let Some(tree) = tree.as_mut() else {
                    return;
                };
                tree.set_ratio(&drag.path, ratio);
                true
            } else {
                false
            };
            if !changed {
                return;
            }
            self.bump_layout_epoch();
            self.position_panes();
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            let ivars = self.ivars();
            if ivars
                .drag
                .try_borrow_mut()
                .ok()
                .and_then(|mut drag| drag.take())
                .is_none()
            {
                return;
            }
            let tree = ivars
                .tree
                .try_borrow()
                .ok()
                .and_then(|tree| tree.as_deref().cloned());
            let callback = ivars
                .on_ratio
                .try_borrow()
                .ok()
                .and_then(|callback| callback.clone());
            if let (Some(cb), Some(tree)) = (callback, tree) {
                // The callback may synchronously re-enter stage mutation, so
                // invoke it only after every RefCell guard has been dropped.
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
        let Ok(mut current) = self.ivars().tree.try_borrow_mut() else {
            return;
        };
        *current = tree.map(Box::new);
        drop(current);
        self.bump_layout_epoch();
        self.position_panes();
    }

    pub fn set_on_ratio(&self, f: Box<dyn Fn(Pane)>) {
        if let Ok(mut callback) = self.ivars().on_ratio.try_borrow_mut() {
            *callback = Some(Rc::from(f));
        }
    }

    pub fn has_view(&self, id: ItemId) -> bool {
        self.ivars()
            .views
            .try_borrow()
            .is_ok_and(|views| views.contains_key(&id))
    }

    pub fn insert_view(&self, id: ItemId, view: Retained<NSView>) {
        let Ok(mut views) = self.ivars().views.try_borrow_mut() else {
            return;
        };
        views.insert(id, view.clone());
        drop(views);
        self.bump_layout_epoch();
        // `addSubview:` can synchronously enter AppKit callbacks. Native work
        // happens only after the view registry borrow has been released.
        self.addSubview(&view);
    }

    pub fn remove_view(&self, id: ItemId) {
        let view = self
            .ivars()
            .views
            .try_borrow_mut()
            .ok()
            .and_then(|mut views| views.remove(&id));
        if let Some(view) = view {
            self.bump_layout_epoch();
            view.removeFromSuperview();
        }
    }

    pub fn set_visible(&self, visible: &[ItemId]) {
        let Some(views) = self.ivars().views.try_borrow().ok().map(|views| {
            views
                .iter()
                .map(|(id, view)| (*id, view.clone()))
                .collect::<Vec<_>>()
        }) else {
            return;
        };
        for (id, view) in views {
            view.setHidden(!visible.contains(&id));
        }
    }

    pub fn set_drop_indicator(&self, zone: Option<Rect>) {
        match zone {
            None => {
                let view = self
                    .ivars()
                    .indicator
                    .try_borrow_mut()
                    .ok()
                    .and_then(|mut indicator| indicator.take());
                if let Some(view) = view {
                    view.removeFromSuperview();
                }
            }
            Some(z) => {
                let existing = self
                    .ivars()
                    .indicator
                    .try_borrow()
                    .ok()
                    .and_then(|indicator| indicator.clone());
                let view = if let Some(view) = existing {
                    view
                } else {
                    let candidate = self.make_indicator();
                    self.addSubview(&candidate);
                    let Ok(mut indicator) = self.ivars().indicator.try_borrow_mut() else {
                        candidate.removeFromSuperview();
                        return;
                    };
                    if let Some(installed) = indicator.as_ref() {
                        let installed = installed.clone();
                        drop(indicator);
                        candidate.removeFromSuperview();
                        installed
                    } else {
                        *indicator = Some(candidate.clone());
                        candidate
                    }
                };
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
        // A frame mutation can synchronously re-enter AppKit. Snapshot all
        // Rust ownership first, release RefCell guards, and retry from the
        // latest model if re-entry changed the tree/view generation.
        for _ in 0..4 {
            let ivars = self.ivars();
            let epoch = ivars.layout_epoch.get();
            let Some(tree) = ivars
                .tree
                .try_borrow()
                .ok()
                .and_then(|tree| tree.as_deref().cloned())
            else {
                return;
            };
            let Some(views) = ivars.views.try_borrow().ok().map(|views| views.clone()) else {
                return;
            };
            let gap = ivars.gap.get();
            let bounds = self.bounds();
            let region = Rect::new(0.0, 0.0, bounds.size.width, bounds.size.height);
            for (id, r) in split::layout(&tree, region, gap) {
                if let Some(view) = views.get(&id) {
                    view.setFrame(NSRect::new(
                        NSPoint::new(r.x, bounds.size.height - r.y - r.height),
                        NSSize::new(r.width, r.height),
                    ));
                }
            }
            if self.ivars().layout_epoch.get() == epoch {
                return;
            }
        }
    }

    fn bump_layout_epoch(&self) {
        let epoch = self.ivars().layout_epoch.get().wrapping_add(1);
        self.ivars().layout_epoch.set(epoch.max(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Class registration validates ivar layout with the ObjC runtime; this
    // catches alignment regressions without launching the app.
    #[test]
    fn stage_class_registers() {
        let _ = <ContentStage as objc2::ClassType>::class();
    }
}
