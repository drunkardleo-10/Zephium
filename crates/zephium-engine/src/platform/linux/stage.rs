//! Gtk mirror of the macOS ContentStage. Content webviews live in the
//! composition root's gtk::Fixed with the chrome webview beneath them, so
//! pane gaps show the chrome background. Divider drags are DOM strips in the
//! chrome; the drop indicator is a popup toplevel painted with cairo.
//! Corner rounding is deferred to the UI phase (needs the theme color
//! channel); gaps stay square on Linux until then.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gtk::prelude::*;
use gtk::{cairo, glib::Propagation};
use wry::WebViewExtUnix;

use zephium_core::geometry::Rect;
use zephium_core::ids::ItemId;
use zephium_core::split::{self, Pane};

use crate::pane_geometry::rounded_native_size;

const INDICATOR_RADIUS: f64 = 10.0;
const INDICATOR_BORDER: f64 = 1.5;
const INDICATOR_FILL: f64 = 0.12;
const INDICATOR_STROKE: f64 = 0.42;

#[derive(Clone)]
struct HostView {
    view: webkit2gtk::WebView,
    presentation_permit: Arc<AtomicBool>,
}

struct State {
    fixed: gtk::Fixed,
    gap: f64,
    origin: (f64, f64),
    size: (f64, f64),
    hidden: bool,
    tree: Option<Pane>,
    views: HashMap<ItemId, HostView>,
    ready: HashSet<ItemId>,
    visible: Vec<ItemId>,
    indicator: Option<gtk::Window>,
    revision: u64,
}

#[derive(Clone)]
pub struct Stage {
    state: Rc<RefCell<State>>,
    sync_scheduled: Rc<Cell<bool>>,
}

impl Stage {
    pub fn new(fixed: gtk::Fixed, gap: f64) -> Self {
        Self {
            state: Rc::new(RefCell::new(State {
                fixed,
                gap,
                origin: (0.0, 0.0),
                size: (0.0, 0.0),
                hidden: true,
                tree: None,
                views: HashMap::new(),
                ready: HashSet::new(),
                visible: Vec::new(),
                indicator: None,
                revision: 0,
            })),
            sync_scheduled: Rc::new(Cell::new(false)),
        }
    }

    /// One native pass for frame, tree and visibility; `None` region hides
    /// the whole stage.
    pub fn apply(&self, region: Option<Rect>, tree: Option<Pane>, visible: &[ItemId]) -> bool {
        {
            let Ok(mut s) = self.state.try_borrow_mut() else {
                return false;
            };
            s.hidden = region.is_none();
            if let Some(r) = region {
                s.origin = (r.x, r.y);
                s.size = (r.width, r.height);
            }
            s.tree = tree;
            s.visible = visible.to_vec();
            s.revision = s.revision.wrapping_add(1).max(1);
        }
        sync(&self.state, &self.sync_scheduled);
        true
    }

    pub fn has_view(&self, id: ItemId) -> bool {
        self.state
            .try_borrow()
            .is_ok_and(|state| state.views.contains_key(&id))
    }

    /// A coalesced layout may run before a later-queued WebKit construction.
    /// Retaining the desired tree lets creation catch the native widget up to
    /// the latest geometry and visibility immediately.
    pub fn contains_item(&self, id: ItemId) -> bool {
        self.state
            .try_borrow()
            .is_ok_and(|state| state.tree.as_ref().is_some_and(|tree| tree.contains(id)))
    }

    pub fn insert_view(
        &self,
        id: ItemId,
        view: &wry::WebView,
        presentation_permit: Arc<AtomicBool>,
    ) -> bool {
        let widget = view.webview();
        widget.set_opacity(0.0);
        let Ok(mut state) = self.state.try_borrow_mut() else {
            return false;
        };
        state.ready.remove(&id);
        state.views.insert(
            id,
            HostView {
                view: widget,
                presentation_permit,
            },
        );
        state.revision = state.revision.wrapping_add(1).max(1);
        drop(state);
        // The retained layout may predate this widget. Apply it now so a
        // mapped WebKit view never remains at its construction-time bounds.
        sync(&self.state, &self.sync_scheduled);
        true
    }

