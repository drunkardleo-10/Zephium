use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dispatch2::{DispatchQueue, MainThreadBound};
use objc2::rc::{Retained, Weak};
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly, Message};
use objc2_app_kit::{NSColor, NSEvent, NSView};
use objc2_foundation::{ns_string, MainThreadMarker, NSNumber, NSPoint, NSRect, NSSize, NSValue};
use objc2_quartz_core::{
    kCAFillModeForwards, CABasicAnimation, CAMediaTiming, CAMediaTimingFunction, CATransaction,
};

use zephium_core::geometry::Rect;
use zephium_core::ids::ItemId;
use zephium_core::split::{self, Divider, Pane};

use crate::pane_geometry::rounded_native_size;

type RatioCallback = Rc<dyn Fn(Pane)>;
type StageFailureCallback = Rc<dyn Fn(usize)>;
const MAX_ASYNC_STAGE_RETRIES: u8 = 4;

#[derive(Clone)]
struct HostView {
    view: Retained<NSView>,
    presentation_permit: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct StageIvars {
    // Boxed: ItemId is a u128 (align 16), while objc2 0.6's `define_class!`
    // registrar supports ivar alignments only through 8 bytes. Keep the tree
    // in a correctly aligned Rust allocation behind a pointer-sized ivar.
    tree: RefCell<Option<Box<Pane>>>,
    views: RefCell<HashMap<ItemId, HostView>>,
    // A raw view is never exposed while it still contains WKWebView's
    // construction-time blank document. `visible` is the logical split
    // model; `ready` advances only after privileged chrome applies and
    // verifies the exact committed URL/revision.
    visible: RefCell<HashSet<ItemId>>,
    ready: RefCell<HashSet<ItemId>>,
    /// Leaves whose current AppKit frame occupies at least one backing pixel
    /// on each axis. A collapsed deep split is hidden, not promoted to an
    /// arbitrary surface; resize can repopulate this set reversibly.
    paintable: RefCell<HashSet<ItemId>>,
    paintable_changed: Cell<bool>,
    // Host layout calls can re-enter AppKit while applying frame/tree/view
    // mutations. This generation makes the newest nested call authoritative
    // over every older stack frame, including visibility of the stage itself.
    content_update_epoch: Cell<u64>,
    desired_container_visible: Cell<bool>,
    layout_epoch: Cell<u64>,
    // Geometry/visibility native calls can re-enter AppKit more often than
    // the bounded synchronous convergence loop permits. One retained main-
    // queue turn owns the retry; while it is pending the entire stage stays
    // hidden so stale split frames cannot paint beneath newer chrome.
    stage_retry_scheduled: Cell<bool>,
    stage_retry_attempts: Cell<u8>,
    stage_retry_terminal: Cell<bool>,
    geometry_pending: Cell<bool>,
    gap: Cell<f64>,
    drag: RefCell<Option<Divider>>,
    indicator: RefCell<Option<Retained<NSView>>>,
    on_ratio: RefCell<Option<RatioCallback>>,
    on_stage_failure: RefCell<Option<StageFailureCallback>>,
    /// A slide that keeps the wider, older frame until it arrives, and the
    /// frame it then takes. The generation retires a completion whose slide
    /// a newer layout has already settled.
    held_frame: Cell<Option<NSRect>>,
    motion_generation: Cell<u64>,
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
            self.bump_layout_epoch();
            if self.position_panes() && self.ivars().paintable_changed.replace(false) {
                let _ = self.sync_visibility();
            }
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if self.ivars().stage_retry_terminal.get() {
                return;
            }
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
            if self.ivars().stage_retry_terminal.get() {
                return;
            }
            let ivars = self.ivars();
            let Some(grabbed) = ivars.drag.try_borrow().ok().and_then(|drag| drag.clone()) else {
                return;
            };
            let (px, py) = self.local_point(event);
            let region = self.region();
            let gap = ivars.gap.get();
            let changed = if let Ok(mut tree) = ivars.tree.try_borrow_mut() {
                let Some(tree) = tree.as_mut() else {
                    return;
                };
                let Some(current) =
                    split::divider_at_path(tree, region, gap, &grabbed.path)
                else {
                    return;
                };
                let ratio = split::ratio_for(current.axis, current.rect, gap, px, py);
                tree.set_ratio(&current.path, ratio);
                true
            } else {
                false
            };
            if !changed {
                return;
            }
            self.bump_layout_epoch();
            if self.position_panes() && self.ivars().paintable_changed.replace(false) {
                let _ = self.sync_visibility();
            }
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            if self.ivars().stage_retry_terminal.get() {
                return;
            }
            let ivars = self.ivars();
            let Some(drag) = ivars
                .drag
                .try_borrow_mut()
                .ok()
                .and_then(|mut drag| drag.take())
            else {
                return;
            };
            // AppKit may deliver the final pointer coordinate in mouseUp
            // without a preceding mouseDragged at that exact position. Land
            // it before publishing the authoritative tree to the shell.
            let (px, py) = self.local_point(event);
            let region = self.region();
            let gap = ivars.gap.get();
            let changed = if let Ok(mut tree) = ivars.tree.try_borrow_mut() {
                tree.as_mut().is_some_and(|tree| {
                    let Some(current) = split::divider_at_path(tree, region, gap, &drag.path) else {
                        return false;
                    };
                    let ratio = split::ratio_for(current.axis, current.rect, gap, px, py);
                    tree.set_ratio(&current.path, ratio);
                    true
                })
            } else {
                false
            };
            if changed {
                self.bump_layout_epoch();
                if self.position_panes() && self.ivars().paintable_changed.replace(false) {
                    let _ = self.sync_visibility();
                }
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
    pub fn new(
        mtm: MainThreadMarker,
        gap: f64,
        on_stage_failure: Box<dyn Fn(usize)>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(StageIvars {
            gap: Cell::new(gap),
            on_stage_failure: RefCell::new(Some(Rc::from(on_stage_failure))),
            ..Default::default()
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setWantsLayer(true);
        // `addSubview:` is a native painting boundary. A newly-created stage
        // remains fail-closed until one exact layout update finishes.
        this.setHidden(true);
        this
    }

    /// Whether a slide to `frame` can be carried out on the layer: the stage
    /// is on screen and only its horizontal extent changes.
    pub fn can_slide_to(&self, frame: NSRect) -> bool {
        let current = self.motion_target();
        !self.isHidden()
            && current.size.width > 0.0
            && current.origin.y == frame.origin.y
            && current.size.height == frame.size.height
            && current.origin.x != frame.origin.x
    }

    /// The frame the stage is at, or is travelling to.
    pub fn motion_target(&self) -> NSRect {
        self.ivars()
            .held_frame
            .get()
            .unwrap_or_else(|| self.frame())
    }

    /// Moves the stage to `frame` as one journey. The stage keeps whichever
    /// of the two frames is wider for the whole of it — the new one at once
    /// when the content grows, the old one until arrival when it shrinks —
    /// so its pages are laid out at most once and no edge ever opens a gap.
    /// Only the layer's translation animates, which the compositor carries
    /// without asking any page to draw again.
    pub fn slide_to(&self, frame: NSRect) {
        self.settle_motion();
        let current = self.frame();
        let Some(layer) = self.layer() else {
            self.setFrame(frame);
            return;
        };
        let generation = self.ivars().motion_generation.get().wrapping_add(1);
        self.ivars().motion_generation.set(generation);
        let (from, to) = if frame.size.width >= current.size.width {
            self.setFrame(frame);
            (current.origin.x - frame.origin.x, 0.0)
        } else {
            self.ivars().held_frame.set(Some(frame));
            (0.0, frame.origin.x - current.origin.x)
        };
        let animation = translation(from, to, SLIDE_SECONDS);
        let stage = Weak::from_retained(&self.retain());
        let arrived = block2::RcBlock::new(move || {
            if let Some(stage) = stage.load() {
                if stage.ivars().motion_generation.get() == generation {
                    stage.settle_motion();
                }
            }
        });
        CATransaction::begin();
        // SAFETY: the block holds the stage weakly and runs on the main thread.
        unsafe { CATransaction::setCompletionBlock(Some(&arrived)) };
        layer.addAnimation_forKey(&animation, Some(ns_string!("zephium.slide")));
        CATransaction::commit();
    }

    /// Brings the stage back into view after a browser page covered it: it
    /// settles in from a breath smaller and fully transparent, the way a
    /// browser page arrives in chrome.
    pub fn arrive(&self) {
        let Some(layer) = self.layer() else {
            return;
        };
        let bounds = self.bounds();
        let centre = (bounds.size.width / 2.0, bounds.size.height / 2.0);
        let fade = CABasicAnimation::animationWithKeyPath(Some(ns_string!("opacity")));
        let scale = CABasicAnimation::animationWithKeyPath(Some(ns_string!("transform.scale")));
        // Scaling about the layer's origin corner, shifted by exactly the
        // amount that makes it a scale about the centre.
        let shift =
            CABasicAnimation::animationWithKeyPath(Some(ns_string!("transform.translation")));
        // SAFETY: NSNumber and NSValue are the value types these key paths take.
        unsafe {
            fade.setFromValue(Some(&NSNumber::new_f64(0.0)));
            fade.setToValue(Some(&NSNumber::new_f64(1.0)));
            scale.setFromValue(Some(&NSNumber::new_f64(ARRIVE_SCALE)));
            scale.setToValue(Some(&NSNumber::new_f64(1.0)));
            shift.setFromValue(Some(&NSValue::valueWithSize(NSSize::new(
                centre.0 * (1.0 - ARRIVE_SCALE),
                centre.1 * (1.0 - ARRIVE_SCALE),
            ))));
            shift.setToValue(Some(&NSValue::valueWithSize(NSSize::new(0.0, 0.0))));
        }
        fade.setDuration(ARRIVE_SECONDS * 0.7);
        fade.setTimingFunction(Some(&ease_out()));
        for animation in [&scale, &shift] {
            animation.setDuration(ARRIVE_SECONDS);
            animation.setTimingFunction(Some(&emphasized()));
        }
        layer.addAnimation_forKey(&fade, Some(ns_string!("zephium.arrive.fade")));
        layer.addAnimation_forKey(&scale, Some(ns_string!("zephium.arrive.scale")));
        layer.addAnimation_forKey(&shift, Some(ns_string!("zephium.arrive.shift")));
    }

    /// Ends any journey at once: the held frame is taken and the layer's
    /// motion removed in the same transaction, so nothing is seen to jump.
    pub fn settle_motion(&self) {
        self.ivars()
            .motion_generation
            .set(self.ivars().motion_generation.get().wrapping_add(1));
        let held = self.ivars().held_frame.take();
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        if let Some(frame) = held {
            self.setFrame(frame);
        }
        if let Some(layer) = self.layer() {
            layer.removeAnimationForKey(ns_string!("zephium.slide"));
        }
        CATransaction::commit();
    }

    /// Reserve authority for one host layout before performing any AppKit
    /// calls. A nested layout increments this epoch and permanently prevents
    /// the older stack frame from revealing the stage container afterward.
    pub fn begin_content_update(&self, visible: bool) -> Option<u64> {
        if self.ivars().stage_retry_terminal.get() {
            if !self.isHidden() {
                self.setHidden(true);
            }
            return None;
        }
        let epoch = self
            .ivars()
            .content_update_epoch
            .get()
            .wrapping_add(1)
            .max(1);
        self.ivars().content_update_epoch.set(epoch);
        self.ivars().desired_container_visible.set(visible);
        if !visible {
            // AppKit may not deliver mouseUp after the owning window is
            // hidden/minimized. Do not let that abandoned native capture keep
            // authorizing an old divider path when the stage is shown again.
            if let Ok(mut drag) = self.ivars().drag.try_borrow_mut() {
                drag.take();
            }
        }
        Some(epoch)
    }

    pub fn content_update_is_current(&self, epoch: u64) -> bool {
        !self.ivars().stage_retry_terminal.get() && self.ivars().content_update_epoch.get() == epoch
    }

    pub fn has_terminal_failure(&self) -> bool {
        self.ivars().stage_retry_terminal.get()
    }

    /// Complete the exact update reserved by `begin_content_update`. Stale
    /// outer updates are no-ops; their nested successor already owns the
    /// retained desired state and native convergence obligation.
    pub fn finish_content_update(&self, epoch: u64) -> bool {
        if self.ivars().stage_retry_terminal.get() {
            if !self.isHidden() {
                self.setHidden(true);
            }
            return false;
        }
        if !self.content_update_is_current(epoch) {
            return true;
        }
        let _ = self.sync_container_visibility();
        // A retained main-queue retry is an accepted native obligation, not a
        // terminal application failure. Only exhaustion/panic makes the host
        // seal the engine; the stage remains hidden while a retry is pending.
        !self.ivars().stage_retry_terminal.get()
    }

    /// An exact layout whose native children could not be established must
    /// never leave the prior stage visible under newer browser chrome.
    pub fn abort_content_update(&self, epoch: u64) {
        if self.content_update_is_current(epoch) {
            self.ivars().desired_container_visible.set(false);
            if let Ok(mut drag) = self.ivars().drag.try_borrow_mut() {
                drag.take();
            }
            let superseding = self
                .ivars()
                .content_update_epoch
                .get()
                .wrapping_add(1)
                .max(1);
            self.ivars().content_update_epoch.set(superseding);
        }
        let _ = self.sync_container_visibility();
    }

    pub fn set_tree(&self, tree: Option<Pane>) -> bool {
        if self.ivars().stage_retry_terminal.get() {
            return false;
        }
        let Ok(mut current) = self.ivars().tree.try_borrow_mut() else {
            return false;
        };
        let next = tree.map(Box::new);
        if current.as_deref() == next.as_deref() {
            drop(current);
            let _ = self.position_panes();
            return !self.ivars().stage_retry_terminal.get();
        }
        let topology_changed = match (current.as_deref(), next.as_deref()) {
            (Some(current), Some(next)) => !current.same_topology(next),
            (None, None) => false,
            (Some(_), None) | (None, Some(_)) => true,
        };
        let drag_active = self
            .ivars()
            .drag
            .try_borrow()
            .is_ok_and(|drag| drag.is_some());
        if drag_active && !topology_changed {
            // During a native drag the stage owns the newest ratio while the
            // shell intentionally waits for mouseUp. A resize can relayout
            // with the shell's older ratio; retain the local tree and let
            // resizeSubviews recompute geometry instead of snapping back.
            return true;
        }
        if topology_changed {
            let Ok(mut drag) = self.ivars().drag.try_borrow_mut() else {
                // Never install a tree whose same binary path could still be
                // authorized by an uncleared gesture.
                return false;
            };
            drag.take();
        }
        *current = next;
        drop(current);
        self.bump_layout_epoch();
        let _ = self.position_panes();
        !self.ivars().stage_retry_terminal.get()
    }

    pub fn set_on_ratio(&self, f: Box<dyn Fn(Pane)>) {
        if self.ivars().stage_retry_terminal.get() {
            return;
        }
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

    /// Returns whether the latest authoritative layout expects this item.
    /// A view can be constructed after that layout task ran, so creation uses
    /// this retained model to attach the late native child without waiting for
    /// another resize or user interaction.
    pub fn allows_download_decision(&self, id: ItemId) -> bool {
        !self.ivars().stage_retry_terminal.get()
            && self.ivars().desired_container_visible.get()
            && self.contains_item(id)
    }

    pub fn contains_item(&self, id: ItemId) -> bool {
        self.ivars()
            .tree
            .try_borrow()
            .is_ok_and(|tree| tree.as_deref().is_some_and(|tree| tree.contains(id)))
    }

    pub fn attached_items(&self) -> Option<Vec<ItemId>> {
        self.ivars()
            .views
            .try_borrow()
            .ok()
            .map(|views| views.keys().copied().collect())
    }

    pub fn retire(&self) {
        self.ivars().stage_retry_terminal.set(true);
        self.ivars().stage_retry_scheduled.set(false);
        self.setHidden(true);
        self.removeFromSuperview();
    }

    pub fn insert_view(
        &self,
        id: ItemId,
        view: Retained<NSView>,
        presentation_permit: Arc<AtomicBool>,
    ) -> bool {
        if self.ivars().stage_retry_terminal.get() {
            return false;
        }
        // `addSubview:` may paint synchronously. Hide before attaching so a
        // new WKWebView cannot expose its default white backing store between
        // construction and the first attributed, chrome-verified document.
        view.setHidden(true);
        let Ok(mut ready) = self.ivars().ready.try_borrow_mut() else {
            return false;
        };
        ready.remove(&id);
        drop(ready);
        let Ok(mut views) = self.ivars().views.try_borrow_mut() else {
            return false;
        };
        views.insert(
            id,
            HostView {
                view: view.clone(),
                presentation_permit,
            },
        );
        drop(views);
        self.bump_layout_epoch();
        // `addSubview:` can synchronously enter AppKit callbacks. Native work
        // happens only after the view registry borrow has been released.
        self.addSubview(&view);
        if self.ivars().stage_retry_terminal.get() {
            return false;
        }
        let _ = self.position_panes();
        let _ = self.sync_visibility();
        !self.ivars().stage_retry_terminal.get()
    }

    pub fn remove_view(&self, id: ItemId) {
        if self.ivars().stage_retry_terminal.get() && !self.isHidden() {
            self.setHidden(true);
        }
        if let Ok(mut visible) = self.ivars().visible.try_borrow_mut() {
            visible.remove(&id);
        }
        if let Ok(mut ready) = self.ivars().ready.try_borrow_mut() {
            ready.remove(&id);
        }
        if let Ok(mut paintable) = self.ivars().paintable.try_borrow_mut() {
            paintable.remove(&id);
        }
        let view = self
            .ivars()
            .views
            .try_borrow_mut()
            .ok()
            .and_then(|mut views| views.remove(&id));
        if let Some(view) = view {
            self.bump_layout_epoch();
            view.view.removeFromSuperview();
        }
    }

    pub fn set_visible(&self, visible: &[ItemId]) -> bool {
        if self.ivars().stage_retry_terminal.get() {
            return false;
        }
        let next = visible.iter().copied().collect::<HashSet<_>>();
        let Some(changed) = self
            .ivars()
            .visible
            .try_borrow_mut()
            .ok()
            .map(|mut current| {
                let changed = *current != next;
                *current = next.clone();
                changed
            })
        else {
            return false;
        };
        if changed {
            self.bump_layout_epoch();
        }
        // Run even for an identical logical value: a prior AppKit re-entry
        // may have forced a fail-closed hide before the newest state settled.
        let _ = self.sync_visibility();
        !self.ivars().stage_retry_terminal.get()
    }

    /// Reveal one exact raw-view generation after privileged chrome verified
    /// its attributed URL/revision and the shell returned the same identity.
    pub fn set_ready(&self, id: ItemId) -> bool {
        if self.ivars().stage_retry_terminal.get() {
            return false;
        }
        let newly_ready = match self.ivars().ready.try_borrow_mut() {
            Ok(mut ready) => ready.insert(id),
            Err(_) => return false,
        };
        if !newly_ready {
            let _ = self.sync_visibility();
            return !self.ivars().stage_retry_terminal.get();
        }
        self.bump_layout_epoch();
        let _ = self.sync_visibility();
        !self.ivars().stage_retry_terminal.get()
    }

    /// Re-arm the presentation barrier for a newly committed main-frame
    /// document. The stage retains this logical state before native sync, so
    /// a later resize/layout pass cannot reveal the new pixels using the
    /// previous document's readiness acknowledgement.
    pub fn set_pending(&self, id: ItemId) -> bool {
        if self.ivars().stage_retry_terminal.get() {
            if !self.isHidden() {
                self.setHidden(true);
            }
            return false;
        }
        let attached = match self.ivars().views.try_borrow() {
            Ok(views) => views.contains_key(&id),
            Err(_) => return false,
        };
        if !attached {
            return true;
        }
        if let Ok(views) = self.ivars().views.try_borrow() {
            if let Some(view) = views.get(&id) {
                view.presentation_permit.store(false, Ordering::Release);
            }
        }
        let removed = match self.ivars().ready.try_borrow_mut() {
            Ok(mut ready) => ready.remove(&id),
            Err(_) => return false,
        };
        if removed {
            self.bump_layout_epoch();
        }
        // Run even when an identical pending value was already retained: a
        // prior AppKit re-entry may have interrupted the fail-closed hide.
        let _ = self.sync_visibility();
        !self.ivars().stage_retry_terminal.get()
    }

    pub fn set_drop_indicator(&self, zone: Option<Rect>) {
        if self.ivars().stage_retry_terminal.get() {
            return;
        }
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

    fn position_panes(&self) -> bool {
        if self.ivars().stage_retry_terminal.get() {
            if !self.isHidden() {
                self.setHidden(true);
            }
            return false;
        }
        // A frame mutation can synchronously re-enter AppKit. Snapshot all
        // Rust ownership first, release RefCell guards, and retry from the
        // latest model if re-entry changed the tree/view generation. Never
        // touch another child after one native call supersedes this snapshot.
        'attempt: for _ in 0..4 {
            let ivars = self.ivars();
            let epoch = ivars.layout_epoch.get();
            let Some(tree) = ivars
                .tree
                .try_borrow()
                .ok()
                .and_then(|tree| tree.as_deref().cloned())
            else {
                let Ok(mut paintable) = ivars.paintable.try_borrow_mut() else {
                    self.defer_stage_retry(true);
                    return false;
                };
                let changed = !paintable.is_empty();
                paintable.clear();
                ivars.paintable_changed.set(changed);
                ivars.geometry_pending.set(false);
                return true;
            };
            let Some(views) = ivars.views.try_borrow().ok().map(|views| views.clone()) else {
                self.defer_stage_retry(true);
                return false;
            };
            let gap = ivars.gap.get();
            let bounds = self.bounds();
            if !self.layout_epoch_is_current(epoch) {
                continue 'attempt;
            }
            let region = Rect::new(0.0, 0.0, bounds.size.width, bounds.size.height);
            let mut paintable = HashSet::new();
            for (id, r) in split::layout(&tree, region, gap) {
                if let Some(view) = views.get(&id) {
                    if !self.layout_epoch_is_current(epoch) {
                        continue 'attempt;
                    }
                    let frame = NSRect::new(
                        NSPoint::new(r.x, bounds.size.height - r.y - r.height),
                        NSSize::new(r.width, r.height),
                    );
                    let backing = self.convertSizeToBacking(NSSize::new(r.width, r.height));
                    if rounded_native_size(backing.width, backing.height, 1.0).is_some() {
                        paintable.insert(id);
                    }
                    let current = view.view.frame();
                    if !self.layout_epoch_is_current(epoch) {
                        continue 'attempt;
                    }
                    if !same_rect(current, frame) {
                        view.view.setFrame(frame);
                        if !self.layout_epoch_is_current(epoch) {
                            continue 'attempt;
                        }
                    }
                }
            }
            if self.layout_epoch_is_current(epoch) {
                let Ok(mut current) = self.ivars().paintable.try_borrow_mut() else {
                    self.defer_stage_retry(true);
                    return false;
                };
                let changed = *current != paintable;
                *current = paintable;
                drop(current);
                self.ivars().paintable_changed.set(changed);
                self.ivars().geometry_pending.set(false);
                return true;
            }
        }
        self.defer_stage_retry(true);
        false
    }

    fn sync_visibility(&self) -> bool {
        if self.ivars().stage_retry_terminal.get() {
            if !self.isHidden() {
                self.setHidden(true);
            }
            return false;
        }
        // `setHidden:` may synchronously enter AppKit. Apply all removals
        // before additions, and never show from a snapshot whose epoch was
        // superseded during native work. A retry observes the newest model;
        // exhaustion leaves uncertain views hidden until the next sync.
        for _ in 0..4 {
            let ivars = self.ivars();
            let epoch = ivars.layout_epoch.get();
            let Some(visible) = ivars.visible.try_borrow().ok().map(|set| set.clone()) else {
                self.defer_stage_retry(false);
                return false;
            };
            let Some(ready) = ivars.ready.try_borrow().ok().map(|set| set.clone()) else {
                self.defer_stage_retry(false);
                return false;
            };
            let Some(paintable) = ivars.paintable.try_borrow().ok().map(|set| set.clone()) else {
                self.defer_stage_retry(false);
                return false;
            };
            let Some(mut views) = ivars.views.try_borrow().ok().map(|views| {
                views
                    .iter()
                    .map(|(id, view)| (*id, view.clone()))
                    .collect::<Vec<_>>()
            }) else {
                self.defer_stage_retry(false);
                return false;
            };
            views.sort_by_key(|(id, _)| *id);

            let mut superseded = false;
            for (_, view) in views.iter().filter(|(id, view)| {
                !visible.contains(id)
                    || !ready.contains(id)
                    || !paintable.contains(id)
                    || !view.presentation_permit.load(Ordering::Acquire)
            }) {
                if !view.view.isHidden() {
                    view.view.setHidden(true);
                }
                if !self.layout_epoch_is_current(epoch) {
                    superseded = true;
                    break;
                }
            }
            if superseded {
                continue;
            }

            for (id, view) in views.iter().filter(|(id, _)| {
                visible.contains(id) && ready.contains(id) && paintable.contains(id)
            }) {
                // Revalidate immediately before every reveal. This prevents
                // an outer stale pass from undoing a nested newer hide.
                let still_current = self.layout_epoch_is_current(epoch)
                    && view.presentation_permit.load(Ordering::Acquire)
                    && self
                        .ivars()
                        .visible
                        .try_borrow()
                        .is_ok_and(|current| current.contains(id))
                    && self
                        .ivars()
                        .ready
                        .try_borrow()
                        .is_ok_and(|current| current.contains(id))
                    && self
                        .ivars()
                        .paintable
                        .try_borrow()
                        .is_ok_and(|current| current.contains(id));
                if !still_current {
                    superseded = true;
                    break;
                }
                let hidden = view.view.isHidden();
                if !self.layout_epoch_is_current(epoch)
                    || !view.presentation_permit.load(Ordering::Acquire)
                {
                    superseded = true;
                    break;
                }
                if hidden {
                    view.view.setHidden(false);
                }
                if !self.layout_epoch_is_current(epoch)
                    || !view.presentation_permit.load(Ordering::Acquire)
                {
                    // `setHidden(false)` may pump a native commit callback.
                    // A permit revoked during that re-entry wins before this
                    // outer pass returns to AppKit for painting.
                    if !view.view.isHidden() {
                        view.view.setHidden(true);
                    }
                    superseded = true;
                    break;
                }
            }
            if !superseded && self.layout_epoch_is_current(epoch) {
                return true;
            }
        }

        // Native re-entry kept changing ownership. Enforce the part that is
        // always safe from the latest model; a later identical sync may show
        // the now-settled desired leaves.
        let visible = self
            .ivars()
            .visible
            .try_borrow()
            .ok()
            .map(|set| set.clone())
            .unwrap_or_default();
        let ready = self
            .ivars()
            .ready
            .try_borrow()
            .ok()
            .map(|set| set.clone())
            .unwrap_or_default();
        let paintable = self
            .ivars()
            .paintable
            .try_borrow()
            .ok()
            .map(|set| set.clone())
            .unwrap_or_default();
        if let Ok(views) = self.ivars().views.try_borrow() {
            for (id, view) in views.iter() {
                if !view.view.isHidden()
                    && (!visible.contains(id)
                        || !ready.contains(id)
                        || !paintable.contains(id)
                        || !view.presentation_permit.load(Ordering::Acquire))
                {
                    view.view.setHidden(true);
                }
            }
        }
        self.defer_stage_retry(false);
        false
    }

    fn defer_stage_retry(&self, geometry: bool) {
        if geometry {
            self.ivars().geometry_pending.set(true);
        }
        if self.ivars().stage_retry_terminal.get() {
            if !self.isHidden() {
                self.setHidden(true);
            }
            return;
        }
        let already_scheduled = self.ivars().stage_retry_scheduled.replace(true);
        // Conceal the container before queueing. A stale geometry snapshot is
        // unsafe even when every child still has a valid navigation permit.
        if !self.isHidden() {
            self.setHidden(true);
        }
        if self.ivars().stage_retry_terminal.get() {
            self.ivars().stage_retry_scheduled.set(false);
            return;
        }
        if already_scheduled {
            return;
        }
        let attempts = self.ivars().stage_retry_attempts.get();
        if attempts >= MAX_ASYNC_STAGE_RETRIES {
            self.fail_stage_retry_terminal();
            return;
        }
        self.ivars()
            .stage_retry_attempts
            .set(attempts.saturating_add(1));
        let Some(mtm) = MainThreadMarker::new() else {
            // ContentStage is MainThreadOnly, so this is an invariant guard.
            // Retain the pending bit and hidden container if it is violated.
            return;
        };
        let stage = MainThreadBound::new(self.retain(), mtm);
        DispatchQueue::main().exec_async(move || {
            // libdispatch callbacks have a C ABI; native re-entry must never
            // let a Rust unwind cross that boundary.
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let Some(mtm) = MainThreadMarker::new() else {
                    return false;
                };
                let stage = stage.get(mtm);
                stage.ivars().stage_retry_scheduled.set(false);
                if stage.position_panes() && stage.sync_visibility() {
                    return stage.sync_container_visibility();
                }
                false
            }));
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            let stage = stage.get(mtm);
            match outcome {
                Ok(true) => stage.ivars().stage_retry_attempts.set(0),
                Ok(false) => {}
                Err(_) => stage.fail_stage_retry_terminal(),
            }
        });
    }

    fn fail_stage_retry_terminal(&self) {
        if self.ivars().stage_retry_terminal.replace(true) {
            return;
        }
        self.ivars().stage_retry_scheduled.set(false);
        if !self.isHidden() {
            self.setHidden(true);
        }
        let callback = self
            .ivars()
            .on_stage_failure
            .try_borrow()
            .ok()
            .and_then(|callback| callback.clone());
        if let Some(callback) = callback {
            callback(self as *const Self as usize);
        }
    }

    fn sync_container_visibility(&self) -> bool {
        if self.ivars().stage_retry_terminal.get() {
            if !self.isHidden() {
                self.setHidden(true);
            }
            return false;
        }
        // `setHidden:` can pump AppKit. Revalidate the exact desired-layout
        // generation before and after a reveal; on any mismatch conceal first
        // and retry from the newest retained fact.
        for _ in 0..4 {
            let epoch = self.ivars().content_update_epoch.get();
            let desired = self.ivars().desired_container_visible.get();
            let presentation_safe = !self.ivars().stage_retry_terminal.get()
                && !self.ivars().geometry_pending.get()
                && !self.ivars().stage_retry_scheduled.get();
            if !desired || !presentation_safe {
                if !self.isHidden() {
                    self.setHidden(true);
                }
                if self.ivars().content_update_epoch.get() == epoch
                    && (!self.ivars().desired_container_visible.get()
                        || self.ivars().geometry_pending.get()
                        || self.ivars().stage_retry_scheduled.get()
                        || self.ivars().stage_retry_terminal.get())
                {
                    return presentation_safe;
                }
                continue;
            }

            let may_reveal = self.ivars().content_update_epoch.get() == epoch
                && self.ivars().desired_container_visible.get()
                && !self.ivars().stage_retry_terminal.get()
                && !self.ivars().geometry_pending.get()
                && !self.ivars().stage_retry_scheduled.get();
            if !may_reveal {
                continue;
            }
            let hidden = self.isHidden();
            if self.ivars().stage_retry_terminal.get()
                || self.ivars().content_update_epoch.get() != epoch
                || !self.ivars().desired_container_visible.get()
                || self.ivars().geometry_pending.get()
                || self.ivars().stage_retry_scheduled.get()
            {
                if !hidden {
                    self.setHidden(true);
                }
                continue;
            }
            if hidden {
                self.setHidden(false);
            }
            if self.ivars().content_update_epoch.get() != epoch
                || !self.ivars().desired_container_visible.get()
                || self.ivars().stage_retry_terminal.get()
                || self.ivars().geometry_pending.get()
                || self.ivars().stage_retry_scheduled.get()
            {
                // A nested hide or newer visible layout owns the container.
                // Conceal before retrying so this stale outer setter cannot
                // expose old leaves even for one compositor turn.
                if !self.isHidden() {
                    self.setHidden(true);
                }
                continue;
            }
            return true;
        }

        // Re-entry did not converge within the bounded synchronous budget.
        // The privileged chrome surface is preferable to stale page pixels;
        // a subsequent identical layout/ready update retries presentation.
        if !self.isHidden() {
            self.setHidden(true);
        }
        self.defer_stage_retry(false);
        false
    }

    fn bump_layout_epoch(&self) {
        let epoch = self.ivars().layout_epoch.get().wrapping_add(1);
        self.ivars().layout_epoch.set(epoch.max(1));
    }

    fn layout_epoch_is_current(&self, epoch: u64) -> bool {
        !self.ivars().stage_retry_terminal.get() && self.ivars().layout_epoch.get() == epoch
    }
}

fn same_rect(left: NSRect, right: NSRect) -> bool {
    left.origin.x == right.origin.x
        && left.origin.y == right.origin.y
        && left.size.width == right.size.width
        && left.size.height == right.size.height
}

/// The chrome's --motion-page and --ease-emphasized, so the page and the
/// sidebar beside it travel as one surface.
const SLIDE_SECONDS: f64 = 0.4;
const ARRIVE_SECONDS: f64 = 0.4;
const ARRIVE_SCALE: f64 = 0.985;

fn emphasized() -> Retained<CAMediaTimingFunction> {
    CAMediaTimingFunction::functionWithControlPoints(0.16, 1.0, 0.3, 1.0)
}

fn ease_out() -> Retained<CAMediaTimingFunction> {
    CAMediaTimingFunction::functionWithControlPoints(0.22, 1.0, 0.36, 1.0)
}

/// A horizontal move added to the layer's resting position, held at its end
/// until the stage settles.
fn translation(from: f64, to: f64, seconds: f64) -> Retained<CABasicAnimation> {
    let animation =
        CABasicAnimation::animationWithKeyPath(Some(ns_string!("transform.translation.x")));
    // SAFETY: NSNumber is the value type a scalar key path takes.
    unsafe {
        animation.setFromValue(Some(&NSNumber::new_f64(from)));
        animation.setToValue(Some(&NSNumber::new_f64(to)));
        animation.setFillMode(kCAFillModeForwards);
    }
    animation.setAdditive(true);
    animation.setRemovedOnCompletion(false);
    animation.setDuration(seconds);
    animation.setTimingFunction(Some(&emphasized()));
    animation
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
