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
        widget.set_sensitive(false);
        widget.set_child_visible(false);
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

    /// Guarded WebKitGTK construction remains unmapped until a Stage owns the
    /// first offscreen map. If the retained layout no longer expects a newly
    /// constructed widget, preserve that non-painting, non-input state;
    /// `insert_view`/`sync` performs the measured first-map path later.
    pub fn exclude_unstaged(view: &wry::WebView) {
        let widget = view.webview();
        // Input is revoked before paint or mapping. Opacity alone does not
        // remove a WebKitGTK native input surface, and an ancestor show_all
        // can restore the ordinary visible property after this function.
        widget.set_sensitive(false);
        widget.set_child_visible(false);
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

    /// Makes an offscreen-mapped WebKit widget presentable only after
    /// privileged chrome verified its attributed URL/revision. The pending
    /// path establishes its compositing surface without exposing paint or
    /// input at trusted-chrome geometry.
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
    /// document. Wry unmaps synchronously at commit; the next Stage sync may
    /// remap it only at the parked offscreen allocation, preventing a newer
    /// document from borrowing the prior document's readiness acknowledgement.
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

        // Fail closed before any geometry or paint work. Opacity is not an
        // input or mapping barrier: revoke sensitivity first and child-visible
        // second. Only then may opacity, position or size change. The mapping
        // barrier both prevents compositor black layers at the old bounds and
        // survives Tao/GTK ancestor show_all calls.
        for (id, view, _, mapped) in &placements {
            let snapshot_ready =
                ready.contains(id) && view.presentation_permit.load(Ordering::Acquire);
            if !mapped || !snapshot_ready {
                if view.view.is_sensitive() {
                    view.view.set_sensitive(false);
                    if !revision_is_current(state, revision) {
                        revoke_and_unmap_for_retry(state, revision, &view.view);
                        continue 'attempt;
                    }
                }
                if view.view.is_child_visible() {
                    view.view.set_child_visible(false);
                    if !revision_is_current(state, revision) {
                        continue 'attempt;
                    }
                }
                if view.view.opacity() != 0.0 {
                    view.view.set_opacity(0.0);
                    if !revision_is_current(state, revision) {
                        continue 'attempt;
                    }
                }
            }
            if !mapped && view.view.is_visible() {
                view.view.hide();
                if !revision_is_current(state, revision) {
                    continue 'attempt;
                }
            }
        }

        // Keep provisional current-layout widgets mapped so WebKitGTK owns a
        // live compositing surface, but park their correctly-sized allocation
        // fully to the left of the composition root. A transparent native child can
        // otherwise paint a black hardware layer and intercept clicks meant
        // for the privileged New Tab surface beneath it.
        for (id, view, pane, mapped) in &placements {
            let snapshot_ready =
                ready.contains(id) && view.presentation_permit.load(Ordering::Acquire);
            if !mapped || snapshot_ready {
                continue;
            }
            if let Some((_, (width, height))) = pane {
                let Some(parked_x) = parked_x(*width) else {
                    // No representable offscreen allocation exists. Keep the
                    // child behind the mapping barrier; a later sane layout
                    // can restore its compositing surface.
                    continue;
                };
                // The child-visible barrier above is still active here, so
                // neither call can expose the old or new native allocation.
                fixed.move_(&view.view, parked_x, 0);
                if !revision_is_current(state, revision) {
                    continue 'attempt;
                }
                view.view.set_size_request(*width, *height);
                if !revision_is_current(state, revision) {
                    continue 'attempt;
                }
            }
            if !view.view.is_child_visible() {
                view.view.set_child_visible(true);
                if !revision_is_current(state, revision) {
                    continue 'attempt;
                }
            }
            if !view.view.is_visible() {
                // wry's own visibility path is show_all; a remapped webkitgtk
                // view keeps a stale compositing surface until forced to relayout
                // permanently. It is already parked, transparent and
                // insensitive before this call is allowed to map it.
                view.view.show_all();
                if !revision_is_current(state, revision) {
                    continue 'attempt;
                }
            }
            // child-visible was synchronously cleared in the first pass even
            // when an ancestor show_all left the ordinary visible property
            // true. Always rebuild the just-remapped offscreen surface.
            view.view.queue_resize();
            if !revision_is_current(state, revision) {
                continue 'attempt;
            }
            view.view.queue_draw();
            if !revision_is_current(state, revision) {
                continue 'attempt;
            }
        }

        for (id, view, pane, mapped) in &placements {
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
            let Some((r, (width, height))) = pane else {
                continue;
            };
            view.view.set_size_request(*width, *height);
            if !view_may_reveal(state, revision, *id, view) {
                revoke_and_unmap_for_retry(state, revision, &view.view);
                continue 'attempt;
            }
            fixed.move_(&view.view, (origin.0 + r.x) as i32, (origin.1 + r.y) as i32);
            if !view_may_reveal(state, revision, *id, view) {
                revoke_and_unmap_for_retry(state, revision, &view.view);
                continue 'attempt;
            }
            if view.view.opacity() != 1.0 {
                view.view.set_opacity(1.0);
            }
            if !view_may_reveal(state, revision, *id, view) {
                // Native calls may pump a newer layout or WebKit commit.
                // Revoke input before paint and park synchronously before
                // retrying, so stale content can neither render nor receive a
                // gesture intended for the newer trusted-chrome state.
                revoke_and_unmap_for_retry(state, revision, &view.view);
                continue 'attempt;
            }
            let remapped = !view.view.is_child_visible();
            if remapped {
                view.view.set_child_visible(true);
                if !view_may_reveal(state, revision, *id, view) {
                    revoke_and_unmap_for_retry(state, revision, &view.view);
                    continue 'attempt;
                }
            }
            if !view.view.is_visible() {
                view.view.show_all();
                if !view_may_reveal(state, revision, *id, view) {
                    revoke_and_unmap_for_retry(state, revision, &view.view);
                    continue 'attempt;
                }
            }
            if remapped {
                view.view.queue_resize();
                if !view_may_reveal(state, revision, *id, view) {
                    revoke_and_unmap_for_retry(state, revision, &view.view);
                    continue 'attempt;
                }
                view.view.queue_draw();
                if !view_may_reveal(state, revision, *id, view) {
                    revoke_and_unmap_for_retry(state, revision, &view.view);
                    continue 'attempt;
                }
            }
            // Input is the final surface enabled. All mapping, geometry,
            // paint and redraw calls have settled under exact checks before
            // this point; revalidate immediately around the setter too.
            if !view_may_reveal(state, revision, *id, view) {
                revoke_and_unmap_for_retry(state, revision, &view.view);
                continue 'attempt;
            }
            if !view.view.is_sensitive() {
                view.view.set_sensitive(true);
            }
            if !view_may_reveal(state, revision, *id, view) {
                revoke_and_unmap_for_retry(state, revision, &view.view);
                continue 'attempt;
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

fn parked_x(child_width: i32) -> Option<i32> {
    if child_width <= 0 {
        return None;
    }
    // Park entirely to the left. A positive offscreen coordinate contributes
    // to GtkFixed's preferred width and can grow the composition root; the
    // right edge at -1 is clipped without changing the root requisition.
    child_width.checked_neg()?.checked_sub(1)
}

fn revoke_and_unmap_for_retry(
    state: &Rc<RefCell<State>>,
    revision: u64,
    view: &webkit2gtk::WebView,
) -> bool {
    // Do not return early when native re-entry supersedes `revision`: this is
    // a terminal fail-closed transition for the stale snapshot. Re-read after
    // every GTK call, and finish with a second mapping barrier so a nested
    // newer sync cannot leave the outer stale surface mapped. The caller then
    // retries from the authoritative revision immediately.
    view.set_sensitive(false);
    let after_sensitivity = revision_is_current(state, revision);
    view.set_child_visible(false);
    let after_unmap = revision_is_current(state, revision);
    view.set_opacity(0.0);
    let after_opacity = revision_is_current(state, revision);
    view.set_child_visible(false);
    let after_final_unmap = revision_is_current(state, revision);
    after_sensitivity && after_unmap && after_opacity && after_final_unmap
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
    let Some((revision, views)) = state.try_borrow().ok().map(|state| {
        (
            state.revision,
            state.views.values().cloned().collect::<Vec<_>>(),
        )
    }) else {
        return;
    };
    for view in views {
        // The already-scheduled authoritative retry remaps only expected
        // panes. Until then, prefer a short compositing-surface interruption
        // over one frame of stale paint or page-owned input.
        let _ = revoke_and_unmap_for_retry(state, revision, &view.view);
        if view.view.is_visible() {
            view.view.hide();
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

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::prelude::ContainerExtManual;

    #[test]
    fn parking_coordinate_places_the_complete_child_left_of_the_root() {
        assert_eq!(parked_x(720), Some(-721));
        assert_eq!(parked_x(1), Some(-2));
        assert_eq!(parked_x(i32::MAX), Some(i32::MIN));
        assert_eq!(parked_x(0), None);
        assert_eq!(parked_x(-1), None);
    }

    #[test]
    fn concealment_revokes_input_before_paint_and_mapping() {
        let source = include_str!("stage.rs");
        let exclude = source
            .split("pub fn exclude_unstaged")
            .nth(1)
            .and_then(|source| source.split("pub fn remove_view").next())
            .expect("unstaged concealment body");
        let sensitivity = exclude
            .find("widget.set_sensitive(false)")
            .expect("input revocation");
        let opacity = exclude
            .find("widget.set_opacity(0.0)")
            .expect("paint concealment");
        let child_visibility = exclude
            .find("widget.set_child_visible(false)")
            .expect("mapping concealment");
        let hide = exclude.find("widget.hide()").expect("ordinary hide");
        assert!(sensitivity < child_visibility);
        assert!(child_visibility < opacity);
        assert!(opacity < hide);

        let sync_conceal = source
            .split("// Fail closed before any geometry or paint work")
            .nth(1)
            .and_then(|source| {
                source
                    .split("// Keep provisional current-layout widgets mapped")
                    .next()
            })
            .expect("synchronous stage concealment pass");
        let sensitivity = sync_conceal
            .find("view.view.set_sensitive(false)")
            .expect("stage input revocation");
        let mapping = sync_conceal
            .find("view.view.set_child_visible(false)")
            .expect("stage mapping barrier");
        assert!(sensitivity < mapping);
        let sensitivity_reentry = &sync_conceal[sensitivity..mapping];
        assert!(sensitivity_reentry.contains("revoke_and_unmap_for_retry"));

        let revoke = source
            .split("fn revoke_and_unmap_for_retry")
            .nth(1)
            .and_then(|source| source.split("fn view_may_reveal").next())
            .expect("fail-closed unmapping helper");
        assert!(
            revoke.find("view.set_sensitive(false)").unwrap()
                < revoke.find("view.set_child_visible(false)").unwrap()
        );
        assert!(
            revoke.find("view.set_child_visible(false)").unwrap()
                < revoke.find("view.set_opacity(0.0)").unwrap()
        );
        assert_eq!(revoke.matches("view.set_child_visible(false)").count(), 2);

        let pending = source
            .split("for (id, view, pane, mapped) in &placements")
            .nth(1)
            .and_then(|source| {
                source
                    .split("for (id, view, pane, mapped) in &placements")
                    .next()
            })
            .expect("pending mapped-surface pass");
        assert!(
            pending
                .find("fixed.move_(&view.view, parked_x, 0)")
                .unwrap()
                < pending.find("view.view.set_size_request").unwrap()
        );
        assert!(
            pending.find("view.view.set_size_request").unwrap()
                < pending.find("view.view.set_child_visible(true)").unwrap()
        );

        let reveal = source
            .split("for (id, view, pane, mapped) in &placements")
            .nth(2)
            .and_then(|source| source.split("if state").next())
            .expect("exact presentation reveal pass");
        let input = reveal
            .rfind("view.view.set_sensitive(true)")
            .expect("final input enablement");
        assert!(reveal.rfind("view.view.queue_draw()").unwrap() < input);
        assert!(reveal[..input].rfind("view_may_reveal").is_some());
        assert!(reveal[input..].find("view_may_reveal").is_some());
    }

    #[test]
    #[ignore = "requires a native GTK display; Linux CI runs native GTK tests under Xvfb"]
    fn stage_parks_pending_views_and_hidden_children_survive_ancestor_show_all() {
        gtk::init().expect("GTK display");
        let application = gtk::Application::new(
            Some("dev.zephium.native-stage-test"),
            gtk::gio::ApplicationFlags::NON_UNIQUE,
        );
        application
            .register(None::<&gtk::gio::Cancellable>)
            .expect("register GTK test application");

        let window = gtk::ApplicationWindow::new(&application);
        window.set_default_size(960, 720);
        let fixed = gtk::Fixed::new();
        let chrome = gtk::DrawingArea::new();
        chrome.set_size_request(960, 720);
        fixed.put(&chrome, 0, 0);
        let context = webkit2gtk::WebContext::new_ephemeral();
        let content = webkit2gtk::WebView::with_context(&context);
        fixed.put(&content, 240, 0);
        let sibling = webkit2gtk::WebView::with_context(&context);
        fixed.put(&sibling, 240, 360);
        window.add(&fixed);
        window.show_all();
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }

        let id = ItemId::from(7);
        let sibling_id = ItemId::from(8);
        let permit = Arc::new(AtomicBool::new(false));
        let sibling_permit = Arc::new(AtomicBool::new(false));
        let stage = Stage::new(fixed.clone(), 8.0);
        {
            let mut state = stage.state.borrow_mut();
            state.views.insert(
                id,
                HostView {
                    view: content.clone(),
                    presentation_permit: permit.clone(),
                },
            );
            state.views.insert(
                sibling_id,
                HostView {
                    view: sibling.clone(),
                    presentation_permit: sibling_permit.clone(),
                },
            );
        }

        let restored_split = Pane::Branch {
            axis: zephium_core::split::Axis::Col,
            ratio: 0.5,
            a: Box::new(Pane::Leaf(id)),
            b: Box::new(Pane::Leaf(sibling_id)),
        };
        assert!(stage.apply(
            Some(Rect::new(240.0, 0.0, 720.0, 720.0)),
            Some(restored_split),
            &[id, sibling_id],
        ));
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        let parked: i32 = fixed.child_property(&content, "x");
        let sibling_parked: i32 = fixed.child_property(&sibling, "x");
        assert_eq!(parked, -721);
        assert_eq!(sibling_parked, parked);
        assert!(content.is_visible());
        assert!(content.is_child_visible());
        assert!(!content.is_sensitive());
        assert_eq!(content.opacity(), 0.0);
        assert!(sibling.is_visible());
        assert!(sibling.is_child_visible());
        assert!(!sibling.is_sensitive());
        assert_eq!(sibling.opacity(), 0.0);

        permit.store(true, Ordering::Release);
        sibling_permit.store(true, Ordering::Release);
        assert!(stage.set_ready(id));
        assert!(stage.set_ready(sibling_id));
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        let presented_x: i32 = fixed.child_property(&content, "x");
        assert_eq!(presented_x, 240);
        assert!(content.is_sensitive());
        assert_eq!(content.opacity(), 1.0);
        let sibling_presented_x: i32 = fixed.child_property(&sibling, "x");
        let sibling_presented_y: i32 = fixed.child_property(&sibling, "y");
        assert_eq!(sibling_presented_x, 240);
        assert!(sibling_presented_y > 0);
        assert!(sibling.is_sensitive());
        assert_eq!(sibling.opacity(), 1.0);

        assert!(stage.apply(None, None, &[]));
        window.show_all();
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        assert!(!content.is_child_visible());
        assert!(!content.is_mapped());
        assert!(!content.is_sensitive());
        assert_eq!(content.opacity(), 0.0);
        assert!(!sibling.is_child_visible());
        assert!(!sibling.is_mapped());
        assert!(!sibling.is_sensitive());
        assert_eq!(sibling.opacity(), 0.0);
    }
}