    /// WebKitGTK views must start mapped to establish a compositing surface.
    /// If a newer retained layout no longer expects a just-created widget,
    /// make it non-painting and unmap it only after construction completed.
    /// `insert_view`/`sync` performs the measured remap-and-redraw path later.
    pub fn exclude_unstaged(view: &wry::WebView) {
        let widget = view.webview();
        widget.set_opacity(0.0);
        if widget.is_visible() {
            widget.hide();
        }
    }

    pub fn remove_view(&self, id: ItemId) {
        // wry owns the widget; dropping the webview removes it from the Fixed.
        if let Ok(mut state) = self.state.try_borrow_mut() {
            state.ready.remove(&id);
            if state.views.remove(&id).is_some() {
                state.revision = state.revision.wrapping_add(1).max(1);
            }
        }
    }

    /// Makes an already-mapped WebKit widget opaque only after privileged
    /// chrome verified its attributed URL/revision. Opacity preserves the
    /// compositing-surface requirement that an
    /// initially unmapped WebKitGTK view does not satisfy.
    pub fn set_ready(&self, id: ItemId) -> bool {
        let attached = match self.state.try_borrow_mut() {
            Ok(mut state) => {
                if !state.views.contains_key(&id) {
                    return false;
                }
                if state.ready.insert(id) {
                    state.revision = state.revision.wrapping_add(1).max(1);
                }
                true
            }
            Err(_) => false,
        };
        if !attached {
            return false;
        }
        // Re-drive an identical value too: an earlier GTK re-entry may have
        // interrupted the native opacity/visibility pass after `ready` was
        // retained. The revision need not change for this retry.
        sync(&self.state, &self.sync_scheduled);
        true
    }

    /// Re-arm the presentation barrier for a newly committed main-frame
    /// document. Keeping the mapped widget transparent preserves WebKitGTK's
    /// compositing surface while preventing a newer document from borrowing
    /// the prior document's readiness acknowledgement.
    pub fn set_pending(&self, id: ItemId) -> bool {
        let attached = match self.state.try_borrow_mut() {
            Ok(mut state) => {
                if !state.views.contains_key(&id) {
                    return true;
                }
                if let Some(view) = state.views.get(&id) {
                    view.presentation_permit.store(false, Ordering::Release);
                }
                if state.ready.remove(&id) {
                    state.revision = state.revision.wrapping_add(1).max(1);
                }
                true
            }
            Err(_) => false,
        };
        if attached {
            sync(&self.state, &self.sync_scheduled);
        }
        attached
    }

    pub fn set_drop_indicator(&self, zone: Option<Rect>) {
        match zone {
            None => {
                let popup = self
                    .state
                    .try_borrow_mut()
                    .ok()
                    .and_then(|mut state| state.indicator.take());
                if let Some(popup) = popup {
                    popup.close();
                }
            }
            Some(zone) => {
                let Some((fixed, origin, existing)) = self
                    .state
                    .try_borrow()
                    .ok()
                    .map(|state| (state.fixed.clone(), state.origin, state.indicator.clone()))
                else {
                    return;
                };
                let popup = if let Some(popup) = existing {
                    popup
                } else {
                    let candidate = make_indicator(&fixed);
                    let Ok(mut state) = self.state.try_borrow_mut() else {
                        candidate.close();
                        return;
                    };
                    if let Some(installed) = state.indicator.as_ref() {
                        let installed = installed.clone();
                        drop(state);
                        candidate.close();
                        installed
                    } else {
                        state.indicator = Some(candidate.clone());
                        candidate
                    }
                };
                let (ox, oy) = fixed
                    .window()
                    .map(|w| {
                        let (_, x, y) = w.origin();
                        (x, y)
                    })
                    .unwrap_or((0, 0));
                popup.move_(
                    ox + (origin.0 + zone.x) as i32,
                    oy + (origin.1 + zone.y) as i32,
                );
                popup.resize(zone.width.max(1.0) as i32, zone.height.max(1.0) as i32);
                popup.show_all();
                popup.queue_draw();
            }
        }
    }
}

fn sync(state: &Rc<RefCell<State>>, sync_scheduled: &Rc<Cell<bool>>) {
    // GTK geometry/visibility calls may synchronously pump callbacks. Clone
    // GObject handles and immutable model data first, then retry from the
    // latest revision if re-entry changed ownership while this pass ran.
    'attempt: for _ in 0..4 {
        let Some((revision, fixed, gap, origin, size, hidden, tree, views, visible, ready)) =
            state.try_borrow().ok().map(|state| {
                (
                    state.revision,
                    state.fixed.clone(),
                    state.gap,
                    state.origin,
                    state.size,
                    state.hidden,
                    state.tree.clone(),
                    state
                        .views
                        .iter()
                        .map(|(id, view)| (*id, view.clone()))
                        .collect::<Vec<_>>(),
                    state.visible.clone(),
                    state.ready.clone(),
                )
            })
        else {
            schedule_sync(state, sync_scheduled);
            return;
        };
        let local = Rect::new(0.0, 0.0, size.0, size.1);
        let panes = tree
            .as_ref()
            .map_or_else(Vec::new, |tree| split::layout(tree, local, gap));
        let placements = views
            .into_iter()
            .map(|(id, view)| {
                let pane = panes.iter().find(|(pid, _)| *pid == id).and_then(|(_, r)| {
                    rounded_native_size(r.width, r.height, 1.0).map(|size| (*r, size))
                });
                let mapped = !hidden && pane.is_some() && visible.contains(&id);
                (id, view, pane, mapped)
            })
            .collect::<Vec<_>>();

        // Fail closed before any mapping or geometry work. In particular,
        // every pane removed by a tab/split switch becomes transparent before
        // a replacement pane can be made opaque.
        for (id, view, _, mapped) in &placements {
            let snapshot_ready =
                ready.contains(id) && view.presentation_permit.load(Ordering::Acquire);
            if (!mapped || !snapshot_ready) && view.view.opacity() != 0.0 {
                view.view.set_opacity(0.0);
                if !revision_is_current(state, revision) {
                    continue 'attempt;
                }
            }
            if !mapped && view.view.is_visible() {
                view.view.hide();
                if !revision_is_current(state, revision) {
                    continue 'attempt;
                }
            }
        }

        // Position and map only transparent widgets. GTK calls can run nested
        // main-loop work, so stop using this snapshot after every call that
        // can supersede its revision.
        for (_, view, pane, mapped) in &placements {
            if !mapped {
                continue;
            }
            if let Some((r, (width, height))) = pane {
                fixed.move_(&view.view, (origin.0 + r.x) as i32, (origin.1 + r.y) as i32);
                if !revision_is_current(state, revision) {
                    continue 'attempt;
                }
                view.view.set_size_request(*width, *height);
                if !revision_is_current(state, revision) {
                    continue 'attempt;
                }
            }
            if !view.view.is_visible() {
                // wry's own visibility path is show_all; a remapped webkitgtk
                // view keeps a stale compositing surface until forced to relayout
                // permanently. It must remain transparent throughout mapping.
                if view.view.opacity() != 0.0 {
                    view.view.set_opacity(0.0);
                    if !revision_is_current(state, revision) {
                        continue 'attempt;
                    }
                }
                view.view.show_all();
                if !revision_is_current(state, revision) {
                    continue 'attempt;
                }
                view.view.queue_resize();
                if !revision_is_current(state, revision) {
                    continue 'attempt;
                }
                view.view.queue_draw();
                if !revision_is_current(state, revision) {
                    continue 'attempt;
                }
            }
        }

        for (id, view, _, mapped) in &placements {
            if !mapped || !ready.contains(id) {
                continue;
            }
            // A native call above may have synchronously installed a newer
            // tab/split layout. Prove both that exact layout and the exact
            // document-generation permit immediately around the reveal.
            if !view_may_reveal(state, revision, *id, view) {
                if !revision_is_current(state, revision) {
                    continue 'attempt;
                }
                continue;
            }
            if view.view.opacity() != 1.0 {
                view.view.set_opacity(1.0);
            }
            if !view_may_reveal(state, revision, *id, view) {
                // `set_opacity` may pump a newer layout or a WebKit commit.
                // Conceal synchronously before retrying the authoritative
                // revision, so stale content never reaches the compositor.
                view.view.set_opacity(0.0);
                if !revision_is_current(state, revision) {
                    continue 'attempt;
                }
            }
        }
        if state
            .try_borrow()
            .is_ok_and(|state| state.revision == revision)
        {
            return;
        }
    }
    schedule_sync(state, sync_scheduled);
    conceal_views_until_retry(state);
}

fn revision_is_current(state: &Rc<RefCell<State>>, revision: u64) -> bool {
    state
        .try_borrow()
        .is_ok_and(|state| state.revision == revision)
}

fn view_may_reveal(state: &Rc<RefCell<State>>, revision: u64, id: ItemId, view: &HostView) -> bool {
    view.presentation_permit.load(Ordering::Acquire)
        && state.try_borrow().is_ok_and(|state| {
            state.revision == revision
                && !state.hidden
                && state.visible.contains(&id)
                && state.ready.contains(&id)
                && state.tree.as_ref().is_some_and(|tree| tree.contains(id))
                && state.views.get(&id).is_some_and(|current| {
                    Arc::ptr_eq(&current.presentation_permit, &view.presentation_permit)
                })
        })
        && view.presentation_permit.load(Ordering::Acquire)
}

fn schedule_sync(state: &Rc<RefCell<State>>, sync_scheduled: &Rc<Cell<bool>>) {
    // At most one idle turn may own the retry obligation. GTK callbacks can
    // synchronously re-enter layout more often than the bounded immediate
    // loop permits; retaining this bit guarantees the latest revision gets a
    // fresh main-loop pass without recursion or an unbounded callback queue.
    if sync_scheduled.replace(true) {
        return;
    }
    let weak_state = Rc::downgrade(state);
    let weak_scheduled = Rc::downgrade(sync_scheduled);
    gtk::glib::idle_add_local_once(move || {
        let (Some(state), Some(sync_scheduled)) = (weak_state.upgrade(), weak_scheduled.upgrade())
        else {
            return;
        };
        sync_scheduled.set(false);
        sync(&state, &sync_scheduled);
    });
}

fn conceal_views_until_retry(state: &Rc<RefCell<State>>) {
    // A GTK frame can be painted before the idle retry. Once immediate
    // convergence is exhausted, no stale tab may remain opaque beneath newer
    // browser chrome; the privileged chrome surface is the fail-closed backing.
    let Some(views) = state
        .try_borrow()
        .ok()
        .map(|state| state.views.values().cloned().collect::<Vec<_>>())
    else {
        return;
    };
    for view in views {
        if view.view.opacity() != 0.0 {
            view.view.set_opacity(0.0);
        }
    }
}

// A popup toplevel, not a child widget: gtk child windows do not alpha-blend
// over sibling native windows, the compositor blends toplevels.
fn make_indicator(fixed: &gtk::Fixed) -> gtk::Window {
    let popup = gtk::Window::new(gtk::WindowType::Popup);
    popup.set_app_paintable(true);
    popup.set_accept_focus(false);
    if let Some(screen) = WidgetExt::screen(fixed) {
        popup.set_visual(screen.rgba_visual().as_ref());
    }
    if let Some(top) = fixed
        .toplevel()
        .and_then(|t| t.downcast::<gtk::Window>().ok())
    {
        popup.set_transient_for(Some(&top));
    }
    popup.connect_draw(|area, cr| {
        let w = area.allocated_width() as f64;
        let h = area.allocated_height() as f64;
        let _ = cr.save();
        cr.set_operator(cairo::Operator::Source);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        let _ = cr.paint();
        let _ = cr.restore();
        let inset = INDICATOR_BORDER / 2.0;
        rounded_rect(
            cr,
            inset,
            inset,
            w - INDICATOR_BORDER,
            h - INDICATOR_BORDER,
            INDICATOR_RADIUS,
        );
        cr.set_source_rgba(1.0, 1.0, 1.0, INDICATOR_FILL);
        let _ = cr.fill_preserve();
        cr.set_source_rgba(1.0, 1.0, 1.0, INDICATOR_STROKE);
        cr.set_line_width(INDICATOR_BORDER);
        let _ = cr.stroke();
        Propagation::Proceed
    });
    popup
}

fn rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, radius: f64) {
    let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
    let (right, bottom) = (x + w, y + h);
    cr.new_sub_path();
    cr.arc(right - r, y + r, r, -0.5 * std::f64::consts::PI, 0.0);
    cr.arc(right - r, bottom - r, r, 0.0, 0.5 * std::f64::consts::PI);
    cr.arc(
        x + r,
        bottom - r,
        r,
        0.5 * std::f64::consts::PI,
        std::f64::consts::PI,
    );
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        1.5 * std::f64::consts::PI,
    );
    cr.close_path();
}
